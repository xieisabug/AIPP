//! Assistant 1 owns one model slot. Only its sync identity is fixed; row IDs
//! (including the provider ID) always belong to the receiving database.
use super::*;

pub(super) const MODEL_ID: &str = "official:assistant.model:quick";
const KIND: &str = "assistant.model";

/// Run outside the scanner's metadata transaction: both databases must commit
/// through the normal recovery journal, including alias and outbox migration.
pub(super) fn prepare(sync: &Connection, data: &Connection) -> Result<(), String> {
    persistence::coordinated(sync, data, || normalize(sync, data).map(|_| ()))
}

/// Preserve the currently exposed selection on first migration. Once mapped,
/// the fixed slot wins over any old duplicate rows. Never pick by a remote ID.
fn normalize(sync: &Connection, data: &Connection) -> Result<Option<i64>, String> {
    let ids = data.prepare("SELECT id FROM assistant_model WHERE assistant_id=1
        ORDER BY CASE WHEN provider_id=0 AND model_code='' AND COALESCE(alias,'')='' THEN 1 ELSE 0 END, id")
        .map_err(|e| e.to_string())?.query_map([], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let Some(first) = ids.first().copied() else { return Ok(None); };
    let mapped = find_local_id_by_object_id(sync, KIND, MODEL_ID)?;
    if mapped.is_some_and(|id| !ids.contains(&id)) {
        return Err("快速助手固定模型同步映射指向不存在或不属于快速助手的本地绑定".into());
    }
    let keep = mapped.unwrap_or(first);
    for id in &ids {
        // Preserve every historical identity before removing duplicate rows.
        sync.execute("INSERT OR REPLACE INTO sync_object_alias (object_type,sync_id,local_id)
            SELECT object_type,sync_id,?2 FROM sync_object_map
            WHERE object_type=?1 AND local_id=?3 AND sync_id!=?4",
            params![KIND, keep, id, MODEL_ID]).map_err(|e| e.to_string())?;
        sync.execute("UPDATE sync_object_alias SET local_id=?2 WHERE object_type=?1 AND local_id=?3",
            params![KIND, keep, id]).map_err(|e| e.to_string())?;
        if *id != keep {
            data.execute("UPDATE assistant_model_config SET assistant_model_id=?1
                WHERE assistant_id=1 AND assistant_model_id=?2", params![keep, id]).map_err(|e| e.to_string())?;
            data.execute("DELETE FROM assistant_model WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
            sync.execute("DELETE FROM sync_object_map WHERE object_type=?1 AND local_id=?2",
                params![KIND, id]).map_err(|e| e.to_string())?;
        }
    }
    remap_sync_object_map(sync, KIND, "assistant_model", keep, MODEL_ID)?;
    // Never rewrite/drop attempted events: their outcomes may still be unknown.
    // The next scan emits the current value under the fixed identity.
    sync.execute("DELETE FROM sync_outbox WHERE object_type=?1 AND sent_once=0
        AND status IN ('pending','failed') AND object_id IN
        (SELECT sync_id FROM sync_object_alias WHERE object_type=?1 AND local_id=?2)",
        params![KIND, keep]).map_err(|e| e.to_string())?;
    Ok(Some(keep))
}

/// Called only AFTER resolve_remote_foreign_keys. Numeric hints in the payload
/// cannot select either the assistant or provider on this device.
pub(super) fn apply_model(
    sync: &Connection, data: &Connection, spec: &SyncTableSpec,
    change: &PullChange, fields: &Map<String, JsonValue>,
) -> Result<bool, String> {
    if spec.object_type != KIND { return Ok(false); }
    if fields.get("assistant_id").and_then(JsonValue::as_i64) != Some(1) {
        if change.object_id == MODEL_ID {
            return Err("快速助手固定模型同步身份的助手引用不匹配".into());
        }
        return Ok(false);
    }
    let existing = normalize(sync, data)?;
    let canonical_received: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sync_shadow
        WHERE object_type=?1 AND object_id=?2 AND deleted_at IS NULL)",
        params![KIND, MODEL_ID], |r| r.get(0)).map_err(|e| e.to_string())?;
    let stale = if change.object_id == MODEL_ID {
        // Legacy identities have independent versions. Their later tombstones
        // or delayed ACKs must not suppress an update to the fixed identity.
        false
    } else {
        canonical_received || existing.map(|id| remote_row_change_is_stale(sync, change, id)).transpose()?.unwrap_or(false)
    };
    let local_id = match existing {
        Some(id) => {
            if !stale { update_row(data, spec, id, fields)?; }
            id
        }
        None => insert_row(data, spec, fields)?,
    };
    remap_sync_object_map(sync, KIND, spec.table, local_id, MODEL_ID)?;
    if change.object_id != MODEL_ID {
        sync.execute("INSERT OR REPLACE INTO sync_object_alias (object_type,sync_id,local_id) VALUES (?1,?2,?3)",
            params![KIND, change.object_id, local_id]).map_err(|e| e.to_string())?;
    }
    upsert_shadow_from_remote_with_fields(sync, spec, change, fields)?;
    Ok(true)
}

pub(super) fn apply_delete(sync: &Connection, data: &Connection, change: &PullChange) -> Result<bool, String> {
    if change.object_type != KIND { return Ok(false); }
    if change.object_id == MODEL_ID {
        return Err("快速助手固定模型槽位不能删除；请同步未选择模型的空值".into());
    }
    if let Some(id) = find_local_id_by_object_id(sync, KIND, &change.object_id)? {
        let quick: bool = data.query_row("SELECT EXISTS(SELECT 1 FROM assistant_model WHERE id=?1 AND assistant_id=1)",
            params![id], |r| r.get(0)).map_err(|e| e.to_string())?;
        if quick {
            normalize(sync, data)?;
            // Retain the alias for parameters still referencing this legacy ID.
            upsert_shadow_from_remote(sync, change)?;
            return Ok(true);
        }
    }
    Ok(false)
}

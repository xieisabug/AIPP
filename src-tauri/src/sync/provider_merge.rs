//! Same-name provider linking (on apply) and repair of duplicates created by
//! earlier syncs.
//!
//! Two devices that each created "SiliconFlow" get unrelated sync identities.
//! Before this module the remote one arrived as a second, keyless provider and
//! the quick assistant (which follows the remote identity) pointed at it.
use super::*;

const PROVIDER: &str = "llm.provider";

/// Remote authors of identities, so a device's own same-named providers are
/// never folded into one another on a receiver.
pub(super) fn record_origin(sync: &Connection, change: &PullChange) -> Result<(), String> {
    if change.object_type != PROVIDER || change.device_id.is_empty() || change.device_id == "server-conflict" {
        return Ok(());
    }
    sync.execute(
        "INSERT OR IGNORE INTO sync_identity_origin (object_type, sync_id, device_id) VALUES (?1, ?2, ?3)",
        params![change.object_type, change.object_id, change.device_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    Ok(!existing_columns(conn, table, &[column])?.is_empty())
}

fn identities(sync: &Connection, object_type: &str, local_id: i64) -> Result<Vec<String>, String> {
    sync.prepare("SELECT sync_id FROM sync_object_map WHERE object_type=?1 AND local_id=?2
        UNION SELECT sync_id FROM sync_object_alias WHERE object_type=?1 AND local_id=?2")
        .map_err(|e| e.to_string())?
        .query_map(params![object_type, local_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

/// For an unmapped, non-official remote provider: the single local non-official
/// provider with the same name (+ api_type) that holds no identity authored by
/// the same remote device. Ambiguity → None (insert as before).
pub(super) fn find_link_target(
    sync: &Connection,
    data: &Connection,
    change: &PullChange,
    fields: &Map<String, JsonValue>,
) -> Result<Option<i64>, String> {
    if change.object_type != PROVIDER || change.object_id.starts_with("official:") {
        return Ok(None);
    }
    if fields.get("is_official").and_then(JsonValue::as_i64).unwrap_or(0) != 0
        || fields.get("is_official").and_then(JsonValue::as_bool).unwrap_or(false)
    {
        return Ok(None);
    }
    let Some(name) = fields.get("name").and_then(JsonValue::as_str) else { return Ok(None) };
    let api_type = fields.get("api_type").and_then(JsonValue::as_str);
    let with_type = api_type.is_some() && has_column(data, "llm_provider", "api_type")?;
    let official = if has_column(data, "llm_provider", "is_official")? { "AND COALESCE(is_official,0)=0" } else { "" };
    let sql = if with_type {
        format!("SELECT id FROM llm_provider WHERE name=?1 AND api_type=?2 {official} ORDER BY id")
    } else {
        format!("SELECT id FROM llm_provider WHERE name=?1 {official} ORDER BY id")
    };
    let mut stmt = data.prepare(&sql).map_err(|e| e.to_string())?;
    let values: Vec<Value> = if with_type {
        vec![Value::Text(name.to_string()), Value::Text(api_type.unwrap_or_default().to_string())]
    } else {
        vec![Value::Text(name.to_string())]
    };
    let ids = stmt.query_map(params_from_iter(values.iter()), |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let mut candidates = Vec::new();
    for id in ids {
        let mut same_author = false;
        for sync_id in identities(sync, PROVIDER, id)? {
            if sync_id.starts_with("official:") { same_author = true; break; }
            let origin: Option<String> = sync.query_row(
                "SELECT device_id FROM sync_identity_origin WHERE object_type=?1 AND sync_id=?2",
                params![PROVIDER, sync_id], |r| r.get(0),
            ).optional().map_err(|e| e.to_string())?;
            if origin.as_deref() == Some(change.device_id.as_str()) { same_author = true; break; }
        }
        if !same_author { candidates.push(id); }
    }
    Ok(if candidates.len() == 1 { candidates.pop() } else { None })
}

/// Link the remote identity to an existing local provider. The local identity
/// stays canonical (no child payload churn); the remote one becomes an alias.
pub(super) fn link(
    sync: &Connection,
    data: &Connection,
    spec: &SyncTableSpec,
    change: &PullChange,
    local_id: i64,
    fields: &Map<String, JsonValue>,
    stale: bool,
) -> Result<(), String> {
    if !stale {
        update_row(data, spec, local_id, fields)?;
    }
    sync.execute(
        "INSERT OR REPLACE INTO sync_object_alias (object_type, sync_id, local_id) VALUES (?1, ?2, ?3)",
        params![PROVIDER, change.object_id, local_id],
    ).map_err(|e| e.to_string())?;
    record_origin(sync, change)?;
    info!(local_provider_id = local_id, "Linked remote provider identity to same-named local provider");
    Ok(())
}

// ---------------- duplicate repair ----------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct DuplicateProviderPair {
    pub name: String,
    pub keep_id: i64,
    pub duplicate_id: i64,
}

fn api_key_of(llm: &Connection, provider_id: i64) -> Result<Option<String>, String> {
    let value: Option<Option<String>> = llm.query_row(
        "SELECT value FROM llm_provider_config WHERE llm_provider_id=?1 AND LOWER(name)='api_key'
         AND TRIM(COALESCE(value,''))!='' ORDER BY id DESC LIMIT 1",
        params![provider_id], |r| r.get(0),
    ).optional().map_err(|e| e.to_string())?;
    Ok(value.flatten().map(|v| v.trim().to_string()))
}

/// Duplicate non-official providers (same name + api_type) that are safe to fold:
/// a keeper with a non-empty API key, and duplicates whose key is empty or the
/// same. Groups with conflicting keys are left alone.
pub(super) fn find_duplicates(llm: &Connection) -> Result<Vec<DuplicateProviderPair>, String> {
    let with_type = has_column(llm, "llm_provider", "api_type")?;
    let group_cols = if with_type { "name, api_type" } else { "name" };
    let mut groups = llm.prepare(&format!(
        "SELECT {group_cols}, GROUP_CONCAT(id) FROM llm_provider WHERE COALESCE(is_official,0)=0
         GROUP BY {group_cols} HAVING COUNT(*) > 1"))
        .map_err(|e| e.to_string())?;
    let rows = groups.query_map([], |r| {
        let name: String = r.get(0)?;
        let ids: String = r.get(if with_type { 2 } else { 1 })?;
        Ok((name, ids))
    }).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let mut pairs = Vec::new();
    for (name, ids) in rows {
        let mut ids: Vec<i64> = ids.split(',').filter_map(|v| v.parse().ok()).collect();
        ids.sort();
        let mut keys = Vec::new();
        for id in &ids { keys.push((*id, api_key_of(llm, *id)?)); }
        let distinct: HashSet<&String> = keys.iter().filter_map(|(_, k)| k.as_ref()).collect();
        if distinct.len() != 1 { continue; }
        let keep_id = keys.iter().find(|(_, k)| k.is_some()).map(|(id, _)| *id).unwrap();
        for (id, _) in keys.iter().filter(|(id, _)| *id != keep_id) {
            pairs.push(DuplicateProviderPair { name: name.clone(), keep_id, duplicate_id: *id });
        }
    }
    Ok(pairs)
}

/// Names of same-name provider groups whose API keys differ; never auto-merged.
pub(super) fn find_key_conflicts(llm: &Connection) -> Result<Vec<String>, String> {
    let with_type = has_column(llm, "llm_provider", "api_type")?;
    let group_cols = if with_type { "name, api_type" } else { "name" };
    let mut groups = llm.prepare(&format!(
        "SELECT name, GROUP_CONCAT(id) FROM llm_provider WHERE COALESCE(is_official,0)=0
         GROUP BY {group_cols} HAVING COUNT(*) > 1 ORDER BY name"))
        .map_err(|e| e.to_string())?;
    let rows = groups.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let mut names = Vec::new();
    for (name, ids) in rows {
        let mut keys = HashSet::new();
        for id in ids.split(',').filter_map(|v| v.parse::<i64>().ok()) {
            if let Some(key) = api_key_of(llm, id)? { keys.insert(key); }
        }
        if keys.len() > 1 && !names.contains(&name) { names.push(name); }
    }
    Ok(names)
}

/// Fold every safe duplicate group (see find_duplicates). Returns merged count.
/// Callers must finish redirects afterwards (finish_redirects / next scan).
pub(super) fn auto_merge(sync: &Connection, llm: &Connection) -> Result<usize, String> {
    let pairs = find_duplicates(llm)?;
    for pair in &pairs {
        merge_in_llm(sync, llm, pair.keep_id, pair.duplicate_id)?;
    }
    Ok(pairs.len())
}

fn alias_all(sync: &Connection, object_type: &str, from: i64, to: i64) -> Result<(), String> {
    for sync_id in identities(sync, object_type, from)? {
        sync.execute("INSERT OR REPLACE INTO sync_object_alias (object_type, sync_id, local_id) VALUES (?1, ?2, ?3)",
            params![object_type, sync_id, to]).map_err(|e| e.to_string())?;
    }
    sync.execute("DELETE FROM sync_object_map WHERE object_type=?1 AND local_id=?2", params![object_type, from])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Phase 1 (sync.db + llm.db, coordinated): fold `dup` into `keep`, alias every
/// identity, and persist redirects for references living in other databases.
/// No identity is dropped, so no delete tombstone is ever published.
pub(super) fn merge_in_llm(sync: &Connection, llm: &Connection, keep: i64, dup: i64) -> Result<(), String> {
    persistence::coordinated(sync, llm, || {
        // Children with a per-provider unique key: (table, key column, object types).
        let children: [(&str, &str, &[&str]); 3] = [
            ("llm_model", "code", &["llm.model"]),
            ("llm_provider_config", "name", &["llm.provider_config", secrets::OBJECT_TYPE]),
            ("llm_model_request_mode_preference", "model_code", &["llm.model_request_mode_preference"]),
        ];
        for (table, key, kinds) in children {
            if existing_columns(llm, table, &["id"])?.is_empty() { continue; }
            let rows = llm.prepare(&format!("SELECT id FROM {table} WHERE llm_provider_id=?1"))
                .map_err(|e| e.to_string())?
                .query_map(params![dup], |r| r.get::<_, i64>(0))
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
            for child in rows {
                let sibling: Option<i64> = llm.query_row(
                    &format!("SELECT k.id FROM {table} k JOIN {table} d ON d.{key} = k.{key}
                              WHERE d.id=?1 AND k.llm_provider_id=?2 ORDER BY k.id DESC LIMIT 1"),
                    params![child, keep], |r| r.get(0),
                ).optional().map_err(|e| e.to_string())?;
                match sibling {
                    Some(sibling) => {
                        for kind in kinds { alias_all(sync, kind, child, sibling)?; }
                        if table == "llm_model" {
                            sync.execute("INSERT OR REPLACE INTO sync_merge_redirect (kind, from_id, to_id) VALUES ('llm_model', ?1, ?2)",
                                params![child, sibling]).map_err(|e| e.to_string())?;
                        }
                        llm.execute(&format!("DELETE FROM {table} WHERE id=?1"), params![child]).map_err(|e| e.to_string())?;
                    }
                    None => {
                        llm.execute(&format!("UPDATE {table} SET llm_provider_id=?1 WHERE id=?2"), params![keep, child])
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        alias_all(sync, PROVIDER, dup, keep)?;
        sync.execute("INSERT OR REPLACE INTO sync_merge_redirect (kind, from_id, to_id) VALUES ('llm_provider', ?1, ?2)",
            params![dup, keep]).map_err(|e| e.to_string())?;
        llm.execute("DELETE FROM llm_provider WHERE id=?1", params![dup]).map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// Phase 2 (idempotent): rewrite references in assistant/conversation/system DBs
/// using persisted redirects, then clear the redirects.
pub(super) fn finish_redirects(
    sync: &Connection,
    assistant: Option<&Connection>,
    conversation: Option<&Connection>,
    system: Option<&Connection>,
) -> Result<usize, String> {
    let redirects: Vec<(String, i64, i64)> = sync
        .prepare("SELECT kind, from_id, to_id FROM sync_merge_redirect").map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    if redirects.is_empty() { return Ok(0); }
    for (kind, from, to) in &redirects {
        match kind.as_str() {
            "llm_model" => {
                if let Some(conv) = conversation {
                    if !existing_columns(conv, "message", &["llm_model_id"])?.is_empty() {
                        conv.execute("UPDATE message SET llm_model_id=?1 WHERE llm_model_id=?2", params![to, from])
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            "llm_provider" => {
                if let Some(db) = assistant { redirect_assistant_models(sync, db, *from, *to)?; }
                if let Some(db) = system { redirect_feature_configs(db, *from, *to)?; }
            }
            _ => {}
        }
    }
    sync.execute("DELETE FROM sync_merge_redirect", []).map_err(|e| e.to_string())?;
    Ok(redirects.len())
}

fn redirect_assistant_models(sync: &Connection, db: &Connection, from: i64, to: i64) -> Result<(), String> {
    if existing_columns(db, "assistant_model", &["provider_id"])?.is_empty() { return Ok(()); }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let rows = tx.prepare("SELECT id, assistant_id, model_code FROM assistant_model WHERE provider_id=?1")
        .map_err(|e| e.to_string())?
        .query_map(params![from], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (id, assistant_id, code) in rows {
        let existing: Option<i64> = tx.query_row(
            "SELECT id FROM assistant_model WHERE assistant_id=?1 AND provider_id=?2 AND model_code=?3",
            params![assistant_id, to, code], |r| r.get(0),
        ).optional().map_err(|e| e.to_string())?;
        match existing {
            Some(other) if other != id => {
                alias_all(sync, "assistant.model", id, other)?;
                if !existing_columns(&tx, "assistant_model_config", &["assistant_model_id"])?.is_empty() {
                    tx.execute("UPDATE assistant_model_config SET assistant_model_id=?1 WHERE assistant_model_id=?2",
                        params![other, id]).map_err(|e| e.to_string())?;
                }
                tx.execute("DELETE FROM assistant_model WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
            }
            _ => {
                tx.execute("UPDATE assistant_model SET provider_id=?1 WHERE id=?2", params![to, id]).map_err(|e| e.to_string())?;
            }
        }
    }
    if !existing_columns(&tx, "assistant_model_config", &["value"])?.is_empty() {
        tx.execute("UPDATE assistant_model_config SET value=?1 WHERE name='acp_provider' AND TRIM(value)=?2",
            params![to.to_string(), from.to_string()]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn redirect_feature_configs(db: &Connection, from: i64, to: i64) -> Result<(), String> {
    if existing_columns(db, "feature_config", &["value"])?.is_empty() { return Ok(()); }
    let rows = db.prepare("SELECT id, feature_code, key, value FROM feature_config")
        .map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (id, feature, key, value) in rows {
        let Some(value) = value else { continue };
        let fields = Map::from_iter([
            ("feature_code".to_string(), json!(feature)), ("key".to_string(), json!(key)), ("value".to_string(), json!(value)),
        ]);
        if let Some((prefix, provider)) = embedded_provider_value("system.feature_config", &fields) {
            if provider == from {
                db.execute("UPDATE feature_config SET value=?1 WHERE id=?2", params![format!("{prefix}{to}"), id])
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

/// Fold rows that sync used to duplicate (same provider + config name, e.g. two
/// `endpoint` rows) into the newest one, which is the value readers already use.
/// Identities of removed rows become aliases, so no tombstone is published.
pub(super) fn dedupe_provider_configs(sync: &Connection, llm: &Connection) -> Result<usize, String> {
    if existing_columns(llm, "llm_provider_config", &["id", "llm_provider_id", "name"])?.len() < 3 { return Ok(0); }
    let synced = format!("(LOWER(name) IN ('endpoint', 'base_url', 'acp_cli_command', 'acp_working_directory', 'acp_additional_args', 'acp_claude_auth_mode') OR {})",
        secrets::SECRET_NAME_SQL);
    let dups: Vec<(i64, i64)> = llm.prepare(&format!(
        "SELECT d.id, (SELECT MAX(k.id) FROM llm_provider_config k WHERE k.llm_provider_id=d.llm_provider_id AND k.name=d.name)
         FROM llm_provider_config d WHERE {synced}
           AND d.id < (SELECT MAX(k.id) FROM llm_provider_config k WHERE k.llm_provider_id=d.llm_provider_id AND k.name=d.name)"))
        .map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    if dups.is_empty() { return Ok(0); }
    persistence::coordinated(sync, llm, || {
        for (dup, keep) in &dups {
            for kind in ["llm.provider_config", secrets::OBJECT_TYPE] { alias_all(sync, kind, *dup, *keep)?; }
            llm.execute("DELETE FROM llm_provider_config WHERE id=?1", params![dup]).map_err(|e| e.to_string())?;
        }
        Ok(())
    })?;
    info!(removed = dups.len(), "Folded duplicated provider config rows created by earlier syncs");
    Ok(dups.len())
}

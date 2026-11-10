//! Wire envelopes use stable sync identities; local envelopes bind local server IDs.
use super::*;

pub(super) fn encrypted_type(kind: &str) -> bool {
    matches!(kind, secrets::OBJECT_TYPE | vault::OBJECT_TYPE)
}

pub(super) fn discard_obsolete_outbox(sync: &Connection, key: Option<&secrets::SecretKey>) -> Result<(),String> {
    let Some(key) = key else { return Ok(()) };
    let rows = sync.prepare("SELECT id,payload_json,sent_once,outcome_known FROM sync_outbox
        WHERE object_type IN ('llm.provider_secret','mcp.vault_secret') AND operation='upsert'")
        .map_err(|e| e.to_string())?.query_map([], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,bool>(2)?,r.get::<_,bool>(3)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (id, raw, sent, known) in rows {
        let value: JsonValue = serde_json::from_str(raw.as_deref().unwrap_or("null")).map_err(|_| "待同步凭据密文格式无效")?;
        let kid = value["fields"]["value"]["kid"].as_str().ok_or("待同步凭据缺少密钥版本")?;
        if kid == key.kid() { continue; }
        if sent && !known { return Err("旧密钥凭据仍有结果未确认的上传，已停止推送，等待同步恢复".into()); }
        sync.execute("DELETE FROM sync_outbox WHERE id=?1", [id]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn seal_snapshots(sync: &Connection, key: &secrets::SecretKey, snapshots: Vec<LocalObjectSnapshot>) -> Result<Vec<LocalObjectSnapshot>, String> {
    snapshots.into_iter().map(|snapshot| {
        let mut payload: JsonValue = serde_json::from_str(&snapshot.payload_json).map_err(|e| e.to_string())?;
        let fields = payload["fields"].as_object_mut().ok_or("凭据同步字段缺失")?;
        let reference = field_as_string(fields, "reference").ok_or("凭据同步引用缺失")?;
        // The local ID is omitted from fields by the generic FK serializer. Extract
        // it from the authenticated local credential, then verify its AAD binding.
        let stored = field_as_string(fields, "value").ok_or("凭据同步密文缺失")?;
        let id = fields.remove("vault_local_id").and_then(|v| v.as_i64()).ok_or("凭据同步本地绑定缺失")?;
        fields.remove("server_id");
        let raw = vault::decrypt(key, &reference, id, &stored)?;
        let mut credential: crate::mcp::apps::credentials::Credential = serde_json::from_str(&raw).map_err(|_| "凭据格式无效")?;
        if credential.server_id != id { return Err("凭据本地归属不一致".into()); }
        credential.server_id = 0;
        let raw = serde_json::to_string(&credential).map_err(|e| e.to_string())?;
        fields.insert("value".into(), secrets::seal(key, &snapshot.object_id, &format!("vault:{reference}"), &raw)?);
        let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        Ok(LocalObjectSnapshot { payload_hash: sync_payload_hash(sync, Some(&payload))?, payload_json, ..snapshot })
    }).collect()
}

pub(super) fn apply(sync: &Connection, db: &Connection, spec: &SyncTableSpec, change: &PullChange, fields: &Map<String,JsonValue>) -> Result<(),String> {
    secrets::remember_server_copy(sync, change)?;
    let Some(key) = secrets::current_key() else { return secrets::park(sync, change, "需要同步口令才能解密连接凭据"); };
    let reference = field_as_string(fields, "reference").ok_or("连接凭据引用缺失")?;
    Uuid::parse_str(&reference).map_err(|_| "连接凭据引用无效")?;
    let envelope = fields.get("value").ok_or("连接凭据密文缺失")?;
    let raw = match secrets::open(&key, &change.object_id, &format!("vault:{reference}"), envelope) {
        Ok(raw) => raw,
        Err(_) => return secrets::park(sync, change, "连接凭据密钥不匹配或密文校验失败，保留本机值"),
    };
    let mut credential: crate::mcp::apps::credentials::Credential = serde_json::from_str(&raw).map_err(|_| "连接凭据内容格式无效")?;
    credential.server_id = field_as_i64(fields, "server_id").filter(|id| *id > 0).ok_or("连接凭据缺少 MCP 服务映射")?;
    crate::mcp::apps::catalog::public_https(&credential.endpoint)?;
    let mapped = find_local_id_by_object_id(sync, vault::OBJECT_TYPE, &change.object_id)?;
    let target = mapped.or(find_row_by_unique_key(db, spec, fields)?);
    if !target.map(|id| remote_row_change_is_stale(sync, change, id)).transpose()?.unwrap_or(false) {
        vault::write_with(db, &key, &reference, &credential)?;
        let id: i64 = db.query_row("SELECT id FROM credential_vault WHERE reference=?1", [&reference], |r| r.get(0)).map_err(|e| e.to_string())?;
        remap_sync_object_map(sync, vault::OBJECT_TYPE, spec.table, id, &change.object_id)?;
    }
    upsert_shadow_from_remote_with_fields(sync, spec, change, fields)?;
    sync.execute("DELETE FROM sync_secret_pending WHERE object_id=?1 AND version<=?2", params![change.object_id, change.version]).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn connection_headers(db: &Connection) -> Result<(), String> {
    db.execute("UPDATE mcp_server SET headers=(SELECT CASE WHEN credential_ref IS NULL THEN '{}' ELSE json_object('x-aipp-credential-ref',credential_ref) END FROM app_connection WHERE server_id=mcp_server.id)
        WHERE id IN (SELECT server_id FROM app_connection)", []).map_err(|e| e.to_string())?;
    Ok(())
}

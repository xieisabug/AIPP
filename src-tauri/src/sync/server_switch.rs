//! Explicit, cancellable server changes. Inspection never mutates the target.
use super::*;
use std::sync::atomic::Ordering;

pub(super) struct SwitchSession {
    id: String,
    resume: bool,
    candidate: Option<Candidate>,
    commit_started: bool,
}

struct Candidate {
    url: String,
    token: String,
    account: String,
    mode: String,
    remote_key: Option<secrets::KeyCheckRecord>,
    key_known: bool,
    credentials_only: bool,
}

#[derive(Serialize)]
pub struct SwitchStarted { id: String, old_url: String, has_sync_state: bool }

#[derive(Deserialize)]
pub struct InspectSwitchRequest {
    id: String,
    server_url: String,
    token: String,
    choice: String,
}

#[derive(Serialize)]
pub struct SwitchInspection {
    mode: Option<String>,
    message: String,
    sample_count: usize,
    needs_choice: bool,
    has_remote_key: bool,
    key_known: bool,
}

#[derive(Deserialize)]
struct Inspection {
    account_id: String,
    latest_cursor: i64,
    remote_empty: bool,
    changes: Vec<PullChange>,
    key_check: Option<JsonValue>,
}

#[derive(Debug)]
struct Sample { seq: i64, kind: String, id: String, version: i64, hash: String, deleted: bool }

// Indexed rowid seeks: bounded work even with hundreds of thousands of objects.
fn samples(conn: &Connection) -> Result<Vec<Sample>, String> {
    let max: i64 = conn.query_row("SELECT COALESCE(MAX(rowid), 0) FROM sync_shadow", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    if max == 0 { return Ok(result); }
    for attempt in 0..40 {
        let start = if attempt == 0 { 1 } else { (rand::random::<u64>() % max as u64) as i64 + 1 };
        let found = conn.query_row(
            "SELECT server_seq, object_type, object_id, server_version, payload_hash, deleted_at IS NOT NULL
             FROM sync_shadow WHERE rowid >= ?1 ORDER BY rowid LIMIT 1",
            [start], |r| Ok(Sample { seq: r.get(0)?, kind: r.get(1)?, id: r.get(2)?, version: r.get(3)?, hash: r.get(4)?, deleted: r.get(5)? }),
        ).optional().map_err(|e| e.to_string())?;
        if let Some(row) = found {
            if row.seq > 0 && seen.insert(row.seq) { result.push(row); }
        }
        if result.len() == 10 { break; }
    }
    Ok(result)
}

fn classify(conn: &Connection, remote: &Inspection, samples: &[Sample]) -> Result<(Option<String>, String), String> {
    if remote.remote_empty { return Ok((Some("new".into()), "服务器没有同步数据，将上传本地数据。".into())); }
    if let Err(error) = ensure_remote_history_is_compatible(conn, remote.latest_cursor) { return Ok((None, error)); }
    if samples.is_empty() { return Ok((None, "本机没有可用于核对的已确认记录，请返回自行选择连接方式。".into())); }
    for sample in samples {
        let matched = remote.changes.iter().find(|r| r.seq == sample.seq);
        let Some(row) = matched else { return Ok((None, format!("服务器缺少本地已确认的历史位置 {}，无法按迁移继续。", sample.seq))); };
        if row.object_type != sample.kind || row.object_id != sample.id || row.version != sample.version
            || (row.operation == "delete") != sample.deleted
            || (!sample.deleted && sync_payload_hash(conn, row.payload.as_ref())? != sample.hash) {
            return Ok((None, format!("历史位置 {} 的对象、版本或内容不一致，无法按迁移继续。", sample.seq)));
        }
    }
    Ok((Some("migration".into()), format!("抽样检查一致，已核对 {} 条记录，未进行全量检查。将按服务器迁移继续。", samples.len())))
}

#[tauri::command]
pub async fn begin_sync_server_switch(app_handle: AppHandle) -> Result<SwitchStarted, String> {
    let state = app_handle.state::<SyncState>();
    let mut slot = state.switch_session.lock().await;
    if slot.is_some() { return Err("已有同步服务器切换正在进行，请先完成或取消。".into()); }
    let old_url = load_settings(&app_handle)?.server_url;
    ensure_sync_db(&app_handle)?;
    let resume = state.status.lock().await.running;
    state.switching.store(true, Ordering::SeqCst);
    stop_worker(&app_handle).await;
    let _guard = state.sync_lock.lock().await;
    let id = Uuid::new_v4().to_string();
    let checked = open_sync_db(&app_handle).and_then(|conn| has_sync_state(&conn));
    let has_sync_state = match checked {
        Ok(value) => value,
        Err(error) => {
            state.switching.store(false, Ordering::SeqCst);
            drop(_guard);
            drop(slot);
            if resume { start_worker(app_handle).await; }
            return Err(error);
        }
    };
    *slot = Some(SwitchSession { id: id.clone(), resume, candidate: None, commit_started: false });
    Ok(SwitchStarted { id, old_url, has_sync_state })
}

#[tauri::command]
pub async fn cancel_sync_server_switch(app_handle: AppHandle, id: String) -> Result<(), String> {
    let state = app_handle.state::<SyncState>();
    let mut slot = state.switch_session.lock().await;
    let session = slot.as_ref().filter(|s| s.id == id).ok_or("切换会话已失效")?;
    if session.commit_started { return Err("切换提交尚未完成恢复，不能返回旧连接；请重试保存并同步，或重启后查看具体同步错误。".into()); }
    let resume = session.resume;
    *slot = None;
    state.switching.store(false, Ordering::SeqCst);
    drop(slot);
    if resume { start_worker(app_handle).await; }
    Ok(())
}

#[tauri::command]
pub async fn inspect_sync_server_switch(app_handle: AppHandle, request: InspectSwitchRequest) -> Result<SwitchInspection, String> {
    let state = app_handle.state::<SyncState>();
    let mut slot = state.switch_session.lock().await;
    let session = slot.as_mut().filter(|s| s.id == request.id).ok_or("切换会话已失效")?;
    if session.commit_started { return Err("切换提交尚未完成恢复，请重试保存并同步。".into()); }
    session.candidate = None;
    let url = request.server_url.trim().trim_end_matches('/').to_string();
    let parsed = reqwest::Url::parse(&url).map_err(|_| "服务器地址必须是有效的 http(s) URL")?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() { return Err("服务器地址必须使用 http 或 https".into()); }
    if !["migration", "new", "auto", "credentials"].contains(&request.choice.as_str()) { return Err("请选择连接方式".into()); }
    let token = if request.token.trim().is_empty() { load_settings(&app_handle)?.token.unwrap_or_default() } else { request.token.trim().into() };
    if token.is_empty() { return Err("请填写访问 Token".into()); }
    ensure_sync_db(&app_handle)?;
    let device = ensure_device_id(&open_sync_db(&app_handle)?)?;
    let status = fetch_remote_status(&url, &token, &device, &CancellationToken::new()).await?;
    let sample = if request.choice == "auto" { samples(&open_sync_db(&app_handle)?)? } else { Vec::new() };
    let response = sync_http_client()?.post(format!("{url}/v1/sync/inspect")).bearer_auth(&token)
        .json(&json!({"sequences": sample.iter().map(|s| s.seq).collect::<Vec<_>>()})).send().await.map_err(|e| format!("检查同步历史请求失败: {e}"))?;
    let remote = if response.status() == StatusCode::NOT_FOUND || response.status() == StatusCode::METHOD_NOT_ALLOWED { None }
        else if !response.status().is_success() { return Err(format!("同步历史检查返回 HTTP {}", response.status())); }
        else { Some(response.json::<Inspection>().await.map_err(|e| format!("解析同步历史检查结果失败: {e}"))?) };
    if remote.as_ref().is_some_and(|r| r.account_id != status.account_id) { return Err("检查期间服务端账号发生变化，请重试".into()); }
    let (mode, message) = match request.choice.as_str() {
        "auto" => match &remote {
            Some(remote) => classify(&open_sync_db(&app_handle)?, remote, &sample)?,
            None => (None, "此服务器不支持自动检查，请返回自行选择连接方式。".into()),
        },
        "credentials" => {
            let previous = get_sync_meta(&open_sync_db(&app_handle)?, "remote_account_scope")?;
            if previous.as_deref() == Some(remote_scope(&url, &status.account_id).as_str()) {
                (Some("migration".into()), "访问 Token 已验证，仍对应原账号，将保留同步进度。".into())
            } else { (None, "访问 Token 对应的账号已变更，请选择服务器迁移或新同步服务器。".into()) }
        },
        "new" => (Some("new".into()), "重新同步本地与服务器的数据，保留本地现有内容；已有数据将合并，可能产生冲突或重复。".into()),
        _ => (Some("migration".into()), "保留现有同步进度，连接新地址后继续同步。".into()),
    };
    let remote_key = remote.as_ref().and_then(|r| r.key_check.as_ref()).map(|payload| {
        let raw = payload.pointer("/fields/value").and_then(JsonValue::as_str).ok_or("服务器同步口令校验记录缺少 value")?;
        serde_json::from_str::<secrets::KeyCheckRecord>(raw).map_err(|e| format!("服务器同步口令校验记录无效: {e}"))
    }).transpose()?;
    let key_known = remote.is_some();
    let has_remote_key = remote_key.is_some();
    if let Some(mode) = &mode {
        session.candidate = Some(Candidate { url, token, account: status.account_id, mode: mode.clone(), remote_key, key_known, credentials_only: request.choice == "credentials" });
    }
    Ok(SwitchInspection { needs_choice: request.choice == "credentials" && mode.is_none(), mode, message, sample_count: sample.len(), has_remote_key, key_known })
}

// Rebuild transport state, never business data or stable object identities.
fn prepare_new_target(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("DELETE FROM sync_cursor; DELETE FROM sync_shadow;
        DELETE FROM sync_dead_letter; DELETE FROM sync_secret_pending; DELETE FROM sync_secret_server;
        DELETE FROM sync_bootstrap_attempt; DELETE FROM sync_branch_route;
        DELETE FROM sync_outbox WHERE operation='upsert' OR status NOT IN ('pending','pushing','failed');
        UPDATE sync_outbox SET base_version=0, status='pending', sent_once=0, outcome_known=0,
            next_retry_at=NULL, retry_count=0;
        DELETE FROM sync_meta WHERE key IN ('bootstrap_origin','remote_account_scope','secret_server_wins_pending');")
        .map_err(|e| e.to_string())?;
    // Deletes remain explicit events, but their IDs/versions belong to this new target.
    let rows = conn.prepare("SELECT id FROM sync_outbox").map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for id in rows { conn.execute("UPDATE sync_outbox SET event_id=?1 WHERE id=?2", params![Uuid::new_v4().to_string(), id]).map_err(|e| e.to_string())?; }
    // Also cover older servers without /inspect: do not queue secrets until
    // their encryption configuration and existing values have been pulled.
    secrets::begin_server_wins(conn)?;
    set_sync_meta(conn, "new_target_pull_pending", "1")?;
    set_sync_meta(conn, "allow_initial_merge", "1")
}

fn bind_target(conn: &Connection, scope: &str, is_new: bool, id: &str) -> Result<(), String> {
    if is_new { prepare_new_target(conn)?; }
    set_sync_meta(conn, "remote_account_scope", scope)?;
    if get_sync_meta(conn, "bootstrap_origin")?.is_some() || is_new {
        set_sync_meta(conn, "bootstrap_origin", scope)?;
    }
    set_sync_meta(conn, META_NEEDS_RESET, "0")?;
    set_sync_meta(conn, "last_server_switch_id", id)
}

#[tauri::command]
pub async fn commit_sync_server_switch(app_handle: AppHandle, feature_state: State<'_, crate::FeatureConfigState>, id: String, passphrase: Option<String>) -> Result<SyncStatusDto, String> {
    let state = app_handle.state::<SyncState>();
    let mut slot = state.switch_session.lock().await;
    let session = slot.as_mut().filter(|s| s.id == id).ok_or("切换会话已失效")?;
    let candidate = session.candidate.as_ref().ok_or("请先检查并确认连接方式")?;
    let _guard = state.sync_lock.lock().await;
    let _credential_guard = crate::mcp::apps::credentials::CREDENTIAL_LOCK.lock().await;
    // Reauthenticate immediately before commit. No remote writes have happened yet.
    let status = fetch_remote_status(&candidate.url, &candidate.token, "", &CancellationToken::new()).await?;
    if status.account_id != candidate.account { return Err("目标账号在检查后发生变化，请返回重新检查".into()); }
    let master = get_or_create_master_key(&app_handle)?;
    let current = load_secret_state(&app_handle)?;
    let phrase = passphrase.filter(|p| !p.is_empty());
    if phrase.is_some() && !candidate.key_known { return Err("服务器不支持口令预检查，请先沿用当前口令，连接后在 API Key 加密同步中输入目标口令。".into()); }
    let record = candidate.remote_key.clone();
    let key = if candidate.credentials_only {
        // Rotating access credentials for the same account never changes API-key encryption.
        None
    } else if let Some(phrase) = phrase {
        let existing = current.ready_key().cloned();
        Some(tauri::async_runtime::spawn_blocking(move || -> Result<secrets::SecretKey, String> {
            if let Some(record) = record {
                let key = secrets::derive_with_record(&phrase, &record)?;
                if key.record != record { return Err("口令与目标服务器不匹配，请重新输入。".into()); }
                Ok(key)
            } else if let Some(existing) = existing {
                let key = secrets::derive_with_record(&phrase, &existing.record)?;
                if key.record != existing.record { return Err("已有加密口令不能在连接流程中更换，请沿用当前口令。".into()); }
                Ok(key)
            } else {
                if phrase.chars().count() < secrets::MIN_PASSPHRASE_CHARS { return Err("同步口令至少需要 8 个字符".into()); }
                secrets::derive_key(&phrase, &rand::random::<[u8; 16]>(), secrets::KDF_ITERATIONS)
            }
        }).await.map_err(|e| format!("验证同步口令失败: {e}"))??)
    } else {
        let key = current.ready_key().cloned();
        if candidate.remote_key.as_ref().is_some_and(|r| key.as_ref().map(|k| &k.record) != Some(r)) {
            return Err("当前口令与目标服务器不匹配，请输入目标服务器的口令。".into());
        }
        key
    };
    let url = candidate.url.clone();
    let token = candidate.token.clone();
    let scope = remote_scope(&url, &candidate.account);
    let is_new = candidate.mode == "new";
    let (ciphertext, nonce) = encrypt_secret_with_key(&master, &token)?;
    {
        recover_pending_business_applies(&app_handle)?;
        // Preserve unsent local deletions before discarding old shadow state.
        if is_new { scan_local_changes(&app_handle)?; }
        let sync = open_sync_db(&app_handle)?;
        let system = open_data_db(&get_db_path(&app_handle, "system.db")?)?;
        session.commit_started = true;
        let apply = || {
            bind_target(&sync, &scope, is_new, &id)?;
            for (name, value) in [("mode", "self_hosted"), ("server_url", url.as_str())] {
                system.execute("INSERT INTO feature_config (feature_code,key,value,data_type,description) VALUES ('data_sync',?1,?2,'string','')
                    ON CONFLICT(feature_code,key) DO UPDATE SET value=excluded.value", params![name, value]).map_err(|e| e.to_string())?;
            }
            system.execute("INSERT INTO secure_config (scope,key,ciphertext,nonce,updated_time) VALUES ('data_sync','token',?1,?2,CURRENT_TIMESTAMP)
                ON CONFLICT(scope,key) DO UPDATE SET ciphertext=excluded.ciphertext,nonce=excluded.nonce,updated_time=CURRENT_TIMESTAMP", params![ciphertext, nonce]).map_err(|e| e.to_string())?;
            Ok(())
        };
        let committed = if let Some(key) = &key {
            vault::switch_server(&app_handle, &sync, &system, key, apply)
        } else {
            persistence::coordinated(&sync, &system, apply)
        };
        if let Err(error) = committed {
            // The business commit may already be durable. Recover before deciding
            // whether cancellation/retry may still return to the old connection.
            persistence::recover(&sync, &system).map_err(|recovery| format!("切换提交失败: {error}；恢复失败: {recovery}"))?;
            if get_sync_meta(&sync, "last_server_switch_id")?.as_deref() != Some(id.as_str()) {
                session.commit_started = false;
                return Err(error);
            }
        }
    }
    // Refresh only the in-memory cache; database settings were committed above.
    for (name, value) in [("mode", "self_hosted"), ("server_url", url.as_str())] {
        let config = FeatureConfig { id: None, feature_code: FEATURE_CODE.into(), key: name.into(), value: value.into(), data_type: "string".into(), description: Some(String::new()) };
        let mut configs = feature_state.configs.lock().await;
        configs.retain(|c| !(c.feature_code == FEATURE_CODE && c.key == name));
        configs.push(config.clone());
        feature_state.config_feature_map.lock().await.entry(FEATURE_CODE.into()).or_default().insert(name.into(), config);
    }
    *slot = None;
    state.switching.store(false, Ordering::SeqCst);
    drop(_guard);
    drop(slot);
    let _ = app_handle.emit("feature_config_changed", ());
    apply_persisted_auto_sync_to_runtime(&app_handle).await;
    if load_auto_sync_enabled(&app_handle) { start_worker(app_handle.clone()).await; }
    // Return promptly; status events report actual completion/errors, never a premature success.
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move { run_sync_once_manual(&app).await; });
    sync_status(&app_handle).await
}

#[cfg(test)]
#[path = "server_switch_tests.rs"]
mod tests;

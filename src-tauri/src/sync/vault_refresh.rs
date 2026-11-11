//! Cross-device OAuth refresh is claimed against a confirmed server version.
use super::*;
use crate::mcp::apps::credentials;

async fn load(app: &AppHandle, reference: &str) -> Result<credentials::Credential,String> {
    let app = app.clone(); let reference = reference.to_owned();
    tokio::task::spawn_blocking(move || credentials::load(&app,&reference)).await.map_err(|e|format!("读取刷新凭据失败: {e}"))?
}
fn expired(value: &credentials::Credential) -> bool {
    value.oauth.as_ref().and_then(|o|o.expires_at).is_some_and(|t|t <= Utc::now().timestamp()+60)
}
pub(crate) async fn resolve(app: &AppHandle, reference: &str, endpoint: &str, server_id: i64) -> Result<HashMap<String,String>,String> {
    let credential = load(app,reference).await?;
    let settings = load_settings(app)?;
    if !expired(&credential) || settings.mode != SyncMode::SelfHosted {
        return credentials::resolve_local(app,reference,endpoint,server_id).await;
    }
    match load_secret_state(app)? {
        secrets::SecretState::NotConfigured => return credentials::resolve_local(app,reference,endpoint,server_id).await,
        secrets::SecretState::Ready(_) => {},
        _ => return Err("共享连接凭据需要正确的同步口令才能刷新，请先解锁凭据同步".into()),
    }
    let state = app.try_state::<SyncState>().ok_or("凭据刷新缺少同步状态")?;
    let _sync_guard = state.sync_lock.lock().await;
    let cancel = CancellationToken::new();
    run_sync_once_inner(app,&cancel).await.map_err(|e|format!("刷新凭据前同步失败: {e}"))?;
    if !expired(&load(app,reference).await?) {
        return credentials::resolve_local(app,reference,endpoint,server_id).await;
    }
    let (object_id,version,device_id) = {
        let db = SystemDatabase::new(app).map_err(|e|e.to_string())?;
        let id:i64 = db.conn.query_row("SELECT id FROM credential_vault WHERE reference=?1",[reference],|r|r.get(0)).map_err(|e|e.to_string())?;
        let sync = open_sync_db(app)?;
        let object_id = find_object_id(&sync,vault::OBJECT_TYPE,id)?.ok_or("连接凭据尚未同步，无法协调刷新")?;
        let version:i64 = sync.query_row("SELECT server_version FROM sync_shadow WHERE object_type=?1 AND object_id=?2 AND deleted_at IS NULL",
            params![vault::OBJECT_TYPE,object_id],|r|r.get(0)).map_err(|_|"连接凭据上传尚未确认，不能开始刷新")?;
        let pending: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE object_type=?1 AND object_id=?2)",params![vault::OBJECT_TYPE,object_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if pending { return Err("连接凭据尚有未确认的上传，暂不能刷新，请先完成同步".into()); }
        (object_id,version,ensure_device_id(&sync)?)
    };
    let token = settings.token.as_deref().ok_or("缺少同步服务 Token，无法协调凭据刷新")?;
    let response = sync_http_client()?.post(format!("{}/v1/sync/vault/claim-refresh",settings.server_url))
        .bearer_auth(token).json(&json!({"device_id":device_id,"object_id":object_id,"version":version,"claim_id":Uuid::new_v4().to_string()}))
        .send().await.map_err(|e|format!("申请凭据刷新失败: {}",e.without_url()))?;
    if response.status() == StatusCode::NOT_FOUND { return Err("同步服务器尚不支持跨设备凭据刷新，请升级同步服务器后重试".into()); }
    if response.status() == StatusCode::CONFLICT {
        return Err("连接凭据已更新或另一设备已开始刷新，请同步后重试；若上次刷新中断，请重新授权此应用".into());
    }
    if !response.status().is_success() { return Err(format!("申请凭据刷新失败: HTTP {}",response.status())); }
    let granted: JsonValue = response.json().await.map_err(|_|"凭据刷新确认响应格式无效")?;
    if granted["granted"] != true { return Err("同步服务器未确认凭据刷新权限".into()); }
    let result = credentials::resolve_local(app,reference,endpoint,server_id).await?;
    // Publish before another device may claim the next version. On failure the
    // local token remains durable and the old claim stays closed to blind retry.
    run_sync_once_inner(app,&cancel).await.map_err(|e|format!("凭据已在本机刷新，但同步新凭据失败: {e}"))?;
    {
        let sync = open_sync_db(app)?;
        let pending: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE object_type=?1 AND object_id=?2)",
            params![vault::OBJECT_TYPE,object_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if pending { return Err("凭据已在本机刷新，但新密文上传尚未确认，请完成同步后重试".into()); }
    }
    let current = load(app,reference).await?;
    if current.oauth.as_ref().is_some_and(|o| result.get("authorization") != Some(&format!("Bearer {}",o.access_token))) {
        return Err("刷新期间连接凭据已被另一设备更新，请重试".into());
    }
    Ok(result)
}

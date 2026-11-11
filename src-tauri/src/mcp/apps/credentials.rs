use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const REFERENCE_HEADER: &str = "x-aipp-credential-ref";
/// The connection exists (it syncs) but this device holds no copy of its credential. A device-local
/// condition, never an authentication failure: recording it would disable the connection everywhere.
pub const MISSING_ON_DEVICE: &str = "此设备没有该连接的凭据：连接凭据只在设置同步口令后才会同步到其他设备，请在「程序功能 → 数据」中设置或解锁同步口令";
pub static CREDENTIAL_LOCK: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));

// Never derive Debug: this value only exists on the trusted Rust side.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub server_id: i64,
    pub endpoint: String,
    #[serde(serialize_with = "serialize_headers")]
    pub headers: HashMap<String, String>,
    pub oauth: Option<OAuthCredential>,
}

// Stable plaintext ordering keeps deterministic sync envelopes from producing
// a new ciphertext every time a HashMap is read on another device.
fn serialize_headers<S: serde::Serializer>(headers: &HashMap<String, String>, serializer: S) -> Result<S::Ok, S::Error> {
    headers.iter().collect::<std::collections::BTreeMap<_, _>>().serialize(serializer)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthCredential {
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub client_auth_method: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
    pub client_id: String,
    pub token_endpoint: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
}

fn entry(reference: &str) -> Result<keyring::Entry, String> {
    uuid::Uuid::parse_str(reference).map_err(|_| "连接凭据引用无效".to_string())?;
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return Err("此平台尚未接入应用连接的系统安全存储；不会保存明文凭据".into());
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    keyring::Entry::new("com.aipp.connected-apps", reference).map_err(|_| "无法访问系统安全存储".into())
}

pub fn save(app: &tauri::AppHandle, reference: &str, value: &Credential) -> Result<(), String> {
    crate::sync::vault::write(app, reference, value)
}

pub fn load(app: &tauri::AppHandle, reference: &str) -> Result<Credential, String> {
    load_optional(app, reference)?.ok_or_else(|| MISSING_ON_DEVICE.into())
}

pub fn load_optional(app: &tauri::AppHandle, reference: &str) -> Result<Option<Credential>, String> {
    if let Some(value) = crate::sync::vault::read(app, reference)? {
        cleanup_legacy(app, reference)?;
        return Ok(Some(value));
    }
    // Migrate the legacy OS-store entry only after the encrypted write succeeds.
    match entry(reference)?.get_password() {
        Ok(raw) => {
            let value: Credential = serde_json::from_str(&raw).map_err(|_| "旧连接凭据格式无效")?;
            let db = crate::db::system_db::SystemDatabase::new(app).map_err(|e|e.to_string())?;
            db.conn.execute("INSERT OR IGNORE INTO credential_vault_legacy_cleanup(reference) VALUES(?1)",[reference]).map_err(|e|e.to_string())?;
            save(app, reference, &value)?;
            cleanup_legacy(app, reference)?;
            Ok(Some(value))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("迁移旧连接凭据失败，请解锁系统安全存储: {e}")),
    }
}
fn cleanup_legacy(app: &tauri::AppHandle, reference: &str) -> Result<(),String> {
    let db = crate::db::system_db::SystemDatabase::new(app).map_err(|e|e.to_string())?;
    let pending: bool = db.conn.query_row("SELECT EXISTS(SELECT 1 FROM credential_vault_legacy_cleanup WHERE reference=?1)",[reference],|r|r.get(0)).map_err(|e|e.to_string())?;
    if pending {
        delete_legacy(reference)?;
        db.conn.execute("DELETE FROM credential_vault_legacy_cleanup WHERE reference=?1",[reference]).map_err(|e|e.to_string())?;
    }
    Ok(())
}
fn delete_legacy(reference: &str) -> Result<(), String> {
    match entry(reference)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除旧连接凭据失败: {e}")),
    }
}
pub fn clear(app: &tauri::AppHandle, reference: &str) -> Result<(), String> {
    delete_legacy(reference)?;
    crate::sync::vault::remove(app, reference)
}

pub fn migrate_all(app: &tauri::AppHandle) -> Result<(), String> {
    let db = super::db(app)?;
    let references = db.conn.prepare("SELECT credential_ref FROM app_connection WHERE credential_ref IS NOT NULL UNION SELECT reference FROM app_credential_cleanup")
        .map_err(|e| e.to_string())?.query_map([], |r| r.get::<_,String>(0)).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for reference in references { load_optional(app, &reference)?; }
    // MCP deletion cascades app metadata in another database. Remove orphaned
    // Vault rows so the ordinary sync deletion scanner publishes tombstones.
    let _lock = crate::sync::vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let system = crate::db::system_db::SystemDatabase::new(app).map_err(|e|e.to_string())?;
    crate::sync::vault::init(&system.conn)?;
    let rows = system.conn.prepare("SELECT reference,server_id FROM credential_vault WHERE server_id>0").map_err(|e|e.to_string())?
        .query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).map_err(|e|e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
    for (reference,id) in rows {
        let exists:bool = db.conn.query_row("SELECT EXISTS(SELECT 1 FROM mcp_server WHERE id=?1)",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !exists { system.conn.execute("DELETE FROM credential_vault WHERE reference=?1",[reference]).map_err(|e|e.to_string())?; }
    }
    Ok(())
}

#[cfg(test)]
pub fn validate_binding(value: &Credential, endpoint: &str) -> Result<(), String> {
    if value.endpoint != endpoint { return Err("服务地址已变化，旧授权不可用于新目标，请重新连接".into()); }
    super::catalog::public_https(endpoint)?;
    Ok(())
}

/// Runs detached from the caller: a tool timeout or user cancel must not drop an in-flight refresh after the
/// provider rotated the refresh token, or a sync-mode refresh claim after it was granted.
pub async fn resolve(app: &tauri::AppHandle, reference: &str, endpoint: &str, server_id: i64) -> Result<HashMap<String, String>, String> {
    let (app, reference, endpoint) = (app.clone(), reference.to_owned(), endpoint.to_owned());
    tokio::spawn(async move { crate::sync::vault_refresh::resolve(&app, &reference, &endpoint, server_id).await })
        .await.map_err(|e| format!("连接 {server_id} 的凭据解析任务失败：{e}"))?
}
pub(crate) async fn resolve_local(app: &tauri::AppHandle, reference: &str, endpoint: &str, server_id: i64) -> Result<HashMap<String, String>, String> {
    resolve_with(Arc::new(SystemStore(app.clone())), &oauth_http_for(app, server_id).await?, reference, endpoint, server_id).await
}

/// OAuth traffic for a connection uses the same proxy setting as its MCP calls.
pub(crate) async fn oauth_http_for(app: &tauri::AppHandle, server_id: i64) -> Result<super::oauth::OAuthHttp, String> {
    let server = crate::db::mcp_db::MCPDatabase::new(app).map_err(|e| e.to_string())?.get_mcp_server(server_id)
        .map_err(|e| format!("读取连接 {server_id} 的网络设置失败：{e}"))?;
    super::oauth::OAuthHttp::with_builder(crate::mcp::http_transport::configured_http_builder(app, &server).await?)
}

pub(super) trait CredentialStore: Send + Sync + 'static {
    fn load(&self, reference: &str) -> Result<Credential, String>;
    fn save(&self, reference: &str, credential: &Credential) -> Result<(), String>;
    fn save_refreshed(&self, reference: &str, _expected: &Credential, credential: &Credential) -> Result<(), String> { self.save(reference,credential) }

}
struct SystemStore(tauri::AppHandle);
impl CredentialStore for SystemStore {
    fn save_refreshed(&self, reference: &str, expected: &Credential, credential: &Credential) -> Result<(), String> {
        crate::sync::vault::compare_write(&self.0, reference, Some(expected), credential)
    }
    fn load(&self, reference: &str) -> Result<Credential, String> { load(&self.0, reference) }
    fn save(&self, reference: &str, credential: &Credential) -> Result<(), String> { save(&self.0, reference, credential) }
}

pub(super) async fn resolve_with(store: Arc<dyn CredentialStore>, http: &super::oauth::OAuthHttp, reference: &str, endpoint: &str, server_id: i64) -> Result<HashMap<String, String>, String> {
    let guard = CREDENTIAL_LOCK.clone().lock_owned().await;
    let reference = reference.to_owned();
    let key = reference.clone();
    let reader = store.clone();
    let mut credential = tokio::task::spawn_blocking(move || reader.load(&key)).await.map_err(|_| "读取连接凭据任务失败")??;
    if credential.endpoint != endpoint { return Err("服务地址已变化，旧授权不可用于新目标，请重新连接".into()); }
    http.endpoint(endpoint)?;
    if credential.server_id != server_id { return Err("凭据属于其他连接，不能复制使用".into()); }
    let expected = credential.clone();
    if credential.oauth.as_ref().is_some_and(|oauth| oauth.expires_at.is_some_and(|expiry| expiry <= chrono::Utc::now().timestamp() + 60)) {
        // Exchange and write run in their own task holding the lock: once the provider rotates the refresh
        // token, the new one must be stored even if the caller times out or is cancelled meanwhile.
        let http = http.clone();
        let endpoint = endpoint.to_owned();
        credential = tokio::spawn(async move {
            let mut credential = credential;
            let oauth = credential.oauth.as_mut().expect("checked above");
            let refresh = oauth.refresh_token.clone().ok_or("登录已过期，请重新授权")?;
            let token = super::oauth::exchange_authenticated(&http, &oauth.token_endpoint, &[
                ("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &oauth.client_id),
                ("resource", oauth.resource.as_deref().unwrap_or(&endpoint)),
            ],oauth.client_auth_method.as_deref().unwrap_or("none"),&oauth.client_id,oauth.client_secret.as_deref()).await?;
            oauth.access_token = token.access_token;
            if token.refresh_token.is_some() { oauth.refresh_token = token.refresh_token; }
            oauth.expires_at = token.expires_in.map(|v| chrono::Utc::now().timestamp() + v as i64);
            let copy = credential.clone();
            tokio::task::spawn_blocking(move || { let _guard = guard; store.save_refreshed(&reference, &expected, &copy) }).await.map_err(|_| "保存刷新凭据任务失败")??;
            Ok::<_, String>(credential)
        }).await.map_err(|e| format!("刷新连接凭据任务失败：{e}"))??;
    } else { drop(guard); }
    if let Some(oauth) = credential.oauth.as_ref() {
        let token = &oauth.access_token;
        if token.is_empty() { return Err("尚未完成登录，请连接应用".into()); }
        credential.headers.insert("authorization".into(), format!("Bearer {token}"));
    }
    Ok(credential.headers)
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod credentials_tests;

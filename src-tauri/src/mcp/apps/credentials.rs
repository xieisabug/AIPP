use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const REFERENCE_HEADER: &str = "x-aipp-credential-ref";
pub static CREDENTIAL_LOCK: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));

// Never derive Debug: this value only exists on the trusted Rust side.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub server_id: i64,
    pub endpoint: String,
    pub headers: HashMap<String, String>,
    pub oauth: Option<OAuthCredential>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthCredential {
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

pub fn save(reference: &str, value: &Credential) -> Result<(), String> {
    let serialized = serde_json::to_string(value).map_err(|_| "无法编码连接凭据")?;
    let entry = entry(reference)?;
    entry.set_password(&serialized).map_err(|_| "系统安全存储写入失败，连接未保存；请检查钥匙串是否解锁")?;
    if entry.get_password().map_err(|_| "无法验证安全存储写入")? != serialized { return Err("安全存储写入校验失败".into()); }
    Ok(())
}

pub fn load(reference: &str) -> Result<Credential, String> {
    load_optional(reference)?.ok_or_else(|| "连接凭据不存在，请重新连接".into())
}

pub fn load_optional(reference: &str) -> Result<Option<Credential>, String> {
    match entry(reference)?.get_password() {
        Ok(raw) => serde_json::from_str(&raw).map(Some).map_err(|_| "连接凭据格式无效，请重新连接".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("系统安全存储不可访问，请解锁后重试".into()),
    }
}

pub fn clear(reference: &str) -> Result<(), String> {
    match entry(reference)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("系统安全存储未能删除凭据；连接已停用，请解锁钥匙串后重试".into()),
    }
}

#[cfg(test)]
pub fn validate_binding(value: &Credential, endpoint: &str) -> Result<(), String> {
    if value.endpoint != endpoint { return Err("服务地址已变化，旧授权不可用于新目标，请重新连接".into()); }
    super::catalog::public_https(endpoint)?;
    Ok(())
}

pub async fn resolve(reference: &str, endpoint: &str, server_id: i64) -> Result<HashMap<String, String>, String> {
    resolve_with(Arc::new(SystemStore), &super::oauth::OAuthHttp::new()?, reference, endpoint, server_id).await
}

pub(super) trait CredentialStore: Send + Sync {
    fn load(&self, reference: &str) -> Result<Credential, String>;
    fn save(&self, reference: &str, credential: &Credential) -> Result<(), String>;
}
struct SystemStore;
impl CredentialStore for SystemStore {
    fn load(&self, reference: &str) -> Result<Credential, String> { load(reference) }
    fn save(&self, reference: &str, credential: &Credential) -> Result<(), String> { save(reference, credential) }
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
    if let Some(oauth) = credential.oauth.as_mut() {
        if oauth.expires_at.is_some_and(|expiry| expiry <= chrono::Utc::now().timestamp() + 60) {
            let refresh = oauth.refresh_token.as_ref().ok_or("登录已过期，请重新授权")?;
            let token = super::oauth::exchange_with(http, &oauth.token_endpoint, &[
                ("grant_type", "refresh_token"), ("refresh_token", refresh), ("client_id", &oauth.client_id),
                ("resource", endpoint),
            ]).await?;
            oauth.access_token = token.access_token;
            if token.refresh_token.is_some() { oauth.refresh_token = token.refresh_token; }
            oauth.expires_at = token.expires_in.map(|v| chrono::Utc::now().timestamp() + v as i64);
            let key = reference.clone();
            let copy = credential.clone();
            // A cancelled caller must not release the lock before the blocking write ends.
            tokio::task::spawn_blocking(move || { let _guard = guard; store.save(&key, &copy) }).await.map_err(|_| "保存刷新凭据任务失败")??;
        }
        let token = &credential.oauth.as_ref().unwrap().access_token;
        if token.is_empty() { return Err("尚未完成登录，请连接应用".into()); }
        credential.headers.insert("authorization".into(), format!("Bearer {token}"));
    }
    Ok(credential.headers)
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod credentials_tests;

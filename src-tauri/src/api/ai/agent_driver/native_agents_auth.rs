//! Explicit browser sign-in; normal chat/model probes never start OAuth.
//! Antigravity signs in through its own ACP runtime; Cursor through the SDK's
//! `Cursor.auth.login()`, storing the minted key per provider instance.
use super::*;

fn logins() -> &'static Mutex<HashMap<i64, Arc<tokio::sync::Notify>>> {
    static LOGINS: OnceLock<Mutex<HashMap<i64, Arc<tokio::sync::Notify>>>> = OnceLock::new();
    LOGINS.get_or_init(Default::default)
}
struct LoginGuard(i64);
impl Drop for LoginGuard { fn drop(&mut self) { if let Ok(mut logins) = logins().lock() { logins.remove(&self.0); } } }

fn cancel_login(provider_id: i64) -> Result<(), String> {
    if let Some(cancel) = logins().lock().map_err(|_|"Login state lock poisoned")?.get(&provider_id) { cancel.notify_one(); }
    Ok(())
}

#[tauri::command]
pub async fn cancel_antigravity_login(provider_id: i64) -> Result<(), String> { cancel_login(provider_id) }

#[tauri::command]
pub async fn cancel_native_agent_login(provider_id: i64) -> Result<(), String> { cancel_login(provider_id) }

#[tauri::command]
pub async fn login_antigravity(app: tauri::AppHandle, provider_id: i64, settings: HashMap<String, String>, on_url: tauri::ipc::Channel<String>) -> Result<(), String> {
    browser_login(&app, provider_id, "antigravity", "Antigravity", settings, on_url).await.map(|_| ())
}

/// Per-provider Cursor credential written by the bridge in the SDK's StoredSdkCredentials shape.
pub(crate) fn cursor_auth_store(app: &tauri::AppHandle, provider_id: i64) -> Result<PathBuf, AppError> {
    Ok(app.path().app_data_dir().map_err(error)?.join("agent-profiles").join("cursor").join(provider_id.to_string()).join("auth.json"))
}

/// Account shown in the provider form. Never contains the key itself.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct CursorAccount { pub signed_in: bool, pub email: Option<String>, pub expires_at_ms: Option<i64>, pub expired: bool }

pub(crate) fn read_cursor_account(path: &std::path::Path, now_ms: i64) -> Result<CursorAccount, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(CursorAccount { signed_in: false, email: None, expires_at_ms: None, expired: false }),
        Err(error) => return Err(format!("无法读取 Cursor 登录信息 {}：{error}", path.display())),
    };
    let value: Value = serde_json::from_str(&text).map_err(|e| format!("Cursor 登录信息 {} 已损坏，请重新登录：{e}", path.display()))?;
    if value["apiKey"].as_str().is_none_or(str::is_empty) { return Err(format!("Cursor 登录信息 {} 缺少密钥，请重新登录", path.display())); }
    let expires_at_ms = value["apiKeyExpiresAtMs"].as_i64();
    let expired = expires_at_ms.is_some_and(|expires| expires <= now_ms);
    Ok(CursorAccount { signed_in: !expired, email: value["email"].as_str().map(str::to_string), expires_at_ms, expired })
}

#[tauri::command]
pub async fn login_cursor(app: tauri::AppHandle, provider_id: i64, settings: HashMap<String, String>, on_url: tauri::ipc::Channel<String>) -> Result<CursorAccount, String> {
    let data = browser_login(&app, provider_id, "cursor", "Cursor", settings, on_url).await?;
    Ok(CursorAccount { signed_in: true, email: data["email"].as_str().map(str::to_string), expires_at_ms: data["expires_at_ms"].as_i64(), expired: false })
}

/// Forget this provider's Cursor sign-in. The minted key stays valid until it
/// expires unless revoked in the Cursor dashboard.
#[tauri::command]
pub async fn logout_cursor(app: tauri::AppHandle, provider_id: i64) -> Result<(), String> {
    if logins().lock().map_err(|_|"Login state lock poisoned")?.contains_key(&provider_id) { return Err("Cursor 正在登录，请先取消登录".into()); }
    // Running Cursor agents hold the key in memory; stop them like an update does.
    env::release_idle_sessions("cursor").await?;
    let path = cursor_auth_store(&app, provider_id).map_err(|e| e.to_string())?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法删除 Cursor 登录信息 {}：{error}", path.display())),
    }
}

async fn browser_login(app: &tauri::AppHandle, provider_id: i64, driver: &'static str, display: &str, settings: HashMap<String, String>, on_url: tauri::ipc::Channel<String>) -> Result<Value, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return Err(format!("MOBILE_UNSUPPORTED: {display} requires a local desktop runtime"));

    let provider_name;
    let mut config = {
        let db = crate::db::llm_db::LLMDatabase::new(app).map_err(|e|e.to_string())?;
        let provider = db.get_llm_provider(provider_id).map_err(|e|e.to_string())?;
        if provider.api_type != driver { return Err(format!("The selected provider is not {display}")); }
        provider_name = provider.name;
        // Use the form snapshot, so a pending autosave cannot sign in with old
        // paths/credentials. This does not persist or mutate provider settings.
        let configs = settings.into_iter().map(|(name, value)| LLMProviderConfig {
            id: 0, name, value, llm_provider_id: provider_id, append_location: String::new(), is_addition: false,
        }).collect::<Vec<_>>();
        configuration(&[], &configs, None).map_err(|e|e.to_string())?
    };
    configure_instance(app, provider_id, driver, &mut config).map_err(|e|e.to_string())?;
    if driver == "cursor" {
        if config.env.contains_key("CURSOR_API_KEY") { return Err("已配置 Cursor API Key（或 CURSOR_API_KEY 环境变量），会优先使用它；如需浏览器登录请先清除".into()); }
        config.wire["key_name"] = json!(format!("AIPP - {provider_name}"));
    }
    let cancel = Arc::new(tokio::sync::Notify::new());
    {
        let mut logins = logins().lock().map_err(|_|"Login state lock poisoned")?;
        if logins.contains_key(&provider_id) { return Err(format!("This {display} provider already has a pending sign-in")); }
        logins.insert(provider_id, cancel.clone());
    }
    let _guard = LoginGuard(provider_id);
    let files = bridge_files().await?;
    config.wire["temp_root"] = json!(files.0);
    let mut command = tokio::process::Command::new(&config.node);
    command.arg(files.0.join("bridge.mjs")).current_dir(&config.cwd).envs(&config.env)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)] { command.creation_flags(0x08000000); }
    #[cfg(unix)] { command.process_group(0); }
    let mut child = command.spawn().map_err(|e|format!("{display} sign-in spawn failed [provider_id={provider_id}]: {e}"))?;
    let diagnostics = AgentDiagnosticBuffer::for_environment(&config.env);
    let buffer = diagnostics.clone();
    let mut stderr = BufReader::new(child.stderr.take().ok_or("Login stderr missing")?).lines();
    let stderr_task = tokio::spawn(async move { while let Ok(Some(line)) = stderr.next_line().await { buffer.push(line); } });
    let exchange = async {
        let mut stdin = child.stdin.take().ok_or("Login stdin missing")?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("Login stdout missing")?).lines();
        write_frame(&mut stdin, &json!({"type":"login", "id":"login", "driver":driver, "config":config.wire})).await?;
        loop {
            let raw = lines.next_line().await.map_err(|e|e.to_string())?.ok_or_else(|| format!("{display} sign-in process closed before authentication completed"))?;
            let frame: Value = serde_json::from_str(&raw).map_err(|e|format!("Invalid {display} login frame: {e}"))?;
            if frame["type"] == "auth_url" {
                on_url.send(frame["url"].as_str().ok_or("Sign-in event omitted authorization URL")?.into()).map_err(|e|format!("Sign-in window is unavailable: {e}"))?;
            } else if frame["type"] == "fatal" || frame["ok"] == false {
                return Err(frame["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("{display} login error omitted native diagnostic")));
            } else if frame["id"] == "login" && frame["data"]["authenticated"] == true { return Ok(frame["data"].clone()); }
        }
    };
    let result = tokio::select! {
        result = timeout(Duration::from_secs(310), exchange) => result.unwrap_or_else(|_|Err(format!("{display} sign-in timed out after 5 minutes"))),
        _ = cancel.notified() => Err(format!("{display} sign-in cancelled by user")),
    };
    terminate_tree(&mut child).await;
    let _ = timeout(Duration::from_secs(2), stderr_task).await;
    result.map_err(|e|format!("{} [provider_id={provider_id}]; diagnostics: {:?}", diagnostics.sanitize(&e), diagnostics.diagnostics(child.try_wait().ok().flatten().map(|s|s.to_string()), None)))
}

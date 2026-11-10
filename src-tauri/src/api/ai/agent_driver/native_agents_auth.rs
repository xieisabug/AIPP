//! Explicit browser sign-in; normal chat/model probes never start OAuth.
use super::*;

fn logins() -> &'static Mutex<HashMap<i64, Arc<tokio::sync::Notify>>> {
    static LOGINS: OnceLock<Mutex<HashMap<i64, Arc<tokio::sync::Notify>>>> = OnceLock::new();
    LOGINS.get_or_init(Default::default)
}
struct LoginGuard(i64);
impl Drop for LoginGuard { fn drop(&mut self) { if let Ok(mut logins) = logins().lock() { logins.remove(&self.0); } } }

#[tauri::command]
pub async fn cancel_antigravity_login(provider_id: i64) -> Result<(), String> {
    if let Some(cancel) = logins().lock().map_err(|_|"Login state lock poisoned")?.get(&provider_id) { cancel.notify_one(); }
    Ok(())
}

#[tauri::command]
pub async fn login_antigravity(app: tauri::AppHandle, provider_id: i64, settings: HashMap<String, String>, on_url: tauri::ipc::Channel<String>) -> Result<(), String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return Err("MOBILE_UNSUPPORTED: Antigravity requires a local desktop runtime".into());

    let mut config = {
        let db = crate::db::llm_db::LLMDatabase::new(&app).map_err(|e|e.to_string())?;
        let provider = db.get_llm_provider(provider_id).map_err(|e|e.to_string())?;
        if provider.api_type != "antigravity" { return Err("The selected provider is not Antigravity".into()); }
        // Use the form snapshot, so a pending autosave cannot sign in with old
        // paths/credentials. This does not persist or mutate provider settings.
        let configs = settings.into_iter().map(|(name, value)| LLMProviderConfig {
            id: 0, name, value, llm_provider_id: provider_id, append_location: String::new(), is_addition: false,
        }).collect::<Vec<_>>();
        configuration(&[], &configs, None).map_err(|e|e.to_string())?
    };
    configure_instance(&app, provider_id, "antigravity", &mut config).map_err(|e|e.to_string())?;
    let cancel = Arc::new(tokio::sync::Notify::new());
    {
        let mut logins = logins().lock().map_err(|_|"Login state lock poisoned")?;
        if logins.contains_key(&provider_id) { return Err("This Antigravity provider already has a pending sign-in".into()); }
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
    let mut child = command.spawn().map_err(|e|format!("Antigravity sign-in spawn failed [provider_id={provider_id}]: {e}"))?;
    let diagnostics = AgentDiagnosticBuffer::for_environment(&config.env);
    let buffer = diagnostics.clone();
    let mut stderr = BufReader::new(child.stderr.take().ok_or("Login stderr missing")?).lines();
    let stderr_task = tokio::spawn(async move { while let Ok(Some(line)) = stderr.next_line().await { buffer.push(line); } });
    let exchange = async {
        let mut stdin = child.stdin.take().ok_or("Login stdin missing")?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("Login stdout missing")?).lines();
        write_frame(&mut stdin, &json!({"type":"login", "id":"login", "driver":"antigravity", "config":config.wire})).await?;
        loop {
            let raw = lines.next_line().await.map_err(|e|e.to_string())?.ok_or("Antigravity sign-in process closed before authenticate completed")?;
            let frame: Value = serde_json::from_str(&raw).map_err(|e|format!("Invalid Antigravity login frame: {e}"))?;
            if frame["type"] == "auth_url" {
                on_url.send(frame["url"].as_str().ok_or("Sign-in event omitted authorization URL")?.into()).map_err(|e|format!("Sign-in window is unavailable: {e}"))?;
            } else if frame["type"] == "fatal" || frame["ok"] == false {
                return Err(frame["error"].as_str().unwrap_or("Antigravity login error omitted native diagnostic").to_string());
            } else if frame["id"] == "login" && frame["data"]["authenticated"] == true { return Ok(()); }
        }
    };
    let result = tokio::select! {
        result = timeout(Duration::from_secs(310), exchange) => result.unwrap_or_else(|_|Err("Antigravity sign-in timed out after 5 minutes".into())),
        _ = cancel.notified() => Err("Antigravity sign-in cancelled by user".into()),
    };
    terminate_tree(&mut child).await;
    let _ = timeout(Duration::from_secs(2), stderr_task).await;
    result.map_err(|e|format!("{} [provider_id={provider_id}]; diagnostics: {:?}", diagnostics.sanitize(&e), diagnostics.diagnostics(child.try_wait().ok().flatten().map(|s|s.to_string()), None)))
}

//! Native Pi RPC, Cursor SDK, OpenCode HTTP/SSE and Kiro/Antigravity ACP adapters. The Node transport
//! is private to these drivers; all application routing uses the shared facade.
use super::registry::{AgentDriver, AgentDriverMetadata, AgentInstanceConfig, AgentSessionLookup, AgentSessionHost, PreparedAgentSession, AgentConfigField, AgentConfigValueType};
use super::session::{AgentSessionCapabilities, AgentSessionControl, AgentSessionHandle, AgentPromptContext};
use super::events::{AgentEventIdentity, emit_to_window};
use super::failure::{AgentDiagnosticBuffer, AgentFailure, AgentFailureStage, emit_normalized_failure_to_window};
use super::capabilities::{AgentCapabilities, AgentCapabilityDeclaration};
use super::AgentTurnRequest;
use crate::api::ai::events::ConversationEvent;
use crate::db::conversation_db::{ConversationDatabase, MessageAttachment, Repository};
use crate::db::llm_db::LLMProviderConfig;
use crate::db::assistant_db::AssistantModelConfig;
use crate::errors::AppError;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, process::Stdio};
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader}, sync::{mpsc, oneshot}, time::{timeout, Duration}};
use tauri::Manager;

pub const DRIVER_IDS: [&str; 5] = ["pi", "cursor", "opencode", "kiro", "antigravity"];
pub const DEFAULT_MODEL: &str = "__agent_default__";
#[path = "native_agents_auth.rs"]
pub(crate) mod auth;
#[path = "native_agents_env.rs"]
pub(crate) mod env;
pub fn is_native_agent(id: &str) -> bool { DRIVER_IDS.contains(&id) }
fn error(detail: impl std::fmt::Display) -> AppError { AppError::ProviderError(detail.to_string()) }

#[derive(Clone, Copy)]
pub(super) struct NativeDriver(pub &'static str);
impl NativeDriver {
    fn declaration(self) -> AgentCapabilityDeclaration {
        AgentCapabilityDeclaration { respond_permission: Some(self.0 != "cursor"),
            image_input: Some(false), audio_input: Some(false), embedded_context_input: Some(false),
            resume_session: Some(true), load_session: Some(false),
            ..AgentCapabilityDeclaration::unknown(true, true, false) }
    }
}

#[async_trait::async_trait]
impl AgentDriver for NativeDriver {
    fn metadata(&self) -> AgentDriverMetadata {
        AgentDriverMetadata { id: self.0, display_name: match self.0 { "pi" => "Pi", "cursor" => "Cursor", "kiro" => "Kiro", "antigravity" => "Antigravity", _ => "OpenCode" },
            message_model_name: self.0,
            operations: AgentSessionCapabilities { structured_attachments: false, cancel_prompt: true, set_config_option: true } }
    }
    fn matches_provider(&self, api_type: &str, _: &[LLMProviderConfig]) -> bool { api_type == self.0 }
    fn config_schema(&self) -> Vec<AgentConfigField> {
        ["agent_node_command", "agent_cli_command", "agent_sdk_directory", "acp_working_directory", "acp_env_vars",
            "antigravity_runtime", "antigravity_harness", "antigravity_auth_method", "antigravity_api_key", "kiro_agent", "kiro_effort", "cursor_api_key",
            "pi_approval", "kiro_autopilot", "antigravity_mode", "tool_review_mode"]
            .iter().map(|name| AgentConfigField { name: name.to_string(), required: false, value_type: AgentConfigValueType::String }).collect()
    }
    fn capabilities(&self) -> AgentCapabilities { AgentCapabilities::baseline(self.0, self.declaration()) }
    fn prepare(&self, request: &AgentTurnRequest<'_>, instance: &AgentInstanceConfig, model_override: Option<&str>) -> Result<Box<dyn PreparedAgentSession>, AppError> {
        let model = model_override.map(str::to_owned).or_else(|| request.assistant.model.first().map(|model| model.model_code.clone())).filter(|s| !s.is_empty() && s != DEFAULT_MODEL);
        let mut config = configuration(&request.assistant.model_configs, &instance.configs, model)?;
        if let Some(overrides) = request.overrides {
            for (key, value) in overrides {
                if !value.is_null() && value.as_str() != Some("") {
                    return Err(error(format!("{} does not support turn override '{key}'; no configuration was ignored", self.0)));
                }
            }
        }
        let proxy_enabled = instance.configs.iter().any(|c| c.name == "proxy_enabled" && c.value == "true");
        if proxy_enabled {
            if let Some(proxy) = crate::api::ai::config::get_network_proxy_from_config(request.feature_configs) {
                crate::api::ai::acp::apply_network_proxy_to_env_vars(&mut config.env, &proxy);
            }
        }
        Ok(Box::new(Prepared { driver: *self, instance: instance.clone(), config, mcp_scope: None }))
    }
    async fn lookup(&self, _: &tauri::AppHandle, conversation_id: i64) -> Result<Option<AgentSessionLookup>, AppError> {
        let mut entries = sessions().lock().await;
        let key = (self.0, conversation_id);
        if entries.get(&key).is_some_and(|entry| entry.control.tx.is_closed()) { entries.remove(&key); }
        Ok(entries.get(&key).map(|entry| AgentSessionLookup {
            handle: AgentSessionHandle::new(entry.control.clone(), entry.run_id.clone()),
            snapshot: entry.snapshot.lock().unwrap().clone(), run_id: entry.run_id.clone(), binding: None,
        }))
    }
}

#[derive(Clone)]
pub(crate) struct NativeConfig {
    node: String, pub cwd: PathBuf, env: HashMap<String, String>, wire: Value,
    /// Assistant `tool_review_mode=auto_model`: native permission requests go to AIPP's review model first.
    review: bool,
}
pub(crate) fn configure_instance(app: &tauri::AppHandle, provider_id: i64, driver: &str, config: &mut NativeConfig) -> Result<(), AppError> {
    if driver == "antigravity" {
        config.wire["profile"] = json!(app.path().app_data_dir().map_err(error)?.join("agent-profiles").join("antigravity").join(provider_id.to_string()));
    }
    if driver == "cursor" {
        config.wire["auth_store"] = json!(auth::cursor_auth_store(app, provider_id)?);
    }
    Ok(())
}
/// Assistant value first, then provider value; blank counts as unset.
fn setting(models: &[AssistantModelConfig], providers: &[LLMProviderConfig], key: &str) -> Option<String> {
    models.iter().find(|c| c.name == key).and_then(|c| c.value.clone())
        .or_else(|| providers.iter().find(|c| c.name == key).map(|c| c.value.clone())).filter(|s| !s.trim().is_empty())
}
pub(crate) fn expand_home(path: &str) -> PathBuf {
    if path == "~" { dirs::home_dir().unwrap_or_else(|| PathBuf::from(path)) }
    else if path.starts_with("~/") || path.starts_with("~\\") { dirs::home_dir().unwrap_or_default().join(&path[2..]) }
    else { PathBuf::from(path) }
}
/// Bridge environment overrides: dedicated credential fields, then provider and
/// assistant variables (later wins), then the chosen Node's directory on PATH.
pub(crate) fn agent_environment(models: &[AssistantModelConfig], providers: &[LLMProviderConfig], node: &str) -> Result<HashMap<String, String>, AppError> {
    let mut env = HashMap::new();
    if let Some(key) = setting(models, providers, "cursor_api_key") { env.insert("CURSOR_API_KEY".into(), key.trim().to_string()); }
    for raw in providers.iter().filter(|c|c.name == "acp_env_vars").map(|c|c.value.as_str())
        .chain(models.iter().filter(|c|c.name == "acp_env_vars").filter_map(|c|c.value.as_deref())) {
        env.extend(parse_environment(raw)?);
    }
    with_node_on_path(node, &mut env, std::env::var_os("PATH"));
    Ok(env)
}
/// An explicitly chosen Node (e.g. an nvm version directory) must also be the `node`
/// and global-CLI directory the agent's own child processes find. Directories already
/// on PATH are left in place so selecting a system Node never reorders the user's PATH.
pub(crate) fn with_node_on_path(node: &str, env: &mut HashMap<String, String>, inherited: Option<std::ffi::OsString>) {
    let node = std::path::Path::new(node);
    let Some(directory) = node.is_absolute().then(|| node.parent()).flatten() else { return; };
    let key = env.keys().find(|key| if cfg!(windows) { key.eq_ignore_ascii_case("PATH") } else { key.as_str() == "PATH" }).cloned();
    let current = key.as_ref().and_then(|key| env.get(key)).map(std::ffi::OsString::from).or(inherited).unwrap_or_default();
    let normalize = |path: &std::path::Path| {
        let text = path.to_string_lossy().trim_end_matches(['/', '\\']).to_string();
        if cfg!(windows) { text.to_lowercase() } else { text }
    };
    if std::env::split_paths(&current).any(|entry| normalize(&entry) == normalize(directory)) { return; }
    let Ok(joined) = std::env::join_paths(std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&current))) else { return; };
    env.insert(key.unwrap_or_else(|| "PATH".into()), joined.to_string_lossy().into_owned());
}
pub(crate) fn configuration(models: &[AssistantModelConfig], providers: &[LLMProviderConfig], model: Option<String>) -> Result<NativeConfig, AppError> {
    let get = |key: &str| setting(models, providers, key);
    let cwd = get("acp_working_directory").map(|path| expand_home(&path))
        .or_else(dirs::home_dir).ok_or_else(|| error("Cannot resolve native agent working directory"))?;
    if !cwd.is_dir() { return Err(error(format!("Agent working directory does not exist: {}", cwd.display()))); }
    let node = get("agent_node_command").unwrap_or_else(|| "node".into());
    let mut env = agent_environment(models, providers, &node)?;
    if get("acp_additional_args").is_some() { return Err(error("Dedicated native agent adapters do not accept generic ACP additional arguments; remove acp_additional_args")); }
    if let Some(key) = get("antigravity_api_key") { env.insert("AIPP_ANTIGRAVITY_API_KEY".into(), key); }
    let review = get("tool_review_mode").as_deref() == Some("auto_model");
    let wire = json!({ "cwd": cwd, "command": get("agent_cli_command"), "sdk_directory": get("agent_sdk_directory"), "model": model,
        "runtime": get("antigravity_runtime"), "harness": get("antigravity_harness"), "auth_method": get("antigravity_auth_method"),
        "auth_key": get("antigravity_api_key"), "kiro_agent": get("kiro_agent"), "kiro_effort": get("kiro_effort"),
        "pi_approval": get("pi_approval"), "kiro_autopilot": get("kiro_autopilot"), "antigravity_mode": get("antigravity_mode"),
        // Part of the session signature, so changing the review mode restarts the native session.
        "aipp_review": review });
    Ok(NativeConfig { node, cwd, env, wire, review })
}

struct Prepared {
    driver: NativeDriver, instance: AgentInstanceConfig, config: NativeConfig,
    mcp_scope: Option<crate::acp_mcp_bridge::provenance::NativeMcpScope>,
}
#[async_trait::async_trait]
impl PreparedAgentSession for Prepared {
    fn metadata(&self) -> AgentDriverMetadata { self.driver.metadata() }
    async fn finalize(&mut self, app: &tauri::AppHandle, cid: i64, payload: String) -> Result<(), AppError> {
        configure_instance(app, self.instance.provider_id, self.driver.0, &mut self.config)?;
        if payload.trim().is_empty() || payload.trim() == "[]" { return Ok(()); }
        use crate::acp_mcp_bridge::*;
        let proxy = ensure_proxy_server(app.clone()).await.map_err(error)?;
        let scope = provenance::NativeMcpScope::new(cid).map_err(error)?;
        let exe = std::env::current_exe().map_err(error)?;
        let db = crate::db::mcp_db::MCPDatabase::db_path(app).map_err(error)?;
        self.config.wire["mcp"] = json!({ "type": "stdio", "command": exe, "args": [ACP_MCP_BRIDGE_ARG], "env": {
            (ACP_MCP_DB_PATH_ENV): db, (ACP_MCP_CONVERSATION_ID_ENV): cid.to_string(),
            (ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV): "1", (ACP_MCP_PROXY_ADDR_ENV): proxy.addr,
            (ACP_MCP_PROXY_TOKEN_ENV): scope.token(), (ACP_MCP_SELECTED_TOOLS_ENV): payload,
        }});
        self.mcp_scope = Some(scope);
        Ok(())
    }
    async fn acquire(self: Box<Self>, host: &dyn AgentSessionHost, cid: i64) -> Result<AgentSessionHandle, AppError> {
        let app = host.app_handle()?.clone();
        let key = (self.driver.0, cid);
        // Include assistant/model/MCP selections as well as provider configuration.
        let mut signature_wire = self.config.wire.clone();
        if let Some(env) = signature_wire["mcp"]["env"].as_object_mut() {
            env.remove(crate::acp_mcp_bridge::ACP_MCP_PROXY_TOKEN_ENV);
        }
        let ordered_env: std::collections::BTreeMap<_, _> = self.config.env.iter().collect();
        let signature = format!("{}:{}:{}:{:?}", self.instance.config_fingerprint, self.config.node, signature_wire, ordered_env);
        let existing = sessions().lock().await.get(&key).cloned();
        if let Some(entry) = existing {
            if entry.signature == signature && !entry.control.tx.is_closed() {
                return Ok(AgentSessionHandle::new(entry.control.clone(), entry.run_id.clone()));
            }
            if !entry.control.tx.is_closed() {
                entry.control.request(json!({"type":"close"})).await?;
                timeout(Duration::from_secs(10), entry.control.tx.closed()).await.map_err(|_|error("Previous native session did not shut down"))?;
            }
        }
        sessions().lock().await.remove(&key);
        let session = ConversationDatabase::new(&app)?.get_agent_session_id_for_instance(cid, self.driver.0, Some(self.instance.provider_id))?;
        let run_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let busy = Arc::new(AtomicBool::new(false));
        let control = NativeControl { driver: self.driver, tx, busy: busy.clone() };
        let snapshot = Arc::new(Mutex::new(json!({ "conversation_id":cid, "agent_kind":self.driver.0, "session_id": null, "is_busy":false, "config_options":[] })));
        let (ready_tx, ready_rx) = oneshot::channel();
        let driver = self.driver;
        let actor_snapshot = snapshot.clone();
        let actor_run = run_id.clone();
        let actor_tx = control.tx.clone();
        tauri::async_runtime::spawn(async move { run_actor(app, cid, actor_run, *self, session, rx, actor_tx, busy, actor_snapshot, ready_tx).await; });
        timeout(Duration::from_secs(60), ready_rx).await.map_err(|_| error(format!("{} native initialization timed out [conversation_id={cid}, run_id={run_id}]", driver.0)))?
            .map_err(|_| error(format!("{} initialization actor closed [conversation_id={cid}, run_id={run_id}]", driver.0)))?.map_err(error)?;
        sessions().lock().await.insert(key, Entry { control: control.clone(), run_id: run_id.clone(), signature, snapshot });
        Ok(AgentSessionHandle::new(control, run_id))
    }
}

#[derive(Clone)]
struct Entry { control: NativeControl, run_id: String, signature: String, snapshot: Arc<Mutex<Value>> }
fn sessions() -> &'static tokio::sync::Mutex<HashMap<(&'static str, i64), Entry>> {
    static SESSIONS: OnceLock<tokio::sync::Mutex<HashMap<(&'static str, i64), Entry>>> = OnceLock::new();
    SESSIONS.get_or_init(Default::default)
}
pub(crate) async fn release_conversation(conversation_id: i64) {
    let mut entries = sessions().lock().await;
    for driver in DRIVER_IDS {
        if let Some(entry) = entries.remove(&(driver, conversation_id)) {
            let _ = entry.control.tx.send(Command::Shutdown);
        }
    }
}
enum Command {
    Shutdown,
    Prompt { id: i64, text: String, window: tauri::Window },
    Request { wire: Value, reply: oneshot::Sender<Result<Value, String>> },
    Reply { turn: i64, request_id: String, value: String },
    Answer { turn: i64, request_id: String, answers: Value },
    InteractionFailed { turn: i64, error: String },
}
#[derive(Clone)]
struct NativeControl { driver: NativeDriver, tx: mpsc::UnboundedSender<Command>, busy: Arc<AtomicBool> }
impl NativeControl {
    async fn request(&self, wire: Value) -> Result<Value, AppError> {
        let (reply, receiver) = oneshot::channel();
        self.tx.send(Command::Request { wire, reply }).map_err(error)?;
        timeout(Duration::from_secs(35), receiver).await.map_err(|_| error(format!("{} control response timed out", self.driver.0)))?
            .map_err(error)?.map_err(error)
    }
}
#[async_trait::async_trait]
impl AgentSessionControl for NativeControl {
    fn metadata(&self) -> AgentDriverMetadata { self.driver.metadata() }
    fn send_prompt(&self, id: i64, text: String, _: Vec<MessageAttachment>, context: &dyn AgentPromptContext) -> Result<(), AppError> {
        let window = context.window()?;
        self.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).map_err(|_|error("Native agent already has an active prompt"))?;
        self.tx.send(Command::Prompt { id, text, window }).map_err(|e| { self.busy.store(false, Ordering::SeqCst); error(e) })
    }
    async fn cancel_current_prompt(&self) -> Result<(), AppError> { self.request(json!({"type":"cancel"})).await.map(|_| ()) }
    async fn set_config_option(&self, key: String, value: String) -> Result<(), AppError> { self.request(json!({"type":"config", "key":key,"value":value})).await.map(|_| ()) }
}

struct BridgeFiles(PathBuf);
impl Drop for BridgeFiles { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
async fn bridge_files() -> Result<BridgeFiles, String> {
    let dir = std::env::temp_dir().join(format!("aipp-native-agents-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir(&dir).await.map_err(|e| e.to_string())?;
    let files = BridgeFiles(dir);
    for (name, content) in [
        ("bridge.mjs", include_str!("native_agents/bridge.mjs")),
        ("common.mjs", include_str!("native_agents/common.mjs")),
        ("pi.mjs", include_str!("native_agents/pi.mjs")),
        ("pi-extension.mjs", include_str!("native_agents/pi-extension.mjs")),
        ("cursor.mjs", include_str!("native_agents/cursor.mjs")),
        ("opencode.mjs", include_str!("native_agents/opencode.mjs")),
        ("acp-native.mjs", include_str!("native_agents/acp-native.mjs")),
        ("auth-browser.mjs", include_str!("native_agents/auth-browser.mjs")),
        ("tools.mjs", include_str!("native_agents/tools.mjs")),
    ] { tokio::fs::write(files.0.join(name), content).await.map_err(|e| e.to_string())?; }
    Ok(files)
}

struct ActiveTurn { id: i64, text: String, window: tauri::Window, activities: HashMap<String, Value>, _mcp_prompt: Option<crate::acp_mcp_bridge::provenance::NativeMcpPrompt>, questions: Vec<tauri::async_runtime::JoinHandle<()>> }
impl Drop for ActiveTurn { fn drop(&mut self) { for task in &self.questions { task.abort(); } } }

async fn run_actor(app: tauri::AppHandle, cid: i64, run: String, mut prepared: Prepared, session: Option<String>,
    mut commands: mpsc::UnboundedReceiver<Command>, tx: mpsc::UnboundedSender<Command>, busy: Arc<AtomicBool>, snapshot: Arc<Mutex<Value>>, ready: oneshot::Sender<Result<(), String>>) {
    let driver = prepared.driver;
    let mut identity = AgentEventIdentity::new(driver.0, cid).with_instance(prepared.instance.provider_id).with_run(&run);
    let diagnostics = AgentDiagnosticBuffer::for_environment(&prepared.config.env);
    let mut ready = Some(ready);
    let mut active: Option<ActiveTurn> = None;
    let mut process = None;
    let mut stderr_task = None;
    // Keep bridge-owned unpack directories alive until after process-tree
    // termination; Windows cannot remove a running harness's files.
    let bridge = bridge_files().await;
    let result: Result<(), String> = async {
        let files = bridge.as_ref().map_err(Clone::clone)?;
        prepared.config.wire["temp_root"] = json!(files.0);
        let mut command = tokio::process::Command::new(&prepared.config.node);
        command.arg(files.0.join("bridge.mjs")).current_dir(&prepared.config.cwd).envs(&prepared.config.env)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)] { command.creation_flags(0x08000000); }
        #[cfg(unix)] { command.process_group(0); }
        process = Some(command.spawn().map_err(|e| format!("{} Node bridge spawn failed: {e}", driver.0))?);
        let child = process.as_mut().unwrap();
        let mut stdin = child.stdin.take().ok_or("Native bridge stdin missing")?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("Native bridge stdout missing")?).lines();
        let mut stderr = BufReader::new(child.stderr.take().ok_or("Native bridge stderr missing")?).lines();
        let buffer = diagnostics.clone();
        stderr_task = Some(tokio::spawn(async move { while let Ok(Some(line)) = stderr.next_line().await { buffer.push(line); } }));
        prepared.config.wire["session"] = json!(session);
        write_frame(&mut stdin, &json!({"type":"init","id":"init","driver":driver.0,"config":prepared.config.wire})).await?;
        let init = timeout(Duration::from_secs(55), lines.next_line()).await.map_err(|_| "Native initialization response timed out")?
            .map_err(|e| e.to_string())?.ok_or("Native bridge closed during initialization")?;
        let frame: Value = serde_json::from_str(&init).map_err(|e| format!("Invalid native initialization frame: {e}"))?;
        if frame["type"] != "response" || frame["id"] != "init" || frame["ok"] != true {
            return Err(format!("{} native initialization/resume failed: {}", driver.0, frame["error"]));
        }
        let native_session = frame["data"]["session"].as_str().filter(|s| !s.is_empty()).ok_or("Native bridge returned no session identity")?.to_string();
        identity = identity.clone().with_session(&native_session);
        snapshot.lock().unwrap()["session_id"] = json!(native_session);
        if let Some(options) = frame["data"]["config_options"].as_array() {
            snapshot.lock().unwrap()["config_options"] = json!(options);
        }
        if ready.take().unwrap().send(Ok(())).is_err() { return Err("Session acquisition caller disappeared".into()); }
        let mut requests: HashMap<String, (Value, oneshot::Sender<Result<Value, String>>)> = HashMap::new();
        let mut next_id = 0u64;
        let mut cancel_deadline = None;
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break; };
                    let mut wire = match command {
                        Command::Shutdown => break,
                        Command::Prompt { id, text, window } => {
                            if active.is_some() { return Err("Native session received overlapping prompts".into()); }
                            identity.response_message_id = Some(id);
                            active = Some(ActiveTurn { id, text: String::new(), window, activities: HashMap::new(), _mcp_prompt: None, questions: Vec::new() });
                            // Shared acquisition has bound this run before Prompt is queued.
                            super::persistence::persist_session_id(&app, &identity, Some(&run), &native_session)?;
                            let mcp_prompt = prepared.mcp_scope.as_ref().map(|scope|scope.begin_prompt(id)).transpose()?;
                            active.as_mut().unwrap()._mcp_prompt = mcp_prompt;
                            snapshot.lock().unwrap()["is_busy"] = json!(true);
                            publish_snapshot(&app, &identity, driver, &snapshot)?;
                            json!({"type":"prompt","text":text,"turn":id.to_string()})
                        }
                        Command::Request { mut wire, reply } => {
                            if wire["type"] == "cancel" {
                                if let Some(turn) = active.as_mut() { for task in turn.questions.drain(..) { task.abort(); } }
                                if active.is_some() { cancel_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(20)); }
                                if let Some(scope) = &prepared.mcp_scope { scope.cancel_prompt(); }
                                super::approval::cancel_app_requests_for_scope(&app, &super::approval::AgentRequestScope::native(driver.0, Some(prepared.instance.provider_id), cid, &run)).await;
                            }
                            if wire["type"] == "close" {
                                if active.is_some() { let _ = reply.send(Err("Cannot replace an active native agent session".into())); continue; }
                                let _ = write_frame(&mut stdin, &json!({"type":"close"})).await;
                                let _ = reply.send(Ok(Value::Null)); break;
                            }
                            next_id += 1;
                            wire["id"] = json!(next_id.to_string());
                            requests.insert(next_id.to_string(), (wire.clone(), reply));
                            wire
                        }
                        Command::Reply { turn, request_id, value } => {
                            if active.as_ref().map(|t|t.id) != Some(turn) { continue; }
                            json!({"type":"reply", "request_id":request_id,"value":value})
                        },
                        Command::Answer { turn, request_id, answers } => {
                            if active.as_ref().map(|t|t.id) != Some(turn) { continue; }
                            json!({"type":"answer", "request_id":request_id,"answers":answers})
                        },
                        Command::InteractionFailed { turn, error } => {
                            if active.as_ref().map(|t|t.id) != Some(turn) { continue; }
                            return Err(error);
                        }
                    };
                    if wire.get("id").is_none() { next_id += 1; wire["id"] = json!(next_id.to_string()); }
                    write_frame(&mut stdin, &wire).await?;
                }
                line = lines.next_line() => {
                    let line = line.map_err(|e| e.to_string())?.ok_or("Native bridge stdout closed unexpectedly")?;
                    let frame: Value = serde_json::from_str(&line).map_err(|e| format!("Invalid native event JSON: {e}"))?;
                    if frame["type"] == "fatal" { return Err(frame["error"].as_str().unwrap_or("Native fatal frame omitted error detail").into()); }
                    if frame["type"] == "config_options" {
                        let options = frame["options"].as_array().ok_or("Native configuration update omitted options")?;
                        snapshot.lock().unwrap()["config_options"] = json!(options);
                        publish_snapshot(&app, &identity, driver, &snapshot)?;
                        continue;
                    }
                    if frame["type"] == "response" {
                        let result = if frame["ok"] == true { Ok(frame["data"].clone()) } else { Err(frame["error"].as_str().unwrap_or("Native response omitted error detail").to_string()) };
                        if let Some((wire, reply)) = frame["id"].as_str().and_then(|id| requests.remove(id)) {
                            if result.is_ok() && wire["type"] == "config" {
                                let key = wire["key"].as_str().unwrap_or_default();
                                let value = wire["value"].clone();
                                let mut state = snapshot.lock().unwrap();
                                let options = state["config_options"].as_array_mut().unwrap();
                                if let Some(option) = options.iter_mut().find(|option|option["id"] == key) { option["current_value"] = value; }
                                else { options.push(json!({"id":key,"name":key,"category":key,"current_value":value,"options":[]})); }
                                drop(state);
                                publish_snapshot(&app, &identity, driver, &snapshot)?;
                            }
                            let failure = result.as_ref().err().cloned();
                            let _ = reply.send(result);
                            if let Some(detail) = failure { return Err(detail); }
                        }
                        else { result?; }
                        continue;
                    }
                    let Some(turn) = active.as_mut() else { continue; };
                    if frame["turn"].as_str() != Some(turn.id.to_string().as_str()) { continue; }
                    match frame["type"].as_str() {
                        Some("text") => { turn.text.push_str(frame["text"].as_str().ok_or("Native text frame omitted text")?); publish_text(&identity, turn, false); }
                        Some("activity") => {
                            let id = frame["id"].as_str().ok_or("Native activity omitted ID")?.to_string();
                            let mut activity = merge_activity(turn.activities.get(&id), &frame, turn.text.chars().count(), turn.activities.len() as u64);
                            activity["conversation_id"] = json!(cid);
                            activity["response_message_id"] = json!(turn.id);
                            activity["item_id"] = json!(format!("{}:{id}", turn.id));
                            activity["agent_kind"] = json!(driver.0);
                            activity["session_id"] = json!(native_session);
                            turn.activities.insert(id, activity.clone());
                            let _ = emit_to_window(&turn.window, identity.clone(), ConversationEvent { r#type:"agent_activity".into(), data:activity });
                        }
                        Some("permission") => {
                            if cancel_deadline.is_some() {
                                let reject = frame["options"].as_array().and_then(|items|items.iter().find(|item|item["kind"] == "reject_once")).ok_or("Late permission omitted rejection choice")?;
                                write_frame(&mut stdin, &json!({"type":"reply","request_id":frame["id"],"value":reject["id"]})).await?;
                                continue;
                            }
                            let review = prepared.config.review.then(|| (driver.metadata().display_name, prepared.config.cwd.display().to_string()));
                            queue_permission(&app, &identity, turn.id, &frame, tx.clone(), review).await?;
                        }
                        Some("question") => {
                            if cancel_deadline.is_some() {
                                write_frame(&mut stdin, &json!({"type":"answer","request_id":frame["id"],"answers":null})).await?;
                                continue;
                            }
                            turn.questions.push(queue_question(&app, &identity, turn.id, &frame, tx.clone())?);
                        }
                        Some("done") => {
                            let failure = frame["error"].as_str();
                            let cancelled = frame["cancelled"] == true || cancel_deadline.is_some();
                            for activity in turn.activities.values_mut() {
                                if settle_activity(activity, cancelled, failure) {
                                    let _ = emit_to_window(&turn.window, identity.clone(), ConversationEvent { r#type:"agent_activity".into(), data:activity.clone() });
                                }
                            }
                            persist_success(&app, turn)?;
                            if let Some(detail) = failure { return Err(detail.into()); }
                            publish_text(&identity, turn, true);
                            let _ = emit_to_window(&turn.window, identity.clone(), ConversationEvent { r#type:"stream_complete".into(), data:json!({"conversation_id":cid,"response_message_id":turn.id,"has_response":!turn.text.is_empty()}) });
                            if !cancelled {
                                use crate::api::ai::agent_completion::{handle_agent_success, AgentKind};
                                let kind = match driver.0 { "pi" => AgentKind::Pi, "cursor" => AgentKind::Cursor, "kiro" => AgentKind::Kiro, "antigravity" => AgentKind::Antigravity, _ => AgentKind::OpenCode };
                                handle_agent_success(&app, &turn.window, cid, &turn.text, turn.id, kind).await;
                            }
                            if let Some(manager) = app.try_state::<crate::state::activity_state::ConversationActivityManager>() { manager.clear_agent_focus(&app, &identity, false).await; }
                            active = None;
                            busy.store(false, Ordering::SeqCst);
                            cancel_deadline = None;
                            super::approval::cancel_app_requests_for_scope(&app, &super::approval::AgentRequestScope::native(driver.0, Some(prepared.instance.provider_id), cid, &run)).await;
                            snapshot.lock().unwrap()["is_busy"] = json!(false);
                            publish_snapshot(&app, &identity, driver, &snapshot)?;
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(900)), if active.is_none() => { let _ = write_frame(&mut stdin, &json!({"type":"close"})).await; break; }
                _ = tokio::time::sleep_until(cancel_deadline.unwrap_or_else(||tokio::time::Instant::now() + Duration::from_secs(86400))), if cancel_deadline.is_some() => {
                    return Err("Native cancel did not reach a terminal event within 20 seconds; terminating its process tree".into());
                }
            }
        }
        Ok(())
    }.await;
    if let Some(child) = process.as_mut() { terminate_tree(child).await; }
    if let Some(task) = stderr_task { let _ = timeout(Duration::from_secs(2), task).await; }
    if let Err(detail) = result {
        let detail = diagnostics.sanitize(&detail);
        if let Some(ready) = ready { let _ = ready.send(Err(format!("{detail} [driver={}, conversation_id={cid}, run_id={run}]; {:?}", driver.0, diagnostics.diagnostics(None, None)))); }
        if let Some(mut turn) = active {
            for activity in turn.activities.values_mut() {
                if settle_activity(activity, false, Some(&detail)) {
                    let _ = emit_to_window(&turn.window, identity.clone(), ConversationEvent { r#type:"agent_activity".into(), data:activity.clone() });
                }
            }
            let _ = persist_success(&app, &turn);
            let failure = AgentFailure::new(identity.clone().with_response_message(turn.id), AgentFailureStage::NativeSession, detail)
                .with_process_diagnostics(diagnostics.diagnostics(process.as_mut().and_then(|p|p.try_wait().ok().flatten()).map(|s|s.to_string()), None));
            let _ = emit_normalized_failure_to_window(&app, &turn.window, failure, driver.metadata().display_name);
            if let Some(manager) = app.try_state::<crate::state::activity_state::ConversationActivityManager>() { manager.clear_agent_focus(&app, &identity, false).await; }
        }
    }
    super::approval::cancel_app_requests_for_scope(&app, &super::approval::AgentRequestScope::native(driver.0, Some(prepared.instance.provider_id), cid, &run)).await;
    busy.store(false, Ordering::SeqCst);
    let _ = super::events::emit_to_chat_windows(&app, identity, ConversationEvent { r#type: "acp_session_state_snapshot".into(), data:json!({"state":null}) });
}

fn publish_snapshot(app: &tauri::AppHandle, identity: &AgentEventIdentity, driver: NativeDriver, snapshot: &Mutex<Value>) -> Result<(), String> {
    let state = snapshot.lock().map_err(|_|"Native snapshot lock poisoned")?.clone();
    let payload = super::capabilities::snapshot_event_payload(driver.0, driver.declaration(), Some(&state)).map_err(|e|e.to_string())?;
    super::events::emit_to_chat_windows(app, identity.clone(), ConversationEvent { r#type:"acp_session_state_snapshot".into(), data:payload })
}

async fn terminate_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)] {
            let mut kill = tokio::process::Command::new("taskkill.exe");
            kill.args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(0x08000000);
            let _ = timeout(Duration::from_secs(5), kill.output()).await;
        }
        #[cfg(unix)] {
            let _ = tokio::process::Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).output().await;
        }
    }
    let _ = child.kill().await;
}

fn parse_environment(value: &str) -> Result<HashMap<String, String>, AppError> {
    if value.trim_start().starts_with('{') { return serde_json::from_str(value).map_err(|e|error(format!("Invalid agent environment JSON: {e}"))); }
    let mut env = HashMap::new();
    for line in value.lines().map(str::trim).filter(|line|!line.is_empty() && !line.starts_with('#')) {
        let (key, value) = line.split_once('=').filter(|(key,_)|!key.trim().is_empty()).ok_or_else(||error("Agent environment lines must use KEY=VALUE"))?;
        env.insert(key.trim().into(), value.trim().into());
    }
    Ok(env)
}

async fn write_frame(stdin: &mut tokio::process::ChildStdin, frame: &Value) -> Result<(), String> {
    stdin.write_all(format!("{frame}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

pub(crate) async fn probe_models(driver: &str, mut config: NativeConfig) -> Result<Vec<(String, String)>, String> {
    if !is_native_agent(driver) { return Err(format!("Unknown native agent driver: {driver}")); }
    let files = bridge_files().await?;
    config.wire["temp_root"] = json!(files.0);
    let mut command = tokio::process::Command::new(&config.node);
    command.arg(files.0.join("bridge.mjs")).current_dir(&config.cwd).envs(&config.env)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)] { command.creation_flags(0x08000000); }
    #[cfg(unix)] { command.process_group(0); }
    let mut child = command.spawn().map_err(|e|format!("{driver} model discovery spawn failed: {e}"))?;
    let diagnostics = AgentDiagnosticBuffer::for_environment(&config.env);
    let buffer = diagnostics.clone();
    let mut stderr = BufReader::new(child.stderr.take().ok_or("Probe stderr missing")?).lines();
    let stderr_task = tokio::spawn(async move { while let Ok(Some(line)) = stderr.next_line().await { buffer.push(line); } });
    let result = timeout(Duration::from_secs(55), async {
        let mut stdin = child.stdin.take().ok_or("Probe stdin missing")?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("Probe stdout missing")?).lines();
        write_frame(&mut stdin, &json!({"type":"probe","id":"probe","driver":driver,"config":config.wire})).await?;
        loop {
            let line = lines.next_line().await.map_err(|e|e.to_string())?.ok_or("Probe stdout closed")?;
            let frame: Value = serde_json::from_str(&line).map_err(|e|format!("Invalid model discovery JSON: {e}"))?;
            if frame["type"] == "fatal" { return Err(frame["error"].to_string()); }
            if frame["id"] != "probe" { continue; }
            if frame["ok"] != true { return Err(frame["error"].to_string()); }
            return frame["data"].as_array().ok_or_else(||"Native models response is not an array".to_string())?.iter().map(|model| {
                let id = model["id"].as_str().filter(|s|!s.is_empty()).ok_or("Native model omitted ID")?;
                Ok((id.to_string(), model["name"].as_str().unwrap_or(id).to_string()))
            }).collect::<Result<Vec<_>, String>>();
        }
    }).await.unwrap_or_else(|_|Err(format!("{driver} model discovery timed out")));
    terminate_tree(&mut child).await;
    let _ = timeout(Duration::from_secs(2), stderr_task).await;
    result.map_err(|e|format!("{}; diagnostics: {:?}", diagnostics.sanitize(&e), diagnostics.diagnostics(child.try_wait().ok().flatten().map(|s|s.to_string()), None)))
}
fn publish_text(identity: &AgentEventIdentity, turn: &ActiveTurn, done: bool) {
    let _ = emit_to_window(&turn.window, identity.clone(), ConversationEvent { r#type:"message_update".into(),
        data: json!({"message_id":turn.id,"message_type":"response","content":turn.text,"is_done":done}) });
}
fn merge_activity(previous: Option<&Value>, frame: &Value, offset: usize, order: u64) -> Value {
    let mut activity = previous.cloned().unwrap_or_else(||json!({"item_id":frame["id"],"content_offset":offset,"order":order,"started_at":chrono::Utc::now(),"metadata":{}}));
    activity["sequence"] = json!(previous.and_then(|v|v["sequence"].as_u64()).unwrap_or(0) + 1);
    for key in ["kind", "title", "input", "error"] {
        if let Some(value) = frame.get(key) { activity[key] = value.clone(); }
    }
    if let Some(metadata) = frame.get("metadata").and_then(Value::as_object) {
        if !activity["metadata"].is_object() { activity["metadata"] = json!({}); }
        for (key, value) in metadata { activity["metadata"][key] = value.clone(); }
    }
    activity["status"] = if frame["status"] == "running" { json!("executing") } else { frame["status"].clone() };
    if matches!(activity["status"].as_str(), Some("success" | "failed" | "cancelled")) && activity.get("finished_at").is_none() {
        activity["finished_at"] = json!(chrono::Utc::now());
    }
    if frame["append"] == true {
        activity["output"] = json!(format!("{}{}", previous.and_then(|p|p["output"].as_str()).unwrap_or_default(), frame["output"].as_str().unwrap_or_default()));
    } else if let Some(output) = frame.get("output") { activity["output"] = output.clone(); }
    activity
}
fn settle_activity(activity: &mut Value, cancelled: bool, failure: Option<&str>) -> bool {
    if activity["status"] != "executing" && activity["status"] != "pending" { return false; }
    activity["sequence"] = json!(activity["sequence"].as_u64().unwrap_or(0) + 1);
    activity["finished_at"] = json!(chrono::Utc::now());
    let unfinished_tool = matches!(activity["kind"].as_str(), Some("tool" | "command" | "patch" | "file_read" | "file_search" | "file_write"));
    activity["status"] = json!(if failure.is_some() { "failed" } else if cancelled || unfinished_tool { "cancelled" } else { "success" });
    if let Some(detail) = failure { activity["error"] = json!(detail); }
    else if unfinished_tool { activity["error"] = json!("智能体已结束本轮，但未返回工具终态，执行结果尚未确认"); }
    true
}
fn persist_success(app: &tauri::AppHandle, turn: &ActiveTurn) -> Result<(), String> {
    let db = ConversationDatabase::new(app).map_err(|e|e.to_string())?;
    let repo = db.message_repo().map_err(|e|e.to_string())?;
    let mut message = repo.read(turn.id).map_err(|e|e.to_string())?.ok_or("Native response message disappeared")?;
    message.content = turn.text.clone(); message.finish_time = Some(chrono::Utc::now());
    let mut metadata: Value = message.metadata_json.as_deref().and_then(|raw|serde_json::from_str(raw).ok()).filter(Value::is_object).unwrap_or_else(||json!({}));
    metadata["agent_activities"] = json!(ordered_activities(&turn.activities));
    message.metadata_json = Some(metadata.to_string());
    repo.update(&message).map_err(|e|e.to_string())?;
    Ok(())
}

fn ordered_activities(activities: &HashMap<String, Value>) -> Vec<&Value> {
    let mut ordered = activities.values().collect::<Vec<_>>();
    ordered.sort_by_key(|activity| activity["order"].as_u64().unwrap_or(u64::MAX));
    ordered
}

/// Pi passes its tool input as a JSON string; show and review the object itself, not escaped text.
pub(crate) fn permission_parameters(input: &Value) -> String {
    match input {
        Value::String(text) => serde_json::from_str::<Value>(text).map(|value| value.to_string()).unwrap_or_else(|_| text.clone()),
        other => other.to_string(),
    }
}

/// After AIPP's review: answer with the advertised one-time allow, or ask the user with the reason.
/// Never picks a persistent `allow_always` option on the user's behalf.
#[derive(Debug, PartialEq)]
pub(crate) enum ReviewOutcome { Allow(String), Ask(String) }
pub(crate) fn review_outcome(options: &[crate::api::ai::acp::AcpPermissionOptionPayload], verdict: &str, reason: &str) -> ReviewOutcome {
    match verdict {
        "safe" => match options.iter().find(|option| option.kind == "allow_once") {
            Some(option) => ReviewOutcome::Allow(option.option_id.clone()),
            None => ReviewOutcome::Ask(format!("自动审核判定安全（{reason}），但智能体没有提供「本次允许」选项，需要人工确认")),
        },
        "risky" => ReviewOutcome::Ask(format!("自动审核认为需要人工确认：{reason}")),
        _ => ReviewOutcome::Ask(format!("自动审核未完成，需要人工确认：{reason}")),
    }
}

/// Review first; the card (and its notification) only appears when the review does not clear the request.
async fn review_permission(app: tauri::AppHandle, event: crate::api::ai::acp::AcpPermissionRequestEvent,
    scope: super::approval::AgentRequestScope, agent: &'static str, working_directory: String) {
    use crate::api::ai::acp::AcpPermissionState;
    use super::approval::{AgentInteractionReply, AgentPermissionReply, AgentReplyTarget};
    let conversation_id = event.conversation_id.unwrap_or_default();
    let draft = crate::mcp::tool_review::review_agent_permission(&app, conversation_id, agent,
        event.title.as_deref().unwrap_or("未命名操作"), event.parameters.as_deref().unwrap_or_default(), &working_directory).await;
    let state = app.state::<AcpPermissionState>();
    // Answered, cancelled or the turn ended while the model was reviewing.
    if state.get_request(&event.request_id).await.is_none() { return; }
    let reason = match review_outcome(&event.options, &draft.verdict, &draft.reason) {
        ReviewOutcome::Allow(option_id) => {
            let target = AgentReplyTarget { request_id: event.request_id.clone(), scope };
            match crate::api::operation_api::resolve_agent_interaction_response(&app, target,
                AgentInteractionReply::Permission(AgentPermissionReply::Select { option_id })).await {
                Ok(_) => {
                    tracing::info!(conversation_id, request_id = %event.request_id, agent, reason = %draft.reason, "Native agent permission approved by AIPP auto-review");
                    return;
                }
                Err(error) => format!("自动审核判定安全，但自动放行失败，需要人工确认：{error}"),
            }
        }
        ReviewOutcome::Ask(reason) => reason,
    };
    if state.get_request(&event.request_id).await.is_none() { return; }
    let mut payload = serde_json::to_value(&event).unwrap_or_else(|_| json!({}));
    payload["review"] = json!({ "verdict": draft.verdict, "reason": reason });
    if let Err(error) = crate::api::operation_api::emit_permission_request_event(&app, crate::api::operation_api::ACP_PERMISSION_REQUEST_EVENT, Some(conversation_id), &payload) {
        tracing::warn!(conversation_id, request_id = %event.request_id, error = %error, "Failed to show reviewed native agent permission");
    }
}

async fn queue_permission(app: &tauri::AppHandle, identity: &AgentEventIdentity, turn: i64, frame: &Value, tx: mpsc::UnboundedSender<Command>,
    review: Option<(&'static str, String)>) -> Result<(), String> {
    use crate::api::ai::acp::{AcpPermissionState, AcpPermissionRequestEvent, AcpPermissionOptionPayload, AcpPermissionDecision};
    let native_id = frame["id"].as_str().ok_or("Native permission request omitted ID")?.to_string();
    let request_id = format!("{}:{}:{}:{turn}:{native_id}", identity.driver_id, identity.conversation_id, identity.run_id.as_deref().unwrap_or_default());
    let options: Vec<AcpPermissionOptionPayload> = frame["options"].as_array().ok_or("Native permission request omitted options")?.iter().map(|option| {
        Ok(AcpPermissionOptionPayload {
            option_id: option["id"].as_str().ok_or("Permission option omitted ID")?.into(),
            name: option["name"].as_str().ok_or("Permission option omitted label")?.into(),
            kind: option["kind"].as_str().ok_or("Permission option omitted kind")?.into(),
        })
    }).collect::<Result<_, String>>()?;
    let rejected = options.iter().find(|o|o.kind == "reject_once").ok_or("Permission request omitted rejection choice")?.option_id.clone();
    let event = AcpPermissionRequestEvent { request_id, conversation_id: Some(identity.conversation_id), agent_kind: Some(identity.driver_id.clone()),
        tool_call_id: native_id.clone(), title: frame["title"].as_str().map(str::to_string), kind: Some("native_agent_permission".into()),
        parameters: Some(permission_parameters(&frame["input"])), options };
    let guard = crate::mcp::builtin_mcp::superadmin::approval::NativeApprovalGuard::capture(app, identity.conversation_id, Some(turn), "native_agent.permission", json!(event))?;
    let scope = super::approval::AgentRequestScope::native(&identity.driver_id, identity.instance_id.as_deref().and_then(|id|id.parse().ok()), identity.conversation_id, identity.run_id.as_deref().unwrap_or_default());
    let (sender, receiver) = oneshot::channel();
    app.state::<AcpPermissionState>().store_request_guarded(event.clone(), sender, scope.clone(), guard.clone()).await?;
    match review {
        Some((agent, working_directory)) => { tauri::async_runtime::spawn(review_permission(app.clone(), event, scope, agent, working_directory)); }
        None => crate::api::operation_api::emit_permission_request_event(app, crate::api::operation_api::ACP_PERMISSION_REQUEST_EVENT, Some(identity.conversation_id), &event)?,
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut value = match receiver.await { Ok(AcpPermissionDecision::Selected(option)) => option, _ => rejected.clone() };
        if value != rejected && guard.as_ref().is_some_and(|guard|guard.validate(&app).is_err()) { value = rejected; }
        let _ = tx.send(Command::Reply { turn, request_id: native_id, value });
    });
    Ok(())
}

fn queue_question(app: &tauri::AppHandle, identity: &AgentEventIdentity, turn: i64, frame: &Value, tx: mpsc::UnboundedSender<Command>) -> Result<tauri::async_runtime::JoinHandle<()>, String> {
    use crate::mcp::builtin_mcp::interaction::{AskUserQuestionRequest, AskUserQuestionMetadata, InteractionState, request_ask_user_question_scoped};
    let request_id = frame["id"].as_str().ok_or("Native question omitted ID")?.to_string();
    let request = AskUserQuestionRequest { questions: serde_json::from_value(frame["questions"].clone()).map_err(|e|format!("Native question schema: {e}"))?,
        answers: None, metadata: Some(AskUserQuestionMetadata { source: Some(identity.driver_id.clone()) }) };
    request.validate()?;
    let app = app.clone();
    let cid = identity.conversation_id;
    let scope = super::approval::AgentRequestScope::native(&identity.driver_id, identity.instance_id.as_deref().and_then(|id|id.parse().ok()), cid, identity.run_id.as_deref().unwrap_or_default());
    Ok(tauri::async_runtime::spawn(async move {
        let result = crate::api::butler_peer::TOOL_MESSAGE.scope(Some(turn), request_ask_user_question_scoped(&app, &app.state::<InteractionState>(), Some(cid), request, Some(scope))).await;
        let command = match result {
            Ok(answers) => Command::Answer { turn, request_id, answers: json!(answers) },
            Err(error) if error == "User cancelled AskUserQuestion" => Command::Answer { turn, request_id, answers: Value::Null },
            Err(error) => Command::InteractionFailed { turn, error },
        };
        let _ = tx.send(command);
    }))
}

#[cfg(test)]
#[path = "native_agents_tests.rs"]
mod tests;

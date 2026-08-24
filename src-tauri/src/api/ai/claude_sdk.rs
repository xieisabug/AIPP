use crate::api::ai::acp::resolve_acp_cli_path;
use crate::api::ai::acp::{
    AcpPermissionDecision, AcpPermissionOptionPayload, AcpPermissionRequestEvent,
    AcpPermissionState, AcpSessionConfigChoicePayload, AcpSessionConfigOptionPayload,
};
use crate::api::ai::codex_app_server::AgentActivityEvent;
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::acp_mcp_bridge::{
    ensure_proxy_server, ACP_MCP_BRIDGE_ARG, ACP_MCP_CONVERSATION_ID_ENV,
    ACP_MCP_DB_PATH_ENV, ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV, ACP_MCP_PROXY_ADDR_ENV,
    ACP_MCP_PROXY_TOKEN_ENV, ACP_MCP_SELECTED_TOOLS_ENV,
};
use crate::api::operation_api::{emit_permission_request_event, ACP_PERMISSION_REQUEST_EVENT};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use crate::state::activity_state::ConversationActivityManager;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tauri::{Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, oneshot},
};

pub const CLAUDE_SDK_API_TYPE: &str = "claude_sdk";

#[derive(Debug)]
struct ClaudeMcpConfigFile {
    path: PathBuf,
}

impl ClaudeMcpConfigFile {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ClaudeMcpConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn is_claude_code_provider(
    provider_api_type: &str,
    provider_configs: &[crate::db::llm_db::LLMProviderConfig],
) -> bool {
    provider_api_type.eq_ignore_ascii_case(CLAUDE_SDK_API_TYPE)
        || provider_api_type.eq_ignore_ascii_case("anthropic")
        || (provider_api_type.eq_ignore_ascii_case("acp")
            && provider_configs.iter().any(|config| {
                matches!(config.name.as_str(), "acp_cli_command" | "claude_cli_command")
                    && config.value.to_ascii_lowercase().contains("claude")
            }))
}

pub fn claude_model_choices(
    models: &[(i64, String, i64, String, String, bool, bool, bool)],
) -> Vec<AcpSessionConfigChoicePayload> {
    models
        .iter()
        .map(|(_, name, _, code, description, _, _, _)| AcpSessionConfigChoicePayload {
            value: code.clone(),
            name: name.clone(),
            description: (!description.trim().is_empty()).then(|| description.clone()),
            group_name: None,
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ClaudeSdkConfig {
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub effort: Option<String>,
    pub model_choices: Vec<AcpSessionConfigChoicePayload>,
    pub selected_mcp_tools_payload: String,
    mcp_config_file: Option<Arc<ClaudeMcpConfigFile>>,
    pub session_signature: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ClaudeConversationSessionState {
    pub conversation_id: i64,
    pub agent_kind: String,
    pub session_id: Option<String>,
    pub has_active_prompt: bool,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub load_session_supported: bool,
    pub session_resume_supported: bool,
    pub restored_session_method: Option<String>,
    pub config_options: Vec<AcpSessionConfigOptionPayload>,
}

enum SessionCommand {
    Prompt { message_id: i64, prompt: String, window: tauri::Window },
    Cancel { response: oneshot::Sender<Result<(), String>> },
    SetConfig { config_id: String, value: String, response: oneshot::Sender<Result<(), String>> },
}

#[derive(Clone)]
pub struct ClaudeSessionHandle {
    sender: mpsc::UnboundedSender<SessionCommand>,
    run_id: String,
}

pub struct ClaudeSessionEntry {
    pub handle: ClaudeSessionHandle,
    pub snapshot: ClaudeConversationSessionState,
    pub config_signature: String,
    pub run_id: String,
}

impl ClaudeSessionEntry {
    pub fn new(
        handle: ClaudeSessionHandle,
        conversation_id: i64,
        config_signature: String,
        model: Option<String>,
        permission_mode: Option<String>,
        effort: Option<String>,
    ) -> Self {
        Self {
            run_id: handle.run_id.clone(),
            handle,
            config_signature,
            snapshot: ClaudeConversationSessionState {
                conversation_id,
                agent_kind: CLAUDE_SDK_API_TYPE.into(),
                model: model.clone(),
                permission_mode,
                config_options: vec![
                    AcpSessionConfigOptionPayload {
                        id: "model".into(), name: "模型".into(), description: Some("下一轮 Claude Code 响应使用的模型".into()), category: Some("model".into()),
                        current_value: model.clone().unwrap_or_default(), options: Vec::new(),
                    },
                    AcpSessionConfigOptionPayload {
                        id: "effort".into(), name: "思考强度".into(), description: Some("下一轮 Claude Code 响应的思考强度".into()), category: Some("thought_level".into()),
                        current_value: effort.unwrap_or_else(|| "default".into()), options: ["default", "low", "medium", "high", "max"].into_iter().map(|value| AcpSessionConfigChoicePayload { value: value.into(), name: value.into(), description: None, group_name: None }).collect(),
                    },
                ],
                ..Default::default()
            },
        }
    }
}

impl ClaudeSessionHandle {
    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        self.sender
            .send(SessionCommand::Prompt { message_id, prompt, window })
            .map_err(|_| AppError::UnknownError("Claude Code session closed".into()))
    }
    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(SessionCommand::Cancel { response: tx })
            .map_err(|_| AppError::UnknownError("Claude Code session closed".into()))?;
        rx.await
            .map_err(|_| AppError::UnknownError("Claude Code session closed".into()))?
            .map_err(AppError::UnknownError)
    }
    pub async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(SessionCommand::SetConfig { config_id, value, response: tx }).map_err(|_| AppError::UnknownError("Claude Code session closed".into()))?;
        rx.await.map_err(|_| AppError::UnknownError("Claude Code session closed".into()))?.map_err(AppError::UnknownError)
    }
}

fn provider_value(configs: &[crate::db::llm_db::LLMProviderConfig], name: &str) -> Option<String> {
    configs
        .iter()
        .find(|c| c.name == name)
        .map(|c| c.value.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn model_value(
    configs: &[crate::db::assistant_db::AssistantModelConfig],
    name: &str,
) -> Option<String> {
    configs
        .iter()
        .find(|c| c.name == name)
        .and_then(|c| c.value.as_deref())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

pub fn extract_claude_sdk_config(
    models: &[crate::db::assistant_db::AssistantModelConfig],
    providers: &[crate::db::llm_db::LLMProviderConfig],
    model: Option<String>,
    model_choices: Vec<AcpSessionConfigChoicePayload>,
) -> Result<ClaudeSdkConfig, AppError> {
    let mut model_choices = model_choices;
    if let Some(current) = model.as_deref() {
        if !model_choices.iter().any(|choice| choice.value == current) {
            model_choices.insert(0, AcpSessionConfigChoicePayload {
                value: current.to_string(), name: format!("{}（当前模型）", current),
                description: Some("当前助手配置或 Claude CLI 实际使用的模型".into()), group_name: None,
            });
        }
    }
    let cli_command = model_value(models, "claude_cli_command")
        .or_else(|| model_value(models, "acp_cli_command"))
        .or_else(|| provider_value(providers, "claude_cli_command"))
        .or_else(|| provider_value(providers, "acp_cli_command"))
        .unwrap_or_else(|| "claude".into());
    let working_directory = model_value(models, "acp_working_directory")
        .or_else(|| provider_value(providers, "acp_working_directory"))
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
    let permission_mode = model_value(models, "claude_permission_mode")
        .or_else(|| provider_value(providers, "claude_permission_mode"));
    let effort = model_value(models, "claude_effort")
        .or_else(|| provider_value(providers, "claude_effort"))
        .filter(|value| value != "default");
    let mut env_vars = HashMap::new();
    // A regular Anthropic provider stores credentials as API fields, while
    // Claude Code consumes the corresponding environment variables.
    if let Some(api_key) = provider_value(providers, "api_key") {
        env_vars.insert("ANTHROPIC_API_KEY".into(), api_key);
    }
    if let Some(base_url) = provider_value(providers, "endpoint")
        .or_else(|| provider_value(providers, "base_url"))
    {
        env_vars.insert("ANTHROPIC_BASE_URL".into(), base_url);
    }
    for raw in [provider_value(providers, "acp_env_vars"), model_value(models, "acp_env_vars")]
        .into_iter()
        .flatten()
    {
        crate::api::ai::acp::merge_acp_env_blob(&mut env_vars, &raw);
    }
    let session_signature = serde_json::to_string(&json!({"cli":cli_command,"cwd":working_directory,"model":model,"permission_mode":permission_mode,"effort":effort,"env":env_vars,"selected_mcp":""})).unwrap_or_default();
    Ok(ClaudeSdkConfig {
        cli_command,
        working_directory,
        env_vars,
        model,
        permission_mode,
        effort,
        model_choices,
        selected_mcp_tools_payload: String::new(),
        mcp_config_file: None,
        session_signature,
    })
}

pub fn refresh_claude_session_signature(config: &mut ClaudeSdkConfig) {
    config.session_signature = serde_json::to_string(&json!({
        "cli": config.cli_command, "cwd": config.working_directory, "model": config.model,
        "permission_mode": config.permission_mode, "effort": config.effort, "env": config.env_vars,
        "selected_mcp": config.selected_mcp_tools_payload,
    })).unwrap_or_default();
}

fn build_claude_mcp_config(
    bridge_command: &std::path::Path,
    mcp_db_path: &std::path::Path,
    conversation_id: i64,
    proxy: &crate::acp_mcp_bridge::AcpMcpProxyConfig,
    selected_mcp_tools_payload: &str,
) -> Result<String, String> {
    let mut env = serde_json::Map::new();
    env.insert(ACP_MCP_DB_PATH_ENV.into(), json!(mcp_db_path.display().to_string()));
    env.insert(ACP_MCP_CONVERSATION_ID_ENV.into(), json!(conversation_id.to_string()));
    env.insert(ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV.into(), json!("1"));
    env.insert(ACP_MCP_PROXY_ADDR_ENV.into(), json!(proxy.addr));
    env.insert(ACP_MCP_PROXY_TOKEN_ENV.into(), json!(proxy.token));
    env.insert(ACP_MCP_SELECTED_TOOLS_ENV.into(), json!(selected_mcp_tools_payload));
    serde_json::to_string(&json!({
        "mcpServers": {
            "aipp": {
                "type": "stdio",
                "command": bridge_command.display().to_string(),
                "args": [ACP_MCP_BRIDGE_ARG],
                "env": env,
            }
        }
    }))
    .map_err(|error| format!("Failed to serialize Claude Code MCP config: {error}"))
}

fn write_claude_mcp_config_file(content: &str) -> Result<Arc<ClaudeMcpConfigFile>, String> {
    let path = std::env::temp_dir().join(format!(
        "aipp-claude-mcp-{}.json",
        uuid::Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|error| format!("Failed to create Claude Code MCP config file: {error}"))?;
    let config_file = Arc::new(ClaudeMcpConfigFile { path });
    file.write_all(content.as_bytes())
        .map_err(|error| format!("Failed to write Claude Code MCP config file: {error}"))?;
    file.flush()
        .map_err(|error| format!("Failed to flush Claude Code MCP config file: {error}"))?;
    Ok(config_file)
}

pub async fn prepare_claude_mcp_bridge(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    config: &mut ClaudeSdkConfig,
) -> Result<(), String> {
    let payload = config.selected_mcp_tools_payload.trim();
    if payload.is_empty() || payload == "[]" {
        config.mcp_config_file = None;
        return Ok(());
    }

    let bridge_command = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve AIPP executable for Claude MCP bridge: {error}"))?;
    let mcp_db_path = MCPDatabase::db_path(app_handle)?;
    let proxy = ensure_proxy_server(app_handle.clone()).await?;
    let mcp_config = build_claude_mcp_config(
        &bridge_command,
        &mcp_db_path,
        conversation_id,
        &proxy,
        &config.selected_mcp_tools_payload,
    )?;
    config.mcp_config_file = Some(write_claude_mcp_config_file(&mcp_config)?);
    Ok(())
}

fn persist(app: &tauri::AppHandle, id: i64, content: &str, done: bool) {
    if let Ok(db) = ConversationDatabase::new(app) {
        if let Ok(repo) = db.message_repo() {
            if let Ok(Some(mut message)) = repo.read(id) {
                message.content = content.into();
                if done {
                    message.finish_time = Some(chrono::Utc::now());
                }
                let _ = repo.update(&message);
            }
        }
    }
}

fn persist_usage(app: &tauri::AppHandle, id: i64, usage: &Value) {
    let Ok(db) = ConversationDatabase::new(app) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(mut message)) = repo.read(id) else { return };
    let input = usage.get("input_tokens").and_then(Value::as_i64).unwrap_or(0) as i32;
    let output = usage.get("output_tokens").and_then(Value::as_i64).unwrap_or(0) as i32;
    let total =
        usage.get("total_tokens").and_then(Value::as_i64).unwrap_or((input + output) as i64) as i32;
    message.input_token_count = input;
    message.output_token_count = output;
    message.token_count = total;
    let mut metadata = message
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    metadata.insert("usage_source".into(), json!("reported"));
    metadata.insert("claude_usage".into(), usage.clone());
    message.metadata_json = Some(Value::Object(metadata).to_string());
    let _ = repo.update(&message);
}

async fn handle_permission(
    app: &tauri::AppHandle,
    conversation_id: i64,
    frame: &Value,
    stdin: &mut tokio::process::ChildStdin,
) -> Result<(), String> {
    let request_id =
        frame.get("request_id").and_then(Value::as_str).unwrap_or("unknown").to_string();
    let request = frame.get("request").cloned().unwrap_or(Value::Null);
    let tool_name = request.get("tool_name").and_then(Value::as_str).unwrap_or("Claude Code tool");
    let event = AcpPermissionRequestEvent {
        request_id: format!("claude:{conversation_id}:{request_id}"),
        conversation_id: Some(conversation_id),
        agent_kind: Some(CLAUDE_SDK_API_TYPE.into()),
        tool_call_id: request
            .get("tool_use_id")
            .and_then(Value::as_str)
            .unwrap_or(&request_id)
            .into(),
        title: Some(format!("允许 Claude Code 使用 {tool_name}")),
        kind: Some("claude_control_request".into()),
        parameters: Some(
            serde_json::to_string_pretty(&request).unwrap_or_else(|_| request.to_string()),
        ),
        options: vec![
            AcpPermissionOptionPayload {
                option_id: "allow".into(),
                name: "本次允许".into(),
                kind: "allow_once".into(),
            },
            AcpPermissionOptionPayload {
                option_id: "deny".into(),
                name: "拒绝".into(),
                kind: "reject_once".into(),
            },
        ],
    };
    let (tx, rx) = oneshot::channel();
    let state = app.state::<AcpPermissionState>();
    state.store_request(event.clone(), tx).await;
    emit_permission_request_event(
        app,
        ACP_PERMISSION_REQUEST_EVENT,
        Some(conversation_id),
        &event,
    )?;
    let decision = rx.await.unwrap_or(AcpPermissionDecision::Cancelled);
    let (behavior, message) = match decision {
        AcpPermissionDecision::Selected(value) if value == "allow" => ("allow", Value::Null),
        _ => ("deny", json!({"message":"Permission denied by user"})),
    };
    let response = json!({
        "type": "control_response",
        "response": {"subtype":"success", "request_id": request_id, "response": {"behavior":behavior, "updatedInput": request.get("input").cloned().unwrap_or(Value::Null), "message":message}}
    });
    stdin.write_all(format!("{response}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

fn emit_update(window: &tauri::Window, conversation_id: i64, id: i64, content: &str, done: bool) {
    let data = MessageUpdateEvent {
        message_id: id,
        message_type: "response".into(),
        content: content.into(),
        is_done: done,
        token_count: None,
        input_token_count: None,
        output_token_count: None,
        ttft_ms: None,
        tps: None,
    };
    let _ = window.emit(
        &format!("conversation_event_{conversation_id}"),
        ConversationEvent {
            r#type: "message_update".into(),
            data: serde_json::to_value(data).unwrap(),
        },
    );
}

fn emit_snapshot(
    app: &tauri::AppHandle,
    conversation_id: i64,
    state: ClaudeConversationSessionState,
) {
    if let Some(claude_state) = app.try_state::<crate::ClaudeSessionState>() {
        let sessions = claude_state.sessions.clone();
        let state_for_cache = state.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(entry) = sessions.lock().await.get_mut(&conversation_id) {
                entry.snapshot = state_for_cache;
            }
        });
    }
    let _ = crate::utils::window_utils::send_conversation_event_to_chat_windows(
        app,
        conversation_id,
        ConversationEvent {
            r#type: "acp_session_state_snapshot".into(),
            data: json!({"state": state}),
        },
    );
}

async fn cleanup_claude_session(
    app: &tauri::AppHandle,
    conversation_id: i64,
    run_id: &str,
) {
    let removed_current_entry = if let Some(state) = app.try_state::<crate::ClaudeSessionState>() {
        let mut sessions = state.sessions.lock().await;
        if sessions
            .get(&conversation_id)
            .is_some_and(|entry| entry.run_id == run_id)
        {
            sessions.remove(&conversation_id);
            true
        } else {
            false
        }
    } else {
        false
    };
    if removed_current_entry {
        let _ = crate::utils::window_utils::send_conversation_event_to_chat_windows(
            app,
            conversation_id,
            ConversationEvent {
                r#type: "acp_session_state_snapshot".into(),
                data: json!({"state": Value::Null}),
            },
        );
    }
}

pub(crate) fn emit_claude_failure(
    app: &tauri::AppHandle,
    conversation_id: i64,
    message_id: i64,
    window: &tauri::Window,
    error: &str,
) {
    let content = format!("Claude Code 运行失败：{error}");
    if let Ok(db) = ConversationDatabase::new(app) {
        if let Ok(repo) = db.message_repo() {
            if let Ok(Some(mut message)) = repo.read(message_id) {
                message.message_type = "error".to_string();
                message.content = content.clone();
                message.finish_time = Some(chrono::Utc::now());
                let _ = repo.update(&message);
            }
        }
    }
    let _ = window.emit(
        &format!("conversation_event_{conversation_id}"),
        ConversationEvent {
            r#type: "message_update".into(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "error".into(),
                content,
                is_done: true,
                token_count: Some(0),
                input_token_count: Some(0),
                output_token_count: Some(0),
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        },
    );
    let _ = window.emit(
        &format!("conversation_event_{conversation_id}"),
        ConversationEvent {
            r#type: "stream_complete".into(),
            data: json!({
                "conversation_id": conversation_id,
                "response_message_id": message_id,
                "has_response": false,
                "error": error,
            }),
        },
    );
}

fn claude_config_options(config: &ClaudeSdkConfig) -> Vec<AcpSessionConfigOptionPayload> {
    vec![
        AcpSessionConfigOptionPayload {
            id: "model".into(), name: "模型".into(), description: Some("下一轮 Claude Code 响应使用的模型".into()), category: Some("model".into()),
            current_value: config.model.clone().unwrap_or_default(), options: config.model_choices.clone(),
        },
        AcpSessionConfigOptionPayload {
            id: "effort".into(), name: "思考强度".into(), description: Some("下一轮 Claude Code 响应的思考强度".into()), category: Some("thought_level".into()),
            current_value: config.effort.clone().unwrap_or_else(|| "default".into()),
            options: ["default", "low", "medium", "high", "max"].into_iter().map(|value| AcpSessionConfigChoicePayload { value: value.into(), name: value.into(), description: None, group_name: None }).collect(),
        },
    ]
}

fn message_content_blocks(frame: &Value) -> &[Value] {
    frame
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn tool_result_text(block: &Value) -> Option<String> {
    let content = block.get("content")?;
    match content {
        Value::String(value) => Some(value.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .map(|item| {
                    item.get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| item.to_string())
                })
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        Value::Null => None,
        value => Some(value.to_string()),
    }
}

fn emit_activity(
    window: &tauri::Window,
    conversation_id: i64,
    message_id: i64,
    session_id: Option<&str>,
    item_id: &str,
    sequence: u64,
    kind: &str,
    status: &str,
    title: Option<String>,
    input: Option<Value>,
    output: Option<String>,
    error: Option<String>,
) {
    let event = AgentActivityEvent {
        conversation_id,
        response_message_id: message_id,
        agent_kind: CLAUDE_SDK_API_TYPE.into(),
        session_id: session_id.map(str::to_string),
        item_id: item_id.into(),
        sequence,
        kind: kind.into(),
        status: status.into(),
        title,
        input,
        output,
        error,
        metadata: json!({"source":"claude"}),
        content_offset: None,
    };
    let _ = window.emit(
        &format!("conversation_event_{conversation_id}"),
        ConversationEvent {
            r#type: "agent_activity".into(),
            data: serde_json::to_value(event).unwrap(),
        },
    );
}

async fn spawn_claude_process(
    config: &ClaudeSdkConfig,
    session_id: Option<&str>,
) -> Result<(
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
), String> {
    let (mut command, resolved) = build_claude_command(config, session_id);
    tracing::info!(
        cli = %resolved.display(),
        model = ?config.model,
        has_anthropic_api_key = config.env_vars.contains_key("ANTHROPIC_API_KEY"),
        has_anthropic_base_url = config.env_vars.contains_key("ANTHROPIC_BASE_URL"),
        "Starting Claude Code stream-json process"
    );
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or("Claude stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Claude stdout unavailable")?;
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::error!(target = "claude_code", "{line}");
            }
        });
    }
    Ok((child, stdin, BufReader::new(stdout).lines()))
}

fn build_claude_command(
    config: &ClaudeSdkConfig,
    session_id: Option<&str>,
) -> (Command, PathBuf) {
    let resolved = resolve_acp_cli_path(&config.cli_command);
    #[cfg(target_os = "windows")]
    let (program, prefix_args): (PathBuf, Vec<String>) = match resolved.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("cmd" | "bat") => (PathBuf::from("cmd.exe"), vec!["/D".into(), "/S".into(), "/C".into(), resolved.display().to_string()]),
        Some("ps1") => (PathBuf::from("pwsh.exe"), vec!["-NoProfile".into(), "-File".into(), resolved.display().to_string()]),
        _ => (resolved.clone(), Vec::new()),
    };
    #[cfg(not(target_os = "windows"))]
    let (program, prefix_args): (PathBuf, Vec<String>) = (resolved.clone(), Vec::new());
    let mut command = Command::new(program);
    command.args(prefix_args).args(["--print", "--verbose", "--input-format", "stream-json", "--output-format", "stream-json", "--include-partial-messages"]);
    if let Some(mcp_config_file) = &config.mcp_config_file {
        command.arg("--mcp-config").arg(mcp_config_file.path());
    }
    if let Some(session_id) = session_id { command.arg("--resume").arg(session_id); }
    if let Some(model) = &config.model { command.args(["--model", model]); }
    if let Some(mode) = &config.permission_mode { command.args(["--permission-mode", mode]); }
    if let Some(effort) = &config.effort { command.args(["--effort", effort]); }
    command.current_dir(&config.working_directory).envs(&config.env_vars).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    (command, resolved)
}

pub fn spawn_claude_session_task(
    app: tauri::AppHandle,
    _conversation_id: i64,
    mut config: ClaudeSdkConfig,
) -> ClaudeSessionHandle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let run_id = uuid::Uuid::new_v4().to_string();
    let task_run_id = run_id.clone();
    let handle = ClaudeSessionHandle { sender: tx, run_id };
    tauri::async_runtime::spawn(async move {
        let stored_session: Option<String> = ConversationDatabase::new(&app)
            .ok()
            .and_then(|db| db.get_agent_session_id(_conversation_id, CLAUDE_SDK_API_TYPE).ok())
            .flatten();
        let (mut child, mut stdin, mut lines) = match spawn_claude_process(&config, stored_session.as_deref()).await {
            Ok(process) => process,
            Err(error) => {
                while let Some(message) = rx.recv().await {
                    match message {
                        SessionCommand::Prompt { message_id, window, .. } => {
                            emit_claude_failure(&app, _conversation_id, message_id, &window, &error);
                            if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                                manager.clear_focus(&app, _conversation_id).await;
                            }
                            break;
                        }
                        SessionCommand::Cancel { response } => {
                            let _ = response.send(Ok(()));
                        }
                        SessionCommand::SetConfig { response, .. } => {
                            let _ = response.send(Err(format!("Claude Code 尚未启动：{error}")));
                        }
                    }
                }
                cleanup_claude_session(&app, _conversation_id, &task_run_id).await;
                return;
            }
        };
        tracing::info!(
            conversation_id = _conversation_id,
            model = ?config.model,
            "Claude Code stream-json process started"
        );
        let mut session_id: Option<String> = stored_session.clone();
        emit_snapshot(
            &app,
            _conversation_id,
            ClaudeConversationSessionState {
                conversation_id: _conversation_id,
                agent_kind: CLAUDE_SDK_API_TYPE.into(),
                session_id: session_id.clone(),
                has_active_prompt: false,
                model: config.model.clone(),
                permission_mode: config.permission_mode.clone(),
                load_session_supported: false,
                session_resume_supported: true,
                restored_session_method: stored_session.as_ref().map(|_| "resume".into()),
                config_options: claude_config_options(&config),
            },
        );
        while let Some(message) = rx.recv().await {
            match message {
                SessionCommand::Cancel { response } => {
                    let _ = child.kill().await;
                    let _ = response.send(Ok(()));
                    break;
                }
                SessionCommand::SetConfig { config_id, value, response } => {
                    match config_id.as_str() {
                        "model" => {
                            if !config.model_choices.iter().any(|choice| choice.value == value) {
                                let _ = response.send(Err(format!("Claude Code 模型不在当前提供商的模型列表中：{value}")));
                                continue;
                            }
                            config.model = Some(value)
                        }
                        "effort" => config.effort = (value != "default").then_some(value),
                        _ => { let _ = response.send(Err(format!("Claude Code 不支持会话配置项: {config_id}"))); continue; }
                    }
                    let _ = child.kill().await;
                    match spawn_claude_process(&config, session_id.as_deref()).await {
                        Ok((new_child, new_stdin, new_lines)) => {
                            child = new_child;
                            stdin = new_stdin;
                            lines = new_lines;
                            let _ = response.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = response.send(Err(format!("重启 Claude Code 会话失败: {error}")));
                            break;
                        }
                    }
                    emit_snapshot(&app, _conversation_id, ClaudeConversationSessionState {
                        conversation_id: _conversation_id, agent_kind: CLAUDE_SDK_API_TYPE.into(), session_id: session_id.clone(), has_active_prompt: false,
                        model: config.model.clone(), permission_mode: config.permission_mode.clone(), load_session_supported: false, session_resume_supported: true,
                        restored_session_method: None, config_options: claude_config_options(&config),
                    });
                }
                SessionCommand::Prompt { message_id, prompt, window } => {
                    let input = json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":prompt}]},"session_id":session_id});
                    if let Err(error) = stdin.write_all(format!("{input}\n").as_bytes()).await {
                        emit_claude_failure(
                            &app,
                            _conversation_id,
                            message_id,
                            &window,
                            &format!("写入 Claude Code 请求失败：{error}"),
                        );
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_focus(&app, _conversation_id).await;
                        }
                        break;
                    }
                    if let Err(error) = stdin.flush().await {
                        emit_claude_failure(
                            &app,
                            _conversation_id,
                            message_id,
                            &window,
                            &format!("提交 Claude Code 请求失败：{error}"),
                        );
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_focus(&app, _conversation_id).await;
                        }
                        break;
                    }
                    if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                        manager.set_assistant_streaming(&app, _conversation_id, message_id).await;
                    }
                    let mut content = String::new();
                    let mut sequence = 0_u64;
                    emit_snapshot(
                        &app,
                        _conversation_id,
                        ClaudeConversationSessionState {
                            conversation_id: _conversation_id,
                            agent_kind: CLAUDE_SDK_API_TYPE.into(),
                            session_id: session_id.clone(),
                            has_active_prompt: true,
                            model: config.model.clone(),
                            permission_mode: config.permission_mode.clone(),
                            load_session_supported: false,
                            session_resume_supported: true,
                            restored_session_method: None,
                            config_options: claude_config_options(&config),
                        },
                    );
                    let mut usage = Value::Null;
                    let mut stream_error: Option<String> = None;
                    loop {
                        let line = tokio::select! {
                            next = lines.next_line() => match next {
                                Ok(Some(line)) => line,
                                Ok(None) => {
                                    stream_error = Some("Claude Code 进程提前退出".into());
                                    break;
                                }
                                Err(error) => {
                                    stream_error = Some(format!("读取 Claude Code 输出失败：{error}"));
                                    break;
                                }
                            },
                            command = rx.recv() => match command {
                                Some(SessionCommand::Cancel { response }) => {
                                    let request_id = uuid::Uuid::new_v4().to_string();
                                    let interrupt = json!({"type":"control_request","request_id":request_id,"request":{"subtype":"interrupt"}});
                                    let result = stdin.write_all(format!("{interrupt}\n").as_bytes()).await.and_then(|_| std::io::Result::Ok(()));
                                    let _ = stdin.flush().await;
                                    let _ = response.send(result.map_err(|error| error.to_string()));
                                    if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                                        manager.clear_focus(&app, _conversation_id).await;
                                    }
                                    continue;
                                }
                                Some(SessionCommand::Prompt { .. }) => continue,
                                Some(SessionCommand::SetConfig { response, .. }) => {
                                    let _ = response.send(Err("Claude Code 配置将在下一次会话启动时生效".into()));
                                    continue;
                                }
                                None => {
                                    stream_error = Some("Claude Code 会话在完成当前请求前已关闭".into());
                                    break;
                                },
                            }
                        };
                        let Ok(frame) = serde_json::from_str::<Value>(&line) else { continue };
                        if let Some(id) = frame.get("session_id").and_then(Value::as_str) {
                            session_id = Some(id.into());
                            if let Ok(db) = ConversationDatabase::new(&app) {
                                let _ = db.upsert_agent_session_id(
                                    _conversation_id,
                                    CLAUDE_SDK_API_TYPE,
                                    id,
                                );
                            }
                        }
                        match frame.get("type").and_then(Value::as_str).unwrap_or("") {
                            "system" => {
                                if let Some(model) = frame.get("model").and_then(Value::as_str) {
                                    config.model = Some(model.to_string());
                                }
                                if let Some(effort) = frame.get("effort").and_then(Value::as_str) {
                                    config.effort = Some(effort.to_string());
                                }
                                emit_snapshot(&app, _conversation_id, ClaudeConversationSessionState {
                                    conversation_id: _conversation_id, agent_kind: CLAUDE_SDK_API_TYPE.into(), session_id: session_id.clone(), has_active_prompt: true,
                                    model: config.model.clone(), permission_mode: config.permission_mode.clone(), load_session_supported: false, session_resume_supported: true,
                                    restored_session_method: None, config_options: claude_config_options(&config),
                                });
                            }
                            "stream_event" => {
                                if let Some(delta) =
                                    frame.pointer("/event/delta/text").and_then(Value::as_str)
                                {
                                    content.push_str(delta);
                                    persist(&app, message_id, &content, false);
                                    emit_update(
                                        &window,
                                        _conversation_id,
                                        message_id,
                                        &content,
                                        false,
                                    );
                                }
                            }
                            "assistant" => {
                                for block in message_content_blocks(&frame) {
                                    if block.get("type").and_then(Value::as_str) == Some("tool_use")
                                    {
                                        sequence += 1;
                                        emit_activity(
                                            &window,
                                            _conversation_id,
                                            message_id,
                                            session_id.as_deref(),
                                            block
                                                .get("id")
                                                .and_then(Value::as_str)
                                                .unwrap_or("tool"),
                                            sequence,
                                            "tool",
                                            "executing",
                                            block
                                                .get("name")
                                                .and_then(Value::as_str)
                                                .map(str::to_string),
                                            block.get("input").cloned(),
                                            None,
                                            None,
                                        );
                                    }
                                }
                            }
                            "user" => {
                                for block in message_content_blocks(&frame) {
                                    if block.get("type").and_then(Value::as_str)
                                        == Some("tool_result")
                                    {
                                        let result = tool_result_text(block);
                                        let is_error = block
                                            .get("is_error")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false);
                                        sequence += 1;
                                        emit_activity(
                                            &window,
                                            _conversation_id,
                                            message_id,
                                            session_id.as_deref(),
                                            block
                                                .get("tool_use_id")
                                                .and_then(Value::as_str)
                                                .unwrap_or("tool"),
                                            sequence,
                                            "tool",
                                            if is_error { "failed" } else { "success" },
                                            None,
                                            None,
                                            (!is_error).then(|| result.clone()).flatten(),
                                            is_error.then(|| result.unwrap_or_else(|| "Claude Code tool failed".into())),
                                        );
                                    }
                                }
                            }
                            "control_request" => {
                                if let Err(error) =
                                    handle_permission(&app, _conversation_id, &frame, &mut stdin)
                                        .await
                                {
                                    stream_error = Some(format!("Claude Code 权限响应失败：{error}"));
                                    break;
                                }
                            }
                            "result" => {
                                if frame.get("is_error").and_then(Value::as_bool) == Some(true)
                                    || frame.get("subtype").and_then(Value::as_str) == Some("error")
                                    || frame.get("error").is_some_and(|value| !value.is_null())
                                {
                                    stream_error = Some(
                                        frame
                                            .get("error")
                                            .and_then(Value::as_str)
                                            .or_else(|| frame.get("result").and_then(Value::as_str))
                                            .unwrap_or("Claude Code 返回错误")
                                            .to_string(),
                                    );
                                    break;
                                }
                                if content.is_empty() {
                                    content = frame
                                        .get("result")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .into();
                                    if content.is_empty() {
                                        if let Some(error) = frame.get("error").and_then(Value::as_str) {
                                            content = format!("Claude Code 返回错误：{error}");
                                        }
                                    }
                                }
                                usage = frame.get("usage").cloned().unwrap_or(Value::Null);
                                break;
                            }
                            _ => {}
                        }
                    }
                    if let Some(error) = stream_error.as_deref() {
                        emit_claude_failure(&app, _conversation_id, message_id, &window, error);
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_focus(&app, _conversation_id).await;
                        }
                        break;
                    }
                    if content.is_empty() {
                        emit_claude_failure(
                            &app,
                            _conversation_id,
                            message_id,
                            &window,
                            "Claude Code 本轮未返回任何内容",
                        );
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_focus(&app, _conversation_id).await;
                        }
                        break;
                    }
                    persist(&app, message_id, &content, true);
                    if !usage.is_null() {
                        persist_usage(&app, message_id, &usage);
                    }
                    emit_update(&window, _conversation_id, message_id, &content, true);
                    let _ = window.emit(
                        &format!("conversation_event_{}", _conversation_id),
                        ConversationEvent {
                            r#type: "stream_complete".into(),
                            data: json!({
                                "conversation_id": _conversation_id,
                                "response_message_id": message_id,
                                "has_response": !content.is_empty(),
                            }),
                        },
                    );
                    if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                        manager.clear_focus(&app, _conversation_id).await;
                    }
                    emit_snapshot(
                        &app,
                        _conversation_id,
                        ClaudeConversationSessionState {
                            conversation_id: _conversation_id,
                            agent_kind: CLAUDE_SDK_API_TYPE.into(),
                            session_id: session_id.clone(),
                            has_active_prompt: false,
                            model: config.model.clone(),
                            permission_mode: config.permission_mode.clone(),
                            load_session_supported: false,
                            session_resume_supported: true,
                            restored_session_method: None,
                            config_options: claude_config_options(&config),
                        },
                    );
                }
            }
        }
        cleanup_claude_session(&app, _conversation_id, &task_run_id).await;
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_to_official_cli() {
        assert_eq!(extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap().cli_command, "claude");
    }

    #[test]
    fn assistant_model_overrides_provider_cli() {
        let provider = crate::db::llm_db::LLMProviderConfig {
            id: 1,
            name: "claude_cli_command".into(),
            llm_provider_id: 1,
            value: "provider-claude".into(),
            append_location: "".into(),
            is_addition: false,
        };
        let model = crate::db::assistant_db::AssistantModelConfig {
            id: 1,
            assistant_id: 1,
            assistant_model_id: 1,
            name: "claude_cli_command".into(),
            value: Some("model-claude".into()),
            value_type: "string".into(),
        };
        assert_eq!(
            extract_claude_sdk_config(&[model], &[provider], None, Vec::new()).unwrap().cli_command,
            "model-claude"
        );
    }

    #[test]
    fn anthropic_provider_fields_become_claude_environment_variables() {
        let providers = vec![
            crate::db::llm_db::LLMProviderConfig {
                id: 1,
                name: "api_key".into(),
                llm_provider_id: 1,
                value: "provider-key".into(),
                append_location: "".into(),
                is_addition: false,
            },
            crate::db::llm_db::LLMProviderConfig {
                id: 2,
                name: "endpoint".into(),
                llm_provider_id: 1,
                value: "https://proxy.example/v1".into(),
                append_location: "".into(),
                is_addition: false,
            },
        ];
        let config = extract_claude_sdk_config(&[], &providers, None, Vec::new()).unwrap();
        assert_eq!(config.env_vars.get("ANTHROPIC_API_KEY"), Some(&"provider-key".to_string()));
        assert_eq!(config.env_vars.get("ANTHROPIC_BASE_URL"), Some(&"https://proxy.example/v1".to_string()));
    }

    #[test]
    fn explicit_claude_env_overrides_anthropic_provider_fields() {
        let providers = vec![
            crate::db::llm_db::LLMProviderConfig {
                id: 1,
                name: "api_key".into(),
                llm_provider_id: 1,
                value: "provider-key".into(),
                append_location: "".into(),
                is_addition: false,
            },
            crate::db::llm_db::LLMProviderConfig {
                id: 2,
                name: "acp_env_vars".into(),
                llm_provider_id: 1,
                value: "ANTHROPIC_API_KEY=explicit-key".into(),
                append_location: "".into(),
                is_addition: false,
            },
        ];
        let config = extract_claude_sdk_config(&[], &providers, None, Vec::new()).unwrap();
        assert_eq!(config.env_vars.get("ANTHROPIC_API_KEY"), Some(&"explicit-key".to_string()));
    }

    #[test]
    fn claude_command_contains_stream_model_and_provider_environment() {
        let providers = vec![
            crate::db::llm_db::LLMProviderConfig {
                id: 1,
                name: "api_key".into(),
                llm_provider_id: 1,
                value: "provider-key".into(),
                append_location: "".into(),
                is_addition: false,
            },
            crate::db::llm_db::LLMProviderConfig {
                id: 2,
                name: "endpoint".into(),
                llm_provider_id: 1,
                value: "https://proxy.example/v1".into(),
                append_location: "".into(),
                is_addition: false,
            },
        ];
        let config = extract_claude_sdk_config(
            &[],
            &providers,
            Some("deepseek-v4-flash".into()),
            Vec::new(),
        )
        .unwrap();
        let (command, _) = build_claude_command(&config, None);
        let command = command.as_std();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|pair| pair == ["--input-format", "stream-json"]));
        assert!(args.windows(2).any(|pair| pair == ["--output-format", "stream-json"]));
        assert!(args.windows(2).any(|pair| pair == ["--model", "deepseek-v4-flash"]));
        let env = command
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key.to_string_lossy(), value.to_string_lossy())))
            .collect::<HashMap<_, _>>();
        assert_eq!(env.get("ANTHROPIC_API_KEY").map(|value| value.as_ref()), Some("provider-key"));
        assert_eq!(env.get("ANTHROPIC_BASE_URL").map(|value| value.as_ref()), Some("https://proxy.example/v1"));
    }

    #[test]
    fn selected_mcp_tools_change_session_signature() {
        let mut config = extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap();
        let original = config.session_signature.clone();
        config.selected_mcp_tools_payload = "[{\"server_id\":1}]".into();
        refresh_claude_session_signature(&mut config);
        assert_ne!(config.session_signature, original);
        assert!(config.session_signature.contains("selected_mcp"));
    }

    #[test]
    fn claude_command_contains_aipp_mcp_config() {
        let proxy = crate::acp_mcp_bridge::AcpMcpProxyConfig {
            addr: "127.0.0.1:3210".into(),
            token: "proxy-token".into(),
        };
        let payload = "[{\"server_id\":1}]";
        let mcp_config = build_claude_mcp_config(
            std::path::Path::new("C:/Aipp/Aipp.exe"),
            std::path::Path::new("C:/Aipp/mcp.db"),
            42,
            &proxy,
            payload,
        )
        .unwrap();
        let parsed: Value = serde_json::from_str(&mcp_config).unwrap();
        let server = parsed.pointer("/mcpServers/aipp").unwrap();
        assert_eq!(server.get("type").and_then(Value::as_str), Some("stdio"));
        assert_eq!(
            server.pointer(&format!("/env/{ACP_MCP_CONVERSATION_ID_ENV}")).and_then(Value::as_str),
            Some("42")
        );
        assert_eq!(
            server.pointer(&format!("/env/{ACP_MCP_SELECTED_TOOLS_ENV}")).and_then(Value::as_str),
            Some(payload)
        );

        let config_file = write_claude_mcp_config_file(&mcp_config).unwrap();
        let mut config = extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap();
        config.mcp_config_file = Some(config_file.clone());
        let (command, _) = build_claude_command(&config, None);
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--mcp-config" && pair[1] == config_file.path().display().to_string()));
    }

    #[test]
    fn parses_all_tool_blocks_and_structured_results() {
        let frame = json!({
            "message": {"content": [
                {"type":"text","text":"before"},
                {"type":"tool_use","id":"tool-1","name":"aipp_t1_search"},
                {"type":"tool_use","id":"tool-2","name":"aipp_t2_fetch"}
            ]}
        });
        let tools = message_content_blocks(&frame)
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
            .count();
        assert_eq!(tools, 2);

        let result = json!({
            "content": [
                {"type":"text","text":"first"},
                {"type":"text","text":"second"}
            ],
            "is_error": true
        });
        assert_eq!(tool_result_text(&result).as_deref(), Some("first\nsecond"));
        assert_eq!(result.get("is_error").and_then(Value::as_bool), Some(true));
    }
}

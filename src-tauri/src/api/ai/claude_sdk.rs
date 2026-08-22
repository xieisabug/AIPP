use crate::api::ai::acp::resolve_acp_cli_path;
use crate::api::ai::acp::{
    AcpPermissionDecision, AcpPermissionOptionPayload, AcpPermissionRequestEvent,
    AcpPermissionState, AcpSessionConfigChoicePayload, AcpSessionConfigOptionPayload,
};
use crate::api::ai::codex_app_server::AgentActivityEvent;
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::api::operation_api::{emit_permission_request_event, ACP_PERMISSION_REQUEST_EVENT};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::errors::AppError;
use crate::state::activity_state::ConversationActivityManager;
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, process::Stdio};
use tauri::{Emitter, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, oneshot},
};

pub const CLAUDE_SDK_API_TYPE: &str = "claude_sdk";

#[derive(Debug, Clone)]
pub struct ClaudeSdkConfig {
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub effort: Option<String>,
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
}

pub struct ClaudeSessionEntry {
    pub handle: ClaudeSessionHandle,
    pub snapshot: ClaudeConversationSessionState,
    pub config_signature: String,
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
) -> Result<ClaudeSdkConfig, AppError> {
    let cli_command = model_value(models, "claude_cli_command")
        .or_else(|| provider_value(providers, "claude_cli_command"))
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
    for raw in [provider_value(providers, "acp_env_vars"), model_value(models, "acp_env_vars")]
        .into_iter()
        .flatten()
    {
        crate::api::ai::acp::merge_acp_env_blob(&mut env_vars, &raw);
    }
    let session_signature = serde_json::to_string(&json!({"cli":cli_command,"cwd":working_directory,"model":model,"permission_mode":permission_mode,"effort":effort,"env":env_vars})).unwrap_or_default();
    Ok(ClaudeSdkConfig {
        cli_command,
        working_directory,
        env_vars,
        model,
        permission_mode,
        effort,
        session_signature,
    })
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

fn claude_config_options(config: &ClaudeSdkConfig) -> Vec<AcpSessionConfigOptionPayload> {
    vec![
        AcpSessionConfigOptionPayload {
            id: "model".into(), name: "模型".into(), description: Some("下一轮 Claude Code 响应使用的模型".into()), category: Some("model".into()),
            current_value: config.model.clone().unwrap_or_default(), options: Vec::new(),
        },
        AcpSessionConfigOptionPayload {
            id: "effort".into(), name: "思考强度".into(), description: Some("下一轮 Claude Code 响应的思考强度".into()), category: Some("thought_level".into()),
            current_value: config.effort.clone().unwrap_or_else(|| "default".into()),
            options: ["default", "low", "medium", "high", "max"].into_iter().map(|value| AcpSessionConfigChoicePayload { value: value.into(), name: value.into(), description: None, group_name: None }).collect(),
        },
    ]
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
        error: None,
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
    let resolved = resolve_acp_cli_path(&config.cli_command);
    #[cfg(target_os = "windows")]
    let (program, prefix_args): (PathBuf, Vec<String>) = match resolved.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("cmd" | "bat") => (PathBuf::from("cmd.exe"), vec!["/D".into(), "/S".into(), "/C".into(), resolved.display().to_string()]),
        Some("ps1") => (PathBuf::from("pwsh.exe"), vec!["-NoProfile".into(), "-File".into(), resolved.display().to_string()]),
        _ => (resolved.clone(), Vec::new()),
    };
    #[cfg(not(target_os = "windows"))]
    let (program, prefix_args): (PathBuf, Vec<String>) = (resolved, Vec::new());
    let mut command = Command::new(program);
    command.args(prefix_args).args(["--print", "--verbose", "--input-format", "stream-json", "--output-format", "stream-json", "--include-partial-messages"]);
    if let Some(session_id) = session_id { command.arg("--resume").arg(session_id); }
    if let Some(model) = &config.model { command.args(["--model", model]); }
    if let Some(mode) = &config.permission_mode { command.args(["--permission-mode", mode]); }
    if let Some(effort) = &config.effort { command.args(["--effort", effort]); }
    let mut child = command.current_dir(&config.working_directory).envs(&config.env_vars).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).kill_on_drop(true).spawn().map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or("Claude stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Claude stdout unavailable")?;
    Ok((child, stdin, BufReader::new(stdout).lines()))
}

pub fn spawn_claude_session_task(
    app: tauri::AppHandle,
    _conversation_id: i64,
    mut config: ClaudeSdkConfig,
) -> ClaudeSessionHandle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = ClaudeSessionHandle { sender: tx };
    tauri::async_runtime::spawn(async move {
        let stored_session: Option<String> = ConversationDatabase::new(&app)
            .ok()
            .and_then(|db| db.get_agent_session_id(_conversation_id, CLAUDE_SDK_API_TYPE).ok())
            .flatten();
        let Ok((mut child, mut stdin, mut lines)) = spawn_claude_process(&config, stored_session.as_deref()).await else { return; };
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
                        "model" => config.model = Some(value),
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
                            continue;
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
                    if stdin.write_all(format!("{input}\n").as_bytes()).await.is_err() {
                        break;
                    }
                    let _ = stdin.flush().await;
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
                    loop {
                        let line = tokio::select! {
                            next = lines.next_line() => match next { Ok(Some(line)) => line, _ => break },
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
                                None => break,
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
                                if let Some(block) = frame.pointer("/message/content/0") {
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
                                        );
                                    }
                                }
                            }
                            "user" => {
                                if let Some(block) = frame.pointer("/message/content/0") {
                                    if block.get("type").and_then(Value::as_str)
                                        == Some("tool_result")
                                    {
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
                                            "success",
                                            None,
                                            None,
                                            block
                                                .get("content")
                                                .and_then(Value::as_str)
                                                .map(str::to_string),
                                        );
                                    }
                                }
                            }
                            "control_request" => {
                                let _ =
                                    handle_permission(&app, _conversation_id, &frame, &mut stdin)
                                        .await;
                            }
                            "result" => {
                                if content.is_empty() {
                                    content = frame
                                        .get("result")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .into();
                                }
                                usage = frame.get("usage").cloned().unwrap_or(Value::Null);
                                break;
                            }
                            _ => {}
                        }
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
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_to_official_cli() {
        assert_eq!(extract_claude_sdk_config(&[], &[], None).unwrap().cli_command, "claude");
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
            extract_claude_sdk_config(&[model], &[provider], None).unwrap().cli_command,
            "model-claude"
        );
    }
}

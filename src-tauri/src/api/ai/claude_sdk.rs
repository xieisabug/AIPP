use crate::api::ai::acp::resolve_acp_cli_path;
use crate::api::ai::acp::{
    AcpPermissionDecision, AcpPermissionOptionPayload, AcpPermissionRequestEvent,
    AcpPermissionState, AcpPlanEntryPayload, AcpSessionConfigChoicePayload,
    AcpSessionConfigOptionPayload,
};
use crate::api::ai::agent_completion::{handle_agent_success, AgentKind};
use crate::api::ai::agent_plan::AgentPlanMessage;
use crate::api::ai::agent_driver::events::{AgentEventIdentity, emit_to_window, emit_to_chat_windows};
use crate::api::ai::agent_driver::failure::{AgentDiagnosticBuffer, AgentFailure, AgentFailureStage, AgentProcessDiagnostics, emit_normalized_failure_to_window};
use crate::api::ai::claude_activity::ClaudeActivities;
use crate::api::ai::agent_activity::persist_activities;
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::acp_mcp_bridge::{
    ensure_proxy_server, ACP_MCP_BRIDGE_ARG, ACP_MCP_CONVERSATION_ID_ENV,
    ACP_MCP_DB_PATH_ENV, ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV, ACP_MCP_PROXY_ADDR_ENV,
    ACP_MCP_PROXY_TOKEN_ENV, ACP_MCP_SELECTED_TOOLS_ENV,
};
use crate::api::operation_api::{emit_permission_request_event, ACP_PERMISSION_REQUEST_EVENT};
use crate::mcp::builtin_mcp::interaction::{
    request_ask_user_question_scoped, AskUserQuestionItem, AskUserQuestionMetadata,
    AskUserQuestionOption, AskUserQuestionRequest, InteractionState,
};
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
    time::{Duration, Instant},
};
use tauri::Manager;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, oneshot, watch},
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
    /// Provider-row identity captured when this native runtime is prepared.
    pub instance_id: Option<i64>,
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub effort: Option<String>,
    pub model_choices: Vec<AcpSessionConfigChoicePayload>,
    pub selected_mcp_tools_payload: String,
    mcp_config_file: Option<Arc<ClaudeMcpConfigFile>>,
    mcp_scope: Option<crate::acp_mcp_bridge::provenance::NativeMcpScope>,
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
    pub connection_event_id: Option<String>,
    pub plan: Vec<AcpPlanEntryPayload>,
    pub plan_explanation: Option<String>,
    pub config_options: Vec<AcpSessionConfigOptionPayload>,
}

enum SessionCommand {
    Prompt { message_id: i64, prompt: String, window: tauri::Window, cancel_generation: u64 },
    Cancel { response: oneshot::Sender<Result<(), String>> },
    SetConfig { config_id: String, value: String, response: oneshot::Sender<Result<(), String>> },
}

#[derive(Clone)]
pub struct ClaudeSessionHandle {
    sender: mpsc::UnboundedSender<SessionCommand>,
    cancel_signal: watch::Sender<u64>,
    run_id: String,
}

pub struct ClaudeSessionEntry {
    pub handle: ClaudeSessionHandle,
    pub snapshot: ClaudeConversationSessionState,
    pub config_signature: String,
    pub run_id: String,
    pub last_activity: Instant,
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
            last_activity: Instant::now(),
        }
    }

    pub(crate) fn reuse_handle(&mut self, config_signature: &str) -> Option<ClaudeSessionHandle> {
        if self.config_signature != config_signature {
            return None;
        }
        self.touch();
        Some(self.handle.clone())
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn is_idle_for(&self, idle_timeout: Duration) -> bool {
        super::agent_session_lifecycle::should_release_idle_session(
            self.snapshot.has_active_prompt,
            self.last_activity.elapsed(),
            idle_timeout,
        )
    }
}

impl ClaudeSessionHandle {
    pub(crate) fn run_id(&self) -> &str { &self.run_id }

    #[cfg(test)]
    pub(crate) fn driver_test_channel() -> (
        Self,
        mpsc::UnboundedReceiver<super::agent_driver::session::TestSessionCommand>,
    ) {
        use super::agent_driver::session::TestSessionCommand;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (observed, observations) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                let command = match command {
                    SessionCommand::Cancel { response } => TestSessionCommand::Cancel { response },
                    SessionCommand::SetConfig { config_id, value, response } => {
                        TestSessionCommand::SetConfig { config_id, value, response }
                    }
                    _ => panic!("unexpected command in driver control test"),
                };
                if observed.send(command).is_err() {
                    break;
                }
            }
        });
        (Self {
            sender,
            cancel_signal: watch::channel(0).0,
            run_id: "driver-test-run".into(),
        }, observations)
    }

    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        self.sender
            .send(SessionCommand::Prompt {
                message_id, prompt, window, cancel_generation: *self.cancel_signal.borrow(),
            })
            .map_err(|_| AppError::UnknownError("Claude Code session closed".into()))
    }
    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        // Wake a blocked approval without consuming/reordering actor commands.
        // The native actor still acknowledges only after writing the interrupt.
        self.cancel_signal.send_modify(|generation| *generation = generation.wrapping_add(1));
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
        instance_id: None,
        cli_command,
        working_directory,
        env_vars,
        model,
        permission_mode,
        effort,
        model_choices,
        selected_mcp_tools_payload: String::new(),
        mcp_config_file: None,
        mcp_scope: None,
        session_signature,
    })
}

pub fn refresh_claude_session_signature(config: &mut ClaudeSdkConfig) {
    config.session_signature = serde_json::to_string(&json!({
        "instance_id": config.instance_id, "cli": config.cli_command, "cwd": config.working_directory, "model": config.model,
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
        config.mcp_scope = None;
        return Ok(());
    }

    let bridge_command = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve AIPP executable for Claude MCP bridge: {error}"))?;
    let mcp_db_path = MCPDatabase::db_path(app_handle)?;
    let mut proxy = ensure_proxy_server(app_handle.clone()).await?;
    let mcp_scope = crate::acp_mcp_bridge::provenance::NativeMcpScope::new(conversation_id)?;
    proxy.token = mcp_scope.token().to_string();
    let mcp_config = build_claude_mcp_config(
        &bridge_command,
        &mcp_db_path,
        conversation_id,
        &proxy,
        &config.selected_mcp_tools_payload,
    )?;
    config.mcp_config_file = Some(write_claude_mcp_config_file(&mcp_config)?);
    config.mcp_scope = Some(mcp_scope);
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

/// Claude Code 的 usage 是 Anthropic 口径：`input_tokens` 不含缓存读取与缓存写入。
/// 换成全应用统一口径（输入含缓存读写），缓存命中/写入另记，统计与缓存命中率才能和其他通道对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClaudeUsageCounts {
    pub input: i64,
    pub output: i64,
    pub total: i64,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
}

pub(crate) fn claude_usage_counts(usage: &Value) -> ClaudeUsageCounts {
    let get = |key: &str| usage.get(key).and_then(Value::as_i64).map(|value| value.max(0));
    let cache_read = get("cache_read_input_tokens");
    let cache_write = get("cache_creation_input_tokens");
    let input = get("input_tokens").unwrap_or(0) + cache_read.unwrap_or(0) + cache_write.unwrap_or(0);
    let output = get("output_tokens").unwrap_or(0);
    ClaudeUsageCounts { input, output, total: input + output, cache_read, cache_write }
}

fn persist_usage(app: &tauri::AppHandle, id: i64, usage: &Value) {
    let Ok(db) = ConversationDatabase::new(app) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(mut message)) = repo.read(id) else { return };
    let counts = claude_usage_counts(usage);
    let clamp = |value: i64| i32::try_from(value).unwrap_or(i32::MAX);
    message.input_token_count = clamp(counts.input);
    message.output_token_count = clamp(counts.output);
    message.token_count = clamp(counts.total);
    let mut metadata = message
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    metadata.insert("usage_source".into(), json!("reported"));
    if let Some(cache_read) = counts.cache_read {
        metadata.insert("cached_input_tokens".into(), json!(cache_read));
        metadata.insert("cached_read_tokens".into(), json!(cache_read));
    }
    if let Some(cache_write) = counts.cache_write {
        metadata.insert("cached_write_tokens".into(), json!(cache_write));
        metadata.insert("cache_creation_tokens".into(), json!(cache_write));
    }
    // 原始 usage 保留下来，便于核对归一前的数字。
    metadata.insert("claude_usage".into(), usage.clone());
    message.metadata_json = Some(Value::Object(metadata).to_string());
    let _ = repo.update(&message);
}

/// 将 Claude Code AskUserQuestion 工具的 input 转成统一的 AskUserQuestion 请求。
/// Claude Code 的 questions 结构与 Codex requestUserInput 类似，但没有 id/isSecret。
fn claude_ask_user_question_payload(input: &Value) -> Result<AskUserQuestionRequest, String> {
    let questions = input
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("Claude Code AskUserQuestion is missing questions")?;
    let mut converted = Vec::with_capacity(questions.len());
    for question in questions {
        let text = question
            .get("question")
            .and_then(Value::as_str)
            .ok_or("Claude Code AskUserQuestion question is missing question")?
            .to_string();
        let header = question
            .get("header")
            .and_then(Value::as_str)
            .unwrap_or("Claude Code")
            .to_string();
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        Some(AskUserQuestionOption {
                            label: option.get("label")?.as_str()?.to_string(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or("选择此项")
                                .to_string(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let multi_select = question
            .get("multiSelect")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        converted.push(AskUserQuestionItem {
            question: text,
            header,
            options,
            multi_select,
            is_secret: false,
        });
    }
    Ok(AskUserQuestionRequest {
        questions: converted,
        answers: None,
        metadata: Some(AskUserQuestionMetadata {
            source: Some("claude_sdk".to_string()),
        }),
    })
}

/// 将用户答案以 问题文本 -> 答案 的形式注入 updatedInput.answers，回给 Claude Code CLI。
fn inject_answers_into_input(input: &Value, answers: &HashMap<String, String>) -> Value {
    let mut updated = input.clone();
    if let Some(obj) = updated.as_object_mut() {
        obj.insert("answers".to_string(), json!(answers));
    }
    updated
}

/// Claude Code 的 AskUserQuestion 工具经由 can_use_tool 控制请求到达，
/// 这里接到统一的 AskUserQuestion 内联卡片上，用户作答后通过
/// updatedInput.answers（问题文本 -> 答案）回给 CLI。
async fn handle_ask_user_question(
    app: &tauri::AppHandle,
    conversation_id: i64,
    request: &Value,
    request_id: &str,
    instance_id: Option<i64>,
    run_id: &str,
    stdin: &mut tokio::process::ChildStdin,
    cancellation: &mut watch::Receiver<u64>,
    cancel_generation: u64,
) -> Result<(), String> {
    let input = request.get("input").cloned().unwrap_or(Value::Null);
    let interaction_state = app.state::<InteractionState>();
    let result = match claude_ask_user_question_payload(&input) {
        Ok(payload) => {
            wait_claude_decision(
                request_ask_user_question_scoped(app, &interaction_state, Some(conversation_id), payload,
                    Some(crate::api::ai::agent_driver::approval::AgentRequestScope::native(CLAUDE_SDK_API_TYPE, instance_id, conversation_id, run_id))),
                cancellation,
                cancel_generation,
            ).await.unwrap_or_else(|| Err("Claude Code prompt was cancelled".into()))
        }
        Err(error) => Err(error),
    };
    let (behavior, updated_input, message) = match result {
        Ok(answers) => ("allow", inject_answers_into_input(&input, &answers), Value::Null),
        Err(error) => {
            tracing::warn!(
                conversation_id,
                error = %error,
                "Claude Code AskUserQuestion was cancelled"
            );
            (
                "deny",
                input.clone(),
                json!({"message": format!("用户未回答提问：{error}")}),
            )
        }
    };
    let response = json!({
        "type": "control_response",
        "response": {"subtype":"success", "request_id": request_id, "response": {"behavior":behavior, "updatedInput": updated_input, "message":message}}
    });
    stdin.write_all(format!("{response}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

async fn handle_permission(
    app: &tauri::AppHandle,
    conversation_id: i64,
    frame: &Value,
    run_id: &str,
    instance_id: Option<i64>,
    stdin: &mut tokio::process::ChildStdin,
    cancellation: &mut watch::Receiver<u64>,
    cancel_generation: u64,
) -> Result<(), String> {
    let request_id =
        frame.get("request_id").and_then(Value::as_str).unwrap_or("unknown").to_string();
    let request = frame.get("request").cloned().unwrap_or(Value::Null);
    let tool_name = request.get("tool_name").and_then(Value::as_str).unwrap_or("Claude Code tool");
    if tool_name == "AskUserQuestion" {
        return handle_ask_user_question(
            app, conversation_id, &request, &request_id, instance_id, run_id, stdin, cancellation, cancel_generation,
        ).await;
    }
    let event = AcpPermissionRequestEvent {
        request_id: format!("claude:{conversation_id}:{run_id}:{request_id}"),
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
    let butler_guard = crate::mcp::builtin_mcp::superadmin::approval::NativeApprovalGuard::capture(
        app, conversation_id, crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).ok().flatten(),
        "claude.permission", serde_json::to_value(&event).map_err(|e| e.to_string())?,
    )?;
    state.store_request_guarded(event.clone(), tx,
        crate::api::ai::agent_driver::approval::AgentRequestScope::native(CLAUDE_SDK_API_TYPE, instance_id, conversation_id, run_id), butler_guard.clone()).await?;
    emit_permission_request_event(
        app,
        ACP_PERMISSION_REQUEST_EVENT,
        Some(conversation_id),
        &event,
    )?;
    let (decision, cancelled_resolution) = wait_claude_permission_decision(
        &state, &event.request_id, rx, cancellation, cancel_generation,
    ).await;
    if let Some(resolution) = cancelled_resolution {
        let _ = crate::api::operation_api::emit_permission_resolved_event(
            app,
            crate::api::operation_api::ACP_PERMISSION_RESOLVED_EVENT,
            &crate::api::operation_api::PermissionResolvedEvent {
                request_id: event.request_id.clone(),
                conversation_id: resolution.conversation_id,
            },
        );
    }
    let grants = match &decision {
        AcpPermissionDecision::Selected(option_id) =>
            crate::api::ai::agent_driver::approval::permission_option_requires_authorization(
                &event.options, option_id,
            ).unwrap_or(true),
        AcpPermissionDecision::Cancelled => false,
    };
    let decision = if let Some(guard) = butler_guard.filter(|_| grants) {
        match guard.validate(app) { Ok(()) => decision, Err(error) => {
            tracing::warn!(%error, "Butler native approval invalidated before Claude execution");
            AcpPermissionDecision::Cancelled
        }}
    } else { decision };
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

fn emit_update(identity: &AgentEventIdentity, window: &tauri::Window, conversation_id: i64, id: i64, content: &str, done: bool) {
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
    let _ = emit_to_window(
        window, identity.clone(),
        ConversationEvent {
            r#type: "message_update".into(),
            data: serde_json::to_value(data).unwrap(),
        },
    );
}

async fn emit_snapshot(
    identity: &AgentEventIdentity,
    app: &tauri::AppHandle,
    conversation_id: i64,
    state: ClaudeConversationSessionState,
) {
    let native_state = app.try_state::<crate::ClaudeSessionState>();
    let mut sessions = if let Some(native) = native_state.as_ref() { Some(native.sessions.lock().await) } else { None };
    let current_run = sessions.as_ref().and_then(|sessions| sessions.get(&conversation_id)).map(|entry| entry.run_id.as_str());
    if !super::agent_driver::events::can_publish_native_snapshot(current_run, identity.run_id.as_deref(), true) { return; }
    if let Some(entry) = sessions.as_mut().and_then(|sessions| sessions.get_mut(&conversation_id)) {
        entry.snapshot = state.clone();
        entry.touch();
    }
    let data = match super::agent_driver::capabilities::snapshot_event_payload(
        CLAUDE_SDK_API_TYPE, super::agent_driver::capabilities::AgentCapabilityDeclaration::claude(), Some(&state),
    ) {
        Ok(data) => data,
        Err(error) => {
            tracing::error!(conversation_id, error = %error, "Claude Code capability snapshot emission failed");
            return;
        }
    };
    let mut identity = identity.clone();
    identity.native_session_id = state.session_id.clone();
    let _ = emit_to_chat_windows(
        app,
        identity,
        ConversationEvent {
            r#type: "acp_session_state_snapshot".into(),
            data,
        },
    );
}

fn remove_claude_session_for_run(
    sessions: &mut HashMap<i64, ClaudeSessionEntry>,
    conversation_id: i64,
    run_id: &str,
) -> bool {
    if sessions.get(&conversation_id).is_some_and(|entry| entry.run_id == run_id) {
        sessions.remove(&conversation_id);
        true
    } else {
        false
    }
}

async fn cleanup_claude_session(
    app: &tauri::AppHandle,
    conversation_id: i64,
    run_id: &str,
    instance_id: Option<i64>,
) {
    let request_scope = crate::api::ai::agent_driver::approval::AgentRequestScope::native(
        CLAUDE_SDK_API_TYPE, instance_id, conversation_id, run_id,
    );
    crate::api::ai::agent_driver::approval::cancel_app_requests_for_scope(app, &request_scope).await;
    if let Some(state) = app.try_state::<crate::ClaudeSessionState>() {
        let mut sessions = state.sessions.lock().await;
        if remove_claude_session_for_run(&mut sessions, conversation_id, run_id) {
            let mut identity = AgentEventIdentity::new(CLAUDE_SDK_API_TYPE, conversation_id).with_run(run_id);
            identity.instance_id = instance_id.map(|id| id.to_string());
            let _ = emit_to_chat_windows(app, identity, ConversationEvent {
                r#type: "acp_session_state_snapshot".into(), data: json!({"state": Value::Null}),
            });
        }
    }
}

fn emit_claude_run_failure(
    app: &tauri::AppHandle, identity: &AgentEventIdentity, message_id: i64,
    window: &tauri::Window, cause: &str, stage: AgentFailureStage, process: AgentProcessDiagnostics,
) {
    let failure = AgentFailure::new(identity.clone().with_response_message(message_id), stage, cause)
        .with_process_diagnostics(process);
    if let Err(error) = emit_normalized_failure_to_window(app, window, failure, "Claude Code") {
        tracing::error!(conversation_id = identity.conversation_id, message_id, error = %error, "Claude Code failure publication incomplete");
    }
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
        AcpSessionConfigOptionPayload {
            id: "permission_mode".into(),
            name: "工作模式".into(),
            description: Some("控制 Claude Code 下一轮是先制定计划还是直接执行".into()),
            category: Some("mode".into()),
            current_value: config.permission_mode.clone().unwrap_or_else(|| "default".into()),
            options: [
                ("default", "执行模式"),
                ("plan", "Plan 模式"),
            ]
            .into_iter()
            .map(|(value, name)| AcpSessionConfigChoicePayload {
                value: value.into(),
                name: name.into(),
                description: None,
                group_name: None,
            })
            .collect(),
        },
    ]
}

fn claude_plan_from_tool_use(block: &Value) -> Option<Vec<AcpPlanEntryPayload>> {
    let tool_name = block.get("name").and_then(Value::as_str)?;
    if !matches!(tool_name, "TodoWrite" | "todo_write") {
        return None;
    }
    let todos = block.pointer("/input/todos")?.as_array()?;
    Some(
        todos
            .iter()
            .filter_map(|todo| {
                let content = todo.get("content").and_then(Value::as_str)?.to_string();
                let status = match todo.get("status").and_then(Value::as_str).unwrap_or("pending") {
                    "in_progress" | "inProgress" => "in_progress",
                    "completed" => "completed",
                    _ => "pending",
                };
                Some(AcpPlanEntryPayload {
                    content,
                    priority: "medium".into(),
                    status: status.into(),
                })
            })
            .collect(),
    )
}

fn claude_full_plan_from_tool_use(block: &Value) -> Option<&str> {
    let tool_name = block.get("name").and_then(Value::as_str)?;
    if !matches!(tool_name, "ExitPlanMode" | "exit_plan_mode") {
        return None;
    }
    block.pointer("/input/plan").and_then(Value::as_str).filter(|plan| !plan.trim().is_empty())
}

/// `system/compact_boundary` 帧：`/compact` 压缩完成。返回 Some(压缩前 token 数)。
pub(crate) fn claude_compact_boundary(frame: &Value) -> Option<Option<u64>> {
    (frame.get("subtype").and_then(Value::as_str) == Some("compact_boundary"))
        .then(|| frame.pointer("/compact_metadata/pre_tokens").and_then(Value::as_u64))
}

/// 流式正文为空时，用 result 帧补正文：result 文本 → 错误说明 → 本轮发生过压缩则写压缩说明。
pub(crate) fn claude_result_fallback_content(frame: &Value, compact_boundary: Option<Option<u64>>) -> String {
    if let Some(text) = frame.get("result").and_then(Value::as_str).filter(|text| !text.is_empty()) {
        return text.to_string();
    }
    if let Some(error) = frame.get("error").and_then(Value::as_str) {
        return format!("Claude Code 返回错误：{error}");
    }
    compact_boundary
        .map(crate::api::ai::agent_driver::manual_compact::compact_done_text)
        .unwrap_or_default()
}

fn claude_session_snapshot(
    conversation_id: i64,
    session_id: Option<String>,
    config: &ClaudeSdkConfig,
    has_active_prompt: bool,
    restored_session_method: Option<String>,
    connection_event_id: Option<String>,
    plan: &[AcpPlanEntryPayload],
) -> ClaudeConversationSessionState {
    ClaudeConversationSessionState {
        conversation_id,
        agent_kind: CLAUDE_SDK_API_TYPE.into(),
        session_id,
        has_active_prompt,
        model: config.model.clone(),
        permission_mode: config.permission_mode.clone(),
        load_session_supported: false,
        session_resume_supported: true,
        restored_session_method,
        connection_event_id,
        plan: plan.to_vec(),
        plan_explanation: None,
        config_options: claude_config_options(config),
    }
}

fn message_content_blocks(frame: &Value) -> &[Value] {
    frame
        .pointer("/message/content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn publish_claude_activities(
    app: &tauri::AppHandle, window: &tauri::Window, identity: &AgentEventIdentity,
    message_id: i64, activities: &mut ClaudeActivities, changes: Vec<Value>,
) -> Result<(), String> {
    if changes.is_empty() { return Ok(()); }
    let mut events = Vec::new();
    for mut activity in changes {
        let native_id = activity["item_id"].as_str().ok_or("Claude activity omitted item ID")?.to_string();
        activity["item_id"] = json!(format!("{message_id}:{native_id}"));
        activity["conversation_id"] = json!(identity.conversation_id);
        activity["response_message_id"] = json!(message_id);
        activity["agent_kind"] = json!(CLAUDE_SDK_API_TYPE);
        activity["session_id"] = json!(identity.native_session_id);
        activities.items.insert(native_id, activity.clone());
        events.push(activity);
    }
    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    let repo = db.message_repo().map_err(|e| e.to_string())?;
    persist_activities(&repo, message_id, &activities.items)?;
    for activity in events {
        emit_to_window(window, identity.clone(), ConversationEvent { r#type: "agent_activity".into(), data: activity })?;
    }
    Ok(())
}

async fn spawn_claude_process(
    config: &ClaudeSdkConfig,
    session_id: Option<&str>,
) -> Result<(
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
), String> {
    spawn_claude_process_with_diagnostics(config, session_id, AgentDiagnosticBuffer::default()).await
}

async fn spawn_claude_process_with_diagnostics(
    config: &ClaudeSdkConfig,
    session_id: Option<&str>,
    diagnostics: AgentDiagnosticBuffer,
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
                diagnostics.push(line.clone());
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

fn claude_result_error(frame: &Value) -> Option<String> {
    if frame.get("is_error").and_then(Value::as_bool) == Some(true)
        || frame.get("subtype").and_then(Value::as_str) == Some("error")
        || frame.get("error").is_some_and(|value| !value.is_null())
    {
        Some(frame.get("error").and_then(Value::as_str)
            .or_else(|| frame.get("result").and_then(Value::as_str))
            .unwrap_or("Claude Code 返回错误").to_string())
    } else {
        None
    }
}

pub(crate) fn confirm_claude_resume(
    expected_session_id: Option<&str>,
    actual_session_id: &str,
    run_id: &str,
) -> Result<Option<(String, String)>, String> {
    let Some(expected_session_id) = expected_session_id else {
        return Ok(None);
    };
    if actual_session_id != expected_session_id {
        return Err(format!(
            "Claude Code 恢复会话失败：请求恢复 session_id={expected_session_id}，CLI 返回 session_id={actual_session_id}"
        ));
    }
    Ok(Some(("resume".into(), format!("{run_id}:resume"))))
}

/// Approval waits observe cancellation independently of the actor command queue.
/// The generation belongs to the enqueued prompt, so an old idle cancellation
/// cannot cancel a later prompt, and a queued/startup cancellation is not lost.
async fn wait_claude_decision<F: std::future::Future>(
    decision: F,
    cancellation: &mut watch::Receiver<u64>,
    expected_generation: u64,
) -> Option<F::Output> {
    tokio::pin!(decision);
    loop {
        if *cancellation.borrow() != expected_generation {
            return None;
        }
        tokio::select! {
            biased;
            changed = cancellation.changed() => {
                if changed.is_err() {
                    return None;
                }
            }
            result = &mut decision => return Some(result),
        }
    }
}

async fn wait_claude_permission_decision(
    state: &AcpPermissionState,
    request_id: &str,
    decision: oneshot::Receiver<AcpPermissionDecision>,
    cancellation: &mut watch::Receiver<u64>,
    expected_generation: u64,
) -> (AcpPermissionDecision, Option<super::acp::AcpPermissionResolution>) {
    match wait_claude_decision(decision, cancellation, expected_generation).await {
        Some(result) => (result.unwrap_or(AcpPermissionDecision::Cancelled), None),
        None => {
            // The run-scoped ID avoids touching a replacement actor's request.
            let resolution = state.resolve_request(request_id, AcpPermissionDecision::Cancelled)
                .await.ok().flatten();
            (AcpPermissionDecision::Cancelled, resolution)
        }
    }
}

/// One step of the production active-turn transport loop. Keeping this boundary
/// independent of desktop event delivery lets protocol tests exercise the actual
/// native command receiver and child pipes, rather than reimplementing controls.
enum ClaudePromptInput {
    Frame(Value),
    Interrupted,
}

async fn read_claude_prompt_input(
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    commands: &mut mpsc::UnboundedReceiver<SessionCommand>,
) -> Result<ClaudePromptInput, String> {
    loop {
        let line = tokio::select! {
            next = lines.next_line() => match next {
                Ok(Some(line)) => line,
                Ok(None) => return Err("Claude Code 进程提前退出".into()),
                Err(error) => return Err(format!("读取 Claude Code 输出失败：{error}")),
            },
            command = commands.recv() => match command {
                Some(SessionCommand::Cancel { response }) => {
                    let request_id = uuid::Uuid::new_v4().to_string();
                    let interrupt = json!({"type":"control_request","request_id":request_id,"request":{"subtype":"interrupt"}});
                    let result = stdin.write_all(format!("{interrupt}\n").as_bytes()).await;
                    let _ = stdin.flush().await;
                    let _ = response.send(result.map_err(|error| error.to_string()));
                    return Ok(ClaudePromptInput::Interrupted);
                }
                Some(SessionCommand::Prompt { .. }) => continue,
                Some(SessionCommand::SetConfig { response, .. }) => {
                    let _ = response.send(Err("Claude Code 配置将在下一次会话启动时生效".into()));
                    continue;
                }
                None => return Err("Claude Code 会话在完成当前请求前已关闭".into()),
            }
        };
        // Preserve the CLI's existing tolerance of non-JSON diagnostic lines.
        if let Ok(frame) = serde_json::from_str::<Value>(&line) {
            return Ok(ClaudePromptInput::Frame(frame));
        }
    }
}

pub fn spawn_claude_session_task(
    app: tauri::AppHandle,
    _conversation_id: i64,
    mut config: ClaudeSdkConfig,
) -> ClaudeSessionHandle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let run_id = uuid::Uuid::new_v4().to_string();
    let task_run_id = run_id.clone();
    let (cancel_signal, mut cancellation) = watch::channel(0);
    let handle = ClaudeSessionHandle { sender: tx, cancel_signal, run_id };
    tauri::async_runtime::spawn(async move {
        let session_lookup = ConversationDatabase::new(&app)
            .map_err(|error| format!("Claude Code restore database unavailable: {error}"))
            .and_then(|db| db.get_agent_session_id_for_instance(_conversation_id, CLAUDE_SDK_API_TYPE, config.instance_id)
                .map_err(|error| format!("Claude Code persisted session lookup failed: {error}")));
        let stored_session = session_lookup.as_ref().ok().cloned().flatten();
        let startup_failure_stage = if session_lookup.is_err() { AgentFailureStage::Persistence }
            else if stored_session.is_some() { AgentFailureStage::Resume } else { AgentFailureStage::Initialize };
        let mut event_identity = AgentEventIdentity::new(CLAUDE_SDK_API_TYPE, _conversation_id).with_run(task_run_id.clone());
        event_identity.instance_id = config.instance_id.map(|id| id.to_string());
        event_identity.native_session_id = stored_session.clone();
        let mut diagnostics = AgentDiagnosticBuffer::for_environment(&config.env_vars);
        let startup = match session_lookup {
            Ok(_) => spawn_claude_process_with_diagnostics(&config, stored_session.as_deref(), diagnostics.clone()).await,
            Err(error) => Err(error),
        };
        let (mut child, mut stdin, mut lines) = match startup {
            Ok(process) => process,
            Err(error) => {
                while let Some(message) = rx.recv().await {
                    match message {
                        SessionCommand::Prompt { message_id, window, .. } => {
                            emit_claude_run_failure(&app, &event_identity, message_id, &window, &error,
                                startup_failure_stage, diagnostics.diagnostics(None, None));
                            if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                                manager.clear_agent_focus(&app, &event_identity, false).await;
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
                cleanup_claude_session(&app, _conversation_id, &task_run_id, config.instance_id).await;
                return;
            }
        };
        tracing::info!(
            conversation_id = _conversation_id,
            model = ?config.model,
            "Claude Code stream-json process started"
        );
        let mut session_id: Option<String> = stored_session.clone();
        let mut pending_resume_session_id = stored_session.clone();
        let mut restored_session_method: Option<String> = None;
        let mut connection_event_id: Option<String> = None;
        let mut current_plan = Vec::<AcpPlanEntryPayload>::new();
        emit_snapshot(&event_identity,
            &app,
            _conversation_id,
            claude_session_snapshot(
                _conversation_id,
                session_id.clone(),
                &config,
                false,
                None,
                None,
                &current_plan,
            ),
        )
        .await;
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
                        "permission_mode" => {
                            if !matches!(value.as_str(), "default" | "plan") {
                                let _ = response.send(Err(format!("Claude Code 不支持工作模式：{value}")));
                                continue;
                            }
                            config.permission_mode = Some(value)
                        }
                        _ => { let _ = response.send(Err(format!("Claude Code 不支持会话配置项: {config_id}"))); continue; }
                    }
                    let _ = child.kill().await;
                    diagnostics = AgentDiagnosticBuffer::for_environment(&config.env_vars);
                    match spawn_claude_process_with_diagnostics(&config, session_id.as_deref(), diagnostics.clone()).await {
                        Ok((new_child, new_stdin, new_lines)) => {
                            child = new_child;
                            stdin = new_stdin;
                            lines = new_lines;
                            pending_resume_session_id = session_id.clone();
                            restored_session_method = None;
                            connection_event_id = None;
                            let _ = response.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = response.send(Err(format!("重启 Claude Code 会话失败: {error}")));
                            break;
                        }
                    }
                    emit_snapshot(&event_identity,
                        &app,
                        _conversation_id,
                        claude_session_snapshot(
                            _conversation_id,
                            session_id.clone(),
                            &config,
                            false,
                            restored_session_method.clone(),
                            connection_event_id.clone(),
                            &current_plan,
                        ),
                    )
                    .await;
                }
                SessionCommand::Prompt { message_id, prompt, window, cancel_generation } => {
                    let _mcp_prompt = match config.mcp_scope.as_ref().map(|scope| scope.begin_prompt(message_id)).transpose() {
                        Ok(guard) => guard.map(|guard| guard.watch_cancellation(cancellation.clone(), cancel_generation)),
                        Err(error) => {
                            emit_claude_run_failure(&app, &event_identity, message_id, &window, &error,
                                AgentFailureStage::Mcp, diagnostics.diagnostics(None, None));
                            break;
                        }
                    };
                    event_identity.response_message_id = Some(message_id);
                    let input = json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":prompt}]},"session_id":session_id});
                    if let Err(error) = stdin.write_all(format!("{input}\n").as_bytes()).await {
                        emit_claude_run_failure(
                            &app,
                            &event_identity,
                            message_id,
                            &window,
                            &format!("写入 Claude Code 请求失败：{error}"), AgentFailureStage::SendPrompt, diagnostics.diagnostics(child.try_wait().ok().flatten().map(|status| status.to_string()), None));
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_agent_focus(&app, &event_identity, false).await;
                        }
                        break;
                    }
                    if let Err(error) = stdin.flush().await {
                        emit_claude_run_failure(
                            &app,
                            &event_identity,
                            message_id,
                            &window,
                            &format!("提交 Claude Code 请求失败：{error}"), AgentFailureStage::SendPrompt, diagnostics.diagnostics(child.try_wait().ok().flatten().map(|status| status.to_string()), None));
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_agent_focus(&app, &event_identity, false).await;
                        }
                        break;
                    }
                    if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                        manager.set_agent_streaming(&app, &event_identity, message_id).await;
                    }
                    let mut content = String::new();
                    let mut agent_plan_message: Option<AgentPlanMessage> = None;
                    let mut activities = ClaudeActivities::default();
                    let mut was_cancelled = false;
                    emit_snapshot(&event_identity,
                        &app,
                        _conversation_id,
                        claude_session_snapshot(
                            _conversation_id,
                            session_id.clone(),
                            &config,
                            true,
                            restored_session_method.clone(),
                            connection_event_id.clone(),
                            &current_plan,
                        ),
                    )
                    .await;
                    let mut usage = Value::Null;
                    // `/compact` 完成时 CLI 发 system/compact_boundary；记录下来，结果为空时写入压缩说明
                    let mut compact_boundary: Option<Option<u64>> = None;
                    let mut stream_error: Option<String> = None;
                    let mut stream_failure_stage = AgentFailureStage::Stream;
                    loop {
                        let frame = match read_claude_prompt_input(&mut stdin, &mut lines, &mut rx).await {
                            Ok(ClaudePromptInput::Frame(frame)) => frame,
                            Ok(ClaudePromptInput::Interrupted) => {
                                was_cancelled = true;
                                if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                                    manager.clear_agent_focus(&app, &event_identity, false).await;
                                }
                                continue;
                            }
                            Err(error) => {
                                stream_error = Some(error);
                                break;
                            }
                        };
                        if let Some(id) = frame.get("session_id").and_then(Value::as_str) {
                            match confirm_claude_resume(
                                pending_resume_session_id.as_deref(),
                                id,
                                &task_run_id,
                            ) {
                                Ok(Some((method, event_id))) => {
                                    restored_session_method = Some(method);
                                    connection_event_id = Some(event_id);
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    stream_failure_stage = AgentFailureStage::Resume;
                                    stream_error = Some(error);
                                    break;
                                }
                            }
                            if pending_resume_session_id.is_some() {
                                pending_resume_session_id = None;
                                emit_snapshot(&event_identity,
                                    &app,
                                    _conversation_id,
                                    claude_session_snapshot(
                                        _conversation_id,
                                        Some(id.into()),
                                        &config,
                                        true,
                                        restored_session_method.clone(),
                                        connection_event_id.clone(),
                                        &current_plan,
                                    ),
                                )
                                .await;
                            }
                            session_id = Some(id.into());
                            event_identity.native_session_id = session_id.clone();
                            let save_result = {
                                let state = app.state::<crate::ClaudeSessionState>();
                                let sessions = state.sessions.lock().await;
                                super::agent_driver::persistence::persist_session_id(&app, &event_identity,
                                    sessions.get(&_conversation_id).map(|entry| entry.run_id.as_str()), id)
                            };
                            if let Err(error) = save_result {
                                stream_failure_stage = AgentFailureStage::Persistence;
                                stream_error = Some(format!("Claude Code session persistence failed: {error}"));
                                break;
                            }
                        }
                        let changes = activities.consume(&frame, content.chars().count());
                        if let Err(error) = publish_claude_activities(&app, &window, &event_identity, message_id, &mut activities, changes) {
                            stream_failure_stage = AgentFailureStage::Persistence;
                            stream_error = Some(format!("Claude Code activity persistence/delivery failed [message_id={message_id}]: {error}"));
                            break;
                        }
                        match frame.get("type").and_then(Value::as_str).unwrap_or("") {
                            "system" => {
                                if let Some(pre_tokens) = claude_compact_boundary(&frame) {
                                    compact_boundary = Some(pre_tokens);
                                }
                                if let Some(model) = frame.get("model").and_then(Value::as_str) {
                                    config.model = Some(model.to_string());
                                }
                                if let Some(effort) = frame.get("effort").and_then(Value::as_str) {
                                    config.effort = Some(effort.to_string());
                                }
                                emit_snapshot(&event_identity,
                                    &app,
                                    _conversation_id,
                                    claude_session_snapshot(
                                        _conversation_id,
                                        session_id.clone(),
                                        &config,
                                        true,
                                        restored_session_method.clone(),
                                        connection_event_id.clone(),
                                        &current_plan,
                                    ),
                                )
                                .await;
                            }
                            "stream_event" => {
                                if let Some(delta) =
                                    frame.pointer("/event/delta/text").and_then(Value::as_str)
                                {
                                    content.push_str(delta);
                                    persist(&app, message_id, &content, false);
                                    emit_update(&event_identity,
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
                                    if let Some(plan) = claude_plan_from_tool_use(block) {
                                        current_plan = plan;
                                        emit_snapshot(&event_identity,
                                            &app,
                                            _conversation_id,
                                            claude_session_snapshot(
                                                _conversation_id,
                                                session_id.clone(),
                                                &config,
                                                true,
                                                restored_session_method.clone(),
                                                connection_event_id.clone(),
                                                &current_plan,
                                            ),
                                        )
                                        .await;
                                        continue;
                                    }
                                    if let Some(plan) = claude_full_plan_from_tool_use(block) {
                                        let item_id = block.get("id").and_then(Value::as_str);
                                        match AgentPlanMessage::create_with_identity(
                                            &app,
                                            &window,
                                            _conversation_id,
                                            message_id,
                                            CLAUDE_SDK_API_TYPE,
                                            session_id.as_deref(),
                                            None,
                                            item_id, event_identity.clone()) {
                                            Ok(mut plan_message) => {
                                            plan_message.replace_content(
                                                &app,
                                                &window,
                                                _conversation_id,
                                                plan.to_string(),
                                                true,
                                            );
                                            agent_plan_message = Some(plan_message);
                                            }
                                            Err(error) => {
                                                stream_failure_stage = AgentFailureStage::Persistence;
                                                stream_error = Some(format!("Claude Code Plan 已返回，但 AIPP 创建 Plan 消息失败: {error}"));
                                            }
                                        }
                                        continue;
                                    }

                                }
                            }
                            "control_request" => {
                                if let Err(error) =
                                    crate::api::butler_peer::TOOL_MESSAGE.scope(Some(message_id), handle_permission(
                                        &app, _conversation_id, &frame, &task_run_id, config.instance_id, &mut stdin,
                                        &mut cancellation, cancel_generation,
                                    )).await
                                {
                                    stream_failure_stage = AgentFailureStage::Permission;
                                    stream_error = Some(format!("Claude Code 权限响应失败：{error}"));
                                    break;
                                }
                            }
                            "result" => {
                                if let Some(error) = claude_result_error(&frame) {
                                    stream_error = Some(error);
                                    break;
                                }
                                if content.is_empty() {
                                    content = claude_result_fallback_content(&frame, compact_boundary);
                                }
                                usage = frame.get("usage").cloned().unwrap_or(Value::Null);
                                break;
                            }
                            _ => {}
                        }
                    }
                    let settled = activities.finish(was_cancelled, stream_error.as_deref());
                    if let Err(error) = publish_claude_activities(&app, &window, &event_identity, message_id, &mut activities, settled) {
                        stream_failure_stage = AgentFailureStage::Persistence;
                        stream_error = Some(format!("Claude Code final activity persistence/delivery failed [message_id={message_id}]: {error}"));
                    }
                    if let Some(error) = stream_error.as_deref() {
                        emit_claude_run_failure(&app, &event_identity, message_id, &window, error, stream_failure_stage, diagnostics.diagnostics(child.try_wait().ok().flatten().map(|status| status.to_string()), None));
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_agent_focus(&app, &event_identity, false).await;
                        }
                        break;
                    }
                    if content.is_empty() && agent_plan_message.is_none() && !activities.items.values().any(|item| item["kind"] == "reasoning" && item["output"].as_str().is_some_and(|text| !text.is_empty())) {
                        emit_claude_run_failure(
                            &app,
                            &event_identity,
                            message_id,
                            &window,
                            "Claude Code 本轮未返回任何内容", AgentFailureStage::Stream, diagnostics.diagnostics(child.try_wait().ok().flatten().map(|status| status.to_string()), None));
                        if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                            manager.clear_agent_focus(&app, &event_identity, false).await;
                        }
                        break;
                    }
                    persist(&app, message_id, &content, true);
                    if !usage.is_null() {
                        persist_usage(&app, message_id, &usage);
                    }
                    emit_update(&event_identity, &window, _conversation_id, message_id, &content, true);
                    let _ = emit_to_window(
                        &window, event_identity.clone(),
                        ConversationEvent {
                            r#type: "stream_complete".into(),
                            data: json!({
                                "conversation_id": _conversation_id,
                                "response_message_id": message_id,
                                "has_response": !content.is_empty(),
                            }),
                        },
                    );
                    let success_content = if content.is_empty() {
                        agent_plan_message.as_ref().map(|plan| plan.content.as_str()).unwrap_or_default()
                    } else {
                        content.as_str()
                    };
                    handle_agent_success(&app, &window, _conversation_id, success_content, message_id, AgentKind::ClaudeCode).await;
                    if let Some(manager) = app.try_state::<ConversationActivityManager>() {
                        manager.clear_agent_focus(&app, &event_identity, false).await;
                    }
                    emit_snapshot(&event_identity,
                        &app,
                        _conversation_id,
                        claude_session_snapshot(
                            _conversation_id,
                            session_id.clone(),
                            &config,
                            false,
                            restored_session_method.clone(),
                            connection_event_id.clone(),
                            &current_plan,
                        ),
                    )
                    .await;
                }
            }
        }
        cleanup_claude_session(&app, _conversation_id, &task_run_id, config.instance_id).await;
    });
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_compact_boundary_reads_pre_tokens() {
        let frame = json!({"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":52000}});
        assert_eq!(claude_compact_boundary(&frame), Some(Some(52000)));
        assert_eq!(claude_compact_boundary(&json!({"type":"system","subtype":"compact_boundary"})), Some(None));
        assert_eq!(claude_compact_boundary(&json!({"type":"system","subtype":"init"})), None);
    }

    #[test]
    fn test_claude_result_fallback_content_after_manual_compact() {
        let empty_result = json!({"type":"result","subtype":"success","result":""});
        assert_eq!(claude_result_fallback_content(&empty_result, Some(Some(52000))), "上下文已压缩（压缩前约 52000 tokens）。");
        assert_eq!(claude_result_fallback_content(&empty_result, None), "");
        let text_result = json!({"type":"result","subtype":"success","result":"done"});
        assert_eq!(claude_result_fallback_content(&text_result, Some(None)), "done");
        let error_result = json!({"type":"result","error":"boom"});
        assert_eq!(claude_result_fallback_content(&error_result, Some(None)), "Claude Code 返回错误：boom");
    }

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
    fn claude_command_contains_selected_permission_mode() {
        let mut config = extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap();
        config.permission_mode = Some("plan".into());
        let (command, _) = build_claude_command(&config, Some("session-1"));
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "plan"]));
        assert!(args.windows(2).any(|pair| pair == ["--resume", "session-1"]));
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
        assert_eq!(crate::api::ai::claude_activity::result_text(&result["content"]), "first\nsecond");
        assert_eq!(result.get("is_error").and_then(Value::as_bool), Some(true));
    }

    #[test]
    fn parses_claude_todo_write_as_plan_snapshot() {
        let plan = claude_plan_from_tool_use(&json!({
            "type": "tool_use",
            "name": "TodoWrite",
            "input": {
                "todos": [
                    {"content": "调查现状", "status": "completed"},
                    {"content": "实现交互", "status": "in_progress"},
                    {"content": "验证结果", "status": "waiting"}
                ]
            }
        }))
        .unwrap();
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].status, "completed");
        assert_eq!(plan[1].status, "in_progress");
        assert_eq!(plan[2].status, "pending");
        assert!(claude_plan_from_tool_use(&json!({
            "type": "tool_use",
            "name": "Read",
            "input": {}
        }))
        .is_none());
    }

    #[test]
    fn parses_claude_exit_plan_mode_body_separately_from_todos() {
        let frame = json!({
            "type": "tool_use",
            "id": "plan-1",
            "name": "ExitPlanMode",
            "input": {"plan": "# 实施方案\n\n1. 修改后端"}
        });
        let plan = claude_full_plan_from_tool_use(&frame);
        assert_eq!(plan, Some("# 实施方案\n\n1. 修改后端"));
        assert!(claude_full_plan_from_tool_use(&json!({
            "type": "tool_use",
            "name": "TodoWrite",
            "input": {"plan": "不是完整 Plan"}
        }))
        .is_none());
    }

    #[test]
    fn test_claude_ask_user_question_payload_maps_questions() {
        let input = json!({
            "questions": [
                {
                    "question": "选择发布环境？",
                    "header": "环境",
                    "multiSelect": false,
                    "options": [
                        {"label": "staging", "description": "预发环境"},
                        {"label": "prod"}
                    ]
                },
                {
                    "question": "需要哪些通知渠道？",
                    "multiSelect": true,
                    "options": [
                        {"label": "邮件", "description": "email"},
                        {"label": "飞书", "description": "feishu"}
                    ]
                }
            ]
        });
        let payload = claude_ask_user_question_payload(&input).unwrap();
        assert_eq!(payload.questions.len(), 2);
        assert_eq!(payload.questions[0].header, "环境");
        assert!(!payload.questions[0].multi_select);
        assert_eq!(payload.questions[0].options.len(), 2);
        assert_eq!(payload.questions[0].options[1].label, "prod");
        // 缺 description 时填充兜底文案，避免校验失败
        assert_eq!(payload.questions[0].options[1].description, "选择此项");
        assert_eq!(payload.questions[1].header, "Claude Code");
        assert!(payload.questions[1].multi_select);
        payload.validate().unwrap();

        // 缺少 questions 时必须报错而不是降级
        assert!(claude_ask_user_question_payload(&json!({})).is_err());
    }

    #[test]
    fn test_claude_ask_user_question_updated_input_injects_answers() {
        // 用户作答后，answers 以 问题文本 -> 答案 的形式写回 updatedInput
        let input = json!({
            "questions": [{"question": "继续吗？", "header": "确认", "options": []}]
        });
        let mut answers = HashMap::new();
        answers.insert("继续吗？".to_string(), "继续".to_string());
        let updated = inject_answers_into_input(&input, &answers);
        assert_eq!(
            updated.pointer("/answers/继续吗？").and_then(Value::as_str),
            Some("继续")
        );
        assert!(updated.get("questions").is_some());
    }
}

#[cfg(test)]
#[path = "claude_sdk_lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(all(test, unix))]
#[path = "claude_sdk_protocol_tests.rs"]
mod protocol_tests;

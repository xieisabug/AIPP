use crate::db::conversation_db::MessageAttachment;
use crate::api::ai::agent_completion::{handle_agent_success, AgentKind};
use crate::api::ai::agent_plan::AgentPlanMessage;
use crate::api::ai::agent_driver::events::{AgentEventIdentity, emit_to_window, emit_to_chat_windows};
use crate::api::ai::agent_driver::failure::{AgentFailure, AgentFailureStage, AgentProcessDiagnostics, emit_normalized_failure_to_window};
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::api::ai::acp::{
    AcpPermissionDecision, AcpPermissionOptionPayload, AcpPermissionRequestEvent,
    AcpPermissionState, AcpPlanEntryPayload,
};
use crate::api::operation_api::{
    emit_permission_request_event, emit_permission_resolved_event, PermissionResolvedEvent,
    ACP_PERMISSION_REQUEST_EVENT, ACP_PERMISSION_RESOLVED_EVENT,
};
use crate::acp_mcp_bridge::{
    ensure_proxy_server, AcpMcpProxyConfig, ACP_MCP_BRIDGE_ARG, ACP_MCP_CONVERSATION_ID_ENV,
    ACP_MCP_DB_PATH_ENV, ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV, ACP_MCP_PROXY_ADDR_ENV,
    ACP_MCP_PROXY_TOKEN_ENV, ACP_MCP_SELECTED_TOOLS_ENV, ACP_MCP_SELECTED_TOOLS_FILE_ENV,
};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use crate::mcp::builtin_mcp::interaction::{
    request_ask_user_question_scoped, AskUserQuestionItem, AskUserQuestionMetadata,
    AskUserQuestionOption, AskUserQuestionRequest, InteractionState,
};
use crate::state::activity_state::ConversationActivityManager;
use super::official_cli;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, error, info, warn};

pub const CODEX_APP_SERVER_API_TYPE: &str = "codex_app_server";

#[path = "codex_app_server_elicitation.rs"]
mod elicitation;

#[derive(Debug, Clone)]
pub struct CodexAppServerConfig {
    /// Provider-row identity captured when this native runtime is prepared.
    pub instance_id: Option<i64>,
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub additional_args: Vec<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub approvals_reviewer: Option<String>,
    pub collaboration_mode: Option<String>,
    pub developer_instructions: Option<String>,
    pub selected_mcp_tools_payload: String,
    pub session_signature: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexConversationSessionState {
    pub conversation_id: i64,
    pub agent_kind: String,
    pub session_id: Option<String>,
    pub load_session_supported: bool,
    pub session_resume_supported: bool,
    pub restored_session_method: Option<String>,
    pub connection_event_id: Option<String>,
    pub current_turn_id: Option<String>,
    pub has_active_prompt: bool,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub approvals_reviewer: Option<String>,
    pub collaboration_mode: Option<String>,
    pub plan: Vec<AcpPlanEntryPayload>,
    pub plan_explanation: Option<String>,
    pub config_options: Vec<CodexSessionConfigOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionConfigOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub current_value: String,
    pub options: Vec<CodexSessionConfigChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionConfigChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
    pub group_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivityEvent {
    pub conversation_id: i64,
    pub response_message_id: i64,
    pub agent_kind: String,
    pub session_id: Option<String>,
    pub item_id: String,
    pub sequence: u64,
    #[serde(default)]
    pub order: Option<u64>,
    pub kind: String,
    pub status: String,
    pub title: Option<String>,
    pub input: Option<Value>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub metadata: Value,
    /// item 开始时已输出的正文字符数（按 Unicode 字符计），用于前端把活动卡片穿插到正文对应位置；
    /// 旧数据没有该字段时前端回退为「全部列在正文之后」
    pub content_offset: Option<u64>,
}

enum CodexSessionCommand {
    Steer { turn: String, text: String, images: Vec<MessageAttachment>, response: oneshot::Sender<Result<(), String>> },
    Prompt { message_id: i64, prompt: String, images: Vec<MessageAttachment>, window: tauri::Window, cancellation_generation: u64 },
    CancelCurrentPrompt { response: oneshot::Sender<Result<(), String>> },
    SetConfigOption { config_id: String, value: String, response: oneshot::Sender<Result<(), String>> },
    Shutdown { reason: String },
}

type CodexStderrBuffer = std::sync::Arc<std::sync::Mutex<VecDeque<String>>>;

const CODEX_STDERR_MAX_LINES: usize = 40;

/// 剥离 ANSI 转义序列（CSI/OSC 等），避免 Codex 彩色 tracing 日志在用户界面显示为乱码
fn strip_ansi_escapes(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            // CSI: ESC [ + 参数字节，以 0x40-0x7E 的结束字节收尾
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ESC ] + 内容，以 BEL 或 ESC \ 收尾
            Some(']') => {
                let mut prev_was_esc = false;
                for c in chars.by_ref() {
                    if c == '\u{7}' || (prev_was_esc && c == '\\') {
                        break;
                    }
                    prev_was_esc = c == '\u{1b}';
                }
            }
            // 其他 ESC + 单字符序列；孤立的末尾 ESC 一并丢弃
            Some(_) | None => {}
        }
    }
    output
}

/// 判断剥离 ANSI 后的行是否为 tracing fmt 的 TRACE/DEBUG/INFO 级噪音（如 span 的 enter/exit）。
/// WARN/ERROR 以及不符合 tracing 格式的行（panic、进程原生输出）一律保留，保证错误可追溯。
fn is_tracing_noise(line: &str) -> bool {
    // tracing fmt 默认格式：`<RFC3339 时间戳>  <LEVEL> <spans>: <target>: <message>`
    let Some(timestamp_end) = line.find(char::is_whitespace) else {
        return false;
    };
    let timestamp = &line[..timestamp_end];
    let bytes = timestamp.as_bytes();
    let looks_like_rfc3339 = bytes.len() >= 20
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && timestamp.ends_with('Z');
    if !looks_like_rfc3339 {
        return false;
    }
    let level = line[timestamp_end..].trim_start();
    ["TRACE", "DEBUG", "INFO"].iter().any(|token| {
        level
            .strip_prefix(token)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

#[derive(Clone)]
pub struct CodexSessionHandle {
    sender: mpsc::UnboundedSender<CodexSessionCommand>,
    run_id: String,
    cancellation: watch::Sender<u64>,
    failure_context: std::sync::Arc<std::sync::Mutex<Option<(i64, tauri::Window)>>>,
}

impl CodexSessionHandle {
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
                    CodexSessionCommand::CancelCurrentPrompt { response } => TestSessionCommand::Cancel { response },
                    CodexSessionCommand::SetConfigOption { config_id, value, response } => {
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
            run_id: "driver-test-run".into(),
            cancellation: watch::channel(0).0,
            failure_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }, observations)
    }

    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        images: Vec<MessageAttachment>,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        *self.failure_context.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some((message_id, window.clone()));
        if self
            .sender
            .send(CodexSessionCommand::Prompt { message_id, prompt, images, window, cancellation_generation: *self.cancellation.borrow() })
            .is_err()
        {
            self.failure_context
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            return Err(AppError::UnknownError(
                format!(
                    "无法提交 Codex 请求：app-server 控制通道已关闭 [run_id={}]",
                    self.run_id
                ),
            ));
        }
        Ok(())
    }

    pub async fn steer(&self, turn: String, text: String, images: Vec<MessageAttachment>) -> Result<(), AppError> {
        let (response, receipt) = oneshot::channel();
        self.sender.send(CodexSessionCommand::Steer { turn, text, images, response })
            .map_err(|_| AppError::ProviderError(format!("Codex steer channel closed [run={}]", self.run_id)))?;
        tokio::time::timeout(std::time::Duration::from_secs(35), receipt).await
            .map_err(|_| AppError::ProviderError(format!("Codex turn/steer acknowledgement timed out [run={}]", self.run_id)))?
            .map_err(|_| AppError::ProviderError(format!("Codex steer receipt lost [run={}]", self.run_id)))?
            .map_err(AppError::ProviderError)
    }

    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        self.cancellation.send_modify(|generation| *generation = generation.wrapping_add(1));
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(CodexSessionCommand::CancelCurrentPrompt { response: tx })
            .map_err(|_| AppError::UnknownError(format!(
                "无法取消 Codex 请求：app-server 控制通道已关闭 [run_id={}]",
                self.run_id
            )))?;
        rx.await
            .map_err(|_| AppError::UnknownError(format!(
                "Codex 取消请求未返回结果：会话任务已退出 [run_id={}]",
                self.run_id
            )))?
            .map_err(AppError::UnknownError)
    }

    pub async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(CodexSessionCommand::SetConfigOption { config_id, value, response: tx })
            .map_err(|_| AppError::UnknownError(format!(
                "无法更新 Codex 会话配置：app-server 控制通道已关闭 [run_id={}]",
                self.run_id
            )))?;
        rx.await.map_err(|_| AppError::UnknownError(format!(
            "Codex 配置更新未返回结果：会话任务已退出 [run_id={}]",
            self.run_id
        )))?
            .map_err(AppError::UnknownError)
    }

    pub(crate) fn shutdown(&self, reason: String) {
        self.cancellation.send_modify(|generation| *generation = generation.wrapping_add(1));
        let _ = self.sender.send(CodexSessionCommand::Shutdown { reason });
    }
}

pub struct CodexSessionEntry {
    pub handle: CodexSessionHandle,
    pub snapshot: CodexConversationSessionState,
    pub config_signature: String,
    pub run_id: String,
    pub last_activity: Instant,
}

impl CodexSessionEntry {
    pub fn new(handle: CodexSessionHandle, conversation_id: i64, config_signature: String) -> Self {
        Self {
            run_id: handle.run_id.clone(),
            handle,
            snapshot: CodexConversationSessionState {
                conversation_id,
                agent_kind: CODEX_APP_SERVER_API_TYPE.to_string(),
                ..Default::default()
            },
            config_signature,
            last_activity: Instant::now(),
        }
    }

    pub(crate) fn reuse_handle(&mut self, config_signature: &str) -> Option<CodexSessionHandle> {
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

pub fn extract_codex_app_server_config(
    model_configs: &[crate::db::assistant_db::AssistantModelConfig],
    provider_configs: &[crate::db::llm_db::LLMProviderConfig],
    model: Option<String>,
) -> Result<CodexAppServerConfig, AppError> {
    let provider_value = |name: &str| {
        provider_configs
            .iter()
            .find(|config| config.name == name)
            .map(|config| config.value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let model_value = |name: &str| {
        model_configs
            .iter()
            .find(|config| config.name == name)
            .and_then(|config| config.value.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    let cli_command = provider_value("codex_cli_command").unwrap_or_else(|| "codex".to_string());
    let working_directory = super::working_directory::from_configs(model_configs, provider_configs, false)?;
    // 与 ACP 通道一致：assistant_model_config（助手级）优先于 provider 默认值
    let additional_args = model_value("acp_additional_args")
        .or_else(|| provider_value("codex_additional_args"))
        .map(|raw| raw.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let mut env_vars = HashMap::new();
    if let Some(home) = dirs::home_dir() {
        let codex_home = home.join(".codex");
        if codex_home.exists() {
            env_vars.insert("CODEX_HOME".to_string(), codex_home.display().to_string());
        }
    }
    for raw in [provider_value("acp_env_vars"), model_value("acp_env_vars")].into_iter().flatten() {
        super::acp::merge_acp_env_blob(&mut env_vars, &raw);
    }
    // 审批策略/沙箱模式的默认值与 provider 表单默认值保持一致，
    // 保证存量 provider（从未保存过这两项配置）也能在界面上反显生效值
    let approval_policy = model_value("codex_approval_policy")
        .or_else(|| provider_value("codex_approval_policy"))
        .or_else(|| Some("on-request".to_string()));
    let sandbox = model_value("codex_sandbox")
        .or_else(|| provider_value("codex_sandbox"))
        .or_else(|| Some("workspace-write".to_string()));
    if let Some(value) = approval_policy.as_deref() {
        if !CODEX_APPROVAL_POLICIES.contains(&value) {
            return Err(AppError::UnknownError(format!("Codex 不支持审批策略：{value}")));
        }
    }
    if let Some(value) = sandbox.as_deref() {
        if !CODEX_SANDBOX_MODES.contains(&value) {
            return Err(AppError::UnknownError(format!("Codex 不支持沙箱模式：{value}")));
        }
    }
    // 未显式指定审批人时，由 Codex 使用自己的配置。
    let approvals_reviewer = model_value("codex_approvals_reviewer")
        .or_else(|| provider_value("codex_approvals_reviewer"));
    if let Some(value) = approvals_reviewer.as_deref() {
        if !CODEX_APPROVALS_REVIEWERS.contains(&value) {
            return Err(AppError::UnknownError(format!("Codex 不支持审批人：{value}")));
        }
    }
    let mut config = CodexAppServerConfig {
        instance_id: None,
        cli_command,
        working_directory,
        env_vars,
        additional_args,
        model,
        reasoning_effort: model_value("reasoning_effort"),
        approval_policy,
        sandbox,
        approvals_reviewer,
        collaboration_mode: None,
        developer_instructions: None,
        selected_mcp_tools_payload: String::new(),
        session_signature: String::new(),
    };
    config.session_signature = codex_config_signature(&config);
    Ok(config)
}

fn codex_config_signature(config: &CodexAppServerConfig) -> String {
    serde_json::to_string(&json!({
        "instance_id": config.instance_id,
        "cli": config.cli_command,
        "cwd": config.working_directory,
        "args": config.additional_args,
        "model": config.model,
        "reasoning_effort": config.reasoning_effort,
        "approval": config.approval_policy,
        "sandbox": config.sandbox,
        "approvals_reviewer": config.approvals_reviewer,
        "collaboration_mode": config.collaboration_mode,
        "env": config.env_vars,
        "developer_instructions": config.developer_instructions,
        "selected_mcp": config.selected_mcp_tools_payload,
    }))
    .unwrap_or_default()
}

pub fn refresh_codex_session_signature(config: &mut CodexAppServerConfig) {
    config.session_signature = codex_config_signature(config);
}

async fn emit_session_snapshot(
    identity: &AgentEventIdentity,
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    state: Option<CodexConversationSessionState>,
) {
    let native_state = app_handle.try_state::<crate::CodexSessionState>();
    let mut sessions = if let Some(native) = native_state.as_ref() { Some(native.sessions.lock().await) } else { None };
    let current_run = sessions.as_ref().and_then(|sessions| sessions.get(&conversation_id)).map(|entry| entry.run_id.as_str());
    if !super::agent_driver::events::can_publish_native_snapshot(current_run, identity.run_id.as_deref(), state.is_some()) { return; }
    if let Some(entry) = sessions.as_mut().and_then(|sessions| sessions.get_mut(&conversation_id)) {
        if let Some(state_for_cache) = &state { entry.snapshot = state_for_cache.clone(); entry.touch(); }
    }
    let data = match super::agent_driver::capabilities::snapshot_event_payload(
        CODEX_APP_SERVER_API_TYPE, super::agent_driver::capabilities::AgentCapabilityDeclaration::codex(), state.as_ref(),
    ) {
        Ok(data) => data,
        Err(error) => {
            tracing::error!(conversation_id, error = %error, "Codex capability snapshot emission failed");
            return;
        }
    };
    let event = ConversationEvent {
        r#type: "acp_session_state_snapshot".to_string(),
        data,
    };
    let mut identity = identity.clone();
    if let Some(state) = &state {
        identity.native_session_id = state.session_id.clone();
        identity.turn_id = state.current_turn_id.clone();
    }
    let _ = emit_to_chat_windows(app_handle, identity, event);
}

fn json_rpc_request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

/// 构造 Codex thread 的 MCP config 覆盖：把 AIPP 桥接进程注册为名为 `aipp` 的 MCP server。
/// 通过 thread/start 与 thread/resume 的 `config` 字段下发（Codex 0.149 已验证），
/// 不修改用户 ~/.codex/config.toml；与 ACP 通道共用同一桥接与代理。
#[cfg(test)]
fn build_codex_mcp_config_overrides(
    bridge_command: &std::path::Path,
    mcp_db_path: &std::path::Path,
    conversation_id: i64,
    proxy: &AcpMcpProxyConfig,
    selected_mcp_tools_payload: &str,
) -> serde_json::Map<String, Value> {
    build_codex_mcp_config_overrides_with_env(
        bridge_command,
        mcp_db_path,
        conversation_id,
        proxy,
        ACP_MCP_SELECTED_TOOLS_ENV,
        selected_mcp_tools_payload,
    )
}

fn build_codex_mcp_config_overrides_with_env(
    bridge_command: &std::path::Path,
    mcp_db_path: &std::path::Path,
    conversation_id: i64,
    proxy: &AcpMcpProxyConfig,
    selected_tools_env_key: &str,
    selected_tools_env_value: &str,
) -> serde_json::Map<String, Value> {
    let mut env = serde_json::Map::new();
    env.insert(ACP_MCP_DB_PATH_ENV.to_string(), json!(mcp_db_path.display().to_string()));
    env.insert(ACP_MCP_CONVERSATION_ID_ENV.to_string(), json!(conversation_id.to_string()));
    env.insert(ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV.to_string(), json!("1"));
    env.insert(ACP_MCP_PROXY_ADDR_ENV.to_string(), json!(proxy.addr));
    env.insert(ACP_MCP_PROXY_TOKEN_ENV.to_string(), json!(proxy.token));
    env.insert(selected_tools_env_key.to_string(), json!(selected_tools_env_value));
    let mut overrides = serde_json::Map::new();
    overrides.insert("mcp_servers.aipp.command".to_string(), json!(bridge_command.display().to_string()));
    overrides.insert("mcp_servers.aipp.args".to_string(), json!([ACP_MCP_BRIDGE_ARG]));
    overrides.insert("mcp_servers.aipp.env".to_string(), Value::Object(env));
    overrides.insert("mcp_servers.aipp.startup_timeout_sec".to_string(), json!(20));
    overrides
}

/// 助手选中了 MCP 工具时生成 thread 级 config 覆盖；未选中时不挂载桥接
async fn codex_mcp_bridge_overrides(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    config: &CodexAppServerConfig,
    mcp_scope: &crate::acp_mcp_bridge::provenance::NativeMcpScope,
) -> Result<Option<serde_json::Map<String, Value>>, String> {
    let payload = config.selected_mcp_tools_payload.trim();
    if payload.is_empty() || payload == "[]" {
        return Ok(None);
    }
    let bridge_command = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve AIPP executable for Codex MCP bridge: {error}"))?;
    let mcp_db_path = MCPDatabase::db_path(app_handle)?;
    let mut proxy = ensure_proxy_server(app_handle.clone()).await?;
    proxy.token = mcp_scope.token().to_string();
    let selected_tools_file = mcp_scope.attach_selected_tools_file(&config.selected_mcp_tools_payload)?;
    let mut overrides = build_codex_mcp_config_overrides_with_env(
        &bridge_command,
        &mcp_db_path,
        conversation_id,
        &proxy,
        ACP_MCP_SELECTED_TOOLS_FILE_ENV,
        &selected_tools_file.display().to_string(),
    );
    if let Some(seconds) = crate::acp_mcp_bridge::native_answer_window(app_handle, payload)? {
        overrides.insert("mcp_servers.aipp.tool_timeout_sec".into(), json!(seconds));
    }
    Ok(Some(overrides))
}

fn rpc_id_key(id: &Value) -> String {
    id.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string())
}

fn codex_permission_options(method: &str, params: &Value) -> Vec<AcpPermissionOptionPayload> {
    if method == elicitation::METHOD {
        // MCP approval is per call. Never infer persistent approval from a form.
        let mut options = codex_permission_options("item/commandExecution/requestApproval",
            &json!({"availableDecisions": ["accept", "decline", "cancel"]}));
        options[2].name = "取消本次调用".into();
        options[2].kind = "reject_once".into();
        return options;
    }
    let available = params.get("availableDecisions").and_then(Value::as_array);
    let mut decisions = available
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if available.is_none() {
        decisions = if method == "item/permissions/requestApproval" {
            vec!["accept", "acceptForSession", "decline"]
        } else {
            vec!["accept", "acceptForSession", "decline", "cancel"]
        };
    }
    let mut options: Vec<_> = decisions
        .into_iter()
        .filter(|value| matches!(*value, "accept" | "acceptForSession" | "cancel" | "decline"))
        .map(|decision| AcpPermissionOptionPayload {
            option_id: decision.to_string(),
            name: match decision {
                "accept" => "本次允许",
                "acceptForSession" => "本会话允许",
                "decline" => "拒绝并继续",
                "cancel" => "拒绝并中止本轮",
                other => other,
            }
            .to_string(),
            kind: match decision {
                "accept" => "allow_once",
                "acceptForSession" => "allow_always",
                "cancel" => "reject_always",
                _ => "reject_once",
            }
            .to_string(),
        })
        .collect();
    if method == "item/commandExecution/requestApproval" {
        for (index, decision) in available.into_iter().flatten().enumerate() {
            if let Some(prefix) = decision.pointer("/acceptWithExecpolicyAmendment/execpolicy_amendment").and_then(Value::as_array) {
                if !prefix.is_empty() && prefix.iter().all(Value::is_string) {
                    options.push(AcpPermissionOptionPayload {
                        option_id: format!("execpolicy:{index}"),
                        name: format!("允许并记住命令前缀：{}", prefix.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" · ")),
                        kind: "allow_always".into(),
                    });
                }
            }
        }
    }
    options
}

fn codex_approval_response(method: &str, params: &Value, decision: AcpPermissionDecision) -> Value {
    if method == elicitation::METHOD {
        return elicitation::approval_response(decision);
    }
    let selected = match decision {
        AcpPermissionDecision::Selected(value) => value,
        AcpPermissionDecision::Cancelled => "cancel".to_string(),
    };
    if method == "item/commandExecution/requestApproval" {
        if let Some(index) = selected.strip_prefix("execpolicy:").and_then(|index| index.parse::<usize>().ok()) {
            if codex_permission_options(method, params).iter().any(|option| option.option_id == selected) {
                return json!({"decision": params["availableDecisions"][index]});
            }
        }
    }
    if method == "item/permissions/requestApproval" {
        if selected == "accept" || selected == "acceptForSession" {
            json!({
                "permissions": params.get("permissions").cloned().unwrap_or_else(|| json!({})),
                "scope": if selected == "acceptForSession" { "session" } else { "turn" }
            })
        } else {
            json!({"permissions": {}})
        }
    } else {
        // Unknown selections cancel the request.
        let supported_method = matches!(method,
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval");
        let selected = if supported_method
            && matches!(selected.as_str(), "accept" | "acceptForSession" | "decline" | "cancel")
        {
            selected.as_str()
        } else {
            warn!(method, decision = %selected, "Unsupported Codex approval decision; cancelling request");
            "cancel"
        };
        json!({"decision": selected})
    }
}

/// Cancellation generations belong to the prompt at enqueue time, so an idle
/// Stop cannot cancel a later prompt, and Stop during startup is not lost.
struct CodexRequestCancellation {
    mcp_prompt: Option<crate::acp_mcp_bridge::provenance::CapturedNativeMcpPrompt>,
    source_message_id: Option<i64>,
    instance_id: Option<i64>,
    signal: watch::Sender<u64>,
    generation: u64,
    run_id: String,
}

impl CodexRequestCancellation {
    async fn cancelled(&self) {
        let mut receiver = self.signal.subscribe();
        loop {
            if *receiver.borrow_and_update() != self.generation {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

async fn wait_codex_interaction<T>(
    interaction: impl std::future::Future<Output = Result<T, String>>,
    cancellation: Option<&CodexRequestCancellation>,
) -> Result<T, String> {
    match cancellation {
        Some(cancellation) => tokio::select! {
            biased;
            () = cancellation.cancelled() => Err("Codex interaction cancelled by Stop".to_string()),
            result = interaction => result,
        },
        None => interaction.await,
    }
}

async fn wait_codex_approval_decision(
    permission_state: &AcpPermissionState,
    request_id: &str,
    decision_rx: oneshot::Receiver<AcpPermissionDecision>,
    cancellation: Option<&CodexRequestCancellation>,
) -> (AcpPermissionDecision, bool) {
    let result = wait_codex_interaction(async {
        Ok(decision_rx.await.unwrap_or(AcpPermissionDecision::Cancelled))
    }, cancellation).await;
    match result {
        Ok(decision) => (decision, false),
        Err(_) => {
            // IDs contain the run identity: never clear another run/conversation.
            let _ = permission_state.resolve_request(request_id, AcpPermissionDecision::Cancelled).await;
            (AcpPermissionDecision::Cancelled, true)
        }
    }
}

async fn handle_approval_request(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    frame: &Value,
    cancellation: Option<&CodexRequestCancellation>,
) -> Result<bool, String> {
    let Some(method) = frame.get("method").and_then(Value::as_str) else {
        return Ok(false);
    };
    if !matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    ) && !elicitation::is_tool_approval(frame)? {
        return Ok(false);
    }
    let rpc_id = frame.get("id").cloned().ok_or("Codex approval request missing id")?;
    let params = frame.get("params").cloned().unwrap_or(Value::Null);
    let item_id = params
        .get("itemId")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let request_id = match cancellation {
        Some(context) => format!("codex:{conversation_id}:{}:{}", context.run_id, rpc_id_key(&rpc_id)),
        None => format!("codex:{conversation_id}:{}", rpc_id_key(&rpc_id)),
    };
    let title = params
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| params.get("reason").and_then(Value::as_str).map(str::to_string))
        .or_else(|| params.get("message").and_then(Value::as_str).map(str::to_string))
        .or_else(|| {
            Some(match method {
                "item/fileChange/requestApproval" => "允许 Codex 修改文件",
                "item/permissions/requestApproval" => "允许 Codex 扩展权限",
                _ => "允许 Codex 执行命令",
            }
            .to_string())
        });
    let event = AcpPermissionRequestEvent {
        request_id: request_id.clone(),
        conversation_id: Some(conversation_id),
        agent_kind: Some(CODEX_APP_SERVER_API_TYPE.to_string()),
        tool_call_id: item_id,
        title,
        kind: Some(method.to_string()),
        parameters: Some(serde_json::to_string_pretty(&params).unwrap_or_else(|_| params.to_string())),
        options: codex_permission_options(method, &params),
    };
    let (decision_tx, decision_rx) = oneshot::channel();
    let permission_state = app_handle.state::<AcpPermissionState>();
    let scope = crate::api::ai::agent_driver::approval::AgentRequestScope {
        driver_id: Some(CODEX_APP_SERVER_API_TYPE.into()),
        instance_id: cancellation.and_then(|context| context.instance_id),
        conversation_id: Some(conversation_id),
        run_id: cancellation.map(|context| context.run_id.clone()),
    };
    let butler_guard = crate::mcp::builtin_mcp::superadmin::approval::NativeApprovalGuard::capture(
        app_handle, conversation_id, cancellation.and_then(|c| c.source_message_id),
        "codex.permission", serde_json::to_value(&event).map_err(|e| e.to_string())?,
    )?;
    let (is_proactive, review_turn) = {
        let conversation = ConversationDatabase::new(app_handle)
            .map_err(|error| format!("Codex 审批连接数据库失败：{error}"))?
            .conversation_repo()
            .map_err(|error| format!("Codex 审批打开会话库失败：{error}"))?
            .read(conversation_id)
            .map_err(|error| format!("Codex 审批读取会话 {conversation_id} 失败：{error}"))?
            .ok_or_else(|| format!("Codex 审批会话 {conversation_id} 不存在"))?;
        let is_proactive = conversation.conversation_kind == crate::proactive::tools::PROACTIVE_RUN_KIND;
        let turn = cancellation.and_then(|context| context.source_message_id);
        let review_turn = if is_proactive {
            Some(turn.unwrap_or(0))
        } else if let Some(turn) = turn {
            if crate::mcp::tool_review::effective_review_mode(
                app_handle, conversation.assistant_id.unwrap_or_default(), conversation_id,
            ) == "auto_model" {
                Some(turn)
            } else {
                None
            }
        } else {
            None
        };
        (is_proactive, review_turn)
    };
    permission_state.store_request_guarded(event.clone(), decision_tx, scope.clone(), butler_guard.clone()).await?;
    let exempt = super::agent_driver::native_agents::approve_exempt_mcp_call(
        app_handle, &event, &scope, cancellation.and_then(|c| c.mcp_prompt.as_ref()),
        crate::acp_mcp_bridge::permission::codex_call(method, &params),
    ).await;
    if let Some(turn) = review_turn.filter(|_| !exempt) {
        if is_proactive {
            tauri::async_runtime::spawn(super::agent_driver::native_agents::review_proactive_permission(
                app_handle.clone(), event.clone(), scope, turn, "Codex",
                params["cwd"].as_str().unwrap_or("未提供").to_string(),
            ));
        } else {
            tauri::async_runtime::spawn(super::agent_driver::native_agents::review_permission(
                app_handle.clone(), event.clone(), scope, turn, "Codex",
                params["cwd"].as_str().unwrap_or("未提供").to_string(),
            ));
        }
    } else if !exempt {
        permission_state.mark_presented(&request_id, json!(event)).await;
        if let Err(error) = emit_permission_request_event(
            app_handle, ACP_PERMISSION_REQUEST_EVENT, Some(conversation_id), &event,
        ) {
            permission_state.remove_request(&request_id).await;
            return Err(error);
        }
    }
    let (decision, cancelled_by_control) = wait_codex_approval_decision(
        &permission_state, &request_id, decision_rx, cancellation,
    ).await;
    if cancelled_by_control {
        let _ = emit_permission_resolved_event(
            app_handle, ACP_PERMISSION_RESOLVED_EVENT,
            &PermissionResolvedEvent { request_id, conversation_id: Some(conversation_id) },
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
        match guard.validate(app_handle) { Ok(()) => decision, Err(error) => {
            warn!(%error, "Butler native approval invalidated before Codex execution");
            AcpPermissionDecision::Cancelled
        }}
    } else { decision };
    let result = codex_approval_response(method, &params, decision);
    write_frame(stdin, &json!({"jsonrpc":"2.0","id":rpc_id,"result":result})).await?;
    Ok(true)
}

fn supported_reasoning_efforts(value: &Value) -> Vec<String> {
    let mut result = Vec::new();
    fn visit(value: &Value, result: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key.eq_ignore_ascii_case("supportedReasoningEfforts") {
                        if let Some(items) = child.as_array() {
                            for item in items {
                                if let Some(name) = item.as_str() {
                                    if !result.iter().any(|existing| existing == name) {
                                        result.push(name.to_string());
                                    }
                                } else if let Some(name) = item.get("effort").or_else(|| item.get("reasoningEffort")).and_then(Value::as_str) {
                                    if !result.iter().any(|existing| existing == name) {
                                        result.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                    visit(child, result);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| visit(item, result)),
            _ => {}
        }
    }
    visit(value, &mut result);
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexModelCatalogEntry {
    pub id: String,
    pub name: String,
    pub supported_efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexModelProbe {
    pub models: Vec<CodexModelCatalogEntry>,
    pub default_model: Option<String>,
    pub default_effort: Option<String>,
}

fn model_catalog(value: &Value) -> Vec<CodexModelCatalogEntry> {
    let mut models = Vec::new();
    fn visit(value: &Value, models: &mut Vec<CodexModelCatalogEntry>) {
        match value {
            Value::Object(map) => {
                let id = map.get("id").or_else(|| map.get("model")).and_then(Value::as_str);
                let label = map.get("displayName").or_else(|| map.get("name")).and_then(Value::as_str);
                if let Some(id) = id {
                    if map.contains_key("supportedReasoningEfforts") || label.is_some() {
                        let entry = CodexModelCatalogEntry {
                            id: id.to_string(),
                            name: label.unwrap_or(id).to_string(),
                            supported_efforts: supported_reasoning_efforts(value),
                            default_effort: map.get("defaultReasoningEffort").and_then(Value::as_str).map(str::to_string),
                            is_default: map.get("isDefault").and_then(Value::as_bool).unwrap_or(false),
                        };
                        if !models.iter().any(|existing| existing.id == entry.id) {
                            models.push(entry);
                        }
                    }
                }
                map.values().for_each(|child| visit(child, models));
            }
            Value::Array(items) => items.iter().for_each(|item| visit(item, models)),
            _ => {}
        }
    }
    visit(value, &mut models);
    models
}

fn codex_config_model_defaults(value: &Value) -> (Option<String>, Option<String>) {
    let config = value.get("config").unwrap_or(value);
    (
        config.get("model").and_then(Value::as_str).map(str::to_string),
        config
            .get("model_reasoning_effort")
            .and_then(Value::as_str)
            .map(str::to_string),
    )
}

/// 启动一次短生命周期 Codex app-server，读取生效配置并探测真实的 model/list 目录。
/// 不创建 thread，也不会写入会话数据库。
pub async fn probe_codex_model_options(
    app_handle: &tauri::AppHandle,
    config: &CodexAppServerConfig,
) -> Result<CodexModelProbe, String> {
    codex_model_catalog(app_handle, config, false).await
}

pub(super) async fn codex_model_catalog(
    app_handle: &tauri::AppHandle, config: &CodexAppServerConfig, check_account: bool,
) -> Result<CodexModelProbe, String> {
    let stderr_buffer = CodexStderrBuffer::default();
    let result = async {
        let (_child, mut stdin, mut lines) =
            spawn_process(config, stderr_buffer.clone()).await?;
        let mut pending_notifications = VecDeque::new();
        let requests = DesktopCodexServerRequests { app_handle, conversation_id: -1, cancellation: None };
        initialize_codex_transport(&requests, -1, "aipp-model-probe", &mut stdin, &mut lines, &mut pending_notifications).await?;
        if check_account {
            write_frame(&mut stdin, &json_rpc_request(4, "account/read", json!({"refreshToken": false}))).await?;
            let account = read_rpc_response_with_approvals(app_handle, -1, &mut stdin, &mut lines, 4, &mut pending_notifications, None)
                .await.map_err(|e| format!("Codex account/read 失败：{e}"))?;
            if account["requiresOpenaiAuth"] == true && account["account"].is_null() {
                return Err("Codex 尚未登录，请先配置账号".to_owned());
            }
        }
        write_frame(
            &mut stdin,
            &json_rpc_request(
                2,
                "config/read",
                json!({
                    "cwd": config.working_directory,
                    "includeLayers": false,
                }),
            ),
        )
        .await?;
        let config_result = read_rpc_response_with_approvals(
            app_handle,
            -1,
            &mut stdin,
            &mut lines,
            2,
            &mut pending_notifications,
            None,
        )
        .await
        .map_err(|error| format!("Codex config/read 失败：{error}"))?;
        let (configured_model, configured_effort) = codex_config_model_defaults(&config_result);

        let mut catalog = Vec::new();
        let mut seen_cursors = std::collections::HashSet::new();
        let mut cursor: Option<String> = None;
        let mut request_id = 5;
        loop {
            let params = match cursor {
                Some(ref value) => json!({ "cursor": value }),
                None => json!({}),
            };
            write_frame(&mut stdin, &json_rpc_request(request_id, "model/list", params)).await?;
            let page = read_rpc_response_with_approvals(
                app_handle, -1, &mut stdin, &mut lines, request_id, &mut pending_notifications, None,
            ).await.map_err(|error| format!("Codex model/list 失败：{error}"))?;
            catalog.extend(model_catalog(&page));
            let Some(next) = page["nextCursor"].as_str().filter(|s| !s.is_empty()) else { break; };
            if !seen_cursors.insert(next.to_owned()) {
                return Err("Codex model/list 返回重复的分页游标".to_owned());
            }
            cursor = Some(next.to_owned());
            request_id += 1;
        }
        if catalog.is_empty() {
            return Err("Codex model/list 返回空模型列表".to_owned());
        }
        let default_model = configured_model.or_else(|| {
            catalog
                .iter()
                .find(|entry| entry.is_default)
                .map(|entry| entry.id.clone())
        });
        let default_effort = configured_effort.or_else(|| {
            default_model.as_deref().and_then(|model_id| {
                catalog
                    .iter()
                    .find(|entry| entry.id == model_id)
                    .and_then(|entry| entry.default_effort.clone())
            })
        });
        Ok(CodexModelProbe {
            models: catalog,
            default_model,
            default_effort,
        })
    }
    .await;
    result.map_err(|error| {
        codex_error_with_diagnostics(&error, -1, "model-probe", &stderr_buffer)
    })
}

struct LightRequests;

#[async_trait::async_trait]
impl CodexServerRequests for LightRequests {
    async fn handle(&self, _: &mut tokio::process::ChildStdin, frame: &Value) -> Result<bool, String> {
        Err(format!("轻量模型请求了交互操作 {}", frame["method"]))
    }
}

pub(super) async fn light_text(config: &CodexAppServerConfig, model: Option<&str>, prompt: &str) -> Result<super::light_model::Reply, String> {
    let stderr = CodexStderrBuffer::default();
    let result = async {
        let (_child, mut stdin, mut lines) = spawn_process(config, stderr.clone()).await?;
        let mut pending = VecDeque::new();
        initialize_codex_transport(&LightRequests, -1, "aipp-light-model", &mut stdin, &mut lines, &mut pending).await?;
        write_frame(&mut stdin, &json_rpc_request(2, "thread/start", json!({
            "model":model, "cwd":config.working_directory, "ephemeral":true, "sandbox":"read-only",
            "approvalPolicy":"never", "allowProviderModelFallback":false, "selectedCapabilityRoots":[],
            "baseInstructions":"Complete the supplied text task directly. Do not use tools.",
            "config":{"features.shell_tool":false, "features.unified_exec":false, "mcp_servers":{}, "web_search":"disabled"}
        }))).await?;
        let started = read_codex_rpc_response(&LightRequests, -1, &mut stdin, &mut lines, 2, &mut pending).await?;
        let thread = started.pointer("/thread/id").and_then(Value::as_str).ok_or("thread/start 缺少 thread.id")?;
        write_frame(&mut stdin, &json_rpc_request(3, "turn/start", json!({
            "threadId":thread, "model":model, "input":[{"type":"text","text":prompt}]
        }))).await?;
        let started = read_codex_rpc_response(&LightRequests, -1, &mut stdin, &mut lines, 3, &mut pending).await?;
        let turn = started.pointer("/turn/id").and_then(Value::as_str).ok_or("turn/start 缺少 turn.id")?;
        let mut text = String::new();
        let mut usage = CodexTurnUsageTracker::default();
        loop {
            let frame = match pending.pop_front() {
                Some(frame) => frame,
                None => serde_json::from_str(&lines.next_line().await.map_err(|e| e.to_string())?
                    .ok_or("Codex 在 turn/completed 前关闭了 stdout")?).map_err(|e| format!("Codex JSON: {e}"))?,
            };
            if frame.get("id").is_some() && frame.get("method").is_some() { LightRequests.handle(&mut stdin, &frame).await?; }
            let params = &frame["params"];
            if params["threadId"] != thread { continue; }
            usage.note_notification(frame["method"].as_str().unwrap_or_default(), params);
            match frame["method"].as_str() {
                Some("thread/tokenUsage/updated") => usage.observe(params),
                Some("item/completed") if params["turnId"] == turn && params["item"]["type"] == "agentMessage" => {
                    text = params["item"]["text"].as_str().unwrap_or_default().to_owned();
                }
                Some("turn/completed") if params["turn"]["id"] == turn => {
                    return if params["turn"]["status"] == "completed" {
                        let usage = usage.turn_usage().map(|u| super::light_model::reported_usage(u.input_tokens.into(), u.output_tokens.into(),
                            Some(u.cached_input_tokens.into()), Some(u.cache_write_input_tokens.into()), Some(u.reasoning_tokens.into()))).unwrap_or_default();
                        Ok(super::light_model::Reply { text, usage })
                    }
                        else { Err(format!("turn {turn} {}：{}", params["turn"]["status"], params["turn"]["error"])) };
                }
                _ => {}
            }
        }
    }.await;
    result.map_err(|e| codex_error_with_diagnostics(&e, -1, "light-model", &stderr))
}

/// Read the provider profile's account limits without opening a thread.
pub(crate) async fn probe_account_limits(
    app: &tauri::AppHandle,
    config: &CodexAppServerConfig,
) -> Result<Value, String> {
    let stderr = CodexStderrBuffer::default();
    let read = async {
        let (_child, mut stdin, mut lines) = spawn_process(config, stderr.clone()).await?;
        let requests = DesktopCodexServerRequests { app_handle: app, conversation_id: -1, cancellation: None };
        let mut pending = VecDeque::new();
        initialize_codex_transport(&requests, -1, "aipp-quota", &mut stdin, &mut lines, &mut pending).await?;
        write_frame(&mut stdin, &json_rpc_request(2, "account/rateLimits/read", Value::Null)).await?;
        read_codex_rpc_response(&requests, -1, &mut stdin, &mut lines, 2, &mut pending).await
    };
    tokio::time::timeout(Duration::from_secs(25), read).await
        .unwrap_or_else(|_| Err("启动或等待额度响应超过 25 秒".into()))
        .map_err(|e| codex_error_with_diagnostics(&e, -1, "account/rateLimits/read", &stderr))
}

fn first_string_for_keys(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(Value::as_str) {
                    return Some(value.to_string());
                }
            }
            map.values().find_map(|child| first_string_for_keys(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|item| first_string_for_keys(item, keys)),
        _ => None,
    }
}

fn codex_reasoning_options(efforts: &[String]) -> Vec<CodexSessionConfigChoice> {
    efforts.iter().map(|value| CodexSessionConfigChoice {
        value: value.clone(),
        name: match value.as_str() {
            "none" => "关闭思考".to_string(),
            other => other.to_string(),
        },
        description: None,
        group_name: None,
    }).collect()
}

/// Codex 审批策略的合法取值（app-server v2 AskForApproval 枚举）
pub(crate) const CODEX_APPROVAL_POLICIES: &[&str] = &["untrusted", "on-request", "never"];
/// Codex 沙箱模式的合法取值（app-server v2 SandboxMode 枚举）
pub(crate) const CODEX_SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];
/// Codex 审批人的合法取值（app-server v2 ApprovalsReviewer 枚举；guardian_subagent 为遗留别名，不暴露）
pub(crate) const CODEX_APPROVALS_REVIEWERS: &[&str] = &["user", "auto_review"];

fn codex_approval_policy_options() -> Vec<CodexSessionConfigChoice> {
    CODEX_APPROVAL_POLICIES.iter().map(|value| CodexSessionConfigChoice {
        value: value.to_string(),
        name: match *value {
            "untrusted" => "仅读命令自动执行（untrusted）".to_string(),
            "on-request" => "由模型决定何时请求审批（on-request）".to_string(),
            "never" => "从不请求审批（never）".to_string(),
            other => other.to_string(),
        },
        description: None,
        group_name: None,
    }).collect()
}

fn codex_sandbox_options() -> Vec<CodexSessionConfigChoice> {
    CODEX_SANDBOX_MODES.iter().map(|value| CodexSessionConfigChoice {
        value: value.to_string(),
        name: match *value {
            "read-only" => "只读（read-only）".to_string(),
            "workspace-write" => "工作区可写（workspace-write）".to_string(),
            "danger-full-access" => "完全访问（danger-full-access）".to_string(),
            other => other.to_string(),
        },
        description: None,
        group_name: None,
    }).collect()
}

fn codex_approvals_reviewer_options() -> Vec<CodexSessionConfigChoice> {
    CODEX_APPROVALS_REVIEWERS.iter().map(|value| CodexSessionConfigChoice {
        value: value.to_string(),
        name: match *value {
            "user" => "人工审批（user）".to_string(),
            "auto_review" => "自动审批（auto_review）".to_string(),
            other => other.to_string(),
        },
        description: None,
        group_name: None,
    }).collect()
}

fn codex_collaboration_mode_options() -> Vec<CodexSessionConfigChoice> {
    [
        ("default", "执行模式", "允许 Codex 调查、修改文件并执行工具"),
        ("plan", "Plan 模式", "只调查、澄清并形成可确认的实施计划"),
    ]
    .into_iter()
    .map(|(value, name, description)| CodexSessionConfigChoice {
        value: value.to_string(),
        name: name.to_string(),
        description: Some(description.to_string()),
        group_name: None,
    })
    .collect()
}

fn codex_supports_plan_mode(value: &Value) -> bool {
    value
        .get("data")
        .and_then(Value::as_array)
        .is_some_and(|modes| {
            modes.iter().any(|mode| {
                mode.get("mode").and_then(Value::as_str) == Some("plan")
            })
        })
}

fn codex_collaboration_mode_json(snapshot: &CodexConversationSessionState) -> Option<Value> {
    let mode = snapshot.collaboration_mode.as_deref()?;
    Some(json!({
        "mode": mode,
        "settings": {
            "model": snapshot.model.clone().unwrap_or_default(),
            "reasoning_effort": snapshot.reasoning_effort,
            "developer_instructions": null,
        }
    }))
}

fn codex_plan_payload(params: &Value) -> Vec<AcpPlanEntryPayload> {
    params
        .get("plan")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let content = entry.get("step").and_then(Value::as_str)?.to_string();
            let status = match entry.get("status").and_then(Value::as_str).unwrap_or("pending") {
                "inProgress" => "in_progress",
                "completed" => "completed",
                _ => "pending",
            };
            Some(AcpPlanEntryPayload {
                content,
                priority: "medium".to_string(),
                status: status.to_string(),
            })
        })
        .collect()
}

/// turn/start 的 sandboxPolicy 是对象形式（camelCase type），
/// 与 thread/start|resume 的 SandboxMode 字符串（kebab-case）不同，这里做映射
fn codex_sandbox_policy_json(sandbox: Option<&str>) -> Option<Value> {
    let policy_type = match sandbox? {
        "read-only" => "readOnly",
        "workspace-write" => "workspaceWrite",
        "danger-full-access" => "dangerFullAccess",
        _ => return None,
    };
    Some(json!({ "type": policy_type }))
}

fn codex_model_choices(
    catalog: &[CodexModelCatalogEntry],
) -> Vec<CodexSessionConfigChoice> {
    catalog.iter().map(|entry| CodexSessionConfigChoice {
        value: entry.id.clone(),
        name: entry.name.clone(),
        description: None,
        group_name: None,
    }).collect()
}

fn apply_codex_config_option(
    snapshot: &mut CodexConversationSessionState,
    catalog: &[CodexModelCatalogEntry],
    config_id: &str,
    value: String,
) -> Result<(), String> {
    if config_id == "model" {
        let model = catalog.iter().find(|model| model.id == value)
            .ok_or_else(|| format!("Codex 模型不在 model/list 返回结果中：{value}"))?;
        snapshot.model = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "model") {
            option.current_value = value;
        }
        let current_effort_supported = snapshot.reasoning_effort.as_ref()
            .is_some_and(|effort| model.supported_efforts.iter().any(|candidate| candidate == effort));
        if !current_effort_supported {
            snapshot.reasoning_effort = model.default_effort.clone().or_else(|| model.supported_efforts.first().cloned());
        }
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "reasoning_effort") {
            option.options = codex_reasoning_options(&model.supported_efforts);
            option.current_value = snapshot.reasoning_effort.clone().unwrap_or_default();
        }
        return Ok(());
    }
    if config_id == "reasoning_effort" || config_id == "thought_level" {
        let model = snapshot.model.as_ref().and_then(|id| catalog.iter().find(|model| &model.id == id));
        if !model.is_some_and(|model| model.supported_efforts.iter().any(|candidate| candidate == &value)) {
            return Err(format!("当前 Codex 模型不支持思考强度：{value}"));
        }
        snapshot.reasoning_effort = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "reasoning_effort") {
            option.current_value = value;
        }
        return Ok(());
    }
    if config_id == "approval_policy" {
        if !CODEX_APPROVAL_POLICIES.contains(&value.as_str()) {
            return Err(format!("Codex 不支持审批策略：{value}"));
        }
        snapshot.approval_policy = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "approval_policy") {
            option.current_value = value;
        }
        return Ok(());
    }
    if config_id == "sandbox" {
        if !CODEX_SANDBOX_MODES.contains(&value.as_str()) {
            return Err(format!("Codex 不支持沙箱模式：{value}"));
        }
        snapshot.sandbox = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "sandbox") {
            option.current_value = value;
        }
        return Ok(());
    }
    if config_id == "approvals_reviewer" {
        if !CODEX_APPROVALS_REVIEWERS.contains(&value.as_str()) {
            return Err(format!("Codex 不支持审批人：{value}"));
        }
        snapshot.approvals_reviewer = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "approvals_reviewer") {
            option.current_value = value;
        }
        return Ok(());
    }
    if config_id == "collaboration_mode" {
        if !matches!(value.as_str(), "default" | "plan") {
            return Err(format!("Codex 不支持协作模式：{value}"));
        }
        if !snapshot
            .config_options
            .iter()
            .any(|option| option.id == "collaboration_mode")
        {
            return Err("当前 Codex app-server 未提供 Plan 模式".to_string());
        }
        snapshot.collaboration_mode = Some(value.clone());
        if let Some(option) = snapshot
            .config_options
            .iter_mut()
            .find(|option| option.id == "collaboration_mode")
        {
            option.current_value = value;
        }
        return Ok(());
    }
    Err(format!("Codex 不支持会话配置：{config_id}"))
}

// Codex truncates individual additionalContext entries above 4,000 UTF-8 bytes.
fn codex_application_context(mut text: &str) -> serde_json::Map<String, Value> {
    let mut entries = serde_json::Map::new();
    while !text.is_empty() {
        let mut end = text.len().min(4000);
        while !text.is_char_boundary(end) { end -= 1; }
        if end < text.len() {
            // Keep paragraphs and individual rules together when they fit.
            if let Some(newline) = text[..end].rfind('\n') { end = newline + 1; }
        }
        let (part, rest) = text.split_at(end);
        entries.insert(format!("aipp_butler_{:08}", entries.len()), json!({"kind":"application","value":part}));
        text = rest;
    }
    entries
}

// The legacy event omits the item ID. Match both event streams by their
// turn-local ordinal so either can arrive first without injecting twice.
#[derive(Default)]
struct CodexContextRestore {
    completed_items: std::collections::HashSet<String>,
    legacy_completions: usize,
    sent: usize,
    pending: std::collections::HashSet<u64>,
}

impl CodexContextRestore {
    async fn on_notification(
        &mut self, method: &str, params: &Value, manual_compact: bool, thread_id: &str,
        context: &serde_json::Map<String, Value>, request_id: &mut u64,
        stdin: &mut (impl tokio::io::AsyncWrite + Unpin),
    ) -> Result<(), String> {
        if context.is_empty() { return Ok(()); }
        let count = match method {
            "item/completed" if params.pointer("/item/type").and_then(Value::as_str) == Some("contextCompaction") => {
                let id = params.pointer("/item/id").and_then(Value::as_str)
                    .ok_or_else(|| format!("Codex thread {thread_id}: contextCompaction item/completed omitted item.id"))?;
                self.completed_items.insert(id.to_owned());
                self.completed_items.len()
            }
            "thread/compacted" => {
                self.legacy_completions += 1;
                self.legacy_completions
            }
            // Explicit /compact may finish without a separate completion item.
            "turn/completed" if manual_compact && params.pointer("/turn/status").and_then(Value::as_str) == Some("completed") => 1,
            _ => return Ok(()),
        };
        if count <= self.sent { return Ok(()); }
        self.sent = count;
        *request_id += 1;
        self.pending.insert(*request_id);
        let items: Vec<_> = context.iter().map(|(key, entry)| {
            let value = entry["value"].as_str().unwrap_or_default();
            json!({"type":"message","role":"developer","content":[{"type":"input_text","text":format!("<{key}>{value}</{key}>")}]})
        }).collect();
        write_frame(stdin, &json_rpc_request(*request_id, "thread/inject_items", json!({"threadId":thread_id,"items":items}))).await
            .map_err(|e| format!("Codex thread {thread_id} 压缩后发送管家指令失败：{e}"))
    }

    fn acknowledge(&mut self, thread_id: &str, frame: &Value) -> Result<(), String> {
        if frame["id"].as_u64().is_some_and(|id| self.pending.remove(&id)) {
            if let Some(error) = frame.get("error") {
                return Err(format!("Codex thread {thread_id} 压缩后恢复管家指令失败：{error}"));
            }
        }
        Ok(())
    }
}

async fn codex_image_items(images: Vec<MessageAttachment>) -> Result<Vec<Value>, String> {
    Ok(crate::utils::prompt_image::agent_images(images).await?.into_iter().map(|image| {
        json!({"type":"image", "url": format!("data:{};base64,{}", image.mime_type, image.data)})
    }).collect())
}

fn codex_turn_start_params(thread_id: &str, snapshot: &CodexConversationSessionState, prompt: &str, context: &serde_json::Map<String, Value>) -> Value {
    let mut params = json!({
        "threadId": thread_id,
        "model": snapshot.model,
        "reasoningEffort": snapshot.reasoning_effort,
        "approvalPolicy": snapshot.approval_policy,
        "sandboxPolicy": codex_sandbox_policy_json(snapshot.sandbox.as_deref()),
        "approvalsReviewer": snapshot.approvals_reviewer,
        "collaborationMode": codex_collaboration_mode_json(snapshot),
        "input": [{"type":"text","text":prompt,"text_elements":[]}],
    });
    if !context.is_empty() { params["additionalContext"] = json!(context); }
    params
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CodexTurnUsage {
    input_tokens: i32,
    output_tokens: i32,
    reasoning_tokens: i32,
    cached_input_tokens: i32,
    cache_write_input_tokens: i32,
    total_tokens: i32,
}

fn codex_user_input_request(
    params: &Value,
) -> Result<(AskUserQuestionRequest, Vec<(String, String, bool)>), String> {
    let questions = params
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("Codex requestUserInput is missing questions")?;
    let mut mappings = Vec::with_capacity(questions.len());
    let mut converted = Vec::with_capacity(questions.len());
    for question in questions {
        let id = question
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Codex requestUserInput question is missing id")?
            .to_string();
        let text = question
            .get("question")
            .and_then(Value::as_str)
            .ok_or("Codex requestUserInput question is missing question")?
            .to_string();
        let header = question
            .get("header")
            .and_then(Value::as_str)
            .unwrap_or("Codex")
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
        mappings.push((id, text.clone(), multi_select));
        converted.push(AskUserQuestionItem {
            question: text,
            header,
            options,
            multi_select,
            is_secret: question
                .get("isSecret")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Ok((
        AskUserQuestionRequest {
            questions: converted,
            answers: None,
            metadata: Some(AskUserQuestionMetadata {
                source: Some("codex_app_server".to_string()),
            }),
        },
        mappings,
    ))
}

fn codex_user_input_response(
    mappings: &[(String, String, bool)],
    answers: &HashMap<String, String>,
) -> Value {
    let answers = mappings
        .iter()
        .filter_map(|(id, question, multi_select)| {
            let answer = answers.get(question)?.trim();
            if answer.is_empty() {
                return None;
            }
            let values = if *multi_select {
                answer
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else {
                vec![answer.to_string()]
            };
            Some((id.clone(), json!({"answers": values})))
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"answers": answers})
}

async fn handle_server_request(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    frame: &Value,
    cancellation: Option<&CodexRequestCancellation>,
) -> Result<bool, String> {
    if handle_approval_request(app_handle, conversation_id, stdin, frame, cancellation).await? {
        return Ok(true);
    }
    if elicitation::handle_form(app_handle, conversation_id, stdin, frame, cancellation).await? {
        return Ok(true);
    }
    if frame.get("method").and_then(Value::as_str) != Some("item/tool/requestUserInput") {
        return Ok(false);
    }
    let rpc_id = frame.get("id").cloned().ok_or("Codex requestUserInput missing id")?;
    let params = frame.get("params").cloned().unwrap_or(Value::Null);
    let (request, mappings) = codex_user_input_request(&params)?;
    let interaction_state = app_handle.state::<InteractionState>();
    // Dropping the request future invokes its existing exact-request cleanup
    // guard; do not clear all conversation questions or consume control commands.
    let result = match wait_codex_interaction(crate::api::butler_peer::TOOL_MESSAGE.scope(cancellation.and_then(|c|c.source_message_id),request_ask_user_question_scoped(
        app_handle,
        &interaction_state,
        Some(conversation_id),
        request,
        Some(crate::api::ai::agent_driver::approval::AgentRequestScope {
            driver_id: Some(CODEX_APP_SERVER_API_TYPE.into()),
            instance_id: cancellation.and_then(|context| context.instance_id),
            conversation_id: Some(conversation_id),
            run_id: cancellation.map(|context| context.run_id.clone()),
        }),
    )), cancellation).await
    {
        Ok(answers) => codex_user_input_response(&mappings, &answers),
        Err(error) => {
            warn!(conversation_id, error = %error, "Codex requestUserInput was cancelled");
            json!({"answers": {}})
        }
    };
    write_frame(stdin, &json!({"jsonrpc":"2.0","id":rpc_id,"result":result})).await?;
    Ok(true)
}

async fn write_frame(stdin: &mut (impl tokio::io::AsyncWrite + Unpin), frame: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(frame)
        .map_err(|error| format!("序列化 Codex JSON-RPC 请求失败：{error}"))?;
    bytes.push(b'\n');
    stdin
        .write_all(&bytes)
        .await
        .map_err(|error| format!("写入 Codex app-server stdin 失败：{error}"))?;
    stdin
        .flush()
        .await
        .map_err(|error| format!("刷新 Codex app-server stdin 失败：{error}"))
}

/// Only the desktop interaction boundary is substituted in protocol tests.
/// The JSON-RPC reader, handshake and native thread RPCs below are shared with
/// run_session; tests do not implement a second driver/session loop.
#[async_trait::async_trait]
trait CodexServerRequests: Sync {
    async fn handle(&self, stdin: &mut tokio::process::ChildStdin, frame: &Value) -> Result<bool, String>;
}

struct DesktopCodexServerRequests<'a> {
    app_handle: &'a tauri::AppHandle,
    conversation_id: i64,
    cancellation: Option<&'a CodexRequestCancellation>,
}

#[async_trait::async_trait]
impl CodexServerRequests for DesktopCodexServerRequests<'_> {
    async fn handle(&self, stdin: &mut tokio::process::ChildStdin, frame: &Value) -> Result<bool, String> {
        handle_server_request(self.app_handle, self.conversation_id, stdin, frame, self.cancellation).await
    }
}

async fn read_rpc_response_with_approvals(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: u64,
    pending_notifications: &mut VecDeque<Value>,
    cancellation: Option<&CodexRequestCancellation>,
) -> Result<Value, String> {
    let requests = DesktopCodexServerRequests { app_handle, conversation_id, cancellation };
    read_codex_rpc_response(&requests, conversation_id, stdin, lines, expected_id, pending_notifications).await
}

async fn read_codex_rpc_response(
    requests: &impl CodexServerRequests,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: u64,
    pending_notifications: &mut VecDeque<Value>,
) -> Result<Value, String> {
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| format!("读取 Codex app-server stdout 失败：{error}"))?
    {
        let frame: Value = serde_json::from_str(&line)
            .map_err(|error| format!("Invalid Codex app-server JSON: {error}: {line}"))?;
        if frame.get("method").is_some() && frame.get("id").is_some() {
            if !requests.handle(stdin, &frame).await? {
                warn!(
                    conversation_id,
                    method = frame.get("method").and_then(|value| value.as_str()).unwrap_or("unknown"),
                    "Unsupported Codex server request"
                );
                write_frame(
                    stdin,
                    &json!({
                        "jsonrpc":"2.0",
                        "id":frame.get("id").cloned().unwrap_or(Value::Null),
                        "error":{"code":-32601,"message":format!("AIPP does not support Codex server request {}", frame.get("method").and_then(Value::as_str).unwrap_or("unknown"))}
                    }),
                )
                .await?;
            }
            continue;
        }
        if frame.get("id").and_then(Value::as_u64) == Some(expected_id) {
            if let Some(error) = frame.get("error") {
                return Err(format!("Codex app-server RPC error: {error}"));
            }
            return Ok(frame.get("result").cloned().unwrap_or(Value::Null));
        }
        if frame.get("method").is_some() {
            pending_notifications.push_back(frame);
        }
    }
    Err(format!(
        "Codex app-server stdout 在等待 JSON-RPC 响应 id={expected_id} 时关闭；进程未返回 JSON-RPC error，底层原因只能从附带的 Codex stderr 诊断"
    ))
}

async fn initialize_codex_transport(
    requests: &impl CodexServerRequests,
    conversation_id: i64,
    client_name: &str,
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    pending_notifications: &mut VecDeque<Value>,
) -> Result<(), String> {
    write_frame(stdin, &json_rpc_request(1, "initialize", json!({
        "clientInfo":{"name":client_name,"title":"AIPP","version":env!("CARGO_PKG_VERSION")},
        "capabilities":{"experimentalApi":true}
    }))).await?;
    read_codex_rpc_response(requests, conversation_id, stdin, lines, 1, pending_notifications).await?;
    write_frame(stdin, &json!({"jsonrpc":"2.0","method":"initialized"})).await
}

// With no reviewer configured, a new Codex thread defaults to user review.
// A resumed thread instead restores its saved reviewer unless explicitly overridden.
fn reviewer_from_codex_config(result: &Value) -> Result<&str, String> {
    let config = result["config"].as_object().ok_or("Codex config/read 响应缺少 config 对象")?;
    match config.get("approvals_reviewer") {
        None | Some(Value::Null) => Ok("user"),
        Some(Value::String(reviewer)) if !reviewer.is_empty() => Ok(reviewer),
        _ => Err("Codex config/read 返回了无效的 approvals_reviewer".into()),
    }
}

async fn open_codex_thread(
    requests: &impl CodexServerRequests,
    conversation_id: i64,
    config: &CodexAppServerConfig,
    stored_thread: Option<&str>,
    mcp_config_overrides: Option<serde_json::Map<String, Value>>,
    request_id: &mut u64,
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    pending_notifications: &mut VecDeque<Value>,
) -> Result<Value, String> {
    let (method, params) = if let Some(thread_id) = stored_thread {
        let reviewer = match config.approvals_reviewer.clone() {
            Some(value) => value,
            None => {
                write_frame(stdin, &json_rpc_request(*request_id, "config/read", json!({
                    "cwd": config.working_directory, "includeLayers": false,
                }))).await?;
                let result = read_codex_rpc_response(requests, conversation_id, stdin, lines, *request_id, pending_notifications)
                    .await.map_err(|error| format!("Codex 恢复会话 {thread_id} 前读取审批人失败：{error}"))?;
                *request_id += 1;
                reviewer_from_codex_config(&result)?.to_owned()
            }
        };
        ("thread/resume", json!({
            "threadId": thread_id,
            "approvalPolicy": config.approval_policy,
            "sandbox": config.sandbox,
            "approvalsReviewer": reviewer,
            "config": mcp_config_overrides,
        }))
    } else {
        ("thread/start", json!({
            "model": config.model,
            "reasoningEffort": config.reasoning_effort,
            "cwd": config.working_directory,
            "approvalPolicy": config.approval_policy,
            "sandbox": config.sandbox,
            "approvalsReviewer": config.approvals_reviewer,
            "config": mcp_config_overrides,
        }))
    };
    write_frame(stdin, &json_rpc_request(*request_id, method, params)).await?;
    read_codex_rpc_response(requests, conversation_id, stdin, lines, *request_id, pending_notifications)
        .await.map_err(|error| {
            if stored_thread.is_some() {
                format!("Codex thread 恢复失败：{error}")
            } else {
                format!("Codex thread 启动失败：{error}")
            }
        })
}

/// Reject only explicitly foreign identities. Some legitimate session-wide or
/// older-protocol notifications have no turn/thread ID and remain supported.
fn codex_notification_matches_turn(frame: &Value, thread_id: &str, turn_id: Option<&str>) -> bool {
    let params = &frame["params"];
    if params.get("threadId").and_then(Value::as_str).is_some_and(|id| id != thread_id) {
        return false;
    }
    let method = frame.get("method").and_then(Value::as_str).unwrap_or_default();
    let event_turn_id = params.get("turnId").and_then(Value::as_str).or_else(|| {
        matches!(method, "turn/started" | "turn/completed")
            .then(|| params.pointer("/turn/id").and_then(Value::as_str)).flatten()
    });
    !matches!((turn_id, event_turn_id), (Some(active), Some(event)) if active != event)
}

enum CodexIdleEvent {
    Frame(Value),
    Command(CodexSessionCommand),
}

async fn next_codex_idle_event(
    lines: &mut Lines<impl tokio::io::AsyncBufRead + Unpin>,
    pending: &mut VecDeque<Value>,
    receiver: &mut mpsc::UnboundedReceiver<CodexSessionCommand>,
) -> Result<Option<CodexIdleEvent>, String> {
    if let Some(frame) = pending.pop_front() { return Ok(Some(CodexIdleEvent::Frame(frame))); }
    loop {
        tokio::select! {
            // Admit an already-started native turn before sending a racing user prompt.
            biased;
            line = lines.next_line() => {
                let line = line.map_err(|e| format!("读取 Codex 后台事件失败：{e}"))?
                    .ok_or("等待后台事件时 Codex app-server 关闭了输出")?;
                match serde_json::from_str(&line) {
                    Ok(frame) => return Ok(Some(CodexIdleEvent::Frame(frame))),
                    Err(error) => warn!(%error, "Invalid idle Codex app-server frame"),
                }
            }
            command = receiver.recv() => return Ok(command.map(CodexIdleEvent::Command)),
        }
    }
}

fn codex_background_turn<'a>(frame: &'a Value, thread_id: &str) -> Option<&'a str> {
    if frame["method"] != "turn/started" || frame["params"]["threadId"] != thread_id { return None; }
    frame.pointer("/params/turn/id")?.as_str().filter(|id| !id.is_empty())
}

fn thread_id_from_result(result: &Value) -> Option<String> {
    result
        .pointer("/thread/id")
        .or_else(|| result.get("threadId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn item_identity(item: &Value) -> (String, String, Option<String>, Option<Value>) {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("unknown").to_string();
    let kind = item.get("type").and_then(Value::as_str).unwrap_or("status").to_string();
    match kind.as_str() {
        "commandExecution" => (
            item_id,
            "command".to_string(),
            item.get("command").and_then(Value::as_str).map(str::to_string),
            Some(json!({"command": item.get("command"), "cwd": item.get("cwd")})),
        ),
        "fileChange" => (item_id, "patch".to_string(), Some("文件变更".to_string()), Some(item.clone())),
        "contextCompaction" => (item_id, "status".to_string(), Some("压缩上下文".to_string()), None),
        "mcpToolCall" | "dynamicToolCall" => (
            item_id,
            "tool".to_string(),
            item.get("tool").and_then(Value::as_str).map(str::to_string),
            item.get("arguments").cloned(),
        ),
        "collabAgentToolCall" | "subAgentActivity" => {
            (item_id, "sub_agent".to_string(), Some(kind), Some(item.clone()))
        }
        _ => (item_id, kind.clone(), Some(kind), Some(item.clone())),
    }
}

fn activity_status(item: &Value, fallback: &str) -> String {
    item.get("status")
        .and_then(Value::as_str)
        .map(|status| match status {
            "inProgress" | "running" => "executing",
            "completed" => "success",
            other => other,
        })
        .unwrap_or(fallback)
        .to_string()
}

fn merge_activity_notification(
    items: &mut HashMap<String, Value>,
    method: &str,
    params: &Value,
) -> Option<Value> {
    if matches!(method, "item/started" | "item/completed") {
        let incoming = params.get("item")?.clone();
        let item_id = incoming.get("id")?.as_str()?.to_string();
        if let Some(existing) = items.get_mut(&item_id) {
            if let (Some(existing_object), Some(incoming_object)) =
                (existing.as_object_mut(), incoming.as_object())
            {
                for (key, value) in incoming_object {
                    existing_object.insert(key.clone(), value.clone());
                }
            } else {
                *existing = incoming;
            }
        } else {
            items.insert(item_id.clone(), incoming);
        }
        return items.get(&item_id).cloned();
    }
    if matches!(
        method,
        "item/commandExecution/outputDelta"
            | "item/fileChange/outputDelta"
            | "item/fileChange/patchUpdated"
            | "item/mcpToolCall/progress"
            | "item/plan/delta"
    ) {
        let item_id = params.get("itemId")?.as_str()?.to_string();
        let kind = if method.contains("fileChange") {
            "fileChange"
        } else if method.contains("mcpToolCall") {
            "mcpToolCall"
        } else if method.contains("plan") {
            "plan"
        } else {
            "commandExecution"
        };
        let item = items
            .entry(item_id.clone())
            .or_insert_with(|| json!({"id":item_id,"type":kind}));
        let object = item.as_object_mut()?;
        if let Some(patch) = params.get("patch") {
            object.insert("aggregatedOutput".to_string(), patch.clone());
        } else if let Some(delta) = params
            .get("delta")
            .or_else(|| params.get("message"))
            .and_then(Value::as_str)
        {
            let previous = object
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .unwrap_or_default();
            object.insert("aggregatedOutput".to_string(), json!(format!("{previous}{delta}")));
        }
        return Some(item.clone());
    }
    None
}

fn generic_item_notification(method: &str, params: &Value) -> Option<Value> {
    if !method.starts_with("item/") {
        return None;
    }
    let item_id = params.get("itemId").and_then(Value::as_str)?;
    Some(json!({
        "id": item_id,
        "type": method.strip_prefix("item/").unwrap_or(method),
        "status": "inProgress",
        "notificationMethod": method,
        "notificationParams": params,
    }))
}

fn codex_turn_usage(params: &Value) -> Option<CodexTurnUsage> {
    CodexTurnUsage::from_breakdown(params.pointer("/tokenUsage/last")?)
}

impl CodexTurnUsage {
    fn from_breakdown(value: &Value) -> Option<Self> {
        Some(CodexTurnUsage {
            input_tokens: value.get("inputTokens")?.as_i64()?.try_into().ok()?,
            output_tokens: value.get("outputTokens")?.as_i64()?.try_into().ok()?,
            reasoning_tokens: value.get("reasoningOutputTokens").and_then(Value::as_i64).unwrap_or(0).try_into().ok()?,
            cached_input_tokens: value.get("cachedInputTokens").and_then(Value::as_i64).unwrap_or(0).try_into().ok()?,
            cache_write_input_tokens: value.get("cacheWriteInputTokens").and_then(Value::as_i64).unwrap_or(0).try_into().ok()?,
            total_tokens: value.get("totalTokens")?.as_i64()?.try_into().ok()?,
        })
    }

    fn saturating_sub(self, other: Self) -> Self {
        let sub = |a: i32, b: i32| a.saturating_sub(b).max(0);
        CodexTurnUsage {
            input_tokens: sub(self.input_tokens, other.input_tokens),
            output_tokens: sub(self.output_tokens, other.output_tokens),
            reasoning_tokens: sub(self.reasoning_tokens, other.reasoning_tokens),
            cached_input_tokens: sub(self.cached_input_tokens, other.cached_input_tokens),
            cache_write_input_tokens: sub(self.cache_write_input_tokens, other.cache_write_input_tokens),
            total_tokens: sub(self.total_tokens, other.total_tokens),
        }
    }
}

/// 汇总一轮 Codex 的用量。
///
/// `thread/tokenUsage/updated` 的 `last` 只是最近一次模型请求，一轮里调用工具会发出多次请求，
/// 只取最后一条会漏掉前面的请求。本轮用量 = 最后一条通知的累计 `total` − 本轮开始前的累计 `total`。
/// 每次请求开始时 Codex 会随限流信息带着上一次的旧数据再推一条，所以模型开始输出前收到的通知只作基线；
/// 没有这类通知时，用第一条新通知的 `total − last` 推出基线。
#[derive(Debug, Default)]
struct CodexTurnUsageTracker {
    model_activity: bool,
    baseline: Option<CodexTurnUsage>,
    first: Option<(CodexTurnUsage, CodexTurnUsage)>,
    latest_total: Option<CodexTurnUsage>,
    latest_last: Option<CodexTurnUsage>,
}

impl CodexTurnUsageTracker {
    /// 用户消息条目在请求发出前就会推送，不代表模型已经开始输出。
    fn note_notification(&mut self, method: &str, params: &Value) {
        if self.model_activity || !method.starts_with("item/") {
            return;
        }
        let user_message = matches!(method, "item/started" | "item/completed")
            && params.pointer("/item/type").and_then(Value::as_str) == Some("userMessage");
        if !user_message {
            self.model_activity = true;
        }
    }

    fn observe(&mut self, params: &Value) {
        let total = params.pointer("/tokenUsage/total").and_then(CodexTurnUsage::from_breakdown);
        let last = codex_turn_usage(params);
        if !self.model_activity {
            if total.is_some() {
                self.baseline = total;
            }
            return;
        }
        if let (Some(total), Some(last)) = (total, last) {
            self.first.get_or_insert((total, last));
        }
        if total.is_some() {
            self.latest_total = total;
        }
        if last.is_some() {
            self.latest_last = last;
        }
    }

    fn turn_usage(&self) -> Option<CodexTurnUsage> {
        let base = self.baseline.or_else(|| self.first.map(|(total, last)| total.saturating_sub(last)));
        match (self.latest_total, base) {
            (Some(latest), Some(base)) => Some(latest.saturating_sub(base)),
            _ => self.latest_last,
        }
    }
}

/// 取 item 首次出现时已输出的正文字符数（Unicode 字符计），后续更新沿用同一偏移
fn activity_content_offset(
    offsets: &mut HashMap<String, u64>,
    item: &Value,
    content: &str,
) -> Option<u64> {
    let item_id = item.get("id").and_then(Value::as_str)?.to_string();
    Some(
        *offsets
            .entry(item_id)
            .or_insert_with(|| content.chars().count() as u64),
    )
}

fn activity_order(orders: &mut HashMap<String, u64>, item: &Value) -> Option<u64> {
    let id = item.get("id")?.as_str()?.to_string();
    let next = orders.len() as u64;
    Some(*orders.entry(id).or_insert(next))
}

fn has_dedicated_message(kind: &str) -> bool {
    matches!(kind, "userMessage" | "agentMessage" | "plan" | "reasoning") || kind.starts_with("reasoning/")
}

/// Deltas may belong to three different bubbles in a single Codex turn.
fn codex_delta_focus(method: &str, params: &Value, response_id: i64, reasoning_id: Option<i64>,
    plan_id: impl FnOnce(&str) -> Option<i64>) -> Option<i64> {
    if params.get("delta")?.as_str()?.is_empty() { return None; }
    match method {
        "item/agentMessage/delta" => Some(response_id),
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => reasoning_id,
        "item/plan/delta" => plan_id(params.get("itemId")?.as_str()?),
        _ => None,
    }
}

fn emit_activity(
    identity: &AgentEventIdentity,
    window: &tauri::Window,
    conversation_id: i64,
    response_message_id: i64,
    thread_id: Option<&str>,
    sequence: u64,
    item: &Value,
    fallback_status: &str,
    content_offset: Option<u64>,
    order: Option<u64>,
) -> Option<i64> {
    let (item_id, kind, title, input) = item_identity(item);
    // 用户输入、agent 正文和 Plan 都有专属展示，不再生成通用活动卡片
    if has_dedicated_message(&kind) {
        return None;
    }
    let activity = AgentActivityEvent {
        conversation_id,
        response_message_id,
        agent_kind: CODEX_APP_SERVER_API_TYPE.to_string(),
        session_id: thread_id.map(str::to_string),
        item_id,
        sequence,
        order,
        kind,
        status: activity_status(item, fallback_status),
        title,
        input,
        output: item
            .get("aggregatedOutput")
            .or_else(|| item.get("result"))
            .map(|value| value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())),
        error: item.get("error").map(Value::to_string),
        metadata: item.clone(),
        content_offset,
    };
    let event = ConversationEvent {
        r#type: "agent_activity".to_string(),
        data: serde_json::to_value(&activity).unwrap(),
    };
    persist_activity(window.app_handle(), response_message_id, &activity);
    let _ = emit_to_window(window, identity.clone(), event);
    Some(response_message_id)
}

fn persist_activity(app_handle: &tauri::AppHandle, message_id: i64, activity: &AgentActivityEvent) {
    let Ok(db) = ConversationDatabase::new(app_handle) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(message)) = repo.read(message_id) else { return };
    let mut metadata = message
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let activities = metadata
        .as_object_mut()
        .expect("metadata normalized to object")
        .entry("agent_activities")
        .or_insert_with(|| json!([]));
    let Some(items) = activities.as_array_mut() else { return };
    let replacement = serde_json::to_value(activity).unwrap_or(Value::Null);
    if let Some(existing) = items.iter_mut().find(|candidate| {
        candidate.get("agent_kind") == replacement.get("agent_kind")
            && candidate.get("session_id") == replacement.get("session_id")
            && candidate.get("item_id") == replacement.get("item_id")
    }) {
        if existing.get("sequence").and_then(Value::as_u64).unwrap_or(0) <= activity.sequence {
            *existing = replacement;
        }
    } else {
        items.push(replacement);
    }
    if let Ok(serialized) = serde_json::to_string(&metadata) {
        let _ = repo.update_metadata(message_id, Some(&serialized));
    }
}

fn persist_response(
    app_handle: &tauri::AppHandle,
    message_id: i64,
    content: &str,
    done: bool,
    usage: Option<CodexTurnUsage>,
    final_text: Option<&str>,
) {
    if let Ok(db) = ConversationDatabase::new(app_handle) {
        if let Ok(repo) = db.message_repo() {
            let _ = repo.update_content(message_id, content);
            if done {
                if let Ok(Some(mut message)) = repo.read(message_id) {
                    message.finish_time = Some(chrono::Utc::now());
                    message.input_token_count = usage.map(|value| value.input_tokens).unwrap_or(0);
                    message.output_token_count = usage
                        .map(|value| value.output_tokens)
                        .unwrap_or_else(|| ((content.chars().count() + 3) / 4) as i32);
                    message.token_count = usage
                        .map(|value| value.total_tokens)
                        .unwrap_or(message.output_token_count);
                    let mut metadata = message.metadata_json.as_deref()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .filter(Value::is_object).unwrap_or_else(|| json!({}));
                    if let Some(text) = final_text {
                        crate::db::butler_execution::record_agent_final(&mut metadata, content, text);
                    }
                    if let Some(usage) = usage {
                        if let Some(map) = metadata.as_object_mut() {
                            map.insert("usage_source".to_string(), json!("reported"));
                            map.insert("thought_tokens".to_string(), json!(usage.reasoning_tokens));
                            map.insert("cached_input_tokens".to_string(), json!(usage.cached_input_tokens));
                            map.insert("cached_read_tokens".to_string(), json!(usage.cached_input_tokens));
                            map.insert("cached_write_tokens".to_string(), json!(usage.cache_write_input_tokens));
                            map.insert("cache_creation_tokens".to_string(), json!(usage.cache_write_input_tokens));
                        }
                    } else {
                        // 没有收到用量通知时输出按字数估算，必须标明是估算，对话信息才会提示。
                        metadata["usage_source"] = json!("estimated");
                    }
                    message.metadata_json = serde_json::to_string(&metadata).ok();
                    let _ = repo.update(&message);
                }
            }
        }
    }
}


fn persist_codex_timing(
    app_handle: &tauri::AppHandle,
    message_id: i64,
    start_time: chrono::DateTime<chrono::Utc>,
    first_token_time: Option<chrono::DateTime<chrono::Utc>>,
) {
    let Ok(db) = ConversationDatabase::new(app_handle) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(mut message)) = repo.read(message_id) else { return };
    message.start_time = Some(start_time);
    message.first_token_time = first_token_time;
    message.ttft_ms = first_token_time.map(|first| (first.timestamp_millis() - start_time.timestamp_millis()).max(0));
    let _ = repo.update(&message);
}

fn persist_codex_reasoning(
    app_handle: &tauri::AppHandle,
    message_id: i64,
    content: &str,
) {
    let Ok(db) = ConversationDatabase::new(app_handle) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(mut message)) = repo.read(message_id) else { return };
    message.content = content.to_string();
    message.finish_time = Some(chrono::Utc::now());
    // Codex reports reasoningOutputTokens as part of the response usage. Keep
    // the separate reasoning bubble informational so conversation totals are
    // not counted twice.
    message.input_token_count = 0;
    message.output_token_count = 0;
    message.token_count = 0;
    let _ = repo.update(&message);
}

/// 记录一行 Codex stderr：先剥离 ANSI 转义，再过滤 tracing 的 TRACE/DEBUG/INFO 噪音，
/// 让缓冲只保留对用户诊断有意义的行。返回 true 表示该行已记入缓冲。
fn record_codex_stderr(buffer: &CodexStderrBuffer, line: String) -> bool {
    let cleaned = super::agent_driver::failure::redact_agent_diagnostic(&strip_ansi_escapes(&line), &[]);
    if cleaned.trim().is_empty() || is_tracing_noise(&cleaned) {
        return false;
    }
    let mut lines = buffer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if lines.len() == CODEX_STDERR_MAX_LINES {
        lines.pop_front();
    }
    lines.push_back(cleaned);
    true
}

fn codex_error_with_diagnostics(
    error: &str,
    conversation_id: i64,
    run_id: &str,
    stderr_buffer: &CodexStderrBuffer,
) -> String {
    let stderr = stderr_buffer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if stderr.trim().is_empty() {
        format!("{error} [conversation_id={conversation_id}, run_id={run_id}]")
    } else {
        format!(
            "{error} [conversation_id={conversation_id}, run_id={run_id}]\nCodex 关键日志（最近 {} 行，已过滤 INFO/DEBUG 噪音）：\n{}",
            stderr.lines().count(),
            stderr.trim()
        )
    }
}

async fn spawn_process(
    config: &CodexAppServerConfig,
    stderr_buffer: CodexStderrBuffer,
) -> Result<(Child, tokio::process::ChildStdin, Lines<BufReader<ChildStdout>>), String> {
    let resolved_cli = official_cli::for_session("codex", &config.cli_command, &config.env_vars).await?;
    let mut command = Command::from(resolved_cli.command(&config.env_vars));
    command
        .arg("app-server")
        .arg("--listen")
        .arg("stdio://")
        .args(&config.additional_args)
        .current_dir(&config.working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        format!("启动 Codex app-server 失败（{}）：{error}", resolved_cli.path.display())
    })?;
    let stdin = child.stdin.take().ok_or("Codex app-server stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Codex app-server stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("Codex app-server stderr unavailable")?;
    let diagnostic_redactions = super::agent_driver::failure::AgentDiagnosticBuffer::for_environment(&config.env_vars);
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    // 缓冲只保留清洗后的关键行；被过滤的噪音行降级到 debug 日志，原文仍可查
                    if record_codex_stderr(&stderr_buffer, diagnostic_redactions.sanitize(&line)) {
                        error!(target = "codex_app_server", "{line}");
                    } else {
                        debug!(target = "codex_app_server", "{line}");
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let detail = format!("读取 Codex stderr 失败：{error}");
                    error!(target = "codex_app_server", "{detail}");
                    record_codex_stderr(&stderr_buffer, detail);
                    break;
                }
            }
        }
    });
    Ok((child, stdin, BufReader::new(stdout).lines()))
}

fn remove_codex_session_for_run(
    sessions: &mut HashMap<i64, CodexSessionEntry>,
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

pub fn spawn_codex_session_task(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    config: CodexAppServerConfig,
) -> CodexSessionHandle {
    let (sender, receiver) = mpsc::unbounded_channel();
    let run_id = uuid::Uuid::new_v4().to_string();
    let cancellation = watch::channel(0).0;
    let task_cancellation = cancellation.clone();
    let failure_context = std::sync::Arc::new(std::sync::Mutex::new(None));
    let task_failure_context = failure_context.clone();
    let task_run_id = run_id.clone();
    let request_scope = crate::api::ai::agent_driver::approval::AgentRequestScope::native(
        CODEX_APP_SERVER_API_TYPE, config.instance_id, conversation_id, task_run_id.clone(),
    );
    tauri::async_runtime::spawn(async move {
        let stderr_buffer = CodexStderrBuffer::default();
        let mut event_identity = AgentEventIdentity::new(CODEX_APP_SERVER_API_TYPE, conversation_id).with_run(task_run_id.clone());
        event_identity.instance_id = config.instance_id.map(|id| id.to_string());
        let mut failure_stage = AgentFailureStage::Process;
        let session_result = run_session(
            app_handle.clone(),
            conversation_id,
            config,
            receiver,
            task_failure_context.clone(),
            &task_run_id,
            stderr_buffer.clone(),
            task_cancellation,
            &mut event_identity,
            &mut failure_stage,
        )
        .await;
        let failed_prompt = {
            task_failure_context
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
        };
        if let Err(error) = session_result {
            let error = codex_error_with_diagnostics(
                &error,
                conversation_id,
                &task_run_id,
                &stderr_buffer,
            );
            error!(conversation_id, error = %error, "Codex app-server session exited");
            if let Some((message_id, window)) = failed_prompt {
                let stderr = stderr_buffer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
                let failure = AgentFailure::new(event_identity.clone().with_response_message(message_id), failure_stage, &error)
                    .with_process_diagnostics(AgentProcessDiagnostics {
                        stderr: (!stderr.is_empty()).then_some(stderr), exit_status: None, protocol_error: None,
                        unavailable_reason: Some("The native actor did not return a separate process exit status; its protocol cause and retained stderr are included".into()),
                    });
                if let Err(publication_error) = emit_normalized_failure_to_window(&app_handle, &window, failure, "Codex") {
                    error!(conversation_id, message_id, error = %publication_error, "Codex failure publication incomplete");
                }
                if let Some(manager) = app_handle.try_state::<ConversationActivityManager>() {
                    manager.clear_agent_focus(&app_handle, &event_identity, false).await;
                }
            }
        } else if let Some((message_id, window)) = failed_prompt {
            let error = codex_error_with_diagnostics(
                "AIPP Codex 会话任务在当前请求尚未完成时异常返回成功；这是内部状态错误",
                conversation_id,
                &task_run_id,
                &stderr_buffer,
            );
            error!(conversation_id, error, "Codex app-server session stopped with an active prompt");
            let stderr = stderr_buffer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
                let failure = AgentFailure::new(event_identity.clone().with_response_message(message_id), failure_stage, &error)
                    .with_process_diagnostics(AgentProcessDiagnostics {
                        stderr: (!stderr.is_empty()).then_some(stderr), exit_status: None, protocol_error: None,
                        unavailable_reason: Some("The native actor did not return a separate process exit status; its protocol cause and retained stderr are included".into()),
                    });
                if let Err(publication_error) = emit_normalized_failure_to_window(&app_handle, &window, failure, "Codex") {
                    error!(conversation_id, message_id, error = %publication_error, "Codex failure publication incomplete");
                }
            if let Some(manager) = app_handle.try_state::<ConversationActivityManager>() {
                manager.clear_agent_focus(&app_handle, &event_identity, false).await;
            }
        }
        crate::api::ai::agent_driver::approval::cancel_app_requests_for_scope(
            &app_handle, &request_scope,
        ).await;
        let removed_current_entry = if let Some(state) = app_handle.try_state::<crate::CodexSessionState>() {
            let mut sessions = state.sessions.lock().await;
            remove_codex_session_for_run(&mut sessions, conversation_id, &task_run_id)
        } else {
            false
        };
        if removed_current_entry {
            emit_session_snapshot(&event_identity, &app_handle, conversation_id, None).await;
        }
    });
    CodexSessionHandle { sender, run_id, cancellation, failure_context }
}

async fn run_session(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    config: CodexAppServerConfig,
    mut receiver: mpsc::UnboundedReceiver<CodexSessionCommand>,
    failure_context: std::sync::Arc<std::sync::Mutex<Option<(i64, tauri::Window)>>>,
    run_id: &str,
    stderr_buffer: CodexStderrBuffer,
    cancellation: watch::Sender<u64>,
    event_identity: &mut AgentEventIdentity,
    failure_stage: &mut AgentFailureStage,
) -> Result<(), String> {
    let mut request_cancellation = CodexRequestCancellation {
        mcp_prompt: None,
        source_message_id: None,
        signal: cancellation, generation: 0, run_id: run_id.to_string(), instance_id: config.instance_id,
    };
    let (_child, mut stdin, mut lines) = spawn_process(&config, stderr_buffer).await?;
    let mut pending_notifications = VecDeque::new();
    let requests = DesktopCodexServerRequests {
        app_handle: &app_handle, conversation_id, cancellation: Some(&request_cancellation),
    };
    *failure_stage = AgentFailureStage::Initialize;
    initialize_codex_transport(&requests, conversation_id, "aipp", &mut stdin, &mut lines, &mut pending_notifications).await?;

    *failure_stage = AgentFailureStage::Persistence;
    let stored_thread = ConversationDatabase::new(&app_handle)
        .map_err(|error| format!("Codex restore database unavailable: {error}"))?
        .get_agent_session_id_for_instance(conversation_id, CODEX_APP_SERVER_API_TYPE, config.instance_id)
        .map_err(|error| format!("Codex persisted session lookup failed: {error}"))?;
    let mut request_id = 2_u64;
    *failure_stage = AgentFailureStage::Mcp;
    let mcp_scope = crate::acp_mcp_bridge::provenance::NativeMcpScope::new(conversation_id)?;
    let mcp_config_overrides = codex_mcp_bridge_overrides(&app_handle, conversation_id, &config, &mcp_scope)
        .await
        .map_err(|error| format!("Codex MCP bridge 初始化失败：{error}"))?;
    *failure_stage = if stored_thread.is_some() { AgentFailureStage::Resume } else { AgentFailureStage::Initialize };
    let thread_result = open_codex_thread(
        &requests, conversation_id, &config, stored_thread.as_deref(), mcp_config_overrides,
        &mut request_id, &mut stdin, &mut lines, &mut pending_notifications,
    ).await?;
    let application_context = codex_application_context(config.developer_instructions.as_deref().unwrap_or_default());
    let restored_session_method = stored_thread.as_ref().map(|_| "resume".to_string());
    let connection_event_id = restored_session_method
        .as_ref()
        .map(|method| format!("{run_id}:{method}"));
    let thread_id = thread_id_from_result(&thread_result)
        .ok_or_else(|| format!("Codex thread response missing thread id: {thread_result}"))?;
    event_identity.native_session_id = Some(thread_id.clone());
    *failure_stage = AgentFailureStage::Configuration;
    request_id += 1;
    let (model_catalog, supported_efforts) = {
        write_frame(&mut stdin, &json_rpc_request(request_id, "model/list", json!({}))).await?;
        let result = read_rpc_response_with_approvals(
            &app_handle, conversation_id, &mut stdin, &mut lines, request_id, &mut pending_notifications, Some(&request_cancellation),
        ).await.map_err(|error| format!("Codex model/list 失败：{error}"))?;
        let catalog = model_catalog(&result);
        if catalog.is_empty() {
            return Err("Codex model/list 未返回任何模型".to_string());
        }
        let efforts = catalog.iter()
            .find(|entry| config.model.as_deref() == Some(entry.id.as_str()))
            .map(|entry| entry.supported_efforts.clone())
            .unwrap_or_else(|| supported_reasoning_efforts(&result));
        (catalog, efforts)
    };
    request_id += 1;
    write_frame(
        &mut stdin,
        &json_rpc_request(request_id, "collaborationMode/list", json!({})),
    )
    .await?;
    let collaboration_modes = read_rpc_response_with_approvals(
        &app_handle,
        conversation_id,
        &mut stdin,
        &mut lines,
        request_id,
        &mut pending_notifications,
        Some(&request_cancellation),
    )
    .await
    .map_err(|error| format!("Codex collaborationMode/list 失败：{error}"))?;
    let plan_mode_supported = codex_supports_plan_mode(&collaboration_modes);
    let reviewer = thread_result["approvalsReviewer"].as_str().map(str::to_owned)
        .or_else(|| config.approvals_reviewer.clone());
    // Save only after the first Prompt is submitted: eager startup can finish
    // before acquisition registers the native handle and provider binding.
    let mut snapshot = CodexConversationSessionState {
        conversation_id,
        agent_kind: CODEX_APP_SERVER_API_TYPE.to_string(),
        session_id: Some(thread_id.clone()),
        load_session_supported: false,
        session_resume_supported: true,
        restored_session_method,
        connection_event_id,
        current_turn_id: None,
        has_active_prompt: false,
        model: first_string_for_keys(&thread_result, &["model", "modelId"])
            .or_else(|| stored_thread.is_none().then(|| config.model.clone()).flatten()),
        reasoning_effort: first_string_for_keys(&thread_result, &["reasoningEffort", "reasoning_effort"])
            .or_else(|| stored_thread.is_none().then(|| config.reasoning_effort.clone()).flatten()),
        approval_policy: config.approval_policy.clone(),
        sandbox: config.sandbox.clone(),
        approvals_reviewer: reviewer.clone(),
        collaboration_mode: plan_mode_supported.then(|| {
            config.collaboration_mode.clone().unwrap_or_else(|| "default".to_string())
        }),
        plan: Vec::new(),
        plan_explanation: None,
        config_options: {
            let mut options = Vec::new();
            {
                options.push(CodexSessionConfigOption {
                    id: "model".to_string(), name: "模型".to_string(),
                    description: Some("Codex 下一轮响应使用的模型；可输入模型 ID".to_string()),
                    category: Some("model".to_string()), current_value: first_string_for_keys(&thread_result, &["model", "modelId"])
                        .or_else(|| stored_thread.is_none().then(|| config.model.clone()).flatten()).unwrap_or_default(),
                    options: codex_model_choices(&model_catalog),
                });
            }
            options.push(CodexSessionConfigOption {
            id: "reasoning_effort".to_string(),
            name: "思考强度".to_string(),
            description: Some("Codex 下一轮响应使用的推理强度".to_string()),
            category: Some("thought_level".to_string()),
            current_value: first_string_for_keys(&thread_result, &["reasoningEffort", "reasoning_effort"])
                .or_else(|| stored_thread.is_none().then(|| config.reasoning_effort.clone()).flatten())
                .or_else(|| supported_efforts.first().cloned()).unwrap_or_default(),
            options: codex_reasoning_options(&supported_efforts),
            });
            options.push(CodexSessionConfigOption {
                id: "approval_policy".to_string(),
                name: "审批策略".to_string(),
                description: Some("Codex 何时请求用户审批；从下一轮起生效".to_string()),
                category: Some("approval".to_string()),
                current_value: config.approval_policy.clone().unwrap_or_default(),
                options: codex_approval_policy_options(),
            });
            options.push(CodexSessionConfigOption {
                id: "sandbox".to_string(),
                name: "沙箱模式".to_string(),
                description: Some("Codex 命令执行的沙箱范围；从下一轮起生效".to_string()),
                category: Some("sandbox".to_string()),
                current_value: config.sandbox.clone().unwrap_or_default(),
                options: codex_sandbox_options(),
            });
            options.push(CodexSessionConfigOption {
                id: "approvals_reviewer".to_string(),
                name: "审批人".to_string(),
                description: Some("Codex 审批请求由人工处理还是自动审批；从下一轮起生效".to_string()),
                category: Some("approvals_reviewer".to_string()),
                current_value: reviewer.unwrap_or_default(),
                options: codex_approvals_reviewer_options(),
            });
            if plan_mode_supported {
                options.push(CodexSessionConfigOption {
                    id: "collaboration_mode".to_string(),
                    name: "工作模式".to_string(),
                    description: Some("控制 Codex 下一轮是先制定计划还是直接执行".to_string()),
                    category: Some("mode".to_string()),
                    current_value: config.collaboration_mode.clone().unwrap_or_else(|| "default".to_string()),
                    options: codex_collaboration_mode_options(),
                });
            }
            options
        },
    };
    if config.collaboration_mode.as_deref() == Some("plan") && !plan_mode_supported {
        return Err("当前 Codex app-server 未提供 Plan 模式".to_string());
    }
    if let Some(model) = snapshot.model.clone() {
        let catalog_model = model_catalog
            .iter()
            .find(|candidate| candidate.id == model)
            .ok_or_else(|| format!("Codex 当前模型不在 model/list 返回结果中：{model}"))?;
        if let Some(effort) = snapshot.reasoning_effort.as_deref() {
            if !catalog_model
                .supported_efforts
                .iter()
                .any(|candidate| candidate == effort)
            {
                return Err(format!("Codex 当前模型 {model} 不支持已配置的思考强度：{effort}"));
            }
        }
        apply_codex_config_option(&mut snapshot, &model_catalog, "model", model)?;
    }
    emit_session_snapshot(event_identity, &app_handle, conversation_id, Some(snapshot.clone())).await;

    enum Next { Command(CodexSessionCommand), Wake(i64, tauri::Window, u64, String) }
    let mut previous: Option<(i64, tauri::Window, u64)> = None;
    let mut finished_turns = std::collections::HashSet::new();
    let mut queued_prompts = VecDeque::new();
    loop {
        // Keep consuming stdout after a successful reply. Child-thread activity and
        // terminal output alone do not constitute a new turn on the parent thread.
        let command = if let Some(command) = queued_prompts.pop_front() {
            Next::Command(command)
        } else if let Some((parent, window, generation)) = &previous {
            match next_codex_idle_event(&mut lines, &mut pending_notifications, &mut receiver).await? {
                Some(CodexIdleEvent::Frame(frame)) => {
                    if frame.get("id").is_some() && frame.get("method").is_some() {
                        write_frame(&mut stdin, &json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32600,"message":"No active AIPP turn for this request"}})).await?;
                        continue;
                    }
                    let Some(id) = codex_background_turn(&frame, &thread_id) else { continue };
                    if finished_turns.contains(id) { continue; }
                    if *generation != *request_cancellation.signal.borrow()
                        || !super::agent_driver::background::can_continue(&app_handle, conversation_id, *parent)? {
                        return Ok(());
                    }
                    let wake_identity = event_identity.clone().with_turn(id);
                    let message_id = super::agent_driver::background::create_reply(&app_handle, &wake_identity, *parent,
                        super::agent_driver::session::AgentSessionKind::Codex.into()).map_err(|e|e.to_string())?;
                    Next::Wake(message_id, window.clone(), *generation, id.to_owned())
                }
                Some(CodexIdleEvent::Command(command)) => Next::Command(command),
                None => break,
            }
        } else {
            match receiver.recv().await { Some(command) => Next::Command(command), None => break }
        };
        match command {
            Next::Command(CodexSessionCommand::Steer { response, .. }) => {
                let _ = response.send(Err("Codex steering target already finished".into()));
            }
            Next::Command(CodexSessionCommand::SetConfigOption { config_id, value, response }) => {
                match apply_codex_config_option(&mut snapshot, &model_catalog, &config_id, value) {
                    Ok(()) => {
                        emit_session_snapshot(event_identity, &app_handle, conversation_id, Some(snapshot.clone())).await;
                        let _ = response.send(Ok(()));
                    }
                    Err(error) => {
                        let _ = response.send(Err(error));
                    }
                }
            }
            Next::Command(CodexSessionCommand::CancelCurrentPrompt { response }) => {
                let _ = response.send(Ok(()));
            }
            Next::Command(CodexSessionCommand::Shutdown { reason }) => {
                info!(conversation_id, run_id, reason, "Codex session shutdown requested");
                return Ok(());
            }
            next => {
                let (message_id, window, cancellation_generation, input, native_turn) = match next {
                    Next::Wake(id, window, generation, turn) => (id, window, generation, None, Some(turn)),
                    Next::Command(CodexSessionCommand::Prompt { message_id, prompt, images, window, cancellation_generation }) =>
                        (message_id, window, cancellation_generation, Some((prompt, images)), None),
                    _ => unreachable!("idle controls handled above"),
                };
                *failure_context.lock().unwrap_or_else(|e| e.into_inner()) = Some((message_id, window.clone()));

                let _mcp_prompt = mcp_scope.begin_prompt(message_id)?
                    .watch_cancellation(request_cancellation.signal.subscribe(), cancellation_generation);
                event_identity.response_message_id = Some(message_id);
                event_identity.turn_id = None;
                *failure_stage = AgentFailureStage::Persistence;
                {
                    let state = app_handle.state::<crate::CodexSessionState>();
                    let sessions = state.sessions.lock().await;
                    super::agent_driver::persistence::persist_session_id(&app_handle, event_identity,
                        sessions.get(&conversation_id).map(|entry| entry.run_id.as_str()), &thread_id)?;
                }
                *failure_stage = AgentFailureStage::SendPrompt;
                request_cancellation.generation = cancellation_generation;
                request_cancellation.source_message_id = Some(message_id);
                request_cancellation.mcp_prompt = mcp_scope.current_prompt()?;
                let turn_start_time = chrono::Utc::now();
                // 手动压缩：改发 thread/compact/start（立即返回 {}，压缩作为一轮 turn 以通知推进）
                let manual_compact = input.as_ref().is_some_and(|(prompt, images)| images.is_empty() && super::agent_driver::manual_compact::is_manual_compact_prompt(prompt));
                let mut turn_id = if let Some((prompt, images)) = input {
                    let (turn_method, turn_params) = if manual_compact {
                        ("thread/compact/start", json!({ "threadId": thread_id }))
                    } else {
                        let mut params = codex_turn_start_params(&thread_id, &snapshot, &prompt, &application_context);
                        params["input"].as_array_mut().unwrap().extend(codex_image_items(images).await?);
                        ("turn/start", params)
                    };
                    request_id += 1;
                    write_frame(
                        &mut stdin,
                        &json_rpc_request(request_id, turn_method, turn_params),
                    )
                    .await?;
                    let turn_result = read_rpc_response_with_approvals(
                        &app_handle,
                        conversation_id,
                        &mut stdin,
                        &mut lines,
                        request_id,
                        &mut pending_notifications,
                        Some(&request_cancellation),
                    )
                    .await?;
                    turn_result
                        .pointer("/turn/id")
                        .or_else(|| turn_result.get("turnId"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else { native_turn };
                snapshot.current_turn_id = turn_id.clone();
                event_identity.turn_id = turn_id.clone();
                *failure_stage = AgentFailureStage::Stream;
                snapshot.has_active_prompt = true;
                emit_session_snapshot(event_identity, &app_handle, conversation_id, Some(snapshot.clone())).await;
                let mut content = String::new();
                let mut final_text: Option<String> = None;
                let mut reasoning = String::new();
                let mut reasoning_message_id: Option<i64> = None;
                let mut first_any_token_time: Option<chrono::DateTime<chrono::Utc>> = None;
                let mut focused_message_id = None;
                let mut usage_tracker = CodexTurnUsageTracker::default();
                let mut restore = CodexContextRestore::default();
                let mut turn_completed = false;
                let mut steer_receipts = HashMap::new();
                let mut completed_normally = false;
                let mut turn_error: Option<String> = None;
                let mut sequence = 0_u64;
                let mut activity_items = HashMap::<String, Value>::new();
                let mut plan_messages = HashMap::<String, AgentPlanMessage>::new();
                // 记录每个 item 开始时的正文字符数，供前端把活动卡片穿插到正文对应位置
                let mut item_content_offsets = HashMap::<String, u64>::new();
                let mut item_orders = HashMap::<String, u64>::new();
                while !turn_completed || !restore.pending.is_empty() {
                    let frame = if let Some(frame) = pending_notifications.pop_front() {
                        frame
                    } else {
                        tokio::select! {
                            maybe_command = receiver.recv() => {
                            match maybe_command {
                                Some(CodexSessionCommand::CancelCurrentPrompt { response }) => {
                                    if let Some(active_turn_id) = turn_id.as_deref() {
                                        request_id += 1;
                                        let result = write_frame(&mut stdin, &json_rpc_request(request_id, "turn/interrupt", json!({"threadId":thread_id,"turnId":active_turn_id}))).await;
                                        let _ = response.send(result);
                                    } else {
                                        let _ = response.send(Ok(()));
                                    }
                                }
                                Some(CodexSessionCommand::Steer { turn, text, images, response }) => {
                                    if response.is_closed() { continue; }
                                    if turn_id.as_deref() != Some(turn.as_str()) || turn_completed {
                                        let _ = response.send(Err("Codex steering target already finished".into()));
                                    } else {
                                        request_id += 1;
                                        let mut items = vec![json!({"type":"text","text":text})];
                                        match codex_image_items(images).await {
                                            Ok(images) => items.extend(images),
                                            Err(error) => { let _ = response.send(Err(error)); continue; }
                                        }
                                        let params = json!({"threadId":thread_id,"expectedTurnId":turn,"input":items});
                                        match write_frame(&mut stdin, &json_rpc_request(request_id, "turn/steer", params)).await {
                                            Ok(()) => { steer_receipts.insert(request_id, response); }
                                            Err(error) => { let _ = response.send(Err(error)); }
                                        }
                                    }
                                }
                                Some(command @ CodexSessionCommand::Prompt { .. }) => queued_prompts.push_back(command),
                                Some(CodexSessionCommand::SetConfigOption { config_id, value, response }) => {
                                    match apply_codex_config_option(&mut snapshot, &model_catalog, &config_id, value) {
                                        Ok(()) => {
                                            emit_session_snapshot(event_identity, &app_handle, conversation_id, Some(snapshot.clone())).await;
                                            let _ = response.send(Ok(()));
                                        }
                                        Err(error) => {
                                            let _ = response.send(Err(error));
                                        }
                                    }
                                }
                                Some(CodexSessionCommand::Shutdown { reason }) => {
                                    return Err(format!(
                                        "AIPP 在 Codex 请求执行期间关闭了控制通道：{reason}"
                                    ));
                                }
                                None => return Err(
                                    "AIPP Codex 控制通道意外关闭：当前请求仍在执行，但所有会话句柄已释放；这表示会话所有权发生错误".to_string()
                                ),
                            }
                            continue;
                            }
                            line = lines.next_line() => {
                            let Some(line) = line.map_err(|error| {
                                format!("读取 Codex app-server stdout 失败（当前 Codex turn 正在执行）：{error}")
                            })? else {
                                return Err(
                                    "Codex app-server stdout 在当前 turn 完成前关闭，且未发送 turn/completed 或 turn/failed；底层原因只能从附带的 Codex stderr 诊断".to_string()
                                );
                            };
                            match serde_json::from_str(&line) {
                                Ok(frame) => frame,
                                Err(error) => { warn!(error = %error, line = %line, "Invalid Codex app-server frame"); continue; }
                            }
                            }
                        }
                    };
                    if frame.get("id").is_some() && frame.get("method").is_some() {
                        if !handle_server_request(
                            &app_handle,
                            conversation_id,
                            &mut stdin,
                            &frame,
                            Some(&request_cancellation),
                        )
                        .await? {
                            warn!(
                                conversation_id,
                                method = frame.get("method").and_then(|value| value.as_str()).unwrap_or("unknown"),
                                "Unsupported Codex server request"
                            );
                            write_frame(
                                &mut stdin,
                                &json!({
                                    "jsonrpc":"2.0",
                                    "id":frame.get("id").cloned().unwrap_or(Value::Null),
                                    "error":{"code":-32601,"message":format!("AIPP does not support Codex server request {}", frame.get("method").and_then(Value::as_str).unwrap_or("unknown"))}
                                }),
                            )
                            .await?;
                        }
                        continue;
                    }
                    if frame.get("method").is_none() {
                        if let Some(reply) = frame["id"].as_u64().and_then(|id| steer_receipts.remove(&id)) {
                            let result = if let Some(error) = frame.get("error") { Err(format!("Codex turn/steer: {error}")) }
                                else if frame.pointer("/result/turnId").and_then(Value::as_str) == turn_id.as_deref() { Ok(()) }
                                else { Err(format!("Codex turn/steer returned an unexpected turn: {}", frame["result"])) };
                            let _ = reply.send(result);
                            continue;
                        }
                        restore.acknowledge(&thread_id, &frame)?;
                        continue;
                    }
                    if !codex_notification_matches_turn(&frame, &thread_id, turn_id.as_deref()) {
                        debug!(conversation_id, thread_id, turn_id = ?turn_id, "Ignoring Codex notification for another thread or turn");
                        continue;
                    }
                    let method = frame.get("method").and_then(Value::as_str).unwrap_or_default();
                    let params = frame.get("params").cloned().unwrap_or(Value::Null);
                    restore.on_notification(method, &params, manual_compact, &thread_id,
                        &application_context, &mut request_id, &mut stdin).await?;
                    usage_tracker.note_notification(method, &params);
                    let mut output_message_id = None;
                    match method {
                                "turn/started" => {
                                    turn_id = params.pointer("/turn/id").and_then(Value::as_str).map(str::to_string).or(turn_id);
                                    snapshot.current_turn_id = turn_id.clone();
                                    event_identity.turn_id = turn_id.clone();
                                }
                                "item/agentMessage/delta" => {
                                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                                        if !delta.is_empty() && first_any_token_time.is_none() {
                                            first_any_token_time = Some(chrono::Utc::now());
                                        }
                                        content.push_str(delta);
                                        persist_response(&app_handle, message_id, &content, false, None, None);
                                        let event = ConversationEvent { r#type: "message_update".to_string(), data: serde_json::to_value(MessageUpdateEvent { message_id, message_type: "response".to_string(), content: content.clone(), is_done: false, token_count: None, input_token_count: None, output_token_count: None, ttft_ms: None, tps: None }).unwrap() };
                                        let _ = emit_to_window(&window, event_identity.clone(), event);
                                    }
                                }
                                "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                                        if !delta.is_empty() && first_any_token_time.is_none() {
                                            first_any_token_time = Some(chrono::Utc::now());
                                        }
                                        reasoning.push_str(delta);
                                        if reasoning_message_id.is_none() {
                                            if let Ok(message) = crate::api::ai_api::add_message(
                                                &app_handle,
                                                Some(message_id),
                                                conversation_id,
                                                "reasoning".to_string(),
                                                String::new(),
                                                Some(0),
                                                Some("codex-app-server".to_string()),
                                                Some(chrono::Utc::now()),
                                                None,
                                                0,
                                                None,
                                                None,
                                            ) {
                                                reasoning_message_id = Some(message.id);
                                                let add_event = ConversationEvent {
                                                    r#type: "message_add".to_string(),
                                                    data: json!({"message_id":message.id,"message_type":"reasoning"}),
                                                };
                                                let _ = emit_to_window(&window, event_identity.clone(), add_event);
                                            }
                                        }
                                        if let Some(reasoning_id) = reasoning_message_id {
                                            persist_response(&app_handle, reasoning_id, &reasoning, false, None, None);
                                            let event = ConversationEvent {
                                                r#type: "message_update".to_string(),
                                                data: serde_json::to_value(MessageUpdateEvent {
                                                    message_id: reasoning_id,
                                                    message_type: "reasoning".to_string(),
                                                    content: reasoning.clone(),
                                                    is_done: false,
                                                    token_count: None,
                                                    input_token_count: None,
                                                    output_token_count: None,
                                                    ttft_ms: None,
                                                    tps: None,
                                                }).unwrap(),
                                            };
                                            let _ = emit_to_window(&window, event_identity.clone(), event);
                                        }
                                    }
                                }
                                "thread/compacted" => {},
                                "item/started" | "item/completed" => {
                                    sequence += 1;
                                    if let Some(item) = merge_activity_notification(&mut activity_items, method, &params) {
                                        let item_type = item.get("type").and_then(Value::as_str);
                                        if item_type == Some("plan") {
                                            let item_id = item.get("id").and_then(Value::as_str).unwrap_or("plan").to_string();
                                            if method == "item/completed" {
                                                let authoritative_text = item.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
                                                if !authoritative_text.is_empty() {
                                                    if !plan_messages.contains_key(&item_id) {
                                                        let plan_message = AgentPlanMessage::create_with_identity(
                                                            &app_handle,
                                                            &window,
                                                            conversation_id,
                                                            message_id,
                                                            CODEX_APP_SERVER_API_TYPE,
                                                            Some(&thread_id),
                                                            turn_id.as_deref(),
                                                            Some(&item_id), event_identity.clone())?;
                                                        plan_messages.insert(item_id.clone(), plan_message);
                                                    }
                                                    if let Some(plan_message) = plan_messages.get_mut(&item_id) {
                                                        output_message_id = Some(plan_message.message_id);
                                                        plan_message.replace_content(
                                                            &app_handle,
                                                            &window,
                                                            conversation_id,
                                                            authoritative_text,
                                                            true,
                                                        );
                                                    }
                                                }
                                            }
                                        } else if item_type == Some("agentMessage") {
                                            if method == "item/completed" {
                                                final_text = if item["phase"] == "commentary" { Some(String::new()) }
                                                    else { item["text"].as_str().map(str::to_owned) };
                                            }
                                        } else {
                                            let offset = activity_content_offset(&mut item_content_offsets, &item, &content);
                                            let order = activity_order(&mut item_orders, &item);
                                            output_message_id = emit_activity(event_identity, &window, conversation_id, message_id, Some(&thread_id), sequence, &item, if method == "item/completed" { "success" } else { "executing" }, offset, order);
                                        }
                                    }
                                }
                                "item/plan/delta" => {
                                    if let (Some(item_id), Some(delta)) = (
                                        params.get("itemId").and_then(Value::as_str),
                                        params.get("delta").and_then(Value::as_str),
                                    ) {
                                        if !plan_messages.contains_key(item_id) {
                                            let plan_message = AgentPlanMessage::create_with_identity(
                                                &app_handle,
                                                &window,
                                                conversation_id,
                                                message_id,
                                                CODEX_APP_SERVER_API_TYPE,
                                                Some(&thread_id),
                                                params.get("turnId").and_then(Value::as_str).or(turn_id.as_deref()),
                                                Some(item_id), event_identity.clone())?;
                                            plan_messages.insert(item_id.to_string(), plan_message);
                                        }
                                        if let Some(plan_message) = plan_messages.get_mut(item_id) {
                                            plan_message.append_delta(&app_handle, &window, conversation_id, delta);
                                        }
                                    }
                                }
                                "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" | "item/fileChange/patchUpdated" | "item/mcpToolCall/progress" => {
                                    sequence += 1;
                                    if let Some(item) = merge_activity_notification(&mut activity_items, method, &params) {
                                        let offset = activity_content_offset(&mut item_content_offsets, &item, &content);
                                        let order = activity_order(&mut item_orders, &item);
                                        output_message_id = emit_activity(event_identity, &window, conversation_id, message_id, Some(&thread_id), sequence, &item, "executing", offset, order);
                                    }
                                }
                                "turn/plan/updated" => {
                                    snapshot.plan = codex_plan_payload(&params);
                                    snapshot.plan_explanation = params
                                        .get("explanation")
                                        .and_then(Value::as_str)
                                        .map(str::to_string);
                                    emit_session_snapshot(event_identity,
                                        &app_handle,
                                        conversation_id,
                                        Some(snapshot.clone()),
                                    )
                                    .await;
                                }
                                "thread/tokenUsage/updated" => {
                                    usage_tracker.observe(&params);
                                }
                                "turn/completed" => {
                                    let turn = params.get("turn").unwrap_or(&params);
                                    if turn.get("status").and_then(Value::as_str) == Some("failed") {
                                        turn_error = turn
                                            .pointer("/error/message")
                                            .and_then(Value::as_str)
                                            .map(str::to_string)
                                            .or_else(|| Some("Codex turn failed".to_string()));
                                    }
                                    completed_normally = turn.get("status").and_then(Value::as_str) == Some("completed");
                                    turn_completed = true;
                                },
                                "mcpServer/startupStatus/updated" => {
                                    let server_name = params.get("name").and_then(Value::as_str).unwrap_or("unknown");
                                    let status = params.get("status").and_then(Value::as_str).unwrap_or("unknown");
                                    if status == "failed" {
                                        let reason = params
                                            .get("error")
                                            .or_else(|| params.get("failureReason"))
                                            .map(Value::to_string)
                                            .unwrap_or_else(|| "未知错误".to_string());
                                        return Err(format!("Codex MCP server {server_name} 启动失败：{reason}"));
                                    } else {
                                        debug!(conversation_id, server = server_name, status, "Codex MCP server startup status");
                                    }
                                }
                                "error" => {
                                    let message = params.get("message").and_then(Value::as_str).unwrap_or("Codex app-server error");
                                    return Err(message.to_string());
                                }
                                _ => {
                                    if let Some(item) = generic_item_notification(method, &params) {
                                        sequence += 1;
                                        let offset = activity_content_offset(&mut item_content_offsets, &item, &content);
                                        let order = activity_order(&mut item_orders, &item);
                                        output_message_id = emit_activity(event_identity, &window, conversation_id, message_id, Some(&thread_id), sequence, &item, "executing", offset, order);
                                    } else if !method.is_empty() {
                                        warn!(conversation_id, method, "Unhandled Codex notification");
                                    }
                                }
                    }
                    let output_message_id = output_message_id.or_else(|| codex_delta_focus(
                        method, &params, message_id, reasoning_message_id,
                        |item_id| plan_messages.get(item_id).map(|plan| plan.message_id),
                    ));
                    if let Some(id) = output_message_id.filter(|id| Some(*id) != focused_message_id) {
                        app_handle.state::<ConversationActivityManager>().set_agent_streaming(&app_handle, event_identity, id).await;
                        focused_message_id = Some(id);
                    }
                    }
                let turn_usage = usage_tracker.turn_usage();
                if content.is_empty() {
                    if let Some(text) = &final_text { content.clone_from(text); }
                }
                if manual_compact && turn_error.is_none() && content.is_empty() {
                    content = super::agent_driver::manual_compact::compact_done_text(None);
                    final_text = None;
                }
                if turn_error.is_none() && content.is_empty() && reasoning.is_empty() && plan_messages.is_empty() {
                    turn_error = Some("Codex 本轮未返回任何内容".to_string());
                }
                if let Some(error) = turn_error {
                    return Err(error);
                }
                persist_codex_timing(&app_handle, message_id, turn_start_time, first_any_token_time);
                for plan_message in plan_messages.values_mut() {
                    if !plan_message.content.is_empty() {
                        plan_message.replace_content(
                            &app_handle,
                            &window,
                            conversation_id,
                            plan_message.content.clone(),
                            true,
                        );
                    }
                }
                let published = if completed_normally { final_text.as_deref() } else { Some("") };
                persist_response(&app_handle, message_id, &content, true, turn_usage, published);
                if let Some(reasoning_id) = reasoning_message_id {
                    persist_codex_reasoning(&app_handle, reasoning_id, &reasoning);
                    let _ = emit_to_window(
                        &window, event_identity.clone(),
                        ConversationEvent {
                            r#type: "message_update".to_string(),
                            data: serde_json::to_value(MessageUpdateEvent {
                                message_id: reasoning_id,
                                message_type: "reasoning".to_string(),
                                content: reasoning.clone(),
                                is_done: true,
                                token_count: Some(0),
                                input_token_count: None,
                                output_token_count: Some(0),
                                ttft_ms: None,
                                tps: None,
                            }).unwrap(),
                        },
                    );
                }
                let estimated_output_tokens = ((content.chars().count() + 3) / 4) as i32;
                let finish_time = chrono::Utc::now();
                let ttft_ms = first_any_token_time.map(|first| (first.timestamp_millis() - turn_start_time.timestamp_millis()).max(0));
                let output_tokens = turn_usage.map(|usage| usage.output_tokens).unwrap_or(estimated_output_tokens);
                let tps = first_any_token_time.and_then(|first| {
                    let duration_ms = finish_time.timestamp_millis() - first.timestamp_millis();
                    (output_tokens > 0 && duration_ms > 0).then(|| output_tokens as f64 * 1000.0 / duration_ms as f64)
                });
                let done_event = ConversationEvent { r#type: "message_update".to_string(), data: serde_json::to_value(MessageUpdateEvent { message_id, message_type: "response".to_string(), content: content.clone(), is_done: true, token_count: Some(turn_usage.map(|usage| usage.total_tokens).unwrap_or(estimated_output_tokens)), input_token_count: turn_usage.map(|usage| usage.input_tokens), output_token_count: Some(output_tokens), ttft_ms, tps }).unwrap() };
                let _ = emit_to_window(&window, event_identity.clone(), done_event);
                let complete_event = ConversationEvent { r#type: "stream_complete".to_string(), data: json!({"conversation_id":conversation_id,"response_message_id":message_id,"reasoning_message_id":reasoning_message_id,"has_response":!content.is_empty(),"has_reasoning":!reasoning.is_empty(),"has_plan":!plan_messages.is_empty(),"response_length":content.len(),"reasoning_length":reasoning.len()}) };
                let _ = emit_to_window(&window, event_identity.clone(), complete_event);
                let success_content = if content.is_empty() {
                    plan_messages.values().max_by_key(|plan| plan.message_id).map(|plan| plan.content.as_str()).unwrap_or_default()
                } else {
                    content.as_str()
                };
                handle_agent_success(&app_handle, &window, conversation_id, success_content, message_id, AgentKind::Codex).await;
                if let Some(manager) = app_handle.try_state::<ConversationActivityManager>() { manager.clear_agent_focus(&app_handle, &event_identity, false).await; }
                snapshot.current_turn_id = None;
                snapshot.has_active_prompt = false;
                emit_session_snapshot(event_identity, &app_handle, conversation_id, Some(snapshot.clone())).await;
                if let Some(id) = turn_id { finished_turns.insert(id); }
                previous = completed_normally.then(|| (message_id, window.clone(), cancellation_generation));
                failure_context.lock().unwrap_or_else(|e| e.into_inner()).take_if(|(id, _)| *id == message_id);
                crate::api::ai_api::native_queue::finish_turn(&app_handle, &window, conversation_id, run_id, completed_normally).await;
            }
        }
    }
    info!(conversation_id, "Codex app-server session command channel closed");
    if failure_context
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_some()
    {
        Err("AIPP Codex 控制通道意外关闭：当前请求尚未完成，但所有会话句柄已释放".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_thread_id_from_v2_response() {
        assert_eq!(thread_id_from_result(&json!({"thread":{"id":"thread-1"}})).as_deref(), Some("thread-1"));
    }

    #[test]
    fn appends_bounded_stderr_and_runtime_ids_to_error() {
        let stderr = CodexStderrBuffer::default();
        for index in 0..=CODEX_STDERR_MAX_LINES {
            record_codex_stderr(&stderr, format!("stderr-{index}"));
        }
        let error = codex_error_with_diagnostics("RPC initialize 失败", 792, "run-1", &stderr);
        assert!(error.contains("RPC initialize 失败"));
        assert!(error.contains("conversation_id=792"));
        assert!(error.contains("run_id=run-1"));
        assert!(!error.contains("stderr-0\n"));
        assert!(error.contains(&format!("stderr-{}", CODEX_STDERR_MAX_LINES)));
    }

    /// 剥离 ANSI 转义：含颜色码的 tracing 行变为纯文本，普通文本原样保留
    #[test]
    fn strips_ansi_escape_sequences_from_tracing_lines() {
        let colored = "\u{1b}[2m2026-08-25T08:27:29.183809Z\u{1b}[0m \u{1b}[33m WARN\u{1b}[0m \u{1b}[1msession_loop\u{1b}[0m\u{1b}[2m:\u{1b}[0m \u{1b}[2mcodex_core::responses_retry\u{1b}[0m\u{1b}[2m:\u{1b}[0m stream disconnected";
        assert_eq!(
            strip_ansi_escapes(colored),
            "2026-08-25T08:27:29.183809Z  WARN session_loop: codex_core::responses_retry: stream disconnected"
        );
        assert_eq!(strip_ansi_escapes("plain output"), "plain output");
        // 孤立的末尾 ESC 不产生乱码
        assert_eq!(strip_ansi_escapes("truncated\u{1b}"), "truncated");
    }

    /// 噪音判定：tracing 的 TRACE/DEBUG/INFO 行被过滤，WARN/ERROR 与非 tracing 格式行保留
    #[test]
    fn filters_tracing_info_noise_but_keeps_warnings_and_plain_output() {
        assert!(is_tracing_noise("2026-08-25T08:27:29.181105Z  INFO session_loop{thread_id=abc}: codex_core::tasks: enter"));
        assert!(is_tracing_noise("2026-08-25T08:27:29.181105Z DEBUG codex_core: detail"));
        assert!(is_tracing_noise("2026-08-25T08:27:29.181105Z TRACE codex_core: detail"));
        assert!(!is_tracing_noise("2026-08-25T08:27:29.183809Z  WARN codex_core::responses_retry: stream disconnected"));
        assert!(!is_tracing_noise("2026-08-25T08:27:29.183809Z ERROR codex_core: fatal"));
        assert!(!is_tracing_noise("thread 'main' panicked at 'boom', src/main.rs:10:5"));
        assert!(!is_tracing_noise("random process output"));
    }

    /// 缓冲只保留清洗后的关键行：INFO 噪音与空行不进缓冲，错误消息带过滤说明
    #[test]
    fn stderr_buffer_keeps_only_meaningful_lines() {
        let stderr = CodexStderrBuffer::default();
        let noise = "2026-08-25T08:27:29.181105Z  INFO session_loop: codex_core::tasks: enter";
        let warning = "\u{1b}[2m2026-08-25T08:27:29.183809Z\u{1b}[0m \u{1b}[33m WARN\u{1b}[0m codex_core::responses_retry: stream disconnected";
        assert!(!record_codex_stderr(&stderr, noise.to_string()));
        assert!(!record_codex_stderr(&stderr, "\u{1b}[2m\u{1b}[0m".to_string()));
        assert!(record_codex_stderr(&stderr, warning.to_string()));
        assert!(record_codex_stderr(&stderr, "plain stderr output".to_string()));

        let error = codex_error_with_diagnostics("turn 失败", 1, "run-1", &stderr);
        assert!(error.contains("WARN codex_core::responses_retry: stream disconnected"));
        assert!(error.contains("plain stderr output"));
        assert!(!error.contains("codex_core::tasks: enter"));
        assert!(!error.contains('\u{1b}'));
        assert!(error.contains("已过滤 INFO/DEBUG 噪音"));
    }

    #[tokio::test]
    async fn shutdown_command_preserves_replacement_reason() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let handle = CodexSessionHandle {
            sender,
            run_id: "old-run".to_string(),
            cancellation: watch::channel(0).0,
            failure_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
        };
        handle.shutdown("auto-connect attempted replacement".to_string());
        let Some(CodexSessionCommand::Shutdown { reason }) = receiver.recv().await else {
            panic!("expected shutdown command");
        };
        assert_eq!(reason, "auto-connect attempted replacement");
    }

    #[test]
    fn maps_command_item_without_numeric_id() {
        let item = json!({"type":"commandExecution","id":"item-abc","command":"cargo check","cwd":"C:/repo","status":"inProgress"});
        let (id, kind, title, input) = item_identity(&item);
        assert_eq!(id, "item-abc");
        assert_eq!(kind, "command");
        assert_eq!(title.as_deref(), Some("cargo check"));
        assert_eq!(input.unwrap()["cwd"], "C:/repo");
        assert_eq!(activity_status(&item, "pending"), "executing");
    }

    #[test]
    fn maps_context_compaction_item_to_readable_status() {
        let item = json!({"type":"contextCompaction","id":"item-compact"});
        let (id, kind, title, input) = item_identity(&item);
        assert_eq!(id, "item-compact");
        assert_eq!(kind, "status");
        assert_eq!(title.as_deref(), Some("压缩上下文"));
        assert!(input.is_none());
    }

    /// item 首次出现时记录当前正文字符数，后续更新沿用同一偏移
    #[test]
    fn pins_content_offset_at_first_seen() {
        let mut offsets = HashMap::new();
        let item = json!({"id":"item-1","type":"commandExecution"});
        let first = activity_content_offset(&mut offsets, &item, "你好");
        assert_eq!(first, Some(2));
        // 正文继续增长后再次取偏移，仍返回首次记录的值
        let second = activity_content_offset(&mut offsets, &item, "你好世界");
        assert_eq!(second, Some(2));
        // 另一个 item 记录自己的偏移
        let other = activity_content_offset(&mut offsets, &json!({"id":"item-2","type":"commandExecution"}), "你好世界");
        assert_eq!(other, Some(4));
        // 没有 id 的 item 拿不到偏移
        assert_eq!(activity_content_offset(&mut offsets, &json!({"type":"commandExecution"}), "你好世界"), None);
    }

    #[test]
    fn maps_codex_approval_decisions_to_protocol_responses() {
        let params = json!({"availableDecisions":["accept","decline"]});
        let options = codex_permission_options("item/commandExecution/requestApproval", &params);
        assert_eq!(options.iter().map(|item| item.option_id.as_str()).collect::<Vec<_>>(), vec!["accept", "decline"]);
        assert_eq!(
            codex_approval_response(
                "item/commandExecution/requestApproval",
                &params,
                AcpPermissionDecision::Selected("accept".to_string())
            ),
            json!({"decision":"accept"})
        );
        assert_eq!(
            codex_approval_response(
                "item/permissions/requestApproval",
                &json!({"permissions":{"fileSystem":{"entries":[]}}}),
                AcpPermissionDecision::Selected("acceptForSession".to_string())
            )["scope"],
            "session"
        );
    }

    #[test]
    fn parses_model_catalog_and_switches_reasoning_capabilities() {
        let catalog = model_catalog(&json!({"data":[
            {"id":"gpt-a","displayName":"GPT A","supportedReasoningEfforts":[{"effort":"low"},{"effort":"high"}],"defaultReasoningEffort":"low","isDefault":true},
            {"id":"gpt-b","displayName":"GPT B","supportedReasoningEfforts":["none","medium"],"isDefault":false}
        ]}));
        assert_eq!(catalog.len(), 2);
        assert!(catalog[0].is_default);
        let mut snapshot = CodexConversationSessionState {
            model: Some("gpt-a".to_string()), reasoning_effort: Some("high".to_string()), ..Default::default()
        };
        snapshot.config_options = vec![
            CodexSessionConfigOption { id: "model".to_string(), name: "模型".to_string(), description: None, category: Some("model".to_string()), current_value: "gpt-a".to_string(), options: vec![] },
            CodexSessionConfigOption { id: "reasoning_effort".to_string(), name: "思考强度".to_string(), description: None, category: Some("thought_level".to_string()), current_value: "high".to_string(), options: vec![] },
        ];
        apply_codex_config_option(&mut snapshot, &catalog, "model", "gpt-b".to_string()).unwrap();
        assert_eq!(snapshot.model.as_deref(), Some("gpt-b"));
        assert_eq!(snapshot.reasoning_effort.as_deref(), Some("none"));
        assert_eq!(snapshot.config_options[1].options.len(), 2);
    }

    #[test]
    fn parses_effective_codex_config_model_defaults() {
        let defaults = codex_config_model_defaults(&json!({
            "config": {
                "model": "gpt-configured",
                "model_reasoning_effort": "high"
            }
        }));
        assert_eq!(defaults.0.as_deref(), Some("gpt-configured"));
        assert_eq!(defaults.1.as_deref(), Some("high"));
    }

    #[test]
    fn does_not_invent_model_choice_when_catalog_is_empty() {
        assert!(codex_model_choices(&[]).is_empty());
    }

    #[test]
    fn builds_turn_start_with_runtime_snapshot_values() {
        let snapshot = CodexConversationSessionState {
            model: Some("gpt-a".to_string()),
            reasoning_effort: Some("ultra".to_string()),
            collaboration_mode: Some("plan".to_string()),
            ..Default::default()
        };
        let params = codex_turn_start_params("thread-1", &snapshot, "hello", &Default::default());
        assert_eq!(params["model"], "gpt-a");
        assert_eq!(params["reasoningEffort"], "ultra");
        assert_eq!(params["collaborationMode"]["mode"], "plan");
        assert_eq!(params["collaborationMode"]["settings"]["model"], "gpt-a");
    }

    #[test]
    fn detects_and_parses_codex_plan_capabilities() {
        assert!(codex_supports_plan_mode(&json!({
            "data": [{"mode": "default"}, {"mode": "plan"}]
        })));
        assert!(!codex_supports_plan_mode(&json!({
            "data": [{"mode": "default"}]
        })));

        let plan = codex_plan_payload(&json!({
            "explanation": "先确认再执行",
            "plan": [
                {"step": "检查现状", "status": "completed"},
                {"step": "实现交互", "status": "inProgress"},
                {"step": "验证结果", "status": "pending"}
            ]
        }));
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].content, "检查现状");
        assert_eq!(plan[0].status, "completed");
        assert_eq!(plan[1].status, "in_progress");
        assert_eq!(plan[2].status, "pending");
    }

    #[test]
    fn applies_codex_collaboration_mode_only_when_supported() {
        let mut snapshot = CodexConversationSessionState {
            collaboration_mode: Some("default".to_string()),
            config_options: vec![CodexSessionConfigOption {
                id: "collaboration_mode".to_string(),
                name: "工作模式".to_string(),
                description: None,
                category: Some("mode".to_string()),
                current_value: "default".to_string(),
                options: codex_collaboration_mode_options(),
            }],
            ..Default::default()
        };
        apply_codex_config_option(
            &mut snapshot,
            &[],
            "collaboration_mode",
            "plan".to_string(),
        )
        .unwrap();
        assert_eq!(snapshot.collaboration_mode.as_deref(), Some("plan"));
        assert_eq!(snapshot.config_options[0].current_value, "plan");

        let mut unsupported = CodexConversationSessionState::default();
        assert!(apply_codex_config_option(
            &mut unsupported,
            &[],
            "collaboration_mode",
            "plan".to_string(),
        )
        .is_err());
    }

    /// turn/start 的审批策略与沙箱覆盖
    ///
    /// 验证内容：
    /// - 未设置时输出 null（schema 允许 null）
    /// - 设置后 approvalPolicy 原样下发字符串
    /// - sandboxPolicy 从 kebab-case 字符串映射为 camelCase type 对象
    #[test]
    fn builds_turn_start_with_approval_and_sandbox_overrides() {
        let default_snapshot = CodexConversationSessionState::default();
        let default_params = codex_turn_start_params("thread-1", &default_snapshot, "hello", &Default::default());
        assert!(default_params["approvalPolicy"].is_null());
        assert!(default_params["sandboxPolicy"].is_null());

        let snapshot = CodexConversationSessionState {
            approval_policy: Some("never".to_string()),
            sandbox: Some("workspace-write".to_string()),
            approvals_reviewer: Some("auto_review".to_string()),
            ..Default::default()
        };
        let params = codex_turn_start_params("thread-1", &snapshot, "hello", &Default::default());
        assert_eq!(params["approvalPolicy"], "never");
        assert_eq!(params["sandboxPolicy"], json!({"type": "workspaceWrite"}));
        assert_eq!(params["approvalsReviewer"], "auto_review");
    }

    /// sandboxPolicy 的 kebab-case → 对象映射
    ///
    /// 验证内容：
    /// - 三种合法沙箱模式分别映射到 readOnly / workspaceWrite / dangerFullAccess
    /// - None 与非法值都返回 None
    #[test]
    fn maps_sandbox_mode_to_turn_sandbox_policy() {
        assert_eq!(codex_sandbox_policy_json(Some("read-only")), Some(json!({"type": "readOnly"})));
        assert_eq!(codex_sandbox_policy_json(Some("workspace-write")), Some(json!({"type": "workspaceWrite"})));
        assert_eq!(codex_sandbox_policy_json(Some("danger-full-access")), Some(json!({"type": "dangerFullAccess"})));
        assert_eq!(codex_sandbox_policy_json(None), None);
        assert_eq!(codex_sandbox_policy_json(Some("bogus")), None);
    }

    /// 会话内切换审批策略与沙箱模式
    ///
    /// 验证内容：
    /// - 合法值更新 snapshot 字段与对应 config_option 的 current_value
    /// - 非法值直接报错且不修改 snapshot
    #[test]
    fn applies_approval_policy_and_sandbox_config_options() {
        let mut snapshot = CodexConversationSessionState::default();
        snapshot.config_options = vec![
            CodexSessionConfigOption { id: "approval_policy".to_string(), name: "审批策略".to_string(), description: None, category: Some("approval".to_string()), current_value: "on-request".to_string(), options: codex_approval_policy_options() },
            CodexSessionConfigOption { id: "sandbox".to_string(), name: "沙箱模式".to_string(), description: None, category: Some("sandbox".to_string()), current_value: "workspace-write".to_string(), options: codex_sandbox_options() },
        ];

        apply_codex_config_option(&mut snapshot, &[], "approval_policy", "untrusted".to_string()).unwrap();
        assert_eq!(snapshot.approval_policy.as_deref(), Some("untrusted"));
        assert_eq!(snapshot.config_options[0].current_value, "untrusted");

        apply_codex_config_option(&mut snapshot, &[], "sandbox", "read-only".to_string()).unwrap();
        assert_eq!(snapshot.sandbox.as_deref(), Some("read-only"));
        assert_eq!(snapshot.config_options[1].current_value, "read-only");

        assert!(apply_codex_config_option(&mut snapshot, &[], "approval_policy", "on-failure".to_string()).is_err());
        assert!(apply_codex_config_option(&mut snapshot, &[], "sandbox", "full-access".to_string()).is_err());
        assert_eq!(snapshot.approval_policy.as_deref(), Some("untrusted"));
        assert_eq!(snapshot.sandbox.as_deref(), Some("read-only"));
    }

    #[test]
    fn maps_codex_request_user_input_to_aipp_and_back() {
        let params = json!({
            "questions": [
                {
                    "id": "scope",
                    "header": "范围",
                    "question": "选择范围",
                    "options": [
                        {"label": "当前文件", "description": "只处理当前文件"},
                        {"label": "全部文件", "description": "处理所有文件"}
                    ]
                },
                {
                    "id": "note",
                    "header": "备注",
                    "question": "补充说明",
                    "options": null,
                    "isSecret": true
                }
            ]
        });
        let (request, mappings) = codex_user_input_request(&params).unwrap();
        assert_eq!(request.questions.len(), 2);
        assert!(request.questions[1].options.is_empty());
        assert!(request.questions[1].is_secret);

        let answers = HashMap::from([
            ("选择范围".to_string(), "全部文件".to_string()),
            ("补充说明".to_string(), "仅测试".to_string()),
        ]);
        let response = codex_user_input_response(&mappings, &answers);
        assert_eq!(response["answers"]["scope"]["answers"], json!(["全部文件"]));
        assert_eq!(response["answers"]["note"]["answers"], json!(["仅测试"]));
    }

    #[test]
    fn reads_exact_turn_usage_and_degrades_unknown_item_notification() {
        let usage = codex_turn_usage(&json!({
            "tokenUsage": {
                "last": {
                    "inputTokens": 12,
                    "outputTokens": 7,
                    "reasoningOutputTokens": 3,
                    "cachedInputTokens": 5,
                    "cacheWriteInputTokens": 2,
                    "totalTokens": 19
                }
            }
        }));
        assert_eq!(
            usage,
            Some(CodexTurnUsage {
                input_tokens: 12,
                output_tokens: 7,
                reasoning_tokens: 3,
                cached_input_tokens: 5,
                cache_write_input_tokens: 2,
                total_tokens: 19,
            })
        );

        let item = generic_item_notification(
            "item/futureFeature/progress",
            &json!({"itemId":"future-1","value":42}),
        )
        .unwrap();
        assert_eq!(item["id"], "future-1");
        assert_eq!(item["type"], "futureFeature/progress");
    }

    fn usage_notification(total: (i64, i64, i64, i64), last: (i64, i64, i64, i64)) -> Value {
        let breakdown = |(input, cached, output, total): (i64, i64, i64, i64)| json!({
            "inputTokens": input, "cachedInputTokens": cached, "outputTokens": output,
            "reasoningOutputTokens": 0, "totalTokens": total
        });
        json!({"threadId": "t", "turnId": "u", "tokenUsage": {"total": breakdown(total), "last": breakdown(last)}})
    }

    #[test]
    fn test_codex_turn_usage_sums_every_request_in_turn() {
        let mut tracker = CodexTurnUsageTracker::default();
        tracker.note_notification("item/started", &json!({"item": {"type": "userMessage"}}));
        // 第一次请求开始时带着上一轮的旧数据
        tracker.observe(&usage_notification((1000, 800, 100, 1100), (300, 200, 40, 340)));
        tracker.note_notification("item/started", &json!({"item": {"type": "reasoning"}}));
        // 第一次请求完成：调用了工具
        tracker.observe(&usage_notification((1500, 1200, 150, 1650), (500, 400, 50, 550)));
        // 第二次请求开始时又推了一遍同样的数据
        tracker.observe(&usage_notification((1500, 1200, 150, 1650), (500, 400, 50, 550)));
        tracker.note_notification("item/agentMessage/delta", &json!({"delta": "ok"}));
        tracker.observe(&usage_notification((2100, 1700, 230, 2330), (600, 500, 80, 680)));

        let usage = tracker.turn_usage().unwrap();
        assert_eq!((usage.input_tokens, usage.cached_input_tokens, usage.output_tokens, usage.total_tokens),
            (1100, 900, 130, 1230));
    }

    #[test]
    fn test_codex_turn_usage_derives_baseline_without_stale_notification() {
        let mut tracker = CodexTurnUsageTracker::default();
        tracker.note_notification("item/agentMessage/delta", &json!({"delta": "a"}));
        tracker.observe(&usage_notification((1500, 1200, 150, 1650), (500, 400, 50, 550)));
        tracker.observe(&usage_notification((2100, 1700, 230, 2330), (600, 500, 80, 680)));
        let usage = tracker.turn_usage().unwrap();
        assert_eq!((usage.input_tokens, usage.output_tokens, usage.total_tokens), (1100, 130, 1230));
    }

    #[test]
    fn test_codex_turn_usage_without_total_falls_back_to_last() {
        let mut tracker = CodexTurnUsageTracker::default();
        tracker.note_notification("item/agentMessage/delta", &json!({"delta": "a"}));
        tracker.observe(&json!({"tokenUsage": {"last": {"inputTokens": 12, "outputTokens": 7, "totalTokens": 19}}}));
        assert_eq!(tracker.turn_usage().map(|u| u.total_tokens), Some(19));
        assert!(CodexTurnUsageTracker::default().turn_usage().is_none());
    }

    #[test]
    fn merges_command_output_deltas_by_string_item_id() {
        let mut items = HashMap::new();
        let first = merge_activity_notification(
            &mut items,
            "item/commandExecution/outputDelta",
            &json!({"itemId":"item-1","delta":"one\n"}),
        )
        .unwrap();
        let second = merge_activity_notification(
            &mut items,
            "item/commandExecution/outputDelta",
            &json!({"itemId":"item-1","delta":"two"}),
        )
        .unwrap();
        assert_eq!(first["aggregatedOutput"], "one\n");
        assert_eq!(second["aggregatedOutput"], "one\ntwo");
    }

    fn model_config(name: &str, value: &str) -> crate::db::assistant_db::AssistantModelConfig {
        crate::db::assistant_db::AssistantModelConfig {
            id: 0,
            assistant_id: 0,
            assistant_model_id: 0,
            name: name.to_string(),
            value: Some(value.to_string()),
            value_type: "string".to_string(),
        }
    }

    fn provider_config(name: &str, value: &str) -> crate::db::llm_db::LLMProviderConfig {
        crate::db::llm_db::LLMProviderConfig {
            id: 0,
            name: name.to_string(),
            llm_provider_id: 0,
            value: value.to_string(),
            append_location: String::new(),
            is_addition: false,
        }
    }

    /// 助手级配置（assistant_model_config）应优先于 provider 默认值
    ///
    /// 验证内容：
    /// - 工作目录/附加启动参数：助手级覆盖 provider 级
    /// - 环境变量：provider 表单 JSON 格式与助手级 KEY=VALUE 行格式均可解析
    /// - 同名环境变量助手级优先
    #[test]
    fn assistant_level_config_overrides_provider_defaults() {
        let model_configs = vec![
            model_config("acp_working_directory", "/tmp/assistant-cwd"),
            model_config("acp_additional_args", "--enable collaboration_modes"),
            model_config("acp_env_vars", "ASSISTANT_KEY=assistant\nSHARED_KEY=from-assistant"),
        ];
        let provider_configs = vec![
            provider_config("acp_working_directory", "/tmp/provider-cwd"),
            provider_config("codex_additional_args", "--profile provider"),
            provider_config(
                "acp_env_vars",
                "{\"SHARED_KEY\":\"from-provider\",\"PROVIDER_KEY\":\"provider\"}",
            ),
        ];
        let config = extract_codex_app_server_config(&model_configs, &provider_configs, None).unwrap();
        assert_eq!(config.working_directory, PathBuf::from("/tmp/assistant-cwd"));
        assert_eq!(
            config.additional_args,
            vec!["--enable".to_string(), "collaboration_modes".to_string()]
        );
        assert_eq!(config.env_vars.get("ASSISTANT_KEY").map(String::as_str), Some("assistant"));
        assert_eq!(config.env_vars.get("SHARED_KEY").map(String::as_str), Some("from-assistant"));
        assert_eq!(config.env_vars.get("PROVIDER_KEY").map(String::as_str), Some("provider"));
    }

    /// 未设置助手级覆盖时回退到 provider 默认值
    #[test]
    fn provider_defaults_apply_without_assistant_overrides() {
        let provider_configs = vec![
            provider_config("acp_working_directory", "/tmp/provider-cwd"),
            provider_config("codex_additional_args", "--profile provider"),
        ];
        let config = extract_codex_app_server_config(&[], &provider_configs, None).unwrap();
        assert_eq!(config.working_directory, PathBuf::from("/tmp/provider-cwd"));
        assert_eq!(
            config.additional_args,
            vec!["--profile".to_string(), "provider".to_string()]
        );
    }

    /// 审批策略/沙箱模式的兜底与校验
    ///
    /// 验证内容：
    /// - 助手/provider 都未配置时使用内置默认值（on-request / workspace-write），保证界面可反显
    /// - 助手级覆盖优先于 provider 级
    /// - 非法值直接报错（不做回退）
    #[test]
    fn approval_policy_and_sandbox_defaults_and_validation() {
        let config = extract_codex_app_server_config(&[], &[], None).unwrap();
        assert_eq!(config.approval_policy.as_deref(), Some("on-request"));
        assert_eq!(config.sandbox.as_deref(), Some("workspace-write"));
        assert!(config.approvals_reviewer.is_none());

        let config = extract_codex_app_server_config(
            &[model_config("codex_approval_policy", "never")],
            &[provider_config("codex_approval_policy", "untrusted"), provider_config("codex_sandbox", "read-only")],
            None,
        )
        .unwrap();
        assert_eq!(config.approval_policy.as_deref(), Some("never"));
        assert_eq!(config.sandbox.as_deref(), Some("read-only"));

        assert!(
            extract_codex_app_server_config(&[], &[provider_config("codex_approval_policy", "on-failure")], None).is_err()
        );
        assert!(
            extract_codex_app_server_config(&[model_config("codex_sandbox", "full-access")], &[], None).is_err()
        );
    }

    fn codex_test_config() -> CodexAppServerConfig {
        CodexAppServerConfig {
            instance_id: None,
            cli_command: "codex".to_string(),
            working_directory: PathBuf::from("/tmp/work"),
            env_vars: HashMap::new(),
            additional_args: Vec::new(),
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox: None,
            approvals_reviewer: None,
            collaboration_mode: None,
            developer_instructions: None,
            selected_mcp_tools_payload: String::new(),
            session_signature: String::new(),
        }
    }

    /// MCP 工具快照纳入签名：绑定变化会触发会话重建
    #[test]
    fn signature_changes_with_selected_mcp_payload() {
        let mut config = codex_test_config();
        config.session_signature = codex_config_signature(&config);
        let without_mcp = config.session_signature.clone();
        config.selected_mcp_tools_payload = "[{\"server_id\":1}]".to_string();
        refresh_codex_session_signature(&mut config);
        assert_ne!(without_mcp, config.session_signature);
    }

    /// 构造的 config 覆盖包含 aipp server 的命令、桥接参数与全部环境变量
    #[test]
    fn build_codex_mcp_config_overrides_contains_bridge_env() {
        let proxy = AcpMcpProxyConfig {
            addr: "127.0.0.1:1234".to_string(),
            token: "tok".to_string(),
        };
        let overrides = build_codex_mcp_config_overrides(
            std::path::Path::new("aipp.exe"),
            std::path::Path::new("mcp.db"),
            42,
            &proxy,
            "[{\"server_id\":1}]",
        );
        assert_eq!(
            overrides.get("mcp_servers.aipp.command").and_then(Value::as_str),
            Some("aipp.exe")
        );
        assert_eq!(
            overrides.get("mcp_servers.aipp.args"),
            Some(&json!([ACP_MCP_BRIDGE_ARG]))
        );
        assert_eq!(
            overrides.get("mcp_servers.aipp.startup_timeout_sec"),
            Some(&json!(20))
        );
        let env = overrides
            .get("mcp_servers.aipp.env")
            .and_then(Value::as_object)
            .unwrap();
        for key in [
            ACP_MCP_DB_PATH_ENV,
            ACP_MCP_CONVERSATION_ID_ENV,
            ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV,
            ACP_MCP_PROXY_ADDR_ENV,
            ACP_MCP_PROXY_TOKEN_ENV,
            ACP_MCP_SELECTED_TOOLS_ENV,
        ] {
            assert!(env.contains_key(key), "missing env {key}");
        }
        assert_eq!(
            env.get(ACP_MCP_CONVERSATION_ID_ENV).and_then(Value::as_str),
            Some("42")
        );
        assert_eq!(
            env.get(ACP_MCP_PROXY_ADDR_ENV).and_then(Value::as_str),
            Some("127.0.0.1:1234")
        );
        assert_eq!(
            env.get(ACP_MCP_SELECTED_TOOLS_ENV).and_then(Value::as_str),
            Some("[{\"server_id\":1}]")
        );
    }
}

#[cfg(test)]
#[path = "codex_app_server_lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
#[path = "codex_app_server_permission_tests.rs"]
mod permission_tests;

#[cfg(all(test, unix))]
#[path = "codex_app_server_protocol_tests.rs"]
mod protocol_tests;

#[cfg(test)]
#[path = "codex_app_server_activity_tests.rs"]
mod activity_tests;

#[cfg(test)]
#[path = "codex_app_server_context_tests.rs"]
mod context_tests;

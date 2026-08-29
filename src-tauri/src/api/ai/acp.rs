//! ACP (Agent Client Protocol) integration module
//! Handles communication with ACP-compatible agents via stdio

use crate::acp_mcp_bridge::{
    ensure_proxy_server, ACP_MCP_BRIDGE_ARG, ACP_MCP_CONVERSATION_ID_ENV, ACP_MCP_DB_PATH_ENV,
    ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV, ACP_MCP_PROXY_ADDR_ENV, ACP_MCP_PROXY_TOKEN_ENV,
    ACP_MCP_SELECTED_TOOLS_ENV,
};
use crate::api::ai::config::build_proxy_env_vars;
use crate::api::ai::agent_completion::{handle_agent_success, AgentKind};
use crate::api::ai::conversation::{
    extract_tool_result, infer_media_type_from_url, parse_data_url,
};
use crate::api::assistant_api::resolve_acp_provider_id;
use crate::api::ai::context_manager::token_estimator::estimate_by_content;
use crate::api::ai::events::{
    ConversationEvent, ConversationListActivityEvent, MCPToolCallUpdateEvent,
    MessageUpdateEvent, TITLE_CHANGE_EVENT,
};
use crate::api::operation_api::{
    emit_permission_request_event, emit_permission_resolved_event, PermissionResolvedEvent,
    ACP_PERMISSION_REQUEST_EVENT, ACP_PERMISSION_RESOLVED_EVENT,
};
use crate::db::assistant_db::{AssistantDatabase, AssistantModelConfig};
use crate::db::conversation_db::{
    AttachmentType, ConversationDatabase, MessageAttachment, Repository,
};
use crate::db::llm_db::{LLMDatabase, LLMProviderConfig};
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::errors::AppError;
use crate::mcp::builtin_mcp::operation::{
    bash_ops::BashOperations,
    file_ops::FileOperations,
    permission::PermissionManager,
    state::OperationState,
    types::{
        BashProcessStatus, GetBashOutputRequest, ReadFileRequest, WriteFileRequest,
    },
};
use crate::plugin::hook_bus::PluginHookBus;
use crate::state::activity_state::ConversationActivityManager;
use crate::utils::window_utils::{
    emit_conversation_list_activity, send_conversation_event_to_chat_windows,
};
use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::schema::ProtocolVersion;
use base64::Engine;
use regex::Regex;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

/// ACP configuration extracted from assistant_model_config
#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub additional_args: Vec<String>,
    pub selected_mcp_tools_payload: String,
    pub session_signature: String,
}

#[derive(Debug, Clone)]
pub struct AcpLaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub extra_env: HashMap<String, String>,
    pub proxy_strategy: String,
}

async fn build_acp_manual_mcp_servers(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    selected_mcp_tools_payload: &str,
) -> Result<Vec<acp::McpServer>, String> {
    if selected_mcp_tools_payload.trim().is_empty() || selected_mcp_tools_payload.trim() == "[]" {
        return Ok(Vec::new());
    }

    let bridge_command = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve AIPP executable for ACP MCP bridge: {error}"))?;
    let mcp_db_path = MCPDatabase::db_path(app_handle)?;
    let proxy_config = ensure_proxy_server(app_handle.clone()).await?;
    Ok(build_acp_manual_mcp_servers_from_parts(
        bridge_command,
        mcp_db_path,
        conversation_id,
        proxy_config.addr,
        proxy_config.token,
        selected_mcp_tools_payload.to_string(),
    ))
}

fn build_acp_manual_mcp_servers_from_parts(
    bridge_command: PathBuf,
    mcp_db_path: PathBuf,
    conversation_id: i64,
    proxy_addr: String,
    proxy_token: String,
    selected_mcp_tools_payload: String,
) -> Vec<acp::McpServer> {
    vec![acp::McpServer::Stdio(
        acp::McpServerStdio::new("AIPP MCP Tools", bridge_command)
            .args(vec![ACP_MCP_BRIDGE_ARG.to_string()])
            .env(vec![
                acp::EnvVariable::new(
                    ACP_MCP_DB_PATH_ENV,
                    mcp_db_path.display().to_string(),
                ),
                acp::EnvVariable::new(
                    ACP_MCP_CONVERSATION_ID_ENV,
                    conversation_id.to_string(),
                ),
                acp::EnvVariable::new(ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV, "1"),
                acp::EnvVariable::new(ACP_MCP_PROXY_ADDR_ENV, proxy_addr),
                acp::EnvVariable::new(ACP_MCP_PROXY_TOKEN_ENV, proxy_token),
                acp::EnvVariable::new(ACP_MCP_SELECTED_TOOLS_ENV, selected_mcp_tools_payload),
            ]),
    )]
}

/// 构建助手当前选中的 MCP server/tool 快照（JSON 字符串，无选中时为 "[]"）。
/// ACP 与 Codex app-server 通道共用：快照通过桥接进程暴露给外部 agent。
pub fn build_selected_mcp_tools_payload(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> Result<String, String> {
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|error| error.to_string())?;
    let servers = assistant_db
        .get_assistant_mcp_servers_with_tools(assistant_id)
        .map_err(|error| error.to_string())?;

    let payload = servers
        .into_iter()
        .filter(|(_, server_name, server_command, server_enabled, _)| {
            *server_enabled
                && server_name != "MCP 动态加载工具"
                && server_command.as_deref() != Some("aipp:dynamic_mcp")
        })
        .filter_map(|(server_id, server_name, _, _, tools)| {
            let enabled_tools = tools
                .into_iter()
                .filter(|(_, _, _, tool_enabled, _, _)| *tool_enabled)
                .map(|(tool_id, tool_name, tool_description, _, _, parameters)| {
                    serde_json::json!({
                        "tool_id": tool_id,
                        "server_id": server_id,
                        "server_name": server_name.clone(),
                        "tool_name": tool_name,
                        "tool_description": tool_description,
                        "parameters": parameters,
                        "is_enabled": true,
                    })
                })
                .collect::<Vec<_>>();
            if enabled_tools.is_empty() {
                None
            } else {
                Some(serde_json::json!({
                    "server_id": server_id,
                    "server_name": server_name,
                    "is_enabled": true,
                    "tools": enabled_tools,
                }))
            }
        })
        .collect::<Vec<_>>();

    serde_json::to_string(&payload).map_err(|error| error.to_string())
}

pub fn refresh_acp_selected_mcp_tools_payload(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
    config: &mut AcpConfig,
) -> Result<(), String> {
    config.selected_mcp_tools_payload = build_selected_mcp_tools_payload(app_handle, assistant_id)?;
    Ok(())
}

pub(crate) fn merge_acp_env_blob(env_vars: &mut HashMap<String, String>, raw: &str) {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                env_vars.insert(key.to_string(), value.trim().to_string());
            }
        }
    }
}

const CLAUDE_SETTINGS_AUTH_MODE: &str = "claude_settings";
const CLAUDE_ENV_AUTH_MODE: &str = "env_vars";
const CLAUDE_AUTH_ENV_KEYS: [&str; 3] =
    ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"];

fn env_blob_contains_any_key(raw: &str, keys: &[&str]) -> bool {
    raw.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }

        trimmed.split_once('=').is_some_and(|(key, _)| {
            keys.iter().any(|candidate| key.trim().eq_ignore_ascii_case(candidate))
        })
    })
}

fn has_explicit_claude_auth_override(
    provider_configs: &[LLMProviderConfig],
    model_configs: &[AssistantModelConfig],
) -> bool {
    let has_provider_override = provider_configs.iter().any(|config| {
        if config.name == "acp_claude_env_vars" {
            return !config.value.trim().is_empty();
        }

        if config.name == "acp_env_vars" {
            return env_blob_contains_any_key(&config.value, &CLAUDE_AUTH_ENV_KEYS);
        }

        config.name.strip_prefix("acp_env_").is_some_and(|key| {
            CLAUDE_AUTH_ENV_KEYS.iter().any(|candidate| key.eq_ignore_ascii_case(candidate))
        })
    });

    let has_model_override = model_configs.iter().any(|config| {
        if config.name == "acp_claude_env_vars" {
            return config.value.as_deref().is_some_and(|value| !value.trim().is_empty());
        }

        if config.name == "acp_env_vars" {
            return config
                .value
                .as_deref()
                .is_some_and(|value| env_blob_contains_any_key(value, &CLAUDE_AUTH_ENV_KEYS));
        }

        config.name.strip_prefix("acp_env_").is_some_and(|key| {
            CLAUDE_AUTH_ENV_KEYS.iter().any(|candidate| key.eq_ignore_ascii_case(candidate))
        }) && config.value.as_deref().is_some_and(|value| !value.trim().is_empty())
    });

    has_provider_override || has_model_override
}

fn resolve_claude_auth_mode(
    cli_command: &str,
    explicit_mode: Option<String>,
    provider_configs: &[LLMProviderConfig],
    model_configs: &[AssistantModelConfig],
) -> Option<String> {
    if cli_command != "claude-code-acp" {
        return None;
    }

    if let Some(mode) = explicit_mode.filter(|mode| !mode.trim().is_empty()) {
        return Some(mode);
    }

    if has_explicit_claude_auth_override(provider_configs, model_configs) {
        Some(CLAUDE_ENV_AUTH_MODE.to_string())
    } else {
        Some(CLAUDE_SETTINGS_AUTH_MODE.to_string())
    }
}

fn load_claude_settings_env_vars_from_path(
    path: &Path,
) -> Result<HashMap<String, String>, AppError> {
    let raw = fs::read_to_string(path).map_err(|e| {
        AppError::UnknownError(format!(
            "读取 Claude settings.json 失败 ({}): {}",
            path.display(),
            e
        ))
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        AppError::UnknownError(format!(
            "解析 Claude settings.json 失败 ({}): {}",
            path.display(),
            e
        ))
    })?;
    let env_object = parsed.get("env").and_then(|value| value.as_object()).ok_or_else(|| {
        AppError::UnknownError(format!("Claude settings.json 中缺少 env 对象 ({})", path.display()))
    })?;

    let mut env_vars = HashMap::new();
    for (key, value) in env_object {
        if let Some(string_value) = value.as_str() {
            env_vars.insert(key.to_string(), string_value.to_string());
        }
    }

    if env_vars.is_empty() {
        return Err(AppError::UnknownError(format!(
            "Claude settings.json 的 env 对象为空 ({})",
            path.display()
        )));
    }

    Ok(env_vars)
}

fn load_claude_settings_env_vars() -> Result<HashMap<String, String>, AppError> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| AppError::UnknownError("无法确定用户 home 目录".to_string()))?;
    load_claude_settings_env_vars_from_path(&home_dir.join(".claude").join("settings.json"))
}

fn get_claude_model_override(
    cli_command: &str,
    env_vars: &HashMap<String, String>,
) -> Option<String> {
    if cli_command != "claude-code-acp" {
        return None;
    }

    env_vars
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("ANTHROPIC_MODEL"))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("default"))
}

/// 为 ACP agent client 注入标准代理环境变量，但不覆盖显式配置的代理变量。
pub fn apply_network_proxy_to_env_vars(
    env_vars: &mut HashMap<String, String>,
    proxy_url: &str,
) -> usize {
    let proxy_env_groups =
        [["HTTP_PROXY", "http_proxy"], ["HTTPS_PROXY", "https_proxy"], ["ALL_PROXY", "all_proxy"]];
    let proxy_envs = build_proxy_env_vars(proxy_url);
    let mut injected = 0;

    for env_group in proxy_env_groups {
        let has_existing = env_vars.keys().any(|existing| {
            env_group.iter().any(|candidate| existing.eq_ignore_ascii_case(candidate))
        });
        if has_existing {
            continue;
        }

        for env_name in env_group {
            if let Some(proxy_value) = proxy_envs.get(env_name) {
                env_vars.insert(env_name.to_string(), proxy_value.clone());
                injected += 1;
            }
        }
    }

    injected
}

#[derive(Debug, Clone)]
pub enum AcpPermissionDecision {
    Selected(String),
    Cancelled,
}

struct PendingAcpPermissionRequest {
    sender: oneshot::Sender<AcpPermissionDecision>,
    conversation_id: Option<i64>,
    event: AcpPermissionRequestEvent,
    review_code: String,
    feishu_message_id: Option<String>,
    allowed_open_id: Option<String>,
    allowed_chat_id: Option<String>,
}

pub struct AcpPermissionResolution {
    pub conversation_id: Option<i64>,
    pub delivered: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AcpPermissionRequestSnapshot {
    pub conversation_id: Option<i64>,
    pub event: AcpPermissionRequestEvent,
    pub review_code: String,
    pub feishu_message_id: Option<String>,
    pub allowed_open_id: Option<String>,
    pub allowed_chat_id: Option<String>,
}

#[derive(Default)]
pub struct AcpPermissionState {
    pending_requests: TokioMutex<HashMap<String, PendingAcpPermissionRequest>>,
}

impl AcpPermissionState {
    pub fn new() -> Self {
        Self { pending_requests: TokioMutex::new(HashMap::new()) }
    }

    fn build_review_code(request_id: &str) -> String {
        let compact: String = request_id
            .chars()
            .filter(|value| value.is_ascii_alphanumeric())
            .take(6)
            .collect::<String>()
            .to_ascii_uppercase();
        if compact.is_empty() {
            "ACP-UNKNOWN".to_string()
        } else {
            format!("ACP-{compact}")
        }
    }

    pub async fn store_request(
        &self,
        event: AcpPermissionRequestEvent,
        sender: oneshot::Sender<AcpPermissionDecision>,
    ) {
        let request_id = event.request_id.clone();
        let conversation_id = event.conversation_id;
        let mut pending = self.pending_requests.lock().await;
        pending.insert(
            request_id.clone(),
            PendingAcpPermissionRequest {
                sender,
                conversation_id,
                event,
                review_code: Self::build_review_code(&request_id),
                feishu_message_id: None,
                allowed_open_id: None,
                allowed_chat_id: None,
            },
        );
    }

    pub async fn resolve_request(
        &self,
        request_id: &str,
        decision: AcpPermissionDecision,
    ) -> Option<AcpPermissionResolution> {
        let mut pending = self.pending_requests.lock().await;
        pending.remove(request_id).map(|request| AcpPermissionResolution {
            conversation_id: request.conversation_id,
            delivered: request.sender.send(decision).is_ok(),
        })
    }

    pub async fn remove_request(&self, request_id: &str) {
        let mut pending = self.pending_requests.lock().await;
        pending.remove(request_id);
    }

    pub async fn get_request(&self, request_id: &str) -> Option<AcpPermissionRequestSnapshot> {
        let pending = self.pending_requests.lock().await;
        pending.get(request_id).map(|request| AcpPermissionRequestSnapshot {
            conversation_id: request.conversation_id,
            event: request.event.clone(),
            review_code: request.review_code.clone(),
            feishu_message_id: request.feishu_message_id.clone(),
            allowed_open_id: request.allowed_open_id.clone(),
            allowed_chat_id: request.allowed_chat_id.clone(),
        })
    }

    pub async fn find_request_by_review_code(
        &self,
        review_code: &str,
    ) -> Option<AcpPermissionRequestSnapshot> {
        let normalized = review_code.trim().to_ascii_uppercase();
        let pending = self.pending_requests.lock().await;
        pending.values().find_map(|request| {
            if request.review_code == normalized {
                Some(AcpPermissionRequestSnapshot {
                    conversation_id: request.conversation_id,
                    event: request.event.clone(),
                    review_code: request.review_code.clone(),
                    feishu_message_id: request.feishu_message_id.clone(),
                    allowed_open_id: request.allowed_open_id.clone(),
                    allowed_chat_id: request.allowed_chat_id.clone(),
                })
            } else {
                None
            }
        })
    }

    pub async fn set_feishu_delivery(
        &self,
        request_id: &str,
        feishu_message_id: Option<String>,
        allowed_open_id: Option<String>,
        allowed_chat_id: Option<String>,
    ) {
        let mut pending = self.pending_requests.lock().await;
        if let Some(request) = pending.get_mut(request_id) {
            request.feishu_message_id = feishu_message_id;
            request.allowed_open_id = allowed_open_id;
            request.allowed_chat_id = allowed_chat_id;
        }
    }

    pub async fn list_requests_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Vec<AcpPermissionRequestSnapshot> {
        let pending = self.pending_requests.lock().await;
        pending
            .values()
            .filter(|request| request.conversation_id == Some(conversation_id))
            .map(|request| AcpPermissionRequestSnapshot {
                conversation_id: request.conversation_id,
                event: request.event.clone(),
                review_code: request.review_code.clone(),
                feishu_message_id: request.feishu_message_id.clone(),
                allowed_open_id: request.allowed_open_id.clone(),
                allowed_chat_id: request.allowed_chat_id.clone(),
            })
            .collect()
    }

    pub async fn has_pending_permission_for_conversation(&self, conversation_id: i64) -> bool {
        let pending = self.pending_requests.lock().await;
        pending.values().any(|request| request.conversation_id == Some(conversation_id))
    }

    pub async fn cancel_requests_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Vec<(String, AcpPermissionResolution)> {
        let request_ids = {
            let pending = self.pending_requests.lock().await;
            pending
                .iter()
                .filter_map(|(request_id, request)| {
                    (request.conversation_id == Some(conversation_id))
                        .then_some(request_id.clone())
                })
                .collect::<Vec<_>>()
        };

        let mut resolutions = Vec::new();
        for request_id in request_ids {
            if let Some(resolution) =
                self.resolve_request(&request_id, AcpPermissionDecision::Cancelled).await
            {
                resolutions.push((request_id, resolution));
            }
        }
        resolutions
    }
}

async fn notify_cancelled_acp_permission_requests(
    app_handle: &tauri::AppHandle,
    resolutions: Vec<(String, AcpPermissionResolution)>,
) {
    for (request_id, resolution) in resolutions {
        if let Err(error) = emit_permission_resolved_event(
            app_handle,
            ACP_PERMISSION_RESOLVED_EVENT,
            &PermissionResolvedEvent {
                request_id: request_id.clone(),
                conversation_id: resolution.conversation_id,
            },
        ) {
            warn!(
                request_id = %request_id,
                error = %error,
                "Failed to emit ACP permission cancellation resolution event"
            );
        }
        if let Some(conversation_id) = resolution.conversation_id {
            if let Err(error) =
                crate::api::butler_api::emit_butler_task_permission_state_changed(
                    app_handle,
                    conversation_id,
                    "acp",
                    false,
                )
                .await
            {
                warn!(
                    conversation_id,
                    error = %error,
                    "failed to refresh Butler ACP permission state after cancellation"
                );
            }
        }
    }
}

/// ACP elicitation（结构化提问）的用户决策。
#[derive(Debug)]
pub enum AcpElicitationDecision {
    Accepted(std::collections::BTreeMap<String, acp::ElicitationContentValue>),
    Declined,
    Cancelled,
}

struct PendingAcpElicitationRequest {
    sender: oneshot::Sender<AcpElicitationDecision>,
    conversation_id: Option<i64>,
}

/// ACP elicitation 挂起请求状态，生命周期与 AcpPermissionState 对齐。
#[derive(Default)]
pub struct AcpElicitationState {
    pending_requests: TokioMutex<HashMap<String, PendingAcpElicitationRequest>>,
}

impl AcpElicitationState {
    pub fn new() -> Self {
        Self { pending_requests: TokioMutex::new(HashMap::new()) }
    }

    pub async fn store_request(
        &self,
        request_id: String,
        conversation_id: Option<i64>,
        sender: oneshot::Sender<AcpElicitationDecision>,
    ) {
        let mut pending = self.pending_requests.lock().await;
        pending.insert(request_id, PendingAcpElicitationRequest { sender, conversation_id });
    }

    pub async fn resolve_request(
        &self,
        request_id: &str,
        decision: AcpElicitationDecision,
    ) -> Option<AcpPermissionResolution> {
        let mut pending = self.pending_requests.lock().await;
        pending.remove(request_id).map(|request| AcpPermissionResolution {
            conversation_id: request.conversation_id,
            delivered: request.sender.send(decision).is_ok(),
        })
    }

    pub async fn remove_request(&self, request_id: &str) {
        let mut pending = self.pending_requests.lock().await;
        pending.remove(request_id);
    }

    pub async fn cancel_requests_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Vec<(String, AcpPermissionResolution)> {
        let request_ids = {
            let pending = self.pending_requests.lock().await;
            pending
                .iter()
                .filter_map(|(request_id, request)| {
                    (request.conversation_id == Some(conversation_id))
                        .then_some(request_id.clone())
                })
                .collect::<Vec<_>>()
        };

        let mut resolutions = Vec::new();
        for request_id in request_ids {
            if let Some(resolution) =
                self.resolve_request(&request_id, AcpElicitationDecision::Cancelled).await
            {
                resolutions.push((request_id, resolution));
            }
        }
        resolutions
    }
}

/// 发送给前端的 elicitation 请求事件；`schema` 为 ACP `ElicitationSchema` 的 JSON 序列化。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpElicitationRequestEvent {
    pub(crate) request_id: String,
    pub(crate) conversation_id: Option<i64>,
    pub(crate) agent_kind: String,
    pub(crate) message: String,
    pub(crate) schema: serde_json::Value,
}

async fn notify_cancelled_acp_elicitation_requests(
    app_handle: &tauri::AppHandle,
    resolutions: Vec<(String, AcpPermissionResolution)>,
) {
    for (request_id, resolution) in resolutions {
        if let Err(error) = emit_permission_resolved_event(
            app_handle,
            crate::api::operation_api::ACP_ELICITATION_RESOLVED_EVENT,
            &PermissionResolvedEvent {
                request_id: request_id.clone(),
                conversation_id: resolution.conversation_id,
            },
        ) {
            warn!(
                request_id = %request_id,
                error = %error,
                "Failed to emit ACP elicitation cancellation resolution event"
            );
        }
    }
}

/// 前端 confirm_acp_elicitation 命令的 JSON 值到 ACP elicitation 内容值的转换。
pub(crate) fn json_to_acp_elicitation_value(
    value: serde_json::Value,
) -> Result<acp::ElicitationContentValue, String> {
    match value {
        serde_json::Value::String(text) => Ok(acp::ElicitationContentValue::String(text)),
        serde_json::Value::Bool(flag) => Ok(acp::ElicitationContentValue::Boolean(flag)),
        serde_json::Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(acp::ElicitationContentValue::Integer(int))
            } else if let Some(float) = number.as_f64() {
                Ok(acp::ElicitationContentValue::Number(float))
            } else {
                Err(format!("unsupported elicitation number value: {number}"))
            }
        }
        serde_json::Value::Array(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    serde_json::Value::String(text) => values.push(text),
                    other => {
                        return Err(format!(
                            "elicitation array items must be strings, got: {other}"
                        ))
                    }
                }
            }
            Ok(acp::ElicitationContentValue::StringArray(values))
        }
        other => Err(format!("unsupported elicitation value type: {other}")),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpPermissionOptionPayload {
    pub(crate) option_id: String,
    pub(crate) name: String,
    pub(crate) kind: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct AcpPermissionRequestEvent {
    pub(crate) request_id: String,
    pub(crate) conversation_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent_kind: Option<String>,
    pub(crate) tool_call_id: String,
    pub(crate) title: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) parameters: Option<String>,
    pub(crate) options: Vec<AcpPermissionOptionPayload>,
}

const ACP_SESSION_STATE_SNAPSHOT_EVENT_TYPE: &str = "acp_session_state_snapshot";

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpSessionModePayload {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpSessionConfigChoicePayload {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
    pub group_name: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpSessionConfigOptionPayload {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub current_value: String,
    pub options: Vec<AcpSessionConfigChoicePayload>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpPromptCapabilitiesPayload {
    pub image: bool,
    pub audio: bool,
    pub embedded_context: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpPlanEntryPayload {
    pub content: String,
    pub priority: String,
    pub status: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpAvailableCommandPayload {
    pub name: String,
    pub description: String,
    pub input_hint: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct AcpPromptUsageSummary {
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    thought_tokens: Option<i64>,
    cached_read_tokens: Option<i64>,
    cached_write_tokens: Option<i64>,
    usage_source: &'static str,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AcpConversationSessionState {
    pub conversation_id: i64,
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub load_session_supported: bool,
    pub session_resume_supported: bool,
    pub restored_session_method: Option<String>,
    pub prompt_capabilities: AcpPromptCapabilitiesPayload,
    pub current_mode_id: Option<String>,
    pub modes: Vec<AcpSessionModePayload>,
    pub config_options: Vec<AcpSessionConfigOptionPayload>,
    pub plan: Vec<AcpPlanEntryPayload>,
    pub available_commands: Vec<AcpAvailableCommandPayload>,
    pub has_active_prompt: bool,
    pub context_tokens_used: Option<u64>,
    pub context_window_size: Option<u64>,
    pub session_cost_amount: Option<f64>,
    pub session_cost_currency: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AcpSessionStateSnapshotEvent {
    state: Option<AcpConversationSessionState>,
}

enum AcpSessionCommand {
    Start {
        window: tauri::Window,
        response: oneshot::Sender<Result<(), String>>,
    },
    Prompt {
        message_id: i64,
        prompt: String,
        attachments: Vec<MessageAttachment>,
        window: tauri::Window,
    },
    CancelCurrentPrompt {
        response: oneshot::Sender<Result<(), String>>,
    },
    SetConfigOption {
        config_id: String,
        value: String,
        response: oneshot::Sender<Result<(), String>>,
    },
}

#[derive(Clone)]
pub struct AcpSessionHandle {
    sender: mpsc::UnboundedSender<AcpSessionCommand>,
    run_id: String,
}

impl AcpSessionHandle {
    pub async fn start(&self, window: tauri::Window) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(AcpSessionCommand::Start { window, response: tx })
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))?;
        rx.await
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))?
            .map_err(AppError::UnknownError)
    }

    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        attachments: Vec<MessageAttachment>,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        self.sender
            .send(AcpSessionCommand::Prompt { message_id, prompt, attachments, window })
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))
    }

    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(AcpSessionCommand::CancelCurrentPrompt { response: tx })
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))?;
        rx.await
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))?
            .map_err(AppError::UnknownError)
    }

    pub async fn set_config_option(
        &self,
        config_id: String,
        value: String,
    ) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(AcpSessionCommand::SetConfigOption {
                config_id,
                value,
                response: tx,
            })
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))?;
        rx.await
            .map_err(|_| AppError::UnknownError("ACP session closed".to_string()))?
            .map_err(AppError::UnknownError)
    }
}

#[derive(Clone)]
pub struct AcpSessionEntry {
    pub handle: AcpSessionHandle,
    pub snapshot: AcpConversationSessionState,
    pub last_activity: Instant,
    pub config_signature: String,
    pub run_id: String,
}

impl AcpSessionEntry {
    pub fn new(handle: AcpSessionHandle, conversation_id: i64, config_signature: impl Into<String>) -> Self {
        let run_id = handle.run_id.clone();
        Self {
            handle,
            snapshot: AcpConversationSessionState { conversation_id, ..Default::default() },
            last_activity: Instant::now(),
            config_signature: config_signature.into(),
            run_id,
        }
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

fn flatten_session_config_choices(
    options: &acp::SessionConfigSelectOptions,
) -> Vec<AcpSessionConfigChoicePayload> {
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(items) => items
            .iter()
            .map(|option| AcpSessionConfigChoicePayload {
                value: option.value.to_string(),
                name: option.name.clone(),
                description: option.description.clone(),
                group_name: None,
            })
            .collect(),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(|option| AcpSessionConfigChoicePayload {
                    value: option.value.to_string(),
                    name: option.name.clone(),
                    description: option.description.clone(),
                    group_name: Some(group.name.clone()),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn session_config_category_to_string(
    category: &acp::SessionConfigOptionCategory,
) -> String {
    match category {
        acp::SessionConfigOptionCategory::Mode => "mode".to_string(),
        acp::SessionConfigOptionCategory::Model => "model".to_string(),
        acp::SessionConfigOptionCategory::ThoughtLevel => "thought_level".to_string(),
        acp::SessionConfigOptionCategory::Other(value) => value.clone(),
        _ => "other".to_string(),
    }
}

fn session_config_option_payload(
    config_option: &acp::SessionConfigOption,
) -> Option<AcpSessionConfigOptionPayload> {
    match &config_option.kind {
        acp::SessionConfigKind::Select(select) => Some(AcpSessionConfigOptionPayload {
            id: config_option.id.to_string(),
            name: config_option.name.clone(),
            description: config_option.description.clone(),
            category: config_option.category.as_ref().map(session_config_category_to_string),
            current_value: select.current_value.to_string(),
            options: flatten_session_config_choices(&select.options),
        }),
        _ => None,
    }
}

fn acp_config_signature(config: &AcpConfig) -> String {
    let mut env_entries = config
        .env_vars
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    env_entries.sort();

    format!(
        "cli={}\ncwd={}\nselected_mcp={}\nargs={}\nenv={}",
        config.cli_command,
        config.working_directory.display(),
        config.selected_mcp_tools_payload,
        config.additional_args.join("\u{1f}"),
        env_entries.join("\u{1f}")
    )
}

pub fn refresh_acp_config_signature(config: &mut AcpConfig) {
    config.session_signature = acp_config_signature(config);
}

fn apply_config_options_to_snapshot(
    snapshot: &mut AcpConversationSessionState,
    config_options: Option<&[acp::SessionConfigOption]>,
) {
    snapshot.config_options = config_options
        .unwrap_or(&[])
        .iter()
        .filter_map(session_config_option_payload)
        .collect();
}

fn prompt_capabilities_payload(
    capabilities: &acp::PromptCapabilities,
) -> AcpPromptCapabilitiesPayload {
    AcpPromptCapabilitiesPayload {
        image: capabilities.image,
        audio: capabilities.audio,
        embedded_context: capabilities.embedded_context,
    }
}

fn plan_entry_priority_to_string(priority: &acp::PlanEntryPriority) -> String {
    match priority {
        acp::PlanEntryPriority::High => "high".to_string(),
        acp::PlanEntryPriority::Medium => "medium".to_string(),
        acp::PlanEntryPriority::Low => "low".to_string(),
        _ => "medium".to_string(),
    }
}

fn plan_entry_status_to_string(status: &acp::PlanEntryStatus) -> String {
    match status {
        acp::PlanEntryStatus::Pending => "pending".to_string(),
        acp::PlanEntryStatus::InProgress => "in_progress".to_string(),
        acp::PlanEntryStatus::Completed => "completed".to_string(),
        _ => "pending".to_string(),
    }
}

fn plan_payload(plan: &acp::Plan) -> Vec<AcpPlanEntryPayload> {
    plan.entries
        .iter()
        .map(|entry| AcpPlanEntryPayload {
            content: entry.content.clone(),
            priority: plan_entry_priority_to_string(&entry.priority),
            status: plan_entry_status_to_string(&entry.status),
        })
        .collect()
}

fn available_command_input_hint(input: Option<&acp::AvailableCommandInput>) -> Option<String> {
    match input {
        Some(acp::AvailableCommandInput::Unstructured(unstructured)) => {
            Some(unstructured.hint.clone())
        }
        _ => None,
    }
}

fn available_commands_payload(
    commands: &[acp::AvailableCommand],
) -> Vec<AcpAvailableCommandPayload> {
    commands
        .iter()
        .map(|command| AcpAvailableCommandPayload {
            name: command.name.clone(),
            description: command.description.clone(),
            input_hint: available_command_input_hint(command.input.as_ref()),
        })
        .collect()
}

fn emit_acp_session_state_snapshot(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    state: Option<AcpConversationSessionState>,
) {
    send_conversation_event_to_chat_windows(
        app_handle,
        conversation_id,
        ConversationEvent {
            r#type: ACP_SESSION_STATE_SNAPSHOT_EVENT_TYPE.to_string(),
            data: serde_json::to_value(AcpSessionStateSnapshotEvent { state }).unwrap(),
        },
    );
}

static AGENT_IDLE_REAPER_STARTED: OnceLock<()> = OnceLock::new();
const AGENT_IDLE_SESSION_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const AGENT_IDLE_REAPER_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn_agent_idle_reaper_once(app_handle: tauri::AppHandle) {
    if AGENT_IDLE_REAPER_STARTED.set(()).is_err() {
        return;
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(AGENT_IDLE_REAPER_INTERVAL);
        loop {
            interval.tick().await;

            let expired_acp_conversation_ids = {
                let session_state = app_handle.state::<crate::AcpSessionState>();
                let mut sessions = session_state.sessions.lock().await;
                let expired = sessions
                    .iter()
                    .filter_map(|(conversation_id, entry)| {
                        entry.is_idle_for(AGENT_IDLE_SESSION_TIMEOUT).then_some(*conversation_id)
                    })
                    .collect::<Vec<_>>();

                for conversation_id in &expired {
                    sessions.remove(conversation_id);
                }

                expired
            };

            for conversation_id in expired_acp_conversation_ids {
                info!(
                    conversation_id,
                    idle_minutes = 15,
                    "ACP session idle timeout reached; releasing session"
                );
                emit_acp_session_state_snapshot(&app_handle, conversation_id, None);
            }

            let expired_codex_sessions = {
                let session_state = app_handle.state::<crate::CodexSessionState>();
                let mut sessions = session_state.sessions.lock().await;
                let expired = sessions
                    .iter()
                    .filter_map(|(conversation_id, entry)| {
                        entry
                            .is_idle_for(AGENT_IDLE_SESSION_TIMEOUT)
                            .then_some((*conversation_id, entry.handle.clone()))
                    })
                    .collect::<Vec<_>>();
                for (conversation_id, handle) in &expired {
                    handle.shutdown("15 分钟无活跃请求，释放本地 Codex app-server 进程".to_string());
                    sessions.remove(conversation_id);
                }
                expired
            };
            for (conversation_id, _) in expired_codex_sessions {
                info!(conversation_id, idle_minutes = 15, "Codex session idle timeout reached; releasing local process");
                emit_acp_session_state_snapshot(&app_handle, conversation_id, None);
            }

            let expired_claude_conversation_ids = {
                let session_state = app_handle.state::<crate::ClaudeSessionState>();
                let mut sessions = session_state.sessions.lock().await;
                let expired = sessions
                    .iter()
                    .filter_map(|(conversation_id, entry)| {
                        entry
                            .is_idle_for(AGENT_IDLE_SESSION_TIMEOUT)
                            .then_some(*conversation_id)
                    })
                    .collect::<Vec<_>>();
                for conversation_id in &expired {
                    sessions.remove(conversation_id);
                }
                expired
            };
            for conversation_id in expired_claude_conversation_ids {
                info!(conversation_id, idle_minutes = 15, "Claude Code session idle timeout reached; releasing local process");
                emit_acp_session_state_snapshot(&app_handle, conversation_id, None);
            }
        }
    });
}

/// Resolve ACP CLI command to its full path
///
/// This function tries to find the CLI executable in the following order:
/// 1. If the command is already an absolute path, use it directly
/// 2. Check ~/.bun/bin/ for bun-installed global packages
/// 3. Check system PATH
/// 4. Fall back to the original command (let the system handle it)
pub fn resolve_acp_cli_path(cli_command: &str) -> PathBuf {
    let cli_path = PathBuf::from(cli_command);

    // If it's already an absolute path, use it directly
    if cli_path.is_absolute() {
        info!("ACP: CLI command is already an absolute path");
        return cli_path;
    }

    // Check ~/.bun/bin/ first (bun-installed global packages)
    if let Some(home) = dirs::home_dir() {
        // On Windows, bun creates .exe files for global packages
        #[cfg(target_os = "windows")]
        let exe_name = format!("{}.exe", cli_command);
        #[cfg(not(target_os = "windows"))]
        let exe_name = cli_command.to_string();

        let bun_bin_path = home.join(".bun").join("bin").join(&exe_name);
        info!("ACP: Checking bun bin path: {}", bun_bin_path.display());
        if bun_bin_path.exists() {
            info!("ACP: Found CLI in bun bin: {}", bun_bin_path.display());
            return bun_bin_path;
        }

        // Also check without .exe on Windows (in case user provides full name)
        #[cfg(target_os = "windows")]
        {
            let bun_bin_path_no_ext = home.join(".bun").join("bin").join(cli_command);
            if bun_bin_path_no_ext.exists() {
                info!("ACP: Found CLI in bun bin (no ext): {}", bun_bin_path_no_ext.display());
                return bun_bin_path_no_ext;
            }
        }
    }

    // Check system PATH using platform-specific command
    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    if let Ok(output) = std::process::Command::new(which_cmd).arg(cli_command).output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // On Windows, `where` may return multiple lines, take the first one
            let first_path = path_str.lines().next().unwrap_or("").trim();
            if !first_path.is_empty() {
                info!("ACP: Found CLI in system PATH: {}", first_path);
                return PathBuf::from(first_path);
            }
        }
    }

    info!("ACP: CLI not found in known paths, using original command: {}", cli_command);
    cli_path
}

fn has_proxy_env_vars(env_vars: &HashMap<String, String>) -> bool {
    env_vars.keys().any(|key| {
        matches!(key.to_ascii_lowercase().as_str(), "http_proxy" | "https_proxy" | "all_proxy")
    })
}

fn is_node_shebang_script(path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let Some(first_line) = content.lines().next() else {
        return false;
    };
    first_line.contains("env node") || first_line.contains("/node")
}

fn test_node_executable(path: &Path) -> bool {
    path.exists()
        && std::process::Command::new(path)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
}

fn find_latest_node_in_dir(base_dir: &Path, nested_suffix: &Path) -> Option<PathBuf> {
    let Ok(entries) = fs::read_dir(base_dir) else {
        return None;
    };

    let mut version_dirs = entries
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    version_dirs.sort_by(|left, right| right.file_name().cmp(&left.file_name()));

    version_dirs.into_iter().find_map(|path| {
        let node_path = path.join(nested_suffix);
        test_node_executable(&node_path).then_some(node_path)
    })
}

fn resolve_node_runtime_path() -> Option<PathBuf> {
    if let Ok(node_path) = which::which("node") {
        return Some(node_path);
    }

    let mut shell_candidates = Vec::new();
    if let Ok(shell) = std::env::var("SHELL") {
        shell_candidates.push(shell);
    }
    shell_candidates.push("/bin/zsh".to_string());
    shell_candidates.push("/bin/bash".to_string());

    for shell in shell_candidates {
        let shell_path = PathBuf::from(&shell);
        if !shell_path.exists() {
            continue;
        }

        let Ok(output) = std::process::Command::new(&shell_path)
            .args(["-lc", "command -v node"])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }

        let resolved = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if resolved.is_empty() {
            continue;
        }

        let node_path = PathBuf::from(resolved);
        if test_node_executable(&node_path) {
            return Some(node_path);
        }
    }

    if let Some(home_dir) = dirs::home_dir() {
        if let Some(node_path) = find_latest_node_in_dir(
            &home_dir.join(".nvm").join("versions").join("node"),
            Path::new("bin").join("node").as_path(),
        ) {
            return Some(node_path);
        }

        if let Some(node_path) = find_latest_node_in_dir(
            &home_dir.join(".fnm").join("node-versions"),
            Path::new("installation").join("bin").join("node").as_path(),
        ) {
            return Some(node_path);
        }

        let volta_node = home_dir.join(".volta").join("bin").join("node");
        if test_node_executable(&volta_node) {
            return Some(volta_node);
        }
    }

    [
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/usr/bin/node"),
    ]
    .into_iter()
    .find(|path| test_node_executable(path))
}

fn node_supports_use_env_proxy(node_path: &Path) -> bool {
    let Ok(output) = std::process::Command::new(node_path).arg("--help").output() else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).contains("--use-env-proxy")
}

/// 各 ACP CLI 预设的默认启动参数，会排在用户 additional_args 之前
fn acp_cli_preset_default_args(cli_command: &str) -> Vec<String> {
    match cli_command {
        // Kimi Code CLI 通过 `kimi acp` 子命令提供 ACP 服务
        "kimi" => vec!["acp".to_string()],
        _ => Vec::new(),
    }
}

pub fn build_acp_launch_plan(
    cli_command: &str,
    resolved_cli_command: &Path,
    additional_args: &[String],
    env_vars: &HashMap<String, String>,
) -> AcpLaunchPlan {
    let mut effective_args = acp_cli_preset_default_args(cli_command);
    effective_args.extend(additional_args.iter().cloned());

    let mut plan = AcpLaunchPlan {
        program: resolved_cli_command.to_path_buf(),
        args: effective_args.clone(),
        extra_env: HashMap::new(),
        proxy_strategy: "standard-env".to_string(),
    };

    // node-shebang 脚本（npm/bun 全局 bin，如 claude-code-acp、dsh-acp-server）
    // 在 Windows 上无法直接 spawn，统一改为显式 node 运行时启动
    if !is_node_shebang_script(resolved_cli_command) {
        if has_proxy_env_vars(env_vars) {
            plan.proxy_strategy = "standard-env-non-node-script".to_string();
        }
        return plan;
    }

    let Some(node_path) = resolve_node_runtime_path() else {
        plan.proxy_strategy = "standard-env-node-not-found".to_string();
        return plan;
    };

    plan.program = node_path.clone();
    plan.args = vec![resolved_cli_command.display().to_string()];
    plan.args.extend(effective_args.iter().cloned());

    if has_proxy_env_vars(env_vars) {
        plan.extra_env.insert("NODE_USE_ENV_PROXY".to_string(), "1".to_string());
        if node_supports_use_env_proxy(&node_path) {
            plan.args.insert(0, "--use-env-proxy".to_string());
            plan.proxy_strategy = "node-use-env-proxy-flag".to_string();
        } else {
            plan.proxy_strategy = "node-use-env-proxy-env".to_string();
        }
    } else {
        plan.proxy_strategy = "node-explicit-runtime".to_string();
    }

    plan
}

fn expand_home_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        dirs::home_dir()
            .map(|home| home.join(&path[2..]))
            .unwrap_or_else(|| PathBuf::from(path))
    } else if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(path))
    } else if path.starts_with('~') {
        dirs::home_dir()
            .map(|home| home.join(&path[1..]))
            .unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    }
}

/// Extract text content from a ContentBlock
fn extract_content_text(content: &acp::ContentBlock) -> String {
    match content {
        acp::ContentBlock::Text(text_content) => text_content.text.clone(),
        acp::ContentBlock::Image(_) => "[Image]".to_string(),
        acp::ContentBlock::Audio(_) => "[Audio]".to_string(),
        acp::ContentBlock::ResourceLink(resource_link) => resource_link.uri.clone(),
        acp::ContentBlock::Resource(resource) => {
            // Extract URI from the nested resource enum
            match &resource.resource {
                acp::EmbeddedResourceResource::TextResourceContents(text) => text.uri.clone(),
                acp::EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.clone(),
                _ => "[Resource]".to_string(),
            }
        }
        _ => "[Unknown content]".to_string(),
    }
}

fn append_buffered_content(buffer: &mut String, content: &acp::ContentBlock) -> String {
    buffer.push_str(&extract_content_text(content));
    buffer.clone()
}

fn build_prompt_to_send(prompt: String, history_prefix: Option<String>) -> String {
    if let Some(prefix) = history_prefix.filter(|value| !value.trim().is_empty()) {
        format!("{}\n\n当前用户请求:\n{}", prefix, prompt)
    } else {
        prompt
    }
}

fn estimate_tokens_for_content_block(content: &acp::ContentBlock) -> usize {
    match content {
        acp::ContentBlock::Text(text_content) => estimate_by_content(&text_content.text),
        acp::ContentBlock::Resource(resource) => match &resource.resource {
            acp::EmbeddedResourceResource::TextResourceContents(text) => {
                estimate_by_content(&text.text)
            }
            acp::EmbeddedResourceResource::BlobResourceContents(_) => 0,
            _ => 0,
        },
        _ => 0,
    }
}

fn merge_acp_usage_metadata(
    existing_metadata_json: Option<&str>,
    usage_summary: &AcpPromptUsageSummary,
) -> Result<String, AppError> {
    let mut map = match existing_metadata_json {
        Some(raw) if !raw.trim().is_empty() => match serde_json::from_str::<JsonValue>(raw) {
            Ok(JsonValue::Object(map)) => map,
            Ok(_) => JsonMap::new(),
            Err(error) => {
                warn!("ACP: Failed to parse existing metadata_json, replacing it: {}", error);
                JsonMap::new()
            }
        },
        _ => JsonMap::new(),
    };

    map.insert(
        "usage_source".to_string(),
        JsonValue::String(usage_summary.usage_source.to_string()),
    );
    if let Some(thought_tokens) = usage_summary.thought_tokens {
        map.insert(
            "thought_tokens".to_string(),
            JsonValue::Number(thought_tokens.into()),
        );
    } else {
        map.remove("thought_tokens");
    }
    if let Some(cached_read_tokens) = usage_summary.cached_read_tokens {
        map.insert(
            "cached_input_tokens".to_string(),
            JsonValue::Number(cached_read_tokens.into()),
        );
        map.insert(
            "cached_read_tokens".to_string(),
            JsonValue::Number(cached_read_tokens.into()),
        );
    } else {
        map.remove("cached_input_tokens");
        map.remove("cached_read_tokens");
    }
    if let Some(cached_write_tokens) = usage_summary.cached_write_tokens {
        map.insert(
            "cached_write_tokens".to_string(),
            JsonValue::Number(cached_write_tokens.into()),
        );
    } else {
        map.remove("cached_write_tokens");
    }

    serde_json::to_string(&JsonValue::Object(map))
        .map_err(|error| AppError::UnknownError(format!("Failed to serialize ACP usage metadata: {error}")))
}

fn attachment_display_name(attachment: &MessageAttachment) -> String {
    attachment
        .attachment_url
        .as_deref()
        .map(|value| {
            Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(value)
                .to_string()
        })
        .unwrap_or_else(|| "attachment".to_string())
}

fn attachment_uri(attachment: &MessageAttachment) -> String {
    if let Some(url) = attachment.attachment_url.as_deref() {
        if url.starts_with("data:") {
            return format!("aipp://attachment/{}", attachment.id);
        }
        if url.starts_with("file://") || url.starts_with("http://") || url.starts_with("https://")
        {
            return url.to_string();
        }
        return format!("file://{}", url);
    }
    format!("aipp://attachment/{}", attachment.id)
}

fn acp_image_block_from_attachment(
    attachment: &MessageAttachment,
) -> Result<acp::ContentBlock, AppError> {
    if let Some(content) = attachment.attachment_content.as_deref() {
        if let Some((mime, b64)) = parse_data_url(content) {
            return Ok(acp::ContentBlock::Image(
                acp::ImageContent::new(b64, mime).uri(Some(attachment_uri(attachment))),
            ));
        }
    }

    let Some(url) = attachment.attachment_url.as_deref() else {
        return Err(AppError::UnknownError(format!(
            "图片附件 {} 缺少内容，无法发送给 ACP Agent",
            attachment_display_name(attachment)
        )));
    };

    if let Some((mime, b64)) = parse_data_url(url) {
        return Ok(acp::ContentBlock::Image(
            acp::ImageContent::new(b64, mime).uri(Some(attachment_uri(attachment))),
        ));
    }

    let path = url.strip_prefix("file://").unwrap_or(url);
    let bytes = fs::read(path).map_err(|error| {
        AppError::UnknownError(format!(
            "读取图片附件失败 ({}): {}",
            attachment_display_name(attachment),
            error
        ))
    })?;
    let mime = infer_media_type_from_url(url);
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(acp::ContentBlock::Image(
        acp::ImageContent::new(b64, mime).uri(Some(attachment_uri(attachment))),
    ))
}

fn text_resource_block_from_attachment(
    attachment: &MessageAttachment,
    content: &str,
) -> acp::ContentBlock {
    let mime = match attachment.attachment_type {
        AttachmentType::Text => "text/plain",
        AttachmentType::PDF => "application/pdf",
        AttachmentType::Word => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        AttachmentType::PowerPoint => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        AttachmentType::Excel => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        }
        _ => "text/plain",
    };

    acp::ContentBlock::Resource(acp::EmbeddedResource::new(
        acp::EmbeddedResourceResource::TextResourceContents(
            acp::TextResourceContents::new(content.to_string(), attachment_uri(attachment))
                .mime_type(Some(mime.to_string())),
        ),
    ))
}

fn text_attachment_fallback_block(
    attachment: &MessageAttachment,
    content: &str,
) -> acp::ContentBlock {
    acp::ContentBlock::Text(acp::TextContent::new(format!(
        "\n\n[{}: {}]\n{}",
        match attachment.attachment_type {
            AttachmentType::PDF => "PDF文档",
            AttachmentType::Word => "Word文档",
            AttachmentType::PowerPoint => "PowerPoint文档",
            AttachmentType::Excel => "Excel文档",
            _ => "文档",
        },
        attachment_display_name(attachment),
        content
    )))
}

fn build_acp_prompt_blocks(
    prompt: String,
    attachments: &[MessageAttachment],
    prompt_capabilities: &acp::PromptCapabilities,
    history_prefix: Option<String>,
) -> Result<Vec<acp::ContentBlock>, AppError> {
    let prompt_to_send = build_prompt_to_send(prompt, history_prefix);
    let mut blocks = Vec::new();
    if !prompt_to_send.trim().is_empty() {
        blocks.push(acp::ContentBlock::Text(acp::TextContent::new(prompt_to_send)));
    }

    for attachment in attachments {
        match attachment.attachment_type {
            AttachmentType::Image => {
                if !prompt_capabilities.image {
                    return Err(AppError::UnknownError(format!(
                        "该 ACP Agent 不支持图片输入，无法发送附件：{}",
                        attachment_display_name(attachment)
                    )));
                }
                blocks.push(acp_image_block_from_attachment(attachment)?);
            }
            AttachmentType::Text
            | AttachmentType::PDF
            | AttachmentType::Word
            | AttachmentType::PowerPoint
            | AttachmentType::Excel => {
                if let Some(content) = attachment.attachment_content.as_deref() {
                    if prompt_capabilities.embedded_context {
                        blocks.push(text_resource_block_from_attachment(attachment, content));
                    } else {
                        blocks.push(text_attachment_fallback_block(attachment, content));
                    }
                }
            }
            AttachmentType::Skill => {}
        }
    }

    Ok(blocks)
}

fn summarize_acp_prompt_usage(
    prompt_response: &acp::PromptResponse,
    prompt_blocks: &[acp::ContentBlock],
    final_content: &str,
    reasoning_content: &str,
) -> AcpPromptUsageSummary {
    if let Some(usage) = prompt_response.usage.as_ref() {
        return AcpPromptUsageSummary {
            total_tokens: usage.total_tokens as i64,
            input_tokens: usage.input_tokens as i64,
            output_tokens: usage.output_tokens as i64,
            thought_tokens: usage.thought_tokens.map(|value| value as i64),
            cached_read_tokens: usage.cached_read_tokens.map(|value| value as i64),
            cached_write_tokens: usage.cached_write_tokens.map(|value| value as i64),
            usage_source: "reported",
        };
    }

    let input_tokens = prompt_blocks
        .iter()
        .map(estimate_tokens_for_content_block)
        .sum::<usize>() as i64;
    let output_tokens = estimate_by_content(final_content) as i64;
    let thought_tokens = (!reasoning_content.trim().is_empty())
        .then(|| estimate_by_content(reasoning_content) as i64);
    let total_tokens = input_tokens + output_tokens + thought_tokens.unwrap_or(0);

    AcpPromptUsageSummary {
        total_tokens,
        input_tokens,
        output_tokens,
        thought_tokens,
        cached_read_tokens: None,
        cached_write_tokens: None,
        usage_source: "estimated",
    }
}

/// Convert ACP ToolCallStatus to string for frontend
fn tool_status_to_string(status: acp::ToolCallStatus) -> String {
    match status {
        acp::ToolCallStatus::Pending => "pending".to_string(),
        acp::ToolCallStatus::InProgress => "executing".to_string(),
        acp::ToolCallStatus::Completed => "success".to_string(),
        acp::ToolCallStatus::Failed => "failed".to_string(),
        _ => "unknown".to_string(),
    }
}

fn acp_tool_status_to_aipp_status(
    status: acp::ToolCallStatus,
    meta: Option<&acp::Meta>,
) -> String {
    match status {
        acp::ToolCallStatus::Pending => {
            if meta_requires_confirmation(meta) == Some(true) {
                "pending".to_string()
            } else {
                "executing".to_string()
            }
        }
        other => tool_status_to_string(other),
    }
}

fn acp_tool_update_status_to_aipp_status(
    status: Option<&acp::ToolCallStatus>,
    meta: Option<&acp::Meta>,
    existing_status: Option<&str>,
    has_progress_content: bool,
) -> String {
    let incoming_is_terminal = status.is_some_and(|status| {
        matches!(
            status,
            acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
        )
    });

    if matches!(existing_status, Some("success" | "failed")) && !incoming_is_terminal {
        return existing_status.unwrap_or_default().to_string();
    }

    if let Some(status) = status {
        return acp_tool_status_to_aipp_status(status.clone(), meta);
    }

    let existing_status = existing_status.unwrap_or("executing");

    if meta_requires_confirmation(meta) == Some(true) && !has_progress_content {
        return "pending".to_string();
    }

    if has_progress_content || existing_status == "pending" {
        return "executing".to_string();
    }

    existing_status.to_string()
}

/// Convert ACP ToolCallId to i64 for frontend
fn tool_call_id_to_i64(id: &acp::ToolCallId) -> i64 {
    id.0.parse().unwrap_or_else(|_| {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        id.0.hash(&mut hasher);
        hasher.finish() as i64
    })
}

fn permission_option_kind_to_string(kind: &acp::PermissionOptionKind) -> String {
    match kind {
        acp::PermissionOptionKind::AllowOnce => "allow_once".to_string(),
        acp::PermissionOptionKind::AllowAlways => "allow_always".to_string(),
        acp::PermissionOptionKind::RejectOnce => "reject_once".to_string(),
        acp::PermissionOptionKind::RejectAlways => "reject_always".to_string(),
        _ => "unknown".to_string(),
    }
}

fn extract_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn extract_bool_field(value: &serde_json::Value, keys: &[&str]) -> Option<bool> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            if let Some(b) = v.as_bool() {
                return Some(b);
            }
        }
    }
    None
}

fn extract_params_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            return Some(match v {
                serde_json::Value::String(s) => s.to_string(),
                _ => serde_json::to_string(v).unwrap_or_else(|_| v.to_string()),
            });
        }
    }
    None
}

fn extract_tool_call_info_from_value(
    value: &serde_json::Value,
) -> Option<(String, String, String)> {
    if !value.is_object() {
        return None;
    }

    if let Some(nested) =
        value.get("tool").or_else(|| value.get("mcp")).or_else(|| value.get("claudeCode"))
    {
        if let Some(info) = extract_tool_call_info_from_value(nested) {
            return Some(info);
        }
    }

    let server_name = extract_string_field(
        value,
        &["server_name", "serverName", "server", "mcp_server", "mcpServer"],
    )?;
    let tool_name = extract_string_field(
        value,
        &["tool_name", "toolName", "tool", "name", "mcp_tool", "mcpTool"],
    )?;
    let parameters =
        extract_params_field(value, &["parameters", "params", "arguments", "args", "input"])
            .unwrap_or_else(|| "{}".to_string());

    Some((server_name, tool_name, parameters))
}

fn extract_acp_tool_call_info(
    raw_input: Option<&serde_json::Value>,
    meta: Option<&acp::Meta>,
) -> Option<(String, String, String)> {
    if let Some(raw_input) = raw_input {
        if let Some(info) = extract_tool_call_info_from_value(raw_input) {
            return Some(info);
        }
    }

    if let Some(meta) = meta {
        let meta_value = serde_json::Value::Object(meta.clone());
        if let Some(info) = extract_tool_call_info_from_value(&meta_value) {
            return Some(info);
        }
    }

    None
}

fn extract_tool_name_from_meta(meta: Option<&acp::Meta>) -> Option<String> {
    let meta = meta?;
    let meta_value = serde_json::Value::Object(meta.clone());
    extract_string_field(&meta_value, &["toolName", "tool_name", "name"]).or_else(|| {
        meta_value
            .get("claudeCode")
            .and_then(|v| extract_string_field(v, &["toolName", "tool_name", "name"]))
    })
}

fn extract_tool_response_from_meta(meta: Option<&acp::Meta>) -> Option<String> {
    let meta = meta?;
    let meta_value = serde_json::Value::Object(meta.clone());
    let response_value = meta_value
        .get("claudeCode")
        .and_then(|v| v.get("toolResponse"))
        .or_else(|| meta_value.get("toolResponse"));
    response_value.map(|v| serde_json::to_string(v).unwrap_or_else(|_| v.to_string()))
}

fn strip_mcp_tool_call_hints(content: &str) -> String {
    let re = Regex::new(r"<!--\s*MCP_TOOL_CALL:.*?-->").ok();
    match re {
        Some(re) => re.replace_all(content, "").to_string(),
        None => content.to_string(),
    }
}

fn build_acp_history_prompt(app_handle: &tauri::AppHandle, conversation_id: i64) -> Option<String> {
    let db = ConversationDatabase::new(app_handle).ok()?;
    let messages = db.message_repo().ok()?.list_by_conversation_id(conversation_id).ok()?;
    if messages.is_empty() {
        return None;
    }

    // 使用统一的 LatestBranch 算法获取最新分支消息
    let latest_branch = crate::api::ai::summary::get_latest_branch_messages(&messages);

    // 找到最新的用户消息 ID（用于排除当前用户提问）
    let latest_user_id = latest_branch
        .iter()
        .filter(|m| m.message_type == "user")
        .max_by_key(|m| m.id)
        .map(|m| m.id);

    let mut entries: Vec<String> = Vec::new();

    for message in &latest_branch {
        if Some(message.id) == latest_user_id {
            continue;
        }

        let mut content = strip_mcp_tool_call_hints(&message.content);
        let content_trimmed = content.trim();
        if content_trimmed.is_empty() {
            continue;
        }

        let label = match message.message_type.as_str() {
            "system" => "系统",
            "user" => "用户",
            "response" | "assistant" | "reasoning" => "助手",
            "tool_result" => "工具结果",
            _ => "助手",
        };

        if message.message_type == "tool_result" {
            if let Some(result) = extract_tool_result(content_trimmed) {
                content = result;
            }
        }

        let cleaned = content.trim();
        if cleaned.is_empty() {
            continue;
        }

        entries.push(format!("{}: {}", label, cleaned));
    }

    if entries.is_empty() {
        return None;
    }

    Some(format!("以下是历史对话，请在此基础上继续：\n\n{}", entries.join("\n\n")))
}

fn build_params_from_raw_input(
    raw_input: Option<&serde_json::Value>,
    meta: Option<&acp::Meta>,
) -> String {
    if let Some(raw_input) = raw_input {
        return serde_json::to_string(raw_input).unwrap_or_else(|_| raw_input.to_string());
    }

    if let Some(meta) = meta {
        let meta_value = serde_json::Value::Object(meta.clone());
        return serde_json::to_string(&meta_value).unwrap_or_else(|_| meta_value.to_string());
    }

    "{}".to_string()
}

fn meta_requires_confirmation(meta: Option<&acp::Meta>) -> Option<bool> {
    let meta = meta?;
    let meta_value = serde_json::Value::Object(meta.clone());
    extract_bool_field(
        &meta_value,
        &[
            "requires_confirmation",
            "requiresConfirmation",
            "approval_required",
            "needs_confirmation",
        ],
    )
}

fn display_server_name(title: &str, fallback: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn resolve_operation_server_name(app_handle: &tauri::AppHandle) -> String {
    if let Ok(db) = MCPDatabase::new(app_handle) {
        if let Ok(name) = db.conn.query_row(
            "SELECT name FROM mcp_server WHERE command = ? AND is_builtin = 1 LIMIT 1",
            ["aipp:operation"],
            |row| row.get::<_, String>(0),
        ) {
            return name;
        }
    }

    "操作工具".to_string()
}

fn resolve_acp_persistence_server_id(app_handle: &tauri::AppHandle) -> Option<i64> {
    let db = MCPDatabase::new(app_handle).ok()?;
    db.conn
        .query_row(
            "SELECT id
             FROM mcp_server
             WHERE is_builtin = 1
               AND command IN ('aipp:operation', 'aipp:dynamic_mcp')
             ORDER BY CASE command
                 WHEN 'aipp:operation' THEN 0
                 WHEN 'aipp:dynamic_mcp' THEN 1
                 ELSE 2
             END
             LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .ok()
}

fn extract_command_from_title(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 2 {
        return Some(trimmed.trim_matches('`').trim().to_string());
    }

    if trimmed == "Terminal" {
        return None;
    }

    Some(trimmed.to_string())
}

fn fallback_acp_tool_call_info(
    app_handle: &tauri::AppHandle,
    tool_call: &acp::ToolCall,
) -> Option<(String, String, String)> {
    let server_name = resolve_operation_server_name(app_handle);

    match tool_call.kind {
        acp::ToolKind::Execute => {
            let command =
                extract_command_from_title(&tool_call.title).unwrap_or_else(|| "".to_string());
            let params = serde_json::json!({
                "command": command,
            })
            .to_string();
            Some((server_name, "execute_bash".to_string(), params))
        }
        acp::ToolKind::Read => {
            let location = tool_call.locations.first()?;
            let mut params = serde_json::json!({
                "file_path": location.path.to_string_lossy(),
            });
            if let Some(line) = location.line {
                params["offset"] = serde_json::Value::from(line as i64);
            }
            Some((server_name, "read_file".to_string(), params.to_string()))
        }
        _ => None,
    }
}

fn fallback_acp_tool_call_info_from_locations(
    app_handle: &tauri::AppHandle,
    locations: Option<&[acp::ToolCallLocation]>,
) -> Option<(String, String, String)> {
    let locations = locations?;
    let location = locations.first()?;
    let server_name = resolve_operation_server_name(app_handle);
    let mut params = serde_json::json!({
        "file_path": location.path.to_string_lossy(),
    });
    if let Some(line) = location.line {
        params["offset"] = serde_json::Value::from(line as i64);
    }
    Some((server_name, "read_file".to_string(), params.to_string()))
}

/// Tauri client implementation that forwards ACP events to the frontend
#[derive(Clone)]
pub struct AcpTauriClient {
    pub app_handle: tauri::AppHandle,
    pub conversation_id: i64,
    pub message_id: Arc<TokioMutex<i64>>,
    pub window: Arc<TokioMutex<tauri::Window>>,
    operation_state: Arc<OperationState>,
    permission_manager: Arc<PermissionManager>,
    /// Accumulated response content buffer for database persistence
    response_content_buffer: Arc<TokioMutex<String>>,
    /// Accumulated reasoning content buffer for database persistence
    reasoning_content_buffer: Arc<TokioMutex<String>>,
    suppress_updates: Arc<TokioMutex<bool>>,
    tool_call_id_map: Arc<TokioMutex<HashMap<String, i64>>>,
    terminal_output_limits: Arc<TokioMutex<HashMap<String, usize>>>,
}

impl AcpTauriClient {
    pub fn new(
        app_handle: tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
        window: tauri::Window,
        operation_state: Arc<OperationState>,
        permission_manager: Arc<PermissionManager>,
    ) -> Self {
        Self {
            app_handle,
            conversation_id,
            message_id: Arc::new(TokioMutex::new(message_id)),
            window: Arc::new(TokioMutex::new(window)),
            operation_state,
            permission_manager,
            response_content_buffer: Arc::new(TokioMutex::new(String::new())),
            reasoning_content_buffer: Arc::new(TokioMutex::new(String::new())),
            suppress_updates: Arc::new(TokioMutex::new(false)),
            tool_call_id_map: Arc::new(TokioMutex::new(HashMap::new())),
            terminal_output_limits: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// Convert ACP session_id to conversation_id
    fn get_conversation_id(&self) -> Option<i64> {
        Some(self.conversation_id)
    }

    /// Update message content in database
    async fn update_message_in_db(&self, content: &str) {
        let message_id = *self.message_id.lock().await;
        if let Ok(db) = ConversationDatabase::new(&self.app_handle) {
            if let Ok(repo) = db.message_repo() {
                if let Err(e) = repo.update_content(message_id, content) {
                    error!("ACP: Failed to update message in DB: {}", e);
                }
            }
        }
    }

    async fn persist_message_usage_in_db(
        &self,
        usage_summary: &AcpPromptUsageSummary,
        final_content: &str,
    ) -> Result<(), AppError> {
        let message_id = *self.message_id.lock().await;
        let db = ConversationDatabase::new(&self.app_handle).map_err(AppError::from)?;
        let repo = db.message_repo()?;
        let Some(mut message) = repo.read(message_id).map_err(AppError::from)? else {
            return Err(AppError::UnknownError(format!(
                "ACP response message not found for usage persistence: {message_id}"
            )));
        };

        message.content = final_content.to_string();
        message.token_count = usage_summary.total_tokens as i32;
        message.input_token_count = usage_summary.input_tokens as i32;
        message.output_token_count = usage_summary.output_tokens as i32;
        if message.finish_time.is_none() {
            message.finish_time = Some(chrono::Utc::now());
        }
        message.metadata_json = Some(merge_acp_usage_metadata(
            message.metadata_json.as_deref(),
            usage_summary,
        )?);
        repo.update(&message).map_err(AppError::from)?;
        Ok(())
    }

    /// Get the accumulated response content
    pub async fn get_response_content(&self) -> String {
        self.response_content_buffer.lock().await.clone()
    }

    /// Get the accumulated reasoning content
    pub async fn get_reasoning_content(&self) -> String {
        self.reasoning_content_buffer.lock().await.clone()
    }

    pub async fn reset_buffers(&self) {
        *self.response_content_buffer.lock().await = String::new();
        *self.reasoning_content_buffer.lock().await = String::new();
    }

    pub async fn set_current_message(&self, message_id: i64) {
        *self.message_id.lock().await = message_id;
    }

    pub async fn set_window(&self, window: tauri::Window) {
        *self.window.lock().await = window;
    }

    pub async fn set_suppress_updates(&self, suppress: bool) {
        *self.suppress_updates.lock().await = suppress;
    }

    async fn update_session_state<F>(&self, update: F) -> Option<AcpConversationSessionState>
    where
        F: FnOnce(&mut AcpConversationSessionState),
    {
        let session_state = self.app_handle.state::<crate::AcpSessionState>();
        let mut sessions = session_state.sessions.lock().await;
        let entry = sessions.get_mut(&self.conversation_id)?;
        entry.touch();
        update(&mut entry.snapshot);
        Some(entry.snapshot.clone())
    }

    async fn publish_session_state(&self, state: Option<AcpConversationSessionState>) {
        emit_acp_session_state_snapshot(&self.app_handle, self.conversation_id, state);
    }

    async fn set_session_bootstrap(
        &self,
        session_id: &str,
        load_session_supported: bool,
        session_resume_supported: bool,
        restored_session_method: Option<&str>,
        prompt_capabilities: &acp::PromptCapabilities,
        config_options: Option<&[acp::SessionConfigOption]>,
    ) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.session_id = Some(session_id.to_string());
                snapshot.load_session_supported = load_session_supported;
                snapshot.session_resume_supported = session_resume_supported;
                snapshot.restored_session_method =
                    restored_session_method.map(|method| method.to_string());
                snapshot.prompt_capabilities = prompt_capabilities_payload(prompt_capabilities);
                apply_config_options_to_snapshot(snapshot, config_options);
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn set_active_prompt(&self, has_active_prompt: bool) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.has_active_prompt = has_active_prompt;
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn apply_current_mode_update(&self, mode_update: &acp::CurrentModeUpdate) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.current_mode_id = Some(mode_update.current_mode_id.to_string());
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn apply_config_option_update(&self, config_update: &acp::ConfigOptionUpdate) {
        let state = self
            .update_session_state(|snapshot| {
                apply_config_options_to_snapshot(snapshot, Some(&config_update.config_options));
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn apply_plan_update(&self, plan: &acp::Plan) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.plan = plan_payload(plan);
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn apply_available_commands_update(&self, commands_update: &acp::AvailableCommandsUpdate) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.available_commands =
                    available_commands_payload(&commands_update.available_commands);
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn apply_usage_update(&self, usage_update: &acp::UsageUpdate) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.context_tokens_used = Some(usage_update.used);
                snapshot.context_window_size = Some(usage_update.size);
                snapshot.session_cost_amount = usage_update.cost.as_ref().map(|cost| cost.amount);
                snapshot.session_cost_currency =
                    usage_update.cost.as_ref().map(|cost| cost.currency.clone());
            })
            .await;
        self.publish_session_state(state).await;
    }

    async fn update_conversation_title(&self, title: &str) {
        let Ok(db) = ConversationDatabase::new(&self.app_handle) else {
            return;
        };
        let Ok(repo) = db.conversation_repo() else {
            return;
        };
        let Ok(Some(mut conversation)) = repo.read(self.conversation_id) else {
            return;
        };
        if conversation.name == title {
            return;
        }
        conversation.name = title.to_string();
        if let Err(error) = repo.update(&conversation) {
            warn!(
                conversation_id = self.conversation_id,
                error = %error,
                "ACP failed to persist conversation title"
            );
            return;
        }
        let _ = self
            .app_handle
            .emit(TITLE_CHANGE_EVENT, (self.conversation_id, title.to_string()));
    }

    async fn apply_session_info_update(&self, info_update: &acp::SessionInfoUpdate) {
        let mut title_to_persist = None;
        let state = self
            .update_session_state(|snapshot| {
                if let Some(title_state) = info_update.title.as_opt_ref() {
                    let next_title = title_state.map(ToOwned::to_owned);
                    if next_title.as_ref().is_some_and(|value| !value.is_empty())
                        && snapshot.title != next_title
                    {
                        title_to_persist = next_title.clone();
                    }
                    snapshot.title = next_title;
                }
                if let Some(updated_at_state) = info_update.updated_at.as_opt_ref() {
                    snapshot.updated_at = updated_at_state.map(ToOwned::to_owned);
                }
            })
            .await;

        if let Some(title) = title_to_persist {
            self.update_conversation_title(&title).await;
        }
        self.publish_session_state(state).await;
    }

    async fn emit_event(&self, event: ConversationEvent) {
        send_conversation_event_to_chat_windows(&self.app_handle, self.conversation_id, event);
    }

    async fn mark_assistant_streaming(&self, message_id: i64) {
        if let Some(activity_manager) = self.app_handle.try_state::<ConversationActivityManager>() {
            activity_manager
                .set_assistant_streaming(&self.app_handle, self.conversation_id, message_id)
                .await;
        }
    }

    async fn sync_tool_shine_status(&self, call_id: i64, status: &str) {
        if let Some(activity_manager) = self.app_handle.try_state::<ConversationActivityManager>() {
            match status {
                "pending" => {
                    activity_manager
                        .set_mcp_pending(&self.app_handle, self.conversation_id, call_id)
                        .await;
                }
                "executing" => {
                    activity_manager
                        .set_mcp_executing(&self.app_handle, self.conversation_id, call_id)
                        .await;
                }
                "success" | "failed" => {
                    activity_manager
                        .finish_mcp_call(&self.app_handle, self.conversation_id, call_id)
                        .await;
                }
                _ => {}
            }
        }
    }

    async fn finish_unfinished_tool_calls(&self, status: &str, error: Option<&str>) {
        let call_ids = {
            let map = self.tool_call_id_map.lock().await;
            map.values().copied().collect::<Vec<_>>()
        };
        if call_ids.is_empty() {
            return;
        }

        let Ok(db) = MCPDatabase::new(&self.app_handle) else {
            return;
        };

        for call_id in call_ids {
            let Ok(call) = db.get_mcp_tool_call(call_id) else {
                continue;
            };
            if !matches!(call.status.as_str(), "pending" | "executing" | "unknown") {
                continue;
            }

            let error_text = if status == "failed" {
                error
                    .unwrap_or("ACP prompt ended before the agent sent a final tool status")
                    .to_string()
            } else {
                String::new()
            };
            let result = if status == "success" { call.result.as_deref() } else { None };
            let error_for_db = (status == "failed").then_some(error_text.as_str());

            if let Err(update_error) =
                db.update_mcp_tool_call_status(call_id, status, result, error_for_db)
            {
                warn!(
                    call_id,
                    status,
                    error = %update_error,
                    "ACP failed to finish unfinished tool call"
                );
                continue;
            }

            let event = ConversationEvent {
                r#type: "mcp_tool_call_update".to_string(),
                data: serde_json::to_value(MCPToolCallUpdateEvent {
                    call_id,
                    conversation_id: self.conversation_id,
                    message_id: call.message_id,
                    status: status.to_string(),
                    llm_call_id: call.llm_call_id.clone(),
                    server_name: Some(call.server_name.clone()),
                    tool_name: Some(call.tool_name.clone()),
                    parameters: Some(call.parameters.clone()),
                    result: call.result.clone(),
                    error: (status == "failed").then_some(error_text),
                    started_time: None,
                    finished_time: Some(chrono::Utc::now()),
                })
                .unwrap(),
            };
            self.emit_event(event).await;
            self.sync_tool_shine_status(call_id, status).await;
        }
    }

    async fn remember_terminal_output_limit(&self, bash_id: &str, output_byte_limit: Option<u64>) {
        if let Some(limit) = output_byte_limit.and_then(|value| usize::try_from(value).ok()) {
            self.terminal_output_limits.lock().await.insert(bash_id.to_string(), limit);
        }
    }

    async fn forget_terminal_output_limit(&self, bash_id: &str) {
        self.terminal_output_limits.lock().await.remove(bash_id);
    }

    async fn append_terminal_output_with_limit(
        state: &OperationState,
        limits: &Arc<TokioMutex<HashMap<String, usize>>>,
        bash_id: &str,
        output: &str,
    ) {
        state.append_bash_output(bash_id, output).await;
        let limit = {
            let limits = limits.lock().await;
            limits.get(bash_id).copied()
        };
        let Some(limit) = limit else {
            return;
        };
        let mut processes = state.bash_processes.lock().await;
        if let Some(info) = processes.get_mut(bash_id) {
            if info.output_buffer.len() <= limit {
                return;
            }
            let mut start = info.output_buffer.len().saturating_sub(limit);
            while start < info.output_buffer.len() && !info.output_buffer.is_char_boundary(start)
            {
                start += 1;
            }
            info.output_buffer = info.output_buffer[start..].to_string();
            info.last_read_pos = info.last_read_pos.saturating_sub(start);
        }
    }

    async fn read_terminal_stream<R>(
        state: Arc<OperationState>,
        limits: Arc<TokioMutex<HashMap<String, usize>>>,
        bash_id: String,
        mut reader: R,
        is_stderr: bool,
    ) where
        R: AsyncRead + Unpin,
    {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => {
                    let decoded = String::from_utf8_lossy(&buffer[..read]);
                    let formatted = if is_stderr {
                        format!("[stderr] {}", decoded)
                    } else {
                        decoded.to_string()
                    };
                    Self::append_terminal_output_with_limit(
                        &state,
                        &limits,
                        &bash_id,
                        &formatted,
                    )
                    .await;
                }
                Err(error) => {
                    let label = if is_stderr { "stderr" } else { "stdout" };
                    Self::append_terminal_output_with_limit(
                        &state,
                        &limits,
                        &bash_id,
                        &format!("[error reading {}: {}]\n", label, error),
                    )
                    .await;
                    break;
                }
            }
        }
    }

    async fn spawn_structured_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> Result<String, String> {
        let bash_id = uuid::Uuid::new_v4().to_string();
        let mut cmd = Command::new(&args.command);
        cmd.args(&args.args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(cwd) = args.cwd.as_ref() {
            cmd.current_dir(cwd);
        }
        for env_var in &args.env {
            cmd.env(&env_var.name, &env_var.value);
        }

        let mut child = cmd.spawn().map_err(|error| {
            format!(
                "Failed to spawn terminal command '{}': {}",
                args.command, error
            )
        })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        self.operation_state.store_bash_process(bash_id.clone(), child).await;
        self.remember_terminal_output_limit(&bash_id, args.output_byte_limit).await;

        let state = Arc::clone(&self.operation_state);
        let limits = Arc::clone(&self.terminal_output_limits);
        let bash_id_for_task = bash_id.clone();
        tokio::spawn(async move {
            let mut tasks = Vec::new();
            if let Some(stdout) = stdout {
                tasks.push(tokio::spawn(Self::read_terminal_stream(
                    Arc::clone(&state),
                    Arc::clone(&limits),
                    bash_id_for_task.clone(),
                    stdout,
                    false,
                )));
            }
            if let Some(stderr) = stderr {
                tasks.push(tokio::spawn(Self::read_terminal_stream(
                    Arc::clone(&state),
                    Arc::clone(&limits),
                    bash_id_for_task.clone(),
                    stderr,
                    true,
                )));
            }
            for task in tasks {
                let _ = task.await;
            }
            let exit_code = state.get_bash_exit_code(&bash_id_for_task).await;
            state.mark_bash_completed(&bash_id_for_task, exit_code).await;
        });

        Ok(bash_id)
    }

    async fn append_tool_call_ui_hint(
        &self,
        server_name: &str,
        tool_name: &str,
        parameters: &str,
        call_id: i64,
        llm_call_id: &str,
    ) {
        let ui_hint = format!(
            "\n\n<!-- MCP_TOOL_CALL:{} -->\n",
            serde_json::json!({
                "server_name": server_name,
                "tool_name": tool_name,
                "parameters": parameters,
                "call_id": call_id,
                "llm_call_id": llm_call_id,
            })
        );

        let full_content = {
            let mut buffer = self.response_content_buffer.lock().await;
            if buffer.contains(&format!("\"llm_call_id\":\"{}\"", llm_call_id)) {
                return;
            }
            buffer.push_str(&ui_hint);
            buffer.clone()
        };

        self.update_message_in_db(&full_content).await;

        let message_id = *self.message_id.lock().await;
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "response".to_string(),
                content: full_content,
                is_done: false,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        };

        self.mark_assistant_streaming(message_id).await;
        self.emit_event(event).await;
    }

    async fn create_acp_tool_call_record(
        &self,
        server_name: &str,
        display_name: &str,
        tool_name: &str,
        parameters: &str,
        llm_call_id: &str,
    ) -> Result<MCPToolCall, String> {
        let message_id = *self.message_id.lock().await;

        match crate::mcp::execution_api::create_mcp_tool_call_with_llm_id(
            self.app_handle.clone(),
            self.conversation_id,
            Some(message_id),
            server_name.to_string(),
            tool_name.to_string(),
            parameters.to_string(),
            Some(llm_call_id),
            Some(message_id),
        )
        .await
        {
            Ok(record) => {
                if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                    let _ = db.update_mcp_tool_call_metadata(
                        record.id,
                        display_name,
                        tool_name,
                        parameters,
                    );
                }
                Ok(record)
            }
            Err(primary_error) => {
                let db = MCPDatabase::new(&self.app_handle)
                    .map_err(|error| format!("打开 MCP 数据库失败: {error}"))?;
                let persistence_server_id = resolve_acp_persistence_server_id(&self.app_handle)
                    .ok_or_else(|| {
                        format!(
                            "ACP 工具调用持久化失败，缺少可用的 MCP 持久化 server_id（原始错误: {primary_error}）"
                        )
                    })?;

                db.create_mcp_tool_call_with_server_id_and_llm_id(
                    self.conversation_id,
                    Some(message_id),
                    persistence_server_id,
                    display_name,
                    tool_name,
                    parameters,
                    Some(llm_call_id),
                    Some(message_id),
                )
                .map_err(|fallback_error| {
                    format!(
                        "ACP 工具调用持久化失败: primary={primary_error}; fallback={fallback_error}"
                    )
                })
            }
        }
    }

    /// Send completion event to frontend
    async fn send_done_event(
        &self,
        message_type: &str,
        content: &str,
        usage_summary: Option<&AcpPromptUsageSummary>,
    ) {
        let message_id = *self.message_id.lock().await;
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: message_type.to_string(),
                content: content.to_string(),
                is_done: true,
                token_count: usage_summary.map(|usage| usage.total_tokens as i32),
                input_token_count: usage_summary.map(|usage| usage.input_tokens as i32),
                output_token_count: usage_summary.map(|usage| usage.output_tokens as i32),
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        };

        self.emit_event(event).await;
        if let Some(activity_manager) = self.app_handle.try_state::<ConversationActivityManager>() {
            activity_manager
                .clear_message_focus_keep_mcp(&self.app_handle, self.conversation_id)
                .await;
        }

        let stream_complete_event = ConversationEvent {
            r#type: "stream_complete".to_string(),
            data: serde_json::json!({
                "conversation_id": self.conversation_id,
                "response_message_id": message_id,
                "reasoning_message_id": null,
                "has_response": message_type == "response",
                "has_reasoning": message_type == "reasoning",
                "response_length": content.len(),
                "reasoning_length": 0,
            }),
        };
        send_conversation_event_to_chat_windows(
            &self.app_handle,
            self.conversation_id,
            stream_complete_event,
        );
        emit_conversation_list_activity(
            &self.app_handle,
            ConversationListActivityEvent {
                conversation_id: self.conversation_id,
                kind: "stream_complete".to_string(),
                is_running: None,
            },
        );
    }

    /// Send error event to frontend
    pub async fn send_error_event(&self, error_message: &str) {
        // Update database with error message
        self.update_message_in_db(error_message).await;

        let message_id = *self.message_id.lock().await;
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "error".to_string(),
                content: error_message.to_string(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        };

        self.emit_event(event).await;
        if let Some(activity_manager) = self.app_handle.try_state::<ConversationActivityManager>() {
            activity_manager.clear_focus(&self.app_handle, self.conversation_id).await;
        }
    }
}

impl AcpTauriClient {
    async fn session_notification(
        &self,
        args: acp::SessionNotification,
    ) -> acp::Result<(), acp::Error> {
        if *self.suppress_updates.lock().await {
            debug!("ACP session_notification suppressed");
            return Ok(());
        }

        // Log the notification type for debugging
        let update_type =
            std::format!("{:?}", args.update).split('(').next().unwrap_or("Unknown").to_string();
        let message_id = *self.message_id.lock().await;
        debug!("ACP session_notification: type={}, message_id={}", update_type, message_id);

        match args.update {
            // User message streaming - just log, don't emit to UI (user message is already shown)
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk { content, .. }) => {
                let text = extract_content_text(&content);
                debug!("ACP UserMessageChunk (ignored): {}", text);
                // Note: We intentionally don't emit this to UI because:
                // 1. The user message is already displayed in the conversation
                // 2. Writing to self.message_id (which is the response message) would be wrong
            }

            // Agent response streaming - accumulate, persist to DB, and emit to frontend
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk { content, .. }) => {
                let text = extract_content_text(&content);
                info!("ACP AgentMessageChunk: {} chars", text.len());

                // Accumulate content
                let full_content = {
                    let mut buffer = self.response_content_buffer.lock().await;
                    append_buffered_content(&mut buffer, &content)
                };

                // Persist to database
                self.update_message_in_db(&full_content).await;

                // Emit full content to frontend (matching existing UI behavior)
                let message_id = *self.message_id.lock().await;
                self.mark_assistant_streaming(message_id).await;
                let event = ConversationEvent {
                    r#type: "message_update".to_string(),
                    data: serde_json::to_value(MessageUpdateEvent {
                        message_id,
                        message_type: "response".to_string(),
                        content: full_content,
                        is_done: false,
                        token_count: None,
                        input_token_count: None,
                        output_token_count: None,
                        ttft_ms: None,
                        tps: None,
                    })
                    .unwrap(),
                };

                let window = self.window.lock().await.clone();
                let event_name = format!("conversation_event_{}", self.conversation_id);
                match window.emit(&event_name, event) {
                    Ok(_) => debug!("ACP: Emitted AgentMessageChunk event"),
                    Err(e) => error!("ACP: Failed to emit AgentMessageChunk event: {}", e),
                }
            }

            // Agent internal reasoning (thoughts) - accumulate and emit as reasoning message type
            acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk { content, .. }) => {
                let text = extract_content_text(&content);
                info!("ACP AgentThoughtChunk: {} chars", text.len());

                // Accumulate reasoning content
                let full_reasoning = {
                    let mut buffer = self.reasoning_content_buffer.lock().await;
                    append_buffered_content(&mut buffer, &content)
                };

                // Emit full reasoning content to frontend
                let message_id = *self.message_id.lock().await;
                self.mark_assistant_streaming(message_id).await;
                let event = ConversationEvent {
                    r#type: "message_update".to_string(),
                    data: serde_json::to_value(MessageUpdateEvent {
                        message_id,
                        message_type: "reasoning".to_string(),
                        content: full_reasoning,
                        is_done: false,
                        token_count: None,
                        input_token_count: None,
                        output_token_count: None,
                        ttft_ms: None,
                        tps: None,
                    })
                    .unwrap(),
                };

                self.emit_event(event).await;
            }

            // New tool call initiated - emit as MCP tool call update with pending status
            acp::SessionUpdate::ToolCall(tool_call) => {
                info!(
                    "ACP ToolCall: id={:?}, title={:?}, status={:?}, kind={:?}",
                    tool_call.tool_call_id, tool_call.title, tool_call.status, tool_call.kind
                );
                debug!(?tool_call, "ACP ToolCall detail");

                if let Some(existing_call_id) = {
                    let map = self.tool_call_id_map.lock().await;
                    map.get(tool_call.tool_call_id.0.as_ref()).cloned()
                } {
                    let (server_name, tool_name, parameters) = match extract_acp_tool_call_info(
                        tool_call.raw_input.as_ref(),
                        tool_call.meta.as_ref(),
                    )
                    .or_else(|| fallback_acp_tool_call_info(&self.app_handle, &tool_call))
                    {
                        Some(info) => info,
                        None => {
                            let display_name =
                                display_server_name(&tool_call.title, "ACP ToolCall");
                            let derived_tool_name =
                                extract_tool_name_from_meta(tool_call.meta.as_ref())
                                    .unwrap_or_else(|| "acp_tool".to_string());
                            let derived_params = build_params_from_raw_input(
                                tool_call.raw_input.as_ref(),
                                tool_call.meta.as_ref(),
                            );
                            (display_name, derived_tool_name, derived_params)
                        }
                    };

                    let display_name = display_server_name(&tool_call.title, &server_name);

                    if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                        let _ = db.update_mcp_tool_call_metadata(
                            existing_call_id,
                            &display_name,
                            &tool_name,
                            &parameters,
                        );
                    }

                    let status_str =
                        acp_tool_status_to_aipp_status(tool_call.status, tool_call.meta.as_ref());

                    let (finished_time, result, error) = match tool_call.status {
                        acp::ToolCallStatus::Completed => {
                            (Some(chrono::Utc::now()), tool_call.raw_output.as_ref(), None)
                        }
                        acp::ToolCallStatus::Failed => {
                            (Some(chrono::Utc::now()), None, tool_call.raw_output.as_ref())
                        }
                        _ => (None, None, None),
                    };

                    let mut result_str = result.map(|r| r.to_string());
                    let mut error_str = error.map(|e| e.to_string());
                    if matches!(status_str.as_str(), "success" | "failed") {
                        if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                            if let Ok(existing_call) = db.get_mcp_tool_call(existing_call_id) {
                                if status_str == "success" && result_str.is_none() {
                                    result_str = existing_call.result;
                                }
                                if status_str == "failed" && error_str.is_none() {
                                    error_str = existing_call.error;
                                }
                            }
                        }
                    }

                    if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                        let _ = db.update_mcp_tool_call_status(
                            existing_call_id,
                            &status_str,
                            result_str.as_deref(),
                            error_str.as_deref(),
                        );
                    }

                    let status_for_shine = status_str.clone();
                    let event = ConversationEvent {
                        r#type: "mcp_tool_call_update".to_string(),
                        data: serde_json::to_value(MCPToolCallUpdateEvent {
                            call_id: existing_call_id,
                            conversation_id: self.conversation_id,
                            message_id: None,
                            status: status_str,
                            llm_call_id: None,
                            server_name: Some(display_name),
                            tool_name: Some(tool_name),
                            parameters: Some(parameters),
                            result: result_str,
                            error: error_str,
                            started_time: None,
                            finished_time,
                        })
                        .unwrap(),
                    };

                    self.emit_event(event).await;
                    self.sync_tool_shine_status(existing_call_id, &status_for_shine).await;
                    return Ok(());
                }

                let (server_name, tool_name, parameters) = match extract_acp_tool_call_info(
                    tool_call.raw_input.as_ref(),
                    tool_call.meta.as_ref(),
                )
                .or_else(|| fallback_acp_tool_call_info(&self.app_handle, &tool_call))
                {
                    Some(info) => info,
                    None => {
                        tracing::warn!(
                            title = %tool_call.title,
                            kind = ?tool_call.kind,
                            locations = tool_call.locations.len(),
                            raw_input = ?tool_call.raw_input,
                            meta = ?tool_call.meta,
                            "ACP ToolCall missing server/tool info; using display-only UI hint"
                        );

                        let display_name = display_server_name(&tool_call.title, "ACP ToolCall");
                        let derived_tool_name =
                            extract_tool_name_from_meta(tool_call.meta.as_ref())
                                .unwrap_or_else(|| "acp_tool".to_string());
                        let derived_params = build_params_from_raw_input(
                            tool_call.raw_input.as_ref(),
                            tool_call.meta.as_ref(),
                        );

                        let persisted_record = self
                            .create_acp_tool_call_record(
                                &display_name,
                                &display_name,
                                &derived_tool_name,
                                &derived_params,
                                &tool_call.tool_call_id.0,
                            )
                            .await;

                        let call_id = match persisted_record {
                            Ok(record) => {
                                {
                                    let mut map = self.tool_call_id_map.lock().await;
                                    map.insert(tool_call.tool_call_id.0.to_string(), record.id);
                                }
                                self.append_tool_call_ui_hint(
                                    &display_name,
                                    &derived_tool_name,
                                    &derived_params,
                                    record.id,
                                    &tool_call.tool_call_id.0,
                                )
                                .await;
                                record.id
                            }
                            Err(error) => {
                                tracing::warn!(error = %error, "ACP failed to create synthetic MCP tool call record");
                                let call_id = tool_call_id_to_i64(&tool_call.tool_call_id);
                                {
                                    let mut map = self.tool_call_id_map.lock().await;
                                    map.insert(tool_call.tool_call_id.0.to_string(), call_id);
                                }
                                self.append_tool_call_ui_hint(
                                    &display_name,
                                    &derived_tool_name,
                                    &derived_params,
                                    call_id,
                                    &tool_call.tool_call_id.0,
                                )
                                .await;
                                call_id
                            }
                        };

                        let status_str = acp_tool_status_to_aipp_status(
                            tool_call.status,
                            tool_call.meta.as_ref(),
                        );

                        let (finished_time, result, error) = match tool_call.status {
                            acp::ToolCallStatus::Completed => {
                                (Some(chrono::Utc::now()), tool_call.raw_output.as_ref(), None)
                            }
                            acp::ToolCallStatus::Failed => {
                                (Some(chrono::Utc::now()), None, tool_call.raw_output.as_ref())
                            }
                            _ => (None, None, None),
                        };

                        let status_for_shine = status_str.clone();
                        let event = ConversationEvent {
                            r#type: "mcp_tool_call_update".to_string(),
                            data: serde_json::to_value(MCPToolCallUpdateEvent {
                                call_id,
                                conversation_id: self.conversation_id,
                                message_id: None,
                                status: status_str,
                                llm_call_id: None,
                                server_name: Some(display_name),
                                tool_name: Some(derived_tool_name),
                                parameters: Some(derived_params),
                                result: result.map(|r| r.to_string()),
                                error: error.map(|e| e.to_string()),
                                started_time: Some(chrono::Utc::now()),
                                finished_time,
                            })
                            .unwrap(),
                        };

                        self.emit_event(event).await;
                        self.sync_tool_shine_status(call_id, &status_for_shine).await;
                        return Ok(());
                    }
                };

                let display_name = display_server_name(&tool_call.title, &server_name);
                let tool_call_record =
                    match self
                        .create_acp_tool_call_record(
                            &server_name,
                            &display_name,
                            &tool_name,
                            &parameters,
                            &tool_call.tool_call_id.0,
                        )
                        .await
                    {
                        Ok(record) => record,
                        Err(e) => {
                            tracing::warn!(error = %e, "ACP failed to create MCP tool call record");
                            let call_id = tool_call_id_to_i64(&tool_call.tool_call_id);
                            let status_str = acp_tool_status_to_aipp_status(
                                tool_call.status,
                                tool_call.meta.as_ref(),
                            );
                            let event = ConversationEvent {
                                r#type: "mcp_tool_call_update".to_string(),
                                data: serde_json::to_value(MCPToolCallUpdateEvent {
                                    call_id,
                                    conversation_id: self.conversation_id,
                                    message_id: None,
                                    status: status_str.clone(),
                                    llm_call_id: None,
                                    server_name: Some(display_name),
                                    tool_name: Some(tool_name),
                                    parameters: Some(parameters),
                                    result: None,
                                    error: None,
                                    started_time: Some(chrono::Utc::now()),
                                    finished_time: None,
                                })
                                .unwrap(),
                            };

                            self.emit_event(event).await;
                            self.sync_tool_shine_status(call_id, &status_str).await;
                            return Ok(());
                        }
                    };

                {
                    let mut map = self.tool_call_id_map.lock().await;
                    map.insert(tool_call.tool_call_id.0.to_string(), tool_call_record.id);
                }

                self.append_tool_call_ui_hint(
                    &display_name,
                    &tool_name,
                    &parameters,
                    tool_call_record.id,
                    &tool_call.tool_call_id.0,
                )
                .await;

                let status_str =
                    acp_tool_status_to_aipp_status(tool_call.status, tool_call.meta.as_ref());

                let (finished_time, result, error) = match tool_call.status {
                    acp::ToolCallStatus::Completed => {
                        (Some(chrono::Utc::now()), tool_call.raw_output.as_ref(), None)
                    }
                    acp::ToolCallStatus::Failed => {
                        (Some(chrono::Utc::now()), None, tool_call.raw_output.as_ref())
                    }
                    _ => (None, None, None),
                };

                let result_str = result.map(|r| r.to_string());
                let error_str = error.map(|e| e.to_string());

                if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                    let _ = db.update_mcp_tool_call_status(
                        tool_call_record.id,
                        &status_str,
                        result_str.as_deref(),
                        error_str.as_deref(),
                    );
                }

                let status_for_shine = status_str.clone();
                let event = ConversationEvent {
                    r#type: "mcp_tool_call_update".to_string(),
                    data: serde_json::to_value(MCPToolCallUpdateEvent {
                        call_id: tool_call_record.id,
                        conversation_id: self.conversation_id,
                        message_id: tool_call_record.message_id,
                        status: status_str,
                        llm_call_id: None,
                        server_name: Some(display_name),
                        tool_name: Some(tool_name),
                        parameters: Some(parameters),
                        result: result_str,
                        error: error_str,
                        started_time: None,
                        finished_time,
                    })
                    .unwrap(),
                };

                self.emit_event(event).await;
                self.sync_tool_shine_status(tool_call_record.id, &status_for_shine).await;
            }

            // Tool call status update - emit as MCP tool call update
            acp::SessionUpdate::ToolCallUpdate(update) => {
                info!(
                    "ACP ToolCallUpdate: id={:?}, status={:?}",
                    update.tool_call_id, update.fields.status
                );
                debug!(?update, "ACP ToolCallUpdate detail");

                let mut call_id_opt = {
                    let map = self.tool_call_id_map.lock().await;
                    map.get(update.tool_call_id.0.as_ref()).cloned()
                };

                if call_id_opt.is_none() {
                    if let Some((server_name, tool_name, parameters)) = extract_acp_tool_call_info(
                        update.fields.raw_input.as_ref(),
                        update.meta.as_ref(),
                    )
                    .or_else(|| {
                        update
                            .fields
                            .title
                            .as_ref()
                            .map(|title| {
                                let mut tool_call =
                                    acp::ToolCall::new(update.tool_call_id.clone(), title.clone());
                                if let Some(kind) = update.fields.kind {
                                    tool_call.kind = kind;
                                }
                                if let Some(locations) = update.fields.locations.clone() {
                                    tool_call.locations = locations;
                                }
                                tool_call
                            })
                            .and_then(|tool_call| {
                                fallback_acp_tool_call_info(&self.app_handle, &tool_call)
                            })
                    })
                    .or_else(|| {
                        fallback_acp_tool_call_info_from_locations(
                            &self.app_handle,
                            update.fields.locations.as_deref(),
                        )
                    }) {
                        let display_name = update
                            .fields
                            .title
                            .as_ref()
                            .map(|title| display_server_name(title, &server_name))
                            .unwrap_or_else(|| server_name.clone());
                        if let Ok(record) = self
                            .create_acp_tool_call_record(
                                &server_name,
                                &display_name,
                                &tool_name,
                                &parameters,
                                &update.tool_call_id.0,
                            )
                            .await
                        {
                            {
                                let mut map = self.tool_call_id_map.lock().await;
                                map.insert(update.tool_call_id.0.to_string(), record.id);
                            }
                            call_id_opt = Some(record.id);
                            self.append_tool_call_ui_hint(
                                &display_name,
                                &tool_name,
                                &parameters,
                                record.id,
                                &update.tool_call_id.0,
                            )
                            .await;
                        }
                    } else {
                        let display_name = update
                            .fields
                            .title
                            .as_ref()
                            .map(|title| display_server_name(title, "ACP ToolCall"))
                            .unwrap_or_else(|| "ACP ToolCall".to_string());
                        let derived_tool_name = extract_tool_name_from_meta(update.meta.as_ref())
                            .unwrap_or_else(|| "acp_tool".to_string());
                        let derived_params = build_params_from_raw_input(
                            update.fields.raw_input.as_ref(),
                            update.meta.as_ref(),
                        );
                        if let Ok(record) = self
                            .create_acp_tool_call_record(
                                &display_name,
                                &display_name,
                                &derived_tool_name,
                                &derived_params,
                                &update.tool_call_id.0,
                            )
                            .await
                        {
                            {
                                let mut map = self.tool_call_id_map.lock().await;
                                map.insert(update.tool_call_id.0.to_string(), record.id);
                            }
                            self.append_tool_call_ui_hint(
                                &display_name,
                                &derived_tool_name,
                                &derived_params,
                                record.id,
                                &update.tool_call_id.0,
                            )
                            .await;
                            call_id_opt = Some(record.id);
                        } else {
                            let derived_call_id = tool_call_id_to_i64(&update.tool_call_id);
                            {
                                let mut map = self.tool_call_id_map.lock().await;
                                map.insert(update.tool_call_id.0.to_string(), derived_call_id);
                            }
                            self.append_tool_call_ui_hint(
                                &display_name,
                                &derived_tool_name,
                                &derived_params,
                                derived_call_id,
                                &update.tool_call_id.0,
                            )
                            .await;
                            call_id_opt = Some(derived_call_id);
                        }
                    }
                }

                let call_id =
                    call_id_opt.unwrap_or_else(|| tool_call_id_to_i64(&update.tool_call_id));

                let existing_call = if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                    db.get_mcp_tool_call(call_id).ok()
                } else {
                    None
                };
                let mut status_str = acp_tool_update_status_to_aipp_status(
                    update.fields.status.as_ref(),
                    update.meta.as_ref(),
                    existing_call.as_ref().map(|call| call.status.as_str()),
                    update.fields.content.is_some() || update.fields.raw_output.is_some(),
                );

                let meta_result = extract_tool_response_from_meta(update.meta.as_ref());

                let mut updated_server_name: Option<String> = None;
                let mut updated_tool_name: Option<String> = None;
                let mut updated_parameters: Option<String> = None;

                if update.fields.title.is_some()
                    || update.fields.raw_input.is_some()
                    || update.fields.locations.is_some()
                    || update.fields.kind.is_some()
                {
                    let title =
                        update.fields.title.as_ref().map(|t| t.as_str()).unwrap_or("ACP ToolCall");
                    let (server_name, tool_name, parameters) = extract_acp_tool_call_info(
                        update.fields.raw_input.as_ref(),
                        update.meta.as_ref(),
                    )
                    .or_else(|| {
                        let mut tool_call =
                            acp::ToolCall::new(update.tool_call_id.clone(), title.to_string());
                        if let Some(kind) = update.fields.kind {
                            tool_call.kind = kind;
                        }
                        if let Some(locations) = update.fields.locations.clone() {
                            tool_call.locations = locations;
                        }
                        fallback_acp_tool_call_info(&self.app_handle, &tool_call).or_else(|| {
                            fallback_acp_tool_call_info_from_locations(
                                &self.app_handle,
                                update.fields.locations.as_deref(),
                            )
                        })
                    })
                    .unwrap_or_else(|| {
                        let display_name = display_server_name(title, "ACP ToolCall");
                        let derived_tool_name = extract_tool_name_from_meta(update.meta.as_ref())
                            .unwrap_or_else(|| "acp_tool".to_string());
                        let derived_params = build_params_from_raw_input(
                            update.fields.raw_input.as_ref(),
                            update.meta.as_ref(),
                        );
                        (display_name, derived_tool_name, derived_params)
                    });

                    let display_name = display_server_name(title, &server_name);
                    updated_server_name = Some(display_name.clone());
                    updated_tool_name = Some(tool_name.clone());
                    updated_parameters = Some(parameters.clone());

                    if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                        let _ = db.update_mcp_tool_call_metadata(
                            call_id,
                            &display_name,
                            &tool_name,
                            &parameters,
                        );
                    }
                }

                let (mut finished_time, result, error) = match &update.fields.status {
                    Some(acp::ToolCallStatus::Completed) => {
                        (Some(chrono::Utc::now()), update.fields.raw_output.as_ref(), None)
                    }
                    Some(acp::ToolCallStatus::Failed) => {
                        (Some(chrono::Utc::now()), None, update.fields.raw_output.as_ref())
                    }
                    _ => (None, None, None),
                };

                let mut result_str = result.map(|r| r.to_string());
                if result_str.is_none() {
                    result_str = meta_result.clone();
                }
                if result_str.is_some() {
                    if finished_time.is_none() {
                        finished_time = Some(chrono::Utc::now());
                    }
                    if status_str == "executing" {
                        status_str = "success".to_string();
                    }
                }
                let mut error_str = error.map(|e| e.to_string());
                if status_str == "success" && result_str.is_none() {
                    result_str = existing_call.as_ref().and_then(|call| call.result.clone());
                }
                if status_str == "failed" && error_str.is_none() {
                    error_str = existing_call.as_ref().and_then(|call| call.error.clone());
                }

                if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                    let _ = db.update_mcp_tool_call_status(
                        call_id,
                        &status_str,
                        result_str.as_deref(),
                        error_str.as_deref(),
                    );
                }

                let status_for_shine = status_str.clone();
                let event = ConversationEvent {
                    r#type: "mcp_tool_call_update".to_string(),
                    data: serde_json::to_value(MCPToolCallUpdateEvent {
                        call_id,
                        conversation_id: self.conversation_id,
                        message_id: None,
                        status: status_str,
                        llm_call_id: None,
                        server_name: updated_server_name,
                        tool_name: updated_tool_name,
                        parameters: updated_parameters,
                        result: result_str,
                        error: error_str,
                        started_time: None,
                        finished_time: finished_time,
                    })
                    .unwrap(),
                };

                self.emit_event(event).await;
                self.sync_tool_shine_status(call_id, &status_for_shine).await;
            }

            // Agent execution plan - log only, no UI support yet
            acp::SessionUpdate::Plan(plan) => {
                info!("ACP Plan: {} entries", plan.entries.len());
                self.apply_plan_update(&plan).await;
            }

            // Available commands update - refresh frontend slash command suggestions
            acp::SessionUpdate::AvailableCommandsUpdate(commands_update) => {
                info!(
                    "ACP AvailableCommandsUpdate: {} commands",
                    commands_update.available_commands.len()
                );
                self.apply_available_commands_update(&commands_update).await;
            }

            // Session mode change - log only, no UI support yet
            acp::SessionUpdate::CurrentModeUpdate(mode_update) => {
                info!("ACP CurrentModeUpdate: mode_id={:?}", mode_update.current_mode_id);
                self.apply_current_mode_update(&mode_update).await;
            }

            // Session info update - keep local conversation metadata in sync
            acp::SessionUpdate::SessionInfoUpdate(info_update) => {
                info!(
                    "ACP SessionInfoUpdate: title={:?}, updated_at={:?}",
                    info_update.title,
                    info_update.updated_at
                );
                self.apply_session_info_update(&info_update).await;
            }

            // Config options update - refresh frontend-visible ACP controls
            acp::SessionUpdate::ConfigOptionUpdate(config_update) => {
                debug!("ACP ConfigOptionUpdate: {:?}", config_update);
                self.apply_config_option_update(&config_update).await;
            }

            acp::SessionUpdate::UsageUpdate(usage_update) => {
                debug!(
                    "ACP UsageUpdate: used={}, size={}, cost={:?}",
                    usage_update.used,
                    usage_update.size,
                    usage_update.cost
                );
                self.apply_usage_update(&usage_update).await;
            }

            // Catch-all for any future variants
            _ => {
                debug!("ACP SessionNotification: unhandled variant");
            }
        }
        Ok(())
    }

    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse, acp::Error> {
        info!("ACP permission request: {:?}", args);

        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let options = args
            .options
            .iter()
            .map(|option| AcpPermissionOptionPayload {
                option_id: option.option_id.0.to_string(),
                name: option.name.clone(),
                kind: permission_option_kind_to_string(&option.kind),
            })
            .collect::<Vec<_>>();

        let parameters = args
            .tool_call
            .fields
            .raw_input
            .as_ref()
            .map(|raw| serde_json::to_string(raw).unwrap_or_else(|_| raw.to_string()));

        let kind = args.tool_call.fields.kind.map(|k| format!("{:?}", k));

        let event = AcpPermissionRequestEvent {
            request_id: request_id.clone(),
            conversation_id: Some(self.conversation_id),
            agent_kind: Some("acp".to_string()),
            tool_call_id: args.tool_call.tool_call_id.0.to_string(),
            title: args.tool_call.fields.title.clone(),
            kind,
            parameters,
            options,
        };

        let state = self.app_handle.state::<AcpPermissionState>();
        state.store_request(event.clone(), tx).await;

        let delivered_to_feishu = match state.get_request(&request_id).await {
            Some(snapshot) => match crate::feishu::try_deliver_acp_permission_to_feishu(
                &self.app_handle,
                self.conversation_id,
                &snapshot,
            )
            .await
            {
                Ok(delivered) => delivered,
                Err(error) => {
                    warn!(
                        conversation_id = self.conversation_id,
                        request_id = %request_id,
                        error = %error,
                        "failed to deliver ACP permission to Feishu"
                    );
                    false
                }
            },
            None => false,
        };

        if let Err(e) = emit_permission_request_event(
            &self.app_handle,
            ACP_PERMISSION_REQUEST_EVENT,
            Some(self.conversation_id),
            &event,
        ) {
            if delivered_to_feishu {
                warn!(
                    conversation_id = self.conversation_id,
                    request_id = %request_id,
                    error = %e,
                    "ACP permission frontend emit failed, but Feishu delivery is active"
                );
            } else {
                state.remove_request(&request_id).await;
                error!(error = %e, "ACP permission request emit failed");
                return Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
            }
        }

        if let Err(error) = crate::api::butler_api::emit_butler_task_permission_state_changed(
            &self.app_handle,
            self.conversation_id,
            "acp",
            true,
        )
        .await
        {
            warn!(
                conversation_id = self.conversation_id,
                error = %error,
                "failed to refresh Butler ACP permission state"
            );
        }

        match rx.await {
            Ok(AcpPermissionDecision::Selected(option_id)) => {
                Ok(acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Selected(
                    acp::SelectedPermissionOutcome::new(acp::PermissionOptionId::new(option_id)),
                )))
            }
            Ok(AcpPermissionDecision::Cancelled) | Err(_) => {
                Ok(acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled))
            }
        }
    }

    /// ACP elicitation（结构化提问）：仅支持 form 模式 + session 作用域。
    /// url 模式与 request 作用域按协议返回 decline，并记录具体原因。
    async fn create_elicitation(
        &self,
        request: acp::CreateElicitationRequest,
    ) -> acp::Result<acp::CreateElicitationResponse> {
        let decline = |reason: String| {
            warn!(
                conversation_id = self.conversation_id,
                reason = %reason,
                "ACP elicitation declined"
            );
            Ok(acp::CreateElicitationResponse::new(acp::ElicitationAction::Decline))
        };

        let form = match &request.mode {
            acp::ElicitationMode::Form(form) => form,
            acp::ElicitationMode::Url(_) => {
                return decline("url-mode elicitation is not advertised/supported by AIPP".into());
            }
            acp::ElicitationMode::Other(other) => {
                return decline(format!("unknown elicitation mode: {}", other.mode));
            }
            _ => {
                return decline("unsupported elicitation mode variant".into());
            }
        };
        match &form.scope {
            acp::ElicitationScope::Session(_) => {}
            acp::ElicitationScope::Request(_) => {
                return decline(
                    "request-scoped elicitation outside a session is not supported".into(),
                );
            }
            _ => {
                return decline("unknown elicitation scope".into());
            }
        }

        let schema_json = match serde_json::to_value(&form.requested_schema) {
            Ok(value) => value,
            Err(error) => {
                error!(
                    conversation_id = self.conversation_id,
                    error = %error,
                    "ACP elicitation schema serialization failed"
                );
                return decline("elicitation schema serialization failed".into());
            }
        };

        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let event = AcpElicitationRequestEvent {
            request_id: request_id.clone(),
            conversation_id: Some(self.conversation_id),
            agent_kind: "acp".to_string(),
            message: request.message.clone(),
            schema: schema_json,
        };

        let state = self.app_handle.state::<AcpElicitationState>();
        state.store_request(request_id.clone(), Some(self.conversation_id), tx).await;

        if let Err(error) = emit_permission_request_event(
            &self.app_handle,
            crate::api::operation_api::ACP_ELICITATION_REQUEST_EVENT,
            Some(self.conversation_id),
            &event,
        ) {
            state.remove_request(&request_id).await;
            error!(error = %error, "ACP elicitation request emit failed");
            return Ok(acp::CreateElicitationResponse::new(acp::ElicitationAction::Cancel));
        }

        match rx.await {
            Ok(AcpElicitationDecision::Accepted(content)) => {
                Ok(acp::CreateElicitationResponse::new(acp::ElicitationAction::Accept(
                    acp::ElicitationAcceptAction::new().content(content),
                )))
            }
            Ok(AcpElicitationDecision::Declined) => {
                Ok(acp::CreateElicitationResponse::new(acp::ElicitationAction::Decline))
            }
            Ok(AcpElicitationDecision::Cancelled) | Err(_) => {
                Ok(acp::CreateElicitationResponse::new(acp::ElicitationAction::Cancel))
            }
        }
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse, acp::Error> {
        info!("ACP write_text_file: path={}", args.path.display());

        let request = WriteFileRequest {
            file_path: args.path.to_string_lossy().to_string(),
            content: args.content,
        };

        match FileOperations::write_file(
            &self.operation_state,
            &self.permission_manager,
            request,
            self.get_conversation_id(),
        )
        .await
        {
            Ok(_) => {
                info!("File written successfully: {}", args.path.display());
                Ok(acp::WriteTextFileResponse::new())
            }
            Err(e) => {
                error!("Failed to write file: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse, acp::Error> {
        info!("ACP read_text_file: path={}", args.path.display());

        let request = ReadFileRequest {
            file_path: args.path.to_string_lossy().to_string(),
            offset: args.line.map(|l| l as usize),
            limit: args.limit.map(|l| l as usize),
        };

        match FileOperations::read_file(
            &self.operation_state,
            &self.permission_manager,
            request,
            self.get_conversation_id(),
        )
        .await
        {
            Ok(response) => {
                info!("File read successfully: {} bytes", response.content.len());
                Ok(acp::ReadTextFileResponse::new(response.content))
            }
            Err(e) => {
                error!("Failed to read file: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn create_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse, acp::Error> {
        info!("ACP create_terminal: command={}", args.command);

        match self.spawn_structured_terminal(args).await {
            Ok(bash_id) => {
                let terminal_id = acp::TerminalId::new(bash_id.clone());
                info!("Terminal created: terminal_id={}, bash_id={}", terminal_id.0, bash_id);
                Ok(acp::CreateTerminalResponse::new(terminal_id))
            }
            Err(e) => {
                error!("Failed to create terminal: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn terminal_output(
        &self,
        args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse, acp::Error> {
        debug!("ACP terminal_output: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        let request = GetBashOutputRequest { bash_id: bash_id.clone(), filter: None };

        match BashOperations::get_bash_output(&self.operation_state, request).await {
            Ok(response) => {
                let exit_status = match response.status {
                    BashProcessStatus::Running => None,
                    BashProcessStatus::Completed | BashProcessStatus::Error => response
                        .exit_code
                        .map(|code| acp::TerminalExitStatus::new().exit_code(Some(code as u32))),
                };

                Ok(acp::TerminalOutputResponse::new(response.output, false)
                    .exit_status(exit_status))
            }
            Err(e) => {
                error!("Failed to get terminal output: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse, acp::Error> {
        info!("ACP release_terminal: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        // Remove the bash process from state (this will kill the process)
        self.operation_state.remove_bash_process(&bash_id).await;
        self.forget_terminal_output_limit(&bash_id).await;

        info!("Terminal released: {}", bash_id);
        Ok(acp::ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse, acp::Error> {
        info!("ACP wait_for_terminal_exit: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        // Wait for the process to complete by polling the state
        loop {
            if !self.operation_state.bash_process_exists(&bash_id).await {
                // Process no longer exists
                break;
            }

            // Check if completed
            let (_output, completed, exit_code) = {
                let processes = self.operation_state.bash_processes.lock().await;
                if let Some(info) = processes.get(&bash_id) {
                    (info.output_buffer.clone(), info.completed, info.exit_code)
                } else {
                    break;
                }
            };

            if completed {
                let exit_status =
                    acp::TerminalExitStatus::new().exit_code(exit_code.map(|c| c as u32));
                info!("Terminal exited: terminal_id={}, exit_code={:?}", bash_id, exit_code);
                return Ok(acp::WaitForTerminalExitResponse::new(exit_status));
            }

            // Wait a bit before checking again
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // If we get here, the process was removed without proper completion
        let exit_status = acp::TerminalExitStatus::new();
        Ok(acp::WaitForTerminalExitResponse::new(exit_status))
    }

    async fn kill_terminal(
        &self,
        args: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse, acp::Error> {
        info!("ACP kill_terminal: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        // Remove the process which will kill it
        self.operation_state.remove_bash_process(&bash_id).await;
        self.forget_terminal_output_limit(&bash_id).await;

        info!("Terminal command killed: {}", bash_id);
        Ok(acp::KillTerminalResponse::new())
    }

    /// 在连接任务里执行 agent→client 请求处理，避免阻塞 dispatch loop。
    ///
    /// v2 SDK 的 `on_receive_request` 回调在 dispatch loop 上 inline await，
    /// 像权限审批、等待终端退出这类可能长时间挂起的处理必须移到 `cx.spawn`
    /// 任务里，handler 本体立即返回。
    fn spawn_request_task<Req, Resp, F, Fut>(
        &self,
        cx: &agent_client_protocol::ConnectionTo<agent_client_protocol::Agent>,
        request: Req,
        responder: agent_client_protocol::Responder<Resp>,
        handler: F,
    ) -> acp::Result<()>
    where
        Req: Send + 'static,
        Resp: agent_client_protocol::JsonRpcResponse,
        F: FnOnce(AcpTauriClient, Req) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = acp::Result<Resp>> + Send + 'static,
    {
        let client = self.clone();
        cx.spawn(async move {
            match handler(client, request).await {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            }
        })
    }
}

/// 注册一个 agent→client 请求处理器，实际处理通过 `AcpTauriClient::spawn_request_task`
/// 移到 `cx.spawn` 任务里执行（见该方法注释）。
macro_rules! register_acp_request_handler {
    ($builder:expr, $client:expr, $req_ty:ty, $method:ident) => {{
        let builder = $builder;
        let client = $client.clone();
        builder.on_receive_request(
            async move |request: $req_ty, responder, cx| {
                client.spawn_request_task(&cx, request, responder, |client, request| async move {
                    client.$method(request).await
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
    }};
}

/// Execute an ACP session
pub fn spawn_acp_session_task(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    acp_config: AcpConfig,
) -> AcpSessionHandle {
    let (sender, receiver) = mpsc::unbounded_channel();
    let run_id = uuid::Uuid::new_v4().to_string();
    let handle = AcpSessionHandle { sender, run_id: run_id.clone() };

    let cleanup_handle = app_handle.clone();
    // agent-client-protocol v2 SDK 不再依赖 tokio，连接 future 全是 Send，
    // 不再需要手工 current_thread runtime + LocalSet
    tokio::spawn(async move {
        let result = run_acp_session(app_handle, conversation_id, acp_config, receiver).await;

        if let Some(permission_state) = cleanup_handle.try_state::<AcpPermissionState>() {
            let resolutions = permission_state.cancel_requests_for_conversation(conversation_id).await;
            notify_cancelled_acp_permission_requests(&cleanup_handle, resolutions).await;
        }
        if let Some(elicitation_state) = cleanup_handle.try_state::<AcpElicitationState>() {
            let resolutions =
                elicitation_state.cancel_requests_for_conversation(conversation_id).await;
            notify_cancelled_acp_elicitation_requests(&cleanup_handle, resolutions).await;
        }

        let removed_current_entry = {
            let session_state = cleanup_handle.state::<crate::AcpSessionState>();
            let mut sessions = session_state.sessions.lock().await;
            if sessions
                .get(&conversation_id)
                .is_some_and(|entry| entry.run_id == run_id)
            {
                sessions.remove(&conversation_id);
                true
            } else {
                false
            }
        };
        if removed_current_entry {
            emit_acp_session_state_snapshot(&cleanup_handle, conversation_id, None);
        }

        if let Err(error) = result {
            error!("ACP session task failed: {}", error);
        }
    });

    handle
}

async fn process_acp_prompt(
    client_handle: &AcpTauriClient,
    cx: &agent_client_protocol::ConnectionTo<agent_client_protocol::Agent>,
    session_id: &str,
    conversation_id: i64,
    message_id: i64,
    prompt: String,
    attachments: Vec<MessageAttachment>,
    prompt_capabilities: acp::PromptCapabilities,
    window: tauri::Window,
    history_prefix: Option<String>,
) -> Result<(), AppError> {
    client_handle.set_current_message(message_id).await;
    client_handle.set_window(window).await;
    client_handle.reset_buffers().await;

    let prompt_blocks =
        build_acp_prompt_blocks(prompt, &attachments, &prompt_capabilities, history_prefix)?;

    info!("ACP: Sending prompt (conversation_id={}, message_id={})", conversation_id, message_id);
    let prompt_response = cx
        .send_request(acp::PromptRequest::new(session_id.to_string(), prompt_blocks.clone()))
        .block_task()
        .await;

    if let Err(e) = &prompt_response {
        let err_msg = format!("ACP prompt failed: {:?}", e);
        error!("ACP: {}", err_msg);
        client_handle
            .finish_unfinished_tool_calls("failed", Some(&err_msg))
            .await;
        client_handle.send_error_event(&err_msg).await;
        return Err(AppError::UnknownError(err_msg));
    }
    info!("ACP: Prompt completed successfully");

    let prompt_response = prompt_response.expect("checked above");
    let final_content = client_handle.get_response_content().await;
    let reasoning_content = client_handle.get_reasoning_content().await;
    let usage_summary = summarize_acp_prompt_usage(
        &prompt_response,
        &prompt_blocks,
        &final_content,
        &reasoning_content,
    );
    client_handle
        .persist_message_usage_in_db(&usage_summary, &final_content)
        .await?;
    let prompt_succeeded = !matches!(
        prompt_response.stop_reason,
        acp::StopReason::Cancelled | acp::StopReason::Refusal
    );
    match prompt_response.stop_reason {
        acp::StopReason::Cancelled => {
            client_handle
                .finish_unfinished_tool_calls(
                    "failed",
                    Some("ACP prompt was cancelled before the agent sent a final tool status"),
                )
                .await;
        }
        acp::StopReason::Refusal => {
            client_handle
                .finish_unfinished_tool_calls(
                    "failed",
                    Some("ACP prompt was refused before the agent sent a final tool status"),
                )
                .await;
        }
        _ => {
            client_handle.finish_unfinished_tool_calls("success", None).await;
        }
    }
    client_handle
        .send_done_event("response", &final_content, Some(&usage_summary))
        .await;
    if prompt_succeeded {
        let completion_window = client_handle.window.lock().await.clone();
        handle_agent_success(
            &client_handle.app_handle,
            &completion_window,
            conversation_id,
            &final_content,
            AgentKind::Acp,
        )
        .await;
    }
    let (assistant_id, model_id, model_code, user_message_id) = ConversationDatabase::new(&client_handle.app_handle)
        .ok()
        .and_then(|db| {
            let message = db.message_repo().ok()?.read(message_id).ok().flatten()?;
            let conversation = db.conversation_repo().ok()?.read(conversation_id).ok().flatten()?;
            Some((
                conversation.assistant_id,
                message.llm_model_id,
                message.llm_model_name,
                message.parent_id,
            ))
        })
        .unwrap_or((None, Some(0), Some("acp".to_string()), None));
    let _ = PluginHookBus::new(client_handle.app_handle.clone())
        .emit_event(
            "chat.afterResponseCompleted",
            serde_json::json!({
                "conversationId": conversation_id,
                "userMessageId": user_message_id,
                "assistantMessageId": message_id,
                "assistantId": assistant_id,
                "modelId": model_id,
                "modelCode": model_code,
                "metadata": { "acp": true }
            }),
        )
        .await;
    Ok(())
}

async fn run_acp_session(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    acp_config: AcpConfig,
    mut receiver: mpsc::UnboundedReceiver<AcpSessionCommand>,
) -> Result<(), AppError> {
    info!("ACP session task started: conversation_id={}", conversation_id);

    let mut startup_responses: Vec<oneshot::Sender<Result<(), String>>> = Vec::new();
    let mut startup_window: Option<tauri::Window> = None;
    let mut first_prompt: Option<(i64, String, Vec<MessageAttachment>, tauri::Window)> = None;

    loop {
        match receiver.recv().await {
            Some(AcpSessionCommand::Start { window, response }) => {
                startup_window = Some(window);
                startup_responses.push(response);
                break;
            }
            Some(AcpSessionCommand::Prompt { message_id, prompt, attachments, window }) => {
                first_prompt = Some((message_id, prompt, attachments, window));
                break;
            }
            Some(AcpSessionCommand::CancelCurrentPrompt { response }) => {
                let _ = response.send(Ok(()));
            }
            Some(AcpSessionCommand::SetConfigOption { response, .. }) => {
                let _ = response.send(Err("ACP session is not ready yet".to_string()));
            }
            None => {
                info!("ACP session task ended before start: conversation_id={}", conversation_id);
                return Ok(());
            }
        }
    }

    let initial_message_id = first_prompt.as_ref().map(|(message_id, _, _, _)| *message_id).unwrap_or(0);
    let initial_window = first_prompt
        .as_ref()
        .map(|(_, _, _, window)| window.clone())
        .or(startup_window)
        .ok_or_else(|| AppError::InternalError("ACP session has no startup window".to_string()))?;

    let send_startup_error = |msg: &str| {
        if let Some((message_id, _, _, window)) = first_prompt.as_ref() {
            if let Ok(db) = ConversationDatabase::new(&app_handle) {
                if let Ok(repo) = db.message_repo() {
                    let _ = repo.update_content(*message_id, msg);
                }
            }
            let event = ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(MessageUpdateEvent {
                    message_id: *message_id,
                    message_type: "error".to_string(),
                    content: msg.to_string(),
                    is_done: true,
                    token_count: None,
                    input_token_count: None,
                    output_token_count: None,
                    ttft_ms: None,
                    tps: None,
                })
                .unwrap(),
            };
            let event_name = format!("conversation_event_{}", conversation_id);
            let _ = window.emit(&event_name, event);
        }
    };

    macro_rules! fail_startup {
        ($err_msg:expr) => {{
            let err_msg = $err_msg;
            send_startup_error(&err_msg);
            for response in startup_responses.drain(..) {
                let _ = response.send(Err(err_msg.clone()));
            }
            return Err(AppError::UnknownError(err_msg));
        }};
    }

    let operation_state = Arc::new(
        app_handle
            .try_state::<OperationState>()
            .map(|state| state.inner().clone())
            .ok_or_else(|| AppError::InternalError("OperationState not found".to_string()))?,
    );
    let permission_manager = Arc::new(PermissionManager::new(app_handle.clone()));

    let resolved_cli_command = resolve_acp_cli_path(&acp_config.cli_command);
    info!("ACP: Original CLI command: {}", acp_config.cli_command);
    info!("ACP: Resolved CLI path: {}", resolved_cli_command.display());

    let launch_plan = build_acp_launch_plan(
        &acp_config.cli_command,
        &resolved_cli_command,
        &acp_config.additional_args,
        &acp_config.env_vars,
    );
    let full_command = if launch_plan.args.is_empty() {
        launch_plan.program.display().to_string()
    } else {
        format!("{} {}", launch_plan.program.display(), launch_plan.args.join(" "))
    };
    info!("ACP: Full command: {} (proxy_strategy={})", full_command, launch_plan.proxy_strategy);
    info!("ACP: Working directory: {}", acp_config.working_directory.display());

    if !acp_config.working_directory.exists() {
        let err_msg = format!(
            "ACP working directory does not exist: {}",
            acp_config.working_directory.display()
        );
        error!("ACP: {}", err_msg);
        fail_startup!(err_msg);
    }
    if !acp_config.working_directory.is_dir() {
        let err_msg = format!(
            "ACP working directory is not a directory: {}",
            acp_config.working_directory.display()
        );
        error!("ACP: {}", err_msg);
        fail_startup!(err_msg);
    }

    let trusted_working_directory = acp_config
        .working_directory
        .canonicalize()
        .unwrap_or_else(|_| acp_config.working_directory.clone());
    operation_state
        .add_conversation_trusted_path(
            conversation_id,
            trusted_working_directory.to_string_lossy().to_string(),
        )
        .await;

    let mut cmd = Command::new(&launch_plan.program);
    cmd.current_dir(&acp_config.working_directory)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    for (key, value) in &acp_config.env_vars {
        cmd.env(key, value);
        debug!(
            "ACP: Set env var: {}={}",
            key,
            if key.to_lowercase().contains("key") || key.to_lowercase().contains("token") {
                "***"
            } else {
                value
            }
        );
    }
    if !acp_config.env_vars.is_empty() {
        info!("ACP: Environment variables set: {}", acp_config.env_vars.len());
    }

    for (key, value) in &launch_plan.extra_env {
        cmd.env(key, value);
        debug!("ACP: Set runtime env var: {}={}", key, value);
    }
    if !launch_plan.extra_env.is_empty() {
        info!("ACP: Runtime environment variables set: {}", launch_plan.extra_env.len());
    }

    if !launch_plan.args.is_empty() {
        cmd.args(&launch_plan.args);
        info!("ACP: Effective args: {:?}", launch_plan.args);
    }

    info!("ACP: Spawning process...");
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let help_msg = match acp_config.cli_command.as_str() {
                "claude-code-acp" => "\n\n安装方法: bun add -g @zed-industries/claude-code-acp\n注意: 需要设置 ANTHROPIC_API_KEY 环境变量",
                "gemini" => "\n\n安装方法: 请参考 Google Gemini CLI 官方文档",
                "kimi" => "\n\n安装方法: 请参考 Kimi Code CLI 官方文档 (https://github.com/MoonshotAI/kimi-code)\n注意: 安装后需先运行 kimi 并 /login 完成登录",
                "dsh-acp-server" => "\n\n安装方法: npm install -g @deepseek-ai/dsh dsh-acp-server\n注意: 需要 Node.js >= 22，模型与密钥在 dsh 中配置",
                _ => "",
            };
            let err_msg = format!(
                "无法启动 ACP 进程 '{}' (resolved: {}, effective: {}, cwd: {}): {}{}",
                acp_config.cli_command,
                resolved_cli_command.display(),
                full_command,
                acp_config.working_directory.display(),
                e,
                help_msg
            );
            error!("ACP: {}", err_msg);
            fail_startup!(err_msg);
        }
    };
    info!("ACP: Process spawned successfully, PID={:?}", child.id());

    let stdin = match child.stdin.take() {
        Some(value) => value,
        None => {
            let err_msg = "Failed to open stdin for ACP process".to_string();
            fail_startup!(err_msg);
        }
    };
    let stdout = match child.stdout.take() {
        Some(value) => value,
        None => {
            let err_msg = "Failed to open stdout for ACP process".to_string();
            fail_startup!(err_msg);
        }
    };
    let stderr = match child.stderr.take() {
        Some(value) => value,
        None => {
            let err_msg = "Failed to open stderr for ACP process".to_string();
            fail_startup!(err_msg);
        }
    };
    let _ = send_startup_error;

    let client_impl = AcpTauriClient::new(
        app_handle.clone(),
        conversation_id,
        initial_message_id,
        initial_window.clone(),
        operation_state,
        permission_manager,
    );
    let client_handle = client_impl.clone();
    let has_initial_prompt = first_prompt.is_some();

    let _stderr_task = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut stderr_reader = BufReader::new(stderr).lines();
        loop {
            match stderr_reader.next_line().await {
                Ok(Some(line)) => info!("[ACP stderr] {}", line),
                Ok(None) => {
                    info!("[ACP stderr] Stream closed (EOF)");
                    break;
                }
                Err(e) => {
                    error!("[ACP stderr] Read error: {}", e);
                    break;
                }
            }
        }
    });
    info!("ACP: Stderr reader spawned");

    let notification_client = client_impl.clone();
    let builder = agent_client_protocol::Client.builder()
        .name("AIPP")
        .on_receive_notification(
            async move |notification: acp::SessionNotification, _cx| {
                notification_client.session_notification(notification).await
            },
            agent_client_protocol::on_receive_notification!(),
        );
    let builder = register_acp_request_handler!(builder, client_impl, acp::RequestPermissionRequest, request_permission);
    let builder = register_acp_request_handler!(builder, client_impl, acp::WriteTextFileRequest, write_text_file);
    let builder = register_acp_request_handler!(builder, client_impl, acp::ReadTextFileRequest, read_text_file);
    let builder = register_acp_request_handler!(builder, client_impl, acp::CreateTerminalRequest, create_terminal);
    let builder = register_acp_request_handler!(builder, client_impl, acp::TerminalOutputRequest, terminal_output);
    let builder = register_acp_request_handler!(builder, client_impl, acp::ReleaseTerminalRequest, release_terminal);
    let builder = register_acp_request_handler!(builder, client_impl, acp::WaitForTerminalExitRequest, wait_for_terminal_exit);
    let builder = register_acp_request_handler!(builder, client_impl, acp::KillTerminalRequest, kill_terminal);
    let builder = register_acp_request_handler!(builder, client_impl, acp::CreateElicitationRequest, create_elicitation);

    info!("ACP: Connecting client session...");
    let startup = AcpSessionStartup { startup_responses, first_prompt, has_initial_prompt };
    let session_result = builder
        .connect_with(
            agent_client_protocol::ByteStreams::new(stdin.compat_write(), stdout.compat()),
            async move |cx| {
                run_acp_connected_session(
                    cx,
                    app_handle,
                    conversation_id,
                    acp_config,
                    client_handle,
                    receiver,
                    startup,
                )
                .await
                .map_err(|error| {
                    acp::Error::internal_error().data(serde_json::Value::String(error.to_string()))
                })
            },
        )
        .await;

    if let Err(error) = &session_result {
        error!("ACP: Session connection failed: {:?}", error);
    }

    info!("ACP: Session ended, cleaning up process");
    if let Err(error) = child.kill().await {
        debug!("ACP: Kill process result: {:?}", error);
    }

    session_result
        .map_err(|error| AppError::UnknownError(format!("ACP session connection error: {error:?}")))
}

/// ACP 会话启动上下文（连接建立前从命令通道收集的信息）
struct AcpSessionStartup {
    startup_responses: Vec<oneshot::Sender<Result<(), String>>>,
    first_prompt: Option<(i64, String, Vec<MessageAttachment>, tauri::Window)>,
    has_initial_prompt: bool,
}

/// 连接建立后的 ACP 会话主流程（v2 SDK 的 main_fn）。
///
/// 注意：`cx` 只能在这里（dispatch loop 之外）和 `cx.spawn` 任务里使用；
/// `on_receive_*` handler 回调内禁止 `block_task`，长阻塞处理一律走
/// `AcpTauriClient::spawn_request_task`。
async fn run_acp_connected_session(
    cx: agent_client_protocol::ConnectionTo<agent_client_protocol::Agent>,
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    acp_config: AcpConfig,
    client_handle: AcpTauriClient,
    mut receiver: mpsc::UnboundedReceiver<AcpSessionCommand>,
    startup: AcpSessionStartup,
) -> Result<(), AppError> {
    let AcpSessionStartup { mut startup_responses, first_prompt, has_initial_prompt } = startup;

    macro_rules! fail_connected_startup {
        ($err_msg:expr) => {{
            let err_msg = $err_msg;
            if has_initial_prompt {
                client_handle.send_error_event(&err_msg).await;
            }
            for response in startup_responses.drain(..) {
                let _ = response.send(Err(err_msg.clone()));
            }
            return Err(AppError::UnknownError(err_msg));
        }};
    }

    info!("ACP: Initializing connection (timeout: 30s)...");
    let init_response = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        cx.send_request(
            acp::InitializeRequest::new(ProtocolVersion::V1)
                .client_info(acp::Implementation::new("AIPP", "0.4.3"))
                .client_capabilities(
                    acp::ClientCapabilities::new()
                        .fs(
                            acp::FileSystemCapabilities::new()
                                .read_text_file(true)
                                .write_text_file(true),
                        )
                        .terminal(true)
                        .elicitation(
                            acp::ElicitationCapabilities::new()
                                .form(acp::ElicitationFormCapabilities::new()),
                        ),
                ),
        )
        .block_task(),
    )
    .await;

    let init_response = match init_response {
        Ok(result) => result,
        Err(_) => {
            let err_msg = "ACP initialize timed out after 30 seconds. The CLI might not support ACP protocol or needs '--mcp' flag.".to_string();
            error!("ACP: {}", err_msg);
            fail_connected_startup!(err_msg);
        }
    };

    let init_response = match init_response {
        Ok(response) => response,
        Err(error) => {
            let err_msg = format!("ACP initialize failed: {error:?}");
            error!("ACP: {}", err_msg);
            fail_connected_startup!(err_msg);
        }
    };
    info!(
        "ACP: Initialize success, protocol_version={:?}",
        init_response.protocol_version
    );
    info!(
        "ACP: Agent capabilities load_session={}, session_resume={}, session_close={}, session_delete={}",
        init_response.agent_capabilities.load_session,
        init_response
            .agent_capabilities
            .session_capabilities
            .resume
            .is_some(),
        init_response
            .agent_capabilities
            .session_capabilities
            .close
            .is_some(),
        init_response
            .agent_capabilities
            .session_capabilities
            .delete
            .is_some()
    );
    let agent_prompt_capabilities =
        init_response.agent_capabilities.prompt_capabilities.clone();
    let session_resume_supported = init_response
        .agent_capabilities
        .session_capabilities
        .resume
        .is_some();
    let session_close_supported = init_response
        .agent_capabilities
        .session_capabilities
        .close
        .is_some();
    let acp_mcp_servers = match build_acp_manual_mcp_servers(
        &app_handle,
        conversation_id,
        &acp_config.selected_mcp_tools_payload,
    )
    .await
    {
        Ok(servers) => servers,
        Err(error) => {
            let err_msg = format!("ACP MCP tool bridge injection failed: {error}");
            error!("ACP: {}", err_msg);
            fail_connected_startup!(err_msg);
        }
    };
    info!(
        "ACP: Injecting {} MCP server(s) via session mcp_servers",
        acp_mcp_servers.len()
    );

    let conversation_db = match ConversationDatabase::new(&app_handle) {
        Ok(db) => db,
        Err(error) => {
            let err_msg = format!("ACP failed to open conversation database: {error}");
            error!("ACP: {}", err_msg);
            fail_connected_startup!(err_msg);
        }
    };
    let mut session_id: Option<String> = None;
    let mut restored_session_method: Option<&'static str> = None;
    let mut should_build_history_fallback = false;
    let mut initial_config_options: Option<Vec<acp::SessionConfigOption>> = None;

    let stored_session_id = match conversation_db.get_acp_session_id(conversation_id) {
        Ok(value) => value,
        Err(error) => {
            let err_msg = format!("ACP failed to read stored session id: {error}");
            error!("ACP: {}", err_msg);
            fail_connected_startup!(err_msg);
        }
    };

    if let Some(stored_session_id) = stored_session_id {
        if session_resume_supported {
            info!(
                "ACP: Resuming existing session (conversation_id={}, session_id={})",
                conversation_id, stored_session_id
            );
            client_handle.set_suppress_updates(true).await;
            let resume_result = cx
                .send_request(
                    acp::ResumeSessionRequest::new(
                        stored_session_id.clone(),
                        acp_config.working_directory.clone(),
                    )
                    .mcp_servers(acp_mcp_servers.clone()),
                )
                .block_task()
                .await;
            client_handle.set_suppress_updates(false).await;

            match resume_result {
                Ok(response) => {
                    if response.config_options.is_some() {
                        initial_config_options = response.config_options.clone();
                    }
                    session_id = Some(stored_session_id.clone());
                    restored_session_method = Some("resume");
                    if let Err(error) = conversation_db.upsert_acp_session_id(
                        conversation_id,
                        session_id.as_deref().unwrap_or_default(),
                    ) {
                        let err_msg = format!("ACP failed to persist session id: {error}");
                        error!("ACP: {}", err_msg);
                        fail_connected_startup!(err_msg);
                    }
                    info!(
                        "ACP: session/resume succeeded (conversation_id={}, session_id={})",
                        conversation_id,
                        session_id.as_deref().unwrap_or_default()
                    );
                }
                Err(error) => {
                    error!("ACP: session/resume failed: {:?}", error);
                }
            }
        }

        if session_id.is_none() && init_response.agent_capabilities.load_session {
            info!(
                "ACP: Loading existing session (conversation_id={}, session_id={})",
                conversation_id, stored_session_id
            );
            client_handle.set_suppress_updates(true).await;
            let load_result = cx
                .send_request(
                    acp::LoadSessionRequest::new(
                        stored_session_id.clone(),
                        acp_config.working_directory.clone(),
                    )
                    .mcp_servers(acp_mcp_servers.clone()),
                )
                .block_task()
                .await;
            client_handle.set_suppress_updates(false).await;

            match load_result {
                Ok(response) => {
                    if response.config_options.is_some() {
                        initial_config_options = response.config_options.clone();
                    }
                    session_id = Some(stored_session_id);
                    restored_session_method = Some("load");
                    if let Err(error) = conversation_db.upsert_acp_session_id(
                        conversation_id,
                        session_id.as_deref().unwrap_or_default(),
                    ) {
                        let err_msg = format!("ACP failed to persist session id: {error}");
                        error!("ACP: {}", err_msg);
                        fail_connected_startup!(err_msg);
                    }
                    info!(
                        "ACP: session/load succeeded (conversation_id={}, session_id={})",
                        conversation_id,
                        session_id.as_deref().unwrap_or_default()
                    );
                }
                Err(error) => {
                    error!("ACP: session/load failed: {:?}", error);
                    should_build_history_fallback = true;
                }
            }
        }

        if session_id.is_none()
            && !session_resume_supported
            && !init_response.agent_capabilities.load_session
        {
            info!(
                "ACP: Agent does not support loadSession or session/resume; creating new session (conversation_id={})",
                conversation_id
            );
            should_build_history_fallback = true;
        } else if session_id.is_none() {
            should_build_history_fallback = true;
        }
    } else {
        info!(
            "ACP: No stored session_id found for conversation_id={}",
            conversation_id
        );
    }

    let session_id = if let Some(session_id) = session_id {
        session_id
    } else {
        info!("ACP: Creating session...");
        let session_response = cx
            .send_request(
                acp::NewSessionRequest::new(acp_config.working_directory.clone())
                    .mcp_servers(acp_mcp_servers.clone()),
            )
            .block_task()
            .await;

        let session_response = match session_response {
            Ok(response) => response,
            Err(error) => {
                let err_msg = format!("ACP new_session failed: {error:?}");
                error!("ACP: {}", err_msg);
                fail_connected_startup!(err_msg);
            }
        };
        if session_response.config_options.is_some() {
            initial_config_options = session_response.config_options.clone();
        }
        let session_id = session_response.session_id.to_string();
        info!("ACP: Session created, session_id={:?}", session_id);

        if let Err(error) = conversation_db.upsert_acp_session_id(conversation_id, &session_id) {
            let err_msg = format!("ACP failed to persist session id: {error}");
            error!("ACP: {}", err_msg);
            fail_connected_startup!(err_msg);
        }
        session_id
    };

    client_handle
        .set_session_bootstrap(
            &session_id,
            init_response.agent_capabilities.load_session,
            session_resume_supported,
            restored_session_method,
            &agent_prompt_capabilities,
            initial_config_options.as_deref(),
        )
        .await;

    if let Some(model_id) =
        get_claude_model_override(&acp_config.cli_command, &acp_config.env_vars)
    {
        if let Some(model_config) = initial_config_options.as_deref().and_then(|options| {
            options.iter().find(|option| {
                matches!(
                    option.category.as_ref(),
                    Some(acp::SessionConfigOptionCategory::Model)
                )
            })
        }) {
            info!(
                "ACP: Applying Claude model override through config option {}={}",
                model_config.id, model_id
            );
            let set_model_result = cx
                .send_request(acp::SetSessionConfigOptionRequest::new(
                    session_id.clone(),
                    model_config.id.clone(),
                    model_id.as_str(),
                ))
                .block_task()
                .await;

            let response_payload = match set_model_result {
                Ok(response_payload) => response_payload,
                Err(error) => {
                    let err_msg = format!(
                        "ACP set_session_config_option failed for Claude model '{}': {:?}",
                        model_id, error
                    );
                    error!("ACP: {}", err_msg);
                    fail_connected_startup!(err_msg);
                }
            };

            let state = client_handle
                .update_session_state(|snapshot| {
                    apply_config_options_to_snapshot(
                        snapshot,
                        Some(&response_payload.config_options),
                    );
                })
                .await;
            client_handle.publish_session_state(state).await;
            info!("ACP: Claude model override applied: {}", model_id);
        } else {
            warn!(
                "ACP Claude model override ignored because agent did not expose a model config option"
            );
        }
    }

    for response in startup_responses.drain(..) {
        let _ = response.send(Ok(()));
    }

    let history_fallback_prompt = if should_build_history_fallback {
        build_acp_history_prompt(&app_handle, conversation_id)
    } else {
        None
    };

    let mut prompt_queue = if let Some((message_id, prompt, attachments, window)) = first_prompt {
        VecDeque::from([(message_id, prompt, attachments, window, history_fallback_prompt)])
    } else {
        VecDeque::new()
    };
    let mut active_prompt: Option<oneshot::Receiver<Result<(), AppError>>> = None;
    let mut agent_closed_connection = false;

    loop {
        if active_prompt.is_none() {
            if let Some((message_id, prompt, attachments, window, history_prefix)) = prompt_queue.pop_front()
            {
                client_handle.set_active_prompt(true).await;
                let cx_prompt = cx.clone();
                let client_handle_clone = client_handle.clone();
                let session_id_clone = session_id.clone();
                let prompt_capabilities = agent_prompt_capabilities.clone();
                let (done_tx, done_rx) = oneshot::channel();
                cx.spawn(async move {
                    let result = process_acp_prompt(
                        &client_handle_clone,
                        &cx_prompt,
                        &session_id_clone,
                        conversation_id,
                        message_id,
                        prompt,
                        attachments,
                        prompt_capabilities,
                        window,
                        history_prefix,
                    )
                    .await;
                    let _ = done_tx.send(result);
                    Ok(())
                })
                .map_err(|error| {
                    AppError::UnknownError(format!("ACP failed to spawn prompt task: {error:?}"))
                })?;
                active_prompt = Some(done_rx);
                continue;
            }

            if receiver.is_closed() {
                break;
            }
        }

        if let Some(prompt_done) = active_prompt.as_mut() {
            tokio::select! {
                maybe_command = receiver.recv() => {
                    match maybe_command {
                        Some(AcpSessionCommand::Prompt { message_id, prompt, attachments, window }) => {
                            prompt_queue.push_back((message_id, prompt, attachments, window, None));
                        }
                        Some(AcpSessionCommand::Start { response, .. }) => {
                            let _ = response.send(Ok(()));
                        }
                        Some(AcpSessionCommand::CancelCurrentPrompt { response }) => {
                            prompt_queue.clear();
                            if let Some(permission_state) = app_handle.try_state::<AcpPermissionState>() {
                                let resolutions =
                                    permission_state.cancel_requests_for_conversation(conversation_id).await;
                                notify_cancelled_acp_permission_requests(&app_handle, resolutions).await;
                            }
                            if let Some(elicitation_state) = app_handle.try_state::<AcpElicitationState>() {
                                let resolutions =
                                    elicitation_state.cancel_requests_for_conversation(conversation_id).await;
                                notify_cancelled_acp_elicitation_requests(&app_handle, resolutions).await;
                            }
                            let result = cx
                                .send_notification(acp::CancelNotification::new(session_id.clone()))
                                .map_err(|error| format!("ACP cancel notification send failed: {error:?}"));
                            let _ = response.send(result);
                        }
                        Some(AcpSessionCommand::SetConfigOption { config_id, value, response }) => {
                            let result = cx
                                .send_request(acp::SetSessionConfigOptionRequest::new(
                                    session_id.clone(),
                                    config_id.clone(),
                                    value.as_str(),
                                ))
                                .block_task()
                                .await
                                .map_err(|error| format!("ACP set_session_config_option failed: {error:?}"));
                            match result {
                                Ok(response_payload) => {
                                    let state = client_handle
                                        .update_session_state(|snapshot| {
                                            apply_config_options_to_snapshot(
                                                snapshot,
                                                Some(&response_payload.config_options),
                                            );
                                        })
                                        .await;
                                    client_handle.publish_session_state(state).await;
                                    let _ = response.send(Ok(()));
                                }
                                Err(error) => {
                                    let _ = response.send(Err(error));
                                }
                            }
                        }
                        None => {}
                    }
                }
                prompt_result = prompt_done => {
                    client_handle.set_active_prompt(false).await;
                    active_prompt = None;
                    match prompt_result {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => return Err(error),
                        Err(_) => {
                            return Err(AppError::UnknownError(
                                "ACP prompt task terminated unexpectedly".to_string(),
                            ));
                        }
                    }
                }
                _ = cx.incoming_closed() => {
                    let err_msg = "ACP agent closed the connection unexpectedly during an active prompt (process exited or crashed)".to_string();
                    error!("ACP: {}", err_msg);
                    client_handle
                        .finish_unfinished_tool_calls("failed", Some(&err_msg))
                        .await;
                    client_handle.send_error_event(&err_msg).await;
                    return Err(AppError::UnknownError(err_msg));
                }
            }
        } else {
            tokio::select! {
                maybe_command = receiver.recv() => {
                    match maybe_command {
                        Some(AcpSessionCommand::Prompt { message_id, prompt, attachments, window }) => {
                            prompt_queue.push_back((message_id, prompt, attachments, window, None));
                        }
                        Some(AcpSessionCommand::Start { response, .. }) => {
                            let _ = response.send(Ok(()));
                        }
                        Some(AcpSessionCommand::CancelCurrentPrompt { response }) => {
                            let _ = response.send(Ok(()));
                        }
                        Some(AcpSessionCommand::SetConfigOption { config_id, value, response }) => {
                            let result = cx
                                .send_request(acp::SetSessionConfigOptionRequest::new(
                                    session_id.clone(),
                                    config_id.clone(),
                                    value.as_str(),
                                ))
                                .block_task()
                                .await
                                .map_err(|error| format!("ACP set_session_config_option failed: {error:?}"));
                            match result {
                                Ok(response_payload) => {
                                    let state = client_handle
                                        .update_session_state(|snapshot| {
                                            apply_config_options_to_snapshot(
                                                snapshot,
                                                Some(&response_payload.config_options),
                                            );
                                        })
                                        .await;
                                    client_handle.publish_session_state(state).await;
                                    let _ = response.send(Ok(()));
                                }
                                Err(error) => {
                                    let _ = response.send(Err(error));
                                }
                            }
                        }
                        None => break,
                    }
                }
                _ = cx.incoming_closed() => {
                    info!("ACP: agent closed the connection while idle");
                    agent_closed_connection = true;
                    break;
                }
            }
        }
    }

    // 优雅关闭：agent 声明支持 session/close 且连接仍存活时，
    // 在会话任务退出（空闲释放/对话切换等）前通知 agent 清理会话状态。
    // 失败不阻断退出，进程仍由外层统一 kill。
    if agent_closed_connection {
        info!("ACP: skip session/close because the agent already closed the connection");
    } else if session_close_supported {
        info!(
            "ACP: sending session/close before shutdown (conversation_id={}, session_id={})",
            conversation_id, session_id
        );
        let close_result = tokio::time::timeout(
            Duration::from_secs(5),
            cx.send_request(acp::CloseSessionRequest::new(session_id.clone())).block_task(),
        )
        .await;
        match close_result {
            Ok(Ok(_)) => {
                info!("ACP: session/close succeeded (session_id={})", session_id);
            }
            Ok(Err(error)) => {
                warn!("ACP: session/close failed: {error:?}");
            }
            Err(_) => {
                warn!("ACP: session/close timed out after 5 seconds");
            }
        }
    }

    Ok(())
}

/// 删除对话时调度 agent 会话清理：
/// 1. 结束 ACP、Codex、Claude Code 的本地运行实例
/// 2. ACP 按能力调用 session/delete
/// 3. 删除全部 agent_kind 的本地 session 映射
///
/// 任何一步失败只记录日志，不影响本地对话删除。
pub fn schedule_acp_session_delete(app_handle: &tauri::AppHandle, conversation_id: i64) {
    let stored_session_id = match ConversationDatabase::new(app_handle) {
        Ok(db) => match db.get_acp_session_id(conversation_id) {
            Ok(value) => value,
            Err(error) => {
                warn!(
                    conversation_id,
                    error = %error,
                    "ACP: session delete skipped, failed to read stored session id"
                );
                None
            }
        },
        Err(error) => {
            warn!(
                conversation_id,
                error = %error,
                "ACP: session delete skipped, failed to open conversation database"
            );
            None
        }
    };
    let acp_delete_config = stored_session_id.as_ref().and_then(|_| {
        match prepare_acp_session_delete_config(app_handle, conversation_id) {
            Ok(config) => Some(config),
            Err(error) => {
                warn!(
                    conversation_id,
                    error = %error,
                    "ACP: failed to snapshot session delete config before conversation deletion"
                );
                None
            }
        }
    });

    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        {
            let session_state = app_handle.state::<crate::AcpSessionState>();
            let mut sessions = session_state.sessions.lock().await;
            if sessions.remove(&conversation_id).is_some() {
                info!(
                    conversation_id,
                    "ACP: removed live session entry for deleted conversation"
                );
            }
        }

        {
            let session_state = app_handle.state::<crate::CodexSessionState>();
            let mut sessions = session_state.sessions.lock().await;
            if let Some(entry) = sessions.remove(&conversation_id) {
                entry.handle.shutdown("用户删除 AIPP 对话，释放本地 Codex app-server 进程".to_string());
                info!(conversation_id, "Codex: removed live session entry for deleted conversation");
            }
        }

        {
            let session_state = app_handle.state::<crate::ClaudeSessionState>();
            let mut sessions = session_state.sessions.lock().await;
            if sessions.remove(&conversation_id).is_some() {
                info!(conversation_id, "Claude Code: removed live session entry for deleted conversation");
            }
        }

        if let (Some(session_id), Some(acp_config)) = (stored_session_id, acp_delete_config) {
            if let Err(error) = run_acp_session_delete(
                &app_handle,
                conversation_id,
                &session_id,
                acp_config,
            )
            .await
            {
                warn!(
                    conversation_id,
                    session_id = %session_id,
                    error = %error,
                    "ACP: agent-side session/delete failed; local cleanup continues"
                );
            }
        }

        match ConversationDatabase::new(&app_handle) {
            Ok(db) => {
                for agent_kind in ["acp", "codex_app_server", "claude_sdk"] {
                    if let Err(error) = db.delete_agent_session_id(conversation_id, agent_kind) {
                        warn!(
                            conversation_id,
                            agent_kind,
                            error = %error,
                            "Agent: failed to delete stored session id"
                        );
                    }
                }
            }
            Err(error) => {
                warn!(
                    conversation_id,
                    error = %error,
                    "ACP: failed to open conversation database for session id cleanup"
                );
            }
        }
    });
}

fn prepare_acp_session_delete_config(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
) -> Result<AcpConfig, AppError> {
    let conversation_db = ConversationDatabase::new(app_handle)?;
    let conversation = conversation_db
        .conversation_repo()?
        .read(conversation_id)?
        .ok_or_else(|| AppError::UnknownError(format!("conversation {conversation_id} not found")))?;
    let assistant_id = conversation.assistant_id.ok_or_else(|| {
        AppError::UnknownError(format!("conversation {conversation_id} has no assistant"))
    })?;
    let assistant_db = AssistantDatabase::new(app_handle)?;
    let assistant = assistant_db.get_assistant(assistant_id)?;
    if assistant.assistant_type != Some(4) {
        return Err(AppError::UnknownError(format!(
            "assistant {assistant_id} is not an ACP assistant"
        )));
    }
    let model_configs = assistant_db.get_assistant_model_configs(assistant_id)?;
    let assistant_models = assistant_db.get_assistant_model(assistant_id)?;
    let provider_id = resolve_acp_provider_id(&assistant_models, &model_configs)
        .ok_or_else(|| AppError::UnknownError("ACP assistant has no provider configured".into()))?;
    let llm_db = LLMDatabase::new(app_handle)?;
    let provider_configs = llm_db.get_llm_provider_config(provider_id)?;
    extract_acp_config(&model_configs, &provider_configs)
}

/// 启动一次性 agent 进程执行 session/delete（能力门控），完成后尽量 session/close 并退出。
async fn run_acp_session_delete(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    session_id: &str,
    acp_config: AcpConfig,
) -> Result<(), AppError> {
    let resolved_cli_command = resolve_acp_cli_path(&acp_config.cli_command);
    let launch_plan = build_acp_launch_plan(
        &acp_config.cli_command,
        &resolved_cli_command,
        &acp_config.additional_args,
        &acp_config.env_vars,
    );

    let mut cmd = Command::new(&launch_plan.program);
    cmd.current_dir(&acp_config.working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (key, value) in &acp_config.env_vars {
        cmd.env(key, value);
    }
    for (key, value) in &launch_plan.extra_env {
        cmd.env(key, value);
    }
    if !launch_plan.args.is_empty() {
        cmd.args(&launch_plan.args);
    }
    let mut child = cmd.spawn().map_err(|error| {
        AppError::UnknownError(format!(
            "failed to spawn ACP process '{}' for session/delete: {error}",
            acp_config.cli_command
        ))
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::UnknownError("failed to open stdin for ACP process".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::UnknownError("failed to open stdout for ACP process".into()))?;

    let session_id_owned = session_id.to_string();
    let connect_result = agent_client_protocol::Client
        .builder()
        .name("AIPP")
        .connect_with(
            agent_client_protocol::ByteStreams::new(stdin.compat_write(), stdout.compat()),
            async move |cx| {
                let to_acp_error = |message: String| {
                    agent_client_protocol::Error::internal_error()
                        .data(serde_json::Value::String(message))
                };

                let init_response = tokio::time::timeout(
                    Duration::from_secs(15),
                    cx.send_request(
                        acp::InitializeRequest::new(ProtocolVersion::V1)
                            .client_info(acp::Implementation::new("AIPP", "0.4.3")),
                    )
                    .block_task(),
                )
                .await
                .map_err(|_| to_acp_error("initialize timed out after 15 seconds".to_string()))?
                .map_err(|error| to_acp_error(format!("initialize failed: {error:?}")))?;

                if init_response
                    .agent_capabilities
                    .session_capabilities
                    .delete
                    .is_none()
                {
                    info!(
                        conversation_id,
                        session_id = %session_id_owned,
                        "ACP: agent does not support session/delete; skipping agent-side cleanup"
                    );
                    return Ok(());
                }

                tokio::time::timeout(
                    Duration::from_secs(15),
                    cx.send_request(acp::DeleteSessionRequest::new(session_id_owned.clone()))
                        .block_task(),
                )
                .await
                .map_err(|_| to_acp_error("session/delete timed out after 15 seconds".to_string()))?
                .map_err(|error| to_acp_error(format!("session/delete failed: {error:?}")))?;
                info!(
                    conversation_id,
                    session_id = %session_id_owned,
                    "ACP: agent-side session/delete succeeded"
                );

                if init_response
                    .agent_capabilities
                    .session_capabilities
                    .close
                    .is_some()
                {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(5),
                        cx.send_request(acp::CloseSessionRequest::new(session_id_owned.clone()))
                            .block_task(),
                    )
                    .await;
                }
                Ok(())
            },
        )
        .await;

    let _ = child.kill().await;

    connect_result.map_err(|error| {
        AppError::UnknownError(format!("ACP session/delete connection failed: {error:?}"))
    })
}

#[tauri::command]
pub async fn get_acp_session_state(
    acp_session_state: tauri::State<'_, crate::AcpSessionState>,
    codex_session_state: tauri::State<'_, crate::CodexSessionState>,
    claude_session_state: tauri::State<'_, crate::ClaudeSessionState>,
    conversation_id: i64,
) -> Result<Option<serde_json::Value>, String> {
    // A conversation can briefly have both entries while switching from the
    // legacy ACP runner to the native Codex runner. Prefer the explicit Codex
    // snapshot so stale ACP state cannot trigger the unsupported-resume notice.
    {
        let sessions = codex_session_state.sessions.lock().await;
        if let Some(entry) = sessions.get(&conversation_id) {
            return Ok(Some(
                serde_json::to_value(&entry.snapshot).unwrap_or(serde_json::Value::Null),
            ));
        }
    }
    {
        let sessions = claude_session_state.sessions.lock().await;
        if let Some(entry) = sessions.get(&conversation_id) {
            return Ok(Some(serde_json::to_value(&entry.snapshot).unwrap_or(serde_json::Value::Null)));
        }
    }
    {
        let mut sessions = acp_session_state.sessions.lock().await;
        if let Some(entry) = sessions.get_mut(&conversation_id) {
            entry.touch();
            return Ok(Some(
                serde_json::to_value(&entry.snapshot).unwrap_or(serde_json::Value::Null),
            ));
        }
    }
    Ok(None)
}

#[tauri::command]
pub async fn set_acp_session_config_option(
    app_handle: tauri::AppHandle,
    acp_session_state: tauri::State<'_, crate::AcpSessionState>,
    conversation_id: i64,
    config_id: String,
    value: String,
) -> Result<(), String> {
    let codex_state = app_handle.state::<crate::CodexSessionState>();
    if let Some(handle) = {
        let sessions = codex_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.handle.clone())
    } {
        handle.set_config_option(config_id, value).await.map_err(|error| error.to_string())?;
        return Ok(());
    }

    let claude_state = app_handle.state::<crate::ClaudeSessionState>();
    if let Some(handle) = {
        let sessions = claude_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.handle.clone())
    } {
        handle.set_config_option(config_id, value).await.map_err(|error| error.to_string())?;
        return Ok(());
    }
    let handle = {
        let mut sessions = acp_session_state.sessions.lock().await;
        sessions.get_mut(&conversation_id).map(|entry| {
            entry.touch();
            entry.handle.clone()
        })
    }
    .ok_or_else(|| "ACP session not found".to_string())?;

    handle
        .set_config_option(config_id, value)
        .await
        .map_err(|error| error.to_string())?;

    let state = {
        let sessions = acp_session_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.snapshot.clone())
    };
    emit_acp_session_state_snapshot(&app_handle, conversation_id, state);
    Ok(())
}

/// Extract ACP configuration from assistant_model_config and llm_provider_config
///
/// Configuration priority:
/// 1. assistant_model_config (assistant-level override)
/// 2. llm_provider_config (provider-level default)
/// 3. hardcoded default
pub fn extract_acp_config(
    model_configs: &[AssistantModelConfig],
    provider_configs: &[LLMProviderConfig],
) -> Result<AcpConfig, AppError> {
    use std::path::PathBuf;

    // Helper to get value from provider_configs
    let get_provider_config = |name: &str| -> Option<String> {
        provider_configs
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| {
                let value = c.value.trim();
                (!value.is_empty()).then(|| value.to_string())
            })
    };

    // Helper to get value from model_configs
    let get_model_config = |name: &str| -> Option<String> {
        model_configs
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.value.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    // 获取 CLI 命令
    // 只从 llm_provider_config 获取，因为这是提供商级别的配置
    // 注意：不同的 agent 需要不同的命令：
    // - Claude Code: 需要安装 @zed-industries/claude-code-acp，命令是 "claude-code-acp"
    // - Gemini: 原生支持 ACP，命令是 "gemini"
    // - Kimi Code CLI: 原生支持 ACP，命令是 "kimi"（启动时自动附加 acp 子命令）
    // - DeepSeek Harness: 需要安装 dsh-acp-server，命令是 "dsh-acp-server"
    let cli_command = get_provider_config("acp_cli_command").ok_or_else(|| {
        AppError::UnknownError(
            "当前 ACP 助手没有拿到有效的 provider CLI 配置：请先在助手配置里选择 ACP 提供商，再到模型提供商配置里选择 claude-code-acp、gemini 或其他 ACP CLI"
                .to_string(),
        )
    })?;

    debug!("ACP: cli_command from provider_config: {:?}", get_provider_config("acp_cli_command"));
    debug!("ACP: final cli_command: {}", cli_command);

    let claude_auth_mode = resolve_claude_auth_mode(
        &cli_command,
        get_model_config("acp_claude_auth_mode")
            .or_else(|| get_provider_config("acp_claude_auth_mode")),
        provider_configs,
        model_configs,
    );

    // 获取工作目录
    // 优先级: assistant_model_config > llm_provider_config > home_dir
    let working_directory = get_model_config("acp_working_directory")
        .or_else(|| get_provider_config("acp_working_directory"))
        .map(|path| expand_home_path(&path))
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));

    // 收集环境变量
    // 从两个配置源收集，model_config 优先级更高
    let mut env_vars = HashMap::new();

    if claude_auth_mode.as_deref() == Some(CLAUDE_SETTINGS_AUTH_MODE) {
        let settings_env = load_claude_settings_env_vars()?;
        info!(
            "ACP Claude auth env loaded from ~/.claude/settings.json: {} vars",
            settings_env.len()
        );
        env_vars.extend(settings_env);
    }

    // 先从 provider_configs 收集
    for config in provider_configs {
        if config.name == "acp_claude_env_vars" {
            if claude_auth_mode.as_deref() == Some(CLAUDE_ENV_AUTH_MODE) {
                merge_acp_env_blob(&mut env_vars, &config.value);
            }
            continue;
        }

        if config.name == "acp_env_vars" {
            merge_acp_env_blob(&mut env_vars, &config.value);
            continue;
        }

        if let Some(key) = config.name.strip_prefix("acp_env_") {
            env_vars.insert(key.to_uppercase(), config.value.clone());
        }
    }

    // 再从 model_configs 收集（会覆盖 provider 的同名配置）
    for config in model_configs {
        if config.name == "acp_claude_env_vars" {
            if claude_auth_mode.as_deref() == Some(CLAUDE_ENV_AUTH_MODE) {
                if let Some(value) = &config.value {
                    merge_acp_env_blob(&mut env_vars, value);
                }
            }
            continue;
        }

        if config.name == "acp_env_vars" {
            if let Some(value) = &config.value {
                merge_acp_env_blob(&mut env_vars, value);
            }
            continue;
        }

        if let Some(key) = config.name.strip_prefix("acp_env_") {
            if let Some(value) = &config.value {
                env_vars.insert(key.to_uppercase(), value.clone());
            }
        }
    }

    // 获取额外参数
    // 优先级: assistant_model_config > llm_provider_config > empty
    let additional_args = get_model_config("acp_additional_args")
        .or_else(|| get_provider_config("acp_additional_args"))
        .map(|args| args.split_whitespace().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    // Log the extracted configuration for debugging
    info!(
        "extract_acp_config: cli_command='{}', working_directory='{}', env_vars={}, additional_args={:?}, claude_auth_mode={:?}",
        cli_command,
        working_directory.display(),
        env_vars.len(),
        additional_args,
        claude_auth_mode
    );

    let mut config = AcpConfig {
        cli_command,
        working_directory,
        env_vars,
        additional_args,
        selected_mcp_tools_payload: "[]".to_string(),
        session_signature: String::new(),
    };
    config.session_signature = acp_config_signature(&config);

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::{
        acp_tool_status_to_aipp_status, acp_tool_update_status_to_aipp_status,
        append_buffered_content, apply_network_proxy_to_env_vars, build_acp_manual_mcp_servers_from_parts,
        build_acp_launch_plan, build_prompt_to_send, extract_acp_config, extract_content_text, get_claude_model_override,
        load_claude_settings_env_vars_from_path,
        merge_acp_usage_metadata, summarize_acp_prompt_usage, AcpPromptUsageSummary,
        AcpElicitationDecision, AcpElicitationState,
        AcpPermissionDecision, AcpPermissionRequestEvent, AcpPermissionState, AcpSessionEntry,
        AcpSessionHandle,
        json_to_acp_elicitation_value,
    };
    use crate::db::assistant_db::AssistantModelConfig;
    use crate::db::llm_db::LLMProviderConfig;
    use agent_client_protocol::schema::v1 as acp;
    use agent_client_protocol::schema::ProtocolVersion;
    use agent_client_protocol::{Channel, ConnectionTo, Responder};
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tempfile::NamedTempFile;
    use tokio::sync::{mpsc, oneshot, Notify};
    use tokio::time::{timeout, Duration};

    fn provider_config(name: &str, value: &str) -> LLMProviderConfig {
        LLMProviderConfig {
            id: 0,
            name: name.to_string(),
            llm_provider_id: 1,
            value: value.to_string(),
            append_location: "header".to_string(),
            is_addition: false,
        }
    }

    fn model_config(name: &str, value: &str) -> AssistantModelConfig {
        AssistantModelConfig {
            id: 0,
            assistant_id: 1,
            assistant_model_id: 1,
            name: name.to_string(),
            value: Some(value.to_string()),
            value_type: "string".to_string(),
        }
    }

    #[derive(Clone)]
    struct FakeStreamingAgent {
        prompt_started: Arc<Notify>,
        release_prompt: Arc<Notify>,
        prompts_received: Arc<Mutex<Vec<(acp::SessionId, Vec<acp::ContentBlock>)>>>,
    }

    impl FakeStreamingAgent {
        fn new() -> Self {
            Self {
                prompt_started: Arc::new(Notify::new()),
                release_prompt: Arc::new(Notify::new()),
                prompts_received: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[derive(Clone)]
    struct RecordingClient {
        session_notifications: Arc<Mutex<Vec<acp::SessionNotification>>>,
    }

    impl RecordingClient {
        fn new() -> Self {
            Self { session_notifications: Arc::new(Mutex::new(Vec::new())) }
        }
    }

    /// v2 SDK 下不再需要 `impl acp::Client/Agent` trait：
    /// 测试直接用 builder 回调注册 initialize/new_session/prompt 处理器，
    /// 并通过 `Channel::duplex()` 建立内存连接对。
    fn spawn_fake_agent(
        fake_agent: FakeStreamingAgent,
        agent_channel: Channel,
    ) -> tokio::task::JoinHandle<Result<(), agent_client_protocol::Error>> {
        tokio::spawn(async move {
            let prompt_agent = fake_agent.clone();
            agent_client_protocol::Agent
                .builder()
                .name("fake-acp-agent")
                .on_receive_request(
                    async move |request: acp::InitializeRequest,
                                responder: Responder<acp::InitializeResponse>,
                                _cx| {
                        responder.respond(acp::InitializeResponse::new(request.protocol_version))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: acp::NewSessionRequest,
                                responder: Responder<acp::NewSessionResponse>,
                                _cx| {
                        responder.respond(acp::NewSessionResponse::new("test-session"))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: acp::PromptRequest,
                                responder: Responder<acp::PromptResponse>,
                                cx: ConnectionTo<agent_client_protocol::Client>| {
                        let session_id = request.session_id.clone();
                        prompt_agent
                            .prompts_received
                            .lock()
                            .unwrap()
                            .push((request.session_id, request.prompt));
                        prompt_agent.prompt_started.notify_one();
                        let release_prompt = prompt_agent.release_prompt.clone();
                        let task_cx = cx.clone();
                        // 在独立任务中先流式发送通知、再等待释放信号后响应 prompt，
                        // 模拟真实 agent “prompt 未结束时持续推送 session/update” 的行为。
                        cx.spawn(async move {
                            for update in [
                                acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
                                    "thinking ".into(),
                                )),
                                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                                    "Hello ".into(),
                                )),
                                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                                    "world".into(),
                                )),
                            ] {
                                task_cx.send_notification(acp::SessionNotification::new(
                                    session_id.clone(),
                                    update,
                                ))?;
                            }
                            release_prompt.notified().await;
                            responder.respond(acp::PromptResponse::new(acp::StopReason::EndTurn));
                            Ok(())
                        })
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(agent_channel, async move |cx| {
                    cx.incoming_closed().await;
                    Ok(())
                })
                .await
        })
    }

    #[test]
    fn extract_acp_config_parses_multiline_env_blob_and_model_overrides() {
        let provider_configs = vec![
            provider_config("acp_cli_command", "gemini"),
            provider_config("acp_env_vars", "FOO=provider\nSHARED=provider"),
        ];
        let model_configs =
            vec![model_config("acp_env_vars", "BAR=model\nSHARED=model\n# COMMENT\nINVALID")];

        let config = extract_acp_config(&model_configs, &provider_configs).unwrap();

        assert_eq!(config.env_vars.get("FOO"), Some(&"provider".to_string()));
        assert_eq!(config.env_vars.get("BAR"), Some(&"model".to_string()));
        assert_eq!(config.env_vars.get("SHARED"), Some(&"model".to_string()));
        assert!(!config.env_vars.contains_key("INVALID"));
        assert_eq!(config.selected_mcp_tools_payload, "[]");
    }

    #[test]
    fn extract_acp_config_ignores_legacy_dynamic_mcp_loading_switch() {
        let provider_configs = vec![provider_config("acp_cli_command", "gemini")];
        let model_configs = vec![model_config("dynamic_mcp_loading_enabled", "false")];

        let config = extract_acp_config(&model_configs, &provider_configs).unwrap();

        assert_eq!(config.selected_mcp_tools_payload, "[]");
        assert!(!config.session_signature.contains("dynamic_mcp"));
    }

    #[test]
    fn extract_acp_config_errors_when_cli_command_is_missing() {
        let error = extract_acp_config(&[], &[]).unwrap_err();

        assert!(
            error.to_string().contains("没有拿到有效的 provider CLI 配置"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn apply_network_proxy_to_env_vars_preserves_existing_proxy_group() {
        let mut env_vars = HashMap::from([(
            "https_proxy".to_string(),
            "http://custom.example.com:8443".to_string(),
        )]);

        let injected =
            apply_network_proxy_to_env_vars(&mut env_vars, "http://proxy.example.com:8080");

        assert_eq!(injected, 4);
        assert_eq!(
            env_vars.get("https_proxy"),
            Some(&"http://custom.example.com:8443".to_string())
        );
        assert_eq!(env_vars.get("HTTP_PROXY"), Some(&"http://proxy.example.com:8080".to_string()));
        assert_eq!(env_vars.get("ALL_PROXY"), Some(&"http://proxy.example.com:8080".to_string()));
    }

    #[test]
    fn build_acp_launch_plan_uses_standard_env_for_non_node_cli() {
        let env_vars =
            HashMap::from([("HTTPS_PROXY".to_string(), "http://127.0.0.1:7897".to_string())]);
        let plan = build_acp_launch_plan(
            "gemini",
            Path::new("/tmp/gemini"),
            &["--foo".to_string()],
            &env_vars,
        );

        assert_eq!(plan.proxy_strategy, "standard-env-non-node-script");
        assert_eq!(plan.program, PathBuf::from("/tmp/gemini"));
        assert_eq!(plan.args, vec!["--foo".to_string()]);
    }

    #[test]
    fn build_acp_launch_plan_prepends_kimi_acp_subcommand() {
        let env_vars = HashMap::new();
        let plan = build_acp_launch_plan(
            "kimi",
            Path::new("/tmp/kimi"),
            &["--foo".to_string()],
            &env_vars,
        );

        assert_eq!(plan.proxy_strategy, "standard-env");
        assert_eq!(plan.program, PathBuf::from("/tmp/kimi"));
        assert_eq!(plan.args, vec!["acp".to_string(), "--foo".to_string()]);
    }

    #[test]
    fn build_acp_launch_plan_switches_non_claude_node_script_to_node_runtime() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "#!/usr/bin/env node").unwrap();
        writeln!(file, "console.log('hello')").unwrap();

        let env_vars = HashMap::new();
        let plan = build_acp_launch_plan("dsh-acp-server", file.path(), &[], &env_vars);

        assert!(plan.program.ends_with("node"));
        assert!(plan.extra_env.get("NODE_USE_ENV_PROXY").is_none());
        assert_eq!(plan.proxy_strategy, "node-explicit-runtime");
        assert_eq!(plan.args.len(), 1);
        assert!(plan.args[0].contains(file.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn build_acp_manual_mcp_servers_injects_stdio_tool_bridge() {
        let servers = build_acp_manual_mcp_servers_from_parts(
            PathBuf::from("Aipp.exe"),
            PathBuf::from("C:/Users/test/AppData/Roaming/aipp/db/mcp.db"),
            42,
            "127.0.0.1:12345".to_string(),
            "token-1".to_string(),
            "[{\"server_id\":7,\"server_name\":\"Search\",\"tools\":[]}]".to_string(),
        );

        assert_eq!(servers.len(), 1);
        let acp::McpServer::Stdio(server) = &servers[0] else {
            panic!("manual MCP bridge should use stdio");
        };
        assert_eq!(server.name, "AIPP MCP Tools");
        assert!(server.args.iter().any(|arg| arg == "--aipp-acp-mcp-bridge"));
        assert!(
            server
                .env
                .iter()
                .any(|env| env.name == "AIPP_ACP_CONVERSATION_ID" && env.value == "42")
        );
        assert!(server.env.iter().any(|env| {
            env.name == "AIPP_ACP_MCP_DB_PATH" && env.value.ends_with("aipp/db/mcp.db")
        }));
        assert!(server.env.iter().any(|env| {
            env.name == "AIPP_ACP_MCP_PROXY_ADDR" && env.value == "127.0.0.1:12345"
        }));
        assert!(server.env.iter().any(|env| {
            env.name == "AIPP_ACP_SELECTED_MCP_TOOLS" && env.value.contains("\"server_id\":7")
        }));
    }

    #[test]
    fn build_acp_launch_plan_switches_claude_node_script_to_node_runtime() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "#!/usr/bin/env node").unwrap();
        writeln!(file, "console.log('hello')").unwrap();

        let env_vars =
            HashMap::from([("HTTPS_PROXY".to_string(), "http://127.0.0.1:7897".to_string())]);
        let plan = build_acp_launch_plan(
            "claude-code-acp",
            file.path(),
            &["--bar".to_string()],
            &env_vars,
        );

        assert!(plan.program.ends_with("node"));
        assert_eq!(plan.extra_env.get("NODE_USE_ENV_PROXY"), Some(&"1".to_string()));
        assert!(plan.proxy_strategy.starts_with("node-use-env-proxy"));
        assert!(plan.args.iter().any(|arg| arg == "--bar"));
        assert!(plan.args.iter().any(|arg| arg.contains(file.path().to_string_lossy().as_ref())));
    }

    #[test]
    fn build_acp_launch_plan_uses_explicit_node_runtime_without_proxy_env() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "#!/usr/bin/env node").unwrap();
        writeln!(file, "console.log('hello')").unwrap();

        let env_vars = HashMap::new();
        let plan = build_acp_launch_plan("claude-code-acp", file.path(), &[], &env_vars);

        assert!(plan.program.ends_with("node"));
        assert!(plan.extra_env.get("NODE_USE_ENV_PROXY").is_none());
        assert_eq!(plan.proxy_strategy, "node-explicit-runtime");
        assert_eq!(plan.args.len(), 1);
        assert!(plan.args[0].contains(file.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn load_claude_settings_env_vars_from_path_reads_env_object() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "{{\"env\":{{\"ANTHROPIC_API_KEY\":\"test-key\",\"ANTHROPIC_BASE_URL\":\"https://proxy.example.com\"}}}}"
        )
        .unwrap();

        let env_vars = load_claude_settings_env_vars_from_path(file.path()).unwrap();

        assert_eq!(env_vars.get("ANTHROPIC_API_KEY"), Some(&"test-key".to_string()));
        assert_eq!(
            env_vars.get("ANTHROPIC_BASE_URL"),
            Some(&"https://proxy.example.com".to_string())
        );
    }

    #[test]
    fn extract_acp_config_merges_manual_claude_env_vars_when_selected() {
        let provider_configs = vec![
            provider_config("acp_cli_command", "claude-code-acp"),
            provider_config("acp_claude_auth_mode", "env_vars"),
            provider_config(
                "acp_claude_env_vars",
                "ANTHROPIC_API_KEY=provider-key\nANTHROPIC_BASE_URL=https://provider.example.com",
            ),
        ];
        let model_configs =
            vec![model_config("acp_claude_env_vars", "ANTHROPIC_API_KEY=model-key")];

        let config = extract_acp_config(&model_configs, &provider_configs).unwrap();

        assert_eq!(config.env_vars.get("ANTHROPIC_API_KEY"), Some(&"model-key".to_string()));
        assert_eq!(
            config.env_vars.get("ANTHROPIC_BASE_URL"),
            Some(&"https://provider.example.com".to_string())
        );
    }

    #[test]
    fn get_claude_model_override_returns_custom_model_only_for_claude() {
        let mut env_vars = HashMap::new();
        env_vars.insert("ANTHROPIC_MODEL".to_string(), "glm-5".to_string());

        assert_eq!(
            get_claude_model_override("claude-code-acp", &env_vars),
            Some("glm-5".to_string())
        );
        assert_eq!(get_claude_model_override("gemini", &env_vars), None);

        env_vars.insert("ANTHROPIC_MODEL".to_string(), "default".to_string());
        assert_eq!(get_claude_model_override("claude-code-acp", &env_vars), None);
    }

    #[test]
    fn acp_session_entry_idle_timeout_ignores_active_prompt() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let handle = AcpSessionHandle { sender, run_id: "test-run".to_string() };
        let mut entry = AcpSessionEntry::new(handle, 42, "test-signature");
        let idle_timeout = Duration::from_secs(15 * 60);

        entry.last_activity = Instant::now() - Duration::from_secs(16 * 60);
        assert!(entry.is_idle_for(idle_timeout));

        entry.snapshot.has_active_prompt = true;
        assert!(!entry.is_idle_for(idle_timeout));
    }

    #[test]
    fn acp_session_entry_touch_resets_idle_timeout() {
        let (sender, _receiver) = mpsc::unbounded_channel();
        let handle = AcpSessionHandle { sender, run_id: "test-run".to_string() };
        let mut entry = AcpSessionEntry::new(handle, 43, "test-signature");
        let idle_timeout = Duration::from_secs(15 * 60);

        entry.last_activity = Instant::now() - Duration::from_secs(16 * 60);
        assert!(entry.is_idle_for(idle_timeout));

        entry.touch();
        assert!(!entry.is_idle_for(idle_timeout));
    }

    #[tokio::test]
    async fn append_buffered_content_accumulates_text_chunks() {
        let mut buffer = String::new();
        let first = append_buffered_content(&mut buffer, &acp::ContentBlock::from("Hello "));
        let second = append_buffered_content(&mut buffer, &acp::ContentBlock::from("world"));

        assert_eq!(first, "Hello ");
        assert_eq!(second, "Hello world");
    }

    #[tokio::test]
    async fn cancel_requests_for_conversation_returns_cancelled_request_resolutions() {
        let state = AcpPermissionState::new();
        let (tx, rx) = oneshot::channel();
        state
            .store_request(
                AcpPermissionRequestEvent {
                    request_id: "request-1".to_string(),
                    conversation_id: Some(42),
                    agent_kind: Some("acp".to_string()),
                    tool_call_id: "tool-1".to_string(),
                    title: Some("Read file".to_string()),
                    kind: Some("read_file".to_string()),
                    parameters: None,
                    options: vec![],
                },
                tx,
            )
            .await;

        let resolutions = state.cancel_requests_for_conversation(42).await;

        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].0, "request-1");
        assert_eq!(resolutions[0].1.conversation_id, Some(42));
        assert!(resolutions[0].1.delivered);
        assert!(matches!(rx.await, Ok(AcpPermissionDecision::Cancelled)));
        assert!(state.get_request("request-1").await.is_none());
    }

    /// 测试 elicitation 挂起请求的存储/决策/取消生命周期
    ///
    /// 验证内容：
    /// - accept 决策能把 typed content 送达等待中的 handler
    /// - cancel_requests_for_conversation 会把同会话的挂起请求标记为 Cancelled
    /// - resolve 后请求从状态中移除
    #[tokio::test]
    async fn acp_elicitation_state_resolves_and_cancels_requests() {
        let state = AcpElicitationState::new();

        let (accept_tx, accept_rx) = oneshot::channel();
        state.store_request("elicit-accept".to_string(), Some(7), accept_tx).await;
        let mut content = std::collections::BTreeMap::new();
        content.insert(
            "city".to_string(),
            acp::ElicitationContentValue::String("北京".to_string()),
        );
        let resolution = state
            .resolve_request("elicit-accept", AcpElicitationDecision::Accepted(content))
            .await
            .expect("request should exist");
        assert_eq!(resolution.conversation_id, Some(7));
        assert!(resolution.delivered);
        match accept_rx.await {
            Ok(AcpElicitationDecision::Accepted(values)) => {
                assert_eq!(
                    values.get("city"),
                    Some(&acp::ElicitationContentValue::String("北京".to_string()))
                );
            }
            other => panic!("expected accept decision, got {:?}", other),
        }

        let (cancel_tx, cancel_rx) = oneshot::channel();
        state.store_request("elicit-cancel".to_string(), Some(7), cancel_tx).await;
        let resolutions = state.cancel_requests_for_conversation(7).await;
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].0, "elicit-cancel");
        assert!(matches!(
            cancel_rx.await,
            Ok(AcpElicitationDecision::Cancelled)
        ));
        assert!(state
            .resolve_request("elicit-cancel", AcpElicitationDecision::Declined)
            .await
            .is_none());
    }

    /// 测试前端 JSON 值到 ACP ElicitationContentValue 的转换
    ///
    /// 验证内容：
    /// - string / bool / integer / number / string-array 的正确映射
    /// - 非字符串数组与不支持的类型返回具体错误
    #[test]
    fn json_to_acp_elicitation_value_maps_supported_types() {
        assert_eq!(
            json_to_acp_elicitation_value(serde_json::json!("hello")).unwrap(),
            acp::ElicitationContentValue::String("hello".to_string())
        );
        assert_eq!(
            json_to_acp_elicitation_value(serde_json::json!(true)).unwrap(),
            acp::ElicitationContentValue::Boolean(true)
        );
        assert_eq!(
            json_to_acp_elicitation_value(serde_json::json!(42)).unwrap(),
            acp::ElicitationContentValue::Integer(42)
        );
        assert_eq!(
            json_to_acp_elicitation_value(serde_json::json!(1.5)).unwrap(),
            acp::ElicitationContentValue::Number(1.5)
        );
        assert_eq!(
            json_to_acp_elicitation_value(serde_json::json!(["a", "b"])).unwrap(),
            acp::ElicitationContentValue::StringArray(vec![
                "a".to_string(),
                "b".to_string()
            ])
        );

        assert!(json_to_acp_elicitation_value(serde_json::json!([1, 2])).is_err());
        assert!(json_to_acp_elicitation_value(serde_json::json!({"k": "v"})).is_err());
        assert!(json_to_acp_elicitation_value(serde_json::Value::Null).is_err());
    }

    #[test]
    fn build_prompt_to_send_prepends_history_prefix() {
        let prompt = build_prompt_to_send(
            "Summarize the workspace".to_string(),
            Some("历史摘要".to_string()),
        );

        assert_eq!(prompt, "历史摘要\n\n当前用户请求:\nSummarize the workspace");
    }

    #[test]
    fn acp_pending_tool_without_confirmation_meta_is_treated_as_executing() {
        assert_eq!(
            acp_tool_status_to_aipp_status(acp::ToolCallStatus::Pending, None),
            "executing"
        );
        assert_eq!(
            acp_tool_update_status_to_aipp_status(
                None,
                None,
                Some("pending"),
                false,
            ),
            "executing"
        );
        assert_eq!(
            acp_tool_update_status_to_aipp_status(
                Some(&acp::ToolCallStatus::Pending),
                None,
                Some("success"),
                false,
            ),
            "success"
        );
    }

    #[tokio::test]
    async fn fake_agent_prompt_flow_streams_notifications_while_prompt_is_pending() {
        let client = RecordingClient::new();
        let fake_agent = FakeStreamingAgent::new();
        let (client_channel, agent_channel) = Channel::duplex();

        let agent_task = spawn_fake_agent(fake_agent.clone(), agent_channel);

        let client_task = {
            let client = client.clone();
            let fake_agent = fake_agent.clone();
            tokio::spawn(async move {
                let note_client = client.clone();
                agent_client_protocol::Client
                    .builder()
                    .name("aipp-test-client")
                    .on_receive_notification(
                        async move |notification: acp::SessionNotification, _cx| {
                            note_client.session_notifications.lock().unwrap().push(notification);
                            Ok(())
                        },
                        agent_client_protocol::on_receive_notification!(),
                    )
                    .connect_with(client_channel, async move |cx| {
                        cx.send_request(
                            acp::InitializeRequest::new(ProtocolVersion::V1).client_info(
                                acp::Implementation::new("aipp-test-client", "0.0.0")
                                    .title("AIPP Test Client"),
                            ),
                        )
                        .block_task()
                        .await
                        .expect("initialize should succeed");

                        let session = cx
                            .send_request(acp::NewSessionRequest::new(std::env::temp_dir()))
                            .block_task()
                            .await
                            .expect("new_session should succeed");
                        let session_id = session.session_id.clone();
                        let prompt_text = build_prompt_to_send(
                            "Summarize the workspace".to_string(),
                            Some("历史摘要".to_string()),
                        );

                        let (prompt_tx, mut prompt_rx) = oneshot::channel();
                        cx.spawn({
                            let cx = cx.clone();
                            let session_id = session_id.clone();
                            async move {
                                let result = cx
                                    .send_request(acp::PromptRequest::new(
                                        session_id,
                                        vec![prompt_text.into()],
                                    ))
                                    .block_task()
                                    .await;
                                let _ = prompt_tx.send(result);
                                Ok(())
                            }
                        })
                        .expect("prompt task should spawn");

                        timeout(Duration::from_secs(5), fake_agent.prompt_started.notified())
                            .await
                            .expect("agent should start the prompt");
                        assert!(matches!(
                            prompt_rx.try_recv(),
                            Err(oneshot::error::TryRecvError::Empty)
                        ));

                        {
                            let prompts = fake_agent.prompts_received.lock().unwrap();
                            assert_eq!(prompts.len(), 1);
                            assert_eq!(
                                extract_content_text(&prompts[0].1[0]),
                                "历史摘要\n\n当前用户请求:\nSummarize the workspace"
                            );
                        }

                        // prompt 挂起期间，agent 推送的 3 条通知应陆续到达 client。
                        timeout(Duration::from_secs(5), async {
                            loop {
                                if client.session_notifications.lock().unwrap().len() >= 3 {
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            }
                        })
                        .await
                        .expect("notifications should stream while prompt is pending");
                        assert!(matches!(
                            prompt_rx.try_recv(),
                            Err(oneshot::error::TryRecvError::Empty)
                        ));

                        fake_agent.release_prompt.notify_one();

                        let prompt_response = timeout(Duration::from_secs(5), prompt_rx)
                            .await
                            .expect("prompt response should arrive")
                            .expect("prompt channel should deliver")
                            .expect("prompt should succeed");
                        assert!(matches!(
                            prompt_response.stop_reason,
                            acp::StopReason::EndTurn
                        ));

                        Ok::<(), agent_client_protocol::Error>(())
                    })
                    .await
            })
        };

        client_task
            .await
            .expect("client task should not panic")
            .expect("client connection should close cleanly");
        timeout(Duration::from_secs(5), agent_task)
            .await
            .expect("agent task should finish after client disconnects")
            .expect("agent task should not panic")
            .expect("agent connection should close cleanly");

        let notifications = client.session_notifications.lock().unwrap().clone();
        let mut reasoning = String::new();
        let mut response = String::new();

        for notification in notifications {
            match notification.update {
                acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk { content, .. }) => {
                    append_buffered_content(&mut reasoning, &content);
                }
                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk { content, .. }) => {
                    append_buffered_content(&mut response, &content);
                }
                _ => {}
            }
        }

        assert_eq!(reasoning, "thinking ");
        assert_eq!(response, "Hello world");
    }

    #[test]
    fn summarize_acp_prompt_usage_prefers_reported_usage() {
        let prompt_response = acp::PromptResponse::new(acp::StopReason::EndTurn).usage(
            acp::Usage::new(120, 40, 50)
                .thought_tokens(Some(20))
                .cached_read_tokens(Some(8))
                .cached_write_tokens(Some(2)),
        );

        let prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            "ignored because usage is reported".to_string(),
        ))];

        let summary = summarize_acp_prompt_usage(
            &prompt_response,
            &prompt_blocks,
            "final response",
            "reasoning",
        );

        assert_eq!(summary.total_tokens, 120);
        assert_eq!(summary.input_tokens, 40);
        assert_eq!(summary.output_tokens, 50);
        assert_eq!(summary.thought_tokens, Some(20));
        assert_eq!(summary.cached_read_tokens, Some(8));
        assert_eq!(summary.cached_write_tokens, Some(2));
        assert_eq!(summary.usage_source, "reported");
    }

    #[test]
    fn summarize_acp_prompt_usage_estimates_when_usage_missing() {
        let prompt_response = acp::PromptResponse::new(acp::StopReason::EndTurn);
        let prompt_blocks = vec![
            acp::ContentBlock::Text(acp::TextContent::new("hello world".to_string())),
            acp::ContentBlock::Text(acp::TextContent::new("你好".to_string())),
        ];

        let summary = summarize_acp_prompt_usage(
            &prompt_response,
            &prompt_blocks,
            "assistant output",
            "internal reasoning",
        );

        assert_eq!(summary.usage_source, "estimated");
        assert!(summary.input_tokens > 0);
        assert!(summary.output_tokens > 0);
        assert!(summary.thought_tokens.unwrap_or(0) > 0);
        assert_eq!(
            summary.total_tokens,
            summary.input_tokens + summary.output_tokens + summary.thought_tokens.unwrap_or(0)
        );
        assert_eq!(summary.cached_read_tokens, None);
        assert_eq!(summary.cached_write_tokens, None);
    }

    #[test]
    fn merge_acp_usage_metadata_preserves_unrelated_fields() {
        let summary = AcpPromptUsageSummary {
            total_tokens: 120,
            input_tokens: 40,
            output_tokens: 50,
            thought_tokens: Some(20),
            cached_read_tokens: Some(8),
            cached_write_tokens: Some(2),
            usage_source: "reported",
        };

        let merged = merge_acp_usage_metadata(
            Some(r#"{"speakerLabel":"ACP","existing":true}"#),
            &summary,
        )
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&merged).unwrap();

        assert_eq!(parsed["speakerLabel"], "ACP");
        assert_eq!(parsed["existing"], true);
        assert_eq!(parsed["usage_source"], "reported");
        assert_eq!(parsed["thought_tokens"], 20);
        assert_eq!(parsed["cached_input_tokens"], 8);
        assert_eq!(parsed["cached_read_tokens"], 8);
        assert_eq!(parsed["cached_write_tokens"], 2);
    }
}

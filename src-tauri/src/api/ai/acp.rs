//! ACP (Agent Client Protocol) integration module
//! Handles communication with ACP-compatible agents via stdio

use crate::api::ai::config::build_proxy_env_vars;
use crate::api::ai::conversation::extract_tool_result;
use crate::api::ai::events::{
    ConversationEvent, MCPToolCallUpdateEvent, MessageUpdateEvent, TITLE_CHANGE_EVENT,
};
use crate::api::operation_api::{
    emit_permission_request_event, emit_permission_resolved_event, PermissionResolvedEvent,
    ACP_PERMISSION_REQUEST_EVENT, ACP_PERMISSION_RESOLVED_EVENT,
};
use crate::db::assistant_db::AssistantModelConfig;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::llm_db::LLMProviderConfig;
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::errors::AppError;
use crate::mcp::builtin_mcp::operation::{
    bash_ops::BashOperations,
    file_ops::FileOperations,
    permission::PermissionManager,
    state::OperationState,
    types::{
        BashProcessStatus, ExecuteBashRequest, GetBashOutputRequest, ReadFileRequest,
        WriteFileRequest,
    },
};
use crate::state::activity_state::ConversationActivityManager;
use crate::utils::window_utils::send_conversation_event_to_chat_windows;
use agent_client_protocol::{
    self as acp, Agent as _, Client as AcpClient, ClientSideConnection, ToolCallLocation,
};
use regex::Regex;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tauri::{Emitter, Manager};
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
}

#[derive(Debug, Clone)]
pub struct AcpLaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub extra_env: HashMap<String, String>,
    pub proxy_strategy: String,
}

fn merge_acp_env_blob(env_vars: &mut HashMap<String, String>, raw: &str) {
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
pub struct AcpConversationSessionState {
    pub conversation_id: i64,
    pub session_id: Option<String>,
    pub title: Option<String>,
    pub updated_at: Option<String>,
    pub current_mode_id: Option<String>,
    pub modes: Vec<AcpSessionModePayload>,
    pub config_options: Vec<AcpSessionConfigOptionPayload>,
    pub has_active_prompt: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AcpSessionStateSnapshotEvent {
    state: Option<AcpConversationSessionState>,
}

enum AcpSessionCommand {
    Prompt { message_id: i64, prompt: String, window: tauri::Window },
    CancelCurrentPrompt {
        response: oneshot::Sender<Result<(), String>>,
    },
    SetMode {
        mode_id: String,
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
}

impl AcpSessionHandle {
    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        self.sender
            .send(AcpSessionCommand::Prompt { message_id, prompt, window })
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

    pub async fn set_mode(&self, mode_id: String) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(AcpSessionCommand::SetMode { mode_id, response: tx })
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
}

impl AcpSessionEntry {
    pub fn new(handle: AcpSessionHandle, conversation_id: i64) -> Self {
        Self {
            handle,
            snapshot: AcpConversationSessionState { conversation_id, ..Default::default() },
        }
    }
}

fn session_mode_payload(mode: &acp::SessionMode) -> AcpSessionModePayload {
    AcpSessionModePayload {
        id: mode.id.to_string(),
        name: mode.name.clone(),
        description: mode.description.clone(),
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

fn apply_session_mode_state_to_snapshot(
    snapshot: &mut AcpConversationSessionState,
    modes: Option<&acp::SessionModeState>,
) {
    if let Some(modes) = modes {
        snapshot.current_mode_id = Some(modes.current_mode_id.to_string());
        snapshot.modes = modes.available_modes.iter().map(session_mode_payload).collect();
    } else {
        snapshot.current_mode_id = None;
        snapshot.modes.clear();
    }
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

pub fn build_acp_launch_plan(
    cli_command: &str,
    resolved_cli_command: &Path,
    additional_args: &[String],
    env_vars: &HashMap<String, String>,
) -> AcpLaunchPlan {
    let mut plan = AcpLaunchPlan {
        program: resolved_cli_command.to_path_buf(),
        args: additional_args.to_vec(),
        extra_env: HashMap::new(),
        proxy_strategy: "standard-env".to_string(),
    };

    if cli_command != "claude-code-acp" {
        return plan;
    }

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
    plan.args.extend(additional_args.iter().cloned());

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

/// Terminal ID to bash_id mapping for ACP terminal management
struct TerminalMapping {
    terminal_id: acp::TerminalId,
    bash_id: String,
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
    if let Some(prefix) = history_prefix {
        format!("{}\n\n当前用户请求:\n{}", prefix, prompt)
    } else {
        prompt
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
    locations: Option<&[ToolCallLocation]>,
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
        update(&mut entry.snapshot);
        Some(entry.snapshot.clone())
    }

    async fn publish_session_state(&self, state: Option<AcpConversationSessionState>) {
        emit_acp_session_state_snapshot(&self.app_handle, self.conversation_id, state);
    }

    async fn set_session_bootstrap(
        &self,
        session_id: &str,
        modes: Option<&acp::SessionModeState>,
        config_options: Option<&[acp::SessionConfigOption]>,
    ) {
        let state = self
            .update_session_state(|snapshot| {
                snapshot.session_id = Some(session_id.to_string());
                apply_session_mode_state_to_snapshot(snapshot, modes);
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
    pub async fn send_done_event(&self, message_type: &str, content: &str) {
        let message_id = *self.message_id.lock().await;
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: message_type.to_string(),
                content: content.to_string(),
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
            activity_manager
                .clear_message_focus_keep_mcp(&self.app_handle, self.conversation_id)
                .await;
        }
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

#[async_trait::async_trait(?Send)]
impl AcpClient for AcpTauriClient {
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

                    let mut status_str = tool_status_to_string(tool_call.status);
                    if status_str == "pending" {
                        if let Some(false) = meta_requires_confirmation(tool_call.meta.as_ref()) {
                            status_str = "executing".to_string();
                        }
                    }

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

                        let mut status_str = tool_status_to_string(tool_call.status);
                        if status_str == "pending" {
                            if let Some(false) = meta_requires_confirmation(tool_call.meta.as_ref())
                            {
                                status_str = "executing".to_string();
                            }
                        }

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

                let message_id = *self.message_id.lock().await;
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
                            let event = ConversationEvent {
                                r#type: "mcp_tool_call_update".to_string(),
                                data: serde_json::to_value(MCPToolCallUpdateEvent {
                                    call_id,
                                    conversation_id: self.conversation_id,
                                    status: "pending".to_string(),
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
                            self.sync_tool_shine_status(call_id, "pending").await;
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

                let mut status_str = tool_status_to_string(tool_call.status);
                if status_str == "pending" {
                    if let Some(false) = meta_requires_confirmation(tool_call.meta.as_ref()) {
                        status_str = "executing".to_string();
                    }
                }

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

                let mut status_str = if let Some(status) = update.fields.status.as_ref() {
                    tool_status_to_string(status.clone())
                } else if let Ok(db) = MCPDatabase::new(&self.app_handle) {
                    db.get_mcp_tool_call(call_id)
                        .map(|call| call.status)
                        .unwrap_or_else(|_| "executing".to_string())
                } else {
                    "executing".to_string()
                };

                if status_str == "pending" {
                    if let Some(false) = meta_requires_confirmation(update.meta.as_ref()) {
                        status_str = "executing".to_string();
                    }
                }

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
                let error_str = error.map(|e| e.to_string());

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
                // TODO: Add frontend support for agent plan display
            }

            // Available commands update - log only, no UI support yet
            acp::SessionUpdate::AvailableCommandsUpdate(commands_update) => {
                info!(
                    "ACP AvailableCommandsUpdate: {} commands",
                    commands_update.available_commands.len()
                );
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

        // Build the full command with args
        let full_command = if args.args.is_empty() {
            args.command.clone()
        } else {
            format!("{} {}", args.command, args.args.join(" "))
        };

        // Create execute bash request
        let request = ExecuteBashRequest {
            command: full_command.clone(),
            description: Some(format!("ACP terminal: {}", full_command)),
            timeout: None,
            run_in_background: Some(true),
        };

        match BashOperations::execute_bash(Some(&self.app_handle), &self.operation_state, request)
            .await
        {
            Ok(response) => {
                let bash_id = response.bash_id.ok_or_else(|| {
                    acp::Error::internal_error().data("No bash_id returned for background process")
                })?;

                // Convert bash_id to TerminalId (wrap in Arc<str>)
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

        info!("Terminal command killed: {}", bash_id);
        Ok(acp::KillTerminalResponse::new())
    }

    async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse, acp::Error> {
        info!("ACP ext_method: method={}, params={:?}", args.method, args.params);

        // For now, return NULL response
        // Custom extensions can be implemented here as needed
        Ok(acp::ExtResponse::new(serde_json::value::RawValue::NULL.to_owned().into()))
    }

    async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<(), acp::Error> {
        debug!("ACP ext_notification: method={}, params={:?}", args.method, args.params);
        Ok(())
    }
}

/// Execute an ACP session
pub fn spawn_acp_session_task(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    acp_config: AcpConfig,
) -> AcpSessionHandle {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = AcpSessionHandle { sender };

    let cleanup_handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| anyhow::Error::msg(e.to_string()))?;
        rt.block_on(async move {
            let local_set = tokio::task::LocalSet::new();
            let result = local_set
                .run_until(async move {
                    run_acp_session(app_handle, conversation_id, acp_config, receiver)
                        .await
                        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
                    Ok::<(), anyhow::Error>(())
                })
                .await;

            if let Some(permission_state) = cleanup_handle.try_state::<AcpPermissionState>() {
                let resolutions = permission_state.cancel_requests_for_conversation(conversation_id).await;
                notify_cancelled_acp_permission_requests(&cleanup_handle, resolutions).await;
            }

            {
                let session_state = cleanup_handle.state::<crate::AcpSessionState>();
                let mut sessions = session_state.sessions.lock().await;
                sessions.remove(&conversation_id);
            }
            emit_acp_session_state_snapshot(&cleanup_handle, conversation_id, None);

            result
        })
    });

    handle
}

async fn process_acp_prompt(
    client_handle: &AcpTauriClient,
    conn: &ClientSideConnection,
    session_id: &str,
    conversation_id: i64,
    message_id: i64,
    prompt: String,
    window: tauri::Window,
    history_prefix: Option<String>,
) -> Result<(), AppError> {
    client_handle.set_current_message(message_id).await;
    client_handle.set_window(window).await;
    client_handle.reset_buffers().await;

    let prompt_to_send = build_prompt_to_send(prompt, history_prefix);

    info!("ACP: Sending prompt (conversation_id={}, message_id={})", conversation_id, message_id);
    let prompt_response = conn
        .prompt(acp::PromptRequest::new(session_id.to_string(), vec![prompt_to_send.into()]))
        .await;

    if let Err(e) = &prompt_response {
        let err_msg = format!("ACP prompt failed: {:?}", e);
        error!("ACP: {}", err_msg);
        client_handle.send_error_event(&err_msg).await;
        return Err(AppError::UnknownError(err_msg));
    }
    info!("ACP: Prompt completed successfully");

    let final_content = client_handle.get_response_content().await;
    client_handle.update_message_in_db(&final_content).await;
    client_handle.send_done_event("response", &final_content).await;
    Ok(())
}

async fn run_acp_session(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    acp_config: AcpConfig,
    mut receiver: mpsc::UnboundedReceiver<AcpSessionCommand>,
) -> Result<(), AppError> {
    info!("ACP session task started: conversation_id={}", conversation_id);

    let (first_message_id, first_prompt, first_window) = loop {
        match receiver.recv().await {
            Some(AcpSessionCommand::Prompt { message_id, prompt, window }) => {
                break (message_id, prompt, window);
            }
            Some(AcpSessionCommand::CancelCurrentPrompt { response }) => {
                let _ = response.send(Ok(()));
            }
            Some(AcpSessionCommand::SetMode { response, .. })
            | Some(AcpSessionCommand::SetConfigOption { response, .. }) => {
                let _ = response.send(Err("ACP session is not ready yet".to_string()));
            }
            None => {
                info!("ACP session task ended before start: conversation_id={}", conversation_id);
                return Ok(());
            }
        }
    };

    let send_startup_error = |window: &tauri::Window, message_id: i64, msg: &str| {
        if let Ok(db) = ConversationDatabase::new(&app_handle) {
            if let Ok(repo) = db.message_repo() {
                let _ = repo.update_content(message_id, msg);
            }
        }
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
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
    };

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
        send_startup_error(&first_window, first_message_id, &err_msg);
        return Err(AppError::UnknownError(err_msg));
    }
    if !acp_config.working_directory.is_dir() {
        let err_msg = format!(
            "ACP working directory is not a directory: {}",
            acp_config.working_directory.display()
        );
        error!("ACP: {}", err_msg);
        send_startup_error(&first_window, first_message_id, &err_msg);
        return Err(AppError::UnknownError(err_msg));
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
                "codex-acp" => "\n\n安装方法: bun add -g @zed-industries/codex-acp",
                "gemini" => "\n\n安装方法: 请参考 Google Gemini CLI 官方文档",
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
            send_startup_error(&first_window, first_message_id, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };
    info!("ACP: Process spawned successfully, PID={:?}", child.id());

    let stdin = match child.stdin.take() {
        Some(value) => value,
        None => {
            let err_msg = "Failed to open stdin for ACP process".to_string();
            send_startup_error(&first_window, first_message_id, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };
    let stdout = match child.stdout.take() {
        Some(value) => value,
        None => {
            let err_msg = "Failed to open stdout for ACP process".to_string();
            send_startup_error(&first_window, first_message_id, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };
    let stderr = match child.stderr.take() {
        Some(value) => value,
        None => {
            let err_msg = "Failed to open stderr for ACP process".to_string();
            send_startup_error(&first_window, first_message_id, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };

    let client_impl = AcpTauriClient::new(
        app_handle.clone(),
        conversation_id,
        first_message_id,
        first_window.clone(),
        operation_state,
        permission_manager,
    );
    let client_handle = client_impl.clone();

    let local_set = tokio::task::LocalSet::new();
    let session_result = local_set
        .run_until(async move {
            info!("ACP: Creating ClientSideConnection...");
            let (conn, handle_io) = ClientSideConnection::new(
                client_impl,
                stdin.compat_write(),
                stdout.compat(),
                |fut| {
                    tokio::task::spawn_local(fut);
                },
            );

            let _io_handle = tokio::task::spawn_local(async move {
                info!("ACP I/O: Starting I/O handler...");
                let _ = handle_io.await;
                info!("ACP I/O: I/O handler finished");
            });
            info!("ACP: I/O handler spawned");

            let _stderr_task = tokio::task::spawn_local(async move {
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

            info!("ACP: Initializing connection (timeout: 30s)...");
            let init_response = tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                conn.initialize(
                    acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                        .client_info(acp::Implementation::new("AIPP", "0.4.3"))
                        .client_capabilities(
                            acp::ClientCapabilities::new()
                                .fs(
                                    acp::FileSystemCapabilities::new()
                                        .read_text_file(true)
                                        .write_text_file(true),
                                )
                                .terminal(true),
                        ),
                ),
            )
            .await;

            let init_response = match init_response {
                Ok(result) => result,
                Err(_) => {
                    let err_msg = "ACP initialize timed out after 30 seconds. The CLI might not support ACP protocol or needs '--mcp' flag.".to_string();
                    error!("ACP: {}", err_msg);
                    client_handle.send_error_event(&err_msg).await;
                    return Err(AppError::UnknownError(err_msg));
                }
            };

            let init_response = match init_response {
                Ok(response) => response,
                Err(error) => {
                    let err_msg = format!("ACP initialize failed: {error:?}");
                    error!("ACP: {}", err_msg);
                    client_handle.send_error_event(&err_msg).await;
                    return Err(AppError::UnknownError(err_msg));
                }
            };
            info!(
                "ACP: Initialize success, protocol_version={:?}",
                init_response.protocol_version
            );
            info!(
                "ACP: Agent capabilities load_session={}",
                init_response.agent_capabilities.load_session
            );

            let conversation_db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
            let mut session_id: Option<String> = None;
            let mut should_build_history_fallback = false;
            let mut initial_modes: Option<acp::SessionModeState> = None;
            let mut initial_config_options: Option<Vec<acp::SessionConfigOption>> = None;

            if let Some(stored_session_id) = conversation_db.get_acp_session_id(conversation_id)? {
                if init_response.agent_capabilities.load_session {
                    info!(
                        "ACP: Loading existing session (conversation_id={}, session_id={})",
                        conversation_id, stored_session_id
                    );
                    client_handle.set_suppress_updates(true).await;
                    let load_result = conn
                        .load_session(acp::LoadSessionRequest::new(
                            stored_session_id.clone(),
                            acp_config.working_directory.clone(),
                        ))
                        .await;
                    client_handle.set_suppress_updates(false).await;

                    match load_result {
                        Ok(response) => {
                            initial_modes = response.modes.clone();
                            initial_config_options = response.config_options.clone();
                            session_id = Some(stored_session_id);
                            conversation_db.upsert_acp_session_id(
                                conversation_id,
                                session_id.as_deref().unwrap_or_default(),
                            )?;
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
                } else {
                    info!(
                        "ACP: Agent does not support loadSession; creating new session (conversation_id={})",
                        conversation_id
                    );
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
                let session_response = conn
                    .new_session(acp::NewSessionRequest::new(acp_config.working_directory.clone()))
                    .await;

                let session_response = match session_response {
                    Ok(response) => response,
                    Err(error) => {
                        let err_msg = format!("ACP new_session failed: {error:?}");
                        error!("ACP: {}", err_msg);
                        client_handle.send_error_event(&err_msg).await;
                        return Err(AppError::UnknownError(err_msg));
                    }
                };
                initial_modes = session_response.modes.clone();
                initial_config_options = session_response.config_options.clone();
                let session_id = session_response.session_id.to_string();
                info!("ACP: Session created, session_id={:?}", session_id);

                conversation_db.upsert_acp_session_id(conversation_id, &session_id)?;
                session_id
            };

            client_handle
                .set_session_bootstrap(
                    &session_id,
                    initial_modes.as_ref(),
                    initial_config_options.as_deref(),
                )
                .await;

            if let Some(model_id) =
                get_claude_model_override(&acp_config.cli_command, &acp_config.env_vars)
            {
                info!(
                    "ACP: Applying Claude session model override from ANTHROPIC_MODEL={}",
                    model_id
                );
                let set_model_result = conn
                    .set_session_model(acp::SetSessionModelRequest::new(
                        session_id.clone(),
                        model_id.clone(),
                    ))
                    .await;

                if let Err(error) = &set_model_result {
                    let err_msg = format!(
                        "ACP set_session_model failed for Claude model '{}': {:?}",
                        model_id, error
                    );
                    error!("ACP: {}", err_msg);
                    client_handle.send_error_event(&err_msg).await;
                    return Err(AppError::UnknownError(err_msg));
                }

                info!("ACP: Claude session model override applied: {}", model_id);
            }

            let history_fallback_prompt = if should_build_history_fallback {
                build_acp_history_prompt(&app_handle, conversation_id)
            } else {
                None
            };

            let conn = Rc::new(conn);
            let mut prompt_queue = VecDeque::from([(
                first_message_id,
                first_prompt,
                first_window,
                history_fallback_prompt,
            )]);
            let mut active_prompt: Option<oneshot::Receiver<Result<(), AppError>>> = None;

            loop {
                if active_prompt.is_none() {
                    if let Some((message_id, prompt, window, history_prefix)) = prompt_queue.pop_front()
                    {
                        client_handle.set_active_prompt(true).await;
                        let conn = Rc::clone(&conn);
                        let client_handle_clone = client_handle.clone();
                        let session_id_clone = session_id.clone();
                        let (done_tx, done_rx) = oneshot::channel();
                        tokio::task::spawn_local(async move {
                            let result = process_acp_prompt(
                                &client_handle_clone,
                                conn.as_ref(),
                                &session_id_clone,
                                conversation_id,
                                message_id,
                                prompt,
                                window,
                                history_prefix,
                            )
                            .await;
                            let _ = done_tx.send(result);
                        });
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
                                Some(AcpSessionCommand::Prompt { message_id, prompt, window }) => {
                                    prompt_queue.push_back((message_id, prompt, window, None));
                                }
                                Some(AcpSessionCommand::CancelCurrentPrompt { response }) => {
                                    prompt_queue.clear();
                                    if let Some(permission_state) = app_handle.try_state::<AcpPermissionState>() {
                                        let resolutions =
                                            permission_state.cancel_requests_for_conversation(conversation_id).await;
                                        notify_cancelled_acp_permission_requests(&app_handle, resolutions).await;
                                    }
                                    let result = conn
                                        .cancel(acp::CancelNotification::new(session_id.clone()))
                                        .await
                                        .map_err(|error| format!("ACP cancel failed: {error:?}"));
                                    let _ = response.send(result);
                                }
                                Some(AcpSessionCommand::SetMode { mode_id, response }) => {
                                    let result = conn
                                        .set_session_mode(acp::SetSessionModeRequest::new(
                                            session_id.clone(),
                                            mode_id.clone(),
                                        ))
                                        .await
                                        .map_err(|error| format!("ACP set_session_mode failed: {error:?}"));
                                    if result.is_ok() {
                                        let state = client_handle
                                            .update_session_state(|snapshot| {
                                                snapshot.current_mode_id = Some(mode_id.clone());
                                            })
                                            .await;
                                        client_handle.publish_session_state(state).await;
                                    }
                                    let _ = response.send(result.map(|_| ()));
                                }
                                Some(AcpSessionCommand::SetConfigOption { config_id, value, response }) => {
                                    let result = conn
                                        .set_session_config_option(acp::SetSessionConfigOptionRequest::new(
                                            session_id.clone(),
                                            config_id.clone(),
                                            value.clone(),
                                        ))
                                        .await
                                        .map_err(|error| format!("ACP set_session_config_option failed: {error:?}"));
                                    if result.is_ok() {
                                        let state = client_handle
                                            .update_session_state(|snapshot| {
                                                if let Some(option) = snapshot
                                                    .config_options
                                                    .iter_mut()
                                                    .find(|option| option.id == config_id)
                                                {
                                                    option.current_value = value.clone();
                                                }
                                            })
                                            .await;
                                        client_handle.publish_session_state(state).await;
                                    }
                                    let _ = response.send(result.map(|_| ()));
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
                    }
                } else {
                    match receiver.recv().await {
                        Some(AcpSessionCommand::Prompt { message_id, prompt, window }) => {
                            prompt_queue.push_back((message_id, prompt, window, None));
                        }
                        Some(AcpSessionCommand::CancelCurrentPrompt { response }) => {
                            let _ = response.send(Ok(()));
                        }
                        Some(AcpSessionCommand::SetMode { mode_id, response }) => {
                            let result = conn
                                .set_session_mode(acp::SetSessionModeRequest::new(
                                    session_id.clone(),
                                    mode_id.clone(),
                                ))
                                .await
                                .map_err(|error| format!("ACP set_session_mode failed: {error:?}"));
                            if result.is_ok() {
                                let state = client_handle
                                    .update_session_state(|snapshot| {
                                        snapshot.current_mode_id = Some(mode_id.clone());
                                    })
                                    .await;
                                client_handle.publish_session_state(state).await;
                            }
                            let _ = response.send(result.map(|_| ()));
                        }
                        Some(AcpSessionCommand::SetConfigOption { config_id, value, response }) => {
                            let result = conn
                                .set_session_config_option(acp::SetSessionConfigOptionRequest::new(
                                    session_id.clone(),
                                    config_id.clone(),
                                    value.clone(),
                                ))
                                .await
                                .map_err(|error| format!("ACP set_session_config_option failed: {error:?}"));
                            if result.is_ok() {
                                let state = client_handle
                                    .update_session_state(|snapshot| {
                                        if let Some(option) = snapshot
                                            .config_options
                                            .iter_mut()
                                            .find(|option| option.id == config_id)
                                        {
                                            option.current_value = value.clone();
                                        }
                                    })
                                    .await;
                                client_handle.publish_session_state(state).await;
                            }
                            let _ = response.send(result.map(|_| ()));
                        }
                        None => break,
                    }
                }
            }

            Ok::<(), AppError>(())
        })
        .await;

    if let Err(error) = session_result {
        error!("ACP: Session failed: {}", error);
        if let Err(kill_err) = child.kill().await {
            debug!("ACP: Kill process result: {:?}", kill_err);
        }
        return Err(error);
    }

    info!("ACP: Session ended, cleaning up process");
    if let Err(error) = child.kill().await {
        debug!("ACP: Kill process result: {:?}", error);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_acp_session_state(
    acp_session_state: tauri::State<'_, crate::AcpSessionState>,
    conversation_id: i64,
) -> Result<Option<AcpConversationSessionState>, String> {
    let sessions = acp_session_state.sessions.lock().await;
    Ok(sessions.get(&conversation_id).map(|entry| entry.snapshot.clone()))
}

#[tauri::command]
pub async fn set_acp_session_mode(
    app_handle: tauri::AppHandle,
    acp_session_state: tauri::State<'_, crate::AcpSessionState>,
    conversation_id: i64,
    mode_id: String,
) -> Result<(), String> {
    let handle = {
        let sessions = acp_session_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.handle.clone())
    }
    .ok_or_else(|| "ACP session not found".to_string())?;

    handle.set_mode(mode_id.clone()).await.map_err(|error| error.to_string())?;

    let state = {
        let mut sessions = acp_session_state.sessions.lock().await;
        if let Some(entry) = sessions.get_mut(&conversation_id) {
            entry.snapshot.current_mode_id = Some(mode_id.clone());
            Some(entry.snapshot.clone())
        } else {
            None
        }
    };
    emit_acp_session_state_snapshot(&app_handle, conversation_id, state);
    Ok(())
}

#[tauri::command]
pub async fn set_acp_session_config_option(
    app_handle: tauri::AppHandle,
    acp_session_state: tauri::State<'_, crate::AcpSessionState>,
    conversation_id: i64,
    config_id: String,
    value: String,
) -> Result<(), String> {
    let handle = {
        let sessions = acp_session_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.handle.clone())
    }
    .ok_or_else(|| "ACP session not found".to_string())?;

    handle
        .set_config_option(config_id.clone(), value.clone())
        .await
        .map_err(|error| error.to_string())?;

    let state = {
        let mut sessions = acp_session_state.sessions.lock().await;
        if let Some(entry) = sessions.get_mut(&conversation_id) {
            if let Some(option) = entry
                .snapshot
                .config_options
                .iter_mut()
                .find(|option| option.id == config_id)
            {
                option.current_value = value;
            }
            Some(entry.snapshot.clone())
        } else {
            None
        }
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
        provider_configs.iter().find(|c| c.name == name).map(|c| c.value.clone())
    };

    // Helper to get value from model_configs
    let get_model_config = |name: &str| -> Option<String> {
        model_configs.iter().find(|c| c.name == name).and_then(|c| c.value.clone())
    };

    // 获取 CLI 命令
    // 只从 llm_provider_config 获取，因为这是提供商级别的配置
    // 注意：不同的 agent 需要不同的命令：
    // - Claude Code: 需要安装 @zed-industries/claude-code-acp，命令是 "claude-code-acp"
    // - Codex: 需要安装 @zed-industries/codex-acp，命令是 "codex-acp"
    // - Gemini: 原生支持 ACP，命令是 "gemini"
    let cli_command =
        get_provider_config("acp_cli_command").unwrap_or_else(|| "claude-code-acp".to_string());

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

    Ok(AcpConfig { cli_command, working_directory, env_vars, additional_args })
}

#[cfg(test)]
mod tests {
    use super::{
        append_buffered_content, apply_network_proxy_to_env_vars, build_acp_launch_plan,
        build_prompt_to_send, extract_acp_config, extract_content_text, get_claude_model_override,
        load_claude_settings_env_vars_from_path, AcpPermissionDecision, AcpPermissionRequestEvent,
        AcpPermissionState,
    };
    use crate::db::assistant_db::AssistantModelConfig;
    use crate::db::llm_db::LLMProviderConfig;
    use agent_client_protocol::{self as acp, Agent as _, Client as _};
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use tempfile::NamedTempFile;
    use tokio::io::duplex;
    use tokio::sync::{oneshot, Notify};
    use tokio::time::{timeout, Duration};
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

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

    #[async_trait::async_trait(?Send)]
    impl acp::Client for RecordingClient {
        async fn request_permission(
            &self,
            _args: acp::RequestPermissionRequest,
        ) -> acp::Result<acp::RequestPermissionResponse> {
            Err(acp::Error::method_not_found())
        }

        async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
            self.session_notifications.lock().unwrap().push(args);
            Ok(())
        }
    }

    #[async_trait::async_trait(?Send)]
    impl acp::Agent for FakeStreamingAgent {
        async fn initialize(
            &self,
            args: acp::InitializeRequest,
        ) -> acp::Result<acp::InitializeResponse> {
            Ok(acp::InitializeResponse::new(args.protocol_version).agent_info(
                acp::Implementation::new("fake-acp-agent", "0.0.0").title("Fake ACP Agent"),
            ))
        }

        async fn authenticate(
            &self,
            _args: acp::AuthenticateRequest,
        ) -> acp::Result<acp::AuthenticateResponse> {
            Ok(acp::AuthenticateResponse::default())
        }

        async fn new_session(
            &self,
            _args: acp::NewSessionRequest,
        ) -> acp::Result<acp::NewSessionResponse> {
            Ok(acp::NewSessionResponse::new("test-session"))
        }

        async fn prompt(&self, args: acp::PromptRequest) -> acp::Result<acp::PromptResponse> {
            self.prompts_received.lock().unwrap().push((args.session_id, args.prompt));
            self.prompt_started.notify_one();
            self.release_prompt.notified().await;
            Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
        }

        async fn cancel(&self, _args: acp::CancelNotification) -> acp::Result<()> {
            Ok(())
        }
    }

    fn create_connection_pair(
        client: RecordingClient,
        agent: FakeStreamingAgent,
    ) -> (acp::ClientSideConnection, acp::AgentSideConnection) {
        let (client_writer, agent_reader) = duplex(4096);
        let (agent_writer, client_reader) = duplex(4096);

        let (client_conn, client_io_task) = acp::ClientSideConnection::new(
            client,
            client_writer.compat_write(),
            client_reader.compat(),
            |fut| {
                tokio::task::spawn_local(fut);
            },
        );
        let (agent_conn, agent_io_task) = acp::AgentSideConnection::new(
            agent,
            agent_writer.compat_write(),
            agent_reader.compat(),
            |fut| {
                tokio::task::spawn_local(fut);
            },
        );

        tokio::task::spawn_local(async move {
            let _ = client_io_task.await;
        });
        tokio::task::spawn_local(async move {
            let _ = agent_io_task.await;
        });

        (client_conn, agent_conn)
    }

    #[test]
    fn extract_acp_config_parses_multiline_env_blob_and_model_overrides() {
        let provider_configs = vec![
            provider_config("acp_cli_command", "codex-acp"),
            provider_config("acp_env_vars", "FOO=provider\nSHARED=provider"),
        ];
        let model_configs =
            vec![model_config("acp_env_vars", "BAR=model\nSHARED=model\n# COMMENT\nINVALID")];

        let config = extract_acp_config(&model_configs, &provider_configs).unwrap();

        assert_eq!(config.env_vars.get("FOO"), Some(&"provider".to_string()));
        assert_eq!(config.env_vars.get("BAR"), Some(&"model".to_string()));
        assert_eq!(config.env_vars.get("SHARED"), Some(&"model".to_string()));
        assert!(!config.env_vars.contains_key("INVALID"));
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
    fn build_acp_launch_plan_uses_standard_env_for_non_claude_cli() {
        let env_vars =
            HashMap::from([("HTTPS_PROXY".to_string(), "http://127.0.0.1:7897".to_string())]);
        let plan = build_acp_launch_plan(
            "codex-acp",
            Path::new("/tmp/codex-acp"),
            &["--foo".to_string()],
            &env_vars,
        );

        assert_eq!(plan.proxy_strategy, "standard-env");
        assert_eq!(plan.program, PathBuf::from("/tmp/codex-acp"));
        assert_eq!(plan.args, vec!["--foo".to_string()]);
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
        assert_eq!(get_claude_model_override("codex-acp", &env_vars), None);

        env_vars.insert("ANTHROPIC_MODEL".to_string(), "default".to_string());
        assert_eq!(get_claude_model_override("claude-code-acp", &env_vars), None);
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

    #[test]
    fn build_prompt_to_send_prepends_history_prefix() {
        let prompt = build_prompt_to_send(
            "Summarize the workspace".to_string(),
            Some("历史摘要".to_string()),
        );

        assert_eq!(prompt, "历史摘要\n\n当前用户请求:\nSummarize the workspace");
    }

    #[tokio::test]
    async fn fake_agent_prompt_flow_streams_notifications_while_prompt_is_pending() {
        let local_set = tokio::task::LocalSet::new();
        local_set
            .run_until(async {
                let client = RecordingClient::new();
                let fake_agent = FakeStreamingAgent::new();
                let (client_conn, agent_conn) =
                    create_connection_pair(client.clone(), fake_agent.clone());

                client_conn
                    .initialize(
                        acp::InitializeRequest::new(acp::ProtocolVersion::LATEST).client_info(
                            acp::Implementation::new("aipp-test-client", "0.0.0")
                                .title("AIPP Test Client"),
                        ),
                    )
                    .await
                    .unwrap();

                let session = client_conn
                    .new_session(acp::NewSessionRequest::new(std::env::temp_dir()))
                    .await
                    .unwrap();
                let session_id = session.session_id.clone();
                let prompt_text = build_prompt_to_send(
                    "Summarize the workspace".to_string(),
                    Some("历史摘要".to_string()),
                );

                let prompt_task = tokio::task::spawn_local({
                    let session_id = session_id.clone();
                    async move {
                        client_conn
                            .prompt(acp::PromptRequest::new(session_id, vec![prompt_text.into()]))
                            .await
                    }
                });

                timeout(Duration::from_secs(5), fake_agent.prompt_started.notified())
                    .await
                    .unwrap();
                assert!(!prompt_task.is_finished());

                {
                    let prompts = fake_agent.prompts_received.lock().unwrap();
                    assert_eq!(prompts.len(), 1);
                    assert_eq!(
                        extract_content_text(&prompts[0].1[0]),
                        "历史摘要\n\n当前用户请求:\nSummarize the workspace"
                    );
                }

                agent_conn
                    .session_notification(acp::SessionNotification::new(
                        session_id.clone(),
                        acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
                            "thinking ".into(),
                        )),
                    ))
                    .await
                    .unwrap();
                agent_conn
                    .session_notification(acp::SessionNotification::new(
                        session_id.clone(),
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            "Hello ".into(),
                        )),
                    ))
                    .await
                    .unwrap();
                agent_conn
                    .session_notification(acp::SessionNotification::new(
                        session_id.clone(),
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            "world".into(),
                        )),
                    ))
                    .await
                    .unwrap();

                tokio::task::yield_now().await;
                assert!(!prompt_task.is_finished());

                fake_agent.release_prompt.notify_one();

                timeout(Duration::from_secs(5), prompt_task).await.unwrap().unwrap().unwrap();

                let notifications = client.session_notifications.lock().unwrap().clone();
                let mut reasoning = String::new();
                let mut response = String::new();

                for notification in notifications {
                    match notification.update {
                        acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk {
                            content,
                            ..
                        }) => {
                            append_buffered_content(&mut reasoning, &content);
                        }
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
                            content,
                            ..
                        }) => {
                            append_buffered_content(&mut response, &content);
                        }
                        _ => {}
                    }
                }

                assert_eq!(reasoning, "thinking ");
                assert_eq!(response, "Hello world");
            })
            .await;
    }
}

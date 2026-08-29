use std::collections::HashSet;
use std::time::Duration;

use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::sync::Mutex;

// Constants
pub(super) const EXPERIMENTAL_FEATURE_CODE: &str = "experimental";
pub(super) const FEISHU_SCOPE: &str = "butler_feishu";
pub(super) const FEISHU_SECRET_KEY: &str = "app_secret";
pub(super) const SECURE_MASTER_KEY: &str = "secure_config_master_key";
pub(super) const SECURE_MASTER_KEY_FILE: &str = "secure-config-master-key.bin";
pub(super) const CHANNEL_FEISHU: &str = "feishu";
pub(super) const BOTLER_SOURCE: &str = "feishu_butler";
pub(super) const TERMINAL_TASK_STATUSES: [&str; 3] = ["succeeded", "failed", "cancelled"];
pub(super) const RELAY_ORIGIN_AIPP: &str = "aipp";
pub(super) const RELAY_ORIGIN_FEISHU: &str = "feishu";
pub(super) const RELAY_ORIGIN_INTERNAL: &str = "internal";
pub(super) const FEISHU_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const FEISHU_RUNTIME_RETRY_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const FEISHU_SETTLE_CHECK_INTERVAL: Duration = Duration::from_millis(500);
pub(super) const FEISHU_RELAY_IDLE_STABLE_CHECKS: usize = 2;
pub(super) const FEISHU_STATUS_READY_DETAIL: &str =
    "飞书 SDK 已启用心跳与内部重连；若长连接退出，AIPP 还会在 5 秒后自动重试";
pub(super) const FEISHU_MENU_NEW_CONVERSATION_EVENT_KEY: &str = "feishu::conversation::new";

// State
pub struct FeishuButlerState {
    pub(super) runtime_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub(super) http_client: reqwest::Client,
    pub(super) relay_workers: Mutex<HashSet<i64>>,
    pub ingress_lock: Mutex<()>,
    pub status: Mutex<FeishuRuntimeStatus>,
}

impl Default for FeishuButlerState {
    fn default() -> Self {
        Self {
            runtime_task: Mutex::new(None),
            http_client: super::api::build_feishu_http_client(),
            relay_workers: Mutex::new(HashSet::new()),
            ingress_lock: Mutex::new(()),
            status: Mutex::new(FeishuRuntimeStatus::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeishuRuntimeStatus {
    pub butler_enabled: bool,
    pub enabled: bool,
    pub configured: bool,
    pub secret_configured: bool,
    pub running: bool,
    pub connected: bool,
    pub app_id: Option<String>,
    pub base_url: Option<String>,
    pub allow_p2p: bool,
    pub allow_group: bool,
    pub group_require_mention: bool,
    pub last_error: Option<String>,
    pub last_event_at: Option<String>,
    pub last_status_at: Option<String>,
    pub status_detail: Option<String>,
    pub status_text: String,
}

#[derive(Debug, Clone)]
pub(super) struct FeishuRuntimeConfig {
    pub(super) butler_enabled: bool,
    pub(super) enabled: bool,
    pub(super) app_id: String,
    pub(super) app_secret: String,
    pub(super) base_url: String,
    pub(super) allow_p2p: bool,
    pub(super) allow_group: bool,
    pub(super) group_require_mention: bool,
    pub(super) only_reply_feishu_originated: bool,
    pub(super) allowed_open_ids: HashSet<String>,
    pub(super) allowed_chat_ids: HashSet<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ChannelLinkTarget {
    pub(super) reply_to_message_id: Option<String>,
    pub(super) external_chat_id: Option<String>,
    pub(super) external_user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct RelayScopeRecord {
    pub(super) id: i64,
    pub(super) channel: String,
    pub(super) conversation_id: i64,
    pub(super) origin: String,
    pub(super) external_chat_id: Option<String>,
    pub(super) external_user_id: Option<String>,
    pub(super) anchor_external_message_id: String,
    pub(super) start_after_local_message_id: i64,
    pub(super) last_delivered_local_message_id: i64,
    pub(super) status: String,
}

#[derive(Debug, Clone)]
pub(super) struct NewRelayScope<'a> {
    pub(super) channel: &'a str,
    pub(super) conversation_id: i64,
    pub(super) origin: &'a str,
    pub(super) external_chat_id: Option<&'a str>,
    pub(super) external_user_id: Option<&'a str>,
    pub(super) anchor_external_message_id: &'a str,
    pub(super) start_after_local_message_id: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct TenantAccessTokenResponse {
    pub(super) code: i32,
    pub(super) msg: String,
    pub(super) tenant_access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SendMessageResponse {
    pub(super) code: i32,
    pub(super) msg: String,
    pub(super) data: Option<SendMessageData>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SendMessageData {
    pub(super) message_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct EventEnvelope {
    pub(super) header: EventHeader,
    pub(super) event: Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct EventHeader {
    pub(super) event_type: String,
    #[serde(default)]
    pub(super) event_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EventBody {
    pub(super) sender: Sender,
    pub(super) message: FeishuMessage,
}

#[derive(Debug, Deserialize)]
pub(super) struct Sender {
    pub(super) sender_id: SenderId,
}

#[derive(Debug, Deserialize)]
pub(super) struct SenderId {
    pub(super) open_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuMessage {
    pub(super) message_id: String,
    pub(super) message_type: String,
    pub(super) content: String,
    pub(super) chat_type: String,
    #[serde(default)]
    pub(super) chat_id: Option<String>,
    #[serde(default)]
    pub(super) parent_id: Option<String>,
    #[serde(default)]
    pub(super) root_id: Option<String>,
    #[serde(default)]
    pub(super) mentions: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TextMessageContent {
    pub(super) text: String,
}

#[derive(Debug, Clone)]
pub(super) struct IncomingTextEvent {
    pub(super) message_id: String,
    pub(super) sender_open_id: String,
    pub(super) chat_id: Option<String>,
    pub(super) text: String,
    pub(super) chat_type: String,
    pub(super) parent_id: Option<String>,
    pub(super) root_id: Option<String>,
    pub(super) has_mentions: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum FeishuCardActionCallback {
    Event(FeishuCardActionEvent),
    Envelope { event: FeishuCardActionEvent },
}

impl FeishuCardActionCallback {
    pub(super) fn event(&self) -> &FeishuCardActionEvent {
        match self {
            Self::Event(event) => event,
            Self::Envelope { event } => event,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuCardActionEvent {
    pub(super) operator: FeishuCardActionOperator,
    pub(super) action: FeishuCardActionDetail,
    #[serde(default)]
    pub(super) context: Option<FeishuCardActionContext>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuCardActionOperator {
    pub(super) open_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuCardActionContext {
    #[serde(default)]
    pub(super) open_message_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuCardActionDetail {
    #[serde(default)]
    pub(super) value: Option<Value>,
    #[serde(default)]
    pub(super) form_value: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuBotMenuEvent {
    pub(super) operator: FeishuBotMenuOperator,
    pub(super) event_key: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuBotMenuOperator {
    pub(super) operator_id: FeishuBotMenuOperatorId,
}

#[derive(Debug, Deserialize)]
pub(super) struct FeishuBotMenuOperatorId {
    pub(super) open_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FeishuBotMenuClickEvent {
    pub(super) operator_open_id: String,
    pub(super) event_key: String,
}

#[derive(Debug, Clone)]
pub(super) struct ChannelLinkRecord<'a> {
    pub(super) external_message_id: &'a str,
    pub(super) external_chat_id: Option<&'a str>,
    pub(super) external_user_id: Option<&'a str>,
    pub(super) conversation_id: i64,
    pub(super) local_message_id: Option<i64>,
    pub(super) direction: &'a str,
    pub(super) payload_type: &'a str,
}

#[derive(Debug, Clone)]
pub(super) struct FeishuReplyOutcome {
    pub(super) message_id: String,
    pub(super) payload_type: &'static str,
    pub(super) interactive_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) enum FeishuCardBlock {
    Markdown(String),
    Table(FeishuMarkdownTable),
}

#[derive(Debug, Clone)]
pub(super) struct FeishuMarkdownTable {
    pub(super) headers: Vec<String>,
    pub(super) rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeishuDebugSendResult {
    pub external_message_id: String,
    pub payload_type: String,
    pub part_count: usize,
    pub interactive_part_count: usize,
    pub text_part_count: usize,
    pub delivery_mode: String,
    pub reply_to_message_id: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub rendered_text: String,
    pub interactive_error: Option<String>,
    pub interactive_card: Option<Value>,
}

// Utility types
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PermissionReplyCommand {
    Operation { review_code: String, decision: &'static str },
    AcpSelect { review_code: String, option_index: usize },
    AcpCancel { review_code: String },
}

// Utility functions that are pure helpers used across modules
pub(super) fn normalize_optional_id(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

pub(super) fn select_receive_target(target: &ChannelLinkTarget) -> Option<(&'static str, &str)> {
    if let Some(chat_id) =
        target.external_chat_id.as_deref().filter(|value| !value.trim().is_empty())
    {
        return Some(("chat_id", chat_id));
    }
    target
        .external_user_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|open_id| ("open_id", open_id))
}

pub(super) fn parse_bool_flag(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

pub(super) fn now_string() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub(super) fn split_allowlist(raw: &str) -> HashSet<String> {
    raw.split(|ch| matches!(ch, '\n' | '\r' | ',' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(super) fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

pub(super) fn normalize_review_code(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

pub(super) fn parse_permission_reply_command(text: &str) -> Option<PermissionReplyCommand> {
    let parts = text.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["批准一次" | "允许一次" | "批准" | "允许", review_code]
            if review_code.starts_with("OP-") =>
        {
            Some(PermissionReplyCommand::Operation {
                review_code: normalize_review_code(review_code),
                decision: "allow",
            })
        }
        ["本任务批准" | "本任务允许" | "对话批准" | "对话允许", review_code]
            if review_code.starts_with("OP-") =>
        {
            Some(PermissionReplyCommand::Operation {
                review_code: normalize_review_code(review_code),
                decision: "allow_for_conversation",
            })
        }
        ["助手批准" | "助手允许", review_code] if review_code.starts_with("OP-") => {
            Some(PermissionReplyCommand::Operation {
                review_code: normalize_review_code(review_code),
                decision: "allow_for_assistant",
            })
        }
        ["拒绝", review_code] if review_code.starts_with("OP-") => {
            Some(PermissionReplyCommand::Operation {
                review_code: normalize_review_code(review_code),
                decision: "deny",
            })
        }
        ["批准" | "允许", option_index, review_code] if review_code.starts_with("ACP-") => {
            let option_index = option_index.parse::<usize>().ok()?;
            if option_index == 0 {
                return None;
            }
            Some(PermissionReplyCommand::AcpSelect {
                review_code: normalize_review_code(review_code),
                option_index,
            })
        }
        ["取消", review_code] if review_code.starts_with("ACP-") => {
            Some(PermissionReplyCommand::AcpCancel {
                review_code: normalize_review_code(review_code),
            })
        }
        _ => None,
    }
}

pub(super) fn feishu_reply_matches_permission_context(
    event: &IncomingTextEvent,
    allowed_open_id: Option<&str>,
    allowed_chat_id: Option<&str>,
    feishu_message_id: Option<&str>,
) -> bool {
    if let Some(expected_open_id) = allowed_open_id {
        if event.sender_open_id != expected_open_id {
            return false;
        }
    }
    if let Some(expected_chat_id) = allowed_chat_id {
        if event.chat_id.as_deref() != Some(expected_chat_id) {
            return false;
        }
    }
    if event.chat_type != "p2p" {
        if let Some(message_id) = feishu_message_id {
            let replied_to_message = event.parent_id.as_deref() == Some(message_id)
                || event.root_id.as_deref() == Some(message_id);
            if !replied_to_message {
                return false;
            }
        }
    }
    true
}

pub(super) fn build_operation_permission_fallback_text(
    request: &crate::mcp::builtin_mcp::operation::state::PermissionRequestSnapshot,
) -> String {
    format!(
        "权限审批 {review_code}\n操作：{operation}\n路径：{path}\n\n可回复：\n- 批准一次 {review_code}\n- 本任务批准 {review_code}\n- 助手批准 {review_code}\n- 拒绝 {review_code}",
        review_code = request.review_code,
        operation = request.event.operation,
        path = truncate_text(&request.event.path, 220),
    )
}

pub(super) fn build_acp_permission_fallback_text(
    request: &crate::api::ai::acp::AcpPermissionRequestSnapshot,
) -> String {
    let mut lines = vec![format!(
        "ACP 权限审批 {review_code}\n标题：{title}\n参数：{parameters}",
        review_code = request.review_code,
        title = request.event.title.as_deref().unwrap_or("未命名"),
        parameters = truncate_text(request.event.parameters.as_deref().unwrap_or("无"), 220),
    )];
    lines.push(String::new());
    lines.push("可回复：".to_string());
    for (index, option) in request.event.options.iter().enumerate() {
        lines.push(format!("- 批准 {} {} （{}）", index + 1, request.review_code, option.name));
    }
    lines.push(format!("- 取消 {}", request.review_code));
    lines.join("\n")
}

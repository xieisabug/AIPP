use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use futures::FutureExt;
use openlark_client::ws_client::{EventDispatcherHandler, LarkWsClient};
use pulldown_cmark::{Options as MarkdownOptions, Parser as MarkdownParser};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::api::ai::acp::{AcpPermissionRequestSnapshot, AcpPermissionState};
use crate::api::ai::types::AiRequest;
use crate::api::ai_api::ask_ai;
use crate::api::butler_api::{
    get_butler_main_continuation_lock, load_or_create_butler_main_internal,
    reset_butler_main_conversation, resolve_butler_execution_window,
    wait_for_butler_main_to_be_idle,
};
use crate::api::operation_api::{confirm_acp_permission, confirm_operation_permission};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::db::system_db::{SecureConfigEntry, SystemDatabase};
use crate::external_channels::presentation::{render_message_for_external_channel, RenderContext};
use crate::mcp::builtin_mcp::interaction::{
    resolve_ask_user_question_response, AskUserQuestionItem, AskUserQuestionRequest,
    AskUserQuestionRequestEvent,
};
use crate::mcp::builtin_mcp::operation::state::PermissionRequestSnapshot;

const EXPERIMENTAL_FEATURE_CODE: &str = "experimental";
const FEISHU_SCOPE: &str = "butler_feishu";
const FEISHU_SECRET_KEY: &str = "app_secret";
const SECURE_MASTER_KEY: &str = "secure_config_master_key";
const CHANNEL_FEISHU: &str = "feishu";
const BOTLER_SOURCE: &str = "feishu_butler";
const TERMINAL_TASK_STATUSES: [&str; 3] = ["succeeded", "failed", "cancelled"];
const RELAY_ORIGIN_AIPP: &str = "aipp";
const RELAY_ORIGIN_FEISHU: &str = "feishu";
const RELAY_ORIGIN_INTERNAL: &str = "internal";
const FEISHU_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const FEISHU_RUNTIME_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const FEISHU_SETTLE_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const FEISHU_SETTLE_TIMEOUT: Duration = Duration::from_secs(120);
const FEISHU_SETTLE_STATUS_INTERVAL_STEPS: usize = 10;
const FEISHU_RELAY_IDLE_STABLE_CHECKS: usize = 2;
const FEISHU_STATUS_READY_DETAIL: &str =
    "飞书 SDK 已启用心跳与内部重连；若长连接退出，AIPP 还会在 5 秒后自动重试";
const FEISHU_MENU_NEW_CONVERSATION_EVENT_KEY: &str = "feishu::conversation::new";

pub struct FeishuButlerState {
    runtime_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    http_client: reqwest::Client,
    relay_workers: Mutex<HashSet<i64>>,
    pub ingress_lock: Mutex<()>,
    pub status: Mutex<FeishuRuntimeStatus>,
}

impl Default for FeishuButlerState {
    fn default() -> Self {
        Self {
            runtime_task: Mutex::new(None),
            http_client: build_feishu_http_client(),
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
struct FeishuRuntimeConfig {
    butler_enabled: bool,
    enabled: bool,
    app_id: String,
    app_secret: String,
    base_url: String,
    allow_p2p: bool,
    allow_group: bool,
    group_require_mention: bool,
    only_reply_feishu_originated: bool,
    allowed_open_ids: HashSet<String>,
    allowed_chat_ids: HashSet<String>,
}

#[derive(Debug, Clone)]
struct ChannelLinkTarget {
    reply_to_message_id: Option<String>,
    external_chat_id: Option<String>,
    external_user_id: Option<String>,
}

#[derive(Debug, Clone)]
struct RelayScopeRecord {
    id: i64,
    channel: String,
    conversation_id: i64,
    origin: String,
    external_chat_id: Option<String>,
    external_user_id: Option<String>,
    anchor_external_message_id: String,
    start_after_local_message_id: i64,
    last_delivered_local_message_id: i64,
    status: String,
}

#[derive(Debug, Clone)]
struct NewRelayScope<'a> {
    channel: &'a str,
    conversation_id: i64,
    origin: &'a str,
    external_chat_id: Option<&'a str>,
    external_user_id: Option<&'a str>,
    anchor_external_message_id: &'a str,
    start_after_local_message_id: i64,
}

#[derive(Debug, Deserialize)]
struct TenantAccessTokenResponse {
    code: i32,
    msg: String,
    tenant_access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    code: i32,
    msg: String,
    data: Option<SendMessageData>,
}

#[derive(Debug, Deserialize)]
struct SendMessageData {
    message_id: String,
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    header: EventHeader,
    event: Value,
}

#[derive(Debug, Deserialize)]
struct EventHeader {
    event_type: String,
    #[serde(default)]
    event_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventBody {
    sender: Sender,
    message: FeishuMessage,
}

#[derive(Debug, Deserialize)]
struct Sender {
    sender_id: SenderId,
}

#[derive(Debug, Deserialize)]
struct SenderId {
    open_id: String,
}

#[derive(Debug, Deserialize)]
struct FeishuMessage {
    message_id: String,
    message_type: String,
    content: String,
    chat_type: String,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    root_id: Option<String>,
    #[serde(default)]
    mentions: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct TextMessageContent {
    text: String,
}

#[derive(Debug, Clone)]
struct IncomingTextEvent {
    message_id: String,
    sender_open_id: String,
    chat_id: Option<String>,
    text: String,
    chat_type: String,
    parent_id: Option<String>,
    root_id: Option<String>,
    has_mentions: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FeishuCardActionCallback {
    Event(FeishuCardActionEvent),
    Envelope { event: FeishuCardActionEvent },
}

impl FeishuCardActionCallback {
    fn event(&self) -> &FeishuCardActionEvent {
        match self {
            Self::Event(event) => event,
            Self::Envelope { event } => event,
        }
    }
}

#[derive(Debug, Deserialize)]
struct FeishuCardActionEvent {
    operator: FeishuCardActionOperator,
    action: FeishuCardActionDetail,
    #[serde(default)]
    context: Option<FeishuCardActionContext>,
}

#[derive(Debug, Deserialize)]
struct FeishuCardActionOperator {
    open_id: String,
}

#[derive(Debug, Deserialize)]
struct FeishuCardActionContext {
    #[serde(default)]
    open_message_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FeishuCardActionDetail {
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    form_value: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize)]
struct FeishuBotMenuEvent {
    operator: FeishuBotMenuOperator,
    event_key: String,
}

#[derive(Debug, Deserialize)]
struct FeishuBotMenuOperator {
    operator_id: FeishuBotMenuOperatorId,
}

#[derive(Debug, Deserialize)]
struct FeishuBotMenuOperatorId {
    open_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeishuBotMenuClickEvent {
    operator_open_id: String,
    event_key: String,
}

#[derive(Debug, Clone)]
struct ChannelLinkRecord<'a> {
    external_message_id: &'a str,
    external_chat_id: Option<&'a str>,
    external_user_id: Option<&'a str>,
    conversation_id: i64,
    local_message_id: Option<i64>,
    direction: &'a str,
    payload_type: &'a str,
}

#[derive(Debug, Clone)]
struct FeishuReplyOutcome {
    message_id: String,
    payload_type: &'static str,
    interactive_error: Option<String>,
    interactive_card: Option<Value>,
}

#[derive(Debug, Clone)]
enum FeishuCardBlock {
    Markdown(String),
    Table(FeishuMarkdownTable),
}

#[derive(Debug, Clone)]
struct FeishuMarkdownTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeishuDebugSendResult {
    pub external_message_id: String,
    pub payload_type: String,
    pub delivery_mode: String,
    pub reply_to_message_id: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub rendered_text: String,
    pub interactive_error: Option<String>,
    pub interactive_card: Option<Value>,
}

fn normalize_optional_id(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn select_receive_target(target: &ChannelLinkTarget) -> Option<(&'static str, &str)> {
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

fn parse_bool_flag(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn split_allowlist(raw: &str) -> HashSet<String> {
    raw.split(|ch| matches!(ch, '\n' | '\r' | ',' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionReplyCommand {
    Operation { review_code: String, decision: &'static str },
    AcpSelect { review_code: String, option_index: usize },
    AcpCancel { review_code: String },
}

fn normalize_review_code(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn parse_permission_reply_command(text: &str) -> Option<PermissionReplyCommand> {
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

fn feishu_reply_matches_permission_context(
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

fn build_operation_permission_fallback_text(request: &PermissionRequestSnapshot) -> String {
    format!(
        "权限审批 {review_code}\n操作：{operation}\n路径：{path}\n\n可回复：\n- 批准一次 {review_code}\n- 本任务批准 {review_code}\n- 助手批准 {review_code}\n- 拒绝 {review_code}",
        review_code = request.review_code,
        operation = request.event.operation,
        path = truncate_text(&request.event.path, 220),
    )
}

fn build_acp_permission_fallback_text(request: &AcpPermissionRequestSnapshot) -> String {
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

fn build_status(config: &FeishuRuntimeConfig) -> FeishuRuntimeStatus {
    FeishuRuntimeStatus {
        butler_enabled: config.butler_enabled,
        enabled: config.enabled,
        configured: !config.app_id.trim().is_empty() && !config.app_secret.trim().is_empty(),
        secret_configured: !config.app_secret.trim().is_empty(),
        running: false,
        connected: false,
        app_id: (!config.app_id.trim().is_empty()).then(|| config.app_id.clone()),
        base_url: Some(config.base_url.clone()),
        allow_p2p: config.allow_p2p,
        allow_group: config.allow_group,
        group_require_mention: config.group_require_mention,
        last_error: None,
        last_event_at: None,
        last_status_at: Some(now_string()),
        status_detail: None,
        status_text: "飞书机器人未启动".to_string(),
    }
}

fn build_feishu_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(FEISHU_HTTP_TIMEOUT)
        .build()
        .expect("failed to build Feishu HTTP client")
}

fn feishu_http_client(app_handle: &AppHandle) -> reqwest::Client {
    app_handle.state::<FeishuButlerState>().http_client.clone()
}

async fn mark_relay_scope_failed_with_log(
    app_handle: &AppHandle,
    scope_id: i64,
    error_message: &str,
    context: &str,
) {
    if let Err(mark_error) = mark_relay_scope_failed(app_handle, scope_id, error_message) {
        error!(
            scope_id,
            relay_error = %error_message,
            mark_error = %mark_error,
            "{context}"
        );
    }
}

async fn set_feishu_runtime_ready_status(app_handle: &AppHandle, detail: impl Into<String>) {
    let detail = detail.into();
    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "飞书机器人已连接，等待消息".to_string();
        status.status_detail = Some(detail);
    })
    .await;
}

async fn replace_status(app_handle: &AppHandle, status: FeishuRuntimeStatus) {
    let state = app_handle.state::<FeishuButlerState>();
    let mut snapshot = status;
    snapshot.last_status_at = Some(now_string());
    *state.status.lock().await = snapshot.clone();
    let _ = app_handle.emit("butler_feishu_status_changed", snapshot);
}

async fn mutate_status<F>(app_handle: &AppHandle, apply: F)
where
    F: FnOnce(&mut FeishuRuntimeStatus),
{
    let state = app_handle.state::<FeishuButlerState>();
    let mut status = state.status.lock().await;
    apply(&mut status);
    status.last_status_at = Some(now_string());
    let snapshot = status.clone();
    drop(status);
    let _ = app_handle.emit("butler_feishu_status_changed", snapshot);
}

async fn spawn_feishu_relay_scope_worker(
    app_handle: &AppHandle,
    scope_id: i64,
    conversation_id: i64,
) {
    let state = app_handle.state::<FeishuButlerState>();
    let mut relay_workers = state.relay_workers.lock().await;
    if !relay_workers.insert(scope_id) {
        return;
    }
    drop(relay_workers);

    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_feishu_relay_scope_worker(&app_handle, scope_id, conversation_id).await;
        let state = app_handle.state::<FeishuButlerState>();
        let mut relay_workers = state.relay_workers.lock().await;
        relay_workers.remove(&scope_id);
        drop(relay_workers);

        if let Err(error) = result {
            mark_relay_scope_failed_with_log(
                &app_handle,
                scope_id,
                &error,
                "failed to persist Feishu relay worker error",
            )
            .await;
            warn!(conversation_id, scope_id, error = %error, "feishu relay worker failed");
        }
    });
}

async fn run_feishu_relay_scope_worker(
    app_handle: &AppHandle,
    scope_id: i64,
    conversation_id: i64,
) -> Result<(), String> {
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let mut idle_checks = 0usize;

    loop {
        let scope = load_relay_scope(app_handle, scope_id)?;
        if matches!(scope.status.as_str(), "completed" | "failed") {
            return Ok(());
        }

        let fresh_config = load_runtime_config(app_handle).await?;
        if !fresh_config.butler_enabled || !fresh_config.enabled {
            return Err("飞书回发已被停用，跳过当前回发任务".to_string());
        }

        flush_feishu_relay_scope(app_handle, &fresh_config, scope_id).await?;

        let scope = load_relay_scope(app_handle, scope_id)?;
        let runtime_state = activity_manager.get_runtime_state(conversation_id).await;
        let pending_tasks = count_pending_butler_tasks(app_handle, conversation_id)?;
        let latest_message_id = get_latest_message_id(app_handle, conversation_id)?;
        let no_new_messages = latest_message_id <= scope.last_delivered_local_message_id;

        if !runtime_state.is_running && pending_tasks == 0 && no_new_messages {
            idle_checks += 1;
            if idle_checks >= FEISHU_RELAY_IDLE_STABLE_CHECKS {
                mark_relay_scope_progress(
                    app_handle,
                    scope_id,
                    scope.last_delivered_local_message_id,
                    "completed",
                )?;
                if scope.origin == RELAY_ORIGIN_FEISHU {
                    set_feishu_runtime_ready_status(
                        app_handle,
                        format!("最近一条飞书消息处理链路已稳定结束；{FEISHU_STATUS_READY_DETAIL}"),
                    )
                    .await;
                }
                return Ok(());
            }
        } else {
            idle_checks = 0;
            let next_status = if runtime_state.is_running && pending_tasks == 0 && no_new_messages {
                "waiting_user_input"
            } else {
                "active"
            };
            mark_relay_scope_progress(
                app_handle,
                scope_id,
                scope.last_delivered_local_message_id,
                next_status,
            )?;
            if scope.origin == RELAY_ORIGIN_FEISHU {
                mutate_status(app_handle, |status| {
                    status.running = true;
                    status.connected = true;
                    status.last_error = None;
                    status.status_text = "总管家正在持续回发飞书消息".to_string();
                    status.status_detail = Some(format!(
                        "飞书消息已受理；会话运行中={}，待完成任务={}，已回发到消息 {}",
                        runtime_state.is_running,
                        pending_tasks,
                        scope.last_delivered_local_message_id
                    ));
                })
                .await;
            }
        }

        sleep(FEISHU_SETTLE_CHECK_INTERVAL).await;
    }
}

fn format_panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn get_or_create_master_key(app_handle: &AppHandle) -> Result<[u8; 32], String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let existing = db.get_config(SECURE_MASTER_KEY).map_err(|e| e.to_string())?;
    let key_b64 = if existing.trim().is_empty() {
        let bytes: [u8; 32] = rand::random();
        let encoded = BASE64.encode(bytes);
        match db.add_system_config(SECURE_MASTER_KEY, &encoded) {
            Ok(_) => encoded,
            Err(_) => {
                db.update_system_config(SECURE_MASTER_KEY, &encoded).map_err(|e| e.to_string())?;
                encoded
            }
        }
    } else {
        existing
    };
    let decoded = BASE64.decode(key_b64).map_err(|e| e.to_string())?;
    decoded.try_into().map_err(|_| "Invalid secure config master key length".to_string())
}

fn encrypt_secret(app_handle: &AppHandle, plaintext: &str) -> Result<(String, String), String> {
    let key_bytes = get_or_create_master_key(app_handle)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Failed to encrypt secret: {e}"))?;
    Ok((BASE64.encode(encrypted), BASE64.encode(nonce_bytes)))
}

fn decrypt_secret(app_handle: &AppHandle, ciphertext: &str, nonce: &str) -> Result<String, String> {
    let key_bytes = get_or_create_master_key(app_handle)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes = BASE64.decode(nonce).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = BASE64.decode(ciphertext).map_err(|e| e.to_string())?;
    let decrypted = cipher
        .decrypt(nonce, encrypted.as_ref())
        .map_err(|e| format!("Failed to decrypt secret: {e}"))?;
    String::from_utf8(decrypted).map_err(|e| e.to_string())
}

pub(crate) fn save_feishu_secret(app_handle: &AppHandle, app_secret: &str) -> Result<(), String> {
    if app_secret.trim().is_empty() {
        return Err("飞书 App Secret 不能为空".to_string());
    }
    let (ciphertext, nonce) = encrypt_secret(app_handle, app_secret.trim())?;
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.upsert_secure_config(&SecureConfigEntry {
        scope: FEISHU_SCOPE.to_string(),
        key: FEISHU_SECRET_KEY.to_string(),
        ciphertext,
        nonce,
        updated_time: None,
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn clear_feishu_secret(app_handle: &AppHandle) -> Result<(), String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.delete_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY).map_err(|e| e.to_string())
}

fn load_feishu_secret(app_handle: &AppHandle) -> Result<Option<String>, String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let Some(entry) =
        db.get_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(decrypt_secret(app_handle, &entry.ciphertext, &entry.nonce)?))
}

async fn load_runtime_config(app_handle: &AppHandle) -> Result<FeishuRuntimeConfig, String> {
    let feature_state = app_handle.state::<crate::FeatureConfigState>();
    let guard = feature_state.config_feature_map.lock().await;
    let experimental = guard.get(EXPERIMENTAL_FEATURE_CODE);
    let get = |key: &str| -> String {
        experimental
            .and_then(|map| map.get(key))
            .map(|config| config.value.clone())
            .unwrap_or_default()
    };
    let app_secret = load_feishu_secret(app_handle)?.unwrap_or_default();
    Ok(FeishuRuntimeConfig {
        butler_enabled: parse_bool_flag(&get("butler_experiment_enabled")),
        enabled: parse_bool_flag(&get("butler_feishu_enabled")),
        app_id: get("butler_feishu_app_id"),
        app_secret,
        base_url: {
            let value = get("butler_feishu_base_url");
            if value.trim().is_empty() {
                "https://open.feishu.cn".to_string()
            } else {
                value
            }
        },
        allow_p2p: !matches!(get("butler_feishu_receive_p2p").as_str(), "false" | "0"),
        allow_group: !matches!(get("butler_feishu_receive_group").as_str(), "false" | "0"),
        group_require_mention: !matches!(
            get("butler_feishu_group_require_mention").as_str(),
            "false" | "0"
        ),
        only_reply_feishu_originated: matches!(
            get("butler_feishu_only_reply_feishu_originated").as_str(),
            "true" | "1"
        ),
        allowed_open_ids: split_allowlist(&get("butler_feishu_allowed_open_ids")),
        allowed_chat_ids: split_allowlist(&get("butler_feishu_allowed_chat_ids")),
    })
}

pub(crate) async fn get_runtime_status(
    app_handle: &AppHandle,
) -> Result<FeishuRuntimeStatus, String> {
    let config = load_runtime_config(app_handle).await?;
    let state = app_handle.state::<FeishuButlerState>();
    let mut status = state.status.lock().await.clone();
    status.butler_enabled = config.butler_enabled;
    status.enabled = config.enabled;
    status.configured = !config.app_id.trim().is_empty() && !config.app_secret.trim().is_empty();
    status.secret_configured = !config.app_secret.trim().is_empty();
    status.app_id = (!config.app_id.trim().is_empty()).then(|| config.app_id);
    status.base_url = Some(config.base_url);
    status.allow_p2p = config.allow_p2p;
    status.allow_group = config.allow_group;
    status.group_require_mention = config.group_require_mention;
    Ok(status)
}

pub(crate) async fn maybe_schedule_butler_feishu_relay_for_aipp_turn(
    app_handle: &AppHandle,
    conversation_id: i64,
    start_after_local_message_id: i64,
    relay_origin: Option<&str>,
) -> Result<(), String> {
    match relay_origin.unwrap_or(RELAY_ORIGIN_AIPP) {
        RELAY_ORIGIN_FEISHU | RELAY_ORIGIN_INTERNAL => return Ok(()),
        _ => {}
    }

    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled || config.only_reply_feishu_originated {
        return Ok(());
    }

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation = db
        .conversation_repo()
        .map_err(|e| e.to_string())?
        .read(conversation_id)
        .map_err(|e| e.to_string())?;
    let Some(conversation) = conversation else {
        return Ok(());
    };
    if conversation.conversation_kind != "butler_main" {
        return Ok(());
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(());
    };

    let scope_id = create_relay_scope(
        app_handle,
        NewRelayScope {
            channel: CHANNEL_FEISHU,
            conversation_id,
            origin: RELAY_ORIGIN_AIPP,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            anchor_external_message_id: target.reply_to_message_id.as_deref().unwrap_or(""),
            start_after_local_message_id,
        },
    )?;
    spawn_feishu_relay_scope_worker(app_handle, scope_id, conversation_id).await;

    Ok(())
}

pub(crate) fn refresh_runtime_async(app_handle: &AppHandle) {
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = refresh_runtime(&app_handle).await {
            warn!(error = %error, "failed to refresh feishu runtime");
        }
    });
}

pub(crate) async fn refresh_runtime(app_handle: &AppHandle) -> Result<FeishuRuntimeStatus, String> {
    crate::ensure_rustls_crypto_provider();
    let config = load_runtime_config(app_handle).await?;
    let state = app_handle.state::<FeishuButlerState>();

    let previous_handle = {
        let mut runtime_task = state.runtime_task.lock().await;
        runtime_task.take()
    };
    if let Some(handle) = previous_handle {
        handle.abort();
    }

    let mut status = build_status(&config);
    if !config.butler_enabled {
        status.status_text = "总管家模式未启用，飞书机器人不会启动".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }
    if !config.enabled {
        status.status_text = "飞书机器人未启用".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        status.status_text = "请先配置飞书 App ID 和 App Secret".to_string();
        status.status_detail = Some("缺少飞书应用凭据，无法启动连接".to_string());
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }

    status.running = true;
    status.status_text = "正在连接飞书长连接".to_string();
    status.status_detail = Some("已创建后台任务，准备初始化飞书 WebSocket 客户端".to_string());
    replace_status(app_handle, status.clone()).await;

    let app_handle_clone = app_handle.clone();
    let config_clone = config.clone();
    let task = tauri::async_runtime::spawn(async move {
        let panic_guard =
            std::panic::AssertUnwindSafe(run_runtime_loop(app_handle_clone.clone(), config_clone))
                .catch_unwind()
                .await;
        if let Err(payload) = panic_guard {
            let panic_message = format_panic_payload(payload);
            mutate_status(&app_handle_clone, |status| {
                status.running = false;
                status.connected = false;
                status.last_error = Some(format!("飞书运行时 panic: {}", panic_message));
                status.status_text = "飞书运行时异常退出".to_string();
                status.status_detail =
                    Some("后台运行任务发生未捕获异常，请检查配置和连接环境".to_string());
            })
            .await;
            warn!(error = %panic_message, "feishu runtime loop panicked");
        }
    });
    {
        let mut runtime_task = state.runtime_task.lock().await;
        *runtime_task = Some(task);
    }
    Ok(status)
}

async fn run_runtime_loop(app_handle: AppHandle, config: FeishuRuntimeConfig) {
    loop {
        mutate_status(&app_handle, |status| {
            status.running = true;
            status.connected = false;
            status.status_text = "正在连接飞书长连接".to_string();
            status.status_detail = Some("正在构建飞书连接配置".to_string());
        })
        .await;

        let ws_config = match openlark_client::Config::builder()
            .app_id(config.app_id.clone())
            .app_secret(config.app_secret.clone())
            .base_url(config.base_url.clone())
            .timeout(FEISHU_HTTP_TIMEOUT)
            .build()
        {
            Ok(config_value) => config_value,
            Err(error) => {
                mutate_status(&app_handle, |status| {
                    status.running = false;
                    status.connected = false;
                    status.last_error = Some(error.to_string());
                    status.status_text = "飞书配置无效".to_string();
                    status.status_detail =
                        Some("连接配置构建失败，请检查 App ID、Secret 和域名".to_string());
                })
                .await;
                return;
            }
        };

        let (payload_tx, payload_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let event_handler = EventDispatcherHandler::builder().payload_sender(payload_tx).build();

        mutate_status(&app_handle, |status| {
            status.running = true;
            status.connected = false;
            status.status_text = "正在连接飞书长连接".to_string();
            status.status_detail = Some("连接配置已生成，正在启动 WebSocket 客户端".to_string());
        })
        .await;

        let processor_app = app_handle.clone();
        let processor_task = tauri::async_runtime::spawn(async move {
            process_payload_loop(processor_app, payload_rx).await;
        });

        set_feishu_runtime_ready_status(
            &app_handle,
            format!("WebSocket 已建立，正在等待飞书事件；{FEISHU_STATUS_READY_DETAIL}"),
        )
        .await;

        let result = LarkWsClient::open(Arc::new(ws_config), event_handler).await;
        processor_task.abort();

        match result {
            Ok(_) => {
                mutate_status(&app_handle, |status| {
                    status.connected = false;
                    status.status_text = "飞书长连接已断开，准备重连".to_string();
                    status.status_detail = Some(
                        "长连接会话正常退出，飞书 SDK 会先内部重连，AIPP 也会在 5 秒后兜底重试"
                            .to_string(),
                    );
                })
                .await;
            }
            Err(error) => {
                mutate_status(&app_handle, |status| {
                    status.connected = false;
                    status.last_error = Some(error.to_string());
                    status.status_text = "飞书连接失败，准备重连".to_string();
                    status.status_detail = Some(
                        "握手或长连接建立失败，飞书 SDK 会先内部重连，AIPP 也会在 5 秒后兜底重试"
                            .to_string(),
                    );
                })
                .await;
            }
        }

        sleep(FEISHU_RUNTIME_RETRY_INTERVAL).await;
    }
}

async fn process_payload_loop(
    app_handle: AppHandle,
    mut payload_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(payload) = payload_rx.recv().await {
        mutate_status(&app_handle, |status| {
            status.last_event_at = Some(now_string());
        })
        .await;

        let config = match load_runtime_config(&app_handle).await {
            Ok(config) => config,
            Err(error) => {
                warn!(error = %error, "failed to reload Feishu runtime config before handling payload");
                mutate_status(&app_handle, |status| {
                    status.last_error = Some(error);
                    status.status_text = "飞书事件到达，但配置读取失败".to_string();
                })
                .await;
                continue;
            }
        };

        if !config.butler_enabled || !config.enabled {
            debug!("ignore Feishu payload because runtime is disabled by latest config");
            continue;
        }

        if let Err(error) = handle_payload(&app_handle, &config, &payload).await {
            warn!(error = %error, "failed to handle feishu payload");
        }
    }
}

async fn handle_payload(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    payload: &[u8],
) -> Result<(), String> {
    let envelope: EventEnvelope = match serde_json::from_slice(payload) {
        Ok(value) => value,
        Err(error) => {
            debug!(error = %error, "ignore unparsable feishu payload");
            return Ok(());
        }
    };
    if envelope.header.event_type == "application.bot.menu_v6" {
        info!(
            event_type = %envelope.header.event_type,
            event_id = %envelope.header.event_id.as_deref().unwrap_or(""),
            "received Feishu bot menu event"
        );
    }
    match envelope.header.event_type.as_str() {
        "im.message.receive_v1" => {
            let Some(event) = parse_incoming_text_event(config, envelope)? else {
                return Ok(());
            };

            if let Err(error) = process_incoming_text_message(app_handle, config, &event).await {
                warn!(message_id = %event.message_id, error = %error, "failed to process feishu message");
                if let Err(reply_error) = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("总管家处理飞书消息失败：{}", truncate_text(&error, 180)),
                )
                .await
                {
                    warn!(
                        message_id = %event.message_id,
                        error = %reply_error,
                        "failed to send Feishu error reply"
                    );
                }
                mutate_status(app_handle, |status| {
                    status.last_error = Some(error);
                    status.status_text = "处理飞书消息时发生错误".to_string();
                })
                .await;
            }
        }
        "card.action.trigger" => {
            handle_card_action_trigger(app_handle, &envelope.event).await?;
        }
        "application.bot.menu_v6" => {
            handle_bot_menu_event(app_handle, config, &envelope.header, &envelope.event).await?;
        }
        _ => {}
    }

    Ok(())
}

fn parse_bot_menu_click_event(
    raw_event: &Value,
) -> Result<Option<FeishuBotMenuClickEvent>, String> {
    let event: FeishuBotMenuEvent =
        serde_json::from_value(raw_event.clone()).map_err(|e| e.to_string())?;
    let operator_open_id = event.operator.operator_id.open_id.trim().to_string();
    let event_key = event.event_key.trim().to_string();
    if operator_open_id.is_empty() || event_key.is_empty() {
        return Ok(None);
    }
    Ok(Some(FeishuBotMenuClickEvent { operator_open_id, event_key }))
}

async fn handle_bot_menu_event(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    header: &EventHeader,
    raw_event: &Value,
) -> Result<(), String> {
    let Some(event) = parse_bot_menu_click_event(raw_event)? else {
        warn!(
            event_id = %header.event_id.as_deref().unwrap_or(""),
            raw_event = %truncate_text(&raw_event.to_string(), 400),
            "ignored Feishu bot menu event because event_key/open_id was missing"
        );
        return Ok(());
    };
    if !config.allowed_open_ids.is_empty()
        && !config.allowed_open_ids.contains(&event.operator_open_id)
    {
        warn!(
            event_id = %header.event_id.as_deref().unwrap_or(""),
            operator_open_id = %event.operator_open_id,
            event_key = %event.event_key,
            "ignored Feishu bot menu event because operator is not in allowlist"
        );
        return Ok(());
    }
    if event.event_key != FEISHU_MENU_NEW_CONVERSATION_EVENT_KEY {
        info!(
            event_id = %header.event_id.as_deref().unwrap_or(""),
            operator_open_id = %event.operator_open_id,
            event_key = %event.event_key,
            "ignored unsupported Feishu bot menu event"
        );
        return Ok(());
    }
    info!(
        event_id = %header.event_id.as_deref().unwrap_or(""),
        operator_open_id = %event.operator_open_id,
        event_key = %event.event_key,
        "processing Feishu bot menu event"
    );

    let event_id = header.event_id.as_deref().map(str::trim).filter(|value| !value.is_empty());
    if let Some(event_id) = event_id {
        if external_message_exists(app_handle, CHANNEL_FEISHU, event_id)? {
            info!(event_id, "ignored duplicated Feishu bot menu event");
            return Ok(());
        }
    }

    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "正在处理飞书菜单事件".to_string();
        status.status_detail = Some("收到“新建会话”菜单点击，正在重置总管家上下文".to_string());
    })
    .await;

    let reset_response = match reset_butler_main_conversation(app_handle.clone()).await {
        Ok(response) => response,
        Err(error) => {
            warn!(
                event_id = %header.event_id.as_deref().unwrap_or(""),
                operator_open_id = %event.operator_open_id,
                event_key = %event.event_key,
                error = %error,
                "failed to reset Butler context from Feishu bot menu event"
            );
            mutate_status(app_handle, |status| {
                status.running = true;
                status.connected = true;
                status.last_error = Some(error.clone());
                status.status_text = "处理飞书菜单事件失败".to_string();
                status.status_detail = Some("总管家主会话重置失败".to_string());
            })
            .await;
            let _ = send_text_message_to_open_id(
                app_handle,
                config,
                &event.operator_open_id,
                &format!("清空上下文失败：{}", truncate_text(&error, 180)),
            )
            .await;
            return Err(error);
        }
    };
    info!(
        event_id = %header.event_id.as_deref().unwrap_or(""),
        operator_open_id = %event.operator_open_id,
        conversation_id = reset_response.conversation.id,
        "reset Butler context from Feishu bot menu event"
    );

    if let Some(event_id) = event_id {
        insert_external_link(
            app_handle,
            ChannelLinkRecord {
                external_message_id: event_id,
                external_chat_id: None,
                external_user_id: Some(&event.operator_open_id),
                conversation_id: reset_response.conversation.id,
                local_message_id: None,
                direction: "inbound",
                payload_type: "menu",
            },
        )?;
    }

    let confirmation_message_id = send_text_message_to_open_id(
        app_handle,
        config,
        &event.operator_open_id,
        "已经清空上下文，并创建了新的总管家会话。",
    )
    .await?;
    info!(
        event_id = %header.event_id.as_deref().unwrap_or(""),
        operator_open_id = %event.operator_open_id,
        confirmation_message_id = %confirmation_message_id,
        "sent Feishu bot menu confirmation message"
    );
    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &confirmation_message_id,
            external_chat_id: None,
            external_user_id: Some(&event.operator_open_id),
            conversation_id: reset_response.conversation.id,
            local_message_id: None,
            direction: "outbound",
            payload_type: "text",
        },
    )?;

    set_feishu_runtime_ready_status(app_handle, "已处理飞书“新建会话”菜单事件，总管家上下文已重置")
        .await;
    Ok(())
}

async fn handle_card_action_trigger(
    app_handle: &AppHandle,
    raw_event: &Value,
) -> Result<(), String> {
    let callback: FeishuCardActionCallback =
        serde_json::from_value(raw_event.clone()).map_err(|e| e.to_string())?;
    let event = callback.event();
    let action_value = callback
        .event()
        .action
        .value
        .as_ref()
        .ok_or_else(|| "飞书卡片回调缺少 action.value".to_string())?;

    if let Some(request_kind) = action_value.get("request_kind").and_then(Value::as_str) {
        match request_kind {
            "operation_permission" => {
                let request_id = action_value
                    .get("request_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "飞书操作权限卡片缺少 request_id".to_string())?;
                let decision = action_value
                    .get("decision")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "飞书操作权限卡片缺少 decision".to_string())?;
                let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
                let request = state
                    .get_permission_request(request_id)
                    .await
                    .ok_or_else(|| "操作权限请求不存在或已处理".to_string())?;
                if let Some(expected_open_id) = request.allowed_open_id.as_deref() {
                    if event.operator.open_id != expected_open_id {
                        return Err("当前飞书用户无权处理该操作权限请求".to_string());
                    }
                }
                confirm_operation_permission(
                    app_handle.clone(),
                    request_id.to_string(),
                    decision.to_string(),
                )
                .await?;
                return Ok(());
            }
            "acp_permission" => {
                let request_id = action_value
                    .get("request_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "飞书 ACP 权限卡片缺少 request_id".to_string())?;
                let state = app_handle.state::<AcpPermissionState>();
                let request = state
                    .get_request(request_id)
                    .await
                    .ok_or_else(|| "ACP 权限请求不存在或已处理".to_string())?;
                if let Some(expected_open_id) = request.allowed_open_id.as_deref() {
                    if event.operator.open_id != expected_open_id {
                        return Err("当前飞书用户无权处理该 ACP 权限请求".to_string());
                    }
                }
                let cancelled =
                    action_value.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
                let option_id =
                    action_value.get("option_id").and_then(Value::as_str).map(ToString::to_string);
                confirm_acp_permission(
                    app_handle.clone(),
                    request_id.to_string(),
                    option_id,
                    Some(cancelled),
                )
                .await?;
                return Ok(());
            }
            _ => {}
        }
    }

    let request_id = action_value
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "飞书卡片回调缺少 request_id".to_string())?;
    let action = action_value.get("action").and_then(Value::as_str).unwrap_or("submit");

    if action == "cancel" {
        match resolve_ask_user_question_response(app_handle, request_id, None, true).await {
            Ok(_) => {}
            Err(error) if is_missing_ask_user_request_error(&error) => {
                if !try_recover_feishu_ask_user_resolution(
                    app_handle,
                    &callback,
                    Err("User cancelled AskUserQuestion".to_string()),
                )
                .await?
                {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        return Ok(());
    }

    let answers =
        match build_ask_user_answers_from_card_callback(app_handle, request_id, &callback).await {
            Ok(answers) => answers,
            Err(error) if is_missing_ask_user_request_error(&error) => {
                let form_value = callback
                    .event()
                    .action
                    .form_value
                    .as_ref()
                    .ok_or_else(|| "飞书卡片回调缺少 form_value".to_string())?;
                if try_recover_feishu_ask_user_resolution(
                    app_handle,
                    &callback,
                    Ok(build_ask_user_question_tool_result(
                        &recover_answers_from_callback_payload(app_handle, &callback, form_value)
                            .await?,
                    )),
                )
                .await?
                {
                    return Ok(());
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
    debug!(
        request_id,
        operator_open_id = %event.operator.open_id,
        "resolved AskUserQuestion from Feishu card callback"
    );
    match resolve_ask_user_question_response(app_handle, request_id, Some(answers), false).await {
        Ok(_) => {}
        Err(error) if is_missing_ask_user_request_error(&error) => {
            let form_value = callback
                .event()
                .action
                .form_value
                .as_ref()
                .ok_or_else(|| "飞书卡片回调缺少 form_value".to_string())?;
            if !try_recover_feishu_ask_user_resolution(
                app_handle,
                &callback,
                Ok(build_ask_user_question_tool_result(
                    &recover_answers_from_callback_payload(app_handle, &callback, form_value)
                        .await?,
                )),
            )
            .await?
            {
                return Err(error);
            }
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn is_missing_ask_user_request_error(error: &str) -> bool {
    error.contains("AskUserQuestion request not found")
}

fn build_ask_user_question_tool_result(answers: &HashMap<String, String>) -> String {
    json!([{
        "type": "json",
        "json": {
            "answers": answers
        }
    }])
    .to_string()
}

fn find_conversation_id_by_external_message(
    app_handle: &AppHandle,
    external_message_id: &str,
) -> Result<Option<i64>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT conversation_id
         FROM external_channel_message_link
         WHERE channel = ?1 AND external_message_id = ?2
         LIMIT 1",
        params![CHANNEL_FEISHU, external_message_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn find_latest_recoverable_ask_user_tool_call(calls: &[MCPToolCall]) -> Option<&MCPToolCall> {
    calls.iter().find(|call| {
        call.tool_name == "ask_user_question"
            && matches!(call.status.as_str(), "pending" | "executing")
    })
}

async fn recover_answers_from_callback_payload(
    app_handle: &AppHandle,
    callback: &FeishuCardActionCallback,
    form_value: &Map<String, Value>,
) -> Result<HashMap<String, String>, String> {
    let open_message_id = callback
        .event()
        .context
        .as_ref()
        .and_then(|context| context.open_message_id.clone())
        .and_then(|value| normalize_optional_id(Some(value)))
        .ok_or_else(|| {
            "飞书卡片回调缺少 open_message_id，无法恢复 ask_user_question 状态".to_string()
        })?;
    let conversation_id =
        find_conversation_id_by_external_message(app_handle, &open_message_id)?
            .ok_or_else(|| format!("未找到飞书消息 {} 关联的会话", open_message_id))?;
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let calls =
        mcp_db.get_mcp_tool_calls_by_conversation(conversation_id).map_err(|e| e.to_string())?;
    let tool_call = find_latest_recoverable_ask_user_tool_call(&calls).ok_or_else(|| {
        format!("会话 {} 中没有可恢复的 ask_user_question 工具调用", conversation_id)
    })?;
    let request: AskUserQuestionRequest = serde_json::from_str(&tool_call.parameters)
        .map_err(|e| format!("解析 ask_user_question 参数失败: {}", e))?;
    map_ask_user_form_values_to_answers(&request.questions, form_value)
}

async fn try_recover_feishu_ask_user_resolution(
    app_handle: &AppHandle,
    callback: &FeishuCardActionCallback,
    execution_result: Result<String, String>,
) -> Result<bool, String> {
    let open_message_id = callback
        .event()
        .context
        .as_ref()
        .and_then(|context| context.open_message_id.clone())
        .and_then(|value| normalize_optional_id(Some(value)));
    let Some(open_message_id) = open_message_id else {
        return Ok(false);
    };
    let Some(conversation_id) =
        find_conversation_id_by_external_message(app_handle, &open_message_id)?
    else {
        return Ok(false);
    };
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let calls =
        mcp_db.get_mcp_tool_calls_by_conversation(conversation_id).map_err(|e| e.to_string())?;

    if let Some(tool_call) = find_latest_recoverable_ask_user_tool_call(&calls) {
        crate::mcp::execution_api::finalize_tool_call_from_external_result(
            app_handle,
            tool_call.id,
            execution_result,
        )
        .await?;
        info!(
            call_id = tool_call.id,
            conversation_id,
            open_message_id = %open_message_id,
            "Recovered AskUserQuestion resolution from Feishu callback"
        );
        return Ok(true);
    }

    if calls.iter().any(|call| {
        call.tool_name == "ask_user_question"
            && matches!(call.status.as_str(), "success" | "failed")
    }) {
        debug!(
            conversation_id,
            open_message_id = %open_message_id,
            "Ignore duplicate or stale Feishu AskUserQuestion callback"
        );
        return Ok(true);
    }

    Ok(false)
}

async fn build_ask_user_answers_from_card_callback(
    app_handle: &AppHandle,
    request_id: &str,
    callback: &FeishuCardActionCallback,
) -> Result<HashMap<String, String>, String> {
    let Some(interaction_state) =
        app_handle.try_state::<crate::mcp::builtin_mcp::interaction::InteractionState>()
    else {
        return Err("InteractionState not found".to_string());
    };
    let request = interaction_state
        .get_ask_user_request(request_id)
        .await
        .ok_or_else(|| "AskUserQuestion request not found".to_string())?;
    let form_value = callback
        .event()
        .action
        .form_value
        .as_ref()
        .ok_or_else(|| "飞书卡片回调缺少 form_value".to_string())?;
    map_ask_user_form_values_to_answers(&request.questions, form_value)
}

fn map_ask_user_form_values_to_answers(
    questions: &[AskUserQuestionItem],
    form_value: &Map<String, Value>,
) -> Result<HashMap<String, String>, String> {
    let mut answers = HashMap::new();
    for (index, question) in questions.iter().enumerate() {
        let field_name = format!("question_{}", index);
        let raw_value = form_value
            .get(&field_name)
            .ok_or_else(|| format!("飞书卡片回答缺少字段 {}", field_name))?;
        let answer = match raw_value {
            Value::String(value) => value.clone(),
            Value::Array(items) => {
                items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
            }
            Value::Object(map) => map
                .get("value")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("values").and_then(Value::as_array).map(|items| {
                        items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
                    })
                })
                .ok_or_else(|| format!("飞书卡片回答字段 {} 结构无效", field_name))?,
            _ => return Err(format!("飞书卡片回答字段 {} 类型无效", field_name)),
        };
        answers.insert(question.question.clone(), answer);
    }
    Ok(answers)
}

fn parse_incoming_text_event(
    config: &FeishuRuntimeConfig,
    envelope: EventEnvelope,
) -> Result<Option<IncomingTextEvent>, String> {
    let event: EventBody = serde_json::from_value(envelope.event).map_err(|e| e.to_string())?;
    if event.message.message_type != "text" {
        return Ok(None);
    }

    let text_content: TextMessageContent =
        serde_json::from_str(&event.message.content).map_err(|e| e.to_string())?;
    let text = text_content.text.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }

    let sender_open_id = event.sender.sender_id.open_id.trim().to_string();
    if sender_open_id.is_empty() {
        return Ok(None);
    }

    let chat_type = event.message.chat_type.trim().to_string();
    if chat_type == "p2p" && !config.allow_p2p {
        return Ok(None);
    }
    if chat_type != "p2p" && !config.allow_group {
        return Ok(None);
    }

    let chat_id = event.message.chat_id.clone();
    if !config.allowed_open_ids.is_empty() && !config.allowed_open_ids.contains(&sender_open_id) {
        return Ok(None);
    }
    if chat_type != "p2p" {
        if let Some(value) = chat_id.as_ref() {
            if !config.allowed_chat_ids.is_empty() && !config.allowed_chat_ids.contains(value) {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
    }

    Ok(Some(IncomingTextEvent {
        message_id: event.message.message_id,
        sender_open_id,
        chat_id,
        text,
        chat_type,
        parent_id: event.message.parent_id,
        root_id: event.message.root_id,
        has_mentions: event.message.mentions.map(|mentions| !mentions.is_empty()).unwrap_or(false),
    }))
}

async fn try_handle_pending_permission_reply(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    event: &IncomingTextEvent,
) -> Result<bool, String> {
    let Some(command) = parse_permission_reply_command(&event.text) else {
        return Ok(false);
    };

    match command {
        PermissionReplyCommand::Operation { review_code, decision } => {
            let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
            let Some(request) = state.find_permission_request_by_review_code(&review_code).await
            else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("未找到待处理的操作权限审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            };
            if !feishu_reply_matches_permission_context(
                event,
                request.allowed_open_id.as_deref(),
                request.allowed_chat_id.as_deref(),
                request.feishu_message_id.as_deref(),
            ) {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("你无权处理审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            }
            confirm_operation_permission(
                app_handle.clone(),
                request.event.request_id.clone(),
                decision.to_string(),
            )
            .await?;
            if let Some(conversation_id) = request.conversation_id {
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &event.message_id,
                        external_chat_id: event.chat_id.as_deref(),
                        external_user_id: Some(&event.sender_open_id),
                        conversation_id,
                        local_message_id: None,
                        direction: "inbound",
                        payload_type: "text",
                    },
                )?;
            }
            let confirmation_text = match decision {
                "allow_for_conversation" => format!("已按“本任务信任”处理审批单 {}", review_code),
                "allow_for_assistant" => format!("已按“助手工作区信任”处理审批单 {}", review_code),
                "deny" => format!("已拒绝审批单 {}", review_code),
                _ => format!("已允许一次审批单 {}", review_code),
            };
            let _ =
                reply_text_message(app_handle, config, &event.message_id, &confirmation_text).await;
            Ok(true)
        }
        PermissionReplyCommand::AcpCancel { review_code } => {
            let state = app_handle.state::<AcpPermissionState>();
            let Some(request) = state.find_request_by_review_code(&review_code).await else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("未找到待处理的 ACP 审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            };
            if !feishu_reply_matches_permission_context(
                event,
                request.allowed_open_id.as_deref(),
                request.allowed_chat_id.as_deref(),
                request.feishu_message_id.as_deref(),
            ) {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("你无权处理审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            }
            confirm_acp_permission(
                app_handle.clone(),
                request.event.request_id.clone(),
                None,
                Some(true),
            )
            .await?;
            if let Some(conversation_id) = request.conversation_id {
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &event.message_id,
                        external_chat_id: event.chat_id.as_deref(),
                        external_user_id: Some(&event.sender_open_id),
                        conversation_id,
                        local_message_id: None,
                        direction: "inbound",
                        payload_type: "text",
                    },
                )?;
            }
            let _ = reply_text_message(
                app_handle,
                config,
                &event.message_id,
                &format!("已取消审批单 {}", review_code),
            )
            .await;
            Ok(true)
        }
        PermissionReplyCommand::AcpSelect { review_code, option_index } => {
            let state = app_handle.state::<AcpPermissionState>();
            let Some(request) = state.find_request_by_review_code(&review_code).await else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("未找到待处理的 ACP 审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            };
            if !feishu_reply_matches_permission_context(
                event,
                request.allowed_open_id.as_deref(),
                request.allowed_chat_id.as_deref(),
                request.feishu_message_id.as_deref(),
            ) {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("你无权处理审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            }
            let Some(option) = request.event.options.get(option_index.saturating_sub(1)) else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("审批单 {} 没有第 {} 个选项", review_code, option_index),
                )
                .await;
                return Ok(true);
            };
            confirm_acp_permission(
                app_handle.clone(),
                request.event.request_id.clone(),
                Some(option.option_id.clone()),
                Some(false),
            )
            .await?;
            if let Some(conversation_id) = request.conversation_id {
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &event.message_id,
                        external_chat_id: event.chat_id.as_deref(),
                        external_user_id: Some(&event.sender_open_id),
                        conversation_id,
                        local_message_id: None,
                        direction: "inbound",
                        payload_type: "text",
                    },
                )?;
            }
            let _ = reply_text_message(
                app_handle,
                config,
                &event.message_id,
                &format!("已按“{}”处理审批单 {}", option.name, review_code),
            )
            .await;
            Ok(true)
        }
    }
}

async fn process_incoming_text_message(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    event: &IncomingTextEvent,
) -> Result<(), String> {
    if external_message_exists(app_handle, CHANNEL_FEISHU, &event.message_id)? {
        return Ok(());
    }

    if event.chat_type != "p2p" && config.group_require_mention {
        let replied_to_bot = linked_to_outbound_message(
            app_handle,
            event.parent_id.as_deref().or(event.root_id.as_deref()),
        )?;
        if !event.has_mentions && !replied_to_bot {
            return Ok(());
        }
    }

    if try_handle_pending_permission_reply(app_handle, config, event).await? {
        return Ok(());
    }

    let state = app_handle.state::<FeishuButlerState>();
    let _ingress_guard = state.ingress_lock.lock().await;

    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "总管家正在处理飞书消息".to_string();
        status.status_detail =
            Some(format!("收到飞书消息，正在总管家主会话中处理（chat_type={}）", event.chat_type));
    })
    .await;

    let butler_conversation = load_or_create_butler_main_internal(app_handle).await?;
    let assistant_id =
        butler_conversation.assistant_id.ok_or_else(|| "总管家主会话缺少 assistant".to_string())?;

    let before_message_max_id = get_latest_message_id(app_handle, butler_conversation.id)?;

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &event.message_id,
            external_chat_id: event.chat_id.as_deref(),
            external_user_id: Some(&event.sender_open_id),
            conversation_id: butler_conversation.id,
            local_message_id: None,
            direction: "inbound",
            payload_type: "text",
        },
    )?;
    let relay_scope_id = create_relay_scope(
        app_handle,
        NewRelayScope {
            channel: CHANNEL_FEISHU,
            conversation_id: butler_conversation.id,
            origin: RELAY_ORIGIN_FEISHU,
            external_chat_id: event.chat_id.as_deref(),
            external_user_id: Some(&event.sender_open_id),
            anchor_external_message_id: &event.message_id,
            start_after_local_message_id: before_message_max_id,
        },
    )?;

    let continuation_lock = get_butler_main_continuation_lock(butler_conversation.id).await;
    {
        let _guard = continuation_lock.lock().await;
        wait_for_butler_main_to_be_idle(app_handle, butler_conversation.id).await;
        let window = resolve_butler_execution_window(app_handle)?;
        let request = AiRequest {
            conversation_id: butler_conversation.id.to_string(),
            assistant_id,
            prompt: event.text.clone(),
            model: None,
            override_model_id: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: Some(true),
            attachment_list: None,
        };
        ask_ai(
            app_handle.clone(),
            app_handle.state::<crate::AppState>(),
            app_handle.state::<crate::AcpSessionState>(),
            app_handle.state::<crate::FeatureConfigState>(),
            app_handle.state::<crate::state::message_token::MessageTokenManager>(),
            app_handle.state::<crate::state::activity_state::ConversationActivityManager>(),
            window,
            request,
            None,
            None,
            None,
            Some(build_feishu_system_message(event)),
            Some(RELAY_ORIGIN_FEISHU.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    if let Some(user_message_id) = find_latest_message_id_by_type(
        app_handle,
        butler_conversation.id,
        before_message_max_id,
        "user",
    )? {
        update_external_link_local_message(
            app_handle,
            CHANNEL_FEISHU,
            &event.message_id,
            user_message_id,
        )?;
    }

    spawn_feishu_relay_scope_worker(app_handle, relay_scope_id, butler_conversation.id).await;
    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "飞书消息已受理，正在持续回发".to_string();
        status.status_detail =
            Some("总管家会继续处理本轮消息，并把后续输出持续回发到飞书".to_string());
    })
    .await;
    Ok(())
}

fn build_feishu_system_message(event: &IncomingTextEvent) -> String {
    format!(
        "<external_channel_input>\nchannel=feishu\nsource={}\nmessage_id={}\nchat_type={}\nchat_id={}\nsender_open_id={}\nreply_parent_id={}\nreply_root_id={}\n</external_channel_input>",
        BOTLER_SOURCE,
        event.message_id,
        event.chat_type,
        event.chat_id.as_deref().unwrap_or(""),
        event.sender_open_id,
        event.parent_id.as_deref().unwrap_or(""),
        event.root_id.as_deref().unwrap_or(""),
    )
}

async fn wait_for_butler_to_settle(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
) -> Result<(), String> {
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let mut idle_checks = 0;
    let max_checks =
        (FEISHU_SETTLE_TIMEOUT.as_millis() / FEISHU_SETTLE_CHECK_INTERVAL.as_millis()) as usize;
    for attempt in 0..max_checks {
        let runtime_state = activity_manager.get_runtime_state(butler_conversation_id).await;
        let pending_tasks = count_pending_butler_tasks(app_handle, butler_conversation_id)?;
        if !runtime_state.is_running && pending_tasks == 0 {
            idle_checks += 1;
            if idle_checks >= 2 {
                return Ok(());
            }
        } else {
            idle_checks = 0;
            if attempt % FEISHU_SETTLE_STATUS_INTERVAL_STEPS == 0 {
                let waited_seconds =
                    (((attempt + 1) as u128) * FEISHU_SETTLE_CHECK_INTERVAL.as_millis()) / 1000;
                mutate_status(app_handle, |status| {
                    status.running = true;
                    status.connected = true;
                    status.status_text = "总管家仍在处理飞书消息".to_string();
                    status.status_detail = Some(format!(
                        "正在等待总管家完成当前消息（运行中={}, 待完成任务={}，已等待 {} 秒）",
                        runtime_state.is_running, pending_tasks, waited_seconds
                    ));
                })
                .await;
            }
        }
        sleep(FEISHU_SETTLE_CHECK_INTERVAL).await;
    }
    let pending_tasks = count_pending_butler_tasks(app_handle, butler_conversation_id)?;
    let runtime_state = activity_manager.get_runtime_state(butler_conversation_id).await;
    Err(format!(
        "等待总管家处理飞书消息超时（运行中={}，待完成任务={}）",
        runtime_state.is_running, pending_tasks
    ))
}

fn count_pending_butler_tasks(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
) -> Result<i64, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT COUNT(1)
         FROM conversation
         WHERE parent_butler_conversation_id = ?1
           AND conversation_kind = 'butler_task'
           AND (
             butler_task_finalized_at IS NULL
             OR COALESCE(butler_task_status, '') NOT IN (?2, ?3, ?4)
           )",
        params![
            butler_conversation_id,
            TERMINAL_TASK_STATUSES[0],
            TERMINAL_TASK_STATUSES[1],
            TERMINAL_TASK_STATUSES[2]
        ],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())
}

fn get_latest_message_id(app_handle: &AppHandle, conversation_id: i64) -> Result<i64, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    Ok(conn
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM message WHERE conversation_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?)
}

fn find_latest_message_id_by_type(
    app_handle: &AppHandle,
    conversation_id: i64,
    after_message_id: i64,
    message_type: &str,
) -> Result<Option<i64>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id
         FROM message
         WHERE conversation_id = ?1 AND id > ?2 AND message_type = ?3
         ORDER BY id DESC
         LIMIT 1",
        params![conversation_id, after_message_id, message_type],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn create_relay_scope(app_handle: &AppHandle, new_scope: NewRelayScope<'_>) -> Result<i64, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO external_channel_relay_scope
            (channel, conversation_id, origin, external_chat_id, external_user_id,
             anchor_external_message_id, start_after_local_message_id, last_delivered_local_message_id,
             status, updated_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 'pending', CURRENT_TIMESTAMP)",
        params![
            new_scope.channel,
            new_scope.conversation_id,
            new_scope.origin,
            new_scope.external_chat_id,
            new_scope.external_user_id,
            new_scope.anchor_external_message_id,
            new_scope.start_after_local_message_id,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

fn load_relay_scope(app_handle: &AppHandle, scope_id: i64) -> Result<RelayScopeRecord, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, channel, conversation_id, origin, external_chat_id, external_user_id,
                anchor_external_message_id, start_after_local_message_id,
                last_delivered_local_message_id, status
         FROM external_channel_relay_scope
         WHERE id = ?1",
        params![scope_id],
        |row| {
            Ok(RelayScopeRecord {
                id: row.get(0)?,
                channel: row.get(1)?,
                conversation_id: row.get(2)?,
                origin: row.get(3)?,
                external_chat_id: row.get(4)?,
                external_user_id: row.get(5)?,
                anchor_external_message_id: row.get(6)?,
                start_after_local_message_id: row.get(7)?,
                last_delivered_local_message_id: row.get(8)?,
                status: row.get(9)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

fn find_active_relay_scope(
    app_handle: &AppHandle,
    conversation_id: i64,
    origin: &str,
) -> Result<Option<RelayScopeRecord>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, channel, conversation_id, origin, external_chat_id, external_user_id,
                anchor_external_message_id, start_after_local_message_id,
                last_delivered_local_message_id, status
         FROM external_channel_relay_scope
         WHERE channel = ?1
           AND conversation_id = ?2
           AND origin = ?3
           AND COALESCE(status, '') NOT IN ('completed', 'failed')
         ORDER BY id DESC
         LIMIT 1",
        params![CHANNEL_FEISHU, conversation_id, origin],
        |row| {
            Ok(RelayScopeRecord {
                id: row.get(0)?,
                channel: row.get(1)?,
                conversation_id: row.get(2)?,
                origin: row.get(3)?,
                external_chat_id: row.get(4)?,
                external_user_id: row.get(5)?,
                anchor_external_message_id: row.get(6)?,
                start_after_local_message_id: row.get(7)?,
                last_delivered_local_message_id: row.get(8)?,
                status: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn mark_relay_scope_progress(
    app_handle: &AppHandle,
    scope_id: i64,
    last_delivered_local_message_id: i64,
    status: &str,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE external_channel_relay_scope
         SET last_delivered_local_message_id = ?2,
             status = ?3,
             last_error = NULL,
             updated_time = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![scope_id, last_delivered_local_message_id, status],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_relay_scope_failed(
    app_handle: &AppHandle,
    scope_id: i64,
    error: &str,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE external_channel_relay_scope
         SET status = 'failed',
             last_error = ?2,
             updated_time = CURRENT_TIMESTAMP
         WHERE id = ?1",
        params![scope_id, truncate_text(error, 500)],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn record_scope_delivery(
    app_handle: &AppHandle,
    scope_id: i64,
    channel: &str,
    conversation_id: i64,
    local_message_id: i64,
    external_message_id: Option<&str>,
    status: &str,
    rendered_text: &str,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO external_channel_message_delivery
            (scope_id, channel, conversation_id, local_message_id, external_message_id, status, rendered_text, updated_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
         ON CONFLICT(scope_id, local_message_id) DO UPDATE SET
            external_message_id = excluded.external_message_id,
            status = excluded.status,
            rendered_text = excluded.rendered_text,
            updated_time = CURRENT_TIMESTAMP",
        params![
            scope_id,
            channel,
            conversation_id,
            local_message_id,
            external_message_id,
            status,
            rendered_text,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn find_latest_feishu_target(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<Option<ChannelLinkTarget>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let linked_target = conn
        .query_row(
            "SELECT external_message_id, external_chat_id, external_user_id
             FROM external_channel_message_link
             WHERE channel = ?1 AND conversation_id = ?2
             ORDER BY created_time DESC, id DESC
             LIMIT 1",
            params![CHANNEL_FEISHU, conversation_id],
            |row| {
                Ok(ChannelLinkTarget {
                    reply_to_message_id: normalize_optional_id(Some(row.get(0)?)),
                    external_chat_id: row.get(1)?,
                    external_user_id: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if linked_target.is_some() {
        return Ok(linked_target);
    }

    conn.query_row(
        "SELECT anchor_external_message_id, external_chat_id, external_user_id
         FROM external_channel_relay_scope
         WHERE channel = ?1 AND conversation_id = ?2
         ORDER BY updated_time DESC, created_time DESC, id DESC
         LIMIT 1",
        params![CHANNEL_FEISHU, conversation_id],
        |row| {
            Ok(ChannelLinkTarget {
                reply_to_message_id: normalize_optional_id(Some(row.get(0)?)),
                external_chat_id: row.get(1)?,
                external_user_id: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub(crate) fn conversation_has_feishu_target(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<bool, String> {
    Ok(find_latest_feishu_target(app_handle, conversation_id)?.is_some())
}

pub(crate) fn inherit_latest_feishu_target(
    app_handle: &AppHandle,
    source_conversation_id: i64,
    target_conversation_id: i64,
) -> Result<(), String> {
    if source_conversation_id == target_conversation_id {
        return Ok(());
    }
    if find_latest_feishu_target(app_handle, target_conversation_id)?.is_some() {
        return Ok(());
    }
    let Some(target) = find_latest_feishu_target(app_handle, source_conversation_id)? else {
        return Ok(());
    };

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let anchor_external_message_id = target.reply_to_message_id.unwrap_or_default();
    conn.execute(
        "INSERT INTO external_channel_relay_scope
            (channel, conversation_id, origin, external_chat_id, external_user_id,
             anchor_external_message_id, start_after_local_message_id, last_delivered_local_message_id,
             status, updated_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 'completed', CURRENT_TIMESTAMP)",
        params![
            CHANNEL_FEISHU,
            target_conversation_id,
            RELAY_ORIGIN_AIPP,
            target.external_chat_id.as_deref(),
            target.external_user_id.as_deref(),
            anchor_external_message_id,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn find_scope_reply_anchor(
    app_handle: &AppHandle,
    scope_id: i64,
) -> Result<Option<String>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let reply_to = conn
        .query_row(
            "SELECT external_message_id
         FROM external_channel_message_delivery
         WHERE scope_id = ?1
           AND status = 'sent'
           AND external_message_id IS NOT NULL
         ORDER BY local_message_id DESC, id DESC
         LIMIT 1",
            params![scope_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(normalize_optional_id(reply_to))
}

fn list_relayable_messages(
    app_handle: &AppHandle,
    conversation_id: i64,
    after_message_id: i64,
) -> Result<Vec<crate::db::conversation_db::Message>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let messages = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .list_by_conversation_id(conversation_id)
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    Ok(messages
        .into_iter()
        .map(|(message, _)| message)
        .filter(|message| seen.insert(message.id))
        .filter(|message| {
            message.id > after_message_id
                && matches!(
                    message.message_type.as_str(),
                    "user" | "response" | "assistant" | "tool_result"
                )
                && is_message_ready_for_feishu_relay(message)
        })
        .collect())
}

fn is_message_ready_for_feishu_relay(message: &crate::db::conversation_db::Message) -> bool {
    match message.message_type.as_str() {
        "response" | "assistant" => message.finish_time.is_some(),
        _ => true,
    }
}

async fn flush_feishu_relay_scope(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    scope_id: i64,
) -> Result<(), String> {
    let scope = load_relay_scope(app_handle, scope_id)?;
    if scope.status == "completed" {
        return Ok(());
    }

    let mut current_reply_to = find_scope_reply_anchor(app_handle, scope.id)?
        .or_else(|| normalize_optional_id(Some(scope.anchor_external_message_id.clone())));
    let start_after = scope.last_delivered_local_message_id.max(scope.start_after_local_message_id);
    let messages = list_relayable_messages(app_handle, scope.conversation_id, start_after)?;
    let mut delivered_count = 0usize;
    let mut last_processed_message_id = scope.last_delivered_local_message_id;

    for message in messages {
        let rendered = render_message_for_external_channel(
            &message,
            &RenderContext { channel: &scope.channel, relay_origin: &scope.origin },
        );
        last_processed_message_id = message.id;

        match rendered {
            Some(rendered_text) if !rendered_text.trim().is_empty() => {
                let outbound = send_markdown_message_to_target(
                    app_handle,
                    config,
                    &ChannelLinkTarget {
                        reply_to_message_id: current_reply_to.clone(),
                        external_chat_id: scope.external_chat_id.clone(),
                        external_user_id: scope.external_user_id.clone(),
                    },
                    &rendered_text,
                )
                .await?;
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &outbound.external_message_id,
                        external_chat_id: scope.external_chat_id.as_deref(),
                        external_user_id: scope.external_user_id.as_deref(),
                        conversation_id: scope.conversation_id,
                        local_message_id: Some(message.id),
                        direction: "outbound",
                        payload_type: &outbound.payload_type,
                    },
                )?;
                record_scope_delivery(
                    app_handle,
                    scope.id,
                    &scope.channel,
                    scope.conversation_id,
                    message.id,
                    Some(&outbound.external_message_id),
                    "sent",
                    &rendered_text,
                )?;
                current_reply_to = Some(outbound.external_message_id.clone());
                delivered_count += 1;
            }
            _ => {
                record_scope_delivery(
                    app_handle,
                    scope.id,
                    &scope.channel,
                    scope.conversation_id,
                    message.id,
                    None,
                    "skipped",
                    "",
                )?;
            }
        }

        mark_relay_scope_progress(app_handle, scope.id, message.id, "sending")?;
    }

    mark_relay_scope_progress(
        app_handle,
        scope.id,
        last_processed_message_id.max(scope.last_delivered_local_message_id),
        if delivered_count > 0 { "active" } else { &scope.status },
    )?;
    Ok(())
}

fn external_message_exists(
    app_handle: &AppHandle,
    channel: &str,
    external_message_id: &str,
) -> Result<bool, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let result = conn
        .query_row(
            "SELECT 1 FROM external_channel_message_link WHERE channel = ?1 AND external_message_id = ?2 LIMIT 1",
            params![channel, external_message_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(result.is_some())
}

fn linked_to_outbound_message(
    app_handle: &AppHandle,
    external_message_id: Option<&str>,
) -> Result<bool, String> {
    let Some(external_message_id) = external_message_id else {
        return Ok(false);
    };
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let result = conn
        .query_row(
            "SELECT 1
             FROM external_channel_message_link
             WHERE channel = ?1 AND external_message_id = ?2 AND direction = 'outbound'
             LIMIT 1",
            params![CHANNEL_FEISHU, external_message_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(result.is_some())
}

fn insert_external_link(
    app_handle: &AppHandle,
    record: ChannelLinkRecord<'_>,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO external_channel_message_link
            (channel, external_message_id, external_chat_id, external_user_id, conversation_id, local_message_id, direction, payload_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            CHANNEL_FEISHU,
            record.external_message_id,
            record.external_chat_id,
            record.external_user_id,
            record.conversation_id,
            record.local_message_id,
            record.direction,
            record.payload_type
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn update_external_link_local_message(
    app_handle: &AppHandle,
    channel: &str,
    external_message_id: &str,
    local_message_id: i64,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE external_channel_message_link
         SET local_message_id = ?1
         WHERE channel = ?2 AND external_message_id = ?3",
        params![local_message_id, channel, external_message_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn fetch_tenant_access_token(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
) -> Result<String, String> {
    crate::ensure_rustls_crypto_provider();
    let client = feishu_http_client(app_handle);
    let url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        config.base_url.trim_end_matches('/')
    );
    let response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "app_id": config.app_id,
            "app_secret": config.app_secret
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: TenantAccessTokenResponse = response.json().await.map_err(|e| e.to_string())?;
    if body.code != 0 {
        return Err(format!("获取 tenant_access_token 失败: {}", body.msg));
    }
    body.tenant_access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "飞书未返回 tenant_access_token".to_string())
}

fn build_feishu_markdown_card(markdown: &str) -> Result<Value, String> {
    let normalized = markdown.replace("\r\n", "\n").trim().to_string();
    if normalized.is_empty() {
        return Err("飞书卡片内容为空".to_string());
    }

    // Validate that the source is parseable markdown before building a card.
    let _ = MarkdownParser::new_ext(&normalized, MarkdownOptions::all()).count();

    let mut elements = Vec::new();
    for (index, block) in split_markdown_into_feishu_blocks(&normalized).into_iter().enumerate() {
        match block {
            FeishuCardBlock::Markdown(content) => {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    elements.push(build_feishu_markdown_element(trimmed));
                }
            }
            FeishuCardBlock::Table(table) => {
                elements.push(build_feishu_table_element(index, &table)?);
            }
        }
    }

    if elements.is_empty() {
        return Err("飞书卡片缺少可发送内容".to_string());
    }

    Ok(json!({
        "schema": "2.0",
        "body": {
            "elements": elements
        }
    }))
}

fn split_markdown_into_feishu_blocks(markdown: &str) -> Vec<FeishuCardBlock> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut blocks = Vec::new();
    let mut markdown_buffer = Vec::new();
    let mut index = 0usize;
    let mut in_fence = false;
    let mut fence_marker = '`';
    let mut fence_length = 0usize;

    while index < lines.len() {
        let line = lines[index];

        if let Some((marker, length)) = parse_fence_delimiter(line) {
            if !in_fence {
                in_fence = true;
                fence_marker = marker;
                fence_length = length;
            } else if marker == fence_marker && length >= fence_length {
                in_fence = false;
            }
            markdown_buffer.push(line.to_string());
            index += 1;
            continue;
        }

        if !in_fence
            && index + 1 < lines.len()
            && looks_like_markdown_table_header(lines[index], lines[index + 1])
        {
            flush_markdown_block(&mut blocks, &mut markdown_buffer);

            let mut table_lines = vec![lines[index].to_string(), lines[index + 1].to_string()];
            index += 2;
            while index < lines.len() && looks_like_markdown_table_row(lines[index]) {
                table_lines.push(lines[index].to_string());
                index += 1;
            }

            match parse_markdown_table(&table_lines) {
                Ok(table) => blocks.push(FeishuCardBlock::Table(table)),
                Err(_) => blocks.push(FeishuCardBlock::Markdown(table_lines.join("\n"))),
            }
            continue;
        }

        markdown_buffer.push(line.to_string());
        index += 1;
    }

    flush_markdown_block(&mut blocks, &mut markdown_buffer);
    blocks
}

fn flush_markdown_block(blocks: &mut Vec<FeishuCardBlock>, markdown_buffer: &mut Vec<String>) {
    if markdown_buffer.is_empty() {
        return;
    }
    let content = markdown_buffer.join("\n");
    markdown_buffer.clear();
    if !content.trim().is_empty() {
        blocks.push(FeishuCardBlock::Markdown(content));
    }
}

fn parse_fence_delimiter(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|ch| *ch == marker).count();
    (length >= 3).then_some((marker, length))
}

fn looks_like_markdown_table_header(header_line: &str, separator_line: &str) -> bool {
    looks_like_markdown_table_row(header_line) && is_markdown_table_separator(separator_line)
}

fn looks_like_markdown_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && trimmed.contains('|')
}

fn is_markdown_table_separator(line: &str) -> bool {
    let cells = split_markdown_table_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let trimmed = cell.trim();
            !trimmed.is_empty()
                && trimmed.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
                && trimmed.chars().any(|ch| ch == '-')
        })
}

fn parse_markdown_table(lines: &[String]) -> Result<FeishuMarkdownTable, String> {
    if lines.len() < 2 {
        return Err("markdown 表格行数不足".to_string());
    }

    let headers = split_markdown_table_row(&lines[0])
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Err("markdown 表格缺少表头".to_string());
    }
    if !is_markdown_table_separator(&lines[1]) {
        return Err("markdown 表格缺少分隔行".to_string());
    }

    let rows = lines
        .iter()
        .skip(2)
        .map(|line| normalize_table_row(split_markdown_table_row(line), headers.len()))
        .collect::<Vec<_>>();

    Ok(FeishuMarkdownTable { headers, rows })
}

fn split_markdown_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = trimmed.chars().peekable();

    if matches!(chars.peek(), Some('|')) {
        chars.next();
    }

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if matches!(chars.peek(), Some('|')) {
                    current.push('|');
                    chars.next();
                } else {
                    current.push(ch);
                }
            }
            '|' => {
                cells.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() || !trimmed.ends_with('|') {
        cells.push(current.trim().to_string());
    }

    cells
}

fn normalize_table_row(mut cells: Vec<String>, width: usize) -> Vec<String> {
    if cells.len() > width {
        cells.truncate(width);
        return cells;
    }
    while cells.len() < width {
        cells.push(String::new());
    }
    cells
}

fn build_feishu_markdown_element(content: &str) -> Value {
    json!({
        "tag": "markdown",
        "content": content,
        "text_align": "left"
    })
}

fn build_feishu_table_element(index: usize, table: &FeishuMarkdownTable) -> Result<Value, String> {
    if table.headers.is_empty() {
        return Err("飞书表格缺少列定义".to_string());
    }

    let columns = table
        .headers
        .iter()
        .enumerate()
        .map(|(column_index, header)| {
            json!({
                "name": format!("col_{}", column_index + 1),
                "display_name": if header.is_empty() {
                    format!("列{}", column_index + 1)
                } else {
                    header.clone()
                },
                "data_type": "lark_md",
                "width": "auto"
            })
        })
        .collect::<Vec<_>>();

    let rows = table
        .rows
        .iter()
        .map(|row| {
            let mut map = Map::new();
            for (column_index, cell) in row.iter().enumerate() {
                map.insert(format!("col_{}", column_index + 1), Value::String(cell.clone()));
            }
            Value::Object(map)
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "tag": "table",
        "element_id": format!("md_table_{}", index + 1),
        "row_height": "low",
        "page_size": rows.len().max(1),
        "columns": columns,
        "rows": rows
    }))
}

async fn reply_markdown_message(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    reply_to_message_id: &str,
    markdown: &str,
) -> Result<FeishuReplyOutcome, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    let mut interactive_error = None;
    let interactive_card = match build_feishu_markdown_card(markdown) {
        Ok(card) => Some(card),
        Err(error) => {
            let error = format!("构建飞书卡片失败: {error}");
            debug!(error = %error, "failed to build feishu markdown card, falling back to raw text");
            interactive_error = Some(error);
            None
        }
    };

    if let Some(card) = interactive_card.as_ref() {
        match send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_interactive_payload(card),
        )
        .await
        {
            Ok(message_id) => {
                return Ok(FeishuReplyOutcome {
                    message_id,
                    payload_type: "interactive",
                    interactive_error,
                    interactive_card,
                })
            }
            Err(error) => {
                warn!(error = %error, "failed to send feishu interactive reply, falling back to raw text");
                interactive_error = Some(format!("发送飞书 interactive 卡片失败: {error}"));
            }
        }
    }

    let message_id = send_reply_message_request(
        &client,
        config,
        &token,
        reply_to_message_id,
        build_feishu_text_payload(markdown),
    )
    .await?;

    Ok(FeishuReplyOutcome { message_id, payload_type: "text", interactive_error, interactive_card })
}

async fn send_message_request(
    client: &reqwest::Client,
    config: &FeishuRuntimeConfig,
    token: &str,
    receive_id_type: &str,
    receive_id: &str,
    payload: Value,
) -> Result<String, String> {
    let url = format!(
        "{}/open-apis/im/v1/messages?receive_id_type={}",
        config.base_url.trim_end_matches('/'),
        receive_id_type
    );
    let mut payload_object =
        payload.as_object().cloned().ok_or_else(|| "飞书发送消息 payload 格式非法".to_string())?;
    payload_object.insert("receive_id".to_string(), Value::String(receive_id.to_string()));
    let response = client
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&Value::Object(payload_object))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: SendMessageResponse = response.json().await.map_err(|e| e.to_string())?;
    if body.code != 0 {
        return Err(format!("发送飞书消息失败: {}", body.msg));
    }
    body.data
        .map(|data| data.message_id)
        .ok_or_else(|| "飞书发送成功但未返回 message_id".to_string())
}

fn build_feishu_text_payload(text: &str) -> Value {
    json!({
        "msg_type": "text",
        "content": json!({ "text": text }).to_string()
    })
}

fn build_feishu_interactive_payload(card: &Value) -> Value {
    json!({
        "msg_type": "interactive",
        "content": card.to_string()
    })
}

async fn send_interactive_card_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    card: &Value,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    if let Some(reply_to_message_id) = target.reply_to_message_id.as_deref() {
        send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_interactive_payload(card),
        )
        .await
    } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
        send_message_request(
            &client,
            config,
            &token,
            receive_id_type,
            receive_id,
            build_feishu_interactive_payload(card),
        )
        .await
    } else {
        Err("当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string())
    }
}

async fn send_text_message_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    if let Some(reply_to_message_id) = target.reply_to_message_id.as_deref() {
        send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_text_payload(text),
        )
        .await
    } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
        send_message_request(
            &client,
            config,
            &token,
            receive_id_type,
            receive_id,
            build_feishu_text_payload(text),
        )
        .await
    } else {
        Err("当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string())
    }
}

async fn send_permission_review_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    card: &Value,
    fallback_text: &str,
) -> Result<FeishuReplyOutcome, String> {
    match send_interactive_card_to_target(app_handle, config, target, card).await {
        Ok(message_id) => Ok(FeishuReplyOutcome {
            message_id,
            payload_type: "interactive",
            interactive_error: None,
            interactive_card: Some(card.clone()),
        }),
        Err(error) => {
            warn!(error = %error, "failed to send permission review card, falling back to raw text");
            let message_id =
                send_text_message_to_target(app_handle, config, target, fallback_text).await?;
            Ok(FeishuReplyOutcome {
                message_id,
                payload_type: "text",
                interactive_error: Some(format!("发送飞书 interactive 卡片失败: {error}")),
                interactive_card: Some(card.clone()),
            })
        }
    }
}

fn build_operation_permission_card(request: &PermissionRequestSnapshot) -> Value {
    let review_code = request.review_code.clone();
    let request_id = request.event.request_id.clone();
    json!({
        "schema": "2.0",
        "config": { "update_multi": true, "wide_screen_mode": true },
        "body": {
            "elements": [
                {
                    "tag": "markdown",
                    "content": format!(
                        "总管家收到一个操作权限请求。\n\n**审批号**：`{review_code}`\n**操作**：{operation}\n**路径**：`{path}`\n\n如果卡片按钮不可用，也可以直接回复：`批准一次 {review_code}` / `本任务批准 {review_code}` / `助手批准 {review_code}` / `拒绝 {review_code}`",
                        review_code = review_code,
                        operation = request.event.operation,
                        path = truncate_text(&request.event.path, 220),
                    ),
                    "text_align": "left"
                },
                {
                    "tag": "action",
                    "actions": [
                        {
                            "tag": "button",
                            "type": "primary",
                            "text": { "tag": "plain_text", "content": "允许一次" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request_id,
                                "decision": "allow"
                            }
                        },
                        {
                            "tag": "button",
                            "text": { "tag": "plain_text", "content": "本任务信任" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request.event.request_id.clone(),
                                "decision": "allow_for_conversation"
                            }
                        },
                        {
                            "tag": "button",
                            "text": { "tag": "plain_text", "content": "助手工作区信任" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request.event.request_id.clone(),
                                "decision": "allow_for_assistant"
                            }
                        },
                        {
                            "tag": "button",
                            "type": "danger",
                            "text": { "tag": "plain_text", "content": "拒绝" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request.event.request_id.clone(),
                                "decision": "deny"
                            }
                        }
                    ]
                }
            ]
        }
    })
}

fn build_acp_permission_card(request: &AcpPermissionRequestSnapshot) -> Value {
    let request_id = request.event.request_id.clone();
    let mut actions = request
        .event
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            json!({
                "tag": "button",
                "type": if option.kind.starts_with("allow") { "primary" } else { "default" },
                "text": {
                    "tag": "plain_text",
                    "content": format!("{} {}", index + 1, option.name)
                },
                "value": {
                    "request_kind": "acp_permission",
                    "request_id": request_id.clone(),
                    "option_id": option.option_id.clone()
                }
            })
        })
        .collect::<Vec<_>>();
    actions.push(json!({
        "tag": "button",
        "type": "danger",
        "text": { "tag": "plain_text", "content": "取消" },
        "value": {
            "request_kind": "acp_permission",
            "request_id": request.event.request_id.clone(),
            "cancelled": true
        }
    }));

    json!({
        "schema": "2.0",
        "config": { "update_multi": true, "wide_screen_mode": true },
        "body": {
            "elements": [
                {
                    "tag": "markdown",
                    "content": format!(
                        "总管家收到一个 ACP 权限请求。\n\n**审批号**：`{review_code}`\n**标题**：{title}\n**类型**：{kind}\n**参数**：`{parameters}`\n\n如果卡片按钮不可用，也可以直接回复：`批准 1 {review_code}`、`批准 2 {review_code}` 或 `取消 {review_code}`。",
                        review_code = request.review_code,
                        title = request.event.title.as_deref().unwrap_or("未命名"),
                        kind = request.event.kind.as_deref().unwrap_or("unknown"),
                        parameters = truncate_text(request.event.parameters.as_deref().unwrap_or("无"), 220),
                    ),
                    "text_align": "left"
                },
                {
                    "tag": "action",
                    "actions": actions
                }
            ]
        }
    })
}

fn build_ask_user_question_card(event: &AskUserQuestionRequestEvent) -> Value {
    let mut elements = Vec::new();
    elements.push(json!({
        "tag": "markdown",
        "content": "总管家需要你补充一些信息后才能继续。",
        "text_align": "left"
    }));

    let mut form_elements = Vec::new();
    for (index, question) in event.questions.iter().enumerate() {
        let field_name = format!("question_{}", index);
        let options = question
            .options
            .iter()
            .map(|option| {
                json!({
                    "text": {
                        "tag": "plain_text",
                        "content": format!("{} - {}", option.label, option.description)
                    },
                    "value": option.label
                })
            })
            .collect::<Vec<_>>();
        form_elements.push(json!({
            "tag": "markdown",
            "content": format!("**{}**\n{}", question.header, question.question),
            "text_align": "left"
        }));
        let tag = if question.multi_select { "multi_select_static" } else { "select_static" };
        form_elements.push(json!({
            "tag": tag,
            "name": field_name,
            "placeholder": {
                "tag": "plain_text",
                "content": format!("请选择：{}", question.question)
            },
            "required": true,
            "options": options,
        }));
    }

    form_elements.push(json!({
        "tag": "button",
        "name": "ask_user_submit",
        "type": "primary",
        "text": { "tag": "plain_text", "content": "提交" },
        "behaviors": [{
            "type": "callback",
            "value": { "action": "submit", "request_id": event.request_id }
        }],
        "form_action_type": "submit"
    }));

    elements.push(json!({
        "tag": "form",
        "name": format!("ask_user_{}", event.request_id),
        "elements": form_elements
    }));

    elements.push(json!({
        "tag": "button",
        "name": "ask_user_cancel",
        "text": { "tag": "plain_text", "content": "取消" },
        "behaviors": [{
            "type": "callback",
            "value": { "action": "cancel", "request_id": event.request_id }
        }]
    }));

    json!({
        "schema": "2.0",
        "config": { "update_multi": true, "wide_screen_mode": true },
        "body": { "elements": elements }
    })
}

pub(crate) async fn try_deliver_operation_permission_to_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    request: &PermissionRequestSnapshot,
) -> Result<bool, String> {
    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled {
        return Ok(false);
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(false);
    };

    let card = build_operation_permission_card(request);
    let fallback_text = build_operation_permission_fallback_text(request);
    let outcome =
        send_permission_review_to_target(app_handle, &config, &target, &card, &fallback_text)
            .await?;
    if let Some(interactive_error) = outcome.interactive_error.as_deref() {
        warn!(
            request_id = %request.event.request_id,
            error = %interactive_error,
            "operation permission Feishu delivery fell back to text"
        );
    }

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &outcome.message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id,
            local_message_id: None,
            direction: "outbound",
            payload_type: outcome.payload_type,
        },
    )?;
    let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
    state
        .set_permission_feishu_delivery(
            &request.event.request_id,
            Some(outcome.message_id.clone()),
            target.external_user_id.clone(),
            target.external_chat_id.clone(),
        )
        .await;
    Ok(true)
}

pub(crate) async fn try_deliver_acp_permission_to_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    request: &AcpPermissionRequestSnapshot,
) -> Result<bool, String> {
    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled {
        return Ok(false);
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(false);
    };

    let card = build_acp_permission_card(request);
    let fallback_text = build_acp_permission_fallback_text(request);
    let outcome =
        send_permission_review_to_target(app_handle, &config, &target, &card, &fallback_text)
            .await?;
    if let Some(interactive_error) = outcome.interactive_error.as_deref() {
        warn!(
            request_id = %request.event.request_id,
            error = %interactive_error,
            "ACP permission Feishu delivery fell back to text"
        );
    }

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &outcome.message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id,
            local_message_id: None,
            direction: "outbound",
            payload_type: outcome.payload_type,
        },
    )?;
    let state = app_handle.state::<AcpPermissionState>();
    state
        .set_feishu_delivery(
            &request.event.request_id,
            Some(outcome.message_id.clone()),
            target.external_user_id.clone(),
            target.external_chat_id.clone(),
        )
        .await;
    Ok(true)
}

pub(crate) async fn try_deliver_ask_user_question_to_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    event: &AskUserQuestionRequestEvent,
) -> Result<bool, String> {
    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled {
        return Ok(false);
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(false);
    };
    let card = build_ask_user_question_card(event);
    let external_message_id =
        send_interactive_card_to_target(app_handle, &config, &target, &card).await?;

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &external_message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id,
            local_message_id: None,
            direction: "outbound",
            payload_type: "interactive",
        },
    )?;

    if let Some(scope) = find_active_relay_scope(app_handle, conversation_id, RELAY_ORIGIN_FEISHU)?
    {
        mark_relay_scope_progress(
            app_handle,
            scope.id,
            scope.last_delivered_local_message_id,
            "waiting_user_input",
        )?;
        spawn_feishu_relay_scope_worker(app_handle, scope.id, conversation_id).await;
    }

    Ok(true)
}

async fn send_reply_message_request(
    client: &reqwest::Client,
    config: &FeishuRuntimeConfig,
    token: &str,
    reply_to_message_id: &str,
    payload: Value,
) -> Result<String, String> {
    let url = format!(
        "{}/open-apis/im/v1/messages/{}/reply",
        config.base_url.trim_end_matches('/'),
        reply_to_message_id
    );
    let response = client
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: SendMessageResponse = response.json().await.map_err(|e| e.to_string())?;
    if body.code != 0 {
        return Err(format!("回发飞书消息失败: {}", body.msg));
    }
    body.data
        .map(|data| data.message_id)
        .ok_or_else(|| "飞书回发成功但未返回 message_id".to_string())
}

async fn reply_text_message(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    reply_to_message_id: &str,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    send_reply_message_request(
        &client,
        config,
        &token,
        reply_to_message_id,
        build_feishu_text_payload(text),
    )
    .await
}

async fn send_text_message_to_open_id(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    open_id: &str,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    send_message_request(
        &client,
        config,
        &token,
        "open_id",
        open_id,
        build_feishu_text_payload(text),
    )
    .await
}

async fn send_markdown_message_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    markdown: &str,
) -> Result<FeishuDebugSendResult, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    let mut interactive_error = None;
    let interactive_card = match build_feishu_markdown_card(markdown) {
        Ok(card) => Some(card),
        Err(error) => {
            let error = format!("构建飞书卡片失败: {error}");
            debug!(error = %error, "failed to build feishu markdown card, falling back to raw text");
            interactive_error = Some(error);
            None
        }
    };
    let delivery_mode = if target.reply_to_message_id.is_some() { "reply" } else { "direct" };
    let selected_target = select_receive_target(target)
        .map(|(target_type, target_id)| (target_type.to_string(), target_id.to_string()));

    if let Some(card) = interactive_card.as_ref() {
        let interactive_result = if let Some(reply_to_message_id) =
            target.reply_to_message_id.as_deref()
        {
            send_reply_message_request(
                &client,
                config,
                &token,
                reply_to_message_id,
                build_feishu_interactive_payload(card),
            )
            .await
        } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
            send_message_request(
                &client,
                config,
                &token,
                receive_id_type,
                receive_id,
                build_feishu_interactive_payload(card),
            )
            .await
        } else {
            Err("当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string())
        };

        match interactive_result {
            Ok(message_id) => {
                return Ok(FeishuDebugSendResult {
                    external_message_id: message_id,
                    payload_type: "interactive".to_string(),
                    delivery_mode: delivery_mode.to_string(),
                    reply_to_message_id: target.reply_to_message_id.clone(),
                    target_type: selected_target
                        .as_ref()
                        .map(|(target_type, _)| target_type.clone()),
                    target_id: selected_target.as_ref().map(|(_, target_id)| target_id.clone()),
                    rendered_text: markdown.to_string(),
                    interactive_error,
                    interactive_card,
                });
            }
            Err(error) => {
                warn!(error = %error, "failed to send feishu interactive message, falling back to raw text");
                interactive_error = Some(format!("发送飞书 interactive 卡片失败: {error}"));
            }
        }
    }

    let message_id = if let Some(reply_to_message_id) = target.reply_to_message_id.as_deref() {
        send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_text_payload(markdown),
        )
        .await?
    } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
        send_message_request(
            &client,
            config,
            &token,
            receive_id_type,
            receive_id,
            build_feishu_text_payload(markdown),
        )
        .await?
    } else {
        return Err(
            "当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string()
        );
    };

    Ok(FeishuDebugSendResult {
        external_message_id: message_id,
        payload_type: "text".to_string(),
        delivery_mode: delivery_mode.to_string(),
        reply_to_message_id: target.reply_to_message_id.clone(),
        target_type: selected_target.as_ref().map(|(target_type, _)| target_type.clone()),
        target_id: selected_target.as_ref().map(|(_, target_id)| target_id.clone()),
        rendered_text: markdown.to_string(),
        interactive_error,
        interactive_card,
    })
}

pub(crate) async fn resend_message_to_feishu_for_debug(
    app_handle: &AppHandle,
    message_id: i64,
) -> Result<FeishuDebugSendResult, String> {
    let config = load_runtime_config(app_handle).await?;
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        return Err("飞书 App ID 或 App Secret 未配置，无法执行调试重发".to_string());
    }

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let message = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .read(message_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("未找到消息: {message_id}"))?;

    let rendered_text = render_message_for_external_channel(
        &message,
        &RenderContext { channel: CHANNEL_FEISHU, relay_origin: RELAY_ORIGIN_AIPP },
    )
    .filter(|content| !content.trim().is_empty())
    .ok_or_else(|| "该消息没有可发送到飞书的可读内容".to_string())?;

    let target =
        find_latest_feishu_target(app_handle, message.conversation_id)?.ok_or_else(|| {
            "当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string()
        })?;
    let outcome =
        send_markdown_message_to_target(app_handle, &config, &target, &rendered_text).await?;

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &outcome.external_message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id: message.conversation_id,
            local_message_id: Some(message.id),
            direction: "outbound",
            payload_type: &outcome.payload_type,
        },
    )?;

    Ok(outcome)
}

pub fn debug_build_feishu_markdown_card(markdown: &str) -> Result<Value, String> {
    build_feishu_markdown_card(markdown)
}

pub fn debug_build_feishu_interactive_payload(markdown: &str) -> Result<Value, String> {
    let card = build_feishu_markdown_card(markdown)?;
    Ok(build_feishu_interactive_payload(&card))
}

pub fn debug_describe_feishu_markdown_blocks(markdown: &str) -> Value {
    Value::Array(
        split_markdown_into_feishu_blocks(markdown)
            .into_iter()
            .map(|block| match block {
                FeishuCardBlock::Markdown(content) => json!({
                    "type": "markdown",
                    "content": content,
                }),
                FeishuCardBlock::Table(table) => json!({
                    "type": "table",
                    "headers": table.headers,
                    "rows": table.rows,
                }),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_markdown_blocks_extracts_table() {
        let blocks = split_markdown_into_feishu_blocks(
            "# Title\n\n| Name | Value |\n| --- | --- |\n| A | **1** |\n| B | [2](https://example.com) |\n\nTail",
        );

        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[0], FeishuCardBlock::Markdown(content) if content.contains("# Title"))
        );
        assert!(matches!(
            &blocks[1],
            FeishuCardBlock::Table(table)
                if table.headers == vec!["Name".to_string(), "Value".to_string()]
                && table.rows.len() == 2
        ));
        assert!(
            matches!(&blocks[2], FeishuCardBlock::Markdown(content) if content.contains("Tail"))
        );
    }

    #[test]
    fn split_markdown_blocks_ignores_table_inside_code_fence() {
        let blocks = split_markdown_into_feishu_blocks(
            "```markdown\n| Name | Value |\n| --- | --- |\n| A | B |\n```\n",
        );

        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], FeishuCardBlock::Markdown(content) if content.contains("```markdown"))
        );
    }

    #[test]
    fn build_feishu_markdown_card_uses_markdown_and_table_elements() {
        let card = build_feishu_markdown_card(
            "# Summary\n\n- item 1\n- item 2\n\n| Name | Status |\n| --- | --- |\n| A | ~~done~~ |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["tag"], "markdown");
        assert_eq!(elements[1]["tag"], "table");
        assert_eq!(elements[1]["columns"][0]["display_name"], "Name");
        assert_eq!(elements[1]["rows"][0]["col_2"], "~~done~~");
    }

    #[test]
    fn parse_markdown_table_handles_alignment_escaped_pipes_and_irregular_rows() {
        let table = parse_markdown_table(&[
            "| Name | Value \\| Detail | Score |".to_string(),
            "| :--- | :------------- | ----: |".to_string(),
            "| Alice | `A\\|B` | 42 |".to_string(),
            "| Bob | plain |".to_string(),
            "| Carol | too | many | columns |".to_string(),
        ])
        .expect("table should parse");

        assert_eq!(
            table.headers,
            vec!["Name".to_string(), "Value | Detail".to_string(), "Score".to_string()]
        );
        assert_eq!(
            table.rows,
            vec![
                vec!["Alice".to_string(), "`A|B`".to_string(), "42".to_string()],
                vec!["Bob".to_string(), "plain".to_string(), String::new()],
                vec!["Carol".to_string(), "too".to_string(), "many".to_string()],
            ]
        );
    }

    #[test]
    fn split_markdown_blocks_keeps_invalid_table_like_text_as_markdown() {
        let blocks = split_markdown_into_feishu_blocks(
            "Value A | Value B\nThis line is not a markdown separator\nnext line",
        );

        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            FeishuCardBlock::Markdown(content)
                if content.contains("Value A | Value B")
                && content.contains("This line is not a markdown separator")
        ));
    }

    #[test]
    fn build_feishu_markdown_card_supports_multiple_tables_and_markdown_blocks() {
        let card = build_feishu_markdown_card(
            "前言\n\n| Key | Value |\n| --- | --- |\n| A | 1 |\n\n中间段落\n\n| Env | Status |\n| --- | --- |\n| Prod | **OK** |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 4);
        assert_eq!(elements[0]["tag"], "markdown");
        assert_eq!(elements[1]["tag"], "table");
        assert_eq!(elements[2]["tag"], "markdown");
        assert_eq!(elements[3]["tag"], "table");
        assert_eq!(elements[3]["rows"][0]["col_2"], "**OK**");
    }

    #[test]
    fn build_feishu_markdown_card_preserves_complex_chinese_supplement_table() {
        let card = build_feishu_markdown_card(
            "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n\
             |------|----------|----------|--------------|\n\
             | **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n\
             | **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n\
             | **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n\
             | **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n\
             | **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n\
             | **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n\
             | **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n\
             | **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n\
             | **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["tag"], "table");
        assert_eq!(elements[0]["columns"][0]["display_name"], "补剂");
        assert_eq!(elements[0]["columns"][3]["display_name"], "关键注意事项");
        assert_eq!(elements[0]["rows"].as_array().expect("rows should be array").len(), 9);
        assert_eq!(elements[0]["rows"][0]["col_1"], "**圣约翰草**");
        assert_eq!(elements[0]["rows"][0]["col_2"], "⭐⭐⭐ 最强");
        assert_eq!(
            elements[0]["rows"][0]["col_4"],
            "⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用"
        );
        assert_eq!(elements[0]["rows"][1]["col_1"], "**SAM-e**");
        assert_eq!(elements[0]["rows"][4]["col_2"], "⭐⭐ 缺乏者有效");
        assert_eq!(elements[0]["rows"][5]["col_4"], "配合抗抑郁药使用效果更佳");
        assert_eq!(elements[0]["rows"][8]["col_1"], "**5-HTP**");
        assert_eq!(elements[0]["rows"][8]["col_4"], "⚠️与抗抑郁药同服有血清素综合征风险");
    }

    #[test]
    fn build_feishu_markdown_card_matches_expected_supplement_table_schema() {
        let markdown = "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n\
                        |------|----------|----------|--------------|\n\
                        | **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n\
                        | **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n\
                        | **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n\
                        | **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n\
                        | **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n\
                        | **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n\
                        | **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n\
                        | **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n\
                        | **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |\n";
        let card = build_feishu_markdown_card(markdown).expect("card should be built");

        let expected = json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    {
                        "tag": "table",
                        "element_id": "md_table_1",
                        "row_height": "low",
                        "page_size": 9,
                        "columns": [
                            { "name": "col_1", "display_name": "补剂", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_2", "display_name": "证据强度", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_3", "display_name": "推荐剂量", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_4", "display_name": "关键注意事项", "data_type": "lark_md", "width": "auto" }
                        ],
                        "rows": [
                            { "col_1": "**圣约翰草**", "col_2": "⭐⭐⭐ 最强", "col_3": "900mg/日 (分3次)", "col_4": "⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用" },
                            { "col_1": "**SAM-e**", "col_2": "⭐⭐⭐ 强", "col_3": "800-1600mg/日", "col_4": "⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险" },
                            { "col_1": "**EPA鱼油**", "col_2": "⭐⭐ 中等", "col_3": "EPA 1-2g/日", "col_4": "⚠️与阿司匹林/华法林同服增加出血风险" },
                            { "col_1": "**藏红花**", "col_2": "⭐⭐ 中等", "col_3": "30mg/日", "col_4": "⚠️孕妇禁用" },
                            { "col_1": "**维生素D**", "col_2": "⭐⭐ 缺乏者有效", "col_3": "1000-4000 IU/日", "col_4": "建议先检测水平再补充" },
                            { "col_1": "**L-甲基叶酸**", "col_2": "⭐⭐ 增效剂", "col_3": "7.5-15mg/日", "col_4": "配合抗抑郁药使用效果更佳" },
                            { "col_1": "**NAC**", "col_2": "⭐⭐ 辅助", "col_3": "2000mg/日", "col_4": "哮喘患者慎用" },
                            { "col_1": "**锌**", "col_2": "⭐ 初步", "col_3": "25-50mg/日", "col_4": "长期高剂量导致铜缺乏" },
                            { "col_1": "**5-HTP**", "col_2": "⭐ 有限", "col_3": "100-300mg/日", "col_4": "⚠️与抗抑郁药同服有血清素综合征风险" }
                        ]
                    }
                ]
            }
        });

        assert_eq!(card, expected);
    }

    #[test]
    fn build_feishu_interactive_payload_serializes_card_into_content_string() {
        let card = json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    {
                        "tag": "markdown",
                        "content": "**bold**",
                        "text_align": "left"
                    }
                ]
            }
        });

        let payload = build_feishu_interactive_payload(&card);
        assert_eq!(payload["msg_type"], "interactive");
        assert!(payload.get("card").is_none());

        let content = payload["content"]
            .as_str()
            .expect("interactive content should be a serialized JSON string");
        let reparsed: Value =
            serde_json::from_str(content).expect("interactive content should parse back to JSON");
        assert_eq!(reparsed, card);
    }

    #[test]
    fn build_feishu_interactive_payload_matches_expected_reply_body_for_supplement_table() {
        let card = json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    {
                        "tag": "table",
                        "element_id": "md_table_1",
                        "row_height": "low",
                        "page_size": 9,
                        "columns": [
                            { "name": "col_1", "display_name": "补剂", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_2", "display_name": "证据强度", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_3", "display_name": "推荐剂量", "data_type": "lark_md", "width": "auto" },
                            { "name": "col_4", "display_name": "关键注意事项", "data_type": "lark_md", "width": "auto" }
                        ],
                        "rows": [
                            { "col_1": "**圣约翰草**", "col_2": "⭐⭐⭐ 最强", "col_3": "900mg/日 (分3次)", "col_4": "⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用" },
                            { "col_1": "**SAM-e**", "col_2": "⭐⭐⭐ 强", "col_3": "800-1600mg/日", "col_4": "⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险" },
                            { "col_1": "**EPA鱼油**", "col_2": "⭐⭐ 中等", "col_3": "EPA 1-2g/日", "col_4": "⚠️与阿司匹林/华法林同服增加出血风险" },
                            { "col_1": "**藏红花**", "col_2": "⭐⭐ 中等", "col_3": "30mg/日", "col_4": "⚠️孕妇禁用" },
                            { "col_1": "**维生素D**", "col_2": "⭐⭐ 缺乏者有效", "col_3": "1000-4000 IU/日", "col_4": "建议先检测水平再补充" },
                            { "col_1": "**L-甲基叶酸**", "col_2": "⭐⭐ 增效剂", "col_3": "7.5-15mg/日", "col_4": "配合抗抑郁药使用效果更佳" },
                            { "col_1": "**NAC**", "col_2": "⭐⭐ 辅助", "col_3": "2000mg/日", "col_4": "哮喘患者慎用" },
                            { "col_1": "**锌**", "col_2": "⭐ 初步", "col_3": "25-50mg/日", "col_4": "长期高剂量导致铜缺乏" },
                            { "col_1": "**5-HTP**", "col_2": "⭐ 有限", "col_3": "100-300mg/日", "col_4": "⚠️与抗抑郁药同服有血清素综合征风险" }
                        ]
                    }
                ]
            }
        });

        let expected_payload = json!({
            "msg_type": "interactive",
            "content": card.to_string()
        });

        let payload = build_feishu_interactive_payload(&card);
        assert_eq!(payload, expected_payload);
    }

    #[test]
    fn build_ask_user_question_card_renders_single_and_multi_select_fields() {
        let card = build_ask_user_question_card(&AskUserQuestionRequestEvent {
            request_id: "req-1".to_string(),
            conversation_id: Some(42),
            questions: vec![
                AskUserQuestionItem {
                    question: "选择一个模型".to_string(),
                    header: "模型".to_string(),
                    options: vec![
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "GPT-5.4".to_string(),
                            description: "推荐".to_string(),
                        },
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "Claude".to_string(),
                            description: "保守".to_string(),
                        },
                    ],
                    multi_select: false,
                },
                AskUserQuestionItem {
                    question: "选择输出格式".to_string(),
                    header: "格式".to_string(),
                    options: vec![
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "表格".to_string(),
                            description: "结构化".to_string(),
                        },
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "列表".to_string(),
                            description: "简洁".to_string(),
                        },
                    ],
                    multi_select: true,
                },
            ],
            metadata: None,
        });

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        let form =
            elements.iter().find(|element| element["tag"] == "form").expect("form should exist");
        let form_elements = form["elements"].as_array().expect("form elements should be array");
        assert!(form_elements.iter().any(|element| element["tag"] == "select_static"));
        assert!(form_elements.iter().any(|element| element["tag"] == "multi_select_static"));
        assert_eq!(form["name"], "ask_user_req-1");
        let submit_button = form_elements.last().expect("submit button should exist");
        assert_eq!(submit_button["tag"], "button");
        assert_eq!(submit_button["name"], "ask_user_submit");
        assert_eq!(submit_button["form_action_type"], "submit");
        assert_eq!(submit_button["behaviors"][0]["type"], "callback");
        assert_eq!(submit_button["behaviors"][0]["value"]["action"], "submit");
        assert_eq!(submit_button["behaviors"][0]["value"]["request_id"], "req-1");

        let cancel_button = elements
            .iter()
            .find(|element| element["name"] == "ask_user_cancel")
            .expect("cancel button should exist");
        assert_eq!(cancel_button["tag"], "button");
        assert_eq!(cancel_button["name"], "ask_user_cancel");
        assert_eq!(cancel_button["behaviors"][0]["type"], "callback");
        assert_eq!(cancel_button["behaviors"][0]["value"]["action"], "cancel");
        assert_eq!(cancel_button["behaviors"][0]["value"]["request_id"], "req-1");
    }

    #[test]
    fn parse_permission_reply_command_supports_operation_variants() {
        assert_eq!(
            parse_permission_reply_command("批准一次 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow",
            })
        );
        assert_eq!(
            parse_permission_reply_command("本任务批准 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow_for_conversation",
            })
        );
        assert_eq!(
            parse_permission_reply_command("助手允许 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow_for_assistant",
            })
        );
        assert_eq!(
            parse_permission_reply_command("拒绝 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "deny",
            })
        );
    }

    #[test]
    fn parse_permission_reply_command_supports_acp_variants() {
        assert_eq!(
            parse_permission_reply_command("批准 2 ACP-QWERTY"),
            Some(PermissionReplyCommand::AcpSelect {
                review_code: "ACP-QWERTY".to_string(),
                option_index: 2,
            })
        );
        assert_eq!(
            parse_permission_reply_command("取消 ACP-QWERTY"),
            Some(PermissionReplyCommand::AcpCancel { review_code: "ACP-QWERTY".to_string() })
        );
        assert_eq!(parse_permission_reply_command("批准 0 ACP-QWERTY"), None);
    }

    #[test]
    fn map_ask_user_form_values_to_answers_supports_single_and_multi_select() {
        let questions = vec![
            AskUserQuestionItem {
                question: "选择一个模型".to_string(),
                header: "模型".to_string(),
                options: vec![
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "GPT-5.4".to_string(),
                        description: "推荐".to_string(),
                    },
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "Claude".to_string(),
                        description: "保守".to_string(),
                    },
                ],
                multi_select: false,
            },
            AskUserQuestionItem {
                question: "选择输出格式".to_string(),
                header: "格式".to_string(),
                options: vec![
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "表格".to_string(),
                        description: "结构化".to_string(),
                    },
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "列表".to_string(),
                        description: "简洁".to_string(),
                    },
                ],
                multi_select: true,
            },
        ];
        let form_value = Map::from_iter([
            ("question_0".to_string(), Value::String("GPT-5.4".to_string())),
            (
                "question_1".to_string(),
                Value::Array(vec![
                    Value::String("表格".to_string()),
                    Value::String("列表".to_string()),
                ]),
            ),
        ]);

        let answers = map_ask_user_form_values_to_answers(&questions, &form_value)
            .expect("answers should map");
        assert_eq!(answers.get("选择一个模型"), Some(&"GPT-5.4".to_string()));
        assert_eq!(answers.get("选择输出格式"), Some(&"表格, 列表".to_string()));
    }

    #[test]
    fn feishu_card_action_callback_parses_inner_event_payload() {
        let raw_event = json!({
            "operator": {
                "open_id": "ou_test_user"
            },
            "context": {
                "open_message_id": "om_test_message"
            },
            "action": {
                "value": {
                    "request_id": "req-1",
                    "action": "submit"
                },
                "form_value": {
                    "question_0": "GPT-5.4"
                }
            }
        });

        let callback: FeishuCardActionCallback =
            serde_json::from_value(raw_event).expect("inner event payload should parse");

        assert_eq!(callback.event().operator.open_id, "ou_test_user");
        assert_eq!(
            callback
                .event()
                .context
                .as_ref()
                .and_then(|context| context.open_message_id.as_deref()),
            Some("om_test_message")
        );
        assert_eq!(
            callback
                .event()
                .action
                .value
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn feishu_card_action_callback_parses_enveloped_payload() {
        let raw_event = json!({
            "event": {
                "operator": {
                    "open_id": "ou_test_user"
                },
                "context": {
                    "open_message_id": "om_test_message"
                },
                "action": {
                    "value": {
                        "request_id": "req-1",
                        "action": "submit"
                    },
                    "form_value": {
                        "question_0": "GPT-5.4"
                    }
                }
            }
        });

        let callback: FeishuCardActionCallback =
            serde_json::from_value(raw_event).expect("enveloped payload should parse");

        assert_eq!(callback.event().operator.open_id, "ou_test_user");
        assert_eq!(
            callback
                .event()
                .context
                .as_ref()
                .and_then(|context| context.open_message_id.as_deref()),
            Some("om_test_message")
        );
        assert_eq!(
            callback
                .event()
                .action
                .value
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn find_latest_recoverable_ask_user_tool_call_prefers_pending_or_executing() {
        let base = crate::db::mcp_db::MCPToolCall {
            id: 1,
            conversation_id: 42,
            message_id: None,
            subtask_id: None,
            server_id: 1,
            server_name: "ui_interaction".to_string(),
            tool_name: "ask_user_question".to_string(),
            parameters: "{}".to_string(),
            status: "success".to_string(),
            result: None,
            error: None,
            created_time: "2026-03-18 00:00:00".to_string(),
            started_time: None,
            finished_time: None,
            llm_call_id: None,
            assistant_message_id: None,
        };
        let calls = vec![
            crate::db::mcp_db::MCPToolCall {
                id: 2,
                status: "executing".to_string(),
                ..base.clone()
            },
            crate::db::mcp_db::MCPToolCall {
                id: 3,
                tool_name: "preview_file".to_string(),
                status: "pending".to_string(),
                ..base.clone()
            },
        ];

        let tool_call =
            find_latest_recoverable_ask_user_tool_call(&calls).expect("tool call should exist");
        assert_eq!(tool_call.id, 2);
    }

    #[test]
    fn parse_bot_menu_click_event_extracts_open_id_and_event_key() {
        let raw_event = json!({
            "operator": {
                "operator_id": {
                    "open_id": "ou_test_user"
                }
            },
            "event_key": "feishu::conversation::new",
            "timestamp": 1669364458
        });

        let event = parse_bot_menu_click_event(&raw_event)
            .expect("menu event should parse")
            .expect("menu event should not be empty");

        assert_eq!(
            event,
            FeishuBotMenuClickEvent {
                operator_open_id: "ou_test_user".to_string(),
                event_key: "feishu::conversation::new".to_string(),
            }
        );
    }

    #[test]
    fn feishu_relay_waits_for_finished_assistant_messages() {
        let now = Utc::now();
        let streaming = crate::db::conversation_db::Message {
            id: 1,
            parent_id: None,
            conversation_id: 1,
            message_type: "response".to_string(),
            content: "半句输出".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: None,
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: None,
            parent_group_id: None,
            tool_calls_json: None,
            first_token_time: None,
            ttft_ms: None,
        };
        let finished =
            crate::db::conversation_db::Message { finish_time: Some(now), ..streaming.clone() };
        let tool_result = crate::db::conversation_db::Message {
            message_type: "tool_result".to_string(),
            finish_time: None,
            ..streaming.clone()
        };

        assert!(!is_message_ready_for_feishu_relay(&streaming));
        assert!(is_message_ready_for_feishu_relay(&finished));
        assert!(is_message_ready_for_feishu_relay(&tool_result));
    }
}

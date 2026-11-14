//! 等待用户审批的提醒：系统通知、会话/助手列表的待审批黄点、助手头像的“待审批”动画共用这里的判断。
//!
//! 需要用户亲自审批的来源：
//! - 操作权限与管理操作批准（`OperationState`）、Agent/ACP 权限请求（`AcpPermissionState`）：
//!   这两类请求一出现就已经轮到用户（Codex 自带的自动审批只把升级的请求转给用户）；
//! - MCP 工具调用：未开启自动审核，或自动审核判定有风险/审核失败而停在 pending 的调用。
//!   自动审核进行中、免审核、审核通过直接执行的调用都不算。
//!
//! 系统通知只在用户没看到这条审批时发：各窗口通过 `set_approval_surface` 报告自己正在展示哪些会话的审批，
//! 展示了该审批的窗口都没有聚焦（或者根本没有窗口展示它）时才弹出。

use crate::api::ai::acp::AcpPermissionState;
use crate::api::operation_api::{
    ACP_PERMISSION_REQUEST_EVENT, ACP_PERMISSION_RESOLVED_EVENT, OPERATION_PERMISSION_REQUEST_EVENT,
    OPERATION_PERMISSION_RESOLVED_EVENT,
};
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;
use crate::db::system_db::FeatureConfig;
use crate::mcp::builtin_mcp::OperationState;
use crate::FeatureConfigState;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tracing::{debug, warn};

/// 某个会话的待审批状态可能变化；前端收到后重新读取 `list_pending_approval_conversations`。
pub const APPROVAL_ATTENTION_EVENT: &str = "approval_attention_changed";
/// 同一会话短时间内连续出现多条审批（例如一轮里多个工具调用）只提醒一次。
const NOTIFY_COOLDOWN: Duration = Duration::from_secs(10);
const BODY_CHAR_LIMIT: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApprovalKind {
    /// 窗口级审批弹窗：操作权限、Agent 权限。
    Permission,
    /// 消息流里的工具调用确认卡片，只有正在看这个会话时才看得到。
    ToolCall,
}

#[derive(Debug, Clone, Serialize)]
struct ApprovalAttentionEvent {
    conversation_id: i64,
}

/// 窗口当前展示审批的范围。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ApprovalSurface {
    /// 正在显示消息流的会话（工具调用确认卡片在这里）。
    pub conversations: Vec<i64>,
    /// 这个窗口会弹出审批弹窗的会话。
    pub permissions: Vec<i64>,
}

impl ApprovalSurface {
    fn shows(&self, kind: ApprovalKind, conversation_id: i64) -> bool {
        match kind {
            ApprovalKind::Permission => self.permissions.contains(&conversation_id),
            ApprovalKind::ToolCall => self.conversations.contains(&conversation_id),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct AttentionBook {
    /// 停在待确认的 MCP 工具调用：call_id -> conversation_id。
    held: HashMap<i64, i64>,
    /// 窗口 label -> 展示范围。
    surfaces: HashMap<String, ApprovalSurface>,
    /// 会话最近一次发系统通知的时间。
    notified: HashMap<i64, Instant>,
}

impl AttentionBook {
    /// 记录一条待确认的工具调用；首次记录返回 true。
    pub(crate) fn hold(&mut self, call_id: i64, conversation_id: i64) -> bool {
        self.held.insert(call_id, conversation_id) != Some(conversation_id)
    }

    /// 工具调用离开 pending；之前确实在等待确认时返回所属会话。
    pub(crate) fn release(&mut self, call_id: i64) -> Option<i64> {
        self.held.remove(&call_id)
    }

    pub(crate) fn held_calls(&self) -> Vec<(i64, i64)> {
        self.held.iter().map(|(call, conversation)| (*call, *conversation)).collect()
    }

    pub(crate) fn set_surface(&mut self, label: &str, surface: ApprovalSurface) {
        if surface == ApprovalSurface::default() {
            self.surfaces.remove(label);
        } else {
            self.surfaces.insert(label.to_string(), surface);
        }
    }

    /// 展示这条审批的窗口。
    pub(crate) fn windows_showing(&self, kind: ApprovalKind, conversation_id: i64) -> Vec<String> {
        let mut labels: Vec<String> = self
            .surfaces
            .iter()
            .filter(|(_, surface)| surface.shows(kind, conversation_id))
            .map(|(label, _)| label.clone())
            .collect();
        labels.sort();
        labels
    }

    /// 冷却期内同一会话不重复提醒；允许提醒时记下本次时间。
    pub(crate) fn take_notify_slot(&mut self, conversation_id: i64, now: Instant) -> bool {
        if let Some(last) = self.notified.get(&conversation_id) {
            if now.saturating_duration_since(*last) < NOTIFY_COOLDOWN {
                return false;
            }
        }
        self.notified.retain(|_, last| now.saturating_duration_since(*last) < NOTIFY_COOLDOWN);
        self.notified.insert(conversation_id, now);
        true
    }
}

fn book() -> MutexGuard<'static, AttentionBook> {
    static BOOK: OnceLock<Mutex<AttentionBook>> = OnceLock::new();
    BOOK.get_or_init(|| Mutex::new(AttentionBook::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 默认开启；用户在显示设置里关闭后不再弹审批通知（列表黄点与头像状态不受影响）。
pub(crate) fn notification_enabled(
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> bool {
    config_feature_map
        .get("display")
        .and_then(|config| config.get("notification_on_permission"))
        .map_or(true, |config| !matches!(config.value.as_str(), "false" | "0"))
}

pub(crate) fn truncate_chars(text: &str, limit: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > limit {
        format!("{}...", text.chars().take(limit.saturating_sub(3)).collect::<String>())
    } else {
        text
    }
}

pub(crate) fn is_permission_request_event(event_name: &str) -> bool {
    matches!(event_name, OPERATION_PERMISSION_REQUEST_EVENT | ACP_PERMISSION_REQUEST_EVENT)
}

pub(crate) fn is_permission_resolved_event(event_name: &str) -> bool {
    matches!(event_name, OPERATION_PERMISSION_RESOLVED_EVENT | ACP_PERMISSION_RESOLVED_EVENT)
}

fn json_text<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|text| !text.is_empty())
}

/// 通知正文里描述这次审批请求的内容（与审批弹窗的文案保持一致）。
pub(crate) fn permission_detail(event_name: &str, payload: &serde_json::Value) -> String {
    if event_name == ACP_PERMISSION_REQUEST_EVENT {
        return match json_text(payload, "title").or_else(|| json_text(payload, "kind")) {
            Some(title) => format!("请求权限：{title}"),
            None => "请求工具权限".to_string(),
        };
    }
    let operation = json_text(payload, "operation").unwrap_or_default();
    let path = json_text(payload, "path").unwrap_or_default();
    if operation == "superadmin_execute" {
        let summary = serde_json::from_str::<serde_json::Value>(path)
            .ok()
            .and_then(|detail| json_text(&detail, "操作").map(str::to_string));
        return match summary {
            Some(summary) => format!("管理操作：{summary}"),
            None => "管理操作等待批准".to_string(),
        };
    }
    let label = match operation {
        "read_file" => "读取文件",
        "write_file" => "写入文件",
        "edit_file" => "编辑文件",
        "list_directory" => "列出目录",
        "" => "操作权限",
        other => other,
    };
    if path.is_empty() { label.to_string() } else { format!("{label} {path}") }
}

/// 停住的工具调用在通知里的描述：自动审核的结论优先，未开启审核时只说明等待确认。
pub(crate) fn tool_call_detail(tool_name: &str, review: Option<(&str, &str)>) -> String {
    let reason = review.map(|(_, reason)| reason.trim()).filter(|reason| !reason.is_empty());
    match (review.map(|(verdict, _)| verdict), reason) {
        (Some("risky"), Some(reason)) => format!("{tool_name}：自动审核未通过，{reason}"),
        (Some("risky"), None) => format!("{tool_name}：自动审核未通过，等待你确认"),
        (Some("error"), _) => format!("{tool_name}：自动审核失败，等待你确认"),
        _ => format!("{tool_name} 等待你确认执行"),
    }
}

/// 弹出审批的窗口是否就是用户正在看的窗口。
fn showing_window_focused(app: &AppHandle, kind: ApprovalKind, conversation_id: Option<i64>) -> bool {
    let Some(conversation_id) = conversation_id else {
        // 没有会话归属的请求会在所有聊天窗口弹出。
        return crate::utils::window_utils::is_chat_or_ask_window_focused(app);
    };
    let labels = book().windows_showing(kind, conversation_id);
    labels.iter().any(|label| crate::utils::window_utils::is_window_focused(app, label))
}

struct ConversationLabels {
    assistant: Option<String>,
    conversation: Option<String>,
}

fn conversation_labels(app: &AppHandle, conversation_id: i64) -> ConversationLabels {
    let conversation = ConversationDatabase::new(app)
        .ok()
        .and_then(|db| db.conversation_repo().ok()?.read(conversation_id).ok().flatten());
    let Some(conversation) = conversation else {
        return ConversationLabels { assistant: None, conversation: None };
    };
    let assistant = conversation
        .assistant_id
        .and_then(|id| AssistantDatabase::new(app).ok()?.get_assistant(id).ok().map(|item| item.name))
        .filter(|name| !name.trim().is_empty());
    // 管家会话的名字是系统起的，不如助手名直观；普通会话附上标题方便区分。
    let named = !conversation.conversation_kind.starts_with("butler_");
    let title = Some(conversation.name.trim().to_string()).filter(|name| named && !name.is_empty());
    ConversationLabels { assistant, conversation: title }
}

async fn notify(app: &AppHandle, kind: ApprovalKind, conversation_id: Option<i64>, detail: String) {
    let enabled = match app.try_state::<FeatureConfigState>() {
        Some(state) => notification_enabled(&*state.config_feature_map.lock().await),
        None => false,
    };
    if !enabled {
        return;
    }
    if showing_window_focused(app, kind, conversation_id) {
        debug!(?conversation_id, ?kind, "approval notification skipped: the window showing it is focused");
        return;
    }
    if let Some(id) = conversation_id {
        if !book().take_notify_slot(id, Instant::now()) {
            debug!(conversation_id = id, "approval notification skipped: cooldown");
            return;
        }
    }
    let labels = conversation_id.map(|id| conversation_labels(app, id));
    let title = match labels.as_ref().and_then(|labels| labels.assistant.as_deref()) {
        Some(name) => format!("等待审批 - {name}"),
        None => "等待审批".to_string(),
    };
    let body = match labels.as_ref().and_then(|labels| labels.conversation.as_deref()) {
        Some(name) => format!("「{name}」{detail}"),
        None => detail,
    };
    let body = truncate_chars(&body, BODY_CHAR_LIMIT);
    if let Err(error) = app.notification().builder().title(&title).body(&body).show() {
        warn!(?conversation_id, error = %error, "failed to send approval notification");
    }
}

fn emit_changed(app: &AppHandle, conversation_id: i64) {
    if let Err(error) = app.emit(APPROVAL_ATTENTION_EVENT, ApprovalAttentionEvent { conversation_id }) {
        warn!(conversation_id, error = %error, "failed to emit approval attention change");
    }
}

/// 权限请求事件发出后调用（`emit_permission_request_event`）。
pub(crate) fn permission_requested(
    app: &AppHandle,
    event_name: &str,
    conversation_id: Option<i64>,
    payload: &serde_json::Value,
) {
    if let Some(id) = conversation_id {
        emit_changed(app, id);
    }
    let detail = permission_detail(event_name, payload);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        notify(&app, ApprovalKind::Permission, conversation_id, detail).await;
    });
}

/// 权限请求处理完毕（`emit_permission_resolved_event`）。
pub(crate) fn permission_resolved(app: &AppHandle, conversation_id: Option<i64>) {
    if let Some(id) = conversation_id {
        emit_changed(app, id);
    }
}

/// 保险箱凭据首次访问某个网站，等用户确认（卡片在消息流里）。
pub(crate) fn vault_use_requested(app: &AppHandle, conversation_id: Option<i64>, detail: String) {
    if let Some(id) = conversation_id {
        emit_changed(app, id);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        notify(&app, ApprovalKind::ToolCall, conversation_id, detail).await;
    });
}

pub(crate) fn vault_use_resolved(app: &AppHandle, conversation_id: Option<i64>) {
    if let Some(id) = conversation_id {
        emit_changed(app, id);
    }
}

/// 工具调用没有自动执行、停下来等用户确认。
pub(crate) async fn tool_call_held(app: &AppHandle, call_id: i64) {
    let loaded = MCPDatabase::new(app).map_err(|e| e.to_string()).and_then(|db| {
        let call = db.get_mcp_tool_call(call_id).map_err(|e| e.to_string())?;
        let review = db.get_tool_review_by_call_id(call_id).ok().flatten();
        Ok((call, review))
    });
    let (call, review) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            warn!(call_id, error = %error, "failed to read held tool call for approval attention");
            return;
        }
    };
    if call.status != "pending" {
        return;
    }
    if book().hold(call_id, call.conversation_id) {
        emit_changed(app, call.conversation_id);
    }
    let detail = tool_call_detail(
        &call.tool_name,
        review.as_ref().map(|review| (review.verdict.as_str(), review.reason.as_str())),
    );
    notify(app, ApprovalKind::ToolCall, Some(call.conversation_id), detail).await;
}

/// 工具调用状态离开 pending（执行、拒绝、取消）。
pub(crate) fn tool_call_settled(app: &AppHandle, call_id: i64) {
    if let Some(conversation_id) = book().release(call_id) {
        emit_changed(app, conversation_id);
    }
}

/// 停住的工具调用里仍处于 pending 的会话；已经被别的路径处理掉的记录顺手清理。
fn held_conversation_ids(app: &AppHandle) -> BTreeSet<i64> {
    let held = book().held_calls();
    if held.is_empty() {
        return BTreeSet::new();
    }
    let db = match MCPDatabase::new(app) {
        Ok(db) => db,
        Err(error) => {
            warn!(error = %error, "failed to open MCP database for held tool calls; keeping previous state");
            return held.into_iter().map(|(_, conversation)| conversation).collect();
        }
    };
    let mut ids = BTreeSet::new();
    for (call_id, conversation_id) in held {
        match db.get_mcp_tool_call(call_id) {
            Ok(call) if call.status == "pending" => {
                ids.insert(conversation_id);
            }
            _ => {
                book().release(call_id);
            }
        }
    }
    ids
}

/// 当前所有等待用户审批的会话。
pub(crate) async fn pending_approval_conversation_ids(app: &AppHandle) -> BTreeSet<i64> {
    let mut ids = held_conversation_ids(app);
    if let Some(state) = app.try_state::<OperationState>() {
        ids.extend(state.pending_permission_conversation_ids().await);
    }
    if let Some(state) = app.try_state::<AcpPermissionState>() {
        ids.extend(state.pending_permission_conversation_ids().await);
    }
    ids.extend(crate::vault::approval::pending_conversation_ids());
    ids
}

#[tauri::command]
pub async fn list_pending_approval_conversations(app_handle: AppHandle) -> Result<Vec<i64>, String> {
    Ok(pending_approval_conversation_ids(&app_handle).await.into_iter().collect())
}

/// 窗口报告自己正在展示哪些会话的审批；空列表表示不再展示任何审批。
/// `permission_conversation_ids` 缺省时与 `conversation_ids` 相同（只为当前会话弹审批的窗口）。
#[tauri::command]
pub fn set_approval_surface(
    window: tauri::Window,
    conversation_ids: Vec<i64>,
    permission_conversation_ids: Option<Vec<i64>>,
) {
    let permissions = permission_conversation_ids.unwrap_or_else(|| conversation_ids.clone());
    book().set_surface(window.label(), ApprovalSurface { conversations: conversation_ids, permissions });
}

#[cfg(test)]
#[path = "approval_attention_tests.rs"]
mod tests;

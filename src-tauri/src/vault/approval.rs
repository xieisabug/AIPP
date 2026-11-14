//! Pause-and-ask before a vault value is used in a tool call.
//!
//! `placeholder::prepare` awaits the user's decision; the chat UI shows an inline card. One approval
//! covers exactly one tool call. Pending requests live in memory only: after a restart nothing is
//! approved implicitly (the safe direction).
use super::placeholder::Destination;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

pub const REQUEST_EVENT: &str = "vault-use-request";
pub const RESOLVED_EVENT: &str = "vault-use-resolved";
const TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntryLabel { pub id: String, pub key: String, pub name: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultUseRequest {
    pub request_id: String,
    pub conversation_id: Option<i64>,
    /// The message the tool call belongs to, so the card can sit next to it.
    pub message_id: Option<i64>,
    pub assistant_name: Option<String>,
    /// Entries that need approval for this call (already-allowed ones are not listed).
    pub entries: Vec<EntryLabel>,
    /// `server · tool`.
    pub tool: String,
    pub destination: Destination,
    /// The call as the assistant wrote it, placeholders included: the command text for shell
    /// commands, otherwise pretty JSON arguments. Never contains values.
    pub preview: String,
    pub preview_kind: &'static str,
    /// "Always allow" is offered only for network destinations.
    pub can_always: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision { Deny, Once, Always }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Resolved { request_id: String, conversation_id: Option<i64> }

struct Pending { request: VaultUseRequest, sender: Option<oneshot::Sender<Decision>> }

static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Removes the pending entry and tells every window to close the card, even when the waiting
/// future is dropped (conversation stopped, tool call cancelled).
struct Cleanup { app: AppHandle, request_id: String, conversation_id: Option<i64> }

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(mut pending) = PENDING.lock() { pending.remove(&self.request_id); }
        let payload = Resolved { request_id: self.request_id.clone(), conversation_id: self.conversation_id };
        if let Err(error) = self.app.emit(RESOLVED_EVENT, &payload) {
            tracing::warn!(%error, "Failed to emit vault-use-resolved");
        }
        crate::api::approval_attention::vault_use_resolved(&self.app, self.conversation_id);
    }
}

pub(crate) fn pending_conversation_ids() -> Vec<i64> {
    let mut ids: Vec<i64> = PENDING.lock().map(|p| p.values().filter_map(|e| e.request.conversation_id).collect()).unwrap_or_default();
    ids.extend(super::input::pending_conversation_ids());
    ids
}

/// Ask the user and wait. Errors carry a stable code for the caller.
pub(crate) async fn ask(app: &AppHandle, request: VaultUseRequest) -> Result<Decision, (&'static str, String)> {
    let (sender, receiver) = oneshot::channel();
    let request_id = request.request_id.clone();
    let conversation_id = request.conversation_id;
    PENDING.lock().map_err(|_| ("vault_error", "确认队列锁已损坏".to_string()))?
        .insert(request_id.clone(), Pending { request: request.clone(), sender: Some(sender) });
    let _cleanup = Cleanup { app: app.clone(), request_id, conversation_id };
    app.emit(REQUEST_EVENT, &request).map_err(|e| ("vault_error", format!("无法显示保险箱使用确认: {e}")))?;
    let names = request.entries.iter().map(|e| format!("「{}」", e.name)).collect::<String>();
    crate::api::approval_attention::vault_use_requested(app, conversation_id, format!("在 {} 中使用{names}", request.destination.label));
    match tokio::time::timeout(TIMEOUT, receiver).await {
        Ok(Ok(decision)) => Ok(decision),
        Ok(Err(_)) => Err(("user_denied", "确认请求已被取消".into())),
        Err(_) => Err(("approval_timeout", "等待用户确认超过 10 分钟，已放弃本次调用".into())),
    }
}

#[tauri::command]
pub async fn vault_list_use_requests(conversation_id: Option<i64>) -> Result<Vec<VaultUseRequest>, String> {
    let pending = PENDING.lock().map_err(|_| "确认队列锁已损坏")?;
    let mut list: Vec<VaultUseRequest> = pending.values()
        .filter(|e| conversation_id.is_none() || e.request.conversation_id == conversation_id)
        .map(|e| e.request.clone()).collect();
    list.sort_by_key(|r| r.created_at);
    Ok(list)
}

#[tauri::command]
pub async fn vault_respond_use_request(request_id: String, decision: Decision) -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "确认队列锁已损坏")?;
    let entry = pending.get_mut(&request_id).ok_or("该确认请求已处理或已失效")?;
    if decision == Decision::Always && !entry.request.can_always {
        return Err("本机工具不能设为总是允许".into());
    }
    let sender = entry.sender.take().ok_or("该确认请求已处理或已失效")?;
    sender.send(decision).map_err(|_| "等待确认的请求已结束".to_string())
}

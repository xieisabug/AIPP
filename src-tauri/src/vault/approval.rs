//! Pause-and-ask for credential use toward a site the user has not approved yet.
//!
//! The executor awaits the user's decision; the chat UI shows an inline card. Pending requests live
//! in memory only: after a restart nothing is approved implicitly (the safe direction).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

pub const REQUEST_EVENT: &str = "vault-use-request";
pub const RESOLVED_EVENT: &str = "vault-use-resolved";
const TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultUseRequest {
    pub request_id: String,
    pub conversation_id: Option<i64>,
    pub assistant_name: Option<String>,
    pub credential_id: String,
    pub credential_name: String,
    pub credential_handle: String,
    pub method: String,
    /// The URL as the assistant wrote it (injected query secrets are never part of it).
    pub url: String,
    pub origin: String,
    pub insecure: bool,
    pub header_names: Vec<String>,
    pub has_body: bool,
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
    PENDING.lock().map(|p| p.values().filter_map(|e| e.request.conversation_id).collect()).unwrap_or_default()
}

/// Ask the user and wait. Errors carry a stable code prefix for the executor.
pub(crate) async fn ask(app: &AppHandle, request: VaultUseRequest) -> Result<Decision, (&'static str, String)> {
    let (sender, receiver) = oneshot::channel();
    let request_id = request.request_id.clone();
    let conversation_id = request.conversation_id;
    PENDING.lock().map_err(|_| ("vault_error", "确认队列锁已损坏".to_string()))?
        .insert(request_id.clone(), Pending { request: request.clone(), sender: Some(sender) });
    let _cleanup = Cleanup { app: app.clone(), request_id, conversation_id };
    app.emit(REQUEST_EVENT, &request).map_err(|e| ("vault_error", format!("无法显示凭据使用确认: {e}")))?;
    let detail = format!("使用凭据「{}」访问 {}", request.credential_name, request.origin);
    crate::api::approval_attention::vault_use_requested(app, conversation_id, detail);
    match tokio::time::timeout(TIMEOUT, receiver).await {
        Ok(Ok(decision)) => Ok(decision),
        Ok(Err(_)) => Err(("user_denied", "确认请求已被取消".into())),
        Err(_) => Err(("approval_timeout", "等待用户确认超过 10 分钟，已放弃本次请求".into())),
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
    let sender = PENDING.lock().map_err(|_| "确认队列锁已损坏")?
        .get_mut(&request_id).and_then(|e| e.sender.take())
        .ok_or("该确认请求已处理或已失效")?;
    sender.send(decision).map_err(|_| "等待确认的请求已结束".to_string())
}

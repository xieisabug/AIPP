//! The assistant asks the user to type values it does not have yet (`vault_request_input`).
//!
//! The chat shows a form card. Values go from the card straight into the vault through
//! `vault_submit_input`; the waiting tool call only learns which entries now exist and gets their
//! placeholders back. The user can decline. Nobody to ask (scheduled runs) means failure.
use super::placeholder::{self, UseError};
use crate::sync::personal_vault::{self, CreateInput, ItemMeta, SecretValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

pub const REQUEST_EVENT: &str = "vault-input-request";
pub const RESOLVED_EVENT: &str = "vault-input-resolved";
const TIMEOUT: Duration = Duration::from_secs(600);
const MAX_FIELDS: usize = 5;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputField {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct InputArgs { items: Vec<InputField>, #[serde(default)] purpose: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInputRequest {
    pub request_id: String,
    pub conversation_id: Option<i64>,
    pub message_id: Option<i64>,
    pub assistant_name: Option<String>,
    pub purpose: String,
    pub fields: Vec<InputField>,
    pub created_at: i64,
}

enum Answer { Saved(Vec<ItemMeta>), Declined }

struct Pending { request: VaultInputRequest, sender: Option<oneshot::Sender<Answer>> }

static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Resolved { request_id: String, conversation_id: Option<i64> }

struct Cleanup { app: AppHandle, request_id: String, conversation_id: Option<i64> }

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(mut pending) = PENDING.lock() { pending.remove(&self.request_id); }
        let payload = Resolved { request_id: self.request_id.clone(), conversation_id: self.conversation_id };
        if let Err(error) = self.app.emit(RESOLVED_EVENT, &payload) {
            tracing::warn!(%error, "Failed to emit vault-input-resolved");
        }
        crate::api::approval_attention::vault_use_resolved(&self.app, self.conversation_id);
    }
}

pub(crate) fn pending_conversation_ids() -> Vec<i64> {
    PENDING.lock().map(|p| p.values().filter_map(|e| e.request.conversation_id).collect()).unwrap_or_default()
}

pub(crate) fn validate_fields(fields: &[InputField]) -> Result<(), UseError> {
    if fields.is_empty() || fields.len() > MAX_FIELDS {
        return Err(UseError::new("invalid_request", format!("items 需要 1–{MAX_FIELDS} 项")));
    }
    let mut seen = Vec::new();
    for field in fields {
        personal_vault::validate_key(&field.key).map_err(|e| UseError::new("invalid_request", format!("{}：{e}", field.key)))?;
        if field.name.trim().is_empty() { return Err(UseError::new("invalid_request", format!("{} 缺少名称", field.key))); }
        if seen.contains(&field.key) { return Err(UseError::new("invalid_request", format!("键 {} 重复", field.key))); }
        seen.push(field.key.clone());
    }
    Ok(())
}

fn entry_json(meta: &ItemMeta) -> Value {
    json!({"key": meta.key, "name": meta.name, "placeholder": format!("<vault:{}>", meta.key)})
}

/// Entry point for the `vault_request_input` tool. Returns the JSON shown to the model.
pub(crate) async fn request(app: &AppHandle, conversation_id: Option<i64>, args: &Value) -> Result<Value, UseError> {
    let args: InputArgs = serde_json::from_value(args.clone()).map_err(|e| UseError::new("invalid_request", format!("参数无效: {e}")))?;
    validate_fields(&args.items)?;
    if args.purpose.trim().is_empty() { return Err(UseError::new("invalid_request", "请在 purpose 里说明要用这些值做什么")); }
    let list_app = app.clone();
    let existing = tauri::async_runtime::spawn_blocking(move || personal_vault::list(&list_app)).await
        .map_err(|e| UseError::new("vault_error", e.to_string()))?
        .map_err(|e| UseError::new("vault_unavailable_on_device", e))?;
    let known: Vec<&ItemMeta> = args.items.iter().filter_map(|f| existing.iter().find(|m| m.key == f.key)).collect();
    let missing: Vec<InputField> = args.items.iter().filter(|f| !existing.iter().any(|m| m.key == f.key)).cloned().collect();
    if missing.is_empty() {
        return Ok(json!({"status": "provided", "items": known.iter().map(|m| entry_json(m)).collect::<Vec<_>>(),
            "note": "这些条目已在保险箱中，直接使用占位符即可"}));
    }
    let caller = placeholder::caller(app, conversation_id);
    if !caller.attended {
        return Err(UseError::new("approval_required", "无人值守的任务不能请用户填写，请让用户先在凭据保险箱中保存这些条目"));
    }
    let request = VaultInputRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        conversation_id,
        message_id: crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).ok().flatten(),
        assistant_name: caller.assistant_name,
        purpose: args.purpose.trim().to_owned(),
        fields: missing,
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    match ask(app, request).await? {
        Answer::Declined => Ok(json!({"status": "declined", "note": "用户拒绝填写，不要再追问同样的信息，除非用户主动提供"})),
        Answer::Saved(saved) => {
            if let Some(id) = conversation_id {
                for meta in &saved { placeholder::grant_once(id, &meta.id); }
            }
            let items: Vec<Value> = known.iter().map(|m| entry_json(m)).chain(saved.iter().map(entry_json)).collect();
            Ok(json!({"status": "provided", "items": items,
                "note": "用户已保存这些值。在工具参数中写占位符即可使用；刚填写的值在 10 分钟内第一次使用不需要再次确认"}))
        }
    }
}

async fn ask(app: &AppHandle, request: VaultInputRequest) -> Result<Answer, UseError> {
    let (sender, receiver) = oneshot::channel();
    let request_id = request.request_id.clone();
    let conversation_id = request.conversation_id;
    PENDING.lock().map_err(|_| UseError::new("vault_error", "填写队列锁已损坏"))?
        .insert(request_id.clone(), Pending { request: request.clone(), sender: Some(sender) });
    let _cleanup = Cleanup { app: app.clone(), request_id, conversation_id };
    app.emit(REQUEST_EVENT, &request).map_err(|e| UseError::new("vault_error", format!("无法显示填写卡片: {e}")))?;
    let names = request.fields.iter().map(|f| format!("「{}」", f.name)).collect::<String>();
    crate::api::approval_attention::vault_use_requested(app, conversation_id, format!("请你填写{names}"));
    match tokio::time::timeout(TIMEOUT, receiver).await {
        Ok(Ok(answer)) => Ok(answer),
        Ok(Err(_)) => Err(UseError::new("user_denied", "填写请求已被取消")),
        Err(_) => Err(UseError::new("approval_timeout", "等待用户填写超过 10 分钟，已放弃")),
    }
}

#[tauri::command]
pub async fn vault_list_input_requests(conversation_id: Option<i64>) -> Result<Vec<VaultInputRequest>, String> {
    let pending = PENDING.lock().map_err(|_| "填写队列锁已损坏")?;
    let mut list: Vec<VaultInputRequest> = pending.values()
        .filter(|e| conversation_id.is_none() || e.request.conversation_id == conversation_id)
        .map(|e| e.request.clone()).collect();
    list.sort_by_key(|r| r.created_at);
    Ok(list)
}

/// The user submitted the form. Values are stored here and never returned; on error the card stays
/// open so the user can correct it.
#[tauri::command]
pub async fn vault_submit_input(app_handle: AppHandle, request_id: String, values: BTreeMap<String, SecretValue>, sync: bool) -> Result<(), String> {
    let fields = {
        let pending = PENDING.lock().map_err(|_| "填写队列锁已损坏")?;
        pending.get(&request_id).filter(|e| e.sender.is_some()).ok_or("该填写请求已处理或已失效")?.request.fields.clone()
    };
    let inputs = fields.iter().map(|field| {
        let value = values.get(&field.key).cloned().ok_or_else(|| format!("请填写「{}」", field.name))?;
        Ok(CreateInput {
            name: field.name.trim().to_owned(), key: field.key.clone(), description: field.description.trim().to_owned(),
            value, allowed_sites: vec![], sync_enabled: sync,
        })
    }).collect::<Result<Vec<_>, String>>()?;
    let app = app_handle.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || personal_vault::create_many(&app, &inputs)).await
        .map_err(|e| format!("保险箱任务失败: {e}"))??;
    crate::api::vault_api::notify_changed(&app_handle);
    let sender = PENDING.lock().map_err(|_| "填写队列锁已损坏")?
        .get_mut(&request_id).and_then(|e| e.sender.take()).ok_or("该填写请求已处理或已失效")?;
    sender.send(Answer::Saved(saved)).map_err(|_| "等待填写的请求已结束（值已保存到保险箱）".to_string())
}

#[tauri::command]
pub async fn vault_decline_input(request_id: String) -> Result<(), String> {
    let sender = PENDING.lock().map_err(|_| "填写队列锁已损坏")?
        .get_mut(&request_id).and_then(|e| e.sender.take()).ok_or("该填写请求已处理或已失效")?;
    sender.send(Answer::Declined).map_err(|_| "等待填写的请求已结束".to_string())
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

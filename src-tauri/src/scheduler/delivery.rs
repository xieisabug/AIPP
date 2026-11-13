//! 把一次运行的产出投递给用户：以执行助手的身份在投递会话里发一条消息。
//!
//! 投递幂等：消息的 metadata 带 `scheduled_run_id`，崩溃恢复重放时不会重复发。
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::api::ai::events::ConversationEvent;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::scheduled_task_db::{ScheduledTask, DELIVER_CONVERSATION, DELIVER_OWNER_CHAT, DELIVER_RUN_CONVERSATION};

pub const SCHEDULED_TASK_DELIVERED_EVENT: &str = "scheduled_task_delivered";
/// 投递消息 metadata 的来源标记，前端据此显示「来自定时任务」。
pub const DELIVERY_ORIGIN: &str = "scheduled_task";
const NOTIFICATION_BODY_LIMIT: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivered {
    pub conversation_id: i64,
    pub message_id: i64,
}

pub struct DeliveryContent<'a> {
    pub run_id: &'a str,
    /// 这次运行的计划时间，写进触发消息。
    pub scheduled_for: Option<chrono::DateTime<chrono::Utc>>,
    pub run_conversation_id: i64,
    pub final_message_id: Option<i64>,
    pub text: &'a str,
    pub llm_model_id: Option<i64>,
    pub llm_model_name: Option<String>,
}

pub(crate) fn delivery_metadata(task: &ScheduledTask, content: &DeliveryContent<'_>, role: &str) -> String {
    serde_json::json!({
        "origin": DELIVERY_ORIGIN,
        "role": role,
        "scheduled_task_id": task.id,
        "scheduled_task_name": task.name,
        "scheduled_run_id": content.run_id,
        "run_conversation_id": content.run_conversation_id,
    })
    .to_string()
}

/// 投递结果前的一条用户侧触发消息：保持用户/助手交替（部分模型接口不接受连续两条助手消息），
/// 也让之后的对话知道这条回复来自定时任务。界面不把它显示成用户发言。
pub(crate) fn trigger_text(task: &ScheduledTask, scheduled_for: Option<chrono::DateTime<chrono::Utc>>) -> String {
    match scheduled_for {
        Some(at) => format!("[定时任务] 《{}》（计划 {}）", task.name, crate::scheduler::expr::format_local(at)),
        None => format!("[定时任务] 《{}》", task.name),
    }
}

fn assistant_name(app: &tauri::AppHandle, assistant_id: i64) -> String {
    AssistantDatabase::new(app)
        .ok()
        .and_then(|db| db.get_assistant(assistant_id).ok())
        .map(|a| a.name)
        .unwrap_or_else(|| format!("助手 #{assistant_id}"))
}

/// 解析投递会话。私聊在投递时解析，用户重开私聊后自动投到新的那条。
pub fn resolve_target(app: &tauri::AppHandle, task: &ScheduledTask) -> Result<i64, String> {
    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    match task.deliver_to.as_str() {
        DELIVER_OWNER_CHAT => {
            if let Some(main_id) = crate::api::butler_api::active_butler_main_conversation_id(app)? {
                let main = db.conversation_repo().map_err(|e| e.to_string())?.read(main_id).map_err(|e| e.to_string())?;
                if main.and_then(|c| c.assistant_id) == Some(task.assistant_id) {
                    return Ok(main_id);
                }
            }
            db.conversation_repo()
                .map_err(|e| e.to_string())?
                .open_butler_contact(task.assistant_id, &assistant_name(app, task.assistant_id))
                .map_err(|e| format!("打开执行助手的私聊失败：{e}"))
        }
        DELIVER_CONVERSATION => {
            let id = task.deliver_conversation_id.ok_or("任务没有指定投递会话")?;
            db.conversation_repo()
                .map_err(|e| e.to_string())?
                .read(id)
                .map_err(|e| e.to_string())?
                .map(|_| id)
                .ok_or_else(|| format!("投递会话 #{id} 已删除"))
        }
        other => Err(format!("未知的投递位置 {other}")),
    }
}

fn existing_delivery(db: &ConversationDatabase, conversation_id: i64, run_id: &str, message_type: &str) -> Result<Option<i64>, String> {
    use rusqlite::OptionalExtension;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id FROM message WHERE conversation_id = ?1 AND message_type = ?3 AND json_valid(metadata_json)
           AND json_extract(metadata_json, '$.scheduled_run_id') = ?2 ORDER BY id LIMIT 1",
        rusqlite::params![conversation_id, run_id, message_type],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// 投递产出。`run_conversation` 方式把运行会话本身放进聊天列表。
pub fn deliver(app: &tauri::AppHandle, task: &ScheduledTask, content: &DeliveryContent<'_>) -> Result<Delivered, String> {
    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    let delivered = if task.deliver_to == DELIVER_RUN_CONVERSATION {
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let mut conversation = repo
            .read(content.run_conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or("运行会话已被删除")?;
        conversation.name = format!("定时任务：{}", task.name);
        conversation.conversation_kind = "normal".to_string();
        conversation.is_hidden_from_normal_chat_list = false;
        repo.update(&conversation).map_err(|e| e.to_string())?;
        let message_id = content.final_message_id.ok_or("运行会话里没有可展示的答复")?;
        if let Some(cache) = app.try_state::<crate::NameCacheState>() {
            let name = assistant_name(app, task.assistant_id);
            let assistant_id = task.assistant_id;
            let cache = cache.assistant_names.clone();
            tauri::async_runtime::spawn(async move {
                cache.lock().await.insert(assistant_id, name);
            });
        }
        let _ = app.emit("conversation_created", conversation.id);
        Delivered { conversation_id: conversation.id, message_id }
    } else {
        let conversation_id = resolve_target(app, task)?;
        let message_id = match existing_delivery(&db, conversation_id, content.run_id, "response")? {
            Some(id) => id,
            None => {
                let now = chrono::Utc::now();
                if existing_delivery(&db, conversation_id, content.run_id, "user")?.is_none() {
                    crate::api::ai_api::add_message_with_metadata(
                        app,
                        None,
                        conversation_id,
                        "user".to_string(),
                        trigger_text(task, content.scheduled_for),
                        None,
                        None,
                        Some(now),
                        Some(now),
                        0,
                        None,
                        None,
                        Some(delivery_metadata(task, content, "trigger")),
                    )
                    .map_err(|e| format!("写入触发消息失败：{e}"))?;
                }
                crate::api::ai_api::add_message_with_metadata(
                    app,
                    None,
                    conversation_id,
                    "response".to_string(),
                    content.text.to_string(),
                    content.llm_model_id,
                    content.llm_model_name.clone(),
                    Some(now),
                    Some(now),
                    0,
                    None,
                    None,
                    Some(delivery_metadata(task, content, "result")),
                )
                .map_err(|e| format!("写入投递消息失败：{e}"))?
                .id
            }
        };
        crate::utils::window_utils::send_conversation_event_to_chat_windows(
            app,
            conversation_id,
            ConversationEvent {
                r#type: "message_add".to_string(),
                data: serde_json::json!({ "message_id": message_id, "message_type": "response" }),
            },
        );
        Delivered { conversation_id, message_id }
    };
    let _ = app.emit(
        SCHEDULED_TASK_DELIVERED_EVENT,
        serde_json::json!({
            "taskId": task.id,
            "taskName": task.name,
            "runId": content.run_id,
            "conversationId": delivered.conversation_id,
            "messageId": delivered.message_id,
            "assistantId": task.assistant_id,
        }),
    );
    notify(app, task, content.text);
    Ok(delivered)
}

/// 窗口不在前台时发系统通知；在前台时由会话列表的未读提示承担。
fn notify(app: &tauri::AppHandle, task: &ScheduledTask, text: &str) {
    use crate::utils::window_utils::{is_chat_or_ask_window_focused, is_window_focused};
    if is_chat_or_ask_window_focused(app) || is_window_focused(app, "butler_experiment") {
        return;
    }
    let body = crate::api::approval_attention::truncate_chars(text, NOTIFICATION_BODY_LIMIT);
    let title = format!("{} · 定时任务《{}》", assistant_name(app, task.assistant_id), task.name);
    if let Err(error) = app.notification().builder().title(&title).body(&body).show() {
        tracing::warn!(task_id = task.id, error = %error, "failed to send scheduled task notification");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::scheduled_task_db::DELIVER_OWNER_CHAT;
    use chrono::Utc;

    #[test]
    fn test_delivery_metadata_marks_origin_and_run_for_idempotency() {
        let now = Utc::now();
        let task = ScheduledTask {
            id: 6, name: "周四休息提醒".into(), is_enabled: true, schedule_expr: "0 16 * * 4".into(),
            schedule_anchor: now, next_run_at: None, last_run_at: None, assistant_id: 40,
            task_prompt: "x".into(), deliver_policy: "always".into(), deliver_to: DELIVER_OWNER_CHAT.into(),
            deliver_conversation_id: None, created_in_conversation_id: None, butler_mode: true, ends_at: None,
            max_runs: None, run_count: 0, missed_policy: "run_once".into(), consecutive_failures: 0,
            paused_reason: None, created_time: now, updated_time: now,
        };
        let content = DeliveryContent {
            run_id: "run-9", scheduled_for: None, run_conversation_id: 77, final_message_id: Some(5), text: "今天休息",
            llm_model_id: None, llm_model_name: None,
        };
        let metadata: serde_json::Value = serde_json::from_str(&delivery_metadata(&task, &content, "result")).unwrap();
        assert_eq!(metadata["origin"], DELIVERY_ORIGIN);
        assert_eq!(metadata["role"], "result");
        assert_eq!(trigger_text(&task, None), "[定时任务] 《周四休息提醒》");
        assert_eq!(metadata["scheduled_run_id"], "run-9");
        assert_eq!(metadata["scheduled_task_name"], "周四休息提醒");
        assert_eq!(metadata["run_conversation_id"], 77);
    }
}

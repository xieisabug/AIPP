//! Preserve the existing error message and terminal event wire contract.
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use serde_json::json;
use tauri::Emitter;

pub(crate) fn emit_agent_failure(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    message_id: i64,
    window: &tauri::Window,
    error: &str,
    provider: &str,
) {
    let content = format!("{provider} 运行失败：{error}");
    if let Ok(db) = ConversationDatabase::new(app_handle) {
        if let Ok(repo) = db.message_repo() {
            if let Ok(Some(mut message)) = repo.read(message_id) {
                message.message_type = "error".to_string();
                message.content = content.clone();
                message.finish_time = Some(chrono::Utc::now());
                let _ = repo.update(&message);
            }
        }
    }
    let _ = window.emit(
        format!("conversation_event_{conversation_id}").as_str(),
        ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "error".to_string(),
                content,
                is_done: true,
                token_count: Some(0),
                input_token_count: Some(0),
                output_token_count: Some(0),
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        },
    );
    let _ = window.emit(
        format!("conversation_event_{conversation_id}").as_str(),
        ConversationEvent {
            r#type: "stream_complete".to_string(),
            data: json!({
                "conversation_id": conversation_id,
                "response_message_id": message_id,
                "has_response": false,
                "error": error,
            }),
        },
    );
}

//! Preserve the existing error message and terminal event wire contract.
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::db::conversation_db::{ConversationDatabase, MessageRepository, Repository};
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
            let _ = persist_failure_message(&repo, message_id, &content);
        }
    }
    for event in failure_events(conversation_id, message_id, content, error) {
        let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), event);
    }
}

pub(super) fn persist_failure_message(repo: &MessageRepository, message_id: i64, content: &str) -> rusqlite::Result<()> {
    if let Some(mut message) = repo.read(message_id)? {
        message.message_type = "error".into();
        message.content = content.into();
        message.finish_time = Some(chrono::Utc::now());
        repo.update(&message)?;
    }
    Ok(())
}

pub(super) fn failure_events(conversation_id: i64, message_id: i64, content: String, error: &str) -> [ConversationEvent; 2] {
    [
        ConversationEvent {
            r#type: "message_update".into(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id, message_type: "error".into(), content, is_done: true,
                token_count: Some(0), input_token_count: Some(0), output_token_count: Some(0),
                ttft_ms: None, tps: None,
            }).unwrap(),
        },
        ConversationEvent {
            r#type: "stream_complete".into(),
            data: json!({
                "conversation_id": conversation_id,
                "response_message_id": message_id,
                "has_response": false,
                "error": error,
            }),
        },
    ]
}

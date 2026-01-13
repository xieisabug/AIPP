use crate::db::conversation_db::{ConversationDatabase, ConversationTokenStats, MessageTokenStats};
use tauri::{AppHandle, Manager};

/// 获取对话的token统计信息
#[tauri::command]
pub async fn get_conversation_token_stats(
    app_handle: AppHandle,
    conversation_id: i64,
) -> Result<ConversationTokenStats, String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.get_conversation_token_stats(conversation_id).map_err(|e| e.to_string())
}

/// 获取单个消息的token统计信息
#[tauri::command]
pub async fn get_message_token_stats(
    app_handle: AppHandle,
    message_id: i64,
) -> Result<MessageTokenStats, String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.get_message_token_stats(message_id).map_err(|e| e.to_string())
}

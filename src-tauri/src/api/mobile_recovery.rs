//! Local recovery is observational: never replay messages/tools or alter sync history.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tauri::Manager;
use crate::db::conversation_db::{AttachmentType, ConversationDatabase, MessageAttachment};
use crate::db::mcp_db::MCPDatabase;
use crate::state::activity_state::ConversationActivityManager;

#[derive(Debug, Serialize)]
pub struct MobileRecoverySnapshot {
    pub conversation_id: i64,
    pub local_running: bool,
    pub unknown_tool_ids: Vec<i64>,
    pub incomplete_message_ids: Vec<i64>,
    pub missing_attachment_ids: Vec<i64>,
    pub source: &'static str,
}

pub fn is_unknown_execution(status: &str, local_active: bool) -> bool {
    status == "executing" && !local_active
}

fn attachment_missing(attachment: &MessageAttachment) -> bool {
    if attachment.attachment_type != AttachmentType::Image && attachment.attachment_content.is_some() {
        return false; // Empty text is still a valid inline attachment.
    }
    let url = attachment.attachment_url.as_deref().unwrap_or("");
    if attachment.attachment_type == AttachmentType::Image {
        if attachment.attachment_content.as_deref().and_then(crate::api::ai::conversation::parse_data_url).is_some()
            || crate::api::ai::conversation::parse_data_url(url).is_some() { return false; }
        if reqwest::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some()) {
            return false; // Reference is usable; this read-only snapshot does not probe the remote URL.
        }
    }
    let path = url.strip_prefix("file://").unwrap_or(url);
    path.is_empty() || !std::path::Path::new(path).is_file()
}

#[tauri::command]
pub async fn get_mobile_recovery_snapshot(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<MobileRecoverySnapshot, String> {
    let local_running = app_handle.state::<ConversationActivityManager>()
        .get_runtime_state(conversation_id).await.is_running;
    let (unknown_tool_ids, incomplete_message_ids, missing_attachment_ids) = {
        let mcp = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let unknown = crate::mcp::execution_api::with_local_tool_activity(|local_calls| {
            let calls = mcp.get_mcp_tool_calls_by_conversation(conversation_id).map_err(|e| e.to_string())?;
            Ok::<Vec<i64>, String>(calls.iter().filter(|call| is_unknown_execution(&call.status, local_calls.contains(&call.id)))
                .map(|call| call.id).collect())
        })?;
        let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let rows = db.message_repo().map_err(|e| e.to_string())?
            .list_by_conversation_id(conversation_id).map_err(|e| e.to_string())?;
        let mut incomplete = std::collections::BTreeSet::new();
        let mut missing = std::collections::BTreeSet::new();
        for (message, attachment) in rows {
            if !local_running && matches!(message.message_type.as_str(), "assistant" | "response" | "reasoning")
                && message.start_time.is_some() && message.finish_time.is_none() {
                incomplete.insert(message.id);
            }
            if let Some(attachment) = attachment {
                if attachment_missing(&attachment) { missing.insert(attachment.id); }
            }
        }
        (unknown, incomplete.into_iter().collect(), missing.into_iter().collect())
    };
    Ok(MobileRecoverySnapshot { conversation_id, local_running, unknown_tool_ids, incomplete_message_ids, missing_attachment_ids, source: "local_runtime_and_persisted_history" })
}

fn initialize_drafts(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS mobile_draft (conversation_key TEXT PRIMARY KEY, content TEXT NOT NULL);")
}

fn write_draft(conn: &Connection, key: &str, content: &str) -> rusqlite::Result<()> {
    if content.is_empty() {
        conn.execute("DELETE FROM mobile_draft WHERE conversation_key=?1", [key])?;
    } else {
        conn.execute("INSERT INTO mobile_draft(conversation_key, content) VALUES (?1, ?2) ON CONFLICT(conversation_key) DO UPDATE SET content=excluded.content", params![key, content])?;
    }
    Ok(())
}

fn read_draft(conn: &Connection, key: &str) -> rusqlite::Result<String> {
    Ok(conn.query_row("SELECT content FROM mobile_draft WHERE conversation_key=?1", [key], |row| row.get(0)).optional()?.unwrap_or_default())
}

/// A dedicated app-local DB deliberately outside the synchronized business DB directory.
async fn with_draft_db<T: Send + 'static>(
    app_handle: tauri::AppHandle,
    action: impl FnOnce(&Connection) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let directory = app_handle.path().app_local_data_dir().map_err(|e| e.to_string())?.join("mobile-local-state");
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let conn = Connection::open(directory.join("drafts.db")).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
        initialize_drafts(&conn).map_err(|e| e.to_string())?;
        action(&conn)
    }).await.map_err(|e| format!("local draft worker failed: {e}"))?
}

#[tauri::command]
pub async fn get_mobile_draft(app_handle: tauri::AppHandle, conversation_key: String) -> Result<String, String> {
    with_draft_db(app_handle, move |conn| read_draft(conn, &conversation_key).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn save_mobile_draft(app_handle: tauri::AppHandle, conversation_key: String, content: String) -> Result<(), String> {
    if content.len() > 2 * 1024 * 1024 {
        return Err("草稿超过本机保存上限 2 MiB，当前输入未被清除".to_string());
    }
    with_draft_db(app_handle, move |conn| write_draft(conn, &conversation_key, &content).map_err(|e| e.to_string())).await
}

#[cfg(test)]
#[path = "mobile_recovery_tests.rs"]
mod tests;

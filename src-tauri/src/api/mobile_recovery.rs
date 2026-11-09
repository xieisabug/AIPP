//! Local recovery is observational: never replay messages/tools or alter sync history.
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use serde::Serialize;
use tauri::Manager;
use crate::db::conversation_db::{AttachmentType, ConversationDatabase, MessageAttachment, Repository};
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MobileDraftAttachment {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub attachment_type: i64,
    pub thumbnail: Option<String>,
}

/// 草稿里记录过、但这次无法恢复的附件（文件被删、记录被清理等）。
/// 只回传 id 与可读名称，供前端逐项提示并引导重新选择；不会回传或重新打开任意路径。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MobileDraftMissingAttachment {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
pub struct MobileDraft {
    pub content: String,
    pub attachments: Vec<MobileDraftAttachment>,
    #[serde(default)]
    pub missing_attachments: Vec<MobileDraftMissingAttachment>,
}

fn initialize_drafts(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mobile_draft (
            conversation_key TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            attachments_json TEXT NOT NULL DEFAULT '[]'
        );",
    )?;
    // Older installs only had content; ignore duplicate-column errors.
    let _ = conn.execute(
        "ALTER TABLE mobile_draft ADD COLUMN attachments_json TEXT NOT NULL DEFAULT '[]'",
        [],
    );
    Ok(())
}

fn write_draft(conn: &Connection, key: &str, content: &str, attachment_ids: &[i64]) -> rusqlite::Result<()> {
    let attachments_json = serde_json::to_string(attachment_ids).unwrap_or_else(|_| "[]".to_string());
    if content.is_empty() && attachment_ids.is_empty() {
        conn.execute("DELETE FROM mobile_draft WHERE conversation_key=?1", [key])?;
    } else {
        conn.execute(
            "INSERT INTO mobile_draft(conversation_key, content, attachments_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(conversation_key) DO UPDATE SET
                content=excluded.content,
                attachments_json=excluded.attachments_json",
            params![key, content, attachments_json],
        )?;
    }
    Ok(())
}

fn read_draft_row(conn: &Connection, key: &str) -> rusqlite::Result<(String, Vec<i64>)> {
    let row = conn
        .query_row(
            "SELECT content, attachments_json FROM mobile_draft WHERE conversation_key=?1",
            [key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    match row {
        Some((content, attachments_json)) => {
            let ids: Vec<i64> = serde_json::from_str(&attachments_json).unwrap_or_default();
            Ok((content, ids))
        }
        None => Ok((String::new(), Vec::new())),
    }
}

fn attachment_name(attachment: &MessageAttachment) -> String {
    attachment
        .attachment_url
        .as_deref()
        .map(|url| {
            Path::new(url)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(url)
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("attachment-{}", attachment.id))
}

fn safe_draft_attachment(attachment: MessageAttachment) -> Option<MobileDraftAttachment> {
    // Only restore refs that still have usable local content (inline data or present file).
    // Never re-open arbitrary filesystem paths from the draft JSON alone.
    if attachment_missing(&attachment) {
        return None;
    }
    let path = attachment.attachment_url.clone().unwrap_or_default();
    let name = attachment_name(&attachment);
    let thumbnail = match attachment.attachment_type {
        AttachmentType::Image => attachment
            .attachment_content
            .as_deref()
            .filter(|content| content.starts_with("data:image/"))
            .map(str::to_string),
        _ => None,
    };
    Some(MobileDraftAttachment {
        id: attachment.id,
        name,
        path,
        attachment_type: attachment.attachment_type as i64,
        thumbnail,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ResolvedDraftAttachments {
    restored: Vec<MobileDraftAttachment>,
    missing: Vec<MobileDraftMissingAttachment>,
}

/// 按草稿保存的顺序把附件分成「可恢复」与「无法恢复」两组；重复 id 只处理一次。
fn classify_draft_attachments(
    ids: &[i64],
    mut read: impl FnMut(i64) -> Result<Option<MessageAttachment>, String>,
) -> Result<ResolvedDraftAttachments, String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = ResolvedDraftAttachments::default();
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        match read(id)? {
            Some(attachment) => {
                let name = attachment_name(&attachment);
                match safe_draft_attachment(attachment) {
                    Some(item) => result.restored.push(item),
                    None => result.missing.push(MobileDraftMissingAttachment { id, name }),
                }
            }
            None => result.missing.push(MobileDraftMissingAttachment {
                id,
                name: format!("attachment-{id}"),
            }),
        }
    }
    Ok(result)
}

fn resolve_draft_attachments(
    app_handle: &tauri::AppHandle,
    ids: &[i64],
) -> Result<ResolvedDraftAttachments, String> {
    if ids.is_empty() {
        return Ok(ResolvedDraftAttachments::default());
    }
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let repo = db.attachment_repo().map_err(|e| e.to_string())?;
    classify_draft_attachments(ids, |id| repo.read(id).map_err(|e| e.to_string()))
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
pub async fn get_mobile_draft(app_handle: tauri::AppHandle, conversation_key: String) -> Result<MobileDraft, String> {
    let (content, attachment_ids) = with_draft_db(app_handle.clone(), {
        let conversation_key = conversation_key.clone();
        move |conn| read_draft_row(conn, &conversation_key).map_err(|e| e.to_string())
    }).await?;
    let resolved = resolve_draft_attachments(&app_handle, &attachment_ids)?;
    Ok(MobileDraft {
        content,
        attachments: resolved.restored,
        missing_attachments: resolved.missing,
    })
}

#[tauri::command]
pub async fn save_mobile_draft(
    app_handle: tauri::AppHandle,
    conversation_key: String,
    content: String,
    attachment_ids: Option<Vec<i64>>,
) -> Result<(), String> {
    if content.len() > 2 * 1024 * 1024 {
        return Err("草稿超过本机保存上限 2 MiB，当前输入未被清除".to_string());
    }
    let attachment_ids = attachment_ids.unwrap_or_default();
    if attachment_ids.len() > 20 {
        return Err("草稿附件超过本机保存上限 20 个".to_string());
    }
    // Persist only safe attachment IDs that still resolve locally; drop stale refs.
    let resolved = resolve_draft_attachments(&app_handle, &attachment_ids)?;
    let safe_ids: Vec<i64> = resolved.restored.iter().map(|item| item.id).collect();
    with_draft_db(app_handle, move |conn| {
        write_draft(conn, &conversation_key, &content, &safe_ids).map_err(|e| e.to_string())
    }).await
}

#[cfg(test)]
#[path = "mobile_recovery_tests.rs"]
mod tests;

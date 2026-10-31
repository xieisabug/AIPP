//! Local receipt journal: a received event is never silently retried after an uncertain outcome.
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use crate::db::conversation_db::ConversationDatabase;

fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS feishu_event_journal (
        event_key TEXT PRIMARY KEY, event_type TEXT NOT NULL, message_id TEXT,
        phase TEXT NOT NULL, outcome TEXT NOT NULL, error TEXT,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
}

pub(super) fn identity(payload: &[u8]) -> (String, String, Option<String>) {
    let value: serde_json::Value = serde_json::from_slice(payload).unwrap_or_default();
    let event_type = value.pointer("/header/event_type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let message_id = value.pointer("/event/message/message_id").and_then(|v| v.as_str()).map(str::to_string);
    let key = value.pointer("/header/event_id").and_then(|v| v.as_str()).filter(|v| !v.is_empty())
        .map(|v| format!("event:{v}"))
        .or_else(|| message_id.as_ref().map(|v| format!("message:{v}")))
        .unwrap_or_else(|| format!("sha256:{}", hex::encode(Sha256::digest(payload))));
    (key, event_type, message_id)
}

fn begin(conn: &Connection, key: &str, kind: &str, message_id: Option<&str>) -> rusqlite::Result<bool> {
    init(conn)?;
    Ok(conn.execute("INSERT OR IGNORE INTO feishu_event_journal (event_key,event_type,message_id,phase,outcome) VALUES (?1,?2,?3,'handle_payload','processing')", params![key, kind, message_id])? == 1)
}

fn finish(conn: &Connection, key: &str, outcome: &str, error: Option<&str>) -> rusqlite::Result<()> {
    conn.execute("UPDATE feishu_event_journal SET outcome=?1,error=?2,updated_at=CURRENT_TIMESTAMP WHERE event_key=?3 AND outcome='processing'", params![outcome, error, key])?;
    Ok(())
}

fn recover(conn: &Connection) -> rusqlite::Result<usize> {
    init(conn)?;
    conn.execute("UPDATE feishu_event_journal SET outcome='unknown',error='事件处理被中断，结果未知；不会自动重发',updated_at=CURRENT_TIMESTAMP WHERE outcome='processing'", [])
}

fn last_problem(conn: &Connection) -> rusqlite::Result<Option<String>> {
    init(conn)?;
    conn.query_row("SELECT event_key,event_type,COALESCE(message_id,''),phase,outcome,COALESCE(error,'') FROM feishu_event_journal WHERE outcome IN ('failed','unknown') ORDER BY updated_at DESC,rowid DESC LIMIT 1", [], |row| {
        Ok(format!("飞书事件 event_id={}; type={}; message_id={}; stage={}; outcome={}; {}", row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?))
    }).optional()
}

fn connection(app: &AppHandle) -> Result<Connection, String> {
    ConversationDatabase::new(app).map_err(|e| e.to_string())?.get_connection().map_err(|e| e.to_string())
}

pub(super) fn start(app: &AppHandle, payload: &[u8]) -> Result<Option<EventGuard>, String> {
    let (key, kind, message_id) = identity(payload);
    let conn = connection(app)?;
    if !begin(&conn, &key, &kind, message_id.as_deref()).map_err(|e| e.to_string())? { return Ok(None); }
    Ok(Some(EventGuard { app: app.clone(), key, finished: false }))
}

pub(super) fn recover_interrupted(app: &AppHandle) -> Result<usize, String> {
    recover(&connection(app)?).map_err(|e| e.to_string())
}

pub(super) fn latest_problem(app: &AppHandle) -> Result<Option<String>, String> {
    last_problem(&connection(app)?).map_err(|e| e.to_string())
}

pub(super) struct EventGuard { app: AppHandle, key: String, finished: bool }

impl EventGuard {
    pub(super) fn complete(&mut self, result: Result<(), String>) -> Result<(), String> {
        let (outcome, error) = match &result { Ok(()) => ("complete", None), Err(error) => ("failed", Some(error.as_str())) };
        finish(&connection(&self.app)?, &self.key, outcome, error).map_err(|e| format!("event_id={}; journal finish: {e}", self.key))?;
        self.finished = true;
        result
    }
}

impl Drop for EventGuard {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = connection(&self.app).and_then(|conn| finish(&conn, &self.key, "unknown", Some("事件处理被中断，结果未知；不会自动重发")).map_err(|e| e.to_string())) {
                tracing::error!(event_id = %self.key, error = %error, "failed to persist interrupted Feishu event");
            }
        }
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;

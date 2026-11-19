use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

tokio::task_local! {
    /// Original bridge parameters, before vault expansion or temporary relay URL generation.
    pub(crate) static PREVIEW_ARGUMENTS: String;
}

fn schema(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_file_preview (
        id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
        anchor INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE, request_id TEXT NOT NULL UNIQUE,
        occurred_ms REAL NOT NULL, parameters TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_butler_file_preview_conversation ON butler_file_preview(conversation_id);
        CREATE TABLE IF NOT EXISTS butler_interaction_event (
            id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
            anchor INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE, occurred_ms INTEGER NOT NULL,
            kind TEXT NOT NULL, request_id TEXT NOT NULL UNIQUE, payload TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_butler_interaction_event_conversation ON butler_interaction_event(conversation_id)")?;
    Ok(())
}

pub fn is_group_member(conn: &Connection, conversation: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1 AND conversation_kind='butler_group_member')",
        [conversation], |r| r.get(0))?)
}

/// Display history only. The original execution owns any pending interaction.
pub fn save_event(conn: &Connection, cid: i64, anchor: i64, kind: &str, request_id: &str, payload: &str) -> Result<()> {
    schema(conn)?;
    conn.execute("INSERT INTO butler_interaction_event(conversation_id,anchor,occurred_ms,kind,request_id,payload)
        SELECT ?1,id,?3,?4,?5,?6 FROM message WHERE id=?2 AND conversation_id=?1",
        params![cid, anchor, chrono::Utc::now().timestamp_millis(), kind, request_id, payload])?;
    Ok(())
}

pub fn save_preview(conn: &Connection, cid: i64, anchor: i64, request: &str, occurred_ms: f64, raw: &str) -> Result<()> {
    schema(conn)?;
    conn.execute("INSERT INTO butler_file_preview(conversation_id,anchor,request_id,occurred_ms,parameters)
        VALUES(?1,?2,?3,?4,?5)", params![cid,anchor,request,occurred_ms,raw])?;
    Ok(())
}

pub fn finish_code(conn: &Connection, cid: i64, request: &str, status: &str) -> Result<()> {
    conn.execute("UPDATE butler_interaction_event SET payload=json_set(payload,'$.status',?3)
        WHERE conversation_id=?1 AND request_id=?2 AND kind='code'", params![cid, request, status])?;
    Ok(())
}

#[derive(Serialize)]
pub struct InteractionRow {
    id: i64, kind: String, request_id: String, anchor: Option<i64>, occurred_ms: f64,
    request: Value, answers: Option<Value>,
}

fn list(conn: &Connection, cid: i64) -> Result<Vec<InteractionRow>> {
    schema(conn)?;
    super::butler_goal::schema(conn)?;
    super::butler_question_history::schema(conn)?;
    // Legacy questions contain no message ID. Their expiry was set to exactly one day after asking.
    let mut stmt = conn.prepare("SELECT q.rowid,'question',q.request_id,json_extract(q.event_json,'$.message_id'),
        coalesce(json_extract(q.event_json,'$.occurred_ms'),(q.expires_at-86400)*1000),q.event_json,
        json_extract(a.record_json,'$.answers')
        FROM butler_goal_question q LEFT JOIN butler_task_question_answer a ON a.task_conversation_id=q.conversation_id
        AND a.question_key='request:'||q.request_id AND json_extract(a.record_json,'$.delivery_status')='delivered'
        WHERE q.conversation_id=?1 AND (q.state='waiting' OR (q.state='answered' AND a.record_json IS NOT NULL
        AND coalesce(json_extract(a.record_json,'$.cancelled'),0)=0))
        AND (json_extract(q.event_json,'$.call_id') IS NULL OR json_extract(q.event_json,'$.recovered')=1)
        UNION ALL SELECT id,'file',request_id,anchor,occurred_ms,'null',NULL FROM butler_file_preview
        WHERE conversation_id=?1
        UNION ALL SELECT g.id,g.kind,g.request_id,g.anchor,g.occurred_ms,g.payload,json_extract(a.record_json,'$.answers')
        FROM butler_interaction_event g LEFT JOIN butler_task_question_answer a ON a.task_conversation_id=g.conversation_id
        AND a.question_key='request:'||g.request_id AND json_extract(a.record_json,'$.delivery_status')='delivered'
        WHERE g.conversation_id=?1 AND (g.kind='report' OR coalesce(json_extract(a.record_json,'$.cancelled'),0)=0)
        ORDER BY 5,1")?;
    let mut rows = stmt.query([cid])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(InteractionRow {
            id: row.get(0)?, kind: row.get(1)?, request_id: row.get(2)?, anchor: row.get(3)?,
            occurred_ms: row.get(4)?, request: serde_json::from_str(&row.get::<_,String>(5)?)?,
            answers: row.get::<_,Option<String>>(6)?.map(|s| serde_json::from_str(&s)).transpose()?,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn list_butler_interactions(app_handle: tauri::AppHandle, conversation_id: i64) -> Result<Vec<InteractionRow>, String> {
    let conn = super::conversation_db::ConversationDatabase::new(&app_handle)
        .and_then(|db| db.get_connection()).map_err(|e| e.to_string())?;
    list(&conn, conversation_id).map_err(|e| e.to_string())
}

fn preview_parameters(conn: &Connection, cid: i64, request: &str) -> Result<String> {
    Ok(conn.query_row("SELECT parameters FROM butler_file_preview WHERE conversation_id=?1 AND request_id=?2",
        params![cid, request], |row| row.get(0))?)
}

#[tauri::command]
pub async fn prepare_butler_file_preview(app_handle: tauri::AppHandle, conversation_id: i64, request_id: String)
    -> Result<crate::mcp::builtin_mcp::interaction::PreviewFileRequestEvent, String>
{
    let raw = {
        let conn = super::conversation_db::ConversationDatabase::new(&app_handle)
            .and_then(|db| db.get_connection()).map_err(|e| e.to_string())?;
        preview_parameters(&conn, conversation_id, &request_id)
            .map_err(|e| format!("读取会话 {conversation_id} 的文件预览 {request_id} 失败：{e}"))?
    };
    let request = serde_json::from_str(&raw).map_err(|e| format!("解析文件预览 {request_id} 失败：{e}"))?;
    crate::mcp::builtin_mcp::interaction::prepare_preview_file_request_for_ui(app_handle, Some(conversation_id), request).await
}

#[cfg(test)]
#[path = "butler_interactions_tests.rs"]
mod tests;

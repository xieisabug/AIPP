use crate::api::ai::{acp::{AcpElicitationState, AcpPermissionState}, events::ConversationRuntimeState};
use crate::db::conversation_db::ConversationDatabase;
use crate::mcp::builtin_mcp::{interaction::InteractionState, OperationState};
use crate::state::activity_state::ConversationActivityManager;
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct ContactActivity {
    assistant_id: i64,
    conversation_id: i64,
    status: Option<String>,
    waiting_peer: bool,
    runtime: Option<ConversationRuntimeState>,
    permission: bool,
    question: bool,
}

fn read_activity(conn: &Connection, main: i64) -> rusqlite::Result<Vec<ContactActivity>> {
    // Independent of the task panel's pagination; private contacts and task executors
    // share the same assistant identity. Old main conversations are not displayed.
    let mut stmt = conn.prepare("SELECT CASE WHEN c.id=?1 THEN 0 ELSE COALESCE(d.executor_assistant_id,c.assistant_id) END,
        c.id,c.butler_task_status,
        EXISTS(SELECT 1 FROM butler_peer_exchange p WHERE p.owner=c.id AND p.state IN ('queued','sending','waiting','returning','reply_sending'))
        FROM conversation c LEFT JOIN butler_task_definition d ON d.task_conversation_id=c.id
        WHERE c.id=?1 OR (c.conversation_kind IN ('butler_contact','butler_task')
            AND COALESCE(d.executor_assistant_id,c.assistant_id) IS NOT NULL)")?;
    let rows = stmt.query_map([main], |r| Ok(ContactActivity {
        assistant_id: r.get(0)?, conversation_id: r.get(1)?, status: r.get(2)?, waiting_peer: r.get(3)?,
        runtime: None, permission: false, question: false,
    }))?;
    rows.collect()
}

#[tauri::command]
pub async fn list_butler_contact_activity(app_handle: AppHandle, main_conversation_id: i64) -> Result<Vec<ContactActivity>, String> {
    let app = app_handle.clone();
    let mut rows = tauri::async_runtime::spawn_blocking(move || {
        let db = ConversationDatabase::new(&app).map_err(|e| e.to_string())?;
        let conn = db.get_connection().map_err(|e| e.to_string())?;
        read_activity(&conn, main_conversation_id).map_err(|e| e.to_string())
    }).await.map_err(|e| format!("读取联系人活动任务失败: {e}"))?
        .map_err(|e| format!("读取联系人活动会话失败 (main={main_conversation_id}): {e}"))?;
    for row in &mut rows {
        let id = row.conversation_id;
        row.runtime = Some(app_handle.state::<ConversationActivityManager>().get_runtime_state(id).await);
        row.permission = app_handle.state::<OperationState>().has_pending_permission_for_conversation(id).await
            || app_handle.state::<AcpPermissionState>().has_pending_permission_for_conversation(id).await;
        row.question = !app_handle.state::<InteractionState>().list_requests_for_conversation(id).await.is_empty()
            || app_handle.state::<AcpElicitationState>().has_pending_request_for_conversation(id).await;
    }
    Ok(rows)
}

#[cfg(test)]
#[path = "butler_contact_activity_tests.rs"]
mod tests;

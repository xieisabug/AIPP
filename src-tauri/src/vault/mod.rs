//! Personal credential vault: placeholders in tool calls, approval and fill-in requests, the HTTP
//! tool and configuration references. Storage and encryption live in `crate::sync::personal_vault`.
pub mod approval;
pub mod config_ref;
pub mod executor;
pub mod input;
pub mod placeholder;
pub mod tool;

// Capture once when a request enters the queue, not each time the UI re-renders.
fn group_anchor(app: &tauri::AppHandle, conversation: Option<i64>, message: Option<i64>) -> Option<crate::db::butler_group::RequestAnchor> {
    let (conversation, message) = (conversation?, message?);
    let result = (|| {
        let db = crate::db::conversation_db::ConversationDatabase::new(app)?;
        let conn = db.get_connection()?;
        crate::db::butler_group::credential_request_anchor(&conn, conversation, message)
    })();
    match result {
        Ok(anchor) => anchor,
        Err(error) => { tracing::warn!(%error, conversation, message, "Failed to resolve Vault group message anchor"); None }
    }
}

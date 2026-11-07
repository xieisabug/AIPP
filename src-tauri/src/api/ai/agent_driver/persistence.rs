//! Persist native session identity only while its captured actor still owns the
//! conversation. The caller holds the native map lock through this synchronous
//! function; the binding lock is held through the SQLite write.
use super::events::{AgentEventIdentity, matches_native_run};
use crate::db::conversation_db::ConversationDatabase;

pub(crate) fn persist_session_id(
    app: &tauri::AppHandle, identity: &AgentEventIdentity,
    current_native_run: Option<&str>, native_session_id: &str,
) -> Result<(), String> {
    if !current_native_run.is_some_and(|run| matches_native_run(run, identity.run_id.as_deref())) {
        return Err(format!("Native session persistence rejected for unregistered or replaced actor [driver={}, conversation_id={}, run_id={:?}]",
            identity.driver_id, identity.conversation_id, identity.run_id));
    }
    let instance_id = identity.instance_id.as_deref().map(str::parse::<i64>).transpose()
        .map_err(|error| format!("Native session instance identity is invalid: {error}"))?;
    let db = ConversationDatabase::new(app).map_err(|error| format!("Native session persistence database unavailable: {error}"))?;
    let mut result = Ok(());
    let current = super::lifecycle::publish_if_current_binding(app, identity.conversation_id,
        &identity.driver_id, identity.run_id.as_deref(), || {
            result = db.upsert_agent_session_id_for_instance(identity.conversation_id,
                &identity.driver_id, instance_id, native_session_id).map_err(|error| error.to_string());
        }).map_err(|error| error.to_string())?;
    if !current {
        return Err(format!("Native session persistence rejected because its provider/run binding was replaced [driver={}, conversation_id={}, run_id={:?}]",
            identity.driver_id, identity.conversation_id, identity.run_id));
    }
    result
}

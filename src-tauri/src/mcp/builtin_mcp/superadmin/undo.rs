use serde_json::json;
use tauri::AppHandle;
use tracing::{info, warn};

use super::audit;
use super::registry::ActionRegistry;
use super::types::RiskLevel;
use crate::db::conversation_db::ConversationDatabase;

/// Handle the `superadmin_undo` tool call.
///
/// Flow:
/// 1. Look up the original audit entry by `audit_id`
/// 2. Validate it is undoable (successful, not dry_run, has snapshot, not already undone)
/// 3. Look up the handler from the registry
/// 4. Call `handler.undo(snapshot, original_args)` to apply the reverse
/// 5. Create a new audit entry for the undo operation itself
/// 6. Mark the original audit entry as undone
pub async fn handle_undo(
    app_handle: &AppHandle,
    registry: &ActionRegistry,
    audit_id: &str,
    reason: Option<&str>,
    butler_conversation_id: Option<i64>,
) -> serde_json::Value {
    // 1. Get DB connection
    let db = match ConversationDatabase::new(app_handle) {
        Ok(db) => db,
        Err(e) => {
            return error_result(&format!("Failed to open database: {}", e));
        }
    };
    let conn = match db.get_connection() {
        Ok(c) => c,
        Err(e) => {
            return error_result(&format!("Failed to get connection: {}", e));
        }
    };

    // 2. Look up the original audit entry
    let entry = match audit::get_audit_entry(&conn, audit_id) {
        Ok(Some(e)) => e,
        Ok(None) => {
            return error_result(&format!("Audit entry not found: {}", audit_id));
        }
        Err(e) => {
            return error_result(&format!("Failed to query audit entry: {}", e));
        }
    };

    // 3. Validate undoability
    if !entry.success {
        return error_result(&format!(
            "Cannot undo audit_id={}: the original action failed (success=false)",
            audit_id
        ));
    }
    if entry.dry_run {
        return error_result(&format!(
            "Cannot undo audit_id={}: it was a dry_run (nothing was actually executed)",
            audit_id
        ));
    }
    if entry.is_undone {
        return error_result(&format!(
            "Cannot undo audit_id={}: it has already been undone (undo_audit_id={})",
            audit_id,
            entry.undo_audit_id.as_deref().unwrap_or("?")
        ));
    }
    let snapshot_json = match &entry.before_snapshot_json {
        Some(s) => s.clone(),
        None => {
            return error_result(&format!(
                "Cannot undo audit_id={}: no before_snapshot was captured for action '{}'",
                audit_id, entry.action_id
            ));
        }
    };

    // 4. Parse snapshot and original args
    let snapshot: serde_json::Value = match serde_json::from_str(&snapshot_json) {
        Ok(v) => v,
        Err(e) => {
            return error_result(&format!("Failed to parse before_snapshot: {}", e));
        }
    };
    let original_args: serde_json::Value = entry
        .args_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(json!({}));

    // 5. Get the handler from registry
    let registered = match registry.get(&entry.action_id) {
        Some(r) => r,
        None => {
            return error_result(&format!(
                "Action '{}' is no longer registered – cannot undo",
                entry.action_id
            ));
        }
    };

    // 6. Call handler.undo()
    let undo_result = registered
        .handler
        .undo(app_handle, &snapshot, &original_args)
        .await;

    let undo_audit_id = audit::generate_audit_id();

    match undo_result {
        Ok(undo_value) => {
            info!(
                original_audit_id = audit_id,
                action_id = %entry.action_id,
                "SuperAdmin undo succeeded"
            );

            // 7. Create audit entry for the undo operation
            let undo_args = json!({
                "audit_id": audit_id,
                "reason": reason,
                "original_action_id": entry.action_id,
            });
            let undo_args_str = serde_json::to_string(&undo_args).ok();
            let undo_result_str = serde_json::to_string(&undo_value).ok();

            if let Err(e) = audit::insert_audit_log(
                &conn,
                &undo_audit_id,
                &format!("undo:{}", entry.action_id),
                &entry.domain,
                RiskLevel::MEDIUM,
                undo_args_str.as_deref(),
                reason,
                false,
                false,
                true,
                undo_result_str.as_deref(),
                None,
                butler_conversation_id,
                "butler",
                None, // undo operations themselves don't need snapshots
            ) {
                warn!(error = %e, "Failed to write undo audit log");
            }

            // 8. Mark original as undone
            if let Err(e) = audit::mark_audit_undone(&conn, audit_id, &undo_audit_id) {
                warn!(error = %e, "Failed to mark original audit as undone");
            }

            json!({
                "content": [{"type": "json", "json": {
                    "success": true,
                    "audit_id": undo_audit_id,
                    "original_audit_id": audit_id,
                    "original_action_id": entry.action_id,
                    "undo_result": undo_value,
                }}],
                "isError": false
            })
        }
        Err(err) => {
            warn!(
                original_audit_id = audit_id,
                action_id = %entry.action_id,
                error = %err,
                "SuperAdmin undo failed"
            );

            // Record the failed undo attempt
            let undo_args = json!({
                "audit_id": audit_id,
                "reason": reason,
                "original_action_id": entry.action_id,
            });
            let undo_args_str = serde_json::to_string(&undo_args).ok();

            if let Err(e) = audit::insert_audit_log(
                &conn,
                &undo_audit_id,
                &format!("undo:{}", entry.action_id),
                &entry.domain,
                RiskLevel::MEDIUM,
                undo_args_str.as_deref(),
                reason,
                false,
                false,
                false,
                None,
                Some(&err),
                butler_conversation_id,
                "butler",
                None,
            ) {
                warn!(error = %e, "Failed to write failed undo audit log");
            }

            error_result(&format!(
                "Undo failed for audit_id={}, action={}: {}",
                audit_id, entry.action_id, err
            ))
        }
    }
}

fn error_result(message: &str) -> serde_json::Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

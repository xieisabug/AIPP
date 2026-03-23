use tauri::AppHandle;
use tracing::{error, info, instrument};

use super::audit;
use super::registry::ActionRegistry;
use super::types::*;
use crate::db::conversation_db::ConversationDatabase;

/// Handle the `superadmin_execute` tool call.
#[instrument(skip(app_handle, registry))]
pub async fn handle_execute(
    app_handle: &AppHandle,
    registry: &ActionRegistry,
    request: ExecuteRequest,
    butler_conversation_id: Option<i64>,
) -> ExecuteResult {
    let audit_id = audit::generate_audit_id();

    // 1. Resolve action
    let registered = match registry.get(&request.action_id) {
        Some(r) => r,
        None => {
            return ExecuteResult {
                success: false,
                action_id: request.action_id.clone(),
                risk_level: RiskLevel::SAFE,
                approval_used: false,
                result: serde_json::Value::Null,
                audit_id,
                error: Some(format!("Action not found: {}", request.action_id)),
            };
        }
    };

    let meta = &registered.meta;

    // 2. Risk gate – high-risk actions require approval (not auto-executed)
    if meta.risk_level.0 >= 3 && !request.dry_run {
        let needs_approval = meta.requires_approval;
        if needs_approval {
            // For Phase 1 we reject high-risk actions outright; Phase 3 will add
            // interactive approval via ask_user_question.
            let err_msg = format!(
                "Action '{}' has risk_level={} and requires user approval. \
                 Use dry_run=true to preview, or request approval through ask_user_question first.",
                request.action_id, meta.risk_level
            );
            log_audit(
                app_handle,
                &audit_id,
                meta,
                &request,
                false,
                None,
                Some(&err_msg),
                false,
                butler_conversation_id,
            );
            return ExecuteResult {
                success: false,
                action_id: request.action_id.clone(),
                risk_level: meta.risk_level,
                approval_used: false,
                result: serde_json::Value::Null,
                audit_id,
                error: Some(err_msg),
            };
        }
    }

    // 3. Execute via handler
    let handler_result = registered
        .handler
        .execute(app_handle, request.args.clone(), request.dry_run)
        .await;

    match handler_result {
        Ok(result_value) => {
            info!(
                action_id = %request.action_id,
                risk_level = %meta.risk_level,
                dry_run = request.dry_run,
                "SuperAdmin action executed successfully"
            );

            log_audit(
                app_handle,
                &audit_id,
                meta,
                &request,
                true,
                Some(&result_value),
                None,
                false,
                butler_conversation_id,
            );

            ExecuteResult {
                success: true,
                action_id: request.action_id.clone(),
                risk_level: meta.risk_level,
                approval_used: false,
                result: result_value,
                audit_id,
                error: None,
            }
        }
        Err(err) => {
            error!(
                action_id = %request.action_id,
                error = %err,
                "SuperAdmin action execution failed"
            );

            log_audit(
                app_handle,
                &audit_id,
                meta,
                &request,
                false,
                None,
                Some(&err),
                false,
                butler_conversation_id,
            );

            ExecuteResult {
                success: false,
                action_id: request.action_id.clone(),
                risk_level: meta.risk_level,
                approval_used: false,
                result: serde_json::Value::Null,
                audit_id,
                error: Some(err),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: persist audit record
// ---------------------------------------------------------------------------

fn log_audit(
    app_handle: &AppHandle,
    audit_id: &str,
    meta: &ActionMeta,
    request: &ExecuteRequest,
    success: bool,
    result: Option<&serde_json::Value>,
    error: Option<&str>,
    approval_used: bool,
    butler_conversation_id: Option<i64>,
) {
    let args_json = serde_json::to_string(&request.args).ok();
    let result_json = result.and_then(|v| serde_json::to_string(v).ok());

    // Best-effort audit logging – do not fail the action if audit DB write fails
    match ConversationDatabase::new(app_handle) {
        Ok(db) => match db.get_connection() {
            Ok(conn) => {
                if let Err(e) = audit::insert_audit_log(
                    &conn,
                    audit_id,
                    &request.action_id,
                    &meta.domain,
                    meta.risk_level,
                    args_json.as_deref(),
                    request.reason.as_deref(),
                    request.dry_run,
                    approval_used,
                    success,
                    result_json.as_deref(),
                    error,
                    butler_conversation_id,
                    "butler",
                ) {
                    error!(error = %e, "Failed to write audit log");
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to get connection for audit log");
            }
        },
        Err(e) => {
            error!(error = %e, "Failed to open conversation DB for audit log");
        }
    }
}

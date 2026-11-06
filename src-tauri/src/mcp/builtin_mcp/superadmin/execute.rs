use tauri::AppHandle;
use tracing::{error, info, instrument};

use super::audit;
use super::registry::ActionRegistry;
use super::types::*;
use crate::db::conversation_db::ConversationDatabase;

/// 管家执行高风险动作的统一决策：风险等级 >= 3 且需要用户批准、又不是预览时拒绝执行。
/// SuperAdmin 与其他能执行同一动作的管家工具入口都必须复用这里，保证决策一致。
pub(crate) fn approval_gate_error(meta: &ActionMeta, dry_run: bool) -> Option<String> {
    if meta.risk_level.0 >= 3 && meta.requires_approval && !dry_run {
        // For Phase 1 we reject high-risk actions outright; Phase 3 will add
        // interactive approval via ask_user_question.
        return Some(format!(
            "Action '{}' has risk_level={} and requires user approval. \
             Use dry_run=true to preview, or request approval through ask_user_question first.",
            meta.action_id, meta.risk_level
        ));
    }
    None
}

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
    {
        if let Some(err_msg) = approval_gate_error(meta, request.dry_run) {
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
                None,
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

    // 3. Capture before-snapshot for write actions (risk > 0)
    let before_snapshot = if meta.risk_level.0 > 0 && !request.dry_run {
        registered.handler.snapshot_before(app_handle, &request.args).await
    } else {
        None
    };

    // 4. Execute via handler
    let handler_result =
        registered.handler.execute(app_handle, request.args.clone(), request.dry_run).await;

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
                before_snapshot.as_ref(),
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
                None, // No snapshot for failed actions
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
    before_snapshot: Option<&serde_json::Value>,
) {
    let args_json = serde_json::to_string(&request.args).ok();
    let result_json = result.and_then(|v| serde_json::to_string(v).ok());
    let snapshot_json = before_snapshot.and_then(|v| serde_json::to_string(v).ok());

    // Best-effort audit logging – do not fail the action if audit DB write fails
    match ConversationDatabase::new(app_handle) {
        Ok(db) => {
            if let Err(e) = db.with_write_connection(|conn| {
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
                    snapshot_json.as_deref(),
                ) {
                    error!(error = %e, "Failed to write audit log");
                }
                Ok(())
            }) {
                error!(error = %e, "Failed to get write connection for audit log");
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to open conversation DB for audit log");
        }
    }
}

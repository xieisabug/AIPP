use tauri::AppHandle;
use tracing::{error, info, instrument};

use super::audit;
use super::registry::ActionRegistry;
use super::types::*;
use crate::db::conversation_db::ConversationDatabase;

/// 管家动作批准边界：显式批准策略不允许被其他入口绕过。
/// SuperAdmin 与其他能执行同一动作的管家工具入口都必须复用这里，保证决策一致。
pub(crate) fn approval_gate_error(meta: &ActionMeta, dry_run: bool) -> Option<String> {
    if (meta.requires_approval || matches!(meta.approval_policy, ApprovalPolicy::UserApprovalRequired | ApprovalPolicy::Deny)) && !dry_run {

        return Some(format!(
            "Action '{}' has risk_level={} and requires user approval. \
             Use superadmin_execute to show the exact one-time approval dialog, or dry_run=true to preview.",
            meta.action_id, meta.risk_level
        ));
    }
    None
}

/// Legacy schedule.create does not carry trusted ownership. Never let a Butler
/// call silently become an unrestricted ordinary schedule (including batch calls).
fn schedule_origin_error(action: &str, dry_run: bool, kind: Option<&str>) -> Option<String> {
    if action == "schedule.create" && !dry_run
        && matches!(kind, Some("butler_main" | "butler_contact" | "butler_task")) {
        Some("管家不能通过旧版 schedule.create 创建无归属的日程。请使用 schedule_task 的 create 操作并绑定当前总管家主会话；创建后须由用户确认来源、模式和期限才会启用。".into())
    } else { None }
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

    let origin_check = (|| -> Result<(), String> {
        if request.action_id != "schedule.create" || request.dry_run { return Ok(()); }
        if let Some(id) = butler_conversation_id {
            use crate::db::conversation_db::Repository;
            let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
            let origin = db.conversation_repo().map_err(|e| e.to_string())?.read(id)
                .map_err(|e| e.to_string())?.ok_or("调用会话不存在，不能创建日程")?;
            if let Some(error) = schedule_origin_error(&request.action_id, request.dry_run, Some(&origin.conversation_kind)) {
                return Err(error);
            }
        }
        Ok(())
    })();
    if let Err(error) = origin_check {
        log_audit(app_handle, &audit_id, meta, &request, false, None, Some(&error), false, butler_conversation_id, None);
        return ExecuteResult { success: false, action_id: request.action_id.clone(), risk_level: meta.risk_level,
            approval_used: false, result: serde_json::Value::Null, audit_id, error: Some(error) };
    }

    // 3. Capture before-snapshot for write actions (risk > 0)
    let before_snapshot = if meta.risk_level.0 > 0 && !request.dry_run {
        registered.handler.snapshot_before(app_handle, &request.args).await
    } else {
        None
    };

    // Bind approval to an immutable action and current work, never a model assertion.
    let approved = if matches!(meta.approval_policy, ApprovalPolicy::Deny) {
        Err("该管理操作禁止执行，不能通过批准绕过".to_string())
    } else if approval_gate_error(meta, request.dry_run).is_some() {
        super::approval::approve(app_handle, meta, &request, butler_conversation_id, before_snapshot.as_ref()).await.map(Some)
    } else { Ok(None) };
    let approved = match approved {
        Ok(binding) => binding,
        Err(error) => {
            log_audit(app_handle,&audit_id,meta,&request,false,None,Some(&error),false,butler_conversation_id,None);
            return ExecuteResult {success:false, action_id:request.action_id.clone(),risk_level:meta.risk_level,
                approval_used:false,result:serde_json::Value::Null,audit_id,error:Some(error)};
        }
    };
    let approval_used = approved.is_some();

    // Snapshot may await; recheck immediately before crossing the action boundary.
    if let Some(binding) = approved.as_ref() {
        if let Err(error) = super::approval::validate(app_handle,binding,&request) {
            log_audit(app_handle,&audit_id,meta,&request,false,None,Some(&error),false,butler_conversation_id,None);
            return ExecuteResult {success:false, action_id:request.action_id.clone(),risk_level:meta.risk_level,
                approval_used:false,result:serde_json::Value::Null,audit_id,error:Some(error)};
        }
    }
    // 4. Execute via handler
    let handler_result =
        if approval_used {
            registered.handler.execute_approved(app_handle, request.args.clone(), before_snapshot.as_ref()).await
        } else {
            registered.handler.execute(app_handle, request.args.clone(), request.dry_run).await
        };

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
                approval_used,
                butler_conversation_id,
                before_snapshot.as_ref(),
            );

            ExecuteResult {
                success: true,
                action_id: request.action_id.clone(),
                risk_level: meta.risk_level,
                approval_used,
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
                approval_used,
                butler_conversation_id,
                None, // No snapshot for failed actions
            );

            ExecuteResult {
                success: false,
                action_id: request.action_id.clone(),
                risk_level: meta.risk_level,
                approval_used,
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

#[cfg(test)]
#[path = "execute_origin_tests.rs"]
mod schedule_origin_tests;

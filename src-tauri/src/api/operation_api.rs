use tauri::{AppHandle, Manager};
use tracing::{info, instrument, warn};

use crate::mcp::builtin_mcp::operation::types::PermissionDecision;
use crate::mcp::builtin_mcp::OperationState;

/// 确认操作权限
#[tauri::command]
#[instrument(skip(app_handle))]
pub async fn confirm_operation_permission(
    app_handle: AppHandle,
    request_id: String,
    decision: String,
) -> Result<bool, String> {
    info!(request_id = %request_id, decision = %decision, "Processing permission confirmation");

    let decision = match decision.as_str() {
        "allow" => PermissionDecision::Allow,
        "allow_and_save" => PermissionDecision::AllowAndSave,
        "deny" => PermissionDecision::Deny,
        _ => {
            warn!(decision = %decision, "Invalid permission decision");
            return Err(format!("Invalid decision: {}", decision));
        }
    };

    // 获取 OperationState
    let state = app_handle
        .try_state::<OperationState>()
        .ok_or_else(|| "OperationState not found".to_string())?;

    // 解决权限请求
    let resolved = state.resolve_permission_request(&request_id, decision).await;

    if resolved {
        info!(request_id = %request_id, "Permission request resolved successfully");
        Ok(true)
    } else {
        warn!(request_id = %request_id, "Permission request not found or already resolved");
        Err("Permission request not found or already resolved".to_string())
    }
}

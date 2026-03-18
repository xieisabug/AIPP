use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, instrument, warn};

use crate::api::ai::acp::{AcpPermissionDecision, AcpPermissionState};
use crate::api::butler_api::emit_butler_task_permission_state_changed;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::mcp::builtin_mcp::operation::types::PermissionDecision;
use crate::mcp::builtin_mcp::OperationState;

pub(crate) const OPERATION_PERMISSION_REQUEST_EVENT: &str = "operation-permission-request";
pub(crate) const OPERATION_PERMISSION_RESOLVED_EVENT: &str = "operation-permission-resolved";
pub(crate) const ACP_PERMISSION_REQUEST_EVENT: &str = "acp-permission-request";
pub(crate) const ACP_PERMISSION_RESOLVED_EVENT: &str = "acp-permission-resolved";

const BUTLER_WINDOW_LABEL: &str = "butler_experiment";
const BUTLER_MAIN_KIND: &str = "butler_main";
const BUTLER_TASK_KIND: &str = "butler_task";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PermissionResolvedEvent {
    pub request_id: String,
    pub conversation_id: Option<i64>,
}

fn can_route_to_visible_butler_window(app_handle: &AppHandle, conversation_id: Option<i64>) -> bool {
    let Some(conversation_id) = conversation_id else {
        return false;
    };
    let Some(window) = app_handle.get_webview_window(BUTLER_WINDOW_LABEL) else {
        return false;
    };
    #[cfg(desktop)]
    if !window.is_visible().unwrap_or(false) {
        return false;
    }

    let db = match ConversationDatabase::new(app_handle) {
        Ok(db) => db,
        Err(error) => {
            warn!(conversation_id, error = %error, "Failed to open conversation database for permission routing");
            return false;
        }
    };
    let repo = match db.conversation_repo() {
        Ok(repo) => repo,
        Err(error) => {
            warn!(conversation_id, error = %error, "Failed to open conversation repository for permission routing");
            return false;
        }
    };
    let Some(conversation) = (match repo.read(conversation_id) {
        Ok(conversation) => conversation,
        Err(error) => {
            warn!(conversation_id, error = %error, "Failed to read conversation for permission routing");
            return false;
        }
    }) else {
        return false;
    };

    matches!(
        conversation.conversation_kind.as_str(),
        BUTLER_MAIN_KIND | BUTLER_TASK_KIND
    )
}

pub(crate) fn emit_permission_request_event<T: Serialize>(
    app_handle: &AppHandle,
    event_name: &str,
    conversation_id: Option<i64>,
    payload: &T,
) -> Result<(), String> {
    if can_route_to_visible_butler_window(app_handle, conversation_id) {
        if let Err(error) = app_handle.emit_to(BUTLER_WINDOW_LABEL, event_name, payload) {
            warn!(
                event_name,
                conversation_id,
                error = %error,
                "Failed to emit permission request to Butler window, falling back to broadcast"
            );
        } else {
            return Ok(());
        }
    }

    app_handle.emit(event_name, payload).map_err(|error| error.to_string())
}

pub(crate) fn emit_permission_resolved_event(
    app_handle: &AppHandle,
    event_name: &str,
    payload: &PermissionResolvedEvent,
) -> Result<(), String> {
    app_handle.emit(event_name, payload).map_err(|error| error.to_string())
}

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
        "allow_for_conversation" => PermissionDecision::AllowForConversation,
        "allow_for_assistant" => PermissionDecision::AllowForAssistant,
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

    if let Some(resolution) = resolved {
        if let Err(error) = emit_permission_resolved_event(
            &app_handle,
            OPERATION_PERMISSION_RESOLVED_EVENT,
            &PermissionResolvedEvent {
                request_id: request_id.clone(),
                conversation_id: resolution.conversation_id,
            },
        ) {
            warn!(request_id = %request_id, error = %error, "Failed to emit operation permission resolution event");
        }
        if let Some(conversation_id) = resolution.conversation_id {
            emit_butler_task_permission_state_changed(
                &app_handle,
                conversation_id,
                "operation",
                false,
            )
            .await?;
        }
        if resolution.delivered {
            info!(request_id = %request_id, "Permission request resolved successfully");
            Ok(true)
        } else {
            warn!(request_id = %request_id, "Permission request receiver dropped before resolution");
            Err("Permission request receiver dropped before resolution".to_string())
        }
    } else {
        warn!(request_id = %request_id, "Permission request not found or already resolved");
        Err("Permission request not found or already resolved".to_string())
    }
}

/// 确认 ACP 工具调用权限
#[tauri::command]
#[instrument(skip(app_handle))]
pub async fn confirm_acp_permission(
    app_handle: AppHandle,
    request_id: String,
    option_id: Option<String>,
    cancelled: Option<bool>,
) -> Result<bool, String> {
    info!(request_id = %request_id, option_id = ?option_id, cancelled = ?cancelled, "Processing ACP permission confirmation");

    let state = app_handle
        .try_state::<AcpPermissionState>()
        .ok_or_else(|| "AcpPermissionState not found".to_string())?;

    let decision = if cancelled.unwrap_or(false) {
        AcpPermissionDecision::Cancelled
    } else if let Some(option_id) = option_id {
        AcpPermissionDecision::Selected(option_id)
    } else {
        return Err("Invalid ACP permission decision".to_string());
    };

    let resolved = state.resolve_request(&request_id, decision).await;

    if let Some(resolution) = resolved {
        if let Err(error) = emit_permission_resolved_event(
            &app_handle,
            ACP_PERMISSION_RESOLVED_EVENT,
            &PermissionResolvedEvent {
                request_id: request_id.clone(),
                conversation_id: resolution.conversation_id,
            },
        ) {
            warn!(request_id = %request_id, error = %error, "Failed to emit ACP permission resolution event");
        }
        if let Some(conversation_id) = resolution.conversation_id {
            emit_butler_task_permission_state_changed(&app_handle, conversation_id, "acp", false)
                .await?;
        }
        if resolution.delivered {
            info!(request_id = %request_id, "ACP permission request resolved successfully");
            Ok(true)
        } else {
            warn!(request_id = %request_id, "ACP permission receiver dropped before resolution");
            Err("ACP permission receiver dropped before resolution".to_string())
        }
    } else {
        warn!(request_id = %request_id, "ACP permission request not found or already resolved");
        Err("ACP permission request not found or already resolved".to_string())
    }
}

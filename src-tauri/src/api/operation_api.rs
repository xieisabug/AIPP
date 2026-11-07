use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, instrument, warn};

use crate::api::ai::acp::{AcpElicitationState, AcpPermissionState};
use crate::api::ai::agent_driver::approval::{
    resolve_app_reply, AgentElicitationReply, AgentInteractionKind, AgentInteractionReply,
    AgentPermissionReply, AgentReplyTarget,
};
use crate::api::butler_api::emit_butler_task_permission_state_changed;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::mcp::builtin_mcp::operation::types::PermissionDecision;
use crate::mcp::builtin_mcp::OperationState;

pub(crate) const OPERATION_PERMISSION_REQUEST_EVENT: &str = "operation-permission-request";
pub(crate) const OPERATION_PERMISSION_RESOLVED_EVENT: &str = "operation-permission-resolved";
pub(crate) const ACP_PERMISSION_REQUEST_EVENT: &str = "acp-permission-request";
pub(crate) const ACP_PERMISSION_RESOLVED_EVENT: &str = "acp-permission-resolved";
pub(crate) const ACP_ELICITATION_REQUEST_EVENT: &str = "acp-elicitation-request";
pub(crate) const ACP_ELICITATION_RESOLVED_EVENT: &str = "acp-elicitation-resolved";

const BUTLER_WINDOW_LABEL: &str = "butler_experiment";
const BUTLER_MAIN_KIND: &str = "butler_main";
const BUTLER_TASK_KIND: &str = "butler_task";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PermissionResolvedEvent {
    pub request_id: String,
    pub conversation_id: Option<i64>,
}

fn can_route_to_visible_butler_window(
    app_handle: &AppHandle,
    conversation_id: Option<i64>,
) -> bool {
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

    matches!(conversation.conversation_kind.as_str(), BUTLER_MAIN_KIND | BUTLER_TASK_KIND)
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

/// The public typed entry point and legacy aliases use one resolution path.
/// Legacy aliases derive ownership from the saved request, not the active driver.
#[tauri::command]
pub async fn confirm_agent_interaction(
    app_handle: AppHandle,
    target: AgentReplyTarget,
    reply: AgentInteractionReply,
) -> Result<bool, String> {
    resolve_agent_interaction_response(&app_handle, target, reply).await
}

pub(crate) async fn resolve_agent_interaction_response(
    app_handle: &AppHandle,
    target: AgentReplyTarget,
    reply: AgentInteractionReply,
) -> Result<bool, String> {
    let resolution = resolve_app_reply(app_handle, target, reply).await?;
    let event_name = match resolution.kind {
        AgentInteractionKind::Permission => Some(ACP_PERMISSION_RESOLVED_EVENT),
        AgentInteractionKind::Elicitation => Some(ACP_ELICITATION_RESOLVED_EVENT),
        AgentInteractionKind::AskUserQuestion => None,
    };
    if let Some(event_name) = event_name {
        if let Err(error) = emit_permission_resolved_event(app_handle, event_name, &PermissionResolvedEvent {
            request_id: resolution.request_id.clone(),
            conversation_id: resolution.scope.conversation_id,
        }) {
            warn!(request_id = %resolution.request_id, error = %error, "Failed to emit interaction resolution event");
        }
    }
    if resolution.kind == AgentInteractionKind::Permission {
        if let Some(conversation_id) = resolution.scope.conversation_id {
            // The decision has already been consumed and delivered. A secondary
            // notification failure must not tell callers to retry that decision.
            if let Err(error) = emit_butler_task_permission_state_changed(
                app_handle, conversation_id, "acp", false,
            ).await {
                warn!(conversation_id, error = %error, "Failed to refresh resolved Agent permission state");
            }
        }
    }
    if resolution.delivered {
        Ok(true)
    } else {
        Err(match resolution.kind {
            AgentInteractionKind::Permission => "ACP permission receiver dropped before resolution",
            AgentInteractionKind::Elicitation => "ACP elicitation receiver dropped before resolution",
            AgentInteractionKind::AskUserQuestion => "AskUserQuestion request not found or already resolved: receiver dropped before resolution",
        }.into())
    }
}

/// Compatibility route used by ACP, Claude, MCP and Feishu callers.
#[tauri::command]
#[instrument(skip(app_handle))]
pub async fn confirm_acp_permission(
    app_handle: AppHandle,
    request_id: String,
    option_id: Option<String>,
    cancelled: Option<bool>,
) -> Result<bool, String> {
    let state = app_handle.try_state::<AcpPermissionState>()
        .ok_or("AcpPermissionState not found")?;
    let decision = AgentPermissionReply::from_legacy(option_id, cancelled.unwrap_or(false))?;
    let scope = state.request_scope(&request_id).await
        .ok_or("ACP permission request not found or already resolved")?;
    resolve_agent_interaction_response(&app_handle,
        AgentReplyTarget { request_id, scope }, AgentInteractionReply::Permission(decision)).await
}

/// ACP form elicitation retains the existing action/value command contract.
#[tauri::command]
#[instrument(skip(app_handle, values))]
pub async fn confirm_acp_elicitation(
    app_handle: AppHandle,
    request_id: String,
    action: String,
    values: Option<std::collections::HashMap<String, serde_json::Value>>,
) -> Result<bool, String> {
    let state = app_handle.try_state::<AcpElicitationState>()
        .ok_or("AcpElicitationState not found")?;
    let decision = match action.as_str() {
        "accept" => AgentElicitationReply::Accept { values: values.unwrap_or_default() },
        "decline" => AgentElicitationReply::Decline,
        "cancel" => AgentElicitationReply::Cancel,
        other => return Err(format!("Unknown ACP elicitation action: {other}")),
    };
    let scope = state.request_scope(&request_id).await
        .ok_or("ACP elicitation request not found or already resolved")?;
    resolve_agent_interaction_response(&app_handle,
        AgentReplyTarget { request_id, scope }, AgentInteractionReply::Elicitation(decision)).await
}

/// Retain the frontend Codex alias without a second decision implementation.
#[tauri::command]
#[instrument(skip(app_handle))]
pub async fn confirm_codex_permission(
    app_handle: AppHandle,
    request_id: String,
    option_id: Option<String>,
    cancelled: Option<bool>,
) -> Result<bool, String> {
    confirm_acp_permission(app_handle, request_id, option_id, cancelled).await
}

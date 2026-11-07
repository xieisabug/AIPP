//! Shared reply boundary over the existing authoritative pending-request stores.
//! A request keeps its creation-time owner. Replies never look up the currently
//! active conversation binding, which may already belong to a replacement run.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::api::ai::acp::{
    AcpElicitationDecision, AcpElicitationState, AcpPermissionDecision, AcpPermissionState,
};
use crate::mcp::builtin_mcp::interaction::InteractionState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequestScope {
    pub driver_id: Option<String>,
    pub instance_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub run_id: Option<String>,
}

impl AgentRequestScope {
    pub fn application(conversation_id: Option<i64>) -> Self {
        Self { driver_id: None, instance_id: None, conversation_id, run_id: None }
    }

    pub fn native(
        driver_id: impl Into<String>,
        instance_id: Option<i64>,
        conversation_id: i64,
        run_id: impl Into<String>,
    ) -> Self {
        Self {
            driver_id: Some(driver_id.into()), instance_id,
            conversation_id: Some(conversation_id), run_id: Some(run_id.into()),
        }
    }

    pub(crate) fn validate(&self, actual: &Self, request_id: &str) -> Result<(), String> {
        // None is an actual application/probe identity, never a wildcard.
        let mismatch = if self.driver_id != actual.driver_id { Some("driver") }
            else if self.instance_id != actual.instance_id { Some("instance") }
            else if self.conversation_id != actual.conversation_id { Some("conversation") }
            else if self.run_id != actual.run_id { Some("run") }
            else { None };
        match mismatch {
            Some(field) => Err(format!("Interaction request '{request_id}' {field} scope mismatch")),
            None => Ok(()),
        }
    }

    pub(crate) fn validate_conversation(&self, conversation_id: Option<i64>) -> Result<(), String> {
        if self.conversation_id != conversation_id {
            return Err("Interaction request conversation does not match its scope".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReplyTarget {
    pub request_id: String,
    pub scope: AgentRequestScope,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentInteractionKind { Permission, Elicitation, AskUserQuestion }

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentPermissionReply { Select { option_id: String }, Cancel }

impl AgentPermissionReply {
    pub fn from_legacy(option_id: Option<String>, cancelled: bool) -> Result<Self, String> {
        if cancelled { Ok(Self::Cancel) }
        else { option_id.map(|option_id| Self::Select { option_id })
            .ok_or_else(|| "Invalid permission decision: select an advertised option or cancel".into()) }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentElicitationReply {
    Accept { values: HashMap<String, serde_json::Value> },
    Decline,
    Cancel,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentQuestionReply { Answer { answers: HashMap<String, String> }, Cancel }

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "decision", rename_all = "snake_case")]
pub enum AgentInteractionReply {
    Permission(AgentPermissionReply),
    Elicitation(AgentElicitationReply),
    AskUserQuestion(AgentQuestionReply),
}

#[derive(Debug, Serialize)]
pub struct AgentInteractionResolution {
    pub request_id: String,
    pub scope: AgentRequestScope,
    pub kind: AgentInteractionKind,
    pub delivered: bool,
}

/// Borrow the real stores; this facade never owns or copies pending requests.
pub(crate) struct AgentInteractionStores<'a> {
    pub permissions: &'a AcpPermissionState,
    pub elicitations: &'a AcpElicitationState,
    pub questions: &'a InteractionState,
}

pub(crate) async fn resolve_reply(
    stores: AgentInteractionStores<'_>,
    target: AgentReplyTarget,
    reply: AgentInteractionReply,
) -> Result<AgentInteractionResolution, String> {
    let (kind, resolution) = match reply {
        AgentInteractionReply::Permission(decision) => {
            let decision = match decision {
                AgentPermissionReply::Select { option_id } => AcpPermissionDecision::Selected(option_id),
                AgentPermissionReply::Cancel => AcpPermissionDecision::Cancelled,
            };
            (AgentInteractionKind::Permission,
                stores.permissions.resolve_request_scoped(&target.request_id, &target.scope, decision).await?)
        }
        AgentInteractionReply::Elicitation(decision) => {
            let decision = match decision {
                AgentElicitationReply::Accept { values } => {
                    let mut content = std::collections::BTreeMap::new();
                    for (key, value) in values {
                        let value = crate::api::ai::acp::json_to_acp_elicitation_value(value)
                            .map_err(|error| format!("elicitation value '{key}' invalid: {error}"))?;
                        content.insert(key, value);
                    }
                    AcpElicitationDecision::Accepted(content)
                }
                AgentElicitationReply::Decline => AcpElicitationDecision::Declined,
                AgentElicitationReply::Cancel => AcpElicitationDecision::Cancelled,
            };
            (AgentInteractionKind::Elicitation,
                stores.elicitations.resolve_request_scoped(&target.request_id, &target.scope, decision).await?)
        }
        AgentInteractionReply::AskUserQuestion(decision) => {
            let decision = match decision {
                AgentQuestionReply::Answer { answers } => {
                    if answers.is_empty() { return Err("Answers cannot be empty".into()); }
                    Ok(answers)
                }
                AgentQuestionReply::Cancel => Err("User cancelled AskUserQuestion".into()),
            };
            (AgentInteractionKind::AskUserQuestion,
                stores.questions.resolve_ask_user_request_scoped(&target.request_id, &target.scope, decision).await?)
        }
    };
    let resolution = resolution.ok_or_else(|| match kind {
        AgentInteractionKind::Permission => "ACP permission request not found or already resolved",
        AgentInteractionKind::Elicitation => "ACP elicitation request not found or already resolved",
        AgentInteractionKind::AskUserQuestion => "AskUserQuestion request not found or already resolved",
    }.to_string())?;
    Ok(AgentInteractionResolution {
        request_id: target.request_id, scope: target.scope, kind, delivered: resolution.delivered,
    })
}

pub(crate) async fn resolve_app_reply(
    app: &AppHandle,
    target: AgentReplyTarget,
    reply: AgentInteractionReply,
) -> Result<AgentInteractionResolution, String> {
    let permissions = app.try_state::<AcpPermissionState>().ok_or("AcpPermissionState not found")?;
    let elicitations = app.try_state::<AcpElicitationState>().ok_or("AcpElicitationState not found")?;
    let questions = app.try_state::<InteractionState>().ok_or("InteractionState not found")?;
    if matches!(&reply, AgentInteractionReply::Permission(AgentPermissionReply::Select { .. })) {
        permissions.validate_butler_reply(app, &target.request_id).await?;
    }
    resolve_reply(AgentInteractionStores {
        permissions: &permissions, elicitations: &elicitations, questions: &questions,
    }, target, reply).await
}

/// Runtime teardown clears only requests created by that exact native owner.
/// It is deliberately distinct from user-initiated application cancel-all.
pub(crate) async fn cancel_app_requests_for_scope(app: &AppHandle, scope: &AgentRequestScope) {
    use crate::api::operation_api::{
        emit_permission_resolved_event, PermissionResolvedEvent,
        ACP_ELICITATION_RESOLVED_EVENT, ACP_PERMISSION_RESOLVED_EVENT,
    };
    if let Some(permissions) = app.try_state::<AcpPermissionState>() {
        for (request_id, resolution) in permissions.cancel_requests_for_scope(scope).await {
            let _ = emit_permission_resolved_event(app, ACP_PERMISSION_RESOLVED_EVENT,
                &PermissionResolvedEvent { request_id, conversation_id: resolution.conversation_id });
            if let Some(conversation_id) = resolution.conversation_id {
                if let Err(error) = crate::api::butler_api::emit_butler_task_permission_state_changed(
                    app, conversation_id, "acp", false,
                ).await {
                    tracing::warn!(conversation_id, error = %error, "Failed to refresh cancelled Agent permission state");
                }
            }
        }
    }
    if let Some(elicitations) = app.try_state::<AcpElicitationState>() {
        for (request_id, resolution) in elicitations.cancel_requests_for_scope(scope).await {
            let _ = emit_permission_resolved_event(app, ACP_ELICITATION_RESOLVED_EVENT,
                &PermissionResolvedEvent { request_id, conversation_id: resolution.conversation_id });
        }
    }
    if let Some(questions) = app.try_state::<InteractionState>() {
        questions.cancel_requests_for_scope(scope).await;
    }
}

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;

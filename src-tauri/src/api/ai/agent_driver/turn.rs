//! The application turn path is selected by driver registration, never by a
//! central transport switch. Conversations and native session stores stay intact.
use super::dispatch::{dispatch_prepared, response_message, TurnEffects, TurnInput};
use super::lifecycle::{bind_session, AgentSessionBinding};
use super::registry::{registry, AgentDriverMetadata, AgentDriverRegistry, AgentInstanceConfig, PreparedAgentSession};
use super::session::AgentSessionHandle;
use crate::api::ai::acp::build_selected_mcp_tools_payload;
use crate::api::ai::events::ConversationEvent;
use crate::api::ai::types::AiResponse;
use crate::api::ai_api::parse_agent_model_override;
use crate::api::assistant_api::{AssistantDetail, resolve_acp_provider_id};
use crate::db::conversation_db::{ConversationDatabase, Message, MessageAttachment, Repository};
use crate::db::llm_db::{LLMDatabase, LLMProviderConfig};
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::state::activity_state::ConversationActivityManager;
use std::collections::HashMap;
use tauri::Manager;

pub(crate) struct AgentTurnRequest<'a> {
    pub assistant: &'a AssistantDetail,
    pub conversation_id: i64,
    pub prompt: String,
    pub display_prompt: String,
    pub override_model_id: Option<String>,
    pub overrides: &'a Option<HashMap<String, serde_json::Value>>,
    pub feature_configs: &'a HashMap<String, HashMap<String, FeatureConfig>>,
    pub messages: &'a [(String, String, Vec<MessageAttachment>)],
}

pub(super) fn load_agent_instance(
    registry: &AgentDriverRegistry,
    assistant: &AssistantDetail,
    override_model_id: Option<&str>,
    load: impl FnOnce(i64) -> Result<(String, Vec<LLMProviderConfig>), AppError>,
) -> Result<(AgentInstanceConfig, Option<String>), AppError> {
    let model_override = override_model_id.filter(|value| !value.trim().is_empty())
        .map(|value| parse_agent_model_override(value).ok_or_else(|| AppError::UnknownError(format!(
            "Agent 所选模型 ID「{value}」格式无效，预期为 model%%provider_id 且提供商 ID 必须大于 0；请重新选择模型，未回退到默认配置"
        )))).transpose()?;
    let provider_id = model_override.map(|(_, provider_id)| provider_id)
        .or_else(|| resolve_acp_provider_id(&assistant.model, &assistant.model_configs))
        .ok_or_else(|| AppError::UnknownError("Agent 助手尚未配置模型提供商".into()))?;
    let (api_type, configs) = load(provider_id)?;
    let instance = registry.resolve_provider(provider_id, &api_type, configs)?;
    Ok((instance, model_override.map(|(model, _)| model.to_string())))
}

/// initialize_conversation has already expanded text attachments into the last
/// runtime user message. Text-only transports must receive that exact prompt;
/// structured transports receive raw text plus attachments once, not twice.
pub(super) fn prompt_for_driver(metadata: AgentDriverMetadata, request: &AgentTurnRequest<'_>) -> Result<String, AppError> {
    if metadata.operations.structured_attachments { return Ok(request.prompt.clone()); }
    let Some((_, contextual_prompt, attachments)) = request.messages.iter().rev().find(|(kind, _, _)| kind == "user") else {
        return Ok(request.prompt.clone());
    };
    if let Some(unsupported) = attachments.iter().find(|attachment| !matches!(attachment.attachment_type, crate::db::conversation_db::AttachmentType::Text | crate::db::conversation_db::AttachmentType::Skill)) {
        return Err(AppError::ProviderError(format!("{} prompt preparation failed [conversation_id={}, attachment_id={}]: this driver accepts text context only; the {:?} attachment cannot be sent without losing user input", metadata.id, request.conversation_id, unsupported.id, unsupported.attachment_type)));
    }
    Ok(contextual_prompt.clone())
}

struct RegisteredPreparedSession {
    instance: AgentInstanceConfig,
    prepared: Box<dyn PreparedAgentSession>,
}

pub(crate) async fn send_turn(app_handle: tauri::AppHandle, window: tauri::Window, request: AgentTurnRequest<'_>) -> Result<AiResponse, AppError> {
    let mut metadata = AgentDriverMetadata {
        id: "unresolved", display_name: "Agent", message_model_name: "agent",
        operations: super::session::AgentSessionCapabilities { structured_attachments: false, cancel_prompt: false, set_config_option: false },
    };
    let mut provider_id = None;
    let mut preparation_stage = super::failure::AgentFailureStage::Configuration;
    let preparation: Result<(RegisteredPreparedSession, String), AppError> = async {
        if cfg!(mobile) {
            return Err(AppError::InternalError(crate::api::system_api::mobile_unsupported_error("本地 CLI Agent（Codex/Claude/ACP）")));
        }
        let (instance, model_override) = load_agent_instance(registry(), request.assistant, request.override_model_id.as_deref(), |provider_id| {
            let database = LLMDatabase::new(&app_handle)?;
            let provider = database.get_llm_provider(provider_id).map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::ProviderError(format!("Agent 助手 {} 引用的提供商 ID {provider_id} 不存在，请在助手配置中重新选择提供商", request.assistant.assistant.id)),
                error => AppError::from(error),
            })?;
            Ok((provider.api_type, database.get_llm_provider_config(provider_id)?))
        })?;
        metadata = registry().driver(&instance.driver_id)?.metadata();
        provider_id = Some(instance.provider_id);
        let mut prepared = registry().prepare(&instance, &request, model_override.as_deref())?;
        let prompt = prompt_for_driver(prepared.metadata(), &request)?;
        preparation_stage = super::failure::AgentFailureStage::Mcp;
        let payload = build_selected_mcp_tools_payload(&app_handle, request.assistant.assistant.id, Some(request.conversation_id)).map_err(AppError::UnknownError)?;
        prepared.finalize(&app_handle, request.conversation_id, payload).await?;
        Ok((RegisteredPreparedSession { instance, prepared }, prompt))
    }.await;
    let effects = DesktopTurnEffects {
        app_handle, window, provider_id, metadata,
        failure_stage: if preparation.is_ok() { super::failure::AgentFailureStage::SendPrompt } else { preparation_stage },
    };
    match preparation {
        Ok((prepared, prompt)) => dispatch_prepared(&effects, prepared, metadata, TurnInput {
            conversation_id: request.conversation_id, prompt,
            display_prompt: request.display_prompt, messages: request.messages,
        }).await,
        Err(error) => super::dispatch::reject_preparation(&effects, metadata, request.conversation_id, request.display_prompt, error).await,
    }
}

struct DesktopTurnEffects {
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    provider_id: Option<i64>,
    metadata: AgentDriverMetadata,
    failure_stage: super::failure::AgentFailureStage,
}

#[async_trait::async_trait]
impl TurnEffects for DesktopTurnEffects {
    type Prepared = RegisteredPreparedSession;
    type Handle = AgentSessionHandle;
    fn create_response(&self, conversation_id: i64, metadata: AgentDriverMetadata) -> Result<Message, AppError> {
        ConversationDatabase::new(&self.app_handle)?.message_repo()?.create(&response_message(conversation_id, metadata)).map_err(AppError::from)
    }
    fn emit(&self, conversation_id: i64, event: ConversationEvent) {
        let mut identity = super::events::AgentEventIdentity::new(self.metadata.id, conversation_id);
        if let Some(provider_id) = self.provider_id { identity = identity.with_instance(provider_id); }
        let _ = super::events::emit_to_window(&self.window, identity, event);
    }
    async fn acquire(&self, prepared: Self::Prepared, conversation_id: i64) -> Result<AgentSessionHandle, AppError> {
        let _gate = super::lifecycle::lock_conversation(&self.app_handle, conversation_id).await?;
        let handle = prepared.prepared.acquire(&self.app_handle, conversation_id).await?;
        if handle.metadata().id != prepared.instance.driver_id {
            return Err(super::registry::routing_error("acquired handle belongs to a different driver"));
        }
        bind_session(&self.app_handle, AgentSessionBinding {
            conversation_id, provider_id: prepared.instance.provider_id,
            driver_id: prepared.instance.driver_id, config_fingerprint: prepared.instance.config_fingerprint,
            run_id: handle.run_id().to_string(),
        })?;
        Ok(handle)
    }
    fn submit(&self, handle: &AgentSessionHandle, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>) -> Result<(), AppError> {
        handle.send_prompt(message_id, prompt, attachments, self.window.clone())
    }
    async fn fail(&self, conversation_id: i64, message_id: i64, metadata: AgentDriverMetadata, detail: &str) {
        let mut identity = super::events::AgentEventIdentity::new(metadata.id, conversation_id);
        if let Some(provider_id) = self.provider_id { identity = identity.with_instance(provider_id); }
        if let Err(error) = super::failure::emit_failure_to_window(&self.app_handle, identity, message_id, &self.window, metadata.display_name, self.failure_stage, detail) {
            tracing::error!(conversation_id, message_id, error, "Agent dispatch failure publication incomplete");
        }
        self.clear_focus(conversation_id).await;
    }
    async fn clear_focus(&self, conversation_id: i64) {
        self.app_handle.state::<ConversationActivityManager>().clear_focus(&self.app_handle, conversation_id).await;
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;

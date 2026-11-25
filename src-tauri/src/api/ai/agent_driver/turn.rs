//! The application turn path is selected by driver registration, never by a
//! central transport switch. Conversations and native session stores stay intact.
use crate::api::ai::agent_driver::capabilities::AttachmentInput;
use super::dispatch::{dispatch_prepared, response_message, TurnEffects, TurnInput};
use super::lifecycle::{bind_session, AgentSessionBinding};
use super::registry::{registry, AgentDriverMetadata, AgentDriverRegistry, AgentInstanceConfig, PreparedAgentSession};
use super::session::AgentSessionHandle;
use crate::api::ai::acp::build_selected_mcp_tools_with_catalog;
use crate::api::ai::events::ConversationEvent;
use crate::api::ai::types::AiResponse;
use crate::api::ai_api::parse_agent_model_override;
use crate::api::assistant_api::{AssistantDetail, resolve_acp_provider_id};
use crate::db::conversation_db::{AttachmentType, ConversationDatabase, Message, MessageAttachment, Repository};
use crate::db::llm_db::{LLMDatabase, LLMProviderConfig, ModelRequestError};
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::state::activity_state::ConversationActivityManager;
use std::collections::HashMap;
use tauri::Manager;

pub(crate) struct AgentTurnRequest<'a> {
    pub assistant: &'a AssistantDetail,
    pub conversation_id: i64,
    pub native_butler_role: Option<String>,
    pub source_message_id: Option<i64>,
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

fn read_turn_provider(
    db: &LLMDatabase, id: i64, assistant_id: i64,
) -> Result<(String, Vec<LLMProviderConfig>), AppError> {
    let provider = db.get_provider_for_request(id).map_err(|error| match error {
        ModelRequestError::Lookup(rusqlite::Error::QueryReturnedNoRows) => AppError::ProviderError(format!(
            "Agent 助手 {assistant_id} 引用的提供商 ID {id} 不存在，请在助手配置中重新选择提供商"
        )),
        error => AppError::ProviderError(error.to_string()),
    })?;
    Ok((provider.api_type, db.get_llm_provider_config(id)?))
}

/// initialize_conversation has already expanded text attachments into the last
/// runtime user message. Text-only transports must receive that exact prompt;
/// structured transports receive raw text plus attachments once, not twice.
pub(super) fn prompt_for_driver(metadata: AgentDriverMetadata, request: &AgentTurnRequest<'_>) -> Result<String, AppError> {
    let Some((_, contextual_prompt, attachments)) = request.messages.iter().rev().find(|(kind, _, _)| kind == "user") else {
        return Ok(request.prompt.clone());
    };
    let prompt = if metadata.operations.attachment_input == AttachmentInput::Structured { &request.prompt } else { contextual_prompt };
    prompt_with_file_paths(metadata, request.conversation_id, prompt, attachments)
}

/// Keep path resolution at dispatch so regenerating a turn checks the execution machine again.
pub(crate) fn prompt_with_file_paths(metadata: AgentDriverMetadata, conversation_id: i64, prompt: &str, attachments: &[MessageAttachment]) -> Result<String, AppError> {
    let mut result = prompt.to_owned();
    for file in attachments {
        let kind = file.attachment_type;
        if matches!(kind, AttachmentType::Skill | AttachmentType::DeepSeekReplay) { continue; }
        let path = std::path::Path::new(file.attachment_url.as_deref().unwrap_or(""));
        let native_content = match kind {
            AttachmentType::Image => metadata.operations.attachment_input != AttachmentInput::PromptText
                && [file.attachment_content.as_deref(), file.attachment_url.as_deref()].into_iter().flatten().any(|data| data.starts_with("data:image/")),
            AttachmentType::Text => file.attachment_content.is_some(),
            _ => false,
        };
        if !path.is_absolute() {
            if native_content { continue; }
            return Err(AppError::ProviderError(format!("{} 准备 {:?} 附件失败 [conversation_id={conversation_id}, attachment_id={}]：{} 在执行机上没有可读文件，请重新选择", metadata.id, kind, file.id, path.display())));
        }
        std::fs::metadata(path).and_then(|m| if m.is_file() { Ok(()) } else {
            Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "附件不是文件"))
        }).map_err(|e| AppError::ProviderError(format!("{} 准备附件失败 [conversation_id={conversation_id}, attachment_id={}]：{}（{e}），请重新选择文件", metadata.id, file.id, path.display())))?;
        if !result.is_empty() { result.push('\n'); }
        result.push_str("附件路径 ");
        let name = path.to_string_lossy();
        if name.chars().any(char::is_control) {
            result.push_str(&serde_json::json!(name).to_string());
        } else {
            result.push_str(&name);
        }
    }
    if attachments.iter().any(|file| matches!(file.attachment_type, AttachmentType::Word | AttachmentType::Excel | AttachmentType::PowerPoint)) {
        result.push_str("\nOffice 原文件供读取。编辑时使用查看器选区的会话引用，经 office_edit 生成新副本。");
    }
    Ok(result)
}

struct RegisteredPreparedSession {
    instance: AgentInstanceConfig,
    prepared: Box<dyn PreparedAgentSession>,
}

pub(crate) async fn send_turn(app_handle: tauri::AppHandle, window: tauri::Window, mut request: AgentTurnRequest<'_>) -> Result<AiResponse, AppError> {
    let mut metadata = AgentDriverMetadata {
        id: "unresolved", display_name: "Agent", message_model_name: "agent",
        operations: super::session::AgentSessionCapabilities { attachment_input: AttachmentInput::PromptText, cancel_prompt: false, set_config_option: false },
    };
    let mut provider_id = None;
    let mut preparation_stage = super::failure::AgentFailureStage::Configuration;
    let preparation: Result<(RegisteredPreparedSession, String), AppError> = async {
        if cfg!(mobile) {
            return Err(AppError::InternalError(crate::api::system_api::mobile_unsupported_error("本地 CLI Agent（Codex/Claude/ACP）")));
        }
        let (instance, model_override) = load_agent_instance(
            registry(), request.assistant, request.override_model_id.as_deref(),
            |id| read_turn_provider(&LLMDatabase::new(&app_handle)?, id, request.assistant.assistant.id),
        )?;
        metadata = registry().driver(&instance.driver_id)?.metadata();
        provider_id = Some(instance.provider_id);
        preparation_stage = super::failure::AgentFailureStage::Mcp;
        let (payload, dynamic_catalog) = build_selected_mcp_tools_with_catalog(&app_handle, request.assistant.assistant.id, Some(request.conversation_id)).map_err(AppError::UnknownError)?;
        if let Some(role) = request.native_butler_role.take() {
            request.native_butler_role = Some(
                crate::acp_mcp_bridge::native_butler_instructions_with_catalog(&role, &payload, &dynamic_catalog)
                    .map_err(AppError::UnknownError)?,
            );
        }
        preparation_stage = super::failure::AgentFailureStage::Configuration;
        let mut prepared = registry().prepare(&instance, &request, model_override.as_deref())?;
        let prompt = prompt_for_driver(prepared.metadata(), &request)?;
        // Native slash commands must arrive verbatim even when the stored message
        // carries AIPP's hidden turn context. Preserve attached turns as ordinary input.
        let prompt = if request.display_prompt.trim() == super::manual_compact::MANUAL_COMPACT_COMMAND
            && super::manual_compact::is_manual_compact_prompt(&prompt)
            && request.messages.iter().rev().find(|(role, _, _)| role == "user")
                .is_some_and(|(_, _, attachments)| attachments.is_empty())
        {
            super::manual_compact::MANUAL_COMPACT_COMMAND.to_owned()
        } else { prompt };
        preparation_stage = super::failure::AgentFailureStage::Mcp;
        prepared.finalize(&app_handle, request.conversation_id, payload).await?;
        Ok((RegisteredPreparedSession { instance, prepared }, prompt))
    }.await;
    let effects = DesktopTurnEffects {
        app_handle, window, provider_id, metadata, source_message_id: request.source_message_id,
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
    source_message_id: Option<i64>,
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    provider_id: Option<i64>,
    metadata: AgentDriverMetadata,
    failure_stage: super::failure::AgentFailureStage,
}

#[async_trait::async_trait]
impl TurnEffects for DesktopTurnEffects {
    type Prepared = RegisteredPreparedSession;
    // Serialize permission selection with both acquisition and prompt submission.
    type Handle = (AgentSessionHandle, tokio::sync::OwnedMutexGuard<()>);
    fn create_response(&self, conversation_id: i64, metadata: AgentDriverMetadata) -> Result<Message, AppError> {
        let mut message=response_message(conversation_id, metadata);
        message.parent_id=self.source_message_id;
        ConversationDatabase::new(&self.app_handle)?.message_repo()?.create(&message).map_err(AppError::from)
    }
    fn emit(&self, conversation_id: i64, event: ConversationEvent) {
        let mut identity = super::events::AgentEventIdentity::new(self.metadata.id, conversation_id);
        if let Some(provider_id) = self.provider_id { identity = identity.with_instance(provider_id); }
        let _ = super::events::emit_to_window(&self.window, identity, event);
    }
    async fn acquire(&self, prepared: Self::Prepared, conversation_id: i64) -> Result<Self::Handle, AppError> {
        let gate = super::lifecycle::lock_conversation(&self.app_handle, conversation_id).await?;
        let handle = prepared.prepared.acquire(&self.app_handle, conversation_id).await?;
        if handle.metadata().id != prepared.instance.driver_id {
            return Err(super::registry::routing_error("acquired handle belongs to a different driver"));
        }
        bind_session(&self.app_handle, AgentSessionBinding {
            conversation_id, provider_id: prepared.instance.provider_id,
            driver_id: prepared.instance.driver_id, config_fingerprint: prepared.instance.config_fingerprint,
            run_id: handle.run_id().to_string(),
        })?;
        Ok((handle, gate))
    }
    fn submit(&self, handle: &Self::Handle, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>) -> Result<(), AppError> {
        handle.0.send_prompt(message_id, prompt, attachments, self.window.clone())
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

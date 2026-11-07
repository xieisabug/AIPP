//! The shared turn sequence, separated from desktop I/O so its ordering and
//! failure contract can be tested without a display, process or real database.
use super::registry::AgentDriverMetadata;
use crate::api::ai::events::{ConversationEvent, MessageAddEvent};
use crate::api::ai::types::AiResponse;
use crate::db::conversation_db::{Message, MessageAttachment};
use crate::errors::AppError;

pub(super) struct TurnInput<'a> {
    pub conversation_id: i64,
    pub prompt: String,
    pub display_prompt: String,
    pub messages: &'a [(String, String, Vec<MessageAttachment>)],
}

// Only I/O is substituted in tests; dispatch_prepared is the production path.
#[async_trait::async_trait]
pub(super) trait TurnEffects: Sync {
    type Prepared: Send;
    type Handle: Send + Sync;
    fn create_response(&self, conversation_id: i64, metadata: AgentDriverMetadata) -> Result<Message, AppError>;
    fn emit(&self, conversation_id: i64, event: ConversationEvent);
    async fn acquire(&self, prepared: Self::Prepared, conversation_id: i64) -> Result<Self::Handle, AppError>;
    fn submit(&self, handle: &Self::Handle, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>) -> Result<(), AppError>;
    async fn fail(&self, conversation_id: i64, message_id: i64, metadata: AgentDriverMetadata, detail: &str);
    async fn clear_focus(&self, _conversation_id: i64) {}
}

pub(super) fn response_message(conversation_id: i64, metadata: impl Into<AgentDriverMetadata>) -> Message {
    let metadata = metadata.into();
    Message {
        id: 0, parent_id: None, conversation_id, message_type: "response".into(),
        content: String::new(), llm_model_id: Some(0),
        llm_model_name: Some(metadata.message_model_name.into()),
        start_time: Some(chrono::Utc::now()), finish_time: None,
        created_time: chrono::Utc::now(), token_count: 0, input_token_count: 0,
        output_token_count: 0, generation_group_id: None, parent_group_id: None,
        tool_calls_json: None, metadata_json: None, first_token_time: None, ttft_ms: None,
    }
}

pub(super) async fn dispatch_prepared<E: TurnEffects>(
    effects: &E,
    prepared: E::Prepared,
    metadata: impl Into<AgentDriverMetadata>,
    input: TurnInput<'_>,
) -> Result<AiResponse, AppError> {
    let metadata = metadata.into();
    let conversation_id = input.conversation_id;
    // A failed write must not start or replace a session, or emit a phantom row.
    let message = match effects.create_response(conversation_id, metadata) {
        Ok(message) => message,
        Err(error) => { effects.clear_focus(conversation_id).await; return Err(error); }
    };
    effects.emit(conversation_id, ConversationEvent {
        r#type: "message_add".into(),
        data: serde_json::to_value(MessageAddEvent {
            message_id: message.id, message_type: "response".into(),
        }).unwrap(),
    });
    let handle = match effects.acquire(prepared, conversation_id).await {
        Ok(handle) => handle,
        Err(error) => {
            let detail = format!("{} session acquisition failed [conversation_id={}, message_id={}]: {}; no fallback was attempted", metadata.id, conversation_id, message.id, error);
            effects.fail(conversation_id, message.id, metadata, &detail).await;
            return Err(AppError::ProviderError(detail));
        }
    };
    // Native Codex/Claude already receive attachment text in the runtime prompt.
    let attachments = if metadata.operations.structured_attachments {
        input.messages.iter().rev()
            .find(|(message_type, _, _)| message_type == "user")
            .map(|(_, _, attachments)| attachments.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Err(error) = effects.submit(&handle, message.id, input.prompt, attachments) {
        let detail = format!(
            "{} prompt dispatch failed [conversation_id={}, message_id={}]: {}; no replacement session was started; process exit diagnostics are unavailable at the command dispatch layer",
            metadata.id, conversation_id, message.id, error,
        );
        effects.fail(conversation_id, message.id, metadata, &detail).await;
        return Err(AppError::UnknownError(detail));
    }
    Ok(AiResponse {
        conversation_id,
        request_prompt_result_with_context: input.display_prompt,
        error_message: None,
    })
}

/// Preparation happens after initialize_conversation persisted the user turn.
/// Its rejection therefore needs a terminal response and the same focus cleanup
/// as a native dispatch failure, without acquiring an actor.
pub(super) async fn reject_preparation<E: TurnEffects>(effects: &E, metadata: AgentDriverMetadata, conversation_id: i64, display_prompt: String, error: AppError) -> Result<AiResponse, AppError> {
    let detail = format!("{} preparation failed [conversation_id={}]: {}", metadata.id, conversation_id, error);
    let message = match effects.create_response(conversation_id, metadata) {
        Ok(message) => message,
        Err(persistence_error) => {
            effects.clear_focus(conversation_id).await;
            return Err(AppError::ProviderError(format!("{detail}; failure response persistence failed: {persistence_error}")));
        }
    };
    effects.emit(conversation_id, ConversationEvent {
        r#type: "message_add".into(),
        data: serde_json::to_value(MessageAddEvent { message_id: message.id, message_type: "response".into() }).unwrap(),
    });
    effects.fail(conversation_id, message.id, metadata, &detail).await;
    Ok(AiResponse { conversation_id, request_prompt_result_with_context: display_prompt, error_message: Some(detail) })
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;

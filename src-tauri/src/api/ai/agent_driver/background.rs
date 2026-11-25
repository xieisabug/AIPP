use super::events::{emit_to_chat_windows, AgentEventIdentity};
use super::registry::AgentDriverMetadata;
use crate::api::ai::events::ConversationEvent;
use crate::db::conversation_db::ConversationDatabase;
use crate::errors::AppError;
use serde_json::json;

pub(in crate::api::ai) fn can_continue(app: &tauri::AppHandle, conversation: i64, previous: i64) -> Result<bool, String> {
    let kind = crate::mcp::butler_tools::conversation_kind(app, Some(conversation)).map_err(|e| e.to_string())?;
    let interactive = match kind.as_str() {
        "normal" | "butler_contact" => true,
        "butler_main" => crate::api::butler_api::active_butler_main_conversation_id(app)? == Some(conversation),
        // Managed runs have their own completion/stop and delivery contract.
        _ => false,
    };
    Ok(interactive && crate::api::ai::events::output_goal_current(app, conversation, Some(previous)))
}

pub(in crate::api::ai) fn create_reply(app: &tauri::AppHandle, identity: &AgentEventIdentity, previous: i64, metadata: AgentDriverMetadata) -> Result<i64, AppError> {
    let db = ConversationDatabase::new(app)?;
    let repo = db.message_repo()?;
    let mut created = None;
    super::lifecycle::publish_if_current_binding(app, identity.conversation_id,
        &identity.driver_id, identity.run_id.as_deref(), || {
            created = Some(insert_reply(&repo, identity, previous, metadata));
        })?;
    let id = created.ok_or_else(|| AppError::ProviderError("后台任务所属的原生会话进程已更换".into()))??;
    if let Err(error) = emit_to_chat_windows(app, identity.clone().with_response_message(id), ConversationEvent {
        r#type: "message_add".into(), data: json!({"message_id": id, "message_type": "response"}),
    }) {
        tracing::warn!(conversation_id = identity.conversation_id, message_id = id, %error, "Native wake row saved but window notification failed");
    }
    Ok(id)
}

fn insert_reply(repo: &crate::db::conversation_db::MessageRepository, identity: &AgentEventIdentity, previous: i64, metadata: AgentDriverMetadata) -> Result<i64, AppError> {
    let origin = if metadata.id == crate::api::ai::claude_sdk::CLAUDE_SDK_API_TYPE {
        "claude_background_wake"
    } else {
        "native_background_wake"
    };
    let mut message = super::response_message(identity.conversation_id, metadata);
    message.metadata_json = Some(json!({"origin": origin}).to_string());
    Ok(repo.create_native_continuation(message, previous)?.id)
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;

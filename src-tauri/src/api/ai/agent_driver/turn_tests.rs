use super::*;
use super::super::session::AgentSessionKind;
use crate::db::assistant_db::{Assistant, AssistantModel};
use crate::db::conversation_db::AttachmentType;
use std::cell::Cell;

fn assistant() -> AssistantDetail {
    AssistantDetail {
        assistant: Assistant { id: 7, name: "test".into(), description: None, assistant_type: Some(4), is_addition: false, created_time: String::new() },
        model: vec![AssistantModel { id: 11, assistant_id: 7, provider_id: 1, model_code: "configured".into(), alias: String::new() }],
        model_configs: vec![], prompts: vec![], prompt_params: vec![], mcp_configs: vec![], mcp_tool_configs: vec![],
    }
}
fn registry() -> AgentDriverRegistry {
    let registry = AgentDriverRegistry::default(); super::super::native::register_builtin_drivers(&registry).unwrap(); registry
}
fn attachment(kind: AttachmentType) -> MessageAttachment {
    MessageAttachment { id: 12, message_id: 9, attachment_type: kind, attachment_url: Some("notes.txt".into()), attachment_content: Some("attached evidence".into()), attachment_hash: None, use_vector: false, token_count: None }
}

#[test]
fn test_turn_selection_invalid_override_never_reads_provider_or_defaults() {
    let registry = registry(); let assistant = assistant();
    for override_id in ["wrong", "model%%0", "model%%-2", "%%3"] {
        let called = Cell::new(false);
        let error = load_agent_instance(&registry, &assistant, Some(override_id), |_| { called.set(true); Ok(("acp".into(), vec![])) }).err().unwrap().to_string();
        assert!(!called.get()); assert!(error.contains("未回退"));
    }
}

#[test]
fn test_turn_selection_selected_instance_read_failure_does_not_retry_default() {
    let registry = registry(); let assistant = assistant(); let calls = Cell::new(0);
    let error = load_agent_instance(&registry, &assistant, Some("selected%%93"), |id| {
        calls.set(calls.get() + 1); assert_eq!(id, 93); Err(AppError::ProviderError("provider 93 missing".into()))
    }).err().unwrap().to_string();
    assert_eq!(calls.get(), 1); assert!(error.contains("provider 93 missing"));
}

#[test]
fn test_turn_selection_rejects_provider_rows_from_another_instance() {
    let registry = registry(); let assistant = assistant();
    let error = load_agent_instance(&registry, &assistant, None, |_| Ok(("codex_app_server".into(), vec![LLMProviderConfig { id: 1, llm_provider_id: 2, name: "codex_cli_command".into(), value: "codex".into(), append_location: String::new(), is_addition: false }]))).err().unwrap().to_string();
    assert!(error.contains("owned by provider 2"));
}

/// Uses the actual turn preparation selector with the initialized runtime user
/// message, not an already-fixed prompt passed directly into dispatch.
#[test]
fn test_turn_text_drivers_receive_initialized_attachment_context_once() {
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let contextual = "private prefix\nquestion\n<fileattachment name=\"notes.txt\">attached evidence</fileattachment>";
    let messages = vec![("user".into(), contextual.into(), vec![attachment(AttachmentType::Text)])];
    let request = AgentTurnRequest { assistant: &assistant, conversation_id: 42, prompt: "private prefix\nquestion".into(), display_prompt: "question".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &messages };
    for kind in [AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let prompt = prompt_for_driver(kind.into(), &request).unwrap();
        assert_eq!(prompt, contextual); assert_eq!(prompt.matches("attached evidence").count(), 1);
    }
    assert_eq!(prompt_for_driver(AgentSessionKind::Acp.into(), &request).unwrap(), request.prompt);
}

#[test]
fn test_turn_text_drivers_reject_non_text_attachments_instead_of_dropping_them() {
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let messages = vec![("user".into(), "question".into(), vec![attachment(AttachmentType::Image)])];
    let request = AgentTurnRequest { assistant: &assistant, conversation_id: 42, prompt: "question".into(), display_prompt: "question".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &messages };
    let error = prompt_for_driver(AgentSessionKind::Codex.into(), &request).err().unwrap().to_string();
    assert!(error.contains("attachment_id=12")); assert!(error.contains("without losing user input"));
}

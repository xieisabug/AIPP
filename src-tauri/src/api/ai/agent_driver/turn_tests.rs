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
fn test_provider_switch_controls_native_agent_turns() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let id = db.add_llm_provider("本地 Codex", "codex_app_server", "", false, false).unwrap();
    let mut assistant = assistant();
    assistant.model[0].provider_id = id;
    let registry = registry();
    let resolve = |model: Option<&str>| load_agent_instance(&registry, &assistant, model,
        |id| read_turn_provider(&db, id, assistant.assistant.id));
    assert!(resolve(None).unwrap_err().to_string().contains("本地 Codex"));
    assert!(resolve(Some(&format!("selected%%{id}"))).unwrap_err().to_string().contains("已停用"));
    db.update_llm_provider(id, "本地 Codex", "codex_app_server", "", true).unwrap();
    assert_eq!(resolve(None).unwrap().0.provider_id, id);
}

#[test]
fn test_onboarding_aux_chief_native_default_uses_provider_model_unless_explicitly_selected() {
    let mut chief = assistant();
    chief.model[0].model_code.clear();
    let registry = registry();
    for api_type in ["codex_app_server", "claude_sdk"] {
        let resolve = |selection| load_agent_instance(&registry, &chief, Some(selection),
            |_| Ok((api_type.to_string(), vec![]))).unwrap().1;
        assert_eq!(resolve("acp%%1"), None);
        assert_eq!(resolve("__agent_default__%%1"), None);
        assert_eq!(resolve("selected%%1"), Some("selected".into()));
    }
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
    let request = AgentTurnRequest { source_message_id: None, native_butler_role: None, assistant: &assistant, conversation_id: 42, prompt: "private prefix\nquestion".into(), display_prompt: "question".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &messages };
    for kind in [AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let prompt = prompt_for_driver(kind.into(), &request).unwrap();
        assert_eq!(prompt, contextual); assert_eq!(prompt.matches("attached evidence").count(), 1);
    }
    assert_eq!(prompt_for_driver(AgentSessionKind::Acp.into(), &request).unwrap(), request.prompt);
}

#[test]
fn test_turn_rejects_image_without_bytes_or_local_path() {
    let assistant = assistant(); let overrides = None; let features = HashMap::new();
    let messages = vec![("user".into(), "question".into(), vec![attachment(AttachmentType::Image)])];
    let request = AgentTurnRequest { source_message_id: None, native_butler_role: None, assistant: &assistant, conversation_id: 42, prompt: "question".into(), display_prompt: "question".into(), override_model_id: None, overrides: &overrides, feature_configs: &features, messages: &messages };
    let error = prompt_for_driver(AgentSessionKind::Codex.into(), &request).err().unwrap().to_string();
    assert!(error.contains("attachment_id=12")); assert!(error.contains("重新选择"));
}

#[test]
fn test_claude_images_keep_text_context_and_missing_pdf_reports_its_id() {
    let assistant = assistant();
    let features = HashMap::new();
    for (kind, accepted) in [(AttachmentType::Image, true), (AttachmentType::PDF, false)] {
        let mut file = attachment(kind);
        if kind == AttachmentType::Image { file.attachment_content = Some("data:image/png;base64,AAAA".into()); }
        let messages = vec![("user".into(), "question + notes + turn context".into(), vec![attachment(AttachmentType::Text), file])];
        let request = AgentTurnRequest { source_message_id: None, native_butler_role: None, assistant: &assistant, conversation_id: 42,
            prompt: "question".into(), display_prompt: "question".into(), override_model_id: None, overrides: &None,
            feature_configs: &features, messages: &messages };
        let result = prompt_for_driver(AgentSessionKind::Claude.into(), &request);
        if accepted {
            assert_eq!(result.unwrap(), messages[0].1);
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("attachment_id=12") && error.contains("PDF"));
        }
    }
}


// T3 attachmentPrompt.test.ts keeps every path at the text budget and identifies missing inputs.
#[test]
fn test_attachment_paths_survive_long_prompts_across_every_driver() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let files: Vec<_> = ["Cargo.toml", "Cargo.lock"].iter().map(|name| MessageAttachment {
        attachment_url: Some(root.join(name).to_str().unwrap().into()), attachment_content: None,
        ..attachment(AttachmentType::File)
    }).collect();
    let long = "x".repeat(120_000);
    let registry = registry();
    for id in ["acp", "codex_app_server", "claude_sdk", "pi", "cursor", "opencode", "kiro", "antigravity"] {
        let metadata = registry.driver(id).unwrap().metadata();
        let prompt = prompt_with_file_paths(metadata, 42, &long, &files).unwrap();
        assert!(prompt.starts_with(&long));
        for file in &files { assert!(prompt.contains(file.attachment_url.as_deref().unwrap()), "{id}"); }
        let mut missing = files[0].clone();
        missing.attachment_url = Some(root.join("missing-attachment-fixture.zip").display().to_string());
        let error = prompt_with_file_paths(metadata, 42, "", &[missing]).unwrap_err().to_string();
        assert!(error.contains("missing-attachment-fixture.zip") && error.contains("attachment_id=12"), "{id}: {error}");
    }
}

#[test]
fn test_attachment_agent_can_reopen_the_original_zip_and_reports_a_later_move() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("工作 资料.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("notes/answer.txt", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"archive evidence 813").unwrap();
    drop(zip.finish().unwrap());
    let file = MessageAttachment { attachment_url: Some(path.display().to_string()), attachment_content: None,
        ..attachment(AttachmentType::File) };
    let metadata = registry().driver("codex_app_server").unwrap().metadata();
    let input = || prompt_with_file_paths(metadata, 42, "", std::slice::from_ref(&file));
    {
        let prompt = input().unwrap();
        let delivered_path = prompt.strip_prefix("附件路径 ").unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(delivered_path).unwrap()).unwrap();
        let mut evidence = String::new();
        archive.by_name("notes/answer.txt").unwrap().read_to_string(&mut evidence).unwrap();
        assert_eq!(evidence, "archive evidence 813");
    }
    std::fs::rename(&path, dir.path().join("移动后.zip")).unwrap();
    assert!(input().unwrap_err().to_string().contains("重新选择"));
}

use super::*;
use crate::api::ai::agent_driver::dispatch::response_message;
use crate::api::ai::agent_driver::session::AgentSessionKind;
use serde_json::Value;
use std::cell::RefCell;

#[allow(dead_code)]
#[path = "../../../db/tests/test_helpers.rs"]
mod database_fixture;

struct RecordingSink<'a> {
    repo: &'a MessageRepository,
    message_id: i64,
    require_persisted: bool,
    events: RefCell<Vec<(String, Value)>>,
}

impl AgentEventSink for RecordingSink<'_> {
    fn emit(&self, name: &str, payload: &Value) -> Result<(), String> {
        if self.require_persisted {
            let message = self.repo.read(self.message_id).unwrap().unwrap();
            assert_eq!(message.message_type, "error", "failure must persist before any publication");
            assert!(message.finish_time.is_some());
        }
        self.events.borrow_mut().push((name.into(), payload.clone()));
        Ok(())
    }
}

fn failure(driver: &str, message_id: i64, run: &str) -> AgentFailure {
    AgentFailure::new(AgentEventIdentity::new(driver, 42).with_instance(17)
        .with_session("session-1").with_run(run).with_turn("turn-1").with_response_message(message_id),
        AgentFailureStage::Protocol, "JSON-RPC stream ended while awaiting turn/completed")
        .with_process_diagnostics(AgentProcessDiagnostics {
            stderr: Some("fatal: native socket closed\ntrace: raw vendor detail".into()),
            exit_status: Some("exit code: 7".into()), protocol_error: Some("invalid frame at byte 19".into()),
            unavailable_reason: None,
        })
}

/// Exercise production normalization, atomic repository persistence and event
/// publication against SQLite memory, substituting only desktop delivery.
#[test]
fn test_agent_failure_persists_diagnostics_before_one_terminal_for_all_drivers() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let repo = MessageRepository::new(database_fixture::create_test_db());
        let mut row = response_message(42, kind);
        row.metadata_json = Some(json!({"existing": {"opaque": [1, null, "原样"]}}).to_string());
        let row = repo.create(&row).unwrap();
        let sink = RecordingSink { repo: &repo, message_id: row.id, require_persisted: true, events: RefCell::new(Vec::new()) };
        let failure = failure(kind.api_type(), row.id, "run-1");
        assert_eq!(persist_and_publish_failure(&repo, &sink, &failure, "Agent").unwrap(), FailurePublication::Published);
        let message = repo.read(row.id).unwrap().unwrap();
        assert_eq!(message.content, failure.content("Agent"));
        assert_eq!(message.message_type, "error");
        assert!(message.content.contains("stage=protocol"));
        assert!(message.content.contains("conversation_id=42"));
        assert!(message.content.contains("run_id=run-1"));
        assert!(message.content.contains("invalid frame at byte 19"));
        assert!(message.content.contains("exit code: 7"));
        assert!(message.content.contains("fatal: native socket closed\ntrace: raw vendor detail"));
        let metadata: Value = serde_json::from_str(message.metadata_json.as_ref().unwrap()).unwrap();
        assert_eq!(metadata["existing"]["opaque"], json!([1, null, "原样"]));
        assert_eq!(metadata["agent_failure"], serde_json::to_value(&failure).unwrap());
        let events = sink.events.borrow();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].0, "conversation_event_42");
        assert_eq!(events[0].1["type"], "message_update");
        assert_eq!(events[0].1["data"]["content"], message.content);
        assert_eq!(events[1].0, "agent_event_42");
        assert_eq!(events[2].1["type"], "stream_complete");
        assert_eq!(events[2].1["data"]["response_message_id"], row.id);
        assert_eq!(events[2].1["data"]["error"], failure.cause);
        assert_eq!(events[2].1["data"]["agent_failure"], serde_json::to_value(&failure).unwrap());
        assert_eq!(events[3].1["identity"], serde_json::to_value(&failure.identity).unwrap());
    }
}

#[test]
fn test_agent_failure_duplicate_and_late_old_run_cannot_overwrite_terminal_message() {
    let repo = MessageRepository::new(database_fixture::create_test_db());
    let old = repo.create(&response_message(42, AgentSessionKind::Codex)).unwrap();
    let sink = RecordingSink { repo: &repo, message_id: old.id, require_persisted: false, events: RefCell::new(Vec::new()) };
    let original = failure("codex_app_server", old.id, "old-run");
    assert_eq!(persist_and_publish_failure(&repo, &sink, &original, "Codex").unwrap(), FailurePublication::Published);
    let first_content = repo.read(old.id).unwrap().unwrap().content;
    let mut late = original.clone();
    late.cause = "late duplicate process teardown diagnostic".into();
    assert_eq!(persist_and_publish_failure(&repo, &sink, &late, "Codex").unwrap(), FailurePublication::AlreadyFinishedOrMissing);
    assert_eq!(repo.read(old.id).unwrap().unwrap().content, first_content);
    assert_eq!(sink.events.borrow().len(), 4);
    // A new run and its distinct response in the same conversation are not
    // suppressed by the previous message's terminal marker.
    let new = repo.create(&response_message(42, AgentSessionKind::Codex)).unwrap();
    let next = failure("codex_app_server", new.id, "new-run");
    assert_eq!(persist_and_publish_failure(&repo, &sink, &next, "Codex").unwrap(), FailurePublication::Published);
    assert_eq!(sink.events.borrow().len(), 8);
    assert!(repo.read(new.id).unwrap().unwrap().content.contains("new-run"));
}

#[test]
fn test_agent_failure_does_not_replace_success_or_cross_conversation_messages() {
    let repo = MessageRepository::new(database_fixture::create_test_db());
    let mut success = response_message(42, AgentSessionKind::Claude);
    success.content = "completed answer".into(); success.finish_time = Some(chrono::Utc::now());
    let success = repo.create(&success).unwrap();
    let other = repo.create(&response_message(43, AgentSessionKind::Claude)).unwrap();
    let sink = RecordingSink { repo: &repo, message_id: success.id, require_persisted: false, events: RefCell::new(Vec::new()) };
    for message_id in [success.id, other.id, 999_999] {
        assert_eq!(persist_and_publish_failure(&repo, &sink, &failure("claude_sdk", message_id, "late-run"), "Claude Code").unwrap(),
            FailurePublication::AlreadyFinishedOrMissing);
    }
    assert!(sink.events.borrow().is_empty());
    assert_eq!(repo.read(success.id).unwrap().unwrap().content, "completed answer");
    assert_eq!(repo.read(success.id).unwrap().unwrap().message_type, "response");
    assert!(repo.read(other.id).unwrap().unwrap().finish_time.is_none());
}

#[test]
fn test_agent_failure_database_write_error_is_published_without_hiding_original_cause() {
    let conn = database_fixture::create_test_db();
    conn.execute_batch("CREATE TRIGGER reject_agent_failure BEFORE UPDATE ON message BEGIN SELECT RAISE(ABORT, 'injected persistence failure'); END;").unwrap();
    let repo = MessageRepository::new(conn);
    let row = repo.create(&response_message(42, AgentSessionKind::Acp)).unwrap();
    let sink = RecordingSink { repo: &repo, message_id: row.id, require_persisted: false, events: RefCell::new(Vec::new()) };
    let original = failure("acp", row.id, "run-1");
    let error = persist_and_publish_failure(&repo, &sink, &original, "ACP").unwrap_err();
    assert!(error.contains("injected persistence failure"));
    assert!(repo.read(row.id).unwrap().unwrap().finish_time.is_none());
    let events = sink.events.borrow();
    assert_eq!(events.len(), 4);
    assert_eq!(events[2].1["data"]["error"], original.cause);
    assert!(events[2].1["data"]["agent_failure"]["persistence_error"].as_str().unwrap().contains("injected persistence failure"));
    assert!(events[0].1["data"]["content"].as_str().unwrap().contains("failure_persistence_error"));
}

#[test]
fn test_agent_failure_records_missing_diagnostics_and_preserves_malformed_metadata() {
    let repo = MessageRepository::new(database_fixture::create_test_db());
    let mut row = response_message(42, AgentSessionKind::Acp); row.metadata_json = Some("opaque-old-metadata".into());
    let row = repo.create(&row).unwrap();
    let sink = RecordingSink { repo: &repo, message_id: row.id, require_persisted: true, events: RefCell::new(Vec::new()) };
    let failure = AgentFailure::new(AgentEventIdentity::new("acp", 42).with_response_message(row.id), AgentFailureStage::Initialize, " ");
    persist_and_publish_failure(&repo, &sink, &failure, "ACP").unwrap();
    let message = repo.read(row.id).unwrap().unwrap();
    assert!(message.content.contains("underlying cause was not provided"));
    assert!(message.content.contains("diagnostics_unavailable"));
    let metadata: Value = serde_json::from_str(message.metadata_json.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["previous_metadata_raw"], "opaque-old-metadata");
    assert_eq!(metadata["agent_failure"]["identity"]["run_id"], Value::Null);
}

#[test]
fn test_agent_failure_registered_extension_retains_custom_driver_and_model() {
    let metadata = super::super::registry::AgentDriverMetadata {
        id: "test_extension", display_name: "Custom Agent", message_model_name: "custom-model",
        operations: super::super::session::AgentSessionCapabilities {
            structured_attachments: true, cancel_prompt: true, set_config_option: true,
        },
    };
    let repo = MessageRepository::new(database_fixture::create_test_db());
    let row = repo.create(&response_message(42, metadata)).unwrap();
    let sink = RecordingSink { repo: &repo, message_id: row.id, require_persisted: true, events: RefCell::new(Vec::new()) };
    let failure = failure(metadata.id, row.id, "extension-run");
    persist_and_publish_failure(&repo, &sink, &failure, metadata.display_name).unwrap();
    let saved = repo.read(row.id).unwrap().unwrap();
    assert_eq!(saved.llm_model_name.as_deref(), Some("custom-model"));
    assert!(saved.content.starts_with("Custom Agent 运行失败"));
    assert_eq!(sink.events.borrow()[3].1["identity"]["driver_id"], "test_extension");
}

#[test]
fn test_agent_diagnostics_are_bounded_and_redact_credentials_before_retention() {
    let buffer = AgentDiagnosticBuffer::for_environment(&std::collections::HashMap::from([
        ("ANTHROPIC_API_KEY".into(), "configured-credential".into()),
        ("NORMAL_SETTING".into(), "visible-context".into()),
    ]));
    for index in 0..140 { buffer.push(format!("line-{index}")); }
    buffer.push("connection failed: configured-credential; api_key=other-secret Authorization: Bearer token-value");
    buffer.push("visible-context: service rejected the request");
    let diagnostic = buffer.diagnostics(Some("exit code: 3".into()), None);
    let stderr = diagnostic.stderr.unwrap();
    assert_eq!(stderr.lines().count(), 128);
    assert!(!stderr.contains("line-0\n"));
    assert!(!stderr.contains("configured-credential"));
    assert!(!stderr.contains("other-secret"));
    assert!(!stderr.contains("token-value"));
    assert!(stderr.contains("connection failed"));
    assert!(stderr.contains("visible-context: service rejected the request"));
    assert_eq!(diagnostic.exit_status.as_deref(), Some("exit code: 3"));
    assert_eq!(diagnostic.unavailable_reason, None);
}

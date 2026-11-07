use super::*;
use crate::api::ai::agent_driver::failure::{failure_events, persist_failure_message};
use crate::db::conversation_db::{AttachmentType, MessageRepository, Repository};
use std::sync::Mutex;

#[allow(dead_code)]
#[path = "../../../db/tests/test_helpers.rs"]
mod database_fixture;

#[derive(Debug)]
struct Submitted {
    conversation_id: i64,
    message_id: i64,
    prompt: String,
    attachments: Vec<MessageAttachment>,
}

// The real MessageRepository and production payload/persistence helpers are
// exercised. Only desktop events and session acquisition/submission are fakes;
// these tests do not claim to run a native CLI or its transport state machine.
struct RecordingEffects {
    repo: Mutex<MessageRepository>,
    actions: Mutex<Vec<String>>,
    events: Mutex<Vec<(i64, ConversationEvent)>>,
    submitted: Mutex<Vec<Submitted>>,
    acquire_barrier: Option<tokio::sync::Barrier>,
    fail_submit: bool,
}

impl RecordingEffects {
    fn new() -> Self {
        Self {
            repo: Mutex::new(MessageRepository::new(database_fixture::create_test_db())),
            actions: Mutex::new(Vec::new()), events: Mutex::new(Vec::new()),
            submitted: Mutex::new(Vec::new()), acquire_barrier: None, fail_submit: false,
        }
    }
}

#[async_trait::async_trait]
impl TurnEffects for RecordingEffects {
    type Prepared = (i64, AgentSessionKind);
    type Handle = i64;

    fn create_response(&self, conversation_id: i64, kind: AgentSessionKind) -> Result<Message, AppError> {
        self.actions.lock().unwrap().push(format!("persist:{conversation_id}"));
        self.repo.lock().unwrap().create(&response_message(conversation_id, kind)).map_err(AppError::from)
    }

    fn emit(&self, conversation_id: i64, event: ConversationEvent) {
        self.actions.lock().unwrap().push(format!("{}:{conversation_id}", event.r#type));
        self.events.lock().unwrap().push((conversation_id, event));
    }

    async fn acquire(&self, prepared: Self::Prepared, conversation_id: i64) -> i64 {
        assert_eq!(prepared.0, conversation_id);
        self.actions.lock().unwrap().push(format!("acquire:{conversation_id}"));
        if let Some(barrier) = &self.acquire_barrier { barrier.wait().await; }
        conversation_id
    }

    fn submit(&self, handle: &i64, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>) -> Result<(), AppError> {
        assert!(self.repo.lock().unwrap().read(message_id).unwrap().is_some());
        self.actions.lock().unwrap().push(format!("submit:{handle}"));
        self.submitted.lock().unwrap().push(Submitted {
            conversation_id: *handle, message_id, prompt, attachments,
        });
        if self.fail_submit {
            Err(AppError::UnknownError("native actor closed [run_id=test-run]".into()))
        } else {
            Ok(())
        }
    }

    async fn fail(&self, conversation_id: i64, message_id: i64, kind: AgentSessionKind, detail: &str) {
        let content = format!("{} 运行失败：{detail}", provider_name(kind));
        persist_failure_message(&self.repo.lock().unwrap(), message_id, &content).unwrap();
        for event in failure_events(conversation_id, message_id, content, detail) {
            self.emit(conversation_id, event);
        }
        self.actions.lock().unwrap().push(format!("clear_focus:{conversation_id}"));
    }
}

fn attachment(id: i64, content: &str) -> MessageAttachment {
    MessageAttachment {
        id, message_id: 12, attachment_type: AttachmentType::Text,
        attachment_url: None, attachment_content: Some(content.into()),
        attachment_hash: Some(format!("hash-{id}")), use_vector: false, token_count: Some(3),
    }
}

fn input<'a>(conversation_id: i64, messages: &'a [(String, String, Vec<MessageAttachment>)]) -> TurnInput<'a> {
    TurnInput { conversation_id, prompt: "runtime prompt with attachment text".into(), display_prompt: "display prompt".into(), messages }
}

#[tokio::test]
async fn test_dispatch_persists_before_acquiring_or_submitting_all_drivers() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let effects = RecordingEffects::new();
        let response = dispatch_prepared(&effects, (42, kind), kind, input(42, &[])).await.unwrap();
        assert_eq!(response.conversation_id, 42);
        assert_eq!(response.request_prompt_result_with_context, "display prompt");
        assert_eq!(*effects.actions.lock().unwrap(), ["persist:42", "message_add:42", "acquire:42", "submit:42"]);
        let submitted = effects.submitted.lock().unwrap();
        let message = effects.repo.lock().unwrap().read(submitted[0].message_id).unwrap().unwrap();
        assert_eq!(message.conversation_id, 42);
        assert_eq!(message.message_type, "response");
        assert_eq!(message.llm_model_id, Some(0));
        assert_eq!(message.llm_model_name, response_message(42, kind).llm_model_name);
        assert!(message.content.is_empty());
        assert!(message.start_time.is_some());
        assert!(message.finish_time.is_none());
        assert_eq!(submitted[0].prompt, "runtime prompt with attachment text");
        let events = effects.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1.data["message_id"], message.id);
        assert_eq!(events[0].1.data["message_type"], "response");
    }
}

#[tokio::test]
async fn test_dispatch_write_failure_has_no_session_or_event_side_effects() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let mut effects = RecordingEffects::new();
        let conn = database_fixture::create_test_db();
        conn.execute_batch("CREATE TRIGGER reject_response BEFORE INSERT ON message BEGIN SELECT RAISE(ABORT, 'injected SQLite write failure'); END;").unwrap();
        effects.repo = Mutex::new(MessageRepository::new(conn));
        let error = dispatch_prepared(&effects, (42, kind), kind, input(42, &[])).await.err().unwrap();
        assert!(error.to_string().contains("SQLite write failure"));
        assert_eq!(*effects.actions.lock().unwrap(), ["persist:42"]);
        assert!(effects.events.lock().unwrap().is_empty());
        assert!(effects.submitted.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn test_dispatch_acp_forwards_only_latest_user_attachments_without_modification() {
    let effects = RecordingEffects::new();
    let expected = vec![attachment(2, "latest"), attachment(3, "second")];
    let messages = vec![
        ("user".into(), "old".into(), vec![attachment(1, "old")]),
        ("user".into(), "new".into(), expected.clone()),
        ("assistant".into(), "reply".into(), vec![attachment(9, "not user")]),
    ];
    dispatch_prepared(&effects, (42, AgentSessionKind::Acp), AgentSessionKind::Acp, input(42, &messages)).await.unwrap();
    assert_eq!(serde_json::to_value(&effects.submitted.lock().unwrap()[0].attachments).unwrap(), serde_json::to_value(expected).unwrap());
}

#[tokio::test]
async fn test_dispatch_native_drivers_keep_runtime_text_without_structured_attachments() {
    for kind in [AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let effects = RecordingEffects::new();
        let messages = vec![("user".into(), "input".into(), vec![attachment(1, "embedded")])];
        dispatch_prepared(&effects, (42, kind), kind, input(42, &messages)).await.unwrap();
        let submitted = effects.submitted.lock().unwrap();
        assert!(submitted[0].attachments.is_empty());
        assert_eq!(submitted[0].prompt, "runtime prompt with attachment text");
    }
}

#[tokio::test]
async fn test_dispatch_closed_actor_persists_failure_emits_one_terminal_and_never_retries() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let mut effects = RecordingEffects::new(); effects.fail_submit = true;
        let error = dispatch_prepared(&effects, (42, kind), kind, input(42, &[])).await.err().unwrap().to_string();
        assert!(error.contains("conversation_id=42"));
        assert!(error.contains("run_id=test-run"));
        assert!(error.contains(kind.api_type()));
        let submitted = effects.submitted.lock().unwrap(); assert_eq!(submitted.len(), 1);
        let message = effects.repo.lock().unwrap().read(submitted[0].message_id).unwrap().unwrap();
        assert_eq!(message.message_type, "error"); assert!(message.finish_time.is_some());
        assert!(message.content.contains("native actor closed"));
        assert_eq!(*effects.actions.lock().unwrap(), ["persist:42", "message_add:42", "acquire:42", "submit:42", "message_update:42", "stream_complete:42", "clear_focus:42"]);
        let events = effects.events.lock().unwrap();
        assert_eq!(events[1].1.data["message_id"], message.id);
        assert_eq!(events[1].1.data["content"], message.content);
        assert_eq!(events[1].1.data["is_done"], true);
        assert_eq!(events[2].1.data["response_message_id"], message.id);
        assert_eq!(events[2].1.data["has_response"], false);
        assert!(events[2].1.data["error"].as_str().unwrap().contains("run_id=test-run"));
    }
}

#[tokio::test]
async fn test_dispatch_concurrent_conversations_keep_prompt_message_and_attachment_identity() {
    let mut effects = RecordingEffects::new();
    effects.acquire_barrier = Some(tokio::sync::Barrier::new(2));
    let a = vec![("user".into(), "a".into(), vec![attachment(1, "a")])];
    let b = vec![("user".into(), "b".into(), vec![attachment(2, "b")])];
    let mut a_input = input(10, &a); a_input.prompt = "runtime prompt A".into();
    let mut b_input = input(20, &b); b_input.prompt = "runtime prompt B".into();
    let (a_result, b_result) = tokio::time::timeout(std::time::Duration::from_secs(2), async { tokio::join!(
        dispatch_prepared(&effects, (10, AgentSessionKind::Acp), AgentSessionKind::Acp, a_input),
        dispatch_prepared(&effects, (20, AgentSessionKind::Acp), AgentSessionKind::Acp, b_input),
    ) }).await.expect("both acquisitions must overlap before either submits");
    assert_eq!(a_result.unwrap().conversation_id, 10); assert_eq!(b_result.unwrap().conversation_id, 20);
    let actions = effects.actions.lock().unwrap();
    let first_submit = actions.iter().position(|action| action.starts_with("submit:")).unwrap();
    assert!(actions[..first_submit].contains(&"acquire:10".to_string()));
    assert!(actions[..first_submit].contains(&"acquire:20".to_string()));
    let submitted = effects.submitted.lock().unwrap(); assert_eq!(submitted.len(), 2);
    assert_ne!(submitted[0].message_id, submitted[1].message_id);
    for item in submitted.iter() {
        let row = effects.repo.lock().unwrap().read(item.message_id).unwrap().unwrap();
        assert_eq!(row.conversation_id, item.conversation_id);
        assert_eq!(item.prompt, if item.conversation_id == 10 { "runtime prompt A" } else { "runtime prompt B" });
        assert_eq!(item.attachments[0].id, if item.conversation_id == 10 { 1 } else { 2 });
    }
}

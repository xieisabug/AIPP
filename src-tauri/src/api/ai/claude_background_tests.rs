use super::*;

#[allow(dead_code)]
#[path = "../../db/tests/test_helpers.rs"]
mod database_fixture;

#[test]
fn test_claude_background_wake_persists_separate_reply_in_original_conversation() {
    use crate::api::ai::agent_driver::{response_message, session::AgentSessionKind};
    use crate::db::conversation_db::MessageRepository;
    let repo = MessageRepository::new(database_fixture::create_test_db());
    let identity = AgentEventIdentity::new(CLAUDE_SDK_API_TYPE, 42);
    let mut previous = response_message(42, AgentSessionKind::Claude);
    previous.content = "等待截图".into();
    previous.finish_time = Some(chrono::Utc::now());
    previous.parent_id = Some(5);
    let previous = repo.create(&previous).unwrap();
    let wake = repo.read(insert_reply(&repo, &identity, previous.id).unwrap()).unwrap().unwrap();
    assert_ne!(wake.id, previous.id);
    assert_eq!(wake.parent_id, Some(previous.id));
    assert_eq!(wake.conversation_id, 42);
    assert!(wake.finish_time.is_none());
    assert_eq!(serde_json::from_str::<Value>(&wake.metadata_json.unwrap()).unwrap()["origin"], "claude_background_wake");
    assert_eq!(repo.read(previous.id).unwrap().unwrap().content, "等待截图");
    let other = AgentEventIdentity::new(CLAUDE_SDK_API_TYPE, 43);
    assert!(insert_reply(&repo, &other, previous.id).is_err());
}

#[test]
fn test_claude_resumed_prompt_waits_for_its_answer_after_background_receipt() {
    use database_fixture::{create_message_test_db, create_test_message};
    use std::ops::ControlFlow::{Break, Continue};
    use crate::api::ai::agent_driver::{response_message, session::AgentSessionKind};

    let (repo, conversation) = create_message_test_db();
    let mut previous = response_message(conversation, AgentSessionKind::Claude);
    previous.content = "等待后台截图".into();
    previous.finish_time = Some(chrono::Utc::now());
    let previous = repo.create(&previous).unwrap();
    let prompt = repo.create(&create_test_message(conversation, "user", "截图好了吗？", Some(previous.id), None)).unwrap();
    let mut current = response_message(conversation, AgentSessionKind::Claude);
    current.parent_id = Some(prompt.id);
    let current = repo.create(&current).unwrap();
    let mut follow_up = ClaudeTurnOutput::new(current.id);
    let receipt = json!({
        "session_id": "resumed", "type": "result", "subtype": "success",
        "origin": {"kind": "task-notification"}, "is_error": false, "num_turns": 0,
        "result": "", "usage": {"output_tokens": 0}
    });
    assert_eq!(follow_up.apply_result(&receipt, None, false), Ok(Continue(())));
    assert!(follow_up.content.is_empty());
    assert!(follow_up.final_text.is_none());
    assert!(follow_up.usage.is_null());
    assert!(repo.read(current.id).unwrap().unwrap().finish_time.is_none());

    let answer = json!({
        "session_id": "resumed", "type": "result", "subtype": "success",
        "is_error": false, "num_turns": 1, "result": "截图已生成。",
        "usage": {"output_tokens": 6}
    });
    assert_eq!(follow_up.apply_result(&answer, None, false), Ok(Break(())));
    assert_eq!(follow_up.content, "截图已生成。");
    assert_eq!(follow_up.final_text.as_deref(), Some("截图已生成。"));
    assert_eq!(follow_up.usage, answer["usage"]);

    follow_up.persist_message(&repo, true, follow_up.final_text.as_deref());
    let saved = repo.read(current.id).unwrap().unwrap();
    assert_eq!(saved.parent_id, Some(prompt.id));
    assert_eq!(saved.content, "截图已生成。");
    assert!(saved.finish_time.is_some());
    assert_eq!(repo.read(previous.id).unwrap().unwrap().content, previous.content);
}

#[test]
fn test_claude_empty_user_result_still_ends_the_prompt() {
    let mut reply = ClaudeTurnOutput::new(12);
    let empty = json!({
        "type": "result", "subtype": "success", "is_error": false, "num_turns": 0, "result": ""
    });
    assert_eq!(reply.apply_result(&empty, None, false), Ok(std::ops::ControlFlow::Break(())));
    assert!(reply.content.is_empty());
}

#[tokio::test]
async fn test_claude_background_idle_notification_then_native_reply_without_input() {
    let (_commands, mut receiver) = mpsc::unbounded_channel();
    let mut tasks = BackgroundTasks::default();
    let start = json!({"type":"system","subtype":"task_started","task_id":"screenshot"});
    tasks.observe(&start, 10);
    let (read, mut writer) = tokio::io::duplex(4096);
    let mut lines = BufReader::new(read).lines();
    let frames = [
        json!({"type":"system","subtype":"background_tasks_changed","tasks":[]}),
        json!({"type":"assistant","parent_tool_use_id":"screenshot","message":{"content":[]}}),
        json!({"type":"system","subtype":"task_notification","task_id":"screenshot","status":"completed"}),
        json!({"type":"result","origin":{"kind":"task-notification"},"subtype":"success","num_turns":0,"result":""}),
        json!({"type":"system","subtype":"init","session_id":"original"}),
    ];
    for (i, frame) in frames.iter().enumerate() {
        writer.write_all(format!("{frame}\n").as_bytes()).await.unwrap();
        let Some(IdleEvent::Frame(received)) = read_idle_event(&mut lines, &mut receiver).await.unwrap() else { panic!("missing stdout frame") };
        let owner = tasks.observe(&received, 12);
        assert_eq!(owner, if i == 2 { Some(10) } else { None });
        assert_eq!(tasks.pending(), i < 2);
        assert_eq!(starts_wake(&received), i == 4);
        assert_eq!(lifecycle_result(&received), i == 3);
    }
    assert!(starts_wake(&json!({"type":"assistant","parent_tool_use_id":null})));
}

#[test]
fn test_claude_background_task_roster_and_parallel_tasks() {
    let mut tasks = BackgroundTasks::default();
    tasks.observe(&json!({"type":"system","subtype":"task_started","task_id":"foreground","is_backgrounded":false}), 10);
    assert!(!tasks.pending());
    tasks.observe(&json!({"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"build"}]}), 10);
    tasks.observe(&json!({"type":"system","subtype":"task_started","task_id":"agent"}), 10);
    tasks.observe(&json!({"type":"system","subtype":"task_notification","task_id":"build"}), 10);
    assert!(tasks.pending());
    tasks.observe(&json!({"type":"system","subtype":"task_notification","task_id":"agent","status":"failed"}), 10);
    assert!(!tasks.pending());
}

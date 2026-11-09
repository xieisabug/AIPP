//! Cross-feature contracts against production SQLite helpers, no model/network dependencies.
use super::{butler_control as control, butler_goal as goal};
use crate::mcp::builtin_mcp::superadmin::approval::ApprovalBinding;
use rusqlite::Connection;

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT,assistant_id INTEGER);
        INSERT INTO conversation VALUES(10,'butler_task',1),(20,'butler_task',2);
        CREATE TABLE message(id INTEGER PRIMARY KEY,parent_id INTEGER,conversation_id INTEGER,message_type TEXT,content TEXT,finish_time TEXT);
        CREATE TABLE queued_conversation_message(id INTEGER PRIMARY KEY,conversation_id INTEGER,queue_kind TEXT,status TEXT,request_json TEXT,prompt TEXT,assistant_id INTEGER);
        INSERT INTO message VALUES(1,NULL,10,'user','draft',NULL),(2,1,10,'tool_result','source evidence','done');").unwrap();
    goal::apply(&conn, 10, "replace", "prepare draft").unwrap();
    goal::apply(&conn, 20, "replace", "independent work").unwrap();
    conn.execute("INSERT INTO message VALUES(3,NULL,20,'user','independent',NULL)",[]).unwrap();
    goal::bind_turn(&conn,10,1,1).unwrap();goal::bind_turn(&conn,20,3,1).unwrap();
    conn
}

fn approval(conn: &Connection, conversation: i64) -> ApprovalBinding {
    ApprovalBinding {
        conversation, action: "send".into(), args: serde_json::json!({"recipient":"test@example.invalid"}),
        goal_revision: goal::current_revision(conn, conversation).unwrap(), latest_user: Some(1), expires_at: 1000,
    }
}

#[test]
fn test_goal_replacement_invalidates_approval_question_and_summary_but_not_other_task() {
    let conn = database();
    let approval_before = approval(&conn, 10);
    let unrelated_approval = approval(&conn, 20);
    let snapshot = goal::prompt(&conn, 10).unwrap().unwrap();
    let checkpoint = format!("<!-- compacted_ids: {} -->\n<context_summary>draft</context_summary>",
        serde_json::json!({"ids":[1],"digest":"source","goal_snapshot":snapshot}));
    goal::save_question(&conn, 10, "draft-question", r#"{"questions":[{"header":"format"}]}"#, Some(1), 100).unwrap();
    goal::save_question(&conn, 20, "other-question", r#"{"questions":[{"header":"place"}]}"#, Some(3), 100).unwrap();
    goal::apply(&conn, 10, "replace", "only summarize; do not send").unwrap();
    assert!(approval_before.validate(&approval(&conn, 10), 101).is_err());
    assert!(goal::validate_question(&conn, "draft-question", 101).is_err());
    assert!(!goal::checkpoint_is_current(&checkpoint, goal::prompt(&conn, 10).unwrap().as_deref()));
    assert!(unrelated_approval.validate(&approval(&conn, 20), 101).is_ok());
    assert_eq!(goal::validate_question(&conn, "other-question", 101).unwrap(), Some(20));
    assert_eq!(conn.query_row("SELECT content FROM message WHERE id=2", [], |r| r.get::<_,String>(0)).unwrap(), "source evidence");
}

#[test]
fn test_pause_preserves_waiting_context_and_does_not_pause_another_task() {
    let conn = database();
    goal::save_question(&conn, 10, "question", r#"{"questions":[{"header":"format"}]}"#, Some(1), 100).unwrap();
    let snapshot = goal::prompt(&conn, 10).unwrap();
    let version = goal::current_revision(&conn, 10).unwrap();
    control::set(&conn, 10, "pause").unwrap();
    assert!(control::ensure_turn(&conn, 10).is_err());
    assert!(control::ensure_delegation(&conn, 10).is_err());
    assert!(control::ensure_turn(&conn, 20).is_ok());
    assert_eq!(goal::prompt(&conn, 10).unwrap(), snapshot);
    assert_eq!(goal::current_revision(&conn, 10).unwrap(), version);
    assert_eq!(goal::waiting_questions(&conn, 10, 101).unwrap().len(), 1);
    control::set(&conn, 10, "resume").unwrap();
    assert!(control::ensure_turn(&conn, 10).is_ok());
    assert_eq!(goal::validate_question(&conn, "question", 101).unwrap(), Some(10));
    assert_eq!(conn.query_row("SELECT count(*) FROM queued_conversation_message", [], |r|r.get::<_,i64>(0)).unwrap(), 0);
}

#[test]
fn test_stop_delegation_keeps_existing_work_answerable_without_creating_another_task() {
    let mut conn = database();
    goal::save_question(&conn, 10, "question", r#"{"questions":[{"header":"format"}]}"#, Some(1), 100).unwrap();
    control::set(&conn, 10, "stop_delegation").unwrap();
    assert!(control::ensure_delegation(&conn, 10).is_err());
    assert!(control::ensure_turn(&conn, 10).is_ok());
    goal::assert_current(&conn, 10, goal::current_revision(&conn, 10).unwrap().unwrap()).unwrap();
    let answers = std::collections::HashMap::from([("format".into(), "markdown".into())]);
    assert_eq!(goal::recover_answer(&mut conn, "question", &answers, false, 101).unwrap(), 10);
    let request:String = conn.query_row("SELECT request_json FROM queued_conversation_message", [], |r|r.get(0)).unwrap();
    let request:serde_json::Value = serde_json::from_str(&request).unwrap();
    assert_eq!(request["conversation_id"], "10");
    assert_eq!(request["assistant_id"], 1);
    assert_eq!(conn.query_row("SELECT count(*) FROM conversation", [], |r|r.get::<_,i64>(0)).unwrap(), 2);
    assert!(control::ensure_delegation(&conn, 10).is_err());
    assert!(control::ensure_delegation(&conn, 20).is_ok());
    assert_eq!(conn.query_row("SELECT content FROM message WHERE id=2", [], |r|r.get::<_,String>(0)).unwrap(), "source evidence");
}

#[test]
fn test_cancel_and_goal_stop_are_terminal_without_destroying_other_work_or_evidence() {
    let conn = database();
    let revision = goal::current_revision(&conn, 10).unwrap().unwrap();
    goal::save_question(&conn, 10, "question", r#"{"questions":[{"header":"format"}]}"#, Some(1), 100).unwrap();
    control::set(&conn, 10, "cancel").unwrap();
    goal::apply(&conn, 10, "stop", "").unwrap();
    assert!(control::set(&conn, 10, "resume").is_err());
    assert!(control::ensure_turn(&conn, 10).is_err());
    assert!(goal::assert_current(&conn, 10, revision).is_err());
    assert!(goal::validate_question(&conn, "question", 101).is_err());
    assert!(control::ensure_turn(&conn, 20).is_ok());
    assert_eq!(goal::current(&conn, 20).unwrap().unwrap().text, "independent work");
    assert_eq!(conn.query_row("SELECT content FROM message WHERE id=2", [], |r|r.get::<_,String>(0)).unwrap(), "source evidence");
}

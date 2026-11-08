use super::resolve_continuation_source;
use crate::db::mcp_db::MCPToolCall;
fn call(id:i64,conversation:i64,native:Option<&str>,source:Option<i64>)->MCPToolCall {
    serde_json::from_value(serde_json::json!({"id":id,"conversation_id":conversation,"message_id":source,
        "assistant_message_id":source.map(|id|id+1),"server_id":1,"server_name":"test","tool_name":"read",
        "parameters":"{}","status":"success","created_time":"now","llm_call_id":native})).unwrap()
}
#[test]
fn public_continuation_uses_exact_call_not_newest_turn() {
    let calls=vec![call(1,10,Some("old-call"),Some(11)),call(2,10,Some("new-call"),Some(21))];
    assert_eq!(resolve_continuation_source(&calls,10,"old-call").unwrap(),12);
    assert!(resolve_continuation_source(&calls,20,"old-call").is_err());
    assert!(resolve_continuation_source(&calls,10,"unknown").is_err());
}
#[test]
fn public_continuation_rejects_reused_ids_and_missing_anchors() {
    let calls=vec![call(1,10,Some("same"),Some(11)),call(2,10,Some("same"),Some(21))];
    assert!(resolve_continuation_source(&calls,10,"same").is_err());
    assert!(resolve_continuation_source(&[call(1,10,None,None)],10,"mcp_tool_call_1").is_err());
    assert_eq!(resolve_continuation_source(&[call(1,10,None,Some(11))],10,"mcp_tool_call_1").unwrap(),12);
}

#[test]
fn native_drivers_cannot_enter_the_application_queue() {
    // Claude, Codex and ACP share native assistant type 4, not a queue capability.
    for _driver in ["claude", "codex", "acp"] {
        assert!(super::ensure_queue_supported(Some(4)).is_err());
    }
    assert!(super::ensure_queue_supported(Some(0)).is_ok());
    assert!(super::ensure_queue_supported(None).is_ok());
    assert!(super::ensure_queue_supported(Some(3)).is_err());
}

#[test]
fn handoff_requires_original_conversation_identity_not_goal_freshness() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT); INSERT INTO conversation VALUES(1,'butler_main'),(2,'butler_contact'); CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER); INSERT INTO message VALUES(10,1),(20,2);").unwrap();
    assert!(super::validate_continuation_identity(&conn,1,Some(10)).is_ok());
    assert!(super::validate_continuation_identity(&conn,1,Some(20)).is_err());
    assert!(super::validate_continuation_identity(&conn,1,Some(999)).is_err());
    assert!(super::validate_continuation_identity(&conn,1,None).is_err());
    crate::db::butler_control::set(&conn,1,"pause").unwrap();
    assert!(crate::db::butler_control::ensure_turn(&conn,1).is_err());
    crate::db::butler_control::set(&conn,1,"cancel").unwrap();
    assert!(crate::db::butler_control::ensure_turn(&conn,1).is_err());
}

#[test]
fn native_activity_uses_live_prompt_not_persistent_session_existence() {
    assert!(super::native_prompt_is_active(&serde_json::json!({"has_active_prompt":true})));
    assert!(!super::native_prompt_is_active(&serde_json::json!({"has_active_prompt":false})));
    assert!(super::native_prompt_is_active(&serde_json::json!({})));
}

#[test]
fn natural_interrupt_does_not_invalidate_running_tool_but_legacy_replacement_does() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT); INSERT INTO conversation VALUES(1,'butler_main'); CREATE TABLE message(id INTEGER PRIMARY KEY,parent_id INTEGER,conversation_id INTEGER,message_type TEXT,content TEXT,finish_time TEXT); INSERT INTO message VALUES(10,NULL,1,'user','original',NULL);").unwrap();
    let original = crate::db::butler_goal::accept_natural_message(&conn,1,"original").unwrap().unwrap();
    crate::db::butler_goal::bind_turn(&conn,1,10,original.revision).unwrap();
    for prompt in ["谢谢", "补充一个例子", "改为中文", "引用的旧要求只供参考"] {
        assert_eq!(super::prepare_queued_goal(&conn,1,None,prompt).unwrap(),None);
        assert_eq!(crate::db::butler_goal::current(&conn,1).unwrap().unwrap(),original);
        assert!(crate::db::butler_control::ensure_execution(&conn,1,Some(10)).is_ok());
    }
    let replacement = super::prepare_queued_goal(&conn,1,Some("replace"),"new").unwrap().unwrap();
    assert!(replacement > original.revision);
    // Identity is still trustworthy for recording evidence and handing off the
    // interrupt, but must not be accepted to continue the old model execution.
    assert!(super::validate_continuation_identity(&conn,1,Some(10)).is_ok());
    assert!(crate::db::butler_control::ensure_execution(&conn,1,Some(10)).is_err());
    assert_eq!(super::prepare_queued_goal(&conn,1,None,"latest adjustment").unwrap(),None);
}

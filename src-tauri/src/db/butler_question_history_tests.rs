use super::*;
use serde_json::json;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT,assistant_id INTEGER);
        CREATE TABLE butler_task_definition(task_conversation_id INTEGER,executor_assistant_id INTEGER);
        INSERT INTO conversation VALUES(10,'butler_contact',1),(20,'butler_contact',2),(41,'butler_task',1);
        INSERT INTO butler_task_definition VALUES(41,1);").unwrap();
    conn
}

#[test]
fn test_task_question_rejects_other_contact_and_deleted_task() {
    let conn = fixture();
    assert!(validate_owner(&conn, 10, 41).is_ok());
    assert!(validate_owner(&conn, 20, 41).is_err());
    conn.execute("DELETE FROM conversation WHERE id=41", []).unwrap();
    assert!(validate_owner(&conn, 10, 41).is_err());
}

#[test]
fn test_task_question_answer_history_survives_reload_and_is_idempotent() {
    let conn = fixture();
    for answer in ["PDF", "wrong duplicate"] {
        save(&conn, 41, AnswerRecord { key: "request:1".into(), questions: json!([{"question":"格式？"}]),
            metadata: json!({"source":"task"}), answers: HashMap::from([("格式？".into(),answer.into())]), cancelled: false, delivery_status:"delivered".into() }).unwrap();
    }
    let records = list(&conn, 41).unwrap();
    assert_eq!(records.len(),1);
    assert_eq!(records[0].answers["格式？"], "PDF");
    assert_eq!(records[0].questions[0]["question"], "格式？");
    assert!(list(&conn, 42).unwrap().is_empty());
}

#[test]
fn test_task_question_secret_answer_not_saved_in_history() {
    let conn = fixture();
    save(&conn, 41, AnswerRecord { key:"request:2".into(), questions:json!([{"question":"密码","header":"Secret","isSecret":true}]),
        metadata:Value::Null, answers:HashMap::from([("密码".into(),"top-secret".into())]),cancelled:false, delivery_status:"delivered".into() }).unwrap();
    assert_eq!(list(&conn,41).unwrap()[0].answers["密码"],"（敏感内容不保存）");
}

#[test]
fn test_stale_question_claim_rejects_cancelled_wrong_task_and_duplicate_answers() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE mcp_tool_call(id INTEGER PRIMARY KEY,conversation_id INTEGER,tool_name TEXT,status TEXT,error TEXT,finished_time TEXT);
        INSERT INTO mcp_tool_call VALUES(1,41,'ask_user_question','pending',NULL,NULL),(2,41,'ask_user_question','failed','Stopped by user',NULL),
        (3,41,'ask_user_question','executing',NULL,NULL),(4,41,'other_tool','pending',NULL,NULL);").unwrap();
    assert!(claim_stale(&conn,42,1,false).is_err());
    assert!(claim_stale(&conn,41,4,false).is_err());
    assert!(claim_stale(&conn,41,2,false).is_err());
    claim_stale(&conn,41,1,false).unwrap();
    assert!(claim_stale(&conn,41,1,false).is_err());
    claim_stale(&conn,41,3,true).unwrap();
    assert!(claim_stale(&conn,41,3,false).is_err());
}

#[test]
fn test_reply_intent_survives_unknown_delivery_without_claiming_answered() {
    let conn = fixture();
    save(&conn,41,AnswerRecord { key:"request:pending".into(),questions:json!([]),metadata:Value::Null,
        answers:HashMap::from([("格式".into(),"PDF".into())]),cancelled:false,delivery_status:"pending".into() }).unwrap();
    assert_eq!(list(&conn,41).unwrap()[0].delivery_status,"pending");
    finish_delivery(&conn,41,"request:pending").unwrap();
    assert_eq!(list(&conn,41).unwrap()[0].delivery_status,"delivered");
}

#[test]
fn test_pending_intent_can_be_corrected_before_delivery_but_final_answer_is_immutable() {
    let conn = fixture();
    for (answer,status) in [("PDF","pending"),("Excel","pending")] {
        save(&conn,41,AnswerRecord { key:"request:retry".into(),questions:json!([]),metadata:Value::Null,
            answers:HashMap::from([("格式".into(),answer.into())]),cancelled:false,delivery_status:status.into() }).unwrap();
    }
    finish_delivery(&conn,41,"request:retry").unwrap();
    save(&conn,41,AnswerRecord { key:"request:retry".into(),questions:json!([]),metadata:Value::Null,
        answers:HashMap::from([("格式".into(),"should-not-replace".into())]),cancelled:false,delivery_status:"pending".into() }).unwrap();
    let record = list(&conn,41).unwrap().remove(0);
    assert_eq!(record.answers["格式"],"Excel");
    assert_eq!(record.delivery_status,"delivered");
}

#[test]
fn test_stale_claim_interruption_keeps_intent_unconfirmed_until_handoff_finishes() {
    let history = fixture();
    let mcp = Connection::open_in_memory().unwrap();
    mcp.execute_batch("CREATE TABLE mcp_tool_call(id INTEGER PRIMARY KEY,conversation_id INTEGER,tool_name TEXT,status TEXT,error TEXT,finished_time TEXT);
        INSERT INTO mcp_tool_call VALUES(1,41,'ask_user_question','pending',NULL,NULL);").unwrap();
    save(&history,41,AnswerRecord { key:"call:1".into(),questions:json!([]),metadata:Value::Null,
        answers:HashMap::from([("格式".into(),"PDF".into())]),cancelled:false,delivery_status:"pending".into() }).unwrap();
    claim_stale(&mcp,41,1,false).unwrap();
    // Simulated restart after MCP CAS but before continuation enqueue: no claim of delivery.
    assert_eq!(list(&history,41).unwrap()[0].delivery_status,"pending");
    assert!(claim_stale(&mcp,41,1,false).is_err());
    assert_eq!(list(&history,41).unwrap()[0].delivery_status,"pending");
    finish_delivery(&history,41,"call:1").unwrap();
    assert_eq!(list(&history,41).unwrap()[0].delivery_status,"delivered");
}

use super::*;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("
        CREATE TABLE conversation(id INTEGER PRIMARY KEY, conversation_kind TEXT, assistant_id INTEGER,
            butler_task_status TEXT, butler_task_finalized_at TEXT, butler_task_summary TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT,
            content TEXT, metadata_json TEXT);
        CREATE TABLE butler_task_definition(id INTEGER PRIMARY KEY, task_conversation_id INTEGER,
            executor_assistant_id INTEGER, butler_conversation_id INTEGER, title TEXT, goal TEXT);
        CREATE TABLE butler_task_result(task_conversation_id INTEGER, final_message_id INTEGER, summary TEXT);
        INSERT INTO conversation VALUES (10,'butler_main',1,NULL,NULL,NULL),
            (20,'butler_contact',2,NULL,NULL,NULL),(30,'butler_contact',3,NULL,NULL,NULL),
            (41,'butler_task',2,'succeeded','done',NULL);
        INSERT INTO butler_task_definition VALUES (1,41,2,10,'报告','写报告');
        INSERT INTO message VALUES (1,41,'response','原报告正文',NULL);
        INSERT INTO butler_task_result VALUES (41,1,'摘要');
    ").unwrap();
    conn
}

fn explanation(conn: &Connection, user: i64, contact: i64, metadata: &str, answer: &str) {
    conn.execute("INSERT INTO message VALUES (?1,?2,'user','这段什么意思',?3)", params![user, contact, metadata]).unwrap();
    conn.execute("INSERT INTO message VALUES (?1,?2,'response',?3,NULL)", params![user + 1, contact, answer]).unwrap();
}

#[test]
fn test_quote_chain_keeps_task_identity_and_latest_excerpt_across_multiple_replies() {
    let conn = fixture();
    explanation(&conn, 100, 20, r#"{"quote":{"ref":"task:41","kind":"task","id":41}}"#, "第一段解释");
    let first = resolve(&conn, 20, "message", 101).unwrap();
    assert_eq!(first.work_ref, "task:41");
    assert_eq!(first.excerpt.as_deref(), Some("第一段解释"));
    explanation(&conn, 102, 20, r#"{"quote":{"ref":"task:41","kind":"message","id":101}}"#, "第二段解释");
    let second = resolve(&conn, 20, "message", 103).unwrap();
    assert_eq!(second.work_ref, "task:41");
    assert_eq!(second.task_conversation_id, Some(41));
    assert_eq!(second.excerpt.as_deref(), Some("第二段解释"));
    assert_eq!(second.result.as_deref(), Some("原报告正文"));
}

#[test]
fn test_quote_chain_rechecks_access_and_deleted_work_instead_of_trusting_metadata() {
    let conn = fixture();
    explanation(&conn, 100, 30, r#"{"quote":{"ref":"task:41","label":"可访问"}}"#, "不能泄露");
    assert!(resolve(&conn, 30, "message", 101).unwrap_err().to_string().contains("无权访问"));
    explanation(&conn, 102, 20, r#"{"quote":{"ref":"task:999"}}"#, "旧解释");
    assert!(resolve(&conn, 20, "message", 103).unwrap_err().to_string().contains("已删除"));
}

#[test]
fn test_quote_chain_does_not_follow_missing_recursive_or_unavailable_identity() {
    let conn = fixture();
    for (i, metadata) in [
        r#"{"quote":{"kind":"message","id":101}}"#,
        r#"{"quote":{"ref":"message:101"}}"#,
        r#"{"quote":{"ref":"task:41","unavailable":"原记录不可用"}}"#,
    ].iter().enumerate() {
        let user = 100 + (i as i64) * 2;
        explanation(&conn, user, 20, metadata, "解释");
        assert!(resolve(&conn, 20, "message", user + 1).is_err());
    }
}

#[test]
fn test_quote_chain_preserves_normal_turn_identity_without_creating_a_task() {
    let conn = fixture();
    conn.execute("INSERT INTO message VALUES (100,20,'user','普通聊天',NULL)", []).unwrap();
    explanation(&conn, 102, 20, r#"{"quote":{"ref":"turn:100","kind":"message","id":101}}"#, "普通解释");
    let view = resolve(&conn, 20, "message", 103).unwrap();
    assert_eq!(view.work_ref, "turn:100");
    assert_eq!(view.task_conversation_id, None);
    assert_eq!(view.title, "普通聊天");
}

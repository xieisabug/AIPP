use super::*;

#[test]
fn test_onboarding_aux_failed_summary_defers_just_that_conversation_and_recovers() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY, conversation_kind TEXT);
        CREATE TABLE message(conversation_id INTEGER, created_time TEXT);
        CREATE TABLE conversation_summary(id INTEGER PRIMARY KEY, conversation_id INTEGER);
        CREATE TABLE conversation_summary_retry(conversation_id INTEGER PRIMARY KEY, retry_after INTEGER, last_error TEXT);
        INSERT INTO conversation VALUES(1,'normal'),(2,'normal');
        INSERT INTO message VALUES(1,'2000-01-01'),(2,'2000-01-01');").unwrap();
    assert_eq!(eligible_conversations(&conn, &[]).unwrap(), [2, 1]);
    record_failure(&conn, 2, "模型拒绝本次请求").unwrap();
    assert_eq!(eligible_conversations(&conn, &[]).unwrap(), [1]);
    let reason: String = conn.query_row("SELECT last_error FROM conversation_summary_retry WHERE conversation_id=2 AND retry_after>unixepoch()+60",
        [], |row| row.get(0)).unwrap();
    assert_eq!(reason, "模型拒绝本次请求");
    conn.execute("UPDATE conversation_summary_retry SET retry_after=unixepoch()-1", []).unwrap();
    assert_eq!(eligible_conversations(&conn, &[]).unwrap(), [2, 1]);
}

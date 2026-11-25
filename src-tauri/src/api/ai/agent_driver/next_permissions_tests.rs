use super::*;
use crate::db::conversation_db::{ensure_agent_session_table, ConversationDatabase};

#[test]
fn test_next_permissions_follow_original_session_and_provider() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE acp_session (conversation_id INTEGER PRIMARY KEY, session_id TEXT, updated_time TEXT);
                        INSERT INTO acp_session VALUES (1,'original',CURRENT_TIMESTAMP)").unwrap();
    ensure_agent_session_table(&conn).unwrap();
    let bind = |provider, session| ConversationDatabase::upsert_agent_session_id_for_instance_with_conn(
        &conn, 1, "acp", Some(provider), session).unwrap();
    let selections = |provider, session| read(&conn, 1, "acp", Some(provider), Some(session)).unwrap();
    assert!(selections(2, "original").is_empty());
    bind(2, "original");
    save(&conn, 1, "acp", 2, "original", "approval", "ask").unwrap();
    save(&conn, 1, "acp", 2, "original", "sandbox", "read-only").unwrap();
    bind(2, "original");
    assert_eq!(selections(2, "original").len(), 2);
    assert!(save(&conn, 1, "acp", 9, "original", "approval", "allow").is_err());
    assert!(save(&conn, 1, "acp", 2, "other", "approval", "allow").is_err());
    assert!(selections(9, "original").is_empty());
    bind(3, "original");
    assert!(selections(3, "original").is_empty());
    save(&conn, 1, "acp", 3, "original", "approval", "ask").unwrap();
    bind(3, "other");
    assert!(selections(3, "other").is_empty());
}

#[test]
fn test_next_permissions_queued_message_uses_latest_selection_at_dispatch() {
    let (db, conn) = ConversationDatabase::shared_in_memory();
    db.create_tables().unwrap();
    conn.execute("INSERT INTO conversation (name, created_time, updated_time) VALUES ('test', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)", []).unwrap();
    let cid = conn.last_insert_rowid();
    db.upsert_agent_session_id_for_instance(cid, "kiro", Some(2), "original").unwrap();
    let queue = db.queued_message_repo().unwrap();
    let row = queue.enqueue(cid, "normal", "{}", "second", 1).unwrap();
    for value in ["on", "off"] {
        save(&conn, cid, "kiro", 2, "original", "autopilot", value).unwrap();
    }
    let outgoing = queue.take_next_for_dispatch(cid, false).unwrap().unwrap();
    assert_eq!(row.id, outgoing.id);
    assert_eq!(outgoing.prompt, "second");
    assert_eq!(read(&db.get_connection().unwrap(), outgoing.conversation_id, "kiro", Some(2), None).unwrap()["autopilot"], "off");
}

#[test]
fn test_next_permissions_recognizes_negotiated_acp_permissions() {
    assert!(deferred("acp", "custom", &serde_json::json!({"category":"mode"})));
    assert!(deferred("acp", "approvalPolicy", &Value::Null));
    assert!(!deferred("acp", "model", &Value::Null));
    assert!(!deferred("kiro", "mode", &Value::Null));
}

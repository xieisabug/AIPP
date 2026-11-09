use super::*;
#[test] fn test_control_pause_persists_and_resume_does_not_restore_delegation() {
 let c=Connection::open_in_memory().unwrap();
 set(&c,1,"stop_delegation").unwrap(); set(&c,1,"pause").unwrap();
 assert!(ensure_turn(&c,1).is_err()); assert!(ensure_turn(&c,2).is_ok());
 set(&c,1,"resume").unwrap(); assert!(ensure_turn(&c,1).is_ok());assert!(ensure_delegation(&c,1).is_err());
}
#[test] fn test_control_cancel_is_irreversible_and_other_work_remains_available() {
 let c=Connection::open_in_memory().unwrap();set(&c,1,"cancel").unwrap();
 assert!(set(&c,1,"resume").is_err());assert!(ensure_turn(&c,1).is_err());assert!(ensure_delegation(&c,2).is_ok());
}

fn execution_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT);
         INSERT INTO conversation VALUES(1,'butler_task'),(2,'butler_task'),(3,'normal');
         CREATE TABLE message(id INTEGER PRIMARY KEY,parent_id INTEGER,conversation_id INTEGER,
           message_type TEXT,content TEXT,finish_time TEXT);
         INSERT INTO message VALUES(10,NULL,1,'user','request',NULL),
           (20,NULL,2,'user','other request',NULL);",
    ).unwrap();
    for (conversation, source) in [(1, 10), (2, 20)] {
        let goal = crate::db::butler_goal::apply(&conn, conversation, "replace", "request")
            .unwrap().unwrap();
        crate::db::butler_goal::bind_turn(&conn, conversation, source, goal.revision).unwrap();
    }
    conn
}

#[test]
fn test_execution_late_tool_results_do_not_resume_paused_or_cancelled_work() {
    let conn = execution_db();
    assert!(ensure_execution(&conn, 1, Some(10)).is_ok());
    set(&conn, 1, "pause").unwrap();
    assert!(ensure_execution(&conn, 1, Some(10)).unwrap_err().to_string().contains("暂停"));
    assert!(ensure_execution(&conn, 2, Some(20)).is_ok());
    set(&conn, 1, "resume").unwrap();
    assert!(ensure_execution(&conn, 1, Some(10)).is_ok());
    set(&conn, 1, "cancel").unwrap();
    assert!(ensure_execution(&conn, 1, Some(10)).unwrap_err().to_string().contains("停止"));
}

#[test]
fn test_execution_resume_does_not_reauthorize_obsolete_source() {
    let conn = execution_db();
    set(&conn, 1, "pause").unwrap();
    crate::db::butler_goal::apply(&conn, 1, "replace", "new requirement").unwrap();
    set(&conn, 1, "resume").unwrap();
    assert!(ensure_execution(&conn, 1, Some(10)).unwrap_err().to_string().contains("目标已变更"));
    assert!(ensure_execution(&conn, 1, None).is_err());
    assert!(ensure_execution(&conn, 3, None).is_ok());
}

#[test]
fn test_execution_rechecks_controls_after_preparation_and_on_retry() {
    let conn = execution_db();
    assert!(ensure_execution(&conn, 1, Some(10)).is_ok());
    // A pause may arrive while context/hooks are awaited, or between retries.
    set(&conn, 1, "pause").unwrap();
    assert!(ensure_execution(&conn, 1, Some(10)).is_err());
    assert!(ensure_execution(&conn, 2, Some(20)).is_ok());
}

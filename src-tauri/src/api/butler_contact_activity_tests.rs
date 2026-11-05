use super::*;

#[test]
fn test_contact_activity_includes_all_tasks_and_preserves_owner() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT,butler_task_status TEXT);
        CREATE TABLE butler_task_definition(task_conversation_id INTEGER,executor_assistant_id INTEGER);
        CREATE TABLE butler_peer_exchange(owner INTEGER,state TEXT);
        INSERT INTO conversation VALUES(1,NULL,'butler_main',NULL),(2,99,'butler_main',NULL),(3,8,'butler_contact',NULL),(4,8,'normal',NULL),
            (5,NULL,'butler_contact',NULL),(6,NULL,'butler_task','running'),(7,NULL,'butler_task','running');
        INSERT INTO butler_task_definition VALUES(7,8);
        INSERT INTO butler_peer_exchange VALUES(3,'waiting');").unwrap();
    for id in 10..130 {
        conn.execute("INSERT INTO conversation VALUES(?1,99,'butler_task','running')", [id]).unwrap();
        conn.execute("INSERT INTO butler_task_definition VALUES(?1,8)", [id]).unwrap();
    }
    let rows = read_activity(&conn, 1).unwrap();
    assert_eq!(rows.len(), 123);
    assert!(!rows.iter().any(|r| r.conversation_id == 5 || r.conversation_id == 6));
    assert_eq!(rows.iter().find(|r| r.conversation_id == 7).unwrap().assistant_id, 8);
    assert_eq!(rows.iter().find(|r| r.conversation_id == 1).unwrap().assistant_id, 0);
    assert!(rows.iter().find(|r| r.conversation_id == 3).unwrap().waiting_peer);
    assert!(rows.iter().filter(|r| r.conversation_id >= 10).all(|r| r.assistant_id == 8 && !r.waiting_peer));
    assert!(!rows.iter().any(|r| r.conversation_id == 2 || r.conversation_id == 4));
}

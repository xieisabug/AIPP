use super::*;
use serde_json::Value;

fn task_fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT,butler_task_status TEXT,butler_task_finalized_at TEXT,butler_task_summary TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,created_time TEXT,content TEXT);
        CREATE TABLE butler_task_definition(id INTEGER PRIMARY KEY,task_conversation_id INTEGER,butler_conversation_id INTEGER,executor_assistant_id INTEGER,goal TEXT,created_time TEXT,result_handling_mode TEXT);
        CREATE TABLE butler_task_result(task_conversation_id INTEGER,created_time TEXT,structured_output_json TEXT,summary TEXT,final_message_id INTEGER,handoff_message_id INTEGER);
        INSERT INTO conversation VALUES(10,1,'butler_main',NULL,NULL,NULL),(20,2,'butler_contact',NULL,NULL,NULL),(30,2,'butler_task','running',NULL,NULL),(40,3,'butler_contact',NULL,NULL,NULL);
        INSERT INTO message VALUES(1,10,'2026-11-05 01:00:00','请让助手问早安');
        INSERT INTO butler_task_definition VALUES(1,30,10,2,'问候用户','2026-11-05 01:00:01',NULL);
    ").unwrap();
    conn
}

fn posts(conn: &Connection, conversation: i64) -> Value {
    let names = HashMap::from([(1,"总管家".into()),(2,"问候助手".into())]);
    serde_json::to_value(task_posts(conn, conversation, &names).unwrap()).unwrap()
}

#[test]
fn test_butler_task_timeline_records_request_then_actual_executor_reply() {
    let conn = task_fixture();
    let outgoing = posts(&conn,10);
    assert_eq!(outgoing.as_array().unwrap().len(),1);
    assert_eq!(outgoing[0]["text"],"问候用户");
    assert_eq!(outgoing[0]["outgoing"],true);
    assert_eq!(outgoing[0]["peer"],"问候助手");
    assert_eq!(outgoing[0]["anchor"],1);
    assert_eq!(posts(&conn,20)[0]["outgoing"],false);
    assert!(posts(&conn,40).as_array().unwrap().is_empty());
    conn.execute_batch("
        INSERT INTO message VALUES(2,30,'2026-11-05 01:00:10','早安！祝你今天顺利。');
        INSERT INTO butler_task_result VALUES(30,'2026-11-05 01:00:11',NULL,'短摘要',2,NULL);
        UPDATE conversation SET butler_task_status='succeeded' WHERE id=30;
    ").unwrap();
    for conv in [10,20] {
        let result = posts(&conn,conv);
        assert_eq!(result.as_array().unwrap().len(),2);
        assert_eq!(result[1]["text"],"早安！祝你今天顺利。");
        assert_eq!(result[1]["task_reply"],true);
        assert_eq!(result[1]["outgoing"],conv==20);
    }
}

#[test]
fn test_butler_task_timeline_reports_auth_failure_without_inventing_reply() {
    let conn = task_fixture();
    conn.execute_batch("
        UPDATE conversation SET butler_task_status='failed',butler_task_finalized_at='2026-11-05 01:00:10',
            butler_task_summary='任务 #30 启动失败：401 Unauthorized' WHERE id=30;
    ").unwrap();
    let result = posts(&conn,10);
    assert_eq!(result[1]["status"],"failed");
    assert_eq!(result[1]["text"],"");
    assert!(result[1]["error"].as_str().unwrap().contains("401 Unauthorized"));
    assert_eq!(result[1]["peer"],"问候助手");
    assert!(result[0]["error"].is_null(), "terminal error is shown once on the result event");
}

#[test]
fn test_butler_task_timeline_marks_direct_report_only_in_owner_view() {
    let conn = task_fixture();
    conn.execute_batch("
        UPDATE butler_task_definition SET result_handling_mode='direct_report' WHERE id=1;
        INSERT INTO message VALUES(2,30,'2026-11-05 01:00:10','核查结论：官网未发布 6.2');
        INSERT INTO butler_task_result VALUES(30,'2026-11-05 01:00:11',NULL,'短摘要',2,NULL);
        UPDATE conversation SET butler_task_status='succeeded' WHERE id=30;
    ").unwrap();
    let owner = posts(&conn,10);
    assert_eq!(owner[0]["direct"],false);
    assert_eq!(owner[1]["direct"],true);
    assert_eq!(owner[1]["text"],"核查结论：官网未发布 6.2");
    assert_eq!(posts(&conn,20)[1]["direct"],false);
}

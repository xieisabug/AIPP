use super::*;

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,name TEXT,conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,
            metadata_json TEXT,created_time TEXT DEFAULT CURRENT_TIMESTAMP);
        INSERT INTO conversation VALUES (10,1,'总管家主会话','butler_main'),(11,1,'总管家主会话（归档）','butler_main'),
            (20,2,'写作','butler_contact'),(30,2,'整理周报','butler_task'),(50,2,'普通聊天','normal');",
    )
    .unwrap();
    conn
}

fn message(conn: &Connection, conv: i64, kind: &str, text: &str, metadata: Option<&str>) -> i64 {
    conn.execute(
        "INSERT INTO message(conversation_id,message_type,content,metadata_json) VALUES (?1,?2,?3,?4)",
        params![conv, kind, text, metadata],
    )
    .unwrap();
    conn.last_insert_rowid()
}

#[test]
fn test_ui_search_only_returns_public_butler_messages() {
    let conn = database();
    let main = message(&conn, 10, "user", "周报 主会话", None);
    let archived = message(&conn, 11, "response", "旧的周报", None);
    let contact = message(&conn, 20, "response", "周报初稿", None);
    let task = message(&conn, 30, "response", "周报任务结果", None);
    message(&conn, 50, "user", "普通聊天里的周报", None);
    message(&conn, 20, "reasoning", "周报思考过程", None);
    message(&conn, 20, "user", "周报内部回调", Some(r#"{"origin":"internal"}"#));
    let peer = message(&conn, 20, "user", "帮我看看周报", Some(r#"{"origin":"butler_peer","sender_bot":"审稿"}"#));
    let names = HashMap::from([(1, "管家".to_string()), (2, "写作".to_string())]);

    let hits = ui_search(&conn, &names, "周报", 50, 0).unwrap();
    let ids: Vec<i64> = hits.iter().map(|h| h["message_id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![peer, task, contact, archived, main]);
    assert_eq!(hits[0]["speaker"], "审稿");
    assert_eq!(hits[1]["conversation_kind"], "butler_task");
    assert_eq!(hits[2]["bot_id"], 2);
    assert_eq!(hits[4]["speaker"], "用户");
    assert_eq!(hits[4]["text"], "周报 主会话");
}

#[test]
fn test_ui_search_paginates_case_insensitively() {
    let conn = database();
    for i in 0..5 {
        message(&conn, 20, "response", &format!("Release {i}"), None);
    }
    let names = HashMap::new();
    let first = ui_search(&conn, &names, "rElEaSe", 3, 0).unwrap();
    let second = ui_search(&conn, &names, "release", 3, 3).unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(second.len(), 2);
    assert_eq!(second[1]["text"], "Release 0");
    assert_eq!(first[0]["bot_name"], "写作", "contact falls back to conversation title");
}

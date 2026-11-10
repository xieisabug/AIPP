use super::*;

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,name TEXT,conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,
            metadata_json TEXT,created_time TEXT DEFAULT CURRENT_TIMESTAMP);
        INSERT INTO conversation VALUES (10,1,'总管家主会话','butler_main'),(11,1,'总管家主会话（归档）','butler_main'),
            (20,2,'写作','butler_contact'),(21,2,'与写作的私聊（存档 2026-11-01 08:00:00）','butler_contact_archive'),
            (30,2,'整理周报','butler_task'),(50,2,'普通聊天','normal');
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER);
        INSERT INTO butler_main_state VALUES (10);",
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

#[test]
fn test_search_marks_archived_contact_and_main_history() {
    let conn = database();
    let current = message(&conn, 20, "response", "预算 新私聊", None);
    let old_contact = message(&conn, 21, "user", "预算 旧私聊", None);
    let old_main = message(&conn, 11, "response", "预算 旧主会话", None);
    let main = message(&conn, 10, "response", "预算 当前主会话", None);
    let names = HashMap::from([(1, "管家".to_string()), (2, "写作".to_string())]);

    let page = search(&conn, &names, "预算", None, None).unwrap();
    let archived: HashMap<i64, bool> = page["matches"].as_array().unwrap().iter()
        .map(|h| (h["message_id"].as_i64().unwrap(), h["archived"].as_bool().unwrap())).collect();
    assert_eq!(archived, HashMap::from([(current, false), (old_contact, true), (old_main, true), (main, false)]));

    // 按助手过滤时，同一助手的归档私聊仍然能找到，并带归档会话名。
    let own = search(&conn, &names, "旧私聊", Some(2), None).unwrap();
    assert_eq!(own["matches"][0]["conversation_kind"], "butler_contact_archive");
    assert_eq!(own["matches"][0]["bot_name"], "写作");
    assert_eq!(own["matches"][0]["conversation_name"], "与写作的私聊（存档 2026-11-01 08:00:00）");
    let around = context(&conn, &names, old_contact, None).unwrap();
    assert_eq!(around["messages"][0]["archived"], true);
}

use super::*;

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,name TEXT,conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,
            metadata_json TEXT,created_time TEXT DEFAULT CURRENT_TIMESTAMP);
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER);
        CREATE TABLE butler_peer_exchange(id INTEGER PRIMARY KEY,state TEXT);
        CREATE TABLE butler_peer_post(id INTEGER PRIMARY KEY,exchange_id INTEGER,conversation_id INTEGER,
            leg TEXT,outgoing INTEGER,anchor INTEGER);
        CREATE TABLE butler_task_result(task_conversation_id INTEGER,final_message_id INTEGER);
        CREATE TABLE scheduled_task(id INTEGER PRIMARY KEY,name TEXT,butler_mode INTEGER);
        CREATE TABLE scheduled_task_run(id INTEGER PRIMARY KEY,task_id INTEGER,run_id TEXT,
            started_time TEXT,conversation_id INTEGER,final_message_id INTEGER);
        INSERT INTO conversation VALUES (10,1,'总管家','butler_main'),(11,1,'总管家（归档）','butler_main'),
            (20,2,'写作','butler_contact'),(30,2,'整理周报','butler_task'),(40,3,'群聊成员','butler_group_member'),
            (50,2,'普通聊天','normal'),(60,4,'计划任务: 日报','scheduled_run'),(61,4,'计划任务: 日报','normal'),
            (62,4,'计划任务: 个人','normal');
        INSERT INTO butler_main_state VALUES (10);
        INSERT INTO scheduled_task VALUES (1,'日报',1),(2,'个人',0);",
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

fn previewable(conn: &Connection, id: i64) -> Result<PreviewRow, String> {
    let row = load(conn, id).unwrap().ok_or_else(|| format!("消息 #{id} 已删除"))?;
    ensure_previewable(&row).map(|_| row)
}

#[test]
fn test_message_ref_parse_accepts_id_or_work_ref() {
    assert_eq!(MessageRef::parse(Some(7), None), Ok(MessageRef::Message(7)));
    assert_eq!(MessageRef::parse(None, Some("exchange:5")), Ok(MessageRef::Exchange(5)));
    assert_eq!(MessageRef::parse(None, Some(" task:12 ")), Ok(MessageRef::Task(12)));
    assert_eq!(MessageRef::parse(None, Some("message:9")), Ok(MessageRef::Message(9)));
    assert_eq!(
        MessageRef::parse(None, Some("scheduled_run:7f1c-uuid")),
        Ok(MessageRef::ScheduledRun("7f1c-uuid".into()))
    );
}

#[test]
fn test_message_ref_parse_rejects_ambiguous_or_malformed_input() {
    assert!(MessageRef::parse(Some(1), Some("task:2")).unwrap_err().contains("只能填一个"));
    assert!(MessageRef::parse(None, None).unwrap_err().contains("work_ref"));
    assert!(MessageRef::parse(Some(0), None).is_err());
    for bad in ["turn:3", "exchange:abc", "task:-1", "scheduled_run:", "12"] {
        assert!(MessageRef::parse(None, Some(bad)).is_err(), "{bad} 应被拒绝");
    }
}

#[test]
fn test_resolve_exchange_uses_recorded_reply_anchor() {
    let conn = database();
    let reply = message(&conn, 20, "response", "审稿意见", None);
    conn.execute_batch(&format!(
        "INSERT INTO butler_peer_exchange VALUES (5,'delivered'),(6,'waiting');
         INSERT INTO butler_peer_post(exchange_id,conversation_id,leg,outgoing,anchor) VALUES
            (5,20,'reply',1,{reply}),(5,10,'reply',0,999),(6,10,'request',1,998);"
    ))
    .unwrap();
    assert_eq!(resolve(&conn, &MessageRef::Exchange(5)), Ok(reply));
    assert!(resolve(&conn, &MessageRef::Exchange(6)).unwrap_err().contains("还没有可展示的答复（状态 waiting）"));
    assert!(resolve(&conn, &MessageRef::Exchange(7)).unwrap_err().contains("不存在"));
}

#[test]
fn test_resolve_task_and_scheduled_run_use_final_message() {
    let conn = database();
    let task_final = message(&conn, 30, "response", "周报已整理", None);
    let run_final = message(&conn, 60, "response", "今日日报", None);
    conn.execute_batch(&format!(
        "INSERT INTO butler_task_result VALUES (30,{task_final}),(31,NULL);
         INSERT INTO scheduled_task_run(task_id,run_id,started_time,conversation_id,final_message_id) VALUES
            (1,'run-ok','2026-11-12 08:00',60,{run_final}),(1,'run-failed','2026-11-12 09:00',NULL,NULL);"
    ))
    .unwrap();
    assert_eq!(resolve(&conn, &MessageRef::Task(30)), Ok(task_final));
    assert!(resolve(&conn, &MessageRef::Task(31)).unwrap_err().contains("还没有最终答复"));
    assert_eq!(resolve(&conn, &MessageRef::ScheduledRun("run-ok".into())), Ok(run_final));
    assert!(resolve(&conn, &MessageRef::ScheduledRun("run-failed".into())).unwrap_err().contains("没有保留可展示的产出"));
    assert!(resolve(&conn, &MessageRef::ScheduledRun("missing".into())).unwrap_err().contains("不存在"));
}

#[test]
fn test_previewable_only_within_butler_scope() {
    let conn = database();
    let contact = message(&conn, 20, "response", "初稿", None);
    let task = message(&conn, 30, "assistant", "任务结果", None);
    let archived = message(&conn, 11, "response", "旧主会话", None);
    let hidden_run = message(&conn, 60, "response", "隐藏运行产出", None);
    let shown_run = message(&conn, 61, "response", "已通知的运行产出", None);
    conn.execute_batch(&format!(
        "INSERT INTO scheduled_task_run(task_id,run_id,started_time,conversation_id) VALUES
            (1,'a','2026-11-12',60),(1,'b','2026-11-12',61),(2,'c','2026-11-12',62);"
    ))
    .unwrap();
    let personal_run = message(&conn, 62, "response", "非管家定时任务", None);
    let group = message(&conn, 40, "response", "群聊发言", None);
    let normal = message(&conn, 50, "response", "普通聊天", None);

    assert_eq!(previewable(&conn, contact).unwrap().conversation_kind, "butler_contact");
    assert!(previewable(&conn, task).is_ok());
    assert!(previewable(&conn, archived).unwrap().archived, "非当前主会话标为归档");
    assert_eq!(previewable(&conn, hidden_run).unwrap().scheduled_task_name.as_deref(), Some("日报"));
    assert_eq!(previewable(&conn, shown_run).unwrap().scheduled_task_name.as_deref(), Some("日报"));
    for (id, label) in [(personal_run, "非管家定时任务"), (group, "群聊"), (normal, "普通聊天")] {
        assert!(previewable(&conn, id).unwrap_err().contains("不在管家会话"), "{label} 不能展示");
    }
}

#[test]
fn test_previewable_rejects_non_reply_messages() {
    let conn = database();
    let user = message(&conn, 20, "user", "用户原话", None);
    let tool = message(&conn, 20, "tool_result", "Tool execution completed", None);
    let reasoning = message(&conn, 20, "reasoning", "思考", None);
    let internal = message(&conn, 10, "response", "内部回合", Some(r#"{"origin":"internal"}"#));
    for id in [user, tool, reasoning] {
        assert!(previewable(&conn, id).unwrap_err().contains("只能展示助手的回复"));
    }
    assert!(previewable(&conn, internal).unwrap_err().contains("内部回合"));
    assert!(previewable(&conn, 9999).unwrap_err().contains("已删除"));
}

#[test]
fn test_scheduled_run_retention_keeps_newest_hidden_runs_only() {
    let conn = database();
    conn.execute_batch(
        "INSERT INTO conversation VALUES (70,4,'r1','scheduled_run'),(71,4,'r2','scheduled_run'),(72,4,'r3','scheduled_run');
         INSERT INTO scheduled_task_run(task_id,run_id,started_time,conversation_id) VALUES
            (1,'r1','2026-11-10',70),(1,'r2','2026-11-11',71),(1,'r3','2026-11-12',72),
            (1,'shown','2026-11-09',61),(2,'other','2026-11-01',62);",
    )
    .unwrap();
    assert_eq!(scheduled_run_conversation_ids_beyond(&conn, 1, 2).unwrap(), vec![70]);
    assert_eq!(scheduled_run_conversation_ids_beyond(&conn, 1, 0).unwrap(), vec![72, 71, 70]);
    assert!(scheduled_run_conversation_ids_beyond(&conn, 2, 0).unwrap().is_empty(), "用户可见的运行会话不参与清理");
}

use super::*;
use crate::db::butler_memory as memories;

#[test]
fn test_proactive_confirmation_binds_displayed_body_on_equal_revision() {
    let shown = (Some(8), 3, Some("提醒我复查"));
    assert!(!proposal::confirmation_matches(shown, (8, 3, "替我预约复查")));
    assert!(!proposal::confirmation_matches(shown, (9, 3, "提醒我复查")));
    assert!(proposal::confirmation_matches(shown, (8, 3, "提醒我复查")));
}

#[test]
fn test_proactive_config_migrates_legacy_fields_and_ignores_removed_keys() {
    let legacy = r#"{
        "enabled": true,
        "private_chats": true,
        "groups": false,
        "normal_chats": true,
        "remember": false,
        "research": false,
        "reminders": false,
        "feishu_open_id": "ou_legacy_123",
        "pulse_hours": 2,
        "daily_limit": 5,
        "quiet_start": "22:30",
        "quiet_end": "08:00",
        "release": "08:30",
        "digest": "23:00",
        "small_model": "m1%%1",
        "deep_model": "assistant:42",
        "adaptive_window": true
    }"#;
    let cfg: Config = serde_json::from_str(legacy).unwrap();
    assert!(cfg.enabled);
    assert!(!cfg.allow_notifications, "legacy reminders=false should map to allow_notifications=false");
    assert_eq!(cfg.deep_model, "assistant:42");
    assert_eq!(agent::parse_native_assistant_id(&cfg.deep_model), Some(42));
    assert_eq!(agent::parse_native_assistant_id("gpt-4o%%1"), None);
    assert!(cfg.validate().is_ok());
}

#[test]
fn test_proactive_prompt_injects_custom_instructions_and_quiet_mode() {
    let cfg = Config {
        allow_notifications: false,
        custom_instructions: "重点关注项目截止日和未完成承诺".into(),
        ..Config::default()
    };
    let sys = prompt::system_prompt(&cfg, "digest");
    assert!(sys.contains("静默模式"));
    assert!(sys.contains("重点关注项目截止日和未完成承诺"));
    let triage = prompt::light_triage_prompt(&cfg);
    assert!(triage.contains("重点关注项目截止日和未完成承诺"));
}

pub(super) fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT, assistant_id INTEGER,conversation_kind TEXT, is_hidden_from_normal_chat_list INTEGER DEFAULT 0,updated_time TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT, content TEXT, metadata_json TEXT,created_time TEXT DEFAULT CURRENT_TIMESTAMP,
            llm_model_id INTEGER,llm_model_name TEXT, token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,
            parent_id INTEGER,start_time TEXT,finish_time TEXT);
        CREATE TABLE butler_work_followup(execution_input_id INTEGER);
        CREATE TABLE scheduled_task(assistant_id INTEGER,is_enabled INTEGER,name TEXT,task_prompt TEXT);
        CREATE TABLE conversation_summary(conversation_id INTEGER);
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER);
        CREATE TABLE butler_chat_group_message(id INTEGER PRIMARY KEY,group_id INTEGER,seq INTEGER,sender_member INTEGER,kind TEXT,body TEXT,created_at TEXT DEFAULT CURRENT_TIMESTAMP);
        CREATE TABLE butler_read_reply(message_id INTEGER PRIMARY KEY);
        INSERT INTO conversation VALUES(10,'联系人',8,'butler_contact',1,NULL),(11,'普通',9,'normal',0,NULL),(12,'群',NULL,'butler_group',1,NULL);
        INSERT INTO message(id,conversation_id,message_type,content) VALUES(1,10,'user','我喜欢喝茶'),(2,11,'user','普通聊天内容'),(3,10,'response','我的推断');
        INSERT INTO message(id,conversation_id,message_type,content,metadata_json) VALUES(4,10,'user','后台转述','{\"origin\":\"butler_peer\"}');
        INSERT INTO butler_chat_group_message(group_id,seq,kind,body) VALUES(12,1,'message','群消息');").unwrap();
    memories::schema(&db).unwrap();
    db.execute_batch(include_str!("schema.sql")).unwrap();
    db
}

#[test]
fn test_proactive_cursor_isolation_and_scope() {
    let db = db();
    let cfg = Config::default();
    let rows = delta::read(&db, &cfg, false).unwrap();
    assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["我喜欢喝茶", "群消息"]);
    delta::advance(&db, &rows, false).unwrap();
    assert!(delta::read(&db, &cfg, false).unwrap().is_empty());
    let digest = delta::read(&db, &cfg, true).unwrap();
    assert_eq!(digest.len(), 2);
    delta::advance(&db, &digest, true).unwrap();
    assert!(delta::read(&db, &cfg, true).unwrap().is_empty());
    assert_eq!(
        delta::read(&db, &Config { normal_chats: true, ..cfg }, true).unwrap()[0].text,
        "普通聊天内容"
    );
}

#[test]
fn test_proactive_oversized_source_resumes_exactly_after_unicode_excerpt() {
    let db = db();
    let message = format!("{}周五复查", "好".repeat(16000));
    db.execute("UPDATE message SET content=?1 WHERE id=1", [message]).unwrap();
    let cfg = Config::default();
    let first = delta::read(&db, &cfg, true).unwrap();
    assert_eq!(first[0].text.chars().count(), 16000);
    delta::advance(&db, &first, true).unwrap();
    let second = delta::read(&db, &cfg, true).unwrap();
    assert_eq!(second[0].text, "周五复查");
    delta::verify(&db, &second[0]).unwrap();
    delta::advance(&db, &second, true).unwrap();
    assert_eq!(delta::pending(&db, &cfg, false).unwrap().0, 0);
}

#[test]
fn test_proactive_memory_undo_restores_source_and_respects_manual_change() {
    let db = db();
    let src = delta::read(&db, &Config::default(), true).unwrap().remove(0);
    let (old, _) =
        memories::remember(&db, memories::Scope::Assistant(8), "喜欢白开水", None, Some(8))
            .unwrap();
    memory::save(&db, &src, 8, "喜欢喝茶", Some((old.id, old.revision)), false).unwrap();
    let updated = memories::get_any(&db, old.id).unwrap().unwrap();
    assert_eq!(updated.origin, "proactive");
    memory::undo(&db, old.id, updated.revision).unwrap();
    let restored = memories::get_any(&db, old.id).unwrap().unwrap();
    assert_eq!(
        (restored.content.as_str(), restored.source_message_id, restored.origin.as_str()),
        ("喜欢白开水", None, "user_turn")
    );
    memory::save(&db, &src, 8, "喜欢喝茶", Some((old.id, restored.revision)), false).unwrap();
    let changed = memories::update(&db, None, old.id, None, "用户亲自修改", None).unwrap();
    assert!(memory::undo(&db, old.id, changed.revision).is_err());
    assert!(memory::save(&db, &src, 8, "Bearer abcdefgh", None, false).is_err());
    let group = delta::read(&db, &Config::default(), true).unwrap().pop().unwrap();
    assert!(memory::save(&db, &group, 8, "共享到其他对话", None, true).is_err());
    let fake = delta::Source { position: 3, ..src };
    assert!(memory::save(&db, &fake, 8, "助手推断", None, false).is_err());
}

#[test]
fn test_proactive_acceptance_commits_one_receipt() {
    let mut db = db();
    db.execute_batch("INSERT INTO butler_proactive_item(uuid,kind,title,action_prompt,source_conversation_id,source_position,updated_at) VALUES('x','proposal','建任务','建任务',10,1,0);
        INSERT INTO butler_proactive_acceptance(uuid,conversation_id,assistant_id,prompt,accepted_at,status) VALUES('x',10,8,'建任务',0,'pending')").unwrap();
    assert!(proposal::write_input(&mut db, "x", 10, 9, "建任务", 1, "m").is_err());
    assert!(proposal::write_input(&mut db, "x", 10, 8, "另一条指令", 1, "m").is_err());
    let id = proposal::write_input(&mut db, "x", 10, 8, "建任务", 1, "m").unwrap();
    assert!(proposal::write_input(&mut db, "x", 10, 8, "建任务", 1, "m").is_err());
    let receipt: i64 = db
        .query_row(
            "SELECT message_id FROM butler_proactive_acceptance WHERE status='sent'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(receipt, id);
}

#[tokio::test]
async fn test_proactive_recovery_skips_busy_conversation_without_reordering_its_proposals() {
    let mut db = db();
    db.execute("UPDATE conversation SET conversation_kind='butler_contact',is_hidden_from_normal_chat_list=1 WHERE id=11", []).unwrap();
    for (key, conversation, assistant, time) in [
        ("a", 10, 8, 0), ("b", 10, 8, 1), ("d", 11, 9, 2), ("c", 11, 9, 2),
    ] {
        db.execute(
            "INSERT INTO butler_proactive_item(uuid,kind,title,action_prompt,source_conversation_id,source_position,updated_at)
             VALUES(?1,'proposal',?1,?1,?2,1,0)",
            rusqlite::params![key, conversation],
        ).unwrap();
        db.execute(
            "INSERT INTO butler_proactive_acceptance(uuid,conversation_id,assistant_id,prompt,accepted_at,status)
             VALUES(?1,?2,?3,?1,?4,'pending')",
            rusqlite::params![key, conversation, assistant, time],
        ).unwrap();
    }
    let heads = proposal::pending_conversation_heads(&db).unwrap();
    assert_eq!(heads, vec![("a".to_string(), 10), ("c".to_string(), 11)]);
    assert_eq!(proposal::first_idle_head(heads.clone(), |cid| async move { cid == 10 }).await.as_deref(), Some("c"));
    assert!(proposal::first_idle_head(heads, |_| async { true }).await.is_none());
    let sent = proposal::write_input(&mut db, "c", 11, 9, "c", 1, "m").unwrap();
    assert!(proposal::write_input(&mut db, "c", 11, 9, "c", 1, "m").is_err());
    let heads = proposal::pending_conversation_heads(&db).unwrap();
    assert_eq!(heads, vec![("a".to_string(), 10), ("d".to_string(), 11)]);
    assert_eq!(proposal::first_idle_head(heads, |_| async { false }).await.as_deref(), Some("a"));
    assert!(sent > 0);
}

#[test]
fn test_proactive_quiet_window_and_budget_independent_digest() {
    let cfg = Config::default();
    assert!(cfg.quiet("23:00"));
    assert!(!cfg.quiet("08:00"));
    assert!("08:29" < cfg.release.as_str());
    assert!(cfg.validate().is_ok());
}

#[test]
fn test_proactive_retention_keeps_only_final_brief_at_original_message_id() {
    let mut db = db();
    for day in 1..=31 {
        db.execute("INSERT INTO conversation(name,conversation_kind) VALUES('过程','butler_proactive_run')",[]).unwrap();
        let session = db.last_insert_rowid();
        db.execute("INSERT INTO butler_proactive_run(id,trigger_kind,status,conversation_id,started_at) VALUES(?1,'digest','acted',?2,?1)",params![day,session]).unwrap();
        if day == 1 {
            db.execute("INSERT INTO message(id,conversation_id,message_type,content) VALUES(71,?1,'response','工具过程'),(72,?1,'response','最终简报')",[session]).unwrap();
            db.execute("INSERT INTO butler_proactive_item(uuid,kind,title,source_conversation_id,prep_message_id,updated_at) VALUES('x','prep_topic','出行',10,72,0)",[]).unwrap();
        }
    }
    runner::cleanup(&mut db).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM conversation WHERE conversation_kind='butler_proactive_run'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 30);
    assert_eq!(
        db.query_row("SELECT count(*) FROM message WHERE id=71", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT c.conversation_kind FROM message m JOIN conversation c ON c.id=m.conversation_id WHERE m.id=72",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap(),
        "butler_proactive_brief"
    );
}

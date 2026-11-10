use super::*;
use crate::db::butler_memory as memory;

/// 小A（助手 2）有私聊 20、派工任务 21，并在项目群 50 里（成员 1，会话 51）；小B（助手 3）有私聊 30，群里是成员 2（会话 52）。
fn fixture() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("
        CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT, assistant_id INTEGER, conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id INTEGER, parent_id INTEGER,
            message_type TEXT, content TEXT, metadata_json TEXT);
        INSERT INTO conversation VALUES
            (10,'总管家',1,'butler_main'),(20,'与小A的私聊',2,'butler_contact'),(21,'小A的任务',2,'butler_task'),
            (30,'与小B的私聊',3,'butler_contact'),(40,'普通聊天',2,'normal'),(50,'项目群',NULL,'butler_group'),
            (51,'小A在项目群',2,'butler_group_member'),(52,'小B在项目群',3,'butler_group_member');
        INSERT INTO message(id,conversation_id,parent_id,message_type,content,metadata_json) VALUES
            (1,20,NULL,'user','记住我喜欢中文',NULL),(2,20,1,'response','好的',NULL),
            (3,30,NULL,'user','你好',NULL),(4,30,3,'response','你好',NULL),
            (5,21,NULL,'user','派工内容',NULL),(6,21,5,'response','收到',NULL),
            (7,40,NULL,'user','普通',NULL),(8,40,7,'response','普通回复',NULL),
            (60,51,NULL,'user','我：大家记住周五交付','{\"origin\":\"butler_group\",\"group_id\":50,\"member_id\":1,\"turn_id\":100}'),
            (61,51,60,'response','收到',NULL),
            (70,52,NULL,'user','小A：@小B 记一下','{\"origin\":\"butler_group\",\"group_id\":50,\"member_id\":2,\"turn_id\":101}'),
            (71,52,70,'response','好',NULL);
    ").unwrap();
    memory::schema(&c).unwrap();
    crate::db::butler_group::schema(&c).unwrap();
    crate::db::butler_work::schema(&c).unwrap();
    c.execute_batch("
        INSERT INTO butler_chat_group(id,name,seq) VALUES(50,'项目群',2);
        INSERT INTO butler_chat_group_member(id,group_id,assistant_id,name,conversation_id,joined_after,delivered_through,active)
            VALUES(1,50,2,'小A',51,0,1,1),(2,50,3,'小B',52,0,2,1);
        INSERT INTO butler_chat_group_message(group_id,seq,sender_member,sender_assistant,sender_name,kind,body)
            VALUES(50,1,NULL,NULL,'我','message','大家记住周五交付'),(50,2,1,2,'小A','message','@小B 记一下');
        INSERT INTO butler_chat_group_turn(id,group_id,member_id,trigger_seq,origin_seq,depth,state,input_through,input_message_id)
            VALUES(100,50,1,1,1,0,'running',1,60),(101,50,2,2,1,1,'running',2,70);
    ").unwrap();
    c
}

fn run(c: &mut Connection, conversation: i64, anchor: Option<i64>, args: Value) -> Result<Value> {
    execute(c, conversation, anchor, &args)
}

fn remember_in(c: &mut Connection, conversation: i64, anchor: i64, content: &str) -> Value {
    run(c, conversation, Some(anchor), json!({"action":"remember","content":content})).unwrap()
}

fn recall_ids(c: &mut Connection, conversation: i64) -> Vec<i64> {
    run(c, conversation, None, json!({"action":"recall"})).unwrap()["memories"].as_array().unwrap()
        .iter().map(|m| m["id"].as_i64().unwrap()).collect()
}

fn memory_count(c: &Connection) -> i64 {
    c.query_row("SELECT count(*) FROM butler_memory", [], |r| r.get(0)).unwrap()
}

#[test]
fn test_memory_api_private_memory_follows_assistant_across_conversations() {
    let mut c = fixture();
    let saved = remember_in(&mut c, 20, 2, "回复使用中文");
    assert_eq!(saved["status"], "saved");
    assert_eq!(saved["memory"]["scope"], "assistant");
    let id = saved["memory"]["id"].as_i64().unwrap();
    assert_eq!(memory::get_any(&c, id).unwrap().unwrap().source(), Some(Source::Message(1)));
    assert_eq!(recall_ids(&mut c, 21), vec![id], "same assistant's task sees it");
    assert!(recall_ids(&mut c, 30).is_empty(), "another assistant does not");
    assert!(recall_ids(&mut c, 51).is_empty(), "private memory never enters a group");
    assert_eq!(remember_in(&mut c, 20, 2, "回复使用中文")["status"], "already_exists");
}

#[test]
fn test_memory_api_update_and_forget_by_id_with_optional_revision() {
    let mut c = fixture();
    let id = remember_in(&mut c, 20, 2, "回复使用中文")["memory"]["id"].as_i64().unwrap();
    let updated = run(&mut c, 20, Some(2), json!({"action":"update","id":id,"content":"回复简短"})).unwrap();
    assert_eq!(updated["memory"]["revision"], 2);
    assert!(run(&mut c, 20, Some(2), json!({"action":"forget","id":id,"revision":1})).is_err());
    assert!(run(&mut c, 30, Some(4), json!({"action":"update","id":id,"content":"越权"})).is_err());
    assert_eq!(run(&mut c, 30, Some(4), json!({"action":"forget","id":id})).unwrap()["status"], "not_found");
    assert_eq!(run(&mut c, 20, Some(2), json!({"action":"forget","id":id,"revision":2})).unwrap()["status"], "forgotten");
    assert_eq!(memory_count(&c), 0);
}

#[test]
fn test_memory_api_global_memory_is_shared_but_private_source_is_not() {
    let mut c = fixture();
    let saved = run(&mut c, 20, Some(2), json!({"action":"remember","scope":"global","content":"所有助手都用中文"})).unwrap();
    let id = saved["memory"]["id"].as_i64().unwrap();
    assert_eq!(recall_ids(&mut c, 30), vec![id]);
    assert_eq!(recall_ids(&mut c, 52), vec![id], "global memory reaches group members");
    assert_eq!(run(&mut c, 30, None, json!({"action":"source","id":id})).unwrap()["status"], "unavailable");
    let own = run(&mut c, 20, None, json!({"action":"source","id":id})).unwrap();
    assert_eq!((own["status"].as_str(), own["text"].as_str()), (Some("available"), Some("记住我喜欢中文")));
}

#[test]
fn test_memory_api_rejects_turns_not_sent_by_the_user() {
    for meta in [r#"{"origin":"internal"}"#, r#"{"origin":"butler_peer","leg":"request"}"#, r#"{"relayOrigin":"x"}"#] {
        let mut c = fixture();
        c.execute("UPDATE message SET metadata_json=?1 WHERE id=1", [meta]).unwrap();
        assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"不应保存"})).is_err(), "{meta}");
    }
    // continue_work 送进运行中任务的要求没有 origin 标记，要从续接记录识别。
    let mut c = fixture();
    c.execute_batch("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(9,21,NULL,'user','总管家转达的要求'),(10,21,9,'response','好');
        INSERT INTO butler_work_followup(id,work_ref,request_key,conversation_id,task_conversation_id,instruction,state,execution_task,execution_input_id)
            VALUES(1,'task:21','k',10,21,'记住这个','sending',21,9);").unwrap();
    let error = run(&mut c, 21, Some(10), json!({"action":"remember","content":"续接"})).unwrap_err().to_string();
    assert!(error.contains("续接"), "{error}");
    assert!(run(&mut c, 21, Some(6), json!({"action":"remember","content":"派工"})).unwrap_err().to_string().contains("派工"));
    assert_eq!(memory_count(&c), 0);
}

#[test]
fn test_memory_api_accepts_user_messages_in_task_after_dispatch() {
    let mut c = fixture();
    c.execute_batch("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(9,21,NULL,'user','记住输出用表格'),(10,21,9,'response','好');").unwrap();
    assert_eq!(remember_in(&mut c, 21, 10, "输出用表格")["status"], "saved");
}

#[test]
fn test_memory_api_older_turn_can_still_write_after_user_sends_more() {
    let mut c = fixture();
    // 普通模型允许运行中插队：用户又发了一条，上一轮里的“记住”仍然是用户本人的要求。
    c.execute("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(11,20,NULL,'user','另外一件事')", []).unwrap();
    assert_eq!(remember_in(&mut c, 20, 2, "回复使用中文")["status"], "saved");
}

#[test]
fn test_memory_api_requires_turn_identity_and_running_work() {
    let mut c = fixture();
    assert!(run(&mut c, 20, None, json!({"action":"remember","content":"无来源"})).is_err());
    assert!(run(&mut c, 20, Some(4), json!({"action":"remember","content":"别的会话"})).is_err());
    for action in ["pause", "cancel"] {
        let mut c = fixture();
        crate::db::butler_control::set(&c, 20, action).unwrap();
        assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"不得写入"})).is_err(), "{action}");
        assert_eq!(memory_count(&c), 0);
    }
}

#[test]
fn test_memory_api_unavailable_outside_butler_assistant_conversations() {
    let mut c = fixture();
    assert!(run(&mut c, 40, Some(8), json!({"action":"recall"})).is_err());
    assert!(run(&mut c, 50, None, json!({"action":"recall"})).is_err(), "the group itself is not an assistant");
    assert!(run(&mut c, 20, Some(2), json!({"action":"delete"})).is_err());
}

#[test]
fn test_memory_api_group_member_writes_shared_group_memory() {
    let mut c = fixture();
    let saved = remember_in(&mut c, 51, 61, "周五交付");
    assert_eq!(saved["memory"]["scope"], "group");
    let id = saved["memory"]["id"].as_i64().unwrap();
    let entry = memory::get_any(&c, id).unwrap().unwrap();
    assert_eq!((entry.scope_id, entry.source(), entry.author_assistant_id), (Some(50), Some(Source::GroupMessage { group: 50, seq: 1 }), Some(2)));
    assert_eq!(recall_ids(&mut c, 52), vec![id], "other members share group memory");
    assert!(recall_ids(&mut c, 20).is_empty(), "group memory stays in the group");
    let source = run(&mut c, 52, None, json!({"action":"source","id":id})).unwrap();
    assert_eq!(source["text"], "大家记住周五交付");
    assert_eq!(run(&mut c, 30, None, json!({"action":"source","id":id})).unwrap_err().to_string().contains("无权"), true);
}

#[test]
fn test_memory_api_group_rejects_relays_stopped_turns_and_private_scope() {
    let mut c = fixture();
    let error = run(&mut c, 52, Some(71), json!({"action":"remember","content":"接力"})).unwrap_err().to_string();
    assert!(error.contains("点名"), "{error}");
    assert!(run(&mut c, 51, Some(61), json!({"action":"remember","scope":"assistant","content":"私有"})).is_err());
    assert!(run(&mut c, 51, Some(61), json!({"action":"remember","content":"x","source_message_id":1})).is_err());
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","scope":"group","content":"x"})).is_err());
    c.execute("UPDATE butler_chat_group_turn SET state='cancelled' WHERE id=100", []).unwrap();
    assert!(run(&mut c, 51, Some(61), json!({"action":"remember","content":"迟到"})).is_err());
    c.execute("UPDATE butler_chat_group_member SET active=0 WHERE id=1", []).unwrap();
    assert!(run(&mut c, 51, None, json!({"action":"recall"})).is_err(), "removed members lose access");
    assert_eq!(memory_count(&c), 0);
}

#[test]
fn test_memory_api_explicit_source_must_be_own_direct_user_message() {
    let mut c = fixture();
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"x","source_message_id":3})).is_err());
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"x","source_message_id":2})).is_err());
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"x","source_message_id":5})).is_err(), "task dispatch is not a user source");
    c.execute("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(12,21,NULL,'user','以后周报用表格')", []).unwrap();
    let saved = run(&mut c, 20, Some(2), json!({"action":"remember","content":"周报用表格","source_message_id":12})).unwrap();
    let id = saved["memory"]["id"].as_i64().unwrap();
    assert_eq!(memory::get_any(&c, id).unwrap().unwrap().source(), Some(Source::Message(12)));
}

#[test]
fn test_memory_turn_prefix_lists_visible_memories_with_scope_tags() {
    let mut c = fixture();
    let own = remember_in(&mut c, 20, 2, "用户在做 AIPP 项目")["memory"]["id"].as_i64().unwrap();
    let global = run(&mut c, 20, Some(2), json!({"action":"remember","scope":"global","content":"回答使用中文"})).unwrap()["memory"]["id"].as_i64().unwrap();
    remember_in(&mut c, 51, 61, "周五交付");
    let prefix = turn_prefix(&c, 21, "AIPP 的进度").unwrap().unwrap();
    assert!(prefix.starts_with("<long_term_memory>"), "{prefix}");
    assert!(prefix.contains(&format!("- #{own} 用户在做 AIPP 项目")), "{prefix}");
    assert!(prefix.contains(&format!("- #{global} （全局）回答使用中文")), "{prefix}");
    assert!(!prefix.contains("周五交付"), "{prefix}");
    assert!(turn_prefix(&c, 52, "进度").unwrap().unwrap().contains("（本群）周五交付"));
    assert!(turn_prefix(&c, 40, "普通聊天").unwrap().is_none());
    assert!(turn_prefix(&c, 50, "群聊本身").unwrap().is_none());
    c.execute("DELETE FROM butler_memory", []).unwrap();
    assert!(turn_prefix(&c, 20, "任意").unwrap().is_none(), "no memories costs no tokens");
}

#[test]
fn test_memory_source_view_shows_label_and_deleted_state() {
    let mut c = fixture();
    let private = remember_in(&mut c, 20, 2, "回复使用中文")["memory"]["id"].as_i64().unwrap();
    let group = remember_in(&mut c, 51, 61, "周五交付")["memory"]["id"].as_i64().unwrap();
    let view = source_view(&c, private).unwrap();
    assert_eq!((view.status.as_str(), view.label.as_deref(), view.text.as_deref()), ("available", Some("与小A的私聊"), Some("记住我喜欢中文")));
    assert_eq!(source_view(&c, group).unwrap().label.as_deref(), Some("群聊「项目群」"));
    c.execute("DELETE FROM message WHERE id=1", []).unwrap();
    assert_eq!(source_view(&c, private).unwrap().status, "deleted");
    assert!(memory::get_any(&c, private).unwrap().is_some(), "deleting the chat keeps the memory");
}

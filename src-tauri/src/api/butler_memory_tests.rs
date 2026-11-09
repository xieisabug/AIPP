use super::*;
fn fixture() -> Connection {
    let c=Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT);
      CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT);
      INSERT INTO conversation VALUES(10,1,'butler_main'),(20,2,'butler_contact'),(21,2,'butler_contact'),(30,3,'butler_contact'),(40,2,'butler_task');
      INSERT INTO message VALUES(1,20,NULL,'user','记住我喜欢中文',NULL),(2,20,1,'response','工具调用',NULL),(3,30,NULL,'user','你好',NULL),(4,40,NULL,'user','派工',NULL),(5,21,NULL,'user','另一个窗口',NULL);").unwrap();
    memory::init(&c).unwrap();c
}
fn save(c:&mut Connection, scope:&str)->Value {
    execute(c,20,Some(2),&json!({"action":"remember","scope":scope,"content":"回复使用中文"})).unwrap()["memory"].clone()
}
#[test]
fn test_memory_api_remember_recall_update_forget_across_own_conversations() {
    let mut c=fixture();let entry=save(&mut c,"assistant");let id=entry["id"].as_i64().unwrap();
    assert_eq!(entry["source_message_id"],1);
    assert_eq!(execute(&mut c,21,None,&json!({"action":"recall","query":"中文"})).unwrap()["memories"].as_array().unwrap().len(),1);
    let changed=execute(&mut c,20,Some(2),&json!({"action":"update","id":id,"revision":1,"content":"回复简短"})).unwrap();
    assert_eq!(changed["memory"]["revision"],2);
    assert!(execute(&mut c,20,Some(2),&json!({"action":"forget","id":id,"revision":1})).is_err());
    assert_eq!(execute(&mut c,20,Some(2),&json!({"action":"forget","id":id,"revision":2})).unwrap()["deleted"],true);
    assert_eq!(c.query_row("SELECT count(*) FROM message",[],|r|r.get::<_,i64>(0)).unwrap(),5);
    assert!(execute(&mut c,20,Some(2),&json!({"action":"update","id":id,"revision":2,"content":"不能复活"})).is_err());
}
#[test]
fn test_memory_api_global_read_does_not_reveal_private_source() {
    let mut c=fixture();let entry=save(&mut c,"global");let id=entry["id"].as_i64().unwrap();
    assert_eq!(execute(&mut c,30,None,&json!({"action":"recall"})).unwrap()["memories"].as_array().unwrap().len(),1);
    assert_eq!(execute(&mut c,30,None,&json!({"action":"source","id":id})).unwrap()["status"],"unavailable");
    assert_eq!(execute(&mut c,20,None,&json!({"action":"source","id":id})).unwrap()["status"],"available");
}
#[test]
fn test_memory_api_private_get_update_forget_and_source_isolation() {
    let mut c=fixture();let entry=save(&mut c,"assistant");let id=entry["id"].as_i64().unwrap();
    assert!(execute(&mut c,30,None,&json!({"action":"get","id":id})).unwrap()["memory"].is_null());
    assert!(execute(&mut c,30,Some(3),&json!({"action":"update","id":id,"revision":1,"content":"不允许"})).is_err());
    assert_eq!(execute(&mut c,30,Some(3),&json!({"action":"forget","id":id,"revision":1})).unwrap()["deleted"],false);
    assert!(execute(&mut c,30,None,&json!({"action":"source","id":id})).is_err());
}
#[test]
fn test_memory_api_rejects_untrusted_and_stale_sources() {
    for meta in ["{\"origin\":\"butler_peer\"}","{\"relay_origin\":\"task\"}","{\"relayOrigin\":\"background\"}","{bad"] {
        let mut c=fixture();c.execute("UPDATE message SET metadata_json=?1 WHERE id=1",[meta]).unwrap();
        assert!(execute(&mut c,20,Some(2),&json!({"action":"remember","content":"错误"})).is_err());
    }
    let mut c=fixture();c.execute_batch("INSERT INTO message VALUES(6,20,NULL,'user','新回合',NULL)").unwrap();
    assert!(execute(&mut c,20,Some(2),&json!({"action":"remember","content":"旧回合"})).is_err());
    assert!(execute(&mut c,40,Some(4),&json!({"action":"remember","content":"派工"})).is_err());
    assert!(execute(&mut c,20,None,&json!({"action":"remember","content":"无来源"})).is_err());
}
#[test]
fn test_memory_api_pause_and_cancel_block_writes_after_async_preparation() {
    for action in ["pause","cancel"] {
        let mut c=fixture();crate::db::butler_control::set(&c,20,action).unwrap();
        assert!(execute(&mut c,20,Some(2),&json!({"action":"remember","content":"不得写入"})).is_err());
        assert_eq!(c.query_row("SELECT count(*) FROM butler_memory",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}
#[test]
fn test_memory_api_deleted_chat_retains_memory_but_source_is_unavailable() {
    let mut c=fixture();let entry=save(&mut c,"assistant");let id=entry["id"].as_i64().unwrap();
    c.execute("DELETE FROM message WHERE conversation_id=20",[]).unwrap();
    c.execute("DELETE FROM conversation WHERE id=20",[]).unwrap();
    assert_eq!(execute(&mut c,21,None,&json!({"action":"source","id":id})).unwrap()["status"],"deleted");
    assert_eq!(execute(&mut c,21,None,&json!({"action":"get","id":id})).unwrap()["memory"]["content"],"回复使用中文");
}
#[test]
fn test_memory_api_rejects_group_forged_identity_and_foreign_source() {
    let mut c=fixture();
    for args in [json!({"action":"remember","scope":"group","content":"x"}),json!({"action":"recall","assistant_id":3}),json!({"action":"remember","content":"x","source_message_id":3})] {
        assert!(execute(&mut c,20,Some(2),&args).is_err());
    }
    c.execute("UPDATE conversation SET conversation_kind='normal' WHERE id=20",[]).unwrap();
    assert!(execute(&mut c,20,Some(2),&json!({"action":"recall"})).is_err());
}

#[test]
fn test_memory_api_cancelled_runtime_token_does_not_write() {
    let mut c=fixture();let cancel=CancellationToken::new();cancel.cancel();
    assert!(execute_checked(&mut c,20,Some(2),&json!({"action":"remember","content":"不应保存"}),&cancel).is_err());
    assert_eq!(c.query_row("SELECT count(*) FROM butler_memory",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

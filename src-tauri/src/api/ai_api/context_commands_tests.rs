use super::*;
use rusqlite::Connection;

#[test]
fn test_context_commands_reload_and_update_preserve_contact_history() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("ATTACH DATABASE ':memory:' AS chat; CREATE TABLE chat.conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,conversation_kind TEXT);
        CREATE TABLE chat.message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,
        created_time TEXT,token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,metadata_json TEXT);
        INSERT INTO conversation VALUES(1,42,'butler_contact_archive'),(2,42,'butler_contact');
        INSERT INTO message(id,conversation_id,message_type,content,created_time) VALUES
        (1,2,'system','old','1'),(2,2,'user','hello','2'),(3,2,'system','<!-- compacted_ids: [] -->','3');").unwrap();
    assert_eq!(append_prompt_notice(&db,42,"中文").unwrap(),Some((2,4)));
    assert_eq!(append_prompt_notice(&db,42,"中文").unwrap(),None);
    assert_eq!(append_prompt_notice(&db,42,"").unwrap(),Some((2,5)));
    replace_system_head(&db,2,"new").unwrap();
    let texts = db.prepare("SELECT content FROM message ORDER BY id").unwrap()
        .query_map([], |r| r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert_eq!(&texts[..3], &["new","hello","<!-- compacted_ids: [] -->"]);
    assert!(texts[3].contains("中文") && texts[4].contains("撤销"));
    assert_eq!(db.query_row("SELECT json_extract(metadata_json,'$.context_reload_after') FROM message WHERE id=1",[],|r|r.get::<_,i64>(0)).unwrap(),5);
}

use super::*;
use crate::db::conversation_identity;

fn databases() -> (Connection, Connection) {
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT);").unwrap();
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    (data,sync)
}

fn scan_conversations(data: &Connection,sync: &Connection) -> Result<usize,String> {
    let spec = specs().into_iter().find(|spec| spec.object_type=="conversation").unwrap();
    incremental::scan(data,sync,&spec,"device",None,&mut Default::default(),|_,_| Ok(true),
        |object| {
            let id = find_local_id_by_object_id(sync,"conversation",object)?;
            data.query_row("SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1)", [id], |row| row.get(0)).map_err(|err| err.to_string())
        })
}

#[test]
fn test_conversation_identity_migrates_and_reads_during_sync_write() {
    let (data,sync) = databases();
    data.execute("INSERT INTO conversation VALUES(1,'old')", []).unwrap();
    let old = ensure_object_id(&sync,"conversation","conversation",1).unwrap();
    conversation_identity::initialize(&data,Some(&sync)).unwrap();
    assert_eq!(conversation_identity::get(&data,1).unwrap(),old);
    sync.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    data.execute("INSERT INTO conversation VALUES(2,'new')", []).unwrap();
    let uuid = conversation_identity::get(&data,2).unwrap();
    assert_eq!(Uuid::parse_str(&uuid).unwrap().get_version_num(),4);
    sync.execute_batch("ROLLBACK").unwrap();
    scan_conversations(&data,&sync).unwrap();
    assert_eq!(find_object_id(&sync,"conversation",2).unwrap(),Some(uuid));
}

#[test]
fn test_incremental_scan_tracks_rollback_update_delete_and_ack_failure() {
    let (data,sync) = databases();
    conversation_identity::initialize(&data,None).unwrap();
    data.execute_batch("INSERT INTO conversation VALUES(1,'a'),(2,'b')").unwrap();
    assert_eq!(scan_conversations(&data,&sync).unwrap(),2);
    data.execute_batch("BEGIN; UPDATE conversation SET name='discarded'; ROLLBACK;").unwrap();
    assert_eq!(scan_conversations(&data,&sync).unwrap(),0);
    data.execute_batch("UPDATE conversation SET name='updated' WHERE id=1; DELETE FROM conversation WHERE id=2;").unwrap();
    data.execute_batch("CREATE TRIGGER reject_scan_ack BEFORE DELETE ON aipp_scan_dirty BEGIN SELECT RAISE(ABORT,'ack interrupted'); END;").unwrap();
    assert!(scan_conversations(&data,&sync).unwrap_err().contains("ack interrupted"));
    data.execute_batch("DROP TRIGGER reject_scan_ack").unwrap();
    scan_conversations(&data,&sync).unwrap();
    assert_eq!(data.query_row("SELECT count(*) FROM aipp_scan_dirty",[],|row|row.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(sync.query_row("SELECT count(*) FROM sync_outbox WHERE payload_json LIKE '%updated%'",[],|row|row.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(sync.query_row("SELECT count(*) FROM sync_outbox WHERE payload_json LIKE '%\"b\"%'",[],|row|row.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn test_incremental_message_scan_visits_only_changes_and_respects_reset() {
    let (data,sync) = databases();
    data.execute_batch("CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,content TEXT,message_type TEXT,finish_time INTEGER);
        WITH RECURSIVE ids(id) AS (VALUES(1) UNION ALL SELECT id+1 FROM ids WHERE id<150)
        INSERT INTO message SELECT id,1,'old','user',NULL FROM ids;").unwrap();
    ensure_object_id(&sync,"conversation","conversation",1).unwrap();
    let spec = specs().into_iter().find(|spec| spec.object_type=="conversation.message").unwrap();
    let visited = || {
        let mut visits = 0;
        incremental::scan(&data,&sync,&spec,"device",None,&mut Default::default(),|_,_| {
            visits += 1;
            Ok(true)
        },|_| Ok(true)).unwrap();
        visits
    };
    assert_eq!(visited(),150);
    assert_eq!(visited(),0);
    data.execute("UPDATE message SET content='edited' WHERE id=77",[]).unwrap();
    assert_eq!(visited(),1);
    assert_eq!(visited(),0);
    clear_sync_state(&sync).unwrap();
    assert_eq!(visited(),150);
}

#[test]
fn test_conversation_identity_remote_pull_and_fork() {
    let (data,sync) = databases();
    conversation_identity::initialize(&data,None).unwrap();
    let spec = specs().into_iter().find(|spec| spec.object_type=="conversation").unwrap();
    let uuid = Uuid::new_v4().to_string();
    let change = PullChange { seq: 1,event_id: "remote-event".into(),device_id: "other".into(), object_type: "conversation".into(),
        object_id: uuid.clone(), operation: "upsert".into(), version: 1, deleted_at: None, created_at: "now".into(),
        payload: Some(json!({"fields":{"name":"remote"},"refs":{}})) };
    apply_change_to_db(&sync,&data,&spec,&change,"local",|_| Ok(())).unwrap();
    let id = find_local_id_by_object_id(&sync,"conversation",&uuid).unwrap().unwrap();
    assert_eq!(conversation_identity::get(&data,id).unwrap(),uuid);
    let fork = fork_sync_conversation_in_db(&data,id,None).unwrap();
    let fork_uuid = map_conversation_identity(&data,&sync,fork).unwrap();
    assert_eq!(conversation_identity::get(&data,fork).unwrap(),fork_uuid);
    assert_ne!(fork_uuid,uuid);
}

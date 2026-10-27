use super::*;

fn databases() -> (Connection, Connection) {
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT)").unwrap();
    (sync, data)
}

fn event() -> PullChange {
    serde_json::from_value(json!({"seq":1,"event_id":"e","device_id":"remote",
        "object_type":"conversation","object_id":"c","operation":"upsert","version":1,
        "payload":{"fields":{"name":"once"},"refs":{}},"deleted_at":null,"created_at":"now"})).unwrap()
}

fn apply(sync: &Connection, data: &Connection, event: &PullChange) -> Result<(), String> {
    let spec = specs().into_iter().find(|s| s.object_type == "conversation").unwrap();
    apply_change_to_db(sync, data, &spec, event, "local", |_| Ok(()))
}

#[test]
fn test_persistence_business_and_identity_rollback_before_commit() {
    let (sync, data) = databases();
    sync.execute_batch("CREATE TRIGGER fail_map BEFORE INSERT ON sync_object_map BEGIN SELECT RAISE(ABORT,'injected map failure'); END").unwrap();
    let event = event();
    apply_pull_page(&sync, &[event], |e| apply(&sync, &data, e)).unwrap();
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_dead_letter", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    sync.execute_batch("DROP TRIGGER fail_map").unwrap();
    assert_eq!(replay_dead_letter_rows(&sync, |e| apply(&sync, &data, e)).unwrap(), (1, 0));
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
}

#[test]
fn test_persistence_recovers_business_commit_after_sync_commit_failure() {
    let (sync, data) = databases();
    sync.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE required(id INTEGER PRIMARY KEY);
        CREATE TABLE deferred_failure(id INTEGER REFERENCES required(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER fail_commit AFTER INSERT ON sync_apply_receipt BEGIN INSERT INTO deferred_failure VALUES (1); END;").unwrap();
    let event = event();
    assert!(apply(&sync, &data, &event).is_err());
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    assert_eq!(data.query_row("SELECT COUNT(*) FROM aipp_sync_apply_journal", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    assert!(find_local_id_by_object_id(&sync, "conversation", "c").unwrap().is_none());
    sync.execute_batch("DROP TRIGGER fail_commit").unwrap();
    apply(&sync, &data, &event).unwrap();
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    assert_eq!(find_local_id_by_object_id(&sync, "conversation", "c").unwrap(), Some(1));
    assert_eq!(data.query_row("SELECT COUNT(*) FROM aipp_sync_apply_journal", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
    // Capture is disabled between applies, including after failures.
    sync.execute("UPDATE sync_shadow SET payload_hash='later'", []).unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_apply_redo", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
}

#[test]
fn test_persistence_receipt_prevents_replaying_committed_metadata_after_cleanup_failure() {
    let (sync, data) = databases();
    persistence::recover(&sync, &data).unwrap();
    data.execute_batch("CREATE TRIGGER fail_cleanup BEFORE DELETE ON aipp_sync_apply_journal BEGIN SELECT RAISE(ABORT,'cleanup failure'); END").unwrap();
    assert!(apply(&sync, &data, &event()).is_err());
    sync.execute("UPDATE sync_shadow SET payload_hash='newer-local-receipt'", []).unwrap();
    data.execute_batch("DROP TRIGGER fail_cleanup").unwrap();
    persistence::recover(&sync, &data).unwrap();
    assert_eq!(sync.query_row("SELECT payload_hash FROM sync_shadow", [], |r| r.get::<_,String>(0)).unwrap(), "newer-local-receipt");
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
}

#[test]
fn test_persistence_fork_history_and_metadata_rollback_together() {
    let (sync, data) = databases();
    data.execute_batch("ALTER TABLE conversation ADD COLUMN created_time TEXT;
        ALTER TABLE conversation ADD COLUMN updated_time TEXT;
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,content TEXT,created_time TEXT,generation_group_id TEXT);
        CREATE TABLE message_attachment(id INTEGER PRIMARY KEY,message_id INTEGER,attachment_content TEXT);
        INSERT INTO conversation VALUES (1,'source','2020','2024');
        INSERT INTO message VALUES (1,1,'old','2021','g1'), (2,1,'tip','2022','g2');
        INSERT INTO message_attachment VALUES (1,1,'file');").unwrap();
    let result = persistence::coordinated(&sync, &data, || {
        let fork = fork_sync_conversation_in_db(&data, 1, Some(1))?;
        ensure_object_id(&sync, "conversation", "conversation", fork)?;
        Err::<(),_>("injected incoming message failure".into())
    });
    assert!(result.is_err());
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    assert_eq!(data.query_row("SELECT COUNT(*) FROM message", [], |r| r.get::<_,i64>(0)).unwrap(), 2);
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_object_map", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
    let fork = persistence::coordinated(&sync, &data, || fork_sync_conversation_in_db(&data, 1, Some(1))).unwrap();
    assert_eq!(data.query_row("SELECT content,created_time,generation_group_id FROM message WHERE conversation_id=?1", params![fork], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).unwrap(), ("old".into(),"2021".into(),"g1".into()));
    assert_eq!(data.query_row("SELECT updated_time FROM conversation WHERE id=?1", params![fork], |r| r.get::<_,String>(0)).unwrap(), "2021");
    assert_eq!(data.query_row("SELECT COUNT(*) FROM message_attachment", [], |r| r.get::<_,i64>(0)).unwrap(), 2);
}

#[test]
fn test_persistence_failed_capture_installation_can_retry_on_same_connection() {
    let (sync, data) = databases();
    sync.execute_batch("DROP TABLE sync_natural_key").unwrap();
    assert!(apply(&sync, &data, &event()).is_err());
    assert!(sync.is_autocommit());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sqlite_temp_master WHERE name LIKE 'sync_apply_capture_%'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    apply(&sync, &data, &event()).unwrap();
    assert_eq!(data.query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn test_persistence_recovers_changed_primary_key_and_delete_rebase() {
    let (sync, data) = databases();
    sync.execute("INSERT INTO sync_object_alias VALUES ('conversation','old',1)", []).unwrap();
    let spec = specs().into_iter().find(|s| s.object_type == "conversation").unwrap();
    enqueue_delete_event(&sync, &spec, "c", 1, "local").unwrap();
    sync.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE required(id INTEGER PRIMARY KEY);
        CREATE TABLE deferred_failure(id INTEGER REFERENCES required(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER fail_commit AFTER INSERT ON sync_apply_receipt BEGIN INSERT INTO deferred_failure VALUES (1); END;").unwrap();
    assert!(persistence::coordinated(&sync, &data, || {
        sync.execute("UPDATE sync_object_alias SET sync_id='new' WHERE sync_id='old'", []).map_err(|e| e.to_string())?;
        sync.execute("UPDATE sync_outbox SET base_version=3", []).map_err(|e| e.to_string())?;
        Ok(())
    }).is_err());
    sync.execute_batch("DROP TRIGGER fail_commit").unwrap();
    persistence::recover(&sync, &data).unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_object_alias", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(sync.query_row("SELECT sync_id FROM sync_object_alias", [], |r| r.get::<_, String>(0)).unwrap(), "new");
    assert_eq!(sync.query_row("SELECT base_version FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
}

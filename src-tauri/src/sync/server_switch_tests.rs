use super::*;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    conn
}
fn remote(empty: bool, cursor: i64) -> Inspection {
    Inspection { account_id: "a".into(), latest_cursor: cursor, remote_empty: empty, changes: vec![], key_check: None }
}
fn add_shadow(conn: &Connection, seq: i64) {
    conn.execute("INSERT INTO sync_shadow VALUES ('conversation',?1,1,?2,?3,NULL,'now')",
        params![format!("c{seq}"), seq, sync_payload_hash(conn, Some(&json!({"fields":{"name":"hello"},"refs":{}}))).unwrap()]).unwrap();
}
#[test]
fn test_server_switch_empty_remote_is_new() {
    let conn = db(); add_shadow(&conn, 100);
    assert_eq!(classify(&conn, &remote(true, 0), &samples(&conn).unwrap()).unwrap().0.as_deref(), Some("new"));
}
#[test]
fn test_server_switch_rollback_and_missing_history_are_rejected() {
    let conn = db(); add_shadow(&conn, 100);
    for cursor in [80, 101] {
        assert!(classify(&conn, &remote(false, cursor), &samples(&conn).unwrap()).unwrap().0.is_none());
    }
}
#[test]
fn test_server_switch_sample_matches_historical_content() {
    let conn = db(); add_shadow(&conn, 100);
    let mut r = remote(false, 200);
    r.changes.push(PullChange { seq:100,event_id:"e".into(),device_id:"d".into(),object_type:"conversation".into(),object_id:"c100".into(),operation:"upsert".into(),version:1,payload:Some(json!({"fields":{"name":"hello"},"refs":{}})),deleted_at:None,created_at:"now".into() });
    assert_eq!(classify(&conn,&r,&samples(&conn).unwrap()).unwrap().0.as_deref(),Some("migration"));
    r.changes[0].payload = Some(json!({"fields":{"name":"different"},"refs":{}}));
    assert!(classify(&conn,&r,&samples(&conn).unwrap()).unwrap().0.is_none());
}
#[test]
fn test_server_switch_samples_are_bounded_and_unique() {
    let conn = db();
    for i in 1..101 { add_shadow(&conn, i); }
    let sampled = samples(&conn).unwrap();
    assert_eq!(sampled.len(), 10);
    assert_eq!(sampled.iter().map(|s|s.seq).collect::<HashSet<_>>().len(),10);
}
#[test]
fn test_server_switch_new_target_keeps_identity_and_pending_deletes() {
    let conn = db(); add_shadow(&conn, 100);
    conn.execute("INSERT INTO sync_cursor VALUES ('default',100,'now')", []).unwrap();
    conn.execute("INSERT INTO sync_object_map (object_type,local_table,local_id,sync_id,created_at) VALUES ('conversation','conversation',1,'c100','now')", []).unwrap();
    conn.execute("INSERT INTO sync_outbox (event_id,object_type,object_id,operation,base_version,local_version,device_id,created_at,status) VALUES ('old','conversation','gone','delete',8,9,'d','now','pending')", []).unwrap();
    prepare_new_target(&conn).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sync_shadow",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sync_object_map",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    let (id,version): (String,i64) = conn.query_row("SELECT event_id,base_version FROM sync_outbox",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_ne!(id,"old"); assert_eq!(version,0);
    assert_eq!(get_sync_meta(&conn,"allow_initial_merge").unwrap().as_deref(),Some("1"));
    assert!(secrets::server_wins_pending(&conn));
}

#[test]
fn test_server_switch_migration_preserves_progress_and_bootstrap() {
    let conn = db(); add_shadow(&conn,100);
    conn.execute("INSERT INTO sync_cursor VALUES ('default',100,'now')",[]).unwrap();
    set_sync_meta(&conn,"bootstrap_origin","old").unwrap();
    set_sync_meta(&conn,META_NEEDS_RESET,"1").unwrap();
    bind_target(&conn,"new",false,"switch-1").unwrap();
    assert_eq!(conn.query_row("SELECT server_cursor FROM sync_cursor",[],|r|r.get::<_,i64>(0)).unwrap(),100);
    assert_eq!(conn.query_row("SELECT server_seq FROM sync_shadow",[],|r|r.get::<_,i64>(0)).unwrap(),100);
    assert_eq!(get_sync_meta(&conn,"bootstrap_origin").unwrap().as_deref(),Some("new"));
    assert!(!needs_reset(&conn).unwrap());
}

#[test]
fn test_server_switch_recovers_config_and_reset_after_interrupted_commit() {
    let sync=db(); add_shadow(&sync,100);
    sync.execute("INSERT INTO sync_cursor VALUES ('default',100,'now')",[]).unwrap();
    let system=Connection::open_in_memory().unwrap();
    system.execute_batch("CREATE TABLE settings(url TEXT); INSERT INTO settings VALUES ('old')").unwrap();
    sync.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE required(id INTEGER PRIMARY KEY);
        CREATE TABLE deferred_failure(id INTEGER REFERENCES required(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER fail_commit AFTER INSERT ON sync_apply_receipt BEGIN INSERT INTO deferred_failure VALUES (1); END;").unwrap();
    let result=persistence::coordinated(&sync,&system,|| {
        bind_target(&sync,"new",true,"switch-1")?;
        system.execute("UPDATE settings SET url='new'",[]).map_err(|e|e.to_string())?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(system.query_row("SELECT url FROM settings",[],|r|r.get::<_,String>(0)).unwrap(),"new");
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_cursor",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    sync.execute_batch("DROP TRIGGER fail_commit").unwrap();
    persistence::recover(&sync,&system).unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_cursor",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(get_sync_meta(&sync,"last_server_switch_id").unwrap().as_deref(),Some("switch-1"));
}

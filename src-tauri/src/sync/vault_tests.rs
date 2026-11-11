use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct MemoryKeys { values: RefCell<HashMap<String,String>>, fail: bool }
impl KeyStore for MemoryKeys {
    fn remove(&self, slot: &str) -> Result<(),String> { self.values.borrow_mut().remove(slot); Ok(()) }
    fn read(&self, slot: &str) -> Result<Option<String>,String> { Ok(self.values.borrow().get(slot).cloned()) }
    fn write(&self, slot: &str, value: &str) -> Result<(),String> {
        if self.fail { return Err("locked".into()); }
        self.values.borrow_mut().insert(slot.into(),value.into()); Ok(())
    }
}
fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE secure_config(scope TEXT,key TEXT);
        CREATE TABLE feature_config(feature_code TEXT,key TEXT,value TEXT,data_type TEXT,description TEXT,UNIQUE(feature_code,key));").unwrap();
    init(&db).unwrap(); db
}
fn credential() -> crate::mcp::apps::credentials::Credential {
    crate::mcp::apps::credentials::Credential { server_id: 17, endpoint:"https://example.com/mcp".into(),
        headers: HashMap::from([("authorization".into(),"Bearer private-test-value".into()),
            ("x-workspace".into(),"workspace-test".into()), ("x-service-key".into(),"another-private-value".into())]), oauth: None }
}
#[test]
fn test_vault_rekeys_local_credentials_with_provider_key() {
    let db = db(); let store = MemoryKeys::default();
    let old = secrets::random_local_key();
    switch(&db,&store,None,&old).unwrap();
    let reference = Uuid::new_v4().to_string();
    write_with(&db,&old,&reference,&credential()).unwrap();
    let before: String = db.query_row("SELECT value FROM credential_vault",[],|r|r.get(0)).unwrap();
    assert!(!before.contains("private-test-value"));
    let new = secrets::prepare_passphrase(&db,"shared-password",false).unwrap();
    switch(&db,&store,Some(&old),&new).unwrap();
    assert!(active(&db,&store).unwrap().unwrap().record == new.record);
    assert_eq!(read_with(&db,&new,&reference).unwrap().unwrap().headers,credential().headers);
    assert!(read_with(&db,&old,&reference).is_err());
    let restored = secrets::prepare_passphrase(&db,"shared-password",false).unwrap();
    assert_eq!(read_with(&db,&restored,&reference).unwrap().unwrap().endpoint,credential().endpoint);
}
#[test]
fn test_vault_key_store_failure_preserves_old_ciphertext() {
    let db = db(); let store = MemoryKeys::default(); let old = secrets::random_local_key();
    switch(&db,&store,None,&old).unwrap();
    let reference = Uuid::new_v4().to_string(); write_with(&db,&old,&reference,&credential()).unwrap();
    let new = secrets::prepare_passphrase(&db,"replacement-password",false).unwrap();
    let failed = MemoryKeys { values: RefCell::new(store.values.borrow().clone()), fail:true };
    assert!(switch(&db,&failed,Some(&old),&new).is_err());
    assert_eq!(read_with(&db,&old,&reference).unwrap().unwrap().headers,credential().headers);
    assert!(secrets::read_record(&db).unwrap().is_none());
}
#[test]
fn test_vault_missing_key_and_corrupt_ciphertext_never_reset_data() {
    let db = db(); let store = MemoryKeys::default(); let old = secrets::random_local_key();
    switch(&db,&store,None,&old).unwrap();
    assert!(active(&db,&MemoryKeys::default()).is_err());
    let reference = Uuid::new_v4().to_string(); write_with(&db,&old,&reference,&credential()).unwrap();
    db.execute("UPDATE credential_vault SET server_id=18",[]).unwrap();
    assert!(read_with(&db,&old,&reference).is_err());
    let new = secrets::prepare_passphrase(&db,"replacement-password",false).unwrap();
    assert!(switch(&db,&store,Some(&old),&new).is_err());
    assert!(secrets::read_record(&db).unwrap().is_none());
}

#[test]
fn test_vault_sync_remaps_server_and_roundtrips_without_echo() {
    let source = db(); let target = db();
    let source_sync = Connection::open_in_memory().unwrap();
    let target_sync = Connection::open_in_memory().unwrap();
    source_sync.execute_batch(SYNC_SCHEMA_SQL).unwrap(); target_sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    let key = secrets::derive_key("same-password", &[7;16], 1000).unwrap();
    let reference = Uuid::new_v4().to_string();
    write_with(&source,&key,&reference,&credential()).unwrap();
    remap_sync_object_map(&source_sync,"mcp.server","mcp_server",17,"server-identity").unwrap();
    remap_sync_object_map(&target_sync,"mcp.server","mcp_server",99,"server-identity").unwrap();
    let spec = specs().into_iter().find(|s| s.object_type == OBJECT_TYPE).unwrap();
    let snapshots = read_local_snapshots_from_db(&source,&spec,&source_sync,&mut Default::default(),|_,_|Ok(true)).unwrap().0;
    let sealed = vault_sync::seal_snapshots(&source_sync,&key,snapshots).unwrap().remove(0);
    assert!(!sealed.payload_json.contains("private-test-value"));
    assert!(!sealed.payload_json.contains("vault_local_id"));
    let change = PullChange { seq:1,event_id:Uuid::new_v4().to_string(),device_id:"source".into(),object_type:OBJECT_TYPE.into(),
        object_id:sealed.object_id,operation:"upsert".into(),version:1,payload:Some(serde_json::from_str(&sealed.payload_json).unwrap()),deleted_at:None,created_at:now_string() };
    secrets::with_state(Some(secrets::SecretState::Ready(key.clone())),||apply_change_to_db(&target_sync,&target,&spec,&change,"target", |_|Ok(()))).unwrap();
    let restored = read_with(&target,&key,&reference).unwrap().unwrap();
    assert_eq!(restored.server_id,99);
    assert_eq!(restored.headers,credential().headers);
    let snapshots = read_local_snapshots_from_db(&target,&spec,&target_sync,&mut Default::default(),|_,_|Ok(true)).unwrap().0;
    let rescan = vault_sync::seal_snapshots(&target_sync,&key,snapshots).unwrap().remove(0);
    assert_eq!(sealed.payload_json,rescan.payload_json);
    assert!(!enqueue_snapshot_if_changed(&target_sync,&rescan,"target").unwrap());
    let deleted = PullChange {operation:"delete".into(),version:2,seq:2,payload:None,..change};
    apply_change_to_db(&target_sync,&target,&spec,&deleted,"target",|_|Ok(())).unwrap();
    assert!(read_with(&target,&key,&reference).unwrap().is_none());
}

#[test]
fn test_vault_wrong_sync_key_parks_ciphertext_without_overwrite() {
    let data = db(); let sync = Connection::open_in_memory().unwrap(); sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    let key = secrets::derive_key("right-password",&[2;16],1000).unwrap();
    let wrong = secrets::derive_key("wrong-password",&[2;16],1000).unwrap();
    let reference = Uuid::new_v4().to_string();
    write_with(&data,&key,&reference,&credential()).unwrap();
    remap_sync_object_map(&sync,"mcp.server","mcp_server",17,"server-identity").unwrap();
    let spec = specs().into_iter().find(|s|s.object_type==OBJECT_TYPE).unwrap();
    let snapshots = read_local_snapshots_from_db(&data,&spec,&sync,&mut Default::default(),|_,_|Ok(true)).unwrap().0;
    let sealed = vault_sync::seal_snapshots(&sync,&key,snapshots).unwrap().remove(0);
    let change = PullChange {seq:1,event_id:Uuid::new_v4().to_string(),device_id:"remote".into(),object_type:OBJECT_TYPE.into(),object_id:sealed.object_id,
        operation:"upsert".into(),version:1,payload:Some(serde_json::from_str(&sealed.payload_json).unwrap()),deleted_at:None,created_at:now_string()};
    secrets::with_state(Some(secrets::SecretState::Ready(wrong)),||apply_change_to_db(&sync,&data,&spec,&change,"local",|_|Ok(()))).unwrap();
    assert_eq!(secrets::pending_count(&sync),1);
    assert_eq!(read_with(&data,&key,&reference).unwrap().unwrap().headers,credential().headers);
    secrets::with_state(Some(secrets::SecretState::Ready(key.clone())),||secrets::replay_pending(&sync,|c|apply_change_to_db(&sync,&data,&spec,c,"local",|_|Ok(())))).unwrap();
    assert_eq!(secrets::pending_count(&sync),0);
}

#[test]
fn test_vault_connection_metadata_maps_server_and_restores_reference_header() {
    let source = Connection::open_in_memory().unwrap(); let target = Connection::open_in_memory().unwrap();
    let a = Connection::open_in_memory().unwrap(); let b = Connection::open_in_memory().unwrap();
    for sync in [&a,&b] { sync.execute_batch(SYNC_SCHEMA_SQL).unwrap(); }
    for data in [&source,&target] {
        data.execute_batch("CREATE TABLE mcp_server(id INTEGER PRIMARY KEY,headers TEXT);").unwrap();
        crate::mcp::apps::init(data).unwrap();
    }
    source.execute("INSERT INTO mcp_server(id) VALUES(17)",[]).unwrap();
    target.execute("INSERT INTO mcp_server(id) VALUES(99)",[]).unwrap();
    let reference = Uuid::new_v4().to_string();
    source.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,credential_ref,status)
        VALUES(17,'request-id','example','{}','Example','{}',?1,'available')",[&reference]).unwrap();
    remap_sync_object_map(&a,"mcp.server","mcp_server",17,"same-server").unwrap();
    remap_sync_object_map(&b,"mcp.server","mcp_server",99,"same-server").unwrap();
    let spec = specs().into_iter().find(|s|s.object_type==CONNECTION_TYPE).unwrap();
    let snapshot = read_local_snapshots_from_db(&source,&spec,&a,&mut Default::default(),|_,_|Ok(true)).unwrap().0.remove(0);
    let change = PullChange {seq:1,event_id:Uuid::new_v4().to_string(),device_id:"a".into(),object_type:CONNECTION_TYPE.into(),object_id:snapshot.object_id,
        operation:"upsert".into(),version:1,payload:Some(serde_json::from_str(&snapshot.payload_json).unwrap()),deleted_at:None,created_at:now_string()};
    apply_change_to_db(&b,&target,&spec,&change,"b",|_|Ok(())).unwrap();
    let (id,status,headers):(i64,String,String) = target.query_row("SELECT c.id,c.status,s.headers FROM app_connection c JOIN mcp_server s ON s.id=c.server_id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(id,99); assert_eq!(status,"unchecked");
    assert_eq!(serde_json::from_str::<JsonValue>(&headers).unwrap()["x-aipp-credential-ref"],reference);
    let snapshot = read_local_snapshots_from_db(&target,&spec,&b,&mut Default::default(),|_,_|Ok(true)).unwrap().0.remove(0);
    assert!(!enqueue_snapshot_if_changed(&b,&snapshot,"b").unwrap());
}

#[test]
fn test_vault_rotation_discards_only_confirmed_obsolete_uploads() {
    let sync = Connection::open_in_memory().unwrap(); sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    let old = secrets::random_local_key(); let new = secrets::random_local_key();
    let envelope = secrets::seal(&old,"object","value","private").unwrap();
    let payload = json!({"fields":{"value":envelope},"refs":{}});
    let snapshot = LocalObjectSnapshot {object_type:OBJECT_TYPE.into(),object_id:Uuid::new_v4().to_string(),payload_hash:sync_payload_hash(&sync,Some(&payload)).unwrap(),payload_json:payload.to_string()};
    enqueue_snapshot_if_changed(&sync,&snapshot,"device").unwrap();
    sync.execute("UPDATE sync_outbox SET sent_once=1,outcome_known=0",[]).unwrap();
    assert!(vault_sync::discard_obsolete_outbox(&sync,Some(&new)).is_err());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    sync.execute("UPDATE sync_outbox SET outcome_known=1",[]).unwrap();
    vault_sync::discard_obsolete_outbox(&sync,Some(&new)).unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn test_vault_database_failure_rolls_back_key_and_ciphertext() {
    let db = db(); let keys = MemoryKeys::default(); let old = secrets::random_local_key();
    switch(&db,&keys,None,&old).unwrap();
    let reference = Uuid::new_v4().to_string(); write_with(&db,&old,&reference,&credential()).unwrap();
    db.execute_batch("CREATE TRIGGER fail_switch BEFORE UPDATE ON credential_vault BEGIN SELECT RAISE(ABORT,'simulated failure'); END;").unwrap();
    let new = secrets::prepare_passphrase(&db,"replacement-password",false).unwrap();
    assert!(switch(&db,&keys,Some(&old),&new).is_err());
    assert!(active(&db,&keys).unwrap().unwrap().record == old.record);
    assert_eq!(read_with(&db,&old,&reference).unwrap().unwrap().headers,credential().headers);
    cleanup_keys(&db,&keys).unwrap();
    assert_eq!(keys.values.borrow().len(),1);
}

#[test]
fn test_vault_keeps_old_sync_key_until_remote_reencryption_finishes() {
    let db = db(); let keys = MemoryKeys::default();
    let old = secrets::prepare_passphrase(&db,"original-password",false).unwrap();
    switch(&db,&keys,None,&old).unwrap();
    let mut new = secrets::prepare_passphrase(&db,"replacement-password",true).unwrap();
    switch(&db,&keys,Some(&old),&new).unwrap();
    assert_eq!(keys.values.borrow().len(),2);
    let envelope = secrets::seal(&old,"remote-object","value","late remote value").unwrap();
    assert!(secrets::open(&new,"remote-object","value",&envelope).is_err());
    new.previous.push(old);
    assert_eq!(secrets::open(&new,"remote-object","value",&envelope).unwrap(),"late remote value");
    assert_eq!(secrets::seal(&new,"remote-object","value","late remote value").unwrap()["kid"],new.kid());
}

#[test]
fn test_vault_legacy_key_migration_preserves_newer_remote_key_record() {
    let db = db(); let keys = MemoryKeys::default();
    let legacy = secrets::derive_key("legacy-password", &[1;16],1000).unwrap();
    let remote = secrets::derive_key("newer-password", &[2;16],1000).unwrap();
    secrets::write_record(&db,&remote.record).unwrap();
    switch_key(&db,&keys,None,&legacy,false).unwrap();
    assert!(active(&db,&keys).unwrap().unwrap().record == legacy.record);
    assert!(secrets::read_record(&db).unwrap().unwrap() == remote.record);
}

/// An empty Vault status read must not touch the OS store or legacy master key.
struct UnavailableKeys;
impl KeyStore for UnavailableKeys {
    fn read(&self, _slot: &str) -> Result<Option<String>, String> { Err("OS store locked".into()) }
    fn write(&self, _slot: &str, _value: &str) -> Result<(), String> { panic!("status must not provision a key") }
    fn remove(&self, _slot: &str) -> Result<(), String> { panic!("status must not remove a key") }
}

#[test]
fn test_vault_state_empty_does_not_provision_key() {
    let db = db();
    for _ in 0..2 {
        let state = state_with(&db, &UnavailableKeys, || panic!("no legacy key to load")).unwrap();
        assert_eq!(state.code(), "not_configured");
        assert!(state.ready_key().is_none());
    }
    assert_eq!(db.query_row("SELECT COUNT(*) FROM credential_vault_key", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_vault_state_remote_record_needs_passphrase_without_creating_key() {
    let db = db();
    let key = secrets::prepare_passphrase(&db, "remote-password", false).unwrap();
    secrets::write_record(&db, &key.record).unwrap();
    let state = state_with(&db, &UnavailableKeys, || panic!("no legacy key to load")).unwrap();
    assert_eq!(state.code(), "needs_passphrase");
    assert_eq!(db.query_row("SELECT COUNT(*) FROM credential_vault_key", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(secrets::read_record(&db).unwrap() == Some(key.record));
}

#[test]
fn test_vault_state_locked_store_pauses_secret_sync_preserves_ciphertext_and_recovers() {
    let db = db(); let store = MemoryKeys::default();
    let key = secrets::prepare_passphrase(&db, "shared-password", false).unwrap();
    switch(&db, &store, None, &key).unwrap();
    let reference = Uuid::new_v4().to_string();
    write_with(&db, &key, &reference, &credential()).unwrap();
    let before: String = db.query_row("SELECT value FROM credential_vault", [], |r| r.get(0)).unwrap();
    let state = super::secret_state_for_sync(state_with(&db, &UnavailableKeys, || panic!("active key exists")));
    assert_eq!(state.code(), "unavailable");
    assert!(state.ready_key().is_none());
    assert!(state.message(0).contains("OS store locked"));
    assert_eq!(db.query_row("SELECT value FROM credential_vault", [], |r| r.get::<_, String>(0)).unwrap(), before);
    let recovered = state_with(&db, &store, || panic!("active key exists")).unwrap();
    assert_eq!(recovered.code(), "ready");
    assert_eq!(read_with(&db, recovered.ready_key().unwrap(), &reference).unwrap().unwrap().headers, credential().headers);
}

#[test]
fn test_vault_state_missing_key_metadata_never_replaces_ciphertext() {
    let db = db();
    let reference = Uuid::new_v4().to_string();
    db.execute("INSERT INTO credential_vault(reference,server_id,value) VALUES(?1,17,'preserved-ciphertext')", [&reference]).unwrap();
    let state = super::secret_state_for_sync(state_with(&db, &UnavailableKeys, || panic!("no legacy key")));
    assert_eq!(state.code(), "unavailable");
    assert_eq!(db.query_row("SELECT value FROM credential_vault", [], |r| r.get::<_, String>(0)).unwrap(), "preserved-ciphertext");
    assert_eq!(db.query_row("SELECT COUNT(*) FROM credential_vault_key", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_vault_state_migrates_legacy_key_without_replacing_newer_record() {
    let db = db(); let store = MemoryKeys::default();
    let legacy = secrets::prepare_passphrase(&db, "old-password", false).unwrap();
    let newer = secrets::prepare_passphrase(&db, "new-password", false).unwrap();
    secrets::write_record(&db, &newer.record).unwrap();
    db.execute("INSERT INTO secure_config(scope,key) VALUES('data_sync','secret_key')", []).unwrap();
    let state = state_with(&db, &store, || Ok(Some(legacy.clone()))).unwrap();
    assert_eq!(state.code(), "mismatch");
    assert!(active(&db, &store).unwrap().unwrap().record == legacy.record);
    assert!(secrets::read_record(&db).unwrap() == Some(newer.record));
    assert_eq!(db.query_row("SELECT COUNT(*) FROM secure_config", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_vault_state_failed_legacy_migration_preserves_cache_then_retries() {
    let db = db(); let failed = MemoryKeys { fail: true, ..Default::default() };
    let legacy = secrets::prepare_passphrase(&db, "legacy-password", false).unwrap();
    secrets::write_record(&db, &legacy.record).unwrap();
    db.execute("INSERT INTO secure_config(scope,key) VALUES('data_sync','secret_key')", []).unwrap();
    let state = super::secret_state_for_sync(state_with(&db, &failed, || Ok(Some(legacy.clone()))));
    assert_eq!(state.code(), "unavailable");
    assert!(state.ready_key().is_none());
    assert_eq!(db.query_row("SELECT COUNT(*) FROM secure_config", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT COUNT(*) FROM credential_vault_key", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(secrets::read_record(&db).unwrap() == Some(legacy.record.clone()));
    let store = MemoryKeys::default();
    assert_eq!(state_with(&db, &store, || Ok(Some(legacy))).unwrap().code(), "ready");
    assert_eq!(db.query_row("SELECT COUNT(*) FROM secure_config", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_vault_state_missing_retained_key_pauses_without_retiring_it() {
    let db = db(); let store = MemoryKeys::default();
    let old = secrets::prepare_passphrase(&db, "old-password", false).unwrap();
    switch(&db, &store, None, &old).unwrap();
    let new = secrets::prepare_passphrase(&db, "new-password", true).unwrap();
    switch(&db, &store, Some(&old), &new).unwrap();
    let slot: String = db.query_row("SELECT slot FROM credential_vault_retired_key WHERE retain=1", [], |r| r.get(0)).unwrap();
    store.values.borrow_mut().remove(&slot);
    let state = super::secret_state_for_sync(state_with(&db, &store, || panic!("active key exists")));
    assert_eq!(state.code(), "unavailable");
    assert!(state.ready_key().is_none());
    assert_eq!(db.query_row("SELECT retain FROM credential_vault_retired_key WHERE slot=?1", [&slot], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(active(&db, &store).unwrap().unwrap().record == new.record);
}

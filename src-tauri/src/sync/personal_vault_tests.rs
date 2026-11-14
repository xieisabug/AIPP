use super::*;

const FAKE: &str = "aipp-fake-token-7f3a91";

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    init(&db).unwrap();
    db
}

fn input(key: &str, value: &str) -> CreateInput {
    CreateInput {
        name: format!("{key} 名称"), key: key.into(), description: "测试".into(),
        value: SecretValue::new(value), allowed_sites: vec![], sync_enabled: true,
    }
}

fn create(db: &Connection, key: &secrets::SecretKey, entry_key: &str, value: &str) -> ItemMeta {
    create_with(db, key, &input(entry_key, value)).unwrap()
}

fn resolve_one(db: &Connection, key: Option<&secrets::SecretKey>, entry_key: &str) -> Result<Resolved, ResolveError> {
    resolve_keys_with(db, key, &[entry_key.to_string()]).map(|mut m| m.remove(entry_key).unwrap())
}

fn usage<'a>(item_id: &'a str) -> NewUsage<'a> {
    NewUsage { item_id, conversation_id: Some(1), assistant_id: Some(2), tool: "AIPP 内置 · vault_http_request",
        destination: "https://api.github.com", outcome: "ok", reason: None }
}

#[test]
fn test_vault_create_resolve_roundtrip() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    assert_eq!(meta.status, "ready");
    assert_eq!(meta.key, "github_pat");
    assert_eq!(resolve_one(&db, Some(&key), "github_pat").unwrap().value, FAKE);
}

#[test]
fn test_vault_metadata_never_contains_secret() {
    let db = db();
    let key = secrets::random_local_key();
    create(&db, &key, "github_pat", FAKE);
    let listed = serde_json::to_string(&list_with(&db, Some(&key)).unwrap()).unwrap();
    assert!(!listed.contains(FAKE));
    assert!(!listed.contains(&FAKE[FAKE.len() - 4..]), "no fragments either");
    let raw: String = db.query_row("SELECT value FROM vault_item", [], |r| r.get(0)).unwrap();
    assert!(!raw.contains(FAKE), "stored value must be ciphertext");
    let resolved = resolve_one(&db, Some(&key), "github_pat").unwrap();
    assert!(!format!("{resolved:?}").contains(FAKE));
    assert!(!format!("{:?}", input("x", FAKE)).contains(FAKE), "inputs never print their value");
}

#[test]
fn test_vault_ciphertext_bound_to_item() {
    let db = db();
    let key = secrets::random_local_key();
    create(&db, &key, "first", "secret-one");
    create(&db, &key, "second", "secret-two");
    db.execute("UPDATE vault_item SET value=(SELECT value FROM vault_item WHERE handle='first') WHERE handle='second'", []).unwrap();
    assert_eq!(resolve_one(&db, Some(&key), "second").unwrap_err().code, "vault_locked");
    assert_eq!(list_with(&db, Some(&key)).unwrap().iter().find(|m| m.key == "second").unwrap().status, "locked");
}

#[test]
fn test_vault_reencrypt_on_key_switch() {
    let db = db();
    let old = secrets::random_local_key();
    let new = secrets::random_local_key();
    create(&db, &old, "github_pat", FAKE);
    reencrypt(&db, Some(&old), &new).unwrap();
    assert_eq!(resolve_one(&db, Some(&new), "github_pat").unwrap().value, FAKE);
    assert_eq!(resolve_one(&db, Some(&old), "github_pat").unwrap_err().code, "vault_locked");
}

#[test]
fn test_vault_reencrypt_requires_old_key_when_items_exist() {
    let db = db();
    let key = secrets::random_local_key();
    create(&db, &key, "github_pat", FAKE);
    assert!(reencrypt(&db, None, &secrets::random_local_key()).is_err());
}

#[test]
fn test_vault_resolve_errors() {
    let db = db();
    let key = secrets::random_local_key();
    assert_eq!(resolve_one(&db, Some(&key), "missing").unwrap_err().code, "credential_not_found");
    create(&db, &key, "github_pat", FAKE);
    assert_eq!(resolve_one(&db, None, "github_pat").unwrap_err().code, "vault_locked");
    // Two devices created the same key and both synced in: refuse instead of guessing.
    db.execute("INSERT INTO vault_item(uuid, handle, name, value, created_at, updated_at)
        SELECT 'dup', handle, name, value, created_at, updated_at FROM vault_item", []).unwrap();
    assert_eq!(resolve_one(&db, Some(&key), "github_pat").unwrap_err().code, "credential_ambiguous");
}

#[test]
fn test_vault_key_validation_and_uniqueness() {
    let db = db();
    let key = secrets::random_local_key();
    for bad in ["", "a", "GitHub", "1abc", "has-dash", "中文"] { assert!(validate_key(bad).is_err(), "{bad}"); }
    create(&db, &key, "github_pat", FAKE);
    assert!(create_with(&db, &key, &input("github_pat", "other")).unwrap_err().contains("已被使用"));
}

#[test]
fn test_vault_value_preserves_whitespace_and_is_required() {
    let db = db();
    let key = secrets::random_local_key();
    assert!(create_with(&db, &key, &input("empty", "")).is_err());
    assert!(create_with(&db, &key, &input("empty", "   ")).is_err());
    let value = " \t password with spaces \r\n";
    let meta = create(&db, &key, "password", value);
    assert_eq!(resolve_one(&db, Some(&key), "password").unwrap().value, value);
    let replacement = "\n second password \t ";
    replace_secret_with(&db, &key, &meta.id, &SecretValue::new(replacement)).unwrap();
    assert_eq!(resolve_one(&db, Some(&key), "password").unwrap().value, replacement);
    assert!(replace_secret_with(&db, &key, &meta.id, &SecretValue::new(" \t\n")).is_err());
    assert_eq!(resolve_one(&db, Some(&key), "password").unwrap().value, replacement);
}

#[test]
fn test_vault_value_limit_counts_original_bytes() {
    let value = format!(" {} ", "x".repeat(MAX_VALUE_BYTES - 2));
    assert_eq!(SecretValue::new(&value).checked().unwrap(), value);
    assert!(SecretValue::new(format!(" {value}")).checked().is_err());
}

#[test]
fn test_vault_create_many_preserves_original_values() {
    let db = db();
    let key = secrets::random_local_key();
    create_many_with(&db, &key, &[input("first", " first "), input("second", "\tsecond\n")]).unwrap();
    assert_eq!(resolve_one(&db, Some(&key), "first").unwrap().value, " first ");
    assert_eq!(resolve_one(&db, Some(&key), "second").unwrap().value, "\tsecond\n");
}

/// An edit while the approval card is open must invalidate the old decision, even within the same
/// millisecond. Neither the old nor the new value is returned, and "always" does not persist.
#[test]
fn test_vault_confirm_use_rejects_replaced_value_without_granting_site() {
    let db = db();
    let key = secrets::random_local_key();
    let first = create(&db, &key, "first", FAKE);
    let second = create(&db, &key, "second", "old-second");
    let expected = resolve_keys_with(&db, Some(&key), &["first".into(), "second".into()]).unwrap();
    replace_secret_with(&db, &key, &second.id, &SecretValue::new("new-second")).unwrap();
    db.execute("UPDATE vault_item SET updated_at=?2 WHERE uuid=?1", params![second.id, second.updated_at]).unwrap();
    let error = confirm_use_with(&db, &key, &expected,
        Some((&[first.id.clone(), second.id.clone()], "https://example.com"))).unwrap_err();
    assert_eq!(error.code, "credential_changed");
    assert!(!error.message.contains(FAKE));
    assert!(!error.message.contains("old-second"));
    assert!(!error.message.contains("new-second"));
    assert!(list_with(&db, Some(&key)).unwrap().iter().all(|item| item.allowed_sites.is_empty()));
}

#[test]
fn test_vault_confirm_use_rejects_deleted_or_recreated_entry() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "token", FAKE);
    let expected = resolve_keys_with(&db, Some(&key), &["token".into()]).unwrap();
    delete_with(&db, &meta.id).unwrap();
    assert_eq!(confirm_use_with(&db, &key, &expected, None).unwrap_err().code, "credential_not_found");
    create(&db, &key, "token", FAKE);
    assert_eq!(confirm_use_with(&db, &key, &expected, None).unwrap_err().code, "credential_changed");
}

#[test]
fn test_vault_confirm_use_rejects_changed_policy() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "token", FAKE);
    approve_site_with(&db, &meta.id, "https://example.com").unwrap();
    let expected = resolve_keys_with(&db, Some(&key), &["token".into()]).unwrap();
    update_with(&db, Some(&key), &meta.id, &UpdateInput { allowed_sites: Some(vec![]), ..Default::default() }).unwrap();
    assert_eq!(confirm_use_with(&db, &key, &expected, None).unwrap_err().code, "credential_changed");
}

#[test]
fn test_vault_confirm_use_allows_unchanged_entries_after_another_use() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "token", " token ");
    let expected = resolve_keys_with(&db, Some(&key), &["token".into()]).unwrap();
    log_usage_with(&db, &usage(&meta.id)).unwrap();
    let current = confirm_use_with(&db, &key, &expected, None).unwrap();
    assert_eq!(current["token"].value, " token ");
    assert!(current["token"].meta.last_used_at.is_some());
    confirm_use_with(&db, &key, &expected, Some((&[meta.id], "https://example.com"))).unwrap();
    assert_eq!(list_with(&db, Some(&key)).unwrap()[0].allowed_sites, vec!["https://example.com"]);
}

#[test]
fn test_vault_confirm_use_rolls_back_partial_site_grants() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "token", FAKE);
    let expected = resolve_keys_with(&db, Some(&key), &["token".into()]).unwrap();
    assert!(confirm_use_with(&db, &key, &expected,
        Some((&[meta.id, "not-approved".into()], "https://example.com"))).is_err());
    assert!(list_with(&db, Some(&key)).unwrap()[0].allowed_sites.is_empty());
}

#[test]
fn test_vault_create_many_is_all_or_nothing() {
    let db = db();
    let key = secrets::random_local_key();
    create(&db, &key, "phone", "13800000000");
    let error = create_many_with(&db, &key, &[input("id_card", "110101199001011234"), input("phone", "13900000000")]).unwrap_err();
    assert!(error.contains("已被使用"));
    assert!(resolve_one(&db, Some(&key), "id_card").is_err(), "the first entry is rolled back");
    let created = create_many_with(&db, &key, &[input("id_card", "110101199001011234"), input("passport", "E12345678")]).unwrap();
    assert_eq!(created.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(), vec!["id_card", "passport"]);
}

#[test]
fn test_vault_normalize_origin() {
    assert_eq!(normalize_origin("https://API.GitHub.com/user/repos?x=1").unwrap(), "https://api.github.com");
    assert_eq!(normalize_origin("api.github.com").unwrap(), "https://api.github.com");
    assert_eq!(normalize_origin("https://example.com:443/").unwrap(), "https://example.com");
    assert_eq!(normalize_origin("http://192.168.1.10:8123").unwrap(), "http://192.168.1.10:8123");
    assert!(normalize_origin("ftp://example.com").is_err());
    assert!(normalize_origin("https://user:pw@example.com").is_err());
}

#[test]
fn test_vault_approve_site_dedups() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    approve_site_with(&db, &meta.id, "https://api.github.com/user").unwrap();
    approve_site_with(&db, &meta.id, "https://API.github.com").unwrap();
    assert_eq!(list_with(&db, Some(&key)).unwrap()[0].allowed_sites, vec!["https://api.github.com".to_string()]);
}

#[test]
fn test_vault_update_and_replace_secret() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    let updated = update_with(&db, Some(&key), &meta.id, &UpdateInput {
        key: Some("gh".into()), allowed_sites: Some(vec!["api.github.com".into()]), ..Default::default()
    }).unwrap();
    assert_eq!(updated.key, "gh");
    assert_eq!(updated.allowed_sites, vec!["https://api.github.com".to_string()]);
    replace_secret_with(&db, &key, &meta.id, &SecretValue::new("new-token")).unwrap();
    assert_eq!(resolve_one(&db, Some(&key), "gh").unwrap().value, "new-token");
}

#[test]
fn test_vault_delete_removes_item_and_usage() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    log_usage_with(&db, &usage(&meta.id)).unwrap();
    assert_eq!(usage_with(&db, &meta.id, 10).unwrap()[0].destination, "https://api.github.com");
    delete_with(&db, &meta.id).unwrap();
    assert!(list_with(&db, Some(&key)).unwrap().is_empty());
    assert!(usage_with(&db, &meta.id, 10).unwrap().is_empty());
    assert!(delete_with(&db, &meta.id).is_err());
}

#[test]
fn test_vault_usage_log_updates_last_used_and_trims() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    assert!(meta.last_used_at.is_none());
    for _ in 0..(USAGE_KEEP_PER_ITEM + 5) { log_usage_with(&db, &usage(&meta.id)).unwrap(); }
    let count: i64 = db.query_row("SELECT COUNT(*) FROM vault_usage_log", [], |r| r.get(0)).unwrap();
    assert_eq!(count, USAGE_KEEP_PER_ITEM);
    assert!(list_with(&db, Some(&key)).unwrap()[0].last_used_at.is_some());
}

// ---------- development-build entries ----------

fn legacy_db(key: &secrets::SecretKey) -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE vault_item (id INTEGER PRIMARY KEY, uuid TEXT NOT NULL UNIQUE, handle TEXT NOT NULL, name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '', auth_kind TEXT NOT NULL, injection TEXT NOT NULL, allowed_sites TEXT NOT NULL DEFAULT '[]',
            sync_enabled INTEGER NOT NULL DEFAULT 1, value TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
            last_used_at INTEGER, config_bindings TEXT NOT NULL DEFAULT '[]');
         CREATE TABLE vault_usage_log (id INTEGER PRIMARY KEY, item_uuid TEXT NOT NULL, conversation_id INTEGER, assistant_id INTEGER,
            method TEXT NOT NULL, url TEXT NOT NULL, status INTEGER, outcome TEXT NOT NULL, reason TEXT, created_at INTEGER NOT NULL);",
    ).unwrap();
    let insert = |uuid: &str, handle: &str, kind: &str, fields: &[(&str, &str)]| {
        let payload = SecretPayload { value: None, fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() };
        db.execute("INSERT INTO vault_item(uuid, handle, name, auth_kind, injection, allowed_sites, value, created_at, updated_at)
            VALUES(?1, ?2, ?2, ?3, '[]', '[\"https://nas.local\"]', ?4, 1, 1)",
            params![uuid, handle, kind, seal_payload(key, uuid, &payload).unwrap()]).unwrap();
    };
    insert("11111111-1111-4111-8111-111111111111", "gh", "bearer", &[("token", FAKE)]);
    insert("22222222-2222-4222-8222-222222222222", "nas", "basic", &[("username", "admin"), ("password", "pw-secret-1")]);
    db
}

#[test]
fn test_legacy_entries_convert_to_single_values() {
    let key = secrets::random_local_key();
    let db = legacy_db(&key);
    init(&db).unwrap();
    assert!(!has_column(&db, "vault_item", "auth_kind").unwrap());
    assert!(has_column(&db, "vault_usage_log", "destination").unwrap());
    assert!(has_legacy(&db));
    migrate_legacy_with(&db, &key).unwrap();
    assert!(!has_legacy(&db));
    assert_eq!(resolve_one(&db, Some(&key), "gh").unwrap().value, FAKE);
    let user = resolve_one(&db, Some(&key), "nas_username").unwrap();
    let password = resolve_one(&db, Some(&key), "nas_password").unwrap();
    assert_eq!((user.value.as_str(), password.value.as_str()), ("admin", "pw-secret-1"));
    assert_eq!(password.meta.allowed_sites, vec!["https://nas.local".to_string()], "the split entry keeps its settings");
    assert_eq!(user.meta.id, "22222222-2222-4222-8222-222222222222", "the original id (and its references) stays with the user name");
}

#[test]
fn test_legacy_entries_are_not_synced_before_conversion() {
    let key = secrets::random_local_key();
    let db = legacy_db(&key);
    init(&db).unwrap();
    let sync = sync_db();
    assert!(sealed_snapshots(&db, &sync, &key).is_empty());
    migrate_legacy_with(&db, &key).unwrap();
    assert_eq!(sealed_snapshots(&db, &sync, &key).len(), 3);
}

// ---------- sync ----------

fn sync_db() -> Connection {
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    sync
}

fn spec() -> SyncTableSpec {
    specs().into_iter().find(|s| s.object_type == OBJECT_TYPE).unwrap()
}

fn sealed_snapshots(data: &Connection, sync: &Connection, key: &secrets::SecretKey) -> Vec<LocalObjectSnapshot> {
    let snapshots = read_local_snapshots_from_db(data, &spec(), sync, &mut Default::default(), |_, _| Ok(true)).unwrap().0;
    seal_snapshots(sync, key, snapshots).unwrap()
}

fn upsert_change(snapshot: &LocalObjectSnapshot) -> PullChange {
    PullChange { seq: 1, event_id: Uuid::new_v4().to_string(), device_id: "source".into(), object_type: OBJECT_TYPE.into(),
        object_id: snapshot.object_id.clone(), operation: "upsert".into(), version: 1,
        payload: Some(serde_json::from_str(&snapshot.payload_json).unwrap()), deleted_at: None, created_at: now_string() }
}

#[test]
fn test_vault_sync_roundtrip_without_plaintext_or_echo() {
    let (source, target) = (db(), db());
    let (source_sync, target_sync) = (sync_db(), sync_db());
    let key = secrets::derive_key("same-passphrase", &[9; 16], 1000).unwrap();
    let meta = create(&source, &key, "github_pat", FAKE);
    log_usage_with(&source, &usage(&meta.id)).unwrap();
    let sealed = sealed_snapshots(&source, &source_sync, &key).remove(0);
    assert!(!sealed.payload_json.contains(FAKE), "wire payload must not contain the secret");
    assert!(!sealed.payload_json.contains("last_used_at"), "local usage time is not synced");
    let change = upsert_change(&sealed);
    secrets::with_state(Some(secrets::SecretState::Ready(key.clone())),
        || apply_change_to_db(&target_sync, &target, &spec(), &change, "target", |_| Ok(()))).unwrap();
    assert_eq!(resolve_one(&target, Some(&key), "github_pat").unwrap().value, FAKE);
    let rescan = sealed_snapshots(&target, &target_sync, &key).remove(0);
    assert_eq!(sealed.payload_json, rescan.payload_json);
    assert!(!enqueue_snapshot_if_changed(&target_sync, &rescan, "target").unwrap(), "applied remote state must not echo back");
}

#[test]
fn test_vault_sync_wrong_key_parks_then_replays() {
    let (source, target) = (db(), db());
    let (source_sync, target_sync) = (sync_db(), sync_db());
    let key = secrets::derive_key("right", &[3; 16], 1000).unwrap();
    let wrong = secrets::derive_key("wrong", &[3; 16], 1000).unwrap();
    create(&source, &key, "github_pat", FAKE);
    let change = upsert_change(&sealed_snapshots(&source, &source_sync, &key).remove(0));
    secrets::with_state(Some(secrets::SecretState::Ready(wrong)),
        || apply_change_to_db(&target_sync, &target, &spec(), &change, "target", |_| Ok(()))).unwrap();
    assert_eq!(secrets::pending_count(&target_sync), 1);
    assert!(list_with(&target, None).unwrap().is_empty(), "nothing is written without the right key");
    secrets::with_state(Some(secrets::SecretState::Ready(key.clone())),
        || secrets::replay_pending(&target_sync, |c| apply_change_to_db(&target_sync, &target, &spec(), c, "target", |_| Ok(())))).unwrap();
    assert_eq!(secrets::pending_count(&target_sync), 0);
    assert_eq!(resolve_one(&target, Some(&key), "github_pat").unwrap().value, FAKE);
}

#[test]
fn test_vault_sync_skips_local_only_items() {
    let source = db();
    let sync = sync_db();
    let key = secrets::random_local_key();
    let meta = create(&source, &key, "local_only", FAKE);
    update_with(&source, Some(&key), &meta.id, &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    assert!(sealed_snapshots(&source, &sync, &key).is_empty());
}

// Exercise the production ready-key scan, queue, ACK, and receiver paths with in-memory DBs.
fn scan_ready(data: &Connection, sync: &Connection, key: &secrets::SecretKey) {
    let (snapshots, present) = read_local_snapshots_from_db(data, &spec(), sync,
        &mut Default::default(), |_, _| Ok(true)).unwrap();
    for snapshot in seal_snapshots(sync, key, snapshots).unwrap() {
        enqueue_snapshot_if_changed(sync, &snapshot, "source").unwrap();
    }
    scan_local_deletes_in_db(sync, &spec(), &present,
        |id| local_row_exists_in_db(sync, data, &spec(), id)).unwrap();
}

// Models a successful server response; the server's actual HTTP delete contract has a Python test.
fn accept_one(sync: &Connection, version: i64, seq: i64) -> PullChange {
    let events = load_outbox_from_db_with_secret_readiness(sync, 100, false, true).unwrap();
    assert_eq!(events.len(), 1);
    mark_events_pushing_in_db(sync, &events).unwrap();
    let (id, event) = events.into_iter().next().unwrap();
    consume_accepted_event(sync, id, &event, &AcceptedEvent {
        event_id: event.event_id.clone(), object_type: event.object_type.clone(),
        object_id: event.object_id.clone(), server_version: version, server_seq: seq,
    }).unwrap();
    PullChange { seq, event_id: event.event_id, device_id: "source".into(),
        object_type: event.object_type, object_id: event.object_id, operation: event.operation.clone(),
        version, payload: event.payload, deleted_at: (event.operation == "delete").then(now_string),
        created_at: now_string() }
}

fn receive(data: &Connection, sync: &Connection, key: Option<&secrets::SecretKey>, change: &PullChange) {
    secrets::with_state(key.cloned().map(secrets::SecretState::Ready),
        || apply_change_to_db(sync, data, &spec(), change, "receiver", |_| Ok(()))).unwrap();
}

/// Disabling an already-synced item sends a payload-free tombstone, removes other devices' copies,
/// preserves the source value, and does not echo or resurrect it. Explicit re-enabling still works.
#[test]
fn test_vault_disable_sync_withdraws_remote_copy_and_can_reenable() {
    let (source, target) = (db(), db());
    let (source_sync, target_sync) = (sync_db(), sync_db());
    let key = secrets::derive_key("same-passphrase", &[8; 16], 1000).unwrap();
    let default_input: CreateInput = serde_json::from_value(json!({
        "name": "GitHub", "key": "github_pat", "value": FAKE,
    })).unwrap();
    let meta = create_with(&source, &key, &default_input).unwrap();
    assert!(meta.sync_enabled, "new entries still default to syncing");
    scan_ready(&source, &source_sync, &key);
    let original = accept_one(&source_sync, 1, 1);
    receive(&target, &target_sync, Some(&key), &original);
    assert_eq!(resolve_one(&target, Some(&key), "github_pat").unwrap().value, FAKE);

    update_with(&source, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&source, &source_sync, &key);
    scan_ready(&source, &source_sync, &key); // Repeated scans deduplicate the withdrawal.
    let deleted = accept_one(&source_sync, 2, 2);
    assert_eq!(deleted.operation, "delete");
    assert_eq!(deleted.object_id, original.object_id);
    assert!(deleted.payload.is_none());
    receive(&source, &source_sync, Some(&key), &deleted); // Its own pull echo is only an ACK.
    receive(&target, &target_sync, Some(&key), &deleted);
    assert!(list_with(&target, Some(&key)).unwrap().is_empty());
    let kept = resolve_one(&source, Some(&key), "github_pat").unwrap();
    assert_eq!(kept.value, FAKE);
    assert!(!kept.meta.sync_enabled);
    for sync in [&source_sync, &target_sync] {
        assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_secret_server", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    scan_ready(&source, &source_sync, &key);
    scan_ready(&target, &target_sync, &key);
    assert!(load_pending_outbox_from_db(&source_sync, 100).unwrap().is_empty());
    assert!(load_pending_outbox_from_db(&target_sync, 100).unwrap().is_empty());

    update_with(&source, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(true), ..Default::default() }).unwrap();
    scan_ready(&source, &source_sync, &key);
    let enabled = accept_one(&source_sync, 1, 3);
    assert_eq!(enabled.operation, "upsert");
    assert!(!serde_json::to_string(&enabled).unwrap().contains(FAKE));
    receive(&target, &target_sync, Some(&key), &enabled);
    assert_eq!(resolve_one(&target, Some(&key), "github_pat").unwrap().value, FAKE);
}

#[test]
fn test_vault_reenable_before_withdrawal_ack_keeps_one_sync_identity() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[2; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    let original = accept_one(&sync, 1, 1);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(true), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    let deleted = accept_one(&sync, 2, 2);
    assert_eq!(deleted.operation, "delete");
    assert!(find_local_id_by_object_id(&sync, OBJECT_TYPE, &original.object_id).unwrap().is_some());
    let restored = accept_one(&sync, 3, 3);
    assert_eq!(restored.operation, "upsert");
    assert_eq!(restored.object_id, original.object_id);
    scan_ready(&data, &sync, &key);
    assert!(load_pending_outbox_from_db(&sync, 100).unwrap().is_empty(), "no duplicate identity is uploaded");
    assert!(resolve_one(&data, Some(&key), "github_pat").unwrap().meta.sync_enabled);
}

#[test]
fn test_vault_reenable_matching_old_payload_still_restores_tombstoned_identity() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[2; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    let original = accept_one(&sync, 1, 1);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(true), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    accept_one(&sync, 2, 2); // Mapping is retained for the queued restore.
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    reconcile_withdrawals(&sync, &data).unwrap(); // Cancels that still-unsent restore.
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(true), ..Default::default() }).unwrap();
    data.execute("UPDATE vault_item SET updated_at=?1", [meta.updated_at]).unwrap(); // Same payload/hash, including millisecond collisions.
    scan_ready(&data, &sync, &key);
    let events = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1.operation, "upsert");
    assert_eq!(events[0].1.object_id, original.object_id);
    assert_eq!(events[0].1.base_version, Some(2));
}

#[test]
fn test_vault_disable_sync_drops_never_sent_upload() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::random_local_key();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    assert_eq!(load_pending_outbox_from_db(&sync, 100).unwrap().len(), 1);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    assert!(load_pending_outbox_from_db(&sync, 100).unwrap().is_empty());
    assert_eq!(resolve_one(&data, Some(&key), "github_pat").unwrap().value, FAKE);
}

/// An uncertain upload must keep its immutable event for an idempotent retry; its queued withdrawal
/// waits for that response and picks up the accepted server version before being sent.
#[test]
fn test_vault_disable_sync_withdraws_after_inflight_upload_recovers() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[7; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    let events = load_pending_outbox_from_db(&sync, 100).unwrap();
    mark_events_pushing_in_db(&sync, &events).unwrap();
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    assert!(load_pending_outbox_from_db(&sync, 100).unwrap().is_empty(), "delete waits for uncertain upload");
    record_event_failure(&sync, events[0].0, "test response lost").unwrap();
    sync.execute("UPDATE sync_outbox SET next_retry_at=NULL", []).unwrap();
    let recovered = accept_one(&sync, 1, 1);
    assert_eq!(recovered.event_id, events[0].1.event_id);
    assert_eq!(recovered.payload, events[0].1.payload);
    let withdrawal = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(withdrawal.len(), 1);
    assert_eq!(withdrawal[0].1.operation, "delete");
    assert_eq!(withdrawal[0].1.base_version, Some(1));
    accept_one(&sync, 2, 2);
    assert_eq!(resolve_one(&data, Some(&key), "github_pat").unwrap().value, FAKE);
}

#[test]
fn test_vault_disable_sync_survives_remote_update_before_scan() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[6; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    let mut remote = accept_one(&sync, 1, 1);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    remote.event_id = Uuid::new_v4().to_string();
    remote.version = 2;
    remote.seq = 2;
    remote.payload.as_mut().unwrap()["fields"]["value"] = serde_json::from_str(&seal_value(&key, &meta.id, "remote-new").unwrap()).unwrap();
    receive(&data, &sync, Some(&key), &remote);
    let kept = resolve_one(&data, Some(&key), "github_pat").unwrap();
    assert!(!kept.meta.sync_enabled);
    assert_eq!(kept.value, FAKE);
    scan_ready(&data, &sync, &key);
    let events = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1.operation, "delete");
    assert_eq!(events[0].1.base_version, Some(2));
}

#[test]
fn test_vault_local_only_copy_survives_concurrent_delete_and_resurrected_identity() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[5; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    let original = accept_one(&sync, 1, 1);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    let tombstone = PullChange { event_id: Uuid::new_v4().to_string(), operation: "delete".into(),
        version: 2, seq: 2, payload: None, deleted_at: Some(now_string()), ..original.clone() };
    receive(&data, &sync, None, &tombstone); // No passphrase needed to preserve the opt-out.
    assert!(find_local_id_by_object_id(&sync, OBJECT_TYPE, &original.object_id).unwrap().is_none());
    assert_eq!(resolve_one(&data, Some(&key), "github_pat").unwrap().value, FAKE);
    let newer = PullChange { event_id: Uuid::new_v4().to_string(), version: 3, seq: 3, ..original };
    receive(&data, &sync, Some(&key), &newer); // Matched by entry UUID, not the cleared sync mapping.
    assert!(!resolve_one(&data, Some(&key), "github_pat").unwrap().meta.sync_enabled);
    scan_ready(&data, &sync, &key);
    let events = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1.operation, "delete");
    assert_eq!(events[0].1.base_version, Some(3));
}

#[test]
fn test_vault_withdrawal_supersedes_locked_receiver_pending_copy() {
    let (source, target) = (db(), db());
    let (source_sync, target_sync) = (sync_db(), sync_db());
    let key = secrets::derive_key("passphrase", &[4; 16], 1000).unwrap();
    let meta = create(&source, &key, "github_pat", FAKE);
    scan_ready(&source, &source_sync, &key);
    let original = accept_one(&source_sync, 1, 1);
    receive(&target, &target_sync, None, &original);
    assert_eq!(secrets::pending_count(&target_sync), 1);
    update_with(&source, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&source, &source_sync, &key);
    receive(&target, &target_sync, None, &accept_one(&source_sync, 2, 2));
    secrets::replay_pending(&target_sync, |change| {
        receive(&target, &target_sync, Some(&key), change);
        Ok(())
    }).unwrap();
    assert_eq!(secrets::pending_count(&target_sync), 0);
    assert!(list_with(&target, Some(&key)).unwrap().is_empty());
}

#[test]
fn test_vault_dispatch_rechecks_opt_out_after_snapshot_and_batch_capture() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[3; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    accept_one(&sync, 1, 1);
    replace_secret_with(&data, &key, &meta.id, &SecretValue::new("replacement")).unwrap();
    let stale_snapshot = sealed_snapshots(&data, &sync, &key).remove(0);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    // The background scan resumes after the settings write, then an old batch is dispatched.
    enqueue_snapshot_if_changed(&sync, &stale_snapshot, "source").unwrap();
    let stale_batch = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(stale_batch[0].1.operation, "upsert");
    assert!(prepare_vault_push_batch_in_db(&sync, &data, &stale_batch).unwrap().is_empty());
    let next = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].1.operation, "delete");
    assert!(next[0].1.payload.is_none());
    assert_eq!(resolve_one(&data, Some(&key), "github_pat").unwrap().value, "replacement");
}

#[test]
fn test_vault_opt_out_during_dispatched_request_withdraws_after_ack() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[3; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    let events = load_pending_outbox_from_db(&sync, 100).unwrap();
    let prepared = prepare_vault_push_batch_in_db(&sync, &data, &events).unwrap();
    assert_eq!(prepared.len(), 1);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    let (id, event) = &prepared[0];
    consume_accepted_event(&sync, *id, event, &AcceptedEvent {
        event_id: event.event_id.clone(), object_type: event.object_type.clone(),
        object_id: event.object_id.clone(), server_version: 1, server_seq: 1,
    }).unwrap();
    // Same recheck as load_pending_outbox, without waiting for another whole sync round.
    reconcile_withdrawals(&sync, &data).unwrap();
    let next = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].1.operation, "delete");
    assert_eq!(next[0].1.base_version, Some(1));
}

#[test]
fn test_vault_withdrawal_drops_permanently_rejected_upload_but_not_unknown_outcome() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[3; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    accept_one(&sync, 1, 1);
    replace_secret_with(&data, &key, &meta.id, &SecretValue::new("replacement")).unwrap();
    scan_ready(&data, &sync, &key);
    let events = load_pending_outbox_from_db(&sync, 100).unwrap();
    mark_events_pushing_in_db(&sync, &events).unwrap();
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    sync.execute("UPDATE sync_outbox SET status='failed',outcome_known=0 WHERE id=?1", [events[0].0]).unwrap();
    reconcile_withdrawals(&sync, &data).unwrap();
    assert!(load_pending_outbox_from_db(&sync, 100).unwrap().is_empty());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox WHERE id=?1", [events[0].0], |r| r.get::<_, i64>(0)).unwrap(), 1);
    // A definite rejection arrives; the already-queued withdrawal must now be allowed through.
    sync.execute("UPDATE sync_outbox SET outcome_known=1 WHERE id=?1", [events[0].0]).unwrap();
    reconcile_withdrawals(&sync, &data).unwrap();
    let next = load_pending_outbox_from_db(&sync, 100).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].1.operation, "delete");
    assert_eq!(next[0].1.base_version, Some(1));
}

#[test]
fn test_vault_late_snapshot_cannot_restore_item_behind_inflight_withdrawal() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::derive_key("passphrase", &[3; 16], 1000).unwrap();
    let meta = create(&data, &key, "github_pat", FAKE);
    scan_ready(&data, &sync, &key);
    accept_one(&sync, 1, 1);
    let stale_snapshot = sealed_snapshots(&data, &sync, &key).remove(0);
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    scan_ready(&data, &sync, &key);
    let deletes = load_pending_outbox_from_db(&sync, 100).unwrap();
    mark_events_pushing_in_db(&sync, &deletes).unwrap();
    enqueue_snapshot_if_changed(&sync, &stale_snapshot, "source").unwrap();
    reconcile_withdrawals(&sync, &data).unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox WHERE operation='upsert'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox WHERE operation='delete'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn test_vault_opt_out_exception_does_not_tombstone_other_filtered_rows() {
    let (data, sync) = (db(), sync_db());
    let key = secrets::random_local_key();
    let meta = create(&data, &key, "github_pat", FAKE);
    let mut other = spec();
    other.object_type = "test.filtered";
    let (snapshots, _) = read_local_snapshots_from_db(&data, &other, &sync,
        &mut Default::default(), |_, _| Ok(true)).unwrap();
    let snapshot = &snapshots[0];
    let mut change = upsert_change(snapshot);
    change.object_type = other.object_type.into();
    upsert_shadow_from_remote(&sync, &change).unwrap();
    update_with(&data, Some(&key), &meta.id,
        &UpdateInput { sync_enabled: Some(false), ..Default::default() }).unwrap();
    assert!(local_row_exists_in_db(&sync, &data, &other, &snapshot.object_id).unwrap());
    assert_eq!(scan_local_deletes_in_db(&sync, &other, &HashSet::new(),
        |id| local_row_exists_in_db(&sync, &data, &other, id)).unwrap(), 0);
}

#[test]
fn test_config_binding_replaces_previous_target_of_same_source() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "openai_key", FAKE);
    assert!(bind_config_with(&db, &meta.id, "provider:3", "https://api.op", "Provider「OpenAI」", "https://api.op").unwrap());
    // The endpoint autosaves while typed: the final value replaces the intermediate one.
    assert!(bind_config_with(&db, &meta.id, "provider:3", "https://api.openai.com", "Provider「OpenAI」", "https://api.openai.com").unwrap());
    assert!(!bind_config_with(&db, &meta.id, "provider:3", "https://api.openai.com", "Provider「OpenAI」", "https://api.openai.com").unwrap());
    assert!(bind_config_with(&db, &meta.id, "mcp:7", "program:abc", "MCP「fs」", "本机程序 npx").unwrap());
    let resolved = resolve_ids_with(&db, Some(&key), &[meta.id.clone()]).unwrap();
    let targets: Vec<_> = resolved[&meta.id].config_bindings.iter().map(|b| b.target.as_str()).collect();
    assert_eq!(targets, vec!["https://api.openai.com", "program:abc"]);
    assert_eq!(resolved[&meta.id].value, FAKE);
    // Metadata listing never decrypts and never carries the value.
    let metas = metas_with(&db, &[meta.id.clone(), "missing".into()]).unwrap();
    assert_eq!(metas.len(), 1);
    assert!(!format!("{:?}", metas[&meta.id]).contains(FAKE));
    assert_eq!(resolve_ids_with(&db, Some(&key), &["missing".into()]).unwrap_err().code, "credential_not_found");
}

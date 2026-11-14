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

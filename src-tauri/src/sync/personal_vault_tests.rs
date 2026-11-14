use super::*;

const FAKE: &str = "aipp-fake-token-7f3a91";

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    init(&db).unwrap();
    db
}

fn bearer(token: &str) -> AuthInput {
    AuthInput { kind: AuthKind::Bearer, token: Some(token.into()), username: None, password: None, headers: vec![], query: vec![] }
}

fn create(db: &Connection, key: &secrets::SecretKey, handle: &str, token: &str) -> ItemMeta {
    create_with(db, key, &CreateInput {
        name: format!("{handle} 名称"), handle: handle.into(), description: "测试".into(),
        auth: bearer(token), allowed_sites: vec![], sync_enabled: true,
    }).unwrap()
}

#[test]
fn test_vault_create_resolve_roundtrip() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    assert_eq!(meta.status, "ready");
    assert_eq!(meta.injection, vec!["Authorization: Bearer <值>".to_string()]);
    let resolved = resolve_with(&db, Some(&key), "github_pat").unwrap();
    assert_eq!(resolved.fields.get("token").map(String::as_str), Some(FAKE));
    assert!(matches!(&resolved.rules[..], [InjectionRule::Header { name, prefix, .. }] if name == "Authorization" && prefix == "Bearer "));
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
    let resolved = resolve_with(&db, Some(&key), "github_pat").unwrap();
    assert!(!format!("{resolved:?}").contains(FAKE));
}

#[test]
fn test_vault_ciphertext_bound_to_item() {
    let db = db();
    let key = secrets::random_local_key();
    create(&db, &key, "first", "secret-one");
    create(&db, &key, "second", "secret-two");
    db.execute("UPDATE vault_item SET value=(SELECT value FROM vault_item WHERE handle='first') WHERE handle='second'", []).unwrap();
    let error = resolve_with(&db, Some(&key), "second").unwrap_err();
    assert_eq!(error.code, "vault_locked");
    assert_eq!(list_with(&db, Some(&key)).unwrap().iter().find(|m| m.handle == "second").unwrap().status, "locked");
}

#[test]
fn test_vault_reencrypt_on_key_switch() {
    let db = db();
    let old = secrets::random_local_key();
    let new = secrets::random_local_key();
    create(&db, &old, "github_pat", FAKE);
    reencrypt(&db, Some(&old), &new).unwrap();
    assert_eq!(resolve_with(&db, Some(&new), "github_pat").unwrap().fields["token"], FAKE);
    assert_eq!(resolve_with(&db, Some(&old), "github_pat").unwrap_err().code, "vault_locked");
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
    assert_eq!(resolve_with(&db, Some(&key), "missing").unwrap_err().code, "credential_not_found");
    create(&db, &key, "github_pat", FAKE);
    assert_eq!(resolve_with(&db, None, "github_pat").unwrap_err().code, "vault_locked");
    // Two devices created the same handle and both synced in: refuse instead of guessing.
    db.execute("INSERT INTO vault_item(uuid, handle, name, auth_kind, injection, value, created_at, updated_at)
        SELECT 'dup', handle, name, auth_kind, injection, value, created_at, updated_at FROM vault_item", []).unwrap();
    assert_eq!(resolve_with(&db, Some(&key), "github_pat").unwrap_err().code, "credential_ambiguous");
}

#[test]
fn test_vault_handle_validation_and_uniqueness() {
    let db = db();
    let key = secrets::random_local_key();
    for bad in ["", "a", "GitHub", "1abc", "has-dash", "中文"] { assert!(validate_handle(bad).is_err(), "{bad}"); }
    create(&db, &key, "github_pat", FAKE);
    let duplicate = create_with(&db, &key, &CreateInput {
        name: "x".into(), handle: "github_pat".into(), description: String::new(),
        auth: bearer("other"), allowed_sites: vec![], sync_enabled: true,
    });
    assert!(duplicate.unwrap_err().contains("已被使用"));
}

#[test]
fn test_vault_empty_secret_rejected() {
    let db = db();
    let key = secrets::random_local_key();
    let result = create_with(&db, &key, &CreateInput {
        name: "x".into(), handle: "empty".into(), description: String::new(),
        auth: bearer("   "), allowed_sites: vec![], sync_enabled: true,
    });
    assert!(result.is_err());
}

#[test]
fn test_vault_auth_kinds_build_rules() {
    let basic = AuthInput { kind: AuthKind::Basic, token: None, username: Some("me".into()), password: Some("pw".into()), headers: vec![], query: vec![] };
    let (rules, fields) = build_auth(&basic).unwrap();
    assert_eq!(rules, vec![InjectionRule::Basic]);
    assert_eq!(fields.len(), 2);

    let headers = AuthInput { kind: AuthKind::Header, token: None, username: None, password: None, query: vec![],
        headers: vec![
            NamedSecret { name: "X-Algolia-Application-Id".into(), prefix: String::new(), value: "app".into() },
            NamedSecret { name: "X-Algolia-API-Key".into(), prefix: String::new(), value: "key".into() },
        ] };
    let (rules, _) = build_auth(&headers).unwrap();
    assert_eq!(rules.len(), 2);

    let duplicate = AuthInput { kind: AuthKind::Header, token: None, username: None, password: None, query: vec![],
        headers: vec![
            NamedSecret { name: "X-Key".into(), prefix: String::new(), value: "a".into() },
            NamedSecret { name: "x-key".into(), prefix: String::new(), value: "b".into() },
        ] };
    assert!(build_auth(&duplicate).is_err());

    let bad_name = AuthInput { kind: AuthKind::Header, token: None, username: None, password: None, query: vec![],
        headers: vec![NamedSecret { name: "Bad Name".into(), prefix: String::new(), value: "a".into() }] };
    assert!(build_auth(&bad_name).is_err());

    let query = AuthInput { kind: AuthKind::Query, token: None, username: None, password: None, headers: vec![],
        query: vec![NamedSecret { name: "appid".into(), prefix: String::new(), value: "q".into() }] };
    let (rules, _) = build_auth(&query).unwrap();
    assert!(matches!(&rules[..], [InjectionRule::Query { name, .. }] if name == "appid"));
}

#[test]
fn test_vault_basic_secret_values_include_pair() {
    let db = db();
    let key = secrets::random_local_key();
    create_with(&db, &key, &CreateInput {
        name: "nas".into(), handle: "nas".into(), description: String::new(),
        auth: AuthInput { kind: AuthKind::Basic, token: None, username: Some("me".into()), password: Some("pw".into()), headers: vec![], query: vec![] },
        allowed_sites: vec![], sync_enabled: true,
    }).unwrap();
    let resolved = resolve_with(&db, Some(&key), "nas").unwrap();
    assert!(resolved.secret_values().contains(&"me:pw".to_string()));
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
    let listed = list_with(&db, Some(&key)).unwrap();
    assert_eq!(listed[0].allowed_sites, vec!["https://api.github.com".to_string()]);
}

#[test]
fn test_vault_update_and_replace_secret() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    let updated = update_with(&db, Some(&key), &meta.id, &UpdateInput {
        handle: Some("gh".into()), allowed_sites: Some(vec!["api.github.com".into()]), ..Default::default()
    }).unwrap();
    assert_eq!(updated.handle, "gh");
    assert_eq!(updated.allowed_sites, vec!["https://api.github.com".to_string()]);
    replace_secret_with(&db, &key, &meta.id, &bearer("new-token")).unwrap();
    assert_eq!(resolve_with(&db, Some(&key), "gh").unwrap().fields["token"], "new-token");
}

#[test]
fn test_vault_delete_removes_item_and_usage() {
    let db = db();
    let key = secrets::random_local_key();
    let meta = create(&db, &key, "github_pat", FAKE);
    log_usage_with(&db, &NewUsage { item_id: &meta.id, conversation_id: Some(1), assistant_id: Some(2), method: "GET",
        url: "https://api.github.com/user", status: Some(200), outcome: "ok", reason: None }).unwrap();
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
    for _ in 0..(USAGE_KEEP_PER_ITEM + 5) {
        log_usage_with(&db, &NewUsage { item_id: &meta.id, conversation_id: None, assistant_id: None, method: "GET",
            url: "https://api.github.com/user", status: Some(200), outcome: "ok", reason: None }).unwrap();
    }
    let count: i64 = db.query_row("SELECT COUNT(*) FROM vault_usage_log", [], |r| r.get(0)).unwrap();
    assert_eq!(count, USAGE_KEEP_PER_ITEM);
    assert!(list_with(&db, Some(&key)).unwrap()[0].last_used_at.is_some());
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
    log_usage_with(&source, &NewUsage { item_id: &meta.id, conversation_id: None, assistant_id: None, method: "GET",
        url: "https://api.github.com/", status: Some(200), outcome: "ok", reason: None }).unwrap();
    let sealed = sealed_snapshots(&source, &source_sync, &key).remove(0);
    assert!(!sealed.payload_json.contains(FAKE), "wire payload must not contain the secret");
    assert!(!sealed.payload_json.contains("last_used_at"), "local usage time is not synced");
    let change = upsert_change(&sealed);
    secrets::with_state(Some(secrets::SecretState::Ready(key.clone())),
        || apply_change_to_db(&target_sync, &target, &spec(), &change, "target", |_| Ok(()))).unwrap();
    assert_eq!(resolve_with(&target, Some(&key), "github_pat").unwrap().fields["token"], FAKE);
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
    assert_eq!(resolve_with(&target, Some(&key), "github_pat").unwrap().fields["token"], FAKE);
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
    assert_eq!(meta.fields, vec![FieldMeta { key: "token".into(), label: "令牌".into() }]);
    assert!(bind_config_with(&db, &meta.id, "provider:3", "https://api.op", "Provider「OpenAI」", "https://api.op").unwrap());
    // The endpoint autosaves while typed: the final value replaces the intermediate one.
    assert!(bind_config_with(&db, &meta.id, "provider:3", "https://api.openai.com", "Provider「OpenAI」", "https://api.openai.com").unwrap());
    assert!(!bind_config_with(&db, &meta.id, "provider:3", "https://api.openai.com", "Provider「OpenAI」", "https://api.openai.com").unwrap());
    assert!(bind_config_with(&db, &meta.id, "mcp:7", "program:abc", "MCP「fs」", "本机程序 npx").unwrap());
    let resolved = resolve_ids_with(&db, Some(&key), &[meta.id.clone()]).unwrap();
    let targets: Vec<_> = resolved[&meta.id].config_bindings.iter().map(|b| b.target.as_str()).collect();
    assert_eq!(targets, vec!["https://api.openai.com", "program:abc"]);
    assert_eq!(resolved[&meta.id].fields.get("token").map(String::as_str), Some(FAKE));
    // Metadata listing never decrypts and never carries the value.
    let metas = metas_with(&db, &[meta.id.clone(), "missing".into()]).unwrap();
    assert_eq!(metas.len(), 1);
    assert!(!format!("{:?}", metas[&meta.id]).contains(FAKE));
    assert_eq!(resolve_ids_with(&db, Some(&key), &["missing".into()]).unwrap_err().code, "credential_not_found");
}

#[test]
fn test_field_labels_for_each_auth_kind() {
    let db = db();
    let key = secrets::random_local_key();
    let basic = create_with(&db, &key, &CreateInput {
        name: "NAS".into(), handle: "nas".into(), description: String::new(),
        auth: AuthInput { kind: AuthKind::Basic, token: None, username: Some("admin".into()), password: Some("pw-123456".into()), headers: vec![], query: vec![] },
        allowed_sites: vec![], sync_enabled: false,
    }).unwrap();
    assert_eq!(basic.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["username", "password"]);
    let header = create_with(&db, &key, &CreateInput {
        name: "Weather".into(), handle: "weather".into(), description: String::new(),
        auth: AuthInput { kind: AuthKind::Header, token: None, username: None, password: None,
            headers: vec![NamedSecret { name: "X-Api-Key".into(), prefix: String::new(), value: "k-123456".into() }], query: vec![] },
        allowed_sites: vec![], sync_enabled: false,
    }).unwrap();
    assert_eq!(header.fields, vec![FieldMeta { key: "header_0".into(), label: "X-Api-Key".into() }]);
}

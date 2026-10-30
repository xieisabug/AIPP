//! Encrypted API-key sync, same-name provider linking and duplicate repair,
//! exercised through the production scan/outbox/apply code on in-memory DBs.
use super::*;

const TYPES: &[&str] = &[
    "llm.provider", "llm.provider_config", "llm.provider_secret", "llm.model",
    "assistant", "assistant.model", "assistant.model_config", "system.feature_config",
];

struct KDev {
    sync: Connection,
    data: Connection,
    system: Connection,
    master: [u8; 32],
    id: &'static str,
}

impl KDev {
    fn new(id: &'static str) -> Self {
        let db = crate::db::assistant_db::AssistantDatabase {
            conn: Connection::open_in_memory().unwrap(),
            mcp_conn: Connection::open_in_memory().unwrap(),
        };
        db.create_tables().unwrap();
        db.conn.execute_batch("
            CREATE TABLE llm_provider(id INTEGER PRIMARY KEY, name TEXT NOT NULL, api_type TEXT NOT NULL DEFAULT 'openai_api',
                description TEXT, is_official INTEGER NOT NULL DEFAULT 0, is_enabled INTEGER NOT NULL DEFAULT 1);
            CREATE TABLE llm_provider_config(id INTEGER PRIMARY KEY, name TEXT NOT NULL, llm_provider_id INTEGER NOT NULL,
                value TEXT, append_location TEXT DEFAULT 'header', is_addition INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE llm_model(id INTEGER PRIMARY KEY, name TEXT, llm_provider_id INTEGER NOT NULL, code TEXT NOT NULL,
                description TEXT, vision_support INTEGER DEFAULT 0, audio_support INTEGER DEFAULT 0, video_support INTEGER DEFAULT 0,
                UNIQUE(llm_provider_id, code));
            CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT, content TEXT, llm_model_id INTEGER);
            INSERT INTO llm_provider(id, name, api_type, is_official) VALUES (1, 'OpenAI', 'openai_api', 1);").unwrap();
        let system = Connection::open_in_memory().unwrap();
        system.execute_batch("
            CREATE TABLE feature_config(id INTEGER PRIMARY KEY AUTOINCREMENT, feature_code TEXT NOT NULL, key TEXT NOT NULL,
                value TEXT, data_type TEXT, description TEXT, UNIQUE(feature_code, key));
            CREATE TABLE secure_config(scope TEXT NOT NULL, key TEXT NOT NULL, ciphertext TEXT NOT NULL, nonce TEXT NOT NULL,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY(scope, key));").unwrap();
        let sync = Connection::open_in_memory().unwrap();
        sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
        Self { sync, data: db.conn, system, master: rand::random(), id }
    }

    fn conn(&self, spec: &SyncTableSpec) -> &Connection {
        if spec.db_name == "system.db" { &self.system } else { &self.data }
    }

    fn state(&self) -> secrets::SecretState {
        secrets::state_from_db(&self.system, &self.master).unwrap()
    }

    /// Mirrors set_sync_passphrase: unlocking an existing record is server-wins.
    fn passphrase(&self, value: &str, replace: bool) -> Result<(), String> {
        let server_wins = secrets::read_record(&self.system)?.is_some() && !replace;
        let was_set = secrets::server_wins_pending(&self.sync);
        if server_wins { secrets::begin_server_wins(&self.sync)?; }
        let result = secrets::set_passphrase(&self.system, &self.master, value, replace).map(|_| ());
        if result.is_err() && server_wins && !was_set { secrets::cancel_server_wins(&self.sync)?; }
        result?;
        self.replay_secrets();
        Ok(())
    }

    fn lock(&self) {
        self.system.execute("DELETE FROM secure_config WHERE scope='data_sync' AND key='secret_key'", []).unwrap();
    }

    /// Mirrors scan_local_changes (repairs, sealing, delete detection).
    fn scan(&self) -> Vec<LocalObjectSnapshot> {
        provider_merge::finish_redirects(&self.sync, Some(&self.data), Some(&self.data), Some(&self.system)).unwrap();
        provider_merge::dedupe_provider_configs(&self.sync, &self.data).unwrap();
        quick_assistant::prepare(&self.sync, &self.data).unwrap();
        let state = self.state();
        let mut all = Vec::new();
        for spec in specs().into_iter().filter(|s| TYPES.contains(&s.object_type)) {
            let key = if spec.object_type == secrets::OBJECT_TYPE {
                let Some(key) = state.ready_key() else { continue };
                if secrets::server_wins_pending(&self.sync) { continue; }
                Some(key.clone())
            } else { None };
            let (rows, present) = read_local_snapshots_from_db(self.conn(&spec), &spec, &self.sync,
                &mut DanglingFkScanReport::default(), |kind, id| {
                    let dep = specs().into_iter().find(|s| s.object_type == kind).unwrap();
                    row_exists_by_id(self.conn(&dep), &dep, id)
                }).unwrap();
            let rows = match key { Some(key) => secrets::seal_snapshots(&self.sync, &key, rows).unwrap(), None => rows };
            for row in &rows { enqueue_snapshot_if_changed(&self.sync, row, self.id).unwrap(); }
            scan_local_deletes_in_db(&self.sync, &spec, &present, |object_id| {
                local_row_exists_in_db(&self.sync, self.conn(&spec), &spec, object_id)
            }).unwrap();
            all.extend(rows);
        }
        all
    }

    fn apply(&self, change: &PullChange) -> Result<(), String> {
        let spec = specs().into_iter().find(|s| s.object_type == change.object_type).unwrap();
        let state = (spec.object_type == secrets::OBJECT_TYPE).then(|| self.state());
        secrets::with_state(state, || {
            apply_change_to_db(&self.sync, self.conn(&spec), &spec, change, self.id, |_| Ok(()))
        })
    }

    /// Mirrors replay_secret_pending: parked changes, then first-unlock server-wins.
    fn replay_secrets(&self) -> usize {
        let state = self.state();
        let Some(key) = state.ready_key() else { return 0 };
        let mut applied = secrets::replay_pending(&self.sync, |change| self.apply(change)).unwrap().0;
        if secrets::server_wins_pending(&self.sync) {
            applied += secrets::reconcile_server_wins(&self.sync, key, |change| self.apply(change)).unwrap();
        }
        applied
    }

    fn key_of(&self, provider: i64) -> Option<String> {
        self.data.query_row("SELECT value FROM llm_provider_config WHERE llm_provider_id=?1 AND name='api_key'",
            [provider], |r| r.get(0)).optional().unwrap()
    }

    fn pending_upserts(&self) -> Vec<PushEvent> {
        load_pending_outbox_from_db(&self.sync, 1000).unwrap().into_iter().map(|(_, e)| e).collect()
    }
}

/// Minimal server with the production conflict rule: stale base_version → conflict,
/// resolved client-side as "server wins" (same as handle_push_response).
#[derive(Default)]
struct MiniServer {
    objects: HashMap<(String, String), (i64, Option<JsonValue>, bool)>,
    log: Vec<PullChange>,
    /// Exactly what left each device (what the server and its operator can see).
    received: Vec<String>,
}

impl MiniServer {
    fn push(&mut self, d: &KDev) {
        {
            let events = load_pending_outbox_from_db(&d.sync, 1000).unwrap();
            mark_events_pushing_in_db(&d.sync, &events).unwrap();
            for (row_id, e) in events {
                self.received.push(serde_json::to_string(&e).unwrap());
                let key = (e.object_type.clone(), e.object_id.clone());
                let current = self.objects.get(&key).cloned();
                if let Some((version, payload, deleted)) = current.clone() {
                    if e.base_version.unwrap_or(0) < version {
                        let conflict = PullChange {
                            seq: 0, event_id: e.event_id.clone(), device_id: "server-conflict".into(),
                            object_type: e.object_type.clone(), object_id: e.object_id.clone(),
                            operation: if deleted { "delete".into() } else { "upsert".into() },
                            version, payload, deleted_at: None, created_at: "now".into(),
                        };
                        match d.apply(&conflict) {
                            Ok(()) => { d.sync.execute("DELETE FROM sync_outbox WHERE id=?1", [row_id]).unwrap(); }
                            // Same as production: keep the event as failed; the pull delivers it later.
                            Err(err) => record_event_failure(&d.sync, row_id, &err).unwrap(),
                        }
                        continue;
                    }
                }
                let version = current.map(|c| c.0 + 1).unwrap_or(1);
                let seq = self.log.len() as i64 + 1;
                self.objects.insert(key, (version, e.payload.clone(), e.operation == "delete"));
                self.log.push(PullChange {
                    seq, event_id: e.event_id.clone(), device_id: d.id.into(), object_type: e.object_type.clone(),
                    object_id: e.object_id.clone(), operation: e.operation.clone(), version, payload: e.payload.clone(),
                    deleted_at: (e.operation == "delete").then(|| "now".to_string()), created_at: "now".into(),
                });
                consume_accepted_event(&d.sync, row_id, &e, &AcceptedEvent {
                    event_id: e.event_id.clone(), object_type: e.object_type.clone(), object_id: e.object_id.clone(),
                    server_version: version, server_seq: seq,
                }).unwrap();
            }
        }
    }

    fn pull(&self, d: &KDev, cursor: &mut i64) {
        let page: Vec<PullChange> = self.log.iter().filter(|c| c.seq > *cursor).cloned().collect();
        apply_pull_page(&d.sync, &page, |c| d.apply(c)).unwrap();
        *cursor = self.log.len() as i64;
    }

    /// One production-ordered round: replay parked → scan → push → pull → replay.
    fn round(&mut self, d: &KDev, cursor: &mut i64) {
        d.replay_secrets();
        provider_merge::auto_merge(&d.sync, &d.data).unwrap();
        d.scan();
        self.push(d);
        self.pull(d, cursor);
        d.replay_secrets();
        provider_merge::auto_merge(&d.sync, &d.data).unwrap();
    }
}

fn seed_custom(d: &KDev, id: i64, name: &str, key: Option<&str>) {
    d.data.execute("INSERT INTO llm_provider(id, name, api_type) VALUES (?1, ?2, 'openai_api')", params![id, name]).unwrap();
    d.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (?1, 'endpoint', 'https://api.siliconflow.cn')", [id]).unwrap();
    if let Some(key) = key {
        d.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (?1, 'api_key', ?2)", params![id, key]).unwrap();
    }
    d.data.execute("INSERT INTO llm_model(llm_provider_id, name, code) VALUES (?1, 'Qwen', 'qwen-max')", [id]).unwrap();
}

fn quick_provider(d: &KDev) -> i64 {
    d.data.query_row("SELECT provider_id FROM assistant_model WHERE assistant_id=1", [], |r| r.get(0)).unwrap()
}

fn providers_named(d: &KDev, name: &str) -> i64 {
    d.data.query_row("SELECT COUNT(*) FROM llm_provider WHERE name=?1", [name], |r| r.get(0)).unwrap()
}

#[test]
fn test_secret_envelope_is_deterministic_bound_and_tamper_evident() {
    let key = secrets::derive_key("correct horse battery", b"0123456789abcdef", secrets::KDF_ITERATIONS).unwrap();
    let a = secrets::seal(&key, "obj-1", "api_key", "sk-live-123").unwrap();
    assert_eq!(a, secrets::seal(&key, "obj-1", "api_key", "sk-live-123").unwrap(), "stable payload hash, no echo");
    assert!(!a.to_string().contains("sk-live-123"));
    assert_eq!(secrets::open(&key, "obj-1", "api_key", &a).unwrap(), "sk-live-123");
    // Bound to object identity and config name: a server cannot relabel a ciphertext.
    assert_eq!(secrets::open(&key, "obj-2", "api_key", &a), Err(secrets::OpenError::Auth));
    assert_eq!(secrets::open(&key, "obj-1", "acp_env_vars", &a), Err(secrets::OpenError::Auth));
    let mut tampered = a.clone();
    tampered["ct"] = json!(BASE64.encode(b"garbage-garbage-garbage"));
    assert_eq!(secrets::open(&key, "obj-1", "api_key", &tampered), Err(secrets::OpenError::Auth));
    let other = secrets::derive_key("another passphrase", b"0123456789abcdef", secrets::KDF_ITERATIONS).unwrap();
    assert_eq!(secrets::open(&other, "obj-1", "api_key", &a), Err(secrets::OpenError::OtherKey));
    assert_eq!(secrets::open(&key, "obj-1", "api_key", &json!("sk-plaintext")), Err(secrets::OpenError::Malformed));
    assert!(format!("{key:?}").len() < 64, "Debug never prints key bytes");
}

#[test]
fn test_secret_sync_two_devices_roundtrip_edit_and_no_plaintext_on_server() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute_batch("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'endpoint', 'https://relay.example'), (1, 'api_key', 'sk-A-secret-1');
        INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'acp_env_vars', 'OPENAI_API_KEY=sk-env-secret');").unwrap();
    a.passphrase("same passphrase on both", false).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    assert!(server.objects.keys().any(|(t, _)| t == secrets::OBJECT_TYPE));
    for pushed in &server.received {
        assert!(!pushed.contains("sk-A-secret-1") && !pushed.contains("sk-env-secret"), "plaintext reached the server: {pushed}");
    }
    assert!(!server.received.iter().any(|p| p.contains("same passphrase on both")));

    // B has no passphrase yet: nothing is written, the change is parked, status says so.
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1), None);
    assert!(matches!(b.state(), secrets::SecretState::NeedsPassphrase));
    assert_eq!(secrets::pending_count(&b.sync), 2);
    assert!(b.state().message(0).contains("需要输入同步口令才能同步 API Key"));

    b.passphrase("same passphrase on both", false).unwrap();
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A-secret-1"));
    assert_eq!(b.data.query_row("SELECT value FROM llm_provider_config WHERE name='acp_env_vars'", [], |r| r.get::<_, String>(0)).unwrap(), "OPENAI_API_KEY=sk-env-secret");
    assert_eq!(secrets::pending_count(&b.sync), 0);
    assert!(b.pending_upserts().is_empty(), "applying a remote secret must not echo it back");

    // Rotation of the key value on B propagates to A; still only ciphertext leaves B.
    b.data.execute("UPDATE llm_provider_config SET value='sk-B-rotated' WHERE name='api_key'", []).unwrap();
    server.round(&b, &mut cb);
    server.round(&a, &mut ca);
    assert_eq!(a.key_of(1).as_deref(), Some("sk-B-rotated"));
    assert!(!server.received.iter().any(|p| p.contains("sk-B-rotated")));
    // Converged: no further uploads on either side.
    a.scan(); b.scan();
    assert!(a.pending_upserts().is_empty() && b.pending_upserts().is_empty());
}

#[test]
fn test_secret_sync_wrong_passphrase_never_overwrites_and_reports() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-A')", []).unwrap();
    b.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-B-local')", []).unwrap();
    a.passphrase("right passphrase", false).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    let err = b.passphrase("wrong passphrase", false).unwrap_err();
    assert!(err.starts_with(secrets::PASSPHRASE_MISMATCH_CODE), "{err}");
    assert!(matches!(b.state(), secrets::SecretState::NeedsPassphrase), "nothing stored for a wrong passphrase");
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-B-local"));
    assert!(!b.pending_upserts().iter().any(|e| e.object_type == secrets::OBJECT_TYPE));
    assert!(b.passphrase("short", false).unwrap_err().contains("至少需要"));

    // Passphrase changed elsewhere: B's old key no longer matches → paused, local key kept.
    b.passphrase("right passphrase", false).unwrap();
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A"), "remote first writer wins once the passphrase matches");
    a.passphrase("brand new passphrase", true).unwrap();
    a.data.execute("UPDATE llm_provider_config SET value='sk-A-2' WHERE name='api_key'", []).unwrap();
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    assert!(matches!(b.state(), secrets::SecretState::Mismatch));
    assert!(b.state().message(0).contains("同步口令已在其他设备更换"));
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A"), "mismatch must never write garbage or the new value");
    assert!(!b.pending_upserts().iter().any(|e| e.object_type == secrets::OBJECT_TYPE));
    b.passphrase("brand new passphrase", false).unwrap();
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A-2"));
}

#[test]
fn test_device_without_passphrase_keeps_local_key_and_never_uploads_it() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-A')", []).unwrap();
    b.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-B-only-local')", []).unwrap();
    a.passphrase("passphrase for a", false).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    for _ in 0..2 { server.round(&b, &mut cb); }
    assert_eq!(b.key_of(1).as_deref(), Some("sk-B-only-local"));
    assert!(!server.received.iter().any(|p| p.contains("sk-B-only-local")));
    assert!(!server.received.iter().any(|p| p.contains("\"device-b\"") && p.contains(secrets::OBJECT_TYPE)));
    // Nothing configured anywhere: secrets stay local, status explains it.
    let c = KDev::new("device-c");
    assert!(c.state().message(0).contains("API Key 不同步"));
}

#[test]
fn test_empty_unsynced_key_is_not_uploaded_over_another_devices_key() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    b.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', '')", []).unwrap();
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-A')", []).unwrap();
    b.passphrase("shared passphrase", false).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&b, &mut cb); // B first, with an empty key
    assert!(!server.objects.keys().any(|(t, _)| t == secrets::OBJECT_TYPE));
    server.round(&a, &mut ca);
    a.passphrase("shared passphrase", false).unwrap();
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    assert_eq!(a.key_of(1).as_deref(), Some("sk-A"));
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A"));
    // Clearing a key that was synced does propagate.
    a.data.execute("UPDATE llm_provider_config SET value='' WHERE name='api_key'", []).unwrap();
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some(""));
}

/// Root cause reproduction: A and B each created "SiliconFlow". Before the fix
/// B received A's identity as a second, keyless provider and the quick
/// assistant followed it → "API key wrong" on B.
#[test]
fn test_same_name_provider_links_and_quick_assistant_keeps_usable_key() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    seed_custom(&a, 5, "SiliconFlow", Some("sk-A"));
    seed_custom(&b, 9, "SiliconFlow", Some("sk-B"));
    a.data.execute("UPDATE assistant_model SET provider_id=5, model_code='qwen-max' WHERE assistant_id=1", []).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    server.round(&b, &mut cb);
    assert_eq!(providers_named(&b, "SiliconFlow"), 1, "linked, not duplicated");
    assert_eq!(quick_provider(&b), 9);
    assert_eq!(b.key_of(9).as_deref(), Some("sk-B"), "usable key on B without any passphrase");
    assert_eq!(b.data.query_row("SELECT COUNT(*) FROM llm_provider_config WHERE llm_provider_id=9 AND name='endpoint'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(b.data.query_row("SELECT COUNT(*) FROM llm_model WHERE llm_provider_id=9", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    // The reverse direction links too, and both sides settle without uploads.
    server.round(&a, &mut ca);
    assert_eq!(providers_named(&a, "SiliconFlow"), 1);
    assert_eq!(quick_provider(&a), 5);
    for _ in 0..2 { server.round(&a, &mut ca); server.round(&b, &mut cb); }
    a.scan(); b.scan();
    assert!(a.pending_upserts().is_empty() && b.pending_upserts().is_empty());

    // With the same passphrase the keys converge too (last writer by server order).
    a.passphrase("shared passphrase", false).unwrap();
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    b.passphrase("shared passphrase", false).unwrap();
    for _ in 0..2 { server.round(&b, &mut cb); server.round(&a, &mut ca); }
    assert_eq!(b.key_of(9), a.key_of(5));
    assert!(b.key_of(9).is_some_and(|k| !k.is_empty()));
    assert_eq!(quick_provider(&b), 9);
}

#[test]
fn test_same_device_duplicates_are_not_folded_on_receiver() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    seed_custom(&a, 5, "Relay", Some("sk-1"));
    seed_custom(&a, 6, "Relay", Some("sk-2")); // different key: a real second provider
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    assert_eq!(providers_named(&b, "Relay"), 2, "two distinct providers authored by one device stay distinct");
    assert_eq!(providers_named(&a, "Relay"), 2, "conflicting keys are never auto-merged");
    assert_eq!(provider_merge::find_key_conflicts(&a.data).unwrap(), vec!["Relay".to_string()]);
}

/// Official providers share an identity; endpoints used to be inserted as a
/// second row and the newest (remote) one overrode B's own endpoint.
#[test]
fn test_official_provider_endpoint_merges_instead_of_duplicating() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'endpoint', 'https://relay-a')", []).unwrap();
    b.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'endpoint', 'https://relay-b')", []).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    server.round(&a, &mut ca);
    for d in [&a, &b] {
        let rows: Vec<String> = d.data.prepare("SELECT value FROM llm_provider_config WHERE llm_provider_id=1 AND name='endpoint'").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(rows.len(), 1, "{}: {rows:?}", d.id);
    }
    let value = |d: &KDev| d.data.query_row("SELECT value FROM llm_provider_config WHERE name='endpoint'", [], |r| r.get::<_, String>(0)).unwrap();
    assert_eq!(value(&a), value(&b));
}

#[test]
fn test_legacy_duplicate_endpoint_rows_are_folded_without_tombstones() {
    let d = KDev::new("device-b");
    d.data.execute_batch("INSERT INTO llm_provider_config(id, llm_provider_id, name, value) VALUES (1, 1, 'endpoint', 'https://mine'), (2, 1, 'endpoint', 'https://remote')").unwrap();
    save_object_map(&d.sync, "llm.provider_config", "llm_provider_config", 1, "cfg-mine").unwrap();
    save_object_map(&d.sync, "llm.provider_config", "llm_provider_config", 2, "cfg-remote").unwrap();
    for id in ["cfg-mine", "cfg-remote"] {
        d.sync.execute("INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, updated_at, server_seq) VALUES ('llm.provider_config', ?1, 1, 'x', 'now', 1)", [id]).unwrap();
    }
    d.scan();
    assert_eq!(d.data.query_row("SELECT COUNT(*) FROM llm_provider_config", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(find_local_id_by_object_id(&d.sync, "llm.provider_config", "cfg-mine").unwrap(), Some(2));
    assert!(!d.pending_upserts().iter().any(|e| e.operation == "delete"), "folding must not delete the row on other devices");
}

/// Repair for devices that already hold a keyless duplicate from an older sync.
#[test]
fn test_merge_legacy_duplicate_provider_moves_references_without_tombstones() {
    let d = KDev::new("device-b");
    seed_custom(&d, 9, "SiliconFlow", Some("sk-B"));
    seed_custom(&d, 12, "SiliconFlow", None); // inserted by an old sync from device A
    let dup_model: i64 = d.data.query_row("SELECT id FROM llm_model WHERE llm_provider_id=12", [], |r| r.get(0)).unwrap();
    let keep_model: i64 = d.data.query_row("SELECT id FROM llm_model WHERE llm_provider_id=9", [], |r| r.get(0)).unwrap();
    d.data.execute("INSERT INTO llm_model(llm_provider_id, name, code) VALUES (12, 'Only on A', 'only-a')", []).unwrap();
    d.data.execute("UPDATE assistant_model SET provider_id=12, model_code='qwen-max' WHERE assistant_id=1", []).unwrap();
    d.data.execute("INSERT INTO message(id, conversation_id, message_type, content, llm_model_id) VALUES (1, 1, 'response', 'hi', ?1)", [dup_model]).unwrap();
    d.system.execute("INSERT INTO feature_config(feature_code, key, value) VALUES ('conversation_summary', 'title_provider_id', '12'), ('conversation_summary', 'title_model', 'qwen-max%%12')", []).unwrap();
    d.scan();
    let mut server = MiniServer::default();
    server.push(&d);
    let a_identity = find_object_id(&d.sync, "llm.provider", 12).unwrap().unwrap();

    let pairs = provider_merge::find_duplicates(&d.data).unwrap();
    assert_eq!(pairs, vec![provider_merge::DuplicateProviderPair { name: "SiliconFlow".into(), keep_id: 9, duplicate_id: 12 }]);
    provider_merge::merge_in_llm(&d.sync, &d.data, 9, 12).unwrap();
    d.scan(); // finishes redirects first, exactly like scan_local_changes

    assert_eq!(providers_named(&d, "SiliconFlow"), 1);
    assert_eq!(quick_provider(&d), 9);
    assert_eq!(d.key_of(9).as_deref(), Some("sk-B"));
    assert_eq!(d.data.query_row("SELECT llm_model_id FROM message WHERE id=1", [], |r| r.get::<_, i64>(0)).unwrap(), keep_model);
    assert_eq!(d.data.query_row("SELECT COUNT(*) FROM llm_model WHERE llm_provider_id=9", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    let feature = |key: &str| d.system.query_row("SELECT value FROM feature_config WHERE key=?1", [key], |r| r.get::<_, String>(0)).unwrap();
    assert_eq!(feature("title_provider_id"), "9");
    assert_eq!(feature("title_model"), "qwen-max%%9");
    assert_eq!(find_local_id_by_object_id(&d.sync, "llm.provider", &a_identity).unwrap(), Some(9));
    let events = d.pending_upserts();
    assert!(!events.iter().any(|e| e.operation == "delete"), "repair must not delete A's provider remotely: {:?}",
        events.iter().map(|e| (&e.object_type, &e.operation)).collect::<Vec<_>>());
    assert!(provider_merge::find_duplicates(&d.data).unwrap().is_empty());
    assert_eq!(d.sync.query_row("SELECT COUNT(*) FROM sync_merge_redirect", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_duplicates_with_conflicting_keys_are_left_alone() {
    let d = KDev::new("device-b");
    seed_custom(&d, 9, "Relay", Some("sk-1"));
    seed_custom(&d, 12, "Relay", Some("sk-2"));
    assert!(provider_merge::find_duplicates(&d.data).unwrap().is_empty());
}

#[test]
fn test_auto_merge_folds_safe_duplicates_and_reports_key_conflicts() {
    let d = KDev::new("device-b");
    seed_custom(&d, 9, "SiliconFlow", Some("sk-B"));
    seed_custom(&d, 12, "SiliconFlow", None);
    seed_custom(&d, 20, "Relay", Some("sk-1"));
    seed_custom(&d, 21, "Relay", Some("sk-2"));
    d.data.execute("UPDATE assistant_model SET provider_id=12, model_code='qwen-max' WHERE assistant_id=1", []).unwrap();
    d.scan();
    assert_eq!(provider_merge::auto_merge(&d.sync, &d.data).unwrap(), 1);
    d.scan();
    assert_eq!(providers_named(&d, "SiliconFlow"), 1);
    assert_eq!(quick_provider(&d), 9);
    assert_eq!(providers_named(&d, "Relay"), 2, "conflicting keys are never merged");
    assert_eq!(provider_merge::find_key_conflicts(&d.data).unwrap(), vec!["Relay".to_string()]);
    assert_eq!(provider_merge::auto_merge(&d.sync, &d.data).unwrap(), 0, "idempotent");
    assert!(!d.pending_upserts().iter().any(|e| e.operation == "delete"));
}


/// First unlock on B: server values win over B's stale local keys; keys the
/// server lacks are pushed; afterwards a real edit on B is last-writer-wins.
#[test]
fn test_first_unlock_is_server_wins_then_edits_propagate() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-A-server')", []).unwrap();
    b.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-B-stale')", []).unwrap();
    seed_custom(&b, 7, "OnlyOnB", Some("sk-B-only"));
    a.passphrase("shared passphrase", false).unwrap(); // first device: pushes its keys
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-B-stale"), "locked: local kept");

    b.passphrase("shared passphrase", false).unwrap();
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A-server"), "unlock adopts the server key");
    let before = server.received.len();
    server.round(&b, &mut cb);
    assert!(!server.received[before..].iter().any(|p| p.contains("official:llm.provider_secret")),
        "B must not upload its stale key for a secret the server already has");
    server.round(&a, &mut ca);
    assert_eq!(a.key_of(1).as_deref(), Some("sk-A-server"));
    let only_b: i64 = a.data.query_row("SELECT id FROM llm_provider WHERE name='OnlyOnB'", [], |r| r.get(0)).unwrap();
    assert_eq!(a.key_of(only_b).as_deref(), Some("sk-B-only"), "a key the server lacked is pushed from B");

    b.data.execute("UPDATE llm_provider_config SET value='sk-B-edited' WHERE llm_provider_id=1 AND name='api_key'", []).unwrap();
    server.round(&b, &mut cb);
    server.round(&a, &mut ca);
    assert_eq!(a.key_of(1).as_deref(), Some("sk-B-edited"), "after reconciliation a real edit wins");
    assert!(!server.received.iter().any(|p| p.contains("sk-B-edited")), "still ciphertext only");
}

/// B had synced before, then lost its key and edited locally while locked.
/// Re-entering the passphrase must restore the server value, not upload the edit.
#[test]
fn test_unlock_after_offline_edit_restores_server_value() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-A')", []).unwrap();
    a.passphrase("shared passphrase", false).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    b.passphrase("shared passphrase", false).unwrap();
    server.round(&b, &mut cb);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A"));
    // Locked again (key removed) and edited offline: no pending remote change exists.
    b.lock();
    b.data.execute("UPDATE llm_provider_config SET value='sk-B-offline' WHERE name='api_key'", []).unwrap();
    server.round(&b, &mut cb);
    assert_eq!(secrets::pending_count(&b.sync), 0);
    let before = server.received.len();
    b.passphrase("shared passphrase", false).unwrap();
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A"), "server copy wins on unlock");
    server.round(&b, &mut cb);
    assert_eq!(server.received.len(), before, "nothing uploaded by the unlock");
    server.round(&a, &mut ca);
    assert_eq!(a.key_of(1).as_deref(), Some("sk-A"));
}

/// Rotation on A is the deliberate write; B adopts the server values after
/// entering the new passphrase, even if it edited keys while mismatched.
#[test]
fn test_rotation_wins_and_mismatched_device_adopts_server_values() {
    let mut server = MiniServer::default();
    let (a, b) = (KDev::new("device-a"), KDev::new("device-b"));
    a.data.execute("INSERT INTO llm_provider_config(llm_provider_id, name, value) VALUES (1, 'api_key', 'sk-A')", []).unwrap();
    a.passphrase("old passphrase", false).unwrap();
    let (mut ca, mut cb) = (0, 0);
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    b.passphrase("old passphrase", false).unwrap();
    server.round(&b, &mut cb);

    a.passphrase("new passphrase", true).unwrap(); // rotation re-encrypts and pushes A's keys
    server.round(&a, &mut ca);
    server.round(&b, &mut cb);
    assert!(matches!(b.state(), secrets::SecretState::Mismatch));
    b.data.execute("UPDATE llm_provider_config SET value='sk-B-while-mismatched' WHERE name='api_key'", []).unwrap();
    server.round(&b, &mut cb);
    let err = b.passphrase("old passphrase", false).unwrap_err();
    assert!(err.starts_with(secrets::PASSPHRASE_MISMATCH_CODE), "{err}");
    assert!(!secrets::server_wins_pending(&b.sync), "a rejected passphrase leaves no reconcile flag");
    b.passphrase("new passphrase", false).unwrap();
    server.round(&b, &mut cb);
    server.round(&a, &mut ca);
    assert_eq!(b.key_of(1).as_deref(), Some("sk-A"));
    assert_eq!(a.key_of(1).as_deref(), Some("sk-A"));
    assert!(!server.received.iter().any(|p| p.contains("sk-B-while-mismatched")));
}

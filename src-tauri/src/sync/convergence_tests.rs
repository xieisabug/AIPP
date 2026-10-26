use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const TEST_TYPES: &[&str] = &[
    "llm.provider", "llm.model", "assistant", "assistant.prompt", "assistant.model", "assistant.model_config",
    "conversation", "conversation.message", "conversation.message_attachment", "conversation.summary", "conversation.todo",
];

struct Device {
    sync: Connection,
    data: Connection,
    id: &'static str,
}

impl Device {
    fn new(id: &'static str) -> Self {
        let sync = Connection::open_in_memory().unwrap();
        sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
        let data = Connection::open_in_memory().unwrap();
        data.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE llm_provider(id INTEGER PRIMARY KEY, name TEXT NOT NULL, is_official INTEGER DEFAULT 0);
            CREATE TABLE llm_model(id INTEGER PRIMARY KEY, name TEXT, llm_provider_id INTEGER NOT NULL REFERENCES llm_provider(id), code TEXT NOT NULL, UNIQUE(llm_provider_id, code));
            CREATE TABLE assistant(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE assistant_prompt(id INTEGER PRIMARY KEY, assistant_id INTEGER NOT NULL REFERENCES assistant(id), prompt TEXT NOT NULL, created_time TEXT);
            CREATE TABLE assistant_model(id INTEGER PRIMARY KEY, assistant_id INTEGER NOT NULL REFERENCES assistant(id), provider_id INTEGER NOT NULL, model_code TEXT NOT NULL, alias TEXT, UNIQUE(assistant_id, provider_id, model_code));
            CREATE TABLE assistant_model_config(id INTEGER PRIMARY KEY, assistant_id INTEGER NOT NULL REFERENCES assistant(id), assistant_model_id INTEGER NOT NULL, name TEXT NOT NULL, value TEXT, UNIQUE(assistant_id, name));
            CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT NOT NULL, assistant_id INTEGER REFERENCES assistant(id), parent_butler_conversation_id INTEGER REFERENCES conversation(id));
            CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id), message_type TEXT NOT NULL, content TEXT NOT NULL,
                llm_model_id INTEGER REFERENCES llm_model(id), parent_id INTEGER REFERENCES message(id), finish_time TEXT, generation_group_id TEXT, parent_group_id TEXT);
            CREATE TABLE message_attachment(id INTEGER PRIMARY KEY, message_id INTEGER NOT NULL REFERENCES message(id), attachment_type TEXT, attachment_content TEXT);
            CREATE TABLE conversation_summary(id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id), summary TEXT);
            CREATE TABLE conversation_todo(id INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id), content TEXT);").unwrap();
        Self { sync, data, id }
    }

    fn scan(&self) -> Vec<LocalObjectSnapshot> {
        let mut result = Vec::new();
        for spec in specs().into_iter().filter(|s| TEST_TYPES.contains(&s.object_type)) {
            let (rows, _) = read_local_snapshots_from_db(&self.data, &spec, &self.sync,
                &mut DanglingFkScanReport::default(), |kind, id| {
                    let spec = specs().into_iter().find(|s| s.object_type == kind).unwrap();
                    row_exists_by_id(&self.data, &spec, id)
                }).unwrap();
            for row in &rows { enqueue_snapshot_if_changed(&self.sync, row, self.id).unwrap(); }
            result.extend(rows);
        }
        result
    }

    fn apply(&self, change: &PullChange) -> Result<(), String> {
        let spec = specs().into_iter().find(|s| s.object_type == change.object_type).unwrap();
        apply_change_to_db(&self.sync, &self.data, &spec, change, self.id, |fields| {
            maybe_fork_message_in_db(&self.sync, &self.data, fields, |_, _| {
                // Any unexpected fork is a hard failure, rather than a fake fork
                // that could silently hide the very regression under test.
                Err("UNEXPECTED_FORK: importing history must keep one conversation".into())
            })
        })
    }

    fn receive(&self, changes: &[PullChange]) {
        apply_pull_page(&self.sync, changes, |change| self.apply(change)).unwrap();
    }

    fn replay(&self) -> (usize, usize) {
        replay_dead_letter_rows(&self.sync, |change| self.apply(change)).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.data.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    fn seed(&self, messages: i64) {
        self.data.execute_batch("INSERT INTO llm_provider VALUES(71, 'provider', 0);
            INSERT INTO llm_model VALUES(83, 'model', 71, 'model-code');
            INSERT INTO assistant VALUES(1, 'quick');
            INSERT INTO assistant_model VALUES(91, 1, 71, 'model-code', 'selected');
            INSERT INTO assistant_model_config VALUES(101, 1, 91, 'temperature', '0.7');
            INSERT INTO conversation VALUES(201, 'one conversation', 1, NULL);
            INSERT INTO conversation_summary VALUES(1, 201, 'summary');
            INSERT INTO conversation_todo VALUES(1, 201, 'todo');").unwrap();
        let tx = self.data.unchecked_transaction().unwrap();
        for i in 1..=messages {
            // Many historical regenerations share one parent, with user and
            // response rows in each generation. Writers do not set parent_id.
            tx.execute("INSERT INTO message VALUES(?1, 201, ?2, ?3, 83, NULL, 'finished', ?4, 'root')",
                params![i, if i % 2 == 1 {"user"} else {"response"}, format!("message-{i}"), format!("generation-{}", (i + 1) / 2)]).unwrap();
        }
        tx.execute("INSERT INTO message_attachment VALUES(1, 1, 'text', 'attachment')", []).unwrap();
        tx.commit().unwrap();
    }
}

fn change(kind: &str, id: &str, seq: i64, payload: JsonValue) -> PullChange {
    PullChange { seq, event_id: format!("event-{seq}"), device_id: "device-a".into(),
        object_type: kind.into(), object_id: id.into(), operation: "upsert".into(),
        version: 1, payload: Some(payload), deleted_at: None, created_at: "now".into() }
}

fn history(source: &Device) -> Vec<PullChange> {
    source.scan().into_iter().enumerate().map(|(i, row)| {
        change(&row.object_type, &row.object_id, i as i64 + 1, serde_json::from_str(&row.payload_json).unwrap())
    }).collect()
}

// A real new client has already initialized assistant 1 and its empty model.
// The empty-receiver fixture alone cannot catch collisions with these seeds.
fn initialized_quick_assistant_device(id: &'static str) -> Device {
    let db = crate::db::assistant_db::AssistantDatabase {
        conn: Connection::open_in_memory().unwrap(),
        mcp_conn: Connection::open_in_memory().unwrap(),
    };
    db.create_tables().unwrap();
    db.conn.execute_batch("CREATE TABLE llm_provider(id INTEGER PRIMARY KEY, name TEXT NOT NULL, is_official INTEGER DEFAULT 0);").unwrap();
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    Device { sync, data: db.conn, id }
}

#[test]
fn test_sync_optional_remote_refs_clear_missing_targets() {
    let spec = specs().into_iter().find(|s| s.object_type == "conversation").unwrap();
    let mut fields = serde_json::Map::from_iter([
        ("name".into(), json!("historical")),
        ("assistant_id".into(), json!(49)),
        ("parent_butler_conversation_id".into(), JsonValue::Null),
    ]);
    let refs = serde_json::Map::from_iter([("assistant_id".into(), json!("missing-assistant"))]);
    resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(None)).unwrap();
    assert_eq!(fields.get("assistant_id"), Some(&JsonValue::Null));
}

#[test]
fn test_sync_quick_assistant_initialized_receiver_loads_explicit_model_and_configs() {
    for reversed in [false, true] {
        let source = initialized_quick_assistant_device("device-a");
        source.data.execute_batch("INSERT INTO llm_provider VALUES(71, 'selected provider', 0);
            UPDATE assistant_model SET id=91, provider_id=71, model_code='selected-model', alias='selected';
            UPDATE assistant_model_config SET assistant_model_id=91;
            UPDATE assistant_model_config SET value='0.42' WHERE name='temperature';").unwrap();
        source.data.execute("UPDATE assistant_prompt SET prompt='custom quick assistant prompt' WHERE assistant_id=1", []).unwrap();
        let mut changes = history(&source);
        if reversed { changes.reverse(); }
        let receiver = initialized_quick_assistant_device("device-b");
        // The same numeric provider ID on another device is unrelated.
        receiver.data.execute("INSERT INTO llm_provider VALUES(71, 'unrelated local provider', 0)", []).unwrap();
        receiver.scan(); // Include the case where the initialization was already scanned.
        for _ in 0..2 {
            for page in changes.chunks(1) { receiver.receive(page); }
            assert_eq!(receiver.replay().1, 0);
        }
        let db = crate::db::assistant_db::AssistantDatabase {
            conn: receiver.data,
            mcp_conn: Connection::open_in_memory().unwrap(),
        };
        db.init_assistant().unwrap(); // Reopening the app must not restore the empty selection.
        assert_eq!(db.get_assistant(1).unwrap().name, "快速使用助手");
        assert_eq!(db.get_assistant_prompt(1).unwrap()[0].prompt, "custom quick assistant prompt");
        assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM assistant_prompt WHERE assistant_id=1", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        let models = db.get_assistant_model(1).unwrap();
        assert_eq!(models.len(), 1, "initialization placeholder must not become the selected model");
        let selected = &models[0];
        assert_eq!(selected.model_code, "selected-model");
        assert_ne!(selected.provider_id, 71);
        assert_ne!(selected.id, 91);
        let provider: String = db.conn.query_row("SELECT name FROM llm_provider WHERE id=?1",
            params![selected.provider_id], |r| r.get(0)).unwrap();
        assert_eq!(provider, "selected provider");
        let configs = db.get_assistant_model_configs(1).unwrap();
        assert_eq!(configs.len(), 4);
        assert!(configs.iter().all(|c| c.assistant_model_id == selected.id));
        assert_eq!(configs.iter().find(|c| c.name == "temperature").unwrap().value.as_deref(), Some("0.42"));
        assert_eq!(db.get_assistant_model_configs_with_model_id(1, selected.id).unwrap().len(), 4);
        // Read-time handling leaves sync identities and rows intact for replay/deletes.
        assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM assistant_model", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    }
}

#[test]
fn test_sync_assistant_model_waits_for_its_provider_and_never_uses_local_numeric_hint() {
    let source = initialized_quick_assistant_device("source");
    source.data.execute_batch("INSERT INTO llm_provider VALUES(71, 'chosen provider', 0);
        UPDATE assistant_model SET provider_id=71, model_code='chosen-model';").unwrap();
    let changes = history(&source);
    let provider = changes.iter().find(|e| e.object_type == "llm.provider").unwrap();
    let model = changes.iter().find(|e| e.object_type == "assistant.model").unwrap();
    let receiver = initialized_quick_assistant_device("receiver");
    receiver.data.execute("INSERT INTO llm_provider VALUES(71, 'unrelated local provider', 0)", []).unwrap();
    receiver.receive(std::slice::from_ref(changes.iter().find(|e| e.object_type == "assistant").unwrap()));
    receiver.receive(std::slice::from_ref(model));
    let error: String = receiver.sync.query_row("SELECT error FROM sync_dead_letter", [], |r| r.get(0)).unwrap();
    assert!(error.contains("缺少同步依赖: assistant.model.provider_id"), "{error}");
    assert_eq!(receiver.data.query_row("SELECT COUNT(*) FROM assistant_model WHERE model_code='chosen-model'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    receiver.receive(std::slice::from_ref(provider));
    assert_eq!(receiver.replay().1, 0);
    let provider_id: i64 = receiver.data.query_row("SELECT provider_id FROM assistant_model WHERE model_code='chosen-model'", [], |r| r.get(0)).unwrap();
    assert_ne!(provider_id, 71);
}

#[test]
fn test_sync_historical_regenerations_and_repeated_replay_keep_one_conversation() {
    let source = Device::new("device-a"); source.seed(60);
    let changes = history(&source);
    let receiver = Device::new("device-b");
    for _ in 0..3 {
        for page in changes.chunks(7) { receiver.receive(page); }
        assert_eq!(receiver.replay().1, 0);
        assert_eq!(receiver.count("conversation"), 1);
        assert_eq!(receiver.count("message"), 60);
    }
    // Numeric IDs differ but a scan of imported data must enqueue no echoes.
    receiver.scan();
    assert_eq!(receiver.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let selected: (String, String) = receiver.data.query_row("SELECT p.name, m.model_code FROM assistant_model m JOIN llm_provider p ON p.id=m.provider_id", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(selected, ("provider".into(), "model-code".into()));
}

#[test]
fn test_sync_transport_reset_preserves_history_identities_and_does_not_echo() {
    let source = Device::new("device-a"); source.seed(60);
    let changes = history(&source);
    let receiver = Device::new("device-b");
    receiver.receive(&changes);
    let before: Vec<_> = receiver.scan().into_iter().map(|s| (s.object_type, s.object_id)).collect();
    for _ in 0..3 {
        clear_sync_state(&receiver.sync).unwrap();
        ensure_initial_sync_is_safe(&receiver.sync, false, false).unwrap();
        for page in changes.chunks(7) { receiver.receive(page); }
        let after: Vec<_> = receiver.scan().into_iter().map(|s| (s.object_type, s.object_id)).collect();
        assert_eq!(before, after, "reset must not allocate replacement UUIDs");
        assert_eq!(receiver.count("conversation"), 1);
        assert_eq!(receiver.count("message"), 60);
        assert_eq!(receiver.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}

#[test]
fn test_sync_populated_clone_without_baseline_stops_before_upload() {
    let clone = Device::new("clone"); clone.seed(12);
    // A copied business DB has the same history but none of the sync identities.
    assert!(ensure_initial_sync_is_safe(&clone.sync, false, false).unwrap_err().contains("同步基线"));
    assert_eq!(clone.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    // An empty destination or empty server remains a supported bootstrap.
    ensure_initial_sync_is_safe(&clone.sync, true, false).unwrap();
    ensure_initial_sync_is_safe(&clone.sync, false, true).unwrap();
    // Merely having scanned and allocated fresh UUIDs is not a confirmed baseline.
    clone.scan();
    assert!(ensure_initial_sync_is_safe(&clone.sync, false, false).is_err());
}

#[test]
fn test_sync_missing_optional_reference_replay_and_reset_never_echo() {
    let receiver = Device::new("device-b");
    let event = change("conversation", "history", 1, json!({
        "fields":{"name":"history", "assistant_id":49, "parent_butler_conversation_id":null},
        "refs":{"assistant_id":"deleted-assistant"}
    }));
    for _ in 0..3 {
        receiver.receive(&[event.clone()]);
        receiver.scan();
        assert_eq!(receiver.count("conversation"), 1);
        assert_eq!(receiver.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        clear_sync_state(&receiver.sync).unwrap();
    }
}

#[test]
fn test_sync_same_device_newer_event_updates_restored_row() {
    let receiver = Device::new("device-a");
    let mut event = change("conversation", "history", 1, json!({
        "fields":{"name":"before", "assistant_id":null, "parent_butler_conversation_id":null}, "refs":{}
    }));
    receiver.receive(&[event.clone()]);
    event.seq = 2; event.version = 2; event.event_id = "own-newer".into();
    event.payload.as_mut().unwrap()["fields"]["name"] = json!("after");
    receiver.receive(&[event]);
    assert_eq!(receiver.data.query_row("SELECT name FROM conversation", [], |r| r.get::<_, String>(0)).unwrap(), "after");
    receiver.scan();
    assert_eq!(receiver.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_sync_reversed_cross_page_dependency_chain_resolves_all_domains() {
    let source = Device::new("device-a"); source.seed(32);
    let mut changes = history(&source); changes.reverse();
    let receiver = Device::new("device-b");
    for page in changes.chunks(3) { receiver.receive(page); }
    assert_eq!(receiver.replay().1, 0);
    for table in ["conversation", "message_attachment", "conversation_summary", "conversation_todo", "assistant_model_config"] {
        assert_eq!(receiver.count(table), 1, "{table}");
    }
    assert_eq!(receiver.count("message"), 32);
}

#[test]
fn test_sync_legacy_parent_sibling_history_does_not_fork() {
    let source = Device::new("device-a"); source.seed(12);
    source.data.execute("UPDATE message SET parent_id=1 WHERE id>1", []).unwrap();
    let receiver = Device::new("device-b");
    for page in history(&source).chunks(2) { receiver.receive(page); }
    assert_eq!(receiver.replay().1, 0);
    assert_eq!(receiver.count("conversation"), 1);
    assert_eq!(receiver.count("message"), 12);
}

#[test]
fn test_sync_unconfirmed_same_ancestor_forks_once_and_unrelated_write_does_not() {
    let source = Device::new("device-a"); source.seed(2);
    for parent in ["other-ancestor", "root"] {
        let receiver = Device::new("device-b"); receiver.receive(&history(&source));
        let conversation_id: i64 = receiver.data.query_row("SELECT id FROM conversation", [], |r| r.get(0)).unwrap();
        receiver.data.execute("INSERT INTO message(id,conversation_id,message_type,content,generation_group_id,parent_group_id)
            VALUES(100,?1,'user','local','local-generation',?2)", params![conversation_id,parent]).unwrap();
        let mut remote = json!({"conversation_id":conversation_id,"generation_group_id":"remote-generation","parent_group_id":"root"})
            .as_object().unwrap().clone();
        let calls = std::cell::Cell::new(0);
        let fork = |_: i64, _: Option<i64>| {
            calls.set(calls.get() + 1);
            receiver.data.execute("INSERT INTO conversation(name) VALUES('sync fork')", []).map_err(|e| e.to_string())?;
            Ok(receiver.data.last_insert_rowid())
        };
        maybe_fork_message_in_db(&receiver.sync, &receiver.data, &mut remote, fork).unwrap();
        if parent == "other-ancestor" {
            assert_eq!(calls.get(), 0);
            assert_eq!(remote["conversation_id"], json!(conversation_id));
        } else {
            assert_eq!(calls.get(), 1);
            let fork_id = remote["conversation_id"].clone();
            assert_ne!(fork_id, json!(conversation_id));
            for group_parent in ["root", "remote-generation"] {
                let mut next = json!({"conversation_id":conversation_id,
                    "generation_group_id":if group_parent == "root" {"remote-generation"} else {"descendant"},
                    "parent_group_id":group_parent}).as_object().unwrap().clone();
                maybe_fork_message_in_db(&receiver.sync, &receiver.data, &mut next, |_, _| panic!("branch forked twice")).unwrap();
                assert_eq!(next["conversation_id"], fork_id);
            }
            assert_eq!(receiver.count("conversation"), 2);
        }
    }
}

#[test]
fn test_sync_required_dependencies_remain_traceable_optional_refs_do_not_block() {
    let receiver = Device::new("device-b");
    let cases = [
        ("conversation.message", "conversation_id", json!({"conversation_id":99})),
        ("conversation.message", "llm_model_id", json!({"conversation_id":null,"llm_model_id":99,"message_type":"user","content":"history"})),
        ("conversation.summary", "conversation_id", json!({"conversation_id":99})),
        ("conversation", "assistant_id", json!({"name":"history","assistant_id":99})),
        ("conversation.message_attachment", "message_id", json!({"message_id":99})),
        ("conversation.todo", "conversation_id", json!({"conversation_id":99})),
        ("conversation", "parent_butler_conversation_id", json!({"name":"history","parent_butler_conversation_id":99})),
    ];
    // Resolve the owner for the model case so it reaches the model diagnostic.
    receiver.data.execute("INSERT INTO conversation VALUES(1,'owner',NULL,NULL)", []).unwrap();
    save_object_map(&receiver.sync, "conversation", "conversation", 1, "owner").unwrap();
    for (i, (kind, column, mut fields)) in cases.into_iter().enumerate() {
        let mut refs = Map::new(); refs.insert(column.into(), json!("missing-root"));
        if column == "llm_model_id" {
            fields["conversation_id"] = json!(1); refs.insert("conversation_id".into(), json!("owner"));
        }
        let incoming = change(kind, &format!("failed-{i}"), i as i64 + 1, json!({"fields":fields,"refs":refs}));
        if ["llm_model_id", "assistant_id", "parent_butler_conversation_id"].contains(&column) {
            receiver.apply(&incoming).unwrap();
        } else {
            let err = receiver.apply(&incoming).unwrap_err();
            assert!(is_dependency_pending_error(&err) && err.contains(column) && err.contains("missing-root"), "{err}");
        }
        receiver.receive(&[incoming]);
    }
    assert_eq!(receiver.replay().1, 4);
    let replacement = change("conversation.message", "failed-0", 20,
        json!({"fields":{"conversation_id":1,"message_type":"user","content":"valid newer snapshot"},"refs":{"conversation_id":"owner"}}));
    receiver.receive(&[replacement]);
    assert_eq!(receiver.replay().1, 3);
}

#[test]
fn test_sync_dead_letter_persistence_failure_preserves_old_record_and_stops_page() {
    let receiver = Device::new("device-b");
    let old = change("conversation", "c", 1, json!({}));
    record_dead_letter(&receiver.sync, &old, "previous").unwrap();
    receiver.sync.execute_batch("CREATE TRIGGER fail_deadletter BEFORE INSERT ON sync_dead_letter
        BEGIN SELECT RAISE(ABORT,'storage failure'); END;").unwrap();
    let latest = change("conversation", "c", 2, json!({}));
    let err = apply_pull_page(&receiver.sync, &[latest], |_| Err("failure".into())).unwrap_err();
    assert!(err.contains("storage failure") && err.contains("保留拉取游标"));
    assert_eq!(receiver.sync.query_row("SELECT json_extract(change_json,'$.seq') FROM sync_dead_letter", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
}

#[test]
fn test_sync_deleted_parent_with_stale_map_is_not_published_as_dependency() {
    let source = Device::new("device-a"); source.seed(2); source.scan();
    // Reproduce old business DBs with deleted assistant/model rows but retained
    // sync maps and historical numeric foreign keys.
    source.data.execute_batch("PRAGMA foreign_keys=OFF; DELETE FROM assistant; DELETE FROM llm_model;").unwrap();
    let rows = source.scan();
    let conv: JsonValue = serde_json::from_str(&rows.iter().find(|r| r.object_type == "conversation").unwrap().payload_json).unwrap();
    assert!(conv["fields"]["assistant_id"].is_null());
    assert!(conv["refs"].get("assistant_id").is_none());
    for row in rows.iter().filter(|r| r.object_type == "conversation.message") {
        let payload: JsonValue = serde_json::from_str(&row.payload_json).unwrap();
        assert!(payload["fields"]["llm_model_id"].is_null());
        assert!(payload["refs"].get("llm_model_id").is_none());
    }
    assert!(!rows.iter().any(|r| r.object_type == "assistant.model"));
    // Required ownership is blocked, never guessed or mapped to a different model.
    source.data.execute("DELETE FROM conversation", []).unwrap();
    assert!(!source.scan().iter().any(|r| r.object_type == "conversation.message"));
}

#[test]
fn test_sync_official_identity_migration_preserves_old_child_refs() {
    let device = Device::new("device-b");
    save_object_map(&device.sync, "assistant", "assistant", 1, "old-assistant-uuid").unwrap();
    assert_eq!(upsert_official_object_map(&device.sync, "assistant", "assistant", 1).unwrap(), "official:assistant:1");
    assert_eq!(find_local_id_by_object_id(&device.sync, "assistant", "old-assistant-uuid").unwrap(), Some(1));
    device.data.execute("INSERT INTO assistant VALUES(1, 'quick')", []).unwrap();
    device.apply(&change("conversation", "c", 2, json!({"fields":{"name":"one","assistant_id":99},"refs":{"assistant_id":"old-assistant-uuid"}}))).unwrap();
    assert_eq!(device.count("conversation"), 1);
}

#[test]
fn test_sync_reset_metadata_restores_own_device_history_instead_of_false_deletes() {
    let source = Device::new("device-a"); source.seed(8);
    let changes = history(&source);
    let restored = Device::new("device-a"); // Same persisted device ID, empty maps.
    for page in changes.chunks(3) { restored.receive(page); }
    assert_eq!(restored.replay().1, 0);
    assert_eq!(restored.count("conversation"), 1);
    assert_eq!(restored.count("message"), 8);
    for kind in ["conversation", "conversation.message", "assistant"] {
        let ids = restored.sync.prepare("SELECT sync_id FROM sync_object_map WHERE object_type=?1").unwrap()
            .query_map(params![kind], |r| r.get::<_, String>(0)).unwrap().collect::<Result<HashSet<_>,_>>().unwrap();
        assert!(find_deleted_candidates(&restored.sync, kind, &ids).unwrap().is_empty());
    }
}

#[test]
fn test_sync_newer_dead_letter_is_not_replaced_by_older_deferred_change() {
    let device = Device::new("device-b");
    let latest = change("conversation", "c", 20, json!({"fields":{"name":"latest"}}));
    record_dead_letter(&device.sync, &latest, "latest failure").unwrap();
    let mut old = latest.clone(); old.seq = 2;
    record_dead_letter(&device.sync, &old, "old failure").unwrap();
    let seq: i64 = device.sync.query_row("SELECT json_extract(change_json,'$.seq') FROM sync_dead_letter", [], |r| r.get(0)).unwrap();
    assert_eq!(seq, 20);
}

#[test]
fn test_sync_outbox_sends_parents_before_children_even_with_old_child_queue_entries() {
    let source = Device::new("device-a"); source.seed(4);
    let mut rows = source.scan(); source.sync.execute("DELETE FROM sync_outbox", []).unwrap();
    rows.reverse();
    for row in rows { enqueue_snapshot_if_changed(&source.sync, &row, source.id).unwrap(); }
    let mut sent = HashSet::new();
    loop {
        let batch = load_pending_outbox_from_db(&source.sync, 2).unwrap();
        if batch.is_empty() { break; }
        for (id, event) in batch {
            for reference in event.payload.as_ref().unwrap()["refs"].as_object().unwrap().values() {
                assert!(sent.contains(reference.as_str().unwrap()), "child {} sent before {}", event.object_type, reference);
            }
            sent.insert(event.object_id);
            source.sync.execute("DELETE FROM sync_outbox WHERE id=?1", params![id]).unwrap();
        }
    }
    assert_eq!(sent.len(), 13);
    assert_eq!(source.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

struct ServerBridge { child: Child, input: ChildStdin, output: BufReader<ChildStdout> }

impl ServerBridge {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("sync-server");
        let python = std::env::var_os("AIPP_SYNC_TEST_PYTHON").map(PathBuf::from).unwrap_or_else(|| {
            if cfg!(windows) { root.join(".venv/Scripts/python.exe") } else { root.join(".venv/bin/python") }
        });
        let mut child = Command::new(python).args(["-u", "-m", "app.tests.sync_client_bridge"])
            .current_dir(root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())
            .spawn().expect("install sync-server test dependencies or set AIPP_SYNC_TEST_PYTHON");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self { child, input, output }
    }
    fn request(&mut self, request: JsonValue) -> JsonValue {
        writeln!(self.input, "{request}").unwrap(); self.input.flush().unwrap();
        let mut line = String::new(); self.output.read_line(&mut line).unwrap();
        let response: JsonValue = serde_json::from_str(&line).expect("server bridge JSON response");
        assert_eq!(response["status"], 200, "{response}");
        response["body"].clone()
    }
    fn pull(&mut self, device: &Device, cursor: &mut i64, page_size: usize) -> Vec<PullChange> {
        let mut all = Vec::new();
        loop {
            let body = self.request(json!({"path":format!("/v1/sync/pull?cursor={cursor}&limit={page_size}"),"device":device.id}));
            let page: PullResponse = serde_json::from_value(body).unwrap();
            assert!(page.changes.iter().all(|c| c.seq > *cursor));
            device.receive(&page.changes); *cursor = page.cursor; all.extend(page.changes);
            if !page.has_more { break; }
        }
        assert_eq!(device.replay().1, 0);
        all
    }
}
impl Drop for ServerBridge {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

/// Runs production Rust snapshot/outbox/FK/apply/replay logic against the actual
/// Python auth/push/merge/pull routes. Every DB on both sides lives in memory.
#[test]
#[ignore = "requires sync-server Python test dependencies; run explicitly with --ignored"]
fn test_sync_server_multidevice_roundtrip() {
    let mut server = ServerBridge::new();
    let source = Device::new("device-a"); source.seed(1200); source.scan();
    loop {
        let events = load_pending_outbox_from_db(&source.sync, 100).unwrap();
        if events.is_empty() { break; }
        let request = PushRequest { device_id: source.id.into(), device_name: "integration".into(),
            events: events.iter().map(|(_, event)| event.clone()).collect() };
        let body = json!({"method":"POST","path":"/v1/sync/push","body":request});
        let first = server.request(body.clone());
        assert_eq!(server.request(body), first, "retry after lost ACK must be idempotent");
        assert_eq!(first["conflicts"], json!([])); assert_eq!(first["rejected"], json!([]));
        for (id, event) in events {
            let ack = first["accepted"].as_array().unwrap().iter().find(|a| a["event_id"] == event.event_id).unwrap();
            apply_ack_to_shadow(&source.sync, &event.object_type, &event.object_id,
                ack["server_version"].as_i64().unwrap(), &event.operation,
                &sync_payload_hash(&source.sync, event.payload.as_ref()).unwrap()).unwrap();
            source.sync.execute("DELETE FROM sync_outbox WHERE id=?1", params![id]).unwrap();
        }
    }
    assert_eq!(source.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let receiver = Device::new("device-b"); let mut cursor_b = 0;
    let changes = server.pull(&receiver, &mut cursor_b, 137);
    let third = Device::new("device-c"); let mut cursor_c = 0;
    server.pull(&third, &mut cursor_c, 499);
    for device in [&receiver, &third] {
        // Repeated full history replay and incremental pulls must be stable.
        for page in changes.chunks(31) { device.receive(page); }
        assert_eq!(device.replay().1, 0);
        assert_eq!(device.count("conversation"), 1);
        assert_eq!(device.count("message"), 1200);
        assert_eq!(device.count("message_attachment"), 1);
        device.scan();
        assert_eq!(device.sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    assert!(server.pull(&receiver, &mut cursor_b, 500).is_empty());
    // Restore on the same device after metadata loss, including own-device echoes.
    let restored = Device::new("device-a"); let mut reset_cursor = 0;
    server.pull(&restored, &mut reset_cursor, 211);
    assert_eq!(restored.count("conversation"), 1); assert_eq!(restored.count("message"), 1200);
}

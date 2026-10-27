use super::*;

fn configured(id: &'static str, model_id: i64, code: &str) -> Device {
    let device = initialized_quick_assistant_device(id);
    device.data.execute("INSERT INTO llm_provider VALUES(71, ?1, 0)", [id]).unwrap();
    device.data.execute("UPDATE assistant_model SET id=?1, provider_id=71, model_code=?2", params![model_id, code]).unwrap();
    device.data.execute("UPDATE assistant_model_config SET assistant_model_id=?1", [model_id]).unwrap();
    device
}

fn selected(device: &Device) -> (i64, i64, String) {
    assert_eq!(device.count("assistant_model"), 1);
    device.data.query_row("SELECT id, provider_id, model_code FROM assistant_model WHERE assistant_id=1", [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
}

fn next_model(source: &Device, seq: i64, version: i64) -> PullChange {
    let row = source.scan().into_iter().find(|s| s.object_type == "assistant.model").unwrap();
    assert_eq!(row.object_id, quick_assistant::MODEL_ID);
    let mut event = change(&row.object_type, &row.object_id, seq, serde_json::from_str(&row.payload_json).unwrap());
    event.version = version;
    event
}

#[test]
fn test_sync_quick_assistant_preconfigured_receiver_maps_local_ids_and_updates_both_ways() {
    for reversed in [false, true] {
        let source = configured("source", 91, "new-model");
        let receiver = configured("receiver", 42, "old-model");
        receiver.scan();
        let mut changes = history(&source);
        if reversed { changes.reverse(); }
        for page in changes.chunks(1) { receiver.receive(page); }
        assert_eq!(receiver.replay().1, 0);
        let (id, provider, model) = selected(&receiver);
        assert_eq!((id, model.as_str()), (42, "new-model"));
        assert_ne!(provider, 71, "same numeric provider ID on B belongs to an unrelated provider");
        assert_eq!(receiver.data.query_row("SELECT name FROM llm_provider WHERE id=?1", [provider], |r| r.get::<_, String>(0)).unwrap(), "source");
        assert_eq!(receiver.data.query_row("SELECT COUNT(*) FROM assistant_model_config WHERE assistant_model_id=42", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
        receiver.data.execute("UPDATE assistant_model SET model_code='back-from-b' WHERE id=42", []).unwrap();
        source.receive(&[next_model(&receiver, 100, 2)]);
        assert_eq!(selected(&source), (91, 71, "back-from-b".into()));
        // Duplicate/older delivery must not undo the latest value.
        for _ in 0..2 { source.receive(&changes); }
        assert_eq!(selected(&source).2, "back-from-b");
        let db = crate::db::assistant_db::AssistantDatabase {
            conn: source.data, mcp_conn: Connection::open_in_memory().unwrap(),
        };
        db.init_assistant().unwrap();
        assert_eq!(db.get_assistant_model(1).unwrap()[0].model_code, "back-from-b");
    }
}

#[test]
fn test_sync_quick_assistant_missing_refs_never_use_remote_numeric_ids() {
    let source = configured("source", 91, "new-model");
    let receiver = configured("receiver", 42, "old-model");
    receiver.scan();
    let mut model = next_model(&source, 100, 1);
    // Even assistant_id is only a hint. An explicit reference must map locally.
    model.payload.as_mut().unwrap()["fields"]["assistant_id"] = json!(987);
    assert!(receiver.apply(&model).unwrap_err().contains("缺少同步依赖: assistant.model.provider_id"));
    assert_eq!(selected(&receiver), (42, 71, "old-model".into()));
    model.payload.as_mut().unwrap()["refs"].as_object_mut().unwrap().remove("provider_id");
    assert!(receiver.apply(&model).is_err());
    assert_eq!(selected(&receiver).2, "old-model");
    receiver.receive(&history(&source));
    assert_eq!(receiver.replay().1, 0);
    assert_eq!(selected(&receiver).2, "new-model");
}

#[test]
fn test_sync_quick_assistant_migration_keeps_aliases_parameters_and_attempted_events() {
    let device = configured("legacy", 42, "current");
    device.data.execute("INSERT INTO assistant_model(id,assistant_id,provider_id,model_code,alias) VALUES(55,1,71,'duplicate','')", []).unwrap();
    device.data.execute("UPDATE assistant_model_config SET assistant_model_id=55 WHERE name='temperature'", []).unwrap();
    for (id, remote) in [(42, "old-selected"), (55, "old-duplicate")] {
        save_object_map(&device.sync, "assistant.model", "assistant_model", id, remote).unwrap();
    }
    device.sync.execute("INSERT INTO sync_outbox(event_id,object_type,object_id,operation,payload_json,local_version,device_id,created_at,status,sent_once)
        VALUES('immutable-attempt','assistant.model','old-duplicate','upsert','{}',1,'legacy','now','failed',1)", []).unwrap();
    device.scan();
    assert_eq!(selected(&device), (42, 71, "current".into()));
    for remote in ["old-selected", "old-duplicate", quick_assistant::MODEL_ID] {
        assert_eq!(find_local_id_by_object_id(&device.sync, "assistant.model", remote).unwrap(), Some(42));
    }
    assert_eq!(device.data.query_row("SELECT COUNT(*) FROM assistant_model_config WHERE assistant_model_id=42", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
    assert_eq!(device.sync.query_row("SELECT payload_json FROM sync_outbox WHERE event_id='immutable-attempt'", [], |r| r.get::<_, String>(0)).unwrap(), "{}");
    assert!(!load_pending_outbox_from_db(&device.sync, 100).unwrap().iter().any(|(_, e)| e.object_id == quick_assistant::MODEL_ID));
    device.sync.execute("UPDATE sync_outbox SET outcome_known=1 WHERE event_id='immutable-attempt'", []).unwrap();
    assert!(load_pending_outbox_from_db(&device.sync, 100).unwrap().iter().any(|(_, e)| e.object_id == quick_assistant::MODEL_ID));
    // A delayed delete ACK must retain the same alias as a pulled delete.
    apply_ack_to_shadow(&device.sync, "assistant.model", "old-duplicate", 1, 800, "delete", "").unwrap();
    assert_eq!(find_local_id_by_object_id(&device.sync, "assistant.model", "old-duplicate").unwrap(), Some(42));
    // A legacy delete never deletes the fixed slot or its parameter aliases.
    let mut delete = change("assistant.model", "old-selected", 900, JsonValue::Null);
    delete.operation = "delete".into(); delete.payload = None;
    device.receive(&[delete]);
    assert_eq!(selected(&device).2, "current");
    assert_eq!(find_local_id_by_object_id(&device.sync, "assistant.model", "old-selected").unwrap(), Some(42));
    // Failure midway through consolidation must roll back both DBs.
    device.data.execute("INSERT INTO assistant_model VALUES(77,1,71,'rollback','')", []).unwrap();
    save_object_map(&device.sync, "assistant.model", "assistant_model", 77, "rollback-model").unwrap();
    device.data.execute_batch("CREATE TRIGGER reject_merge BEFORE DELETE ON assistant_model BEGIN SELECT RAISE(ABORT,'injected merge failure'); END;").unwrap();
    assert!(quick_assistant::prepare(&device.sync, &device.data).unwrap_err().contains("injected merge failure"));
    assert_eq!(find_local_id_by_object_id(&device.sync, "assistant.model", "rollback-model").unwrap(), Some(77));
    assert_eq!(device.count("assistant_model"), 2);
}

#[test]
fn test_sync_quick_assistant_legacy_replay_cannot_override_canonical_or_clear_choice() {
    let source = configured("source", 91, "canonical");
    let receiver = configured("receiver", 42, "old");
    receiver.scan(); receiver.receive(&history(&source));
    let mut legacy = next_model(&source, 1000, 1);
    legacy.object_id = "legacy-model".into();
    legacy.payload.as_mut().unwrap()["fields"]["model_code"] = json!("obsolete");
    receiver.receive(&[legacy]);
    assert_eq!(selected(&receiver).2, "canonical");
    assert_eq!(find_local_id_by_object_id(&receiver.sync, "assistant.model", "legacy-model").unwrap(), Some(42));
    source.data.execute("UPDATE assistant_model SET provider_id=0,model_code='',alias=''", []).unwrap();
    // Legacy high sequence must not block the fixed identity's newer version.
    receiver.receive(&[next_model(&source, 200, 2)]);
    assert_eq!(selected(&receiver), (42, 0, "".into()));
}

#[test]
fn test_sync_quick_assistant_does_not_merge_other_assistants() {
    let device = configured("device", 42, "quick");
    device.data.execute_batch("INSERT INTO assistant(id,name) VALUES(2,'comparison');
        INSERT INTO assistant_model(id,assistant_id,provider_id,model_code) VALUES(50,2,71,'first'),(51,2,71,'second');").unwrap();
    device.scan();
    assert_eq!(device.count("assistant_model"), 3);
    assert_ne!(find_object_id(&device.sync, "assistant.model", 50).unwrap().unwrap(), quick_assistant::MODEL_ID);
    assert_ne!(find_object_id(&device.sync, "assistant.model", 51).unwrap().unwrap(), quick_assistant::MODEL_ID);
}

#[test]
#[ignore = "requires sync-server Python test dependencies"]
fn test_sync_quick_assistant_server_roundtrip_updates_fixed_slot_and_parameter_refs() {
    fn push_all(server: &mut ServerBridge, device: &Device) {
        loop {
            let events = load_pending_outbox_from_db(&device.sync, 100).unwrap();
            if events.is_empty() { break; }
            mark_events_pushing_in_db(&device.sync, &events).unwrap();
            let response = server.request(json!({"method":"POST","path":"/v1/sync/push","body":PushRequest {
                device_id:device.id.into(),device_name:"quick regression".into(),
                events:events.iter().map(|(_,e)|e.clone()).collect()
            }}));
            assert_eq!(response["conflicts"], json!([]));
            assert_eq!(response["rejected"], json!([]));
            for value in response["accepted"].as_array().unwrap() {
                let ack: AcceptedEvent = serde_json::from_value(value.clone()).unwrap();
                let (id, event) = events.iter().find(|(_,e)| e.event_id == ack.event_id).unwrap();
                consume_accepted_event(&device.sync, *id, event, &ack).unwrap();
            }
        }
    }
    let mut server = ServerBridge::new();
    let source = configured("source", 91, "first-from-a");
    let receiver = configured("receiver", 42, "old-on-b");
    source.scan(); push_all(&mut server, &source);
    let mut cursor_a = 0; let mut cursor_b = 0;
    server.pull(&receiver, &mut cursor_b, 1);
    assert_eq!(selected(&receiver).0, 42);
    assert_eq!(selected(&receiver).2, "first-from-a");
    assert_ne!(selected(&receiver).1, 71);
    assert_eq!(receiver.data.query_row("SELECT COUNT(*) FROM assistant_model_config WHERE assistant_model_id=42", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
    receiver.scan(); push_all(&mut server, &receiver);
    server.pull(&source, &mut cursor_a, 1);
    receiver.data.execute("UPDATE assistant_model SET model_code='second-from-b' WHERE id=42", []).unwrap();
    receiver.scan(); push_all(&mut server, &receiver);
    let changes = server.pull(&source, &mut cursor_a, 1);
    assert_eq!(selected(&source), (91, 71, "second-from-b".into()));
    source.receive(&changes);
    source.data.execute("UPDATE assistant_model SET model_code='third-from-a' WHERE id=91", []).unwrap();
    source.scan(); push_all(&mut server, &source);
    server.pull(&receiver, &mut cursor_b, 2);
    assert_eq!(selected(&receiver).2, "third-from-a");
    source.scan(); receiver.scan();
    assert!(load_pending_outbox_from_db(&source.sync, 100).unwrap().is_empty());
    assert!(load_pending_outbox_from_db(&receiver.sync, 100).unwrap().is_empty());
}

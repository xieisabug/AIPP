use super::*;

#[test]
fn test_cascade_delete_sync_children_can_revive_and_stale_aliases_are_repaired() {
    for legacy_stale_mapping in [false, true] {
        let sync = sync_db();
        let data = Connection::open_in_memory().unwrap();
        data.execute_batch("CREATE TABLE assistant(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT);
            CREATE TABLE assistant_prompt(id INTEGER PRIMARY KEY AUTOINCREMENT, assistant_id INTEGER, prompt TEXT);
            INSERT INTO assistant VALUES(7,'old'); INSERT INTO assistant_prompt VALUES(10,7,'old');").unwrap();
        save_object_map(&sync,"assistant","assistant",7,"owner").unwrap();
        save_object_map(&sync,"assistant.prompt","assistant_prompt",10,"prompt").unwrap();
        sync.execute("INSERT INTO sync_object_alias VALUES ('assistant.prompt','prompt-alias',10)", []).unwrap();
        let mut deletion = change("assistant","owner",2,JsonValue::Null);
        deletion.operation = "delete".into();
        apply_change_to_db(&sync,&data,&spec("assistant"),&deletion,"local", |_|Ok(())).unwrap();
        assert_eq!(find_local_id_by_object_id(&sync,"assistant.prompt","prompt").unwrap(),None);
        assert_eq!(find_local_id_by_object_id(&sync,"assistant.prompt","prompt-alias").unwrap(),None);
        if legacy_stale_mapping {
            save_object_map(&sync,"assistant.prompt","assistant_prompt",10,"prompt").unwrap();
            sync.execute("INSERT INTO sync_object_alias VALUES ('assistant.prompt','prompt-alias',10)", []).unwrap();
        }
        let parent = change("assistant","owner",3,json!({"fields":{"name":"revived"},"refs":{}}));
        apply_change_to_db(&sync,&data,&spec("assistant"),&parent,"local", |_|Ok(())).unwrap();
        let child = change("assistant.prompt","prompt",4,json!({"fields":{"prompt":"revived"},"refs":{"assistant_id":"owner"}}));
        for _ in 0..2 {
            apply_change_to_db(&sync,&data,&spec("assistant.prompt"),&child,"local", |_|Ok(())).unwrap();
        }
        assert_eq!(data.query_row("SELECT COUNT(*) FROM assistant_prompt p JOIN assistant a ON a.id=p.assistant_id WHERE a.name='revived' AND p.prompt='revived'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_ne!(find_local_id_by_object_id(&sync,"assistant.prompt","prompt").unwrap(),Some(10));
        assert_eq!(find_local_id_by_object_id(&sync,"assistant.prompt","prompt-alias").unwrap(),None);
    }
}

fn sync_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    conn
}

fn spec(name: &str) -> SyncTableSpec {
    specs().into_iter().find(|s| s.object_type == name).unwrap()
}

fn change(object_type: &str, object_id: &str, version: i64, payload: JsonValue) -> PullChange {
    serde_json::from_value(json!({
        "seq": version, "event_id": format!("event-{object_id}-{version}"), "device_id": "other",
        "object_type": object_type, "object_id": object_id, "operation": "upsert",
        "version": version, "payload": payload, "deleted_at": null, "created_at": "now"
    })).unwrap()
}

fn snapshot(conn: &Connection, content: &str) -> LocalObjectSnapshot {
    let payload = json!({"fields": {"content": content}, "refs": {}});
    LocalObjectSnapshot {
        object_type: "conversation.message".into(), object_id: "message-1".into(),
        payload_hash: sync_payload_hash(conn, Some(&payload)).unwrap(),
        payload_json: payload.to_string(),
    }
}

#[test]
fn test_llm_catalogue_children_wait_for_required_provider() {
    for kind in ["llm.model", "llm.provider_config", "llm.model_request_mode_preference"] {
        let spec = spec(kind);
        let mut fields = Map::from_iter([("llm_provider_id".into(), json!(71))]);
        let refs = Map::from_iter([("llm_provider_id".into(), json!("original-provider"))]);
        let error = resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(None)).unwrap_err();
        assert!(is_dependency_pending_error(&error), "{kind}: {error}");
        resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(Some(209))).unwrap();
        assert_eq!(fields["llm_provider_id"], json!(209));
        let raw = HashMap::from_iter([("llm_provider_id".into(), json!(71))]);
        assert!(build_fk_refs(&spec, &raw, &mut fields, |_, _| Ok(FkLookup::Dangling), None).unwrap().is_none());
    }
}

#[test]
fn test_sync_official_flag_keeps_imported_identity_without_rowid_collision() {
    let sync = sync_db();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE llm_provider(id INTEGER PRIMARY KEY, name TEXT, is_official INTEGER);
        INSERT INTO llm_provider VALUES(40, 'imported', 1), (41, 'actual seed', 1);").unwrap();
    remap_sync_object_map(&sync, "llm.provider", "llm_provider", 40, "imported-uuid").unwrap();
    // A remote official identity has a different local rowid on this device.
    remap_sync_object_map(&sync, "llm.provider", "llm_provider", 41, "official:llm.provider:40").unwrap();
    let imported = change("llm.provider", "imported-uuid", 1,
        json!({"fields":{"name":"imported","is_official":1},"refs":{}}));
    upsert_shadow_from_remote(&sync, &imported).unwrap();
    for _ in 0..2 {
        let (snapshots, _) = read_local_snapshots_from_db(&data, &spec("llm.provider"), &sync,
            &mut DanglingFkScanReport::default(), |_, _| Ok(true)).unwrap();
        assert_eq!(snapshots[0].object_id, "imported-uuid");
        assert_eq!(snapshots[1].object_id, "official:llm.provider:40");
        assert!(!enqueue_snapshot_if_changed(&sync, &snapshots[0], "device-b").unwrap());
        assert_eq!(find_local_id_by_object_id(&sync, "llm.provider", "official:llm.provider:40").unwrap(), Some(41));
    }
}

#[test]
fn test_sync_official_provider_import_does_not_overwrite_occupied_custom_rowid() {
    for has_mapping in [false, true] {
        let sync = sync_db();
        let data = Connection::open_in_memory().unwrap();
        data.execute_batch("CREATE TABLE llm_provider(id INTEGER PRIMARY KEY, name TEXT, is_official INTEGER);
            INSERT INTO llm_provider VALUES(40, 'unrelated local provider', 0);").unwrap();
        if has_mapping { remap_sync_object_map(&sync, "llm.provider", "llm_provider", 40, "local-provider-uuid").unwrap(); }
        let event = change("llm.provider", "official:llm.provider:40", 1,
            json!({"fields":{"name":"remote official provider","is_official":1},"refs":{}}));
        for _ in 0..2 {
            apply_change_to_db(&sync, &data, &spec("llm.provider"), &event, "b", |_| Ok(())).unwrap();
        }
        assert_eq!(data.query_row("SELECT name FROM llm_provider WHERE id=40", [], |r|r.get::<_,String>(0)).unwrap(), "unrelated local provider");
        assert_eq!(data.query_row("SELECT COUNT(*) FROM llm_provider", [], |r|r.get::<_,i64>(0)).unwrap(), 2);
        assert_eq!(find_local_id_by_object_id(&sync, "llm.provider", "official:llm.provider:40").unwrap(), Some(41));
        let (snapshots, _) = read_local_snapshots_from_db(&data, &spec("llm.provider"), &sync,
            &mut DanglingFkScanReport::default(), |_, _| Ok(true)).unwrap();
        assert_eq!(snapshots[1].object_id, "official:llm.provider:40");
        assert!(!enqueue_snapshot_if_changed(&sync, &snapshots[1], "b").unwrap());
    }
}

#[test]
fn test_sync_server_history_rollback_stops_with_old_receipts_intact() {
    let sync = sync_db();
    let old = change("assistant", "missing-parent", 1, json!({"fields":{"name":"original"},"refs":{}}));
    upsert_shadow_from_remote(&sync, &old).unwrap();
    sync.execute("INSERT INTO sync_cursor VALUES ('default', 118444, 'now')", []).unwrap();
    let error = ensure_remote_history_is_compatible(&sync, 33289).unwrap_err();
    assert!(error.contains("118444") && error.contains("33289") && error.contains("停止"));
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_shadow", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    ensure_remote_history_is_compatible(&sync, 118444).unwrap();
    ensure_remote_history_is_compatible(&sync, 118445).unwrap();
}

#[test]
fn test_sync_server_history_reset_cannot_hide_confirmed_sequences() {
    let sync = sync_db();
    let mut old = change("assistant", "original", 1, json!({"fields":{"name":"original"},"refs":{}}));
    old.seq = 118444;
    upsert_shadow_from_remote(&sync, &old).unwrap();
    clear_sync_state(&sync).unwrap();
    assert!(ensure_remote_history_is_compatible(&sync, 33289).is_err());
    assert!(ensure_remote_history_is_compatible(&sync, 0).is_err());
}

#[test]
fn test_sync_server_history_empty_server_rejects_legacy_receipts_but_allows_new_device() {
    let sync = sync_db();
    ensure_remote_history_is_compatible(&sync, 0).unwrap();
    ensure_remote_history_is_compatible(&sync, 33289).unwrap();
    let mut old = change("assistant", "original", 1, json!({"fields":{"name":"original"},"refs":{}}));
    old.seq = 0; // Old clients did not record per-object server_seq.
    upsert_shadow_from_remote(&sync, &old).unwrap();
    assert!(ensure_remote_history_is_compatible(&sync, 0).is_err());
}

#[test]
fn test_remote_optional_conversation_refs_clear_missing_targets() {
    let spec = spec("conversation");
    let mut fields = Map::from_iter([
        ("name".into(), json!("historical")),
        ("assistant_id".into(), json!(49)),
        ("parent_butler_conversation_id".into(), JsonValue::Null),
    ]);
    let refs = Map::from_iter([("assistant_id".into(), json!("missing-assistant"))]);
    resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(None)).unwrap();
    assert_eq!(fields.get("assistant_id"), Some(&JsonValue::Null));
}

#[test]
fn test_legacy_sync_schema_migration_preserves_attempted_events() {
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch("CREATE TABLE sync_outbox(id INTEGER PRIMARY KEY);
        INSERT INTO sync_outbox VALUES(1);
        CREATE TABLE sync_shadow(object_id TEXT PRIMARY KEY);
        INSERT INTO sync_shadow VALUES('old');").unwrap();
    ensure_sync_db_columns(&sync).unwrap();
    ensure_sync_db_columns(&sync).unwrap();
    assert_eq!(sync.query_row("SELECT sent_once FROM sync_outbox", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(sync.query_row("SELECT server_seq FROM sync_shadow", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_retry_attempted_event_preserves_original_base_and_payload() {
    let sync = sync_db();
    upsert_shadow_from_remote(&sync, &change("conversation", "c", 9,
        json!({"fields":{"name":"latest"}}))).unwrap();
    sync.execute("INSERT INTO sync_outbox
        (event_id, object_type, object_id, operation, base_version, payload_json, local_version, device_id, status, sent_once, created_at)
        VALUES ('original', 'conversation', 'c', 'upsert', 2, '{\"fields\":{\"name\":\"old\"}}', 1, 'device', 'failed', 1, 'now')", []).unwrap();
    assert_eq!(reset_failed_events_inner(&sync).unwrap(), 1);
    let result = sync.query_row("SELECT event_id, base_version, payload_json, status, sent_once FROM sync_outbox", [], |row|
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?,
            row.get::<_, String>(3)?, row.get::<_, i64>(4)?))).unwrap();
    assert_eq!(result, ("original".into(), 2, "{\"fields\":{\"name\":\"old\"}}".into(), "pending".into(), 1));
}

#[test]
fn test_payload_hash_ignores_device_local_foreign_keys() {
    let sync = sync_db();
    let source = json!({"fields":{"conversation_id":7,"llm_model_id":2,"content":"hello"},
        "refs":{"conversation_id":"conv-shared","llm_model_id":"model-shared"}});
    let mut destination = source.clone();
    destination["fields"]["conversation_id"] = json!(400);
    destination["fields"]["llm_model_id"] = json!(800);
    assert_eq!(sync_payload_hash(&sync, Some(&source)).unwrap(), sync_payload_hash(&sync, Some(&destination)).unwrap());
    destination["fields"]["content"] = json!("changed");
    assert_ne!(sync_payload_hash(&sync, Some(&source)).unwrap(), sync_payload_hash(&sync, Some(&destination)).unwrap());
}

#[test]
fn test_remote_missing_soft_reference_does_not_echo_after_pull() {
    let sync = sync_db();
    let spec = spec("conversation");
    let remote = json!({
        "fields": {"name": "historical", "assistant_id": 49},
        "refs": {"assistant_id": "missing-assistant"}
    });
    let mut resolved_fields = remote["fields"].as_object().unwrap().clone();
    resolved_fields.insert("assistant_id".into(), JsonValue::Null);
    let change = change("conversation", "conversation-1", 1, remote);
    upsert_shadow_from_remote_with_fields(&sync, &spec, &change, &resolved_fields).unwrap();

    let local = json!({
        "fields": {"name": "historical", "assistant_id": null},
        "refs": {}
    });
    let shadow_hash: String = sync
        .query_row(
            "SELECT payload_hash FROM sync_shadow WHERE object_type = 'conversation' AND object_id = 'conversation-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(shadow_hash, sync_payload_hash(&sync, Some(&local)).unwrap());
}

#[test]
fn test_embedded_provider_values_resolve_without_losing_model_identity() {
    let sync = sync_db();
    let source = json!({"fields":{"feature_code":"experimental","key":"butler_model_id","value":"model-a%%28"},
        "refs":{"value_provider_id":"provider-shared"}});
    let mut local = source.clone();
    resolve_remote_foreign_keys(&spec("system.feature_config"), local["fields"].as_object_mut().unwrap(),
        source["refs"].as_object().unwrap(), |kind, id| {
            assert_eq!((kind, id), ("llm.provider", "provider-shared")); Ok(Some(99))
        }).unwrap();
    assert_eq!(local["fields"]["value"], json!("model-a%%99"));
    assert_eq!(sync_payload_hash(&sync, Some(&source)).unwrap(), sync_payload_hash(&sync, Some(&local)).unwrap());
    local["fields"]["value"] = json!("model-b%%99");
    assert_ne!(sync_payload_hash(&sync, Some(&source)).unwrap(), sync_payload_hash(&sync, Some(&local)).unwrap());
    let err = resolve_remote_foreign_keys(&spec("system.feature_config"), local["fields"].as_object_mut().unwrap(),
        &Map::new(), |_, _| panic!("raw IDs must be rejected")).unwrap_err();
    assert!(err.contains("value_provider_id"));
    let mut fields = json!({"feature_code":"conversation_summary","key":"title_provider_id","value":"28"}).as_object().unwrap().clone();
    resolve_embedded_provider_value("system.feature_config", &mut fields,
        source["refs"].as_object().unwrap(), |_, _| Ok(Some(99))).unwrap();
    assert_eq!(fields["value"], json!("99"));
    fields = json!({"name":"acp_provider","value":"28"}).as_object().unwrap().clone();
    resolve_embedded_provider_value("assistant.model_config", &mut fields,
        source["refs"].as_object().unwrap(), |_, _| Ok(Some(99))).unwrap();
    assert_eq!(fields["value"], json!("99"));
}

#[test]
fn test_unique_model_merge_keeps_both_reference_identities() {
    let sync = sync_db();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE llm_model (id INTEGER PRIMARY KEY, llm_provider_id INTEGER, code TEXT, name TEXT, UNIQUE(llm_provider_id, code));
        INSERT INTO llm_model VALUES (80, 9, 'model-a', 'local');").unwrap();
    remap_sync_object_map(&sync, "llm.model", "llm_model", 80, "old-id").unwrap();
    let fields = json!({"llm_provider_id":9,"code":"model-a","name":"remote"}).as_object().unwrap().clone();
    upsert_by_unique_key(&sync, &data, &spec("llm.model"), "new-id", &fields).unwrap();
    assert_eq!(find_local_id_by_object_id(&sync, "llm.model", "old-id").unwrap(), Some(80));
    assert_eq!(find_local_id_by_object_id(&sync, "llm.model", "new-id").unwrap(), Some(80));
    assert_eq!(data.query_row("SELECT COUNT(*) FROM llm_model", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    let a = json!({"fields":{"llm_model_id":1},"refs":{"llm_model_id":"old-id"}});
    let b = json!({"fields":{"llm_model_id":80},"refs":{"llm_model_id":"new-id"}});
    assert_eq!(sync_payload_hash(&sync, Some(&a)).unwrap(), sync_payload_hash(&sync, Some(&b)).unwrap());
}

#[test]
fn test_alias_delete_preserves_canonical_model_and_canonical_delete_clears_aliases() {
    let sync = sync_db();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE llm_model(id INTEGER PRIMARY KEY); INSERT INTO llm_model VALUES (80);").unwrap();
    remap_sync_object_map(&sync, "llm.model", "llm_model", 80, "old").unwrap();
    remap_sync_object_map(&sync, "llm.model", "llm_model", 80, "new").unwrap();
    let mut deletion = change("llm.model", "old", 2, JsonValue::Null);
    deletion.operation = "delete".into();
    apply_delete(&sync, &data, &spec("llm.model"), &deletion).unwrap();
    assert!(row_exists_by_id(&data, &spec("llm.model"), 80).unwrap());
    assert_eq!(find_local_id_by_object_id(&sync, "llm.model", "old").unwrap(), None);
    remap_sync_object_map(&sync, "llm.model", "llm_model", 80, "newest").unwrap();
    deletion.object_id = "newest".into();
    apply_delete(&sync, &data, &spec("llm.model"), &deletion).unwrap();
    assert!(!row_exists_by_id(&data, &spec("llm.model"), 80).unwrap());
    assert_eq!(find_local_id_by_object_id(&sync, "llm.model", "new").unwrap(), None);
}

#[test]
fn test_old_remote_and_ack_do_not_roll_back_shadow_or_mapping() {
    let sync = sync_db();
    let latest = change("conversation", "c", 9, json!({"fields":{"name":"latest"}}));
    upsert_shadow_from_remote(&sync, &latest).unwrap();
    remap_sync_object_map(&sync, "conversation", "conversation", 2, "c").unwrap();
    let older = change("conversation", "c", 2, json!({"fields":{"name":"old"}}));
    assert!(remote_change_already_applied(&sync, &older).unwrap());
    upsert_shadow_from_remote(&sync, &older).unwrap();
    apply_ack_to_shadow(&sync, "conversation", "c", 3, 0, "delete", "old").unwrap();
    assert_eq!(sync.query_row("SELECT server_version FROM sync_shadow", [], |r| r.get::<_, i64>(0)).unwrap(), 9);
    assert_eq!(find_local_id_by_object_id(&sync, "conversation", "c").unwrap(), Some(2));
}

#[test]
fn test_merged_alias_replay_uses_sequence_instead_of_independent_versions() {
    let sync = sync_db();
    remap_sync_object_map(&sync, "llm.model", "llm_model", 80, "old-id").unwrap();
    remap_sync_object_map(&sync, "llm.model", "llm_model", 80, "new-id").unwrap();
    let mut latest = change("llm.model", "new-id", 2, json!({"fields":{"name":"latest"}}));
    latest.seq = 100;
    upsert_shadow_from_remote(&sync, &latest).unwrap();
    let mut old = change("llm.model", "old-id", 500, json!({"fields":{"name":"old"}}));
    old.seq = 99;
    assert!(remote_change_already_applied(&sync, &old).unwrap());
    old.seq = 101;
    assert!(!remote_change_already_applied(&sync, &old).unwrap());
    old.seq = 0; // Conflict replies do not carry a global sequence.
    assert!(!remote_change_already_applied(&sync, &old).unwrap());
}

#[test]
fn test_orphan_message_is_blocked_and_remote_error_names_missing_owner() {
    let mut fields = json!({"conversation_id":null,"content":"orphan"}).as_object().unwrap().clone();
    let raw = fields.clone().into_iter().collect();
    assert!(build_fk_refs(&spec("conversation.message"), &raw, &mut fields, |_, _| panic!("no numeric ref"), None).unwrap().is_none());
    let err = resolve_remote_foreign_keys(&spec("conversation.message"), &mut fields, &Map::new(), |_, _| Ok(None)).unwrap_err();
    assert!(err.contains("conversation_id") && err.contains("refs"));
    assert!(!is_unique_constraint_error("NOT NULL constraint failed: assistant_model.assistant_id"));
}

#[test]
fn test_unbound_assistant_model_config_keeps_sentinel() {
    for value in [json!(-1), JsonValue::Null] {
        let mut fields = json!({"assistant_id":1,"assistant_model_id":value,"name":"temperature"}).as_object().unwrap().clone();
        let raw = fields.clone().into_iter().collect();
        let refs = build_fk_refs(&spec("assistant.model_config"), &raw, &mut fields, |kind, _| {
            assert_eq!(kind, "assistant"); Ok(FkLookup::Resolved("assistant-shared".into()))
        }, None).unwrap().unwrap();
        assert!(!refs.contains_key("assistant_model_id"));
        resolve_remote_foreign_keys(&spec("assistant.model_config"), &mut fields, &refs, |_, _| Ok(Some(55))).unwrap();
        assert_eq!(fields["assistant_id"], json!(55));
        assert_eq!(fields["assistant_model_id"], json!(-1));
    }
}

#[test]
fn test_unsent_streaming_snapshots_coalesce_but_retries_are_immutable() {
    let sync = sync_db();
    enqueue_snapshot_if_changed(&sync, &snapshot(&sync, "first"), "device").unwrap();
    enqueue_snapshot_if_changed(&sync, &snapshot(&sync, "second"), "device").unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    sync.execute("UPDATE sync_outbox SET sent_once = 1, retry_count = 1", []).unwrap();
    let original: String = sync.query_row("SELECT event_id FROM sync_outbox", [], |r| r.get(0)).unwrap();
    enqueue_snapshot_if_changed(&sync, &snapshot(&sync, "latest"), "device").unwrap();
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(sync.query_row("SELECT payload_json FROM sync_outbox WHERE event_id = ?1", params![original], |r| r.get::<_, String>(0)).unwrap(), snapshot(&sync, "second").payload_json);
}

#[test]
fn test_reverting_unsent_edit_to_shadow_drops_obsolete_upload() {
    let sync = sync_db();
    let original = snapshot(&sync, "original");
    apply_ack_to_shadow(&sync, &original.object_type, &original.object_id, 1, 0, "upsert", &original.payload_hash).unwrap();
    enqueue_snapshot_if_changed(&sync, &snapshot(&sync, "temporary"), "device").unwrap();
    assert!(!enqueue_snapshot_if_changed(&sync, &original, "device").unwrap());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_dead_letter_replay_resolves_reversed_dependencies_in_one_request() {
    let sync = sync_db();
    for id in ["child", "parent", "orphan"] {
        record_dead_letter(&sync, &change("conversation", id, 1, json!({})), "initial").unwrap();
    }
    let mut parent_ready = false;
    let mut attempts = 0;
    let result = replay_dead_letter_rows(&sync, |c| {
        attempts += 1;
        match c.object_id.as_str() {
            "child" if !parent_ready => Err("缺少同步依赖: child -> parent".into()),
            "parent" => { parent_ready = true; Ok(()) },
            "orphan" => Err("必需归属 conversation_id 为空且缺少 refs".into()),
            _ => Ok(()),
        }
    }).unwrap();
    assert_eq!(result, (2, 1));
    assert_eq!(attempts, 4);
    let summary = summarize_dead_letters(&sync).unwrap().unwrap();
    assert!(summary.contains("orphan") && summary.contains("conversation_id"));
}

#[test]
fn test_remote_turn_response_and_descendants_reuse_one_fork() {
    let sync = sync_db();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY); INSERT INTO conversation VALUES (1), (2);").unwrap();
    let remote = json!({"conversation_id":1,"generation_group_id":"remote-turn"}).as_object().unwrap().clone();
    remember_sync_branch(&sync, 1, &remote, 2).unwrap();
    let mut response = remote.clone();
    assert!(route_sync_branch(&sync, &data, &mut response).unwrap());
    assert_eq!(response["conversation_id"], json!(2));
    let mut child = json!({"conversation_id":1,"generation_group_id":"next-turn","parent_group_id":"remote-turn"}).as_object().unwrap().clone();
    assert!(route_sync_branch(&sync, &data, &mut child).unwrap());
    assert_eq!(child["conversation_id"], json!(2));
    child["conversation_id"] = json!(1);
    child.remove("parent_group_id");
    assert!(route_sync_branch(&sync, &data, &mut child).unwrap());
}

#[test]
fn test_sync_unknown_outcome_keeps_identity_blocks_successors_and_pull_echo_preserves_latest() {
    let sync = sync_db();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO conversation VALUES(1,'latest')").unwrap();
    remap_sync_object_map(&sync,"conversation","conversation",1,"c").unwrap();
    let make = |name: &str| {
        let payload = json!({"fields":{"name":name},"refs":{}});
        LocalObjectSnapshot {object_type:"conversation".into(),object_id:"c".into(),payload_json:payload.to_string(),payload_hash:sync_payload_hash(&sync,Some(&payload)).unwrap()}
    };
    enqueue_snapshot_if_changed(&sync,&make("old"),"local").unwrap();
    let first = load_pending_outbox_from_db(&sync,10).unwrap();
    mark_events_pushing_in_db(&sync,&first).unwrap();
    for _ in 0..20 { record_event_failure(&sync,first[0].0,"ACK lost").unwrap(); }
    enqueue_snapshot_if_changed(&sync,&make("latest"),"local").unwrap();
    assert!(load_pending_outbox_from_db(&sync,10).unwrap().is_empty());
    // A legacy failed attempted event must also block the new edit.
    sync.execute("UPDATE sync_outbox SET status='failed' WHERE id=?1",params![first[0].0]).unwrap();
    assert!(load_pending_outbox_from_db(&sync,10).unwrap().is_empty());
    let mut echo = change("conversation","c",1,first[0].1.payload.clone().unwrap());
    echo.event_id=first[0].1.event_id.clone(); echo.seq=90;
    apply_change_to_db(&sync,&data,&spec("conversation"),&echo,"local",|_|Ok(())).unwrap();
    assert_eq!(data.query_row("SELECT name FROM conversation",[],|r|r.get::<_,String>(0)).unwrap(),"latest");
    let next=load_pending_outbox_from_db(&sync,10).unwrap();
    assert_eq!(next.len(),1); assert_eq!(next[0].1.base_version,Some(1));
    assert_eq!(next[0].1.payload.as_ref().unwrap()["fields"]["name"],"latest");
}

#[test]
fn test_sync_attempted_delete_intent_survives_echo_for_message_and_natural_config() {
    for kind in ["conversation.message","system.feature_config"] {
        for attempted in [false,true] {
            let sync=sync_db(); let data=Connection::open_in_memory().unwrap();
            data.execute_batch("CREATE TABLE feature_config(id INTEGER PRIMARY KEY,feature_code TEXT,key TEXT,value TEXT);
                CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,content TEXT)").unwrap();
            let payload=if kind=="system.feature_config" {json!({"fields":{"feature_code":"f","key":"k","value":"v"},"refs":{}})}
                else {json!({"fields":{"conversation_id":1,"content":"v"},"refs":{"conversation_id":"c"}})};
            let id=if kind=="system.feature_config" {natural_object_id(&spec(kind),&HashMap::from_iter([("feature_code".into(),json!("f")),("key".into(),json!("k"))])).unwrap()} else {"m".into()};
            let row=LocalObjectSnapshot{object_type:kind.into(),object_id:id.clone(),payload_json:payload.to_string(),payload_hash:sync_payload_hash(&sync,Some(&payload)).unwrap()};
            enqueue_snapshot_if_changed(&sync,&row,"local").unwrap();
            let events=load_pending_outbox_from_db(&sync,10).unwrap();
            if attempted {mark_events_pushing_in_db(&sync,&events).unwrap();}
            scan_local_deletes_in_db(&sync,&spec(kind),&HashSet::new(),|_|Ok(false)).unwrap();
            if !attempted {
                assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),0);
                continue;
            }
            assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),2);
            let mut echo=change(kind,&id,1,payload); echo.event_id=events[0].1.event_id.clone();
            apply_change_to_db(&sync,&data,&spec(kind),&echo,"local",|_|Ok(())).unwrap();
            let next=load_pending_outbox_from_db(&sync,10).unwrap();
            assert_eq!(next.len(),1);assert_eq!(next[0].1.operation,"delete");assert_eq!(next[0].1.base_version,Some(1));
            // A replay using a different event ID also cannot restore the row.
            echo.event_id="another-author".into();echo.version=2;echo.seq=2;
            apply_change_to_db(&sync,&data,&spec(kind),&echo,"local",|_|Ok(())).unwrap();
            let table=if kind=="system.feature_config" {"feature_config"} else {"message"};
            assert_eq!(data.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0);
        }
    }
}

#[test]
fn test_sync_ack_failure_rolls_back_receipt_outbox_and_successor_rebase() {
    let sync=sync_db();
    enqueue_snapshot_if_changed(&sync,&snapshot(&sync,"first"),"local").unwrap();
    let events=load_pending_outbox_from_db(&sync,10).unwrap();
    mark_events_pushing_in_db(&sync,&events).unwrap();
    enqueue_snapshot_if_changed(&sync,&snapshot(&sync,"second"),"local").unwrap();
    sync.execute_batch("CREATE TRIGGER fail_ack BEFORE DELETE ON sync_outbox BEGIN SELECT RAISE(ABORT,'cannot delete outbox'); END").unwrap();
    let ack=AcceptedEvent{event_id:events[0].1.event_id.clone(),object_type:events[0].1.object_type.clone(),object_id:events[0].1.object_id.clone(),server_version:2,server_seq:20};
    assert!(consume_accepted_event(&sync,events[0].0,&events[0].1,&ack).is_err());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_shadow",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    sync.execute_batch("DROP TRIGGER fail_ack").unwrap();
    consume_accepted_event(&sync,events[0].0,&events[0].1,&ack).unwrap();
    apply_ack_to_shadow(&sync,&ack.object_type,&ack.object_id,2,0,"upsert","same-version").unwrap();
    apply_ack_to_shadow(&sync,&ack.object_type,&ack.object_id,1,5,"upsert","older").unwrap();
    assert_eq!(sync.query_row("SELECT server_seq FROM sync_shadow",[],|r|r.get::<_,i64>(0)).unwrap(),20);
    assert_eq!(load_pending_outbox_from_db(&sync,10).unwrap()[0].1.base_version,Some(2));
}

#[test]
fn test_sync_bootstrap_recovery_requires_bound_origin_and_recorded_attempts() {
    let sync=sync_db();
    enqueue_snapshot_if_changed(&sync,&snapshot(&sync,"first"),"local").unwrap();
    assert!(ensure_bootstrap_origin(&sync,"https://one","account-a").is_err());
    assert!(load_outbox_from_db(&sync,10,true).unwrap().is_empty());
    bind_remote_account(&sync,"https://one/","account-a",true).unwrap();
    let events=load_pending_outbox_from_db(&sync,10).unwrap();
    mark_events_pushing_in_db(&sync,&events).unwrap();
    record_event_failure(&sync,events[0].0,"first response lost").unwrap();
    enqueue_snapshot_if_changed(&sync,&snapshot(&sync,"latest"),"local").unwrap();
    ensure_bootstrap_origin(&sync,"https://one","account-a").unwrap();
    assert!(ensure_bootstrap_origin(&sync,"https://one","account-b").is_err());
    assert!(bind_remote_account(&sync,"https://one","account-b",false).is_err());
    assert!(ensure_bootstrap_origin(&sync,"https://two","account-a").is_err());
    let retry=load_outbox_from_db(&sync,10,true).unwrap();
    assert_eq!(retry.len(),1);assert_eq!(retry[0].1.event_id,events[0].1.event_id);
    let ack=AcceptedEvent{event_id:retry[0].1.event_id.clone(),object_type:retry[0].1.object_type.clone(),object_id:retry[0].1.object_id.clone(),server_version:1,server_seq:1};
    consume_accepted_event(&sync,retry[0].0,&retry[0].1,&ack).unwrap();
    assert!(load_outbox_from_db(&sync,10,true).unwrap().is_empty());
    ensure_initial_sync_is_safe(&sync,false,false).unwrap();
}

#[test]
fn test_sync_bounded_natural_identity_retains_keys_for_remote_delete() {
    let sync=sync_db(); let data=Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE feature_config(id INTEGER PRIMARY KEY,feature_code TEXT,key TEXT,value TEXT)").unwrap();
    let spec=spec("system.feature_config");
    let key="长配置项".repeat(100);
    let fields=HashMap::from_iter([("feature_code".into(),json!("feature")),("key".into(),json!(key))]);
    let id=natural_object_id(&spec,&fields).unwrap();assert!(id.starts_with("natural-sha256:"));assert!(valid_sync_id(&id));
    let mut event=change(spec.object_type,&id,1,json!({"fields":{"feature_code":"feature","key":key,"value":"v"},"refs":{}}));
    apply_change_to_db(&sync,&data,&spec,&event,"local",|_|Ok(())).unwrap();
    assert!(local_row_exists_in_db(&sync,&data,&spec,&id).unwrap());
    event.version=2;event.seq=2;event.operation="delete".into();event.payload=None;
    apply_change_to_db(&sync,&data,&spec,&event,"local",|_|Ok(())).unwrap();
    assert!(!local_row_exists_in_db(&sync,&data,&spec,&id).unwrap());
    // No-key tombstone is valid on a fresh receiver.
    let fresh=sync_db(); apply_change_to_db(&fresh,&data,&spec,&event,"local",|_|Ok(())).unwrap();
    let a=HashMap::from_iter([("feature_code".into(),json!("a\u{1f}b")),("key".into(),json!("c"))]);
    let b=HashMap::from_iter([("feature_code".into(),json!("a")),("key".into(),json!("b\u{1f}c"))]);
    assert_ne!(natural_object_id(&spec,&a).unwrap(),natural_object_id(&spec,&b).unwrap());
}

#[test]
fn test_sync_invalid_outbox_id_isolates_only_bad_event() {
    let sync=sync_db();
    let mut bad=snapshot(&sync,"bad");bad.object_id="invalid @ model".into();
    enqueue_snapshot_if_changed(&sync,&bad,"local").unwrap();
    enqueue_snapshot_if_changed(&sync,&snapshot(&sync,"good"),"local").unwrap();
    let events=load_pending_outbox_from_db(&sync,10).unwrap();
    assert_eq!(events.len(),1);assert_eq!(events[0].1.object_id,"message-1");
    assert_eq!(sync.query_row("SELECT status FROM sync_outbox WHERE object_id=?1",params![bad.object_id],|r|r.get::<_,String>(0)).unwrap(),"failed");
}

#[test]
fn test_sync_push_response_requires_exact_complete_matching_results() {
    let sync = sync_db();
    enqueue_snapshot_if_changed(&sync, &snapshot(&sync, "first"), "local").unwrap();
    let events = load_pending_outbox_from_db(&sync, 10).unwrap();
    let event = &events[0].1;
    let accepted = AcceptedEvent { event_id: event.event_id.clone(), object_type: event.object_type.clone(),
        object_id: event.object_id.clone(), server_version: 1, server_seq: 1 };
    let mut response = PushResponse { accepted: vec![], rejected: vec![], conflicts: vec![] };
    assert!(validate_push_response(&events, &response).unwrap_err().contains("缺少"));
    response.accepted.push(accepted.clone());
    validate_push_response(&events, &response).unwrap();
    response.rejected.push(RejectedEvent { event_id: event.event_id.clone(), reason: "duplicate".into() });
    assert!(validate_push_response(&events, &response).unwrap_err().contains("重复"));
    response.rejected.clear();
    response.accepted[0].object_id = "wrong-object".into();
    assert!(validate_push_response(&events, &response).unwrap_err().contains("身份不匹配"));
    response.accepted[0] = accepted;
    response.accepted[0].event_id = "unknown-event".into();
    assert!(validate_push_response(&events, &response).unwrap_err().contains("未提交"));
    response.accepted.clear();
    response.conflicts.push(ConflictEvent { event_id: event.event_id.clone(), object_type: event.object_type.clone(),
        object_id: "wrong-object".into(), server_version: 1, server_payload: None, server_operation: "delete".into() });
    assert!(validate_push_response(&events, &response).unwrap_err().contains("身份不匹配"));
}

/// Parent-first tombstones must use production cleanup even on connections
/// without foreign-key enforcement, and leave child deletes visible to scanning.
#[test]
fn test_cascade_delete_sync_parent_first_replay_and_scan_do_not_resurrect_children() {
    for foreign_keys in [false, true] {
        let sync = sync_db();
        let data = Connection::open_in_memory().unwrap();
        data.pragma_update(None, "foreign_keys", foreign_keys).unwrap();
        data.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT);
            CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER REFERENCES conversation(id), parent_id INTEGER);
            CREATE TABLE message_attachment(id INTEGER PRIMARY KEY, message_id INTEGER REFERENCES message(id));
            CREATE TABLE acp_session(conversation_id INTEGER, session_id TEXT);
            INSERT INTO conversation VALUES(1,'delete'),(2,'keep');
            INSERT INTO message VALUES(11,1,NULL),(22,2,11);
            INSERT INTO message_attachment VALUES(111,11),(222,22);
            INSERT INTO acp_session VALUES(1,'session'),(2,'keep');").unwrap();
        for (kind,table,id,key) in [("conversation","conversation",1,"owner"),
            ("conversation.message","message",11,"message"),("conversation.message_attachment","message_attachment",111,"attachment")] {
            save_object_map(&sync,kind,table,id,key).unwrap();
            upsert_shadow_from_remote(&sync,&change(kind,key,1,json!({"fields":{}}))).unwrap();
        }
        let mut deletion = change("conversation","owner",2,JsonValue::Null);
        deletion.operation="delete".into(); deletion.deleted_at=Some("now".into());
        apply_change_to_db(&sync,&data,&spec("conversation"),&deletion,"local", |_|Ok(())).unwrap();
        apply_change_to_db(&sync,&data,&spec("conversation"),&deletion,"local", |_|Ok(())).unwrap();
        assert_eq!(data.query_row("SELECT COUNT(*) FROM message_attachment WHERE message_id=11",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(data.query_row("SELECT COUNT(*) FROM message WHERE id=22 AND parent_id IS NULL",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(data.query_row("SELECT COUNT(*) FROM acp_session WHERE conversation_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert!(find_deleted_candidates(&sync,"conversation",&HashSet::new()).unwrap().is_empty());
        for kind in ["conversation.message","conversation.message_attachment"] {
            assert_eq!(scan_local_deletes_in_db(&sync,&spec(kind),&HashSet::new(),|_|Ok(false)).unwrap(),1);
            assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox WHERE object_type=?1 AND operation='delete'",[kind],|r|r.get::<_,i64>(0)).unwrap(),1);
            deletion.object_type=kind.into();
            deletion.object_id=if kind=="conversation.message" {"message"} else {"attachment"}.into();
            apply_change_to_db(&sync,&data,&spec(kind),&deletion,"local", |_|Ok(())).unwrap();
            assert_eq!(find_local_id_by_object_id(&sync,kind,&deletion.object_id).unwrap(),None);
        }
    }
}

#[test]
fn test_cascade_delete_sync_failure_keeps_mapping_and_does_not_ack_tombstone() {
    let sync=sync_db(); let data=Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY);
        CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER);
        CREATE TABLE message_attachment(id INTEGER PRIMARY KEY, message_id INTEGER);
        INSERT INTO conversation VALUES(1); INSERT INTO message VALUES(11,1); INSERT INTO message_attachment VALUES(111,11);
        CREATE TRIGGER fail_message_delete BEFORE DELETE ON message BEGIN SELECT RAISE(ABORT,'injected message delete failure'); END;").unwrap();
    save_object_map(&sync,"conversation","conversation",1,"owner").unwrap();
    let mut deletion=change("conversation","owner",2,JsonValue::Null);
    deletion.operation="delete".into(); deletion.deleted_at=Some("now".into());
    let error=apply_change_to_db(&sync,&data,&spec("conversation"),&deletion,"local", |_|Ok(())).unwrap_err();
    assert!(error.contains("owner") && error.contains("injected message delete failure"),"{error}");
    assert_eq!(find_local_id_by_object_id(&sync,"conversation","owner").unwrap(),Some(1));
    assert_eq!(data.query_row("SELECT COUNT(*) FROM message_attachment",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert!(!remote_change_already_applied(&sync,&deletion).unwrap());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_shadow",[],|r|r.get::<_,i64>(0)).unwrap(),0);
}

#[test]
fn test_cascade_delete_collected_artifact_survives_missing_assistant_on_import() {
    let sync=sync_db(); let data=Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE artifacts_collection(id INTEGER PRIMARY KEY, name TEXT, code TEXT, assistant_id INTEGER);").unwrap();
    let event=change("artifacts.collection","artifact",1,json!({
        "fields":{"name":"saved","code":"keep content","assistant_id":99},
        "refs":{"assistant_id":"deleted-assistant"}
    }));
    apply_change_to_db(&sync,&data,&spec("artifacts.collection"),&event,"local", |_|Ok(())).unwrap();
    assert_eq!(data.query_row("SELECT COUNT(*) FROM artifacts_collection WHERE code='keep content' AND assistant_id IS NULL",[],|r|r.get::<_,i64>(0)).unwrap(),1);
}

#[test]
fn test_cascade_delete_sync_attached_child_mapping_recovers_after_metadata_commit_failure() {
    let sync = sync_db();
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("ATTACH ':memory:' AS aipp_delete_assistant;
        CREATE TABLE mcp_server_tool(id INTEGER PRIMARY KEY, server_id INTEGER);
        CREATE TABLE aipp_delete_assistant.assistant_mcp_tool_config(id INTEGER PRIMARY KEY, mcp_tool_id INTEGER);
        INSERT INTO mcp_server_tool VALUES(5,1);
        INSERT INTO aipp_delete_assistant.assistant_mcp_tool_config VALUES(9,5);").unwrap();
    save_object_map(&sync,"mcp.server_tool","mcp_server_tool",5,"tool").unwrap();
    save_object_map(&sync,"assistant.mcp_tool_config","assistant_mcp_tool_config",9,"config").unwrap();
    sync.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE required(id INTEGER PRIMARY KEY);
        CREATE TABLE deferred_failure(id INTEGER REFERENCES required(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER fail_commit AFTER INSERT ON sync_apply_receipt BEGIN INSERT INTO deferred_failure VALUES(1); END;").unwrap();
    let mut deletion = change("mcp.server_tool","tool",2,JsonValue::Null);
    deletion.operation = "delete".into();
    assert!(apply_change_to_db(&sync,&data,&spec("mcp.server_tool"),&deletion,"local", |_|Ok(())).is_err());
    assert_eq!(data.query_row("SELECT COUNT(*) FROM aipp_delete_assistant.assistant_mcp_tool_config",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(find_local_id_by_object_id(&sync,"assistant.mcp_tool_config","config").unwrap(),Some(9));
    sync.execute_batch("DROP TRIGGER fail_commit").unwrap();
    apply_change_to_db(&sync,&data,&spec("mcp.server_tool"),&deletion,"local", |_|Ok(())).unwrap();
    assert_eq!(find_local_id_by_object_id(&sync,"assistant.mcp_tool_config","config").unwrap(),None);
    assert_eq!(find_local_id_by_object_id(&sync,"mcp.server_tool","tool").unwrap(),None);
    assert!(remote_change_already_applied(&sync,&deletion).unwrap());
}

#[test]
fn test_sync_update_missing_row_returns_error() {
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE assistant(id INTEGER PRIMARY KEY, name TEXT)").unwrap();
    let fields = Map::from_iter([("name".into(),json!("missing"))]);
    assert!(update_row(&data,&spec("assistant"),7,&fields).unwrap_err().contains("local_id=7"));
}

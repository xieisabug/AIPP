use super::*;

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
    apply_ack_to_shadow(&sync, "conversation", "c", 3, "delete", "old").unwrap();
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
    apply_ack_to_shadow(&sync, &original.object_type, &original.object_id, 1, "upsert", &original.payload_hash).unwrap();
    enqueue_snapshot_if_changed(&sync, &snapshot(&sync, "temporary"), "device").unwrap();
    assert!(!enqueue_snapshot_if_changed(&sync, &original, "device").unwrap());
    assert_eq!(sync.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_dead_letter_replay_resolves_reversed_dependencies_in_one_request() {
    let sync = sync_db();
    for id in ["child", "parent", "orphan"] {
        record_dead_letter(&sync, &change("conversation", id, 1, json!({})), "initial");
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

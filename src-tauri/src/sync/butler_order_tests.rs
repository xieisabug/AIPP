use super::*;

fn device() -> (Connection, Connection) {
    let data = Connection::open_in_memory().unwrap();
    data.execute_batch("CREATE TABLE assistant(id INTEGER PRIMARY KEY);
        CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT);
        CREATE TABLE feature_config(id INTEGER PRIMARY KEY,feature_code TEXT,key TEXT,value TEXT,data_type TEXT,description TEXT,
            UNIQUE(feature_code,key));").unwrap();
    let sync = Connection::open_in_memory().unwrap();
    sync.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    (data, sync)
}

#[test]
fn test_butler_contact_order_migrates_and_syncs_the_mixed_list_across_local_ids() {
    let (mut a, sync_a) = device();
    a.execute_batch(
        "INSERT INTO assistant VALUES(17),(1);
        INSERT INTO conversation VALUES(17,'butler_group'),(18,'normal');",
    )
    .unwrap();
    let legacy =
        ["group:17", "assistant:17", "assistant:1", "group:18", "assistant:99", "group:17"]
            .map(String::from);
    let portable = portable_order(&sync_a, &a, &a, &legacy).unwrap();
    assert_eq!(portable.len(), 3);
    assert!(store(&mut a, &sync_a, &portable, true).unwrap());
    let spec = specs().into_iter().find(|s| s.object_type == "system.feature_config").unwrap();
    let (rows, _) = read_local_snapshots_from_db(
        &a,
        &spec,
        &sync_a,
        &mut DanglingFkScanReport::default(),
        |_, _| Ok(true),
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    let change = PullChange {
        seq: 1,
        event_id: "order".into(),
        device_id: "A".into(),
        object_type: row.object_type.clone(),
        object_id: row.object_id.clone(),
        operation: "upsert".into(),
        version: 1,
        payload: Some(serde_json::from_str(&row.payload_json).unwrap()),
        deleted_at: None,
        created_at: now_string(),
    };
    let (mut b, sync_b) = device();
    apply_change_to_db(&sync_b, &b, &spec, &change, "B", |_| Ok(())).unwrap();
    let merged = setting(&b).unwrap().unwrap();
    assert!(local_order(&sync_b, &merged).unwrap().is_empty());
    for (key, id) in portable.iter().zip([24, 56, 1]) {
        let (kind, uid) = split_entry(key).unwrap();
        save_object_map(&sync_b, kind, kind, id, uid).unwrap();
    }
    assert_eq!(local_order(&sync_b, &merged).unwrap(), ["group:24", "assistant:56", "assistant:1"]);
    assert!(!store(&mut b, &sync_b, &[portable[2].clone()], true).unwrap());
    // Reordering two visible rows keeps the absent assistant in its old slot.
    let reordered = [portable[2].clone(), portable[0].clone()];
    assert!(store(&mut b, &sync_b, &reordered, false).unwrap());
    assert_eq!(
        local_order(&sync_b, &setting(&b).unwrap().unwrap()).unwrap(),
        ["assistant:1", "assistant:56", "group:24"]
    );
    assert!(!store(&mut b, &sync_b, &reordered, false).unwrap());

    // A previously downloaded order may still name an identity that has merged.
    remap_sync_object_map(&sync_b, "assistant", "assistant", 56, "replacement-id").unwrap();
    let renamed =
        ["assistant:replacement-id".to_string(), portable[2].clone(), portable[0].clone()];
    assert!(store(&mut b, &sync_b, &renamed, false).unwrap());
    assert_eq!(setting(&b).unwrap().unwrap(), renamed);
}

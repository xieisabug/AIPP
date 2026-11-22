use super::*;

#[test]
fn test_decision_catalog_sync_preserves_custom_models_and_matches_preview() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let provider = db.add_llm_provider("TypeSafe", "typesafe_jev", "", false, true).unwrap();
    db.add_llm_model("My Jev", provider, "my-jev", "custom version", true, false, false).unwrap();
    let custom_id = db.conn.last_insert_rowid();

    let preview = typesafe_catalog(&db, provider).unwrap();
    let codes = |models: &[LlmModel]| models.iter().map(|m| m.code.clone()).collect::<HashSet<_>>();
    let expected_count = crate::decision::TYPESAFE_JEV_PRESET_MODELS.len() + 1;
    assert_eq!(preview.len(), expected_count);
    assert_eq!(preview.iter().filter(|m| m.id != 0).count(), 1);
    let synced = sync_typesafe_models(&db, provider).unwrap();
    assert_eq!(codes(&synced), codes(&preview));
    assert!(synced.iter().all(|m| m.id > 0));
    let custom = synced.iter().find(|m| m.code == "my-jev").unwrap();
    assert_eq!((custom.id, custom.name.as_str(), custom.description.as_str()),
        (custom_id, "My Jev", "custom version"));
    assert!(custom.vision_support);

    let retried = sync_typesafe_models(&db, provider).unwrap();
    assert_eq!(retried.len(), expected_count);
    for model in &synced {
        assert!(retried.iter().any(|m| m.code == model.code && m.id == model.id));
    }
}

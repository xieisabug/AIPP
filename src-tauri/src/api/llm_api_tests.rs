use super::*;

#[test]
fn test_decision_modes_match_provider_in_menu_save_and_legacy_lookup() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    for (api_type, expected) in [
        ("openai_api", vec!["chat_completions", "responses", "openai_decisions"]),
        ("openai", vec!["chat_completions", "responses", "openai_decisions"]),
        ("gemini", vec!["google_gemini", "gemini_decisions"]),
        ("typesafe_jev", vec!["jev_systemone"]),
    ] {
        let provider_id = db.add_llm_provider(api_type, api_type, "", false, true).unwrap();
        let provider = db.get_llm_provider(provider_id).unwrap();
        let (options, default_mode) = protocol_choice(&db, api_type, provider_id, "jev-latest", None).unwrap();
        assert_eq!(options.unwrap(), expected);
        assert_eq!(default_mode, expected[0]);
        for mode in ["chat_completions", "responses", "google_gemini", "openai_decisions", "gemini_decisions", "jev_systemone"] {
            assert_eq!(validate_request_mode(&db, &provider, "model", mode).is_ok(), expected.contains(&mode));
        }
        if api_type == "openai_api" {
            let code = "gemini-decision-1";
            let (_, inferred_mode) = protocol_choice(&db, api_type, provider_id, code, None).unwrap();
            assert_eq!(inferred_mode, "chat_completions");
            db.add_llm_model(code, provider_id, code, "", false, false, false).unwrap();
            db.upsert_model_request_mode(provider_id, code, "gemini_decisions").unwrap();
            let saved = db.get_model_request_mode(provider_id, code).unwrap();
            let (_, mode) = protocol_choice(&db, api_type, provider_id, code, saved.as_ref()).unwrap();
            assert_eq!(mode, crate::db::llm_db::PROVIDER_MODE_MISMATCH);
            let stored_code = code.to_owned();
            assert!(db.get_llm_model_detail(&provider_id, &stored_code).unwrap_err().to_string().contains("gemini_decisions"));
            assert!(!db.get_models_for_select_with_mode(true).unwrap().iter().any(|row| row.1 == code));
            db.upsert_model_request_mode(provider_id, code, "openai_decisions").unwrap();
            assert_eq!(db.get_llm_model_detail(&provider_id, &stored_code).unwrap().model.request_mode, "openai_decisions");
        }
    }
}

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

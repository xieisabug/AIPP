use super::*;

#[test]
fn test_onboarding_probe_reuses_hidden_conversation_identity() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::conversation_db::ensure_conversation_table(&conn).unwrap();
    crate::db::conversation_identity::initialize(&conn, None).unwrap();
    let id = onboarding_probe_conversation(&conn).unwrap();
    assert_eq!(id, onboarding_probe_conversation(&conn).unwrap());
    assert!(uuid::Uuid::parse_str(&crate::db::conversation_identity::get(&conn, id).unwrap()).is_ok());
    let visible: i64 = conn.query_row("SELECT count(*) FROM conversation WHERE is_hidden_from_normal_chat_list=0", [], |r| r.get(0)).unwrap();
    assert_eq!(visible, 0);
}

#[tokio::test]
async fn test_onboarding_probe_over_http_classifies_rejections_and_accepts_empty_content() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    for (status, kind) in [(200, "ok"), (401, "auth"), (402, "insufficient_balance"), (404, "model_not_found"), (429, "rate_limited")] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(socket);
            let mut header = String::new();
            reader.read_line(&mut header).await.unwrap();
            assert!(header.starts_with("POST /v1/chat/completions "));
            let mut length = 0;
            loop {
                header.clear();
                assert!(reader.read_line(&mut header).await.unwrap() > 0);
                if header == "\r\n" { break; }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") { length = value.trim().parse().unwrap(); }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).await.unwrap();
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["messages"][0]["content"], "hi");
            assert!(request["max_tokens"] == 32 || request["max_completion_tokens"] == 32);
            let response = if status == 200 {
                r#"{"id":"test","model":"test","choices":[{"index":0,"message":{"role":"assistant","content":""},"finish_reason":"length"}]}"#
            } else { r#"{"error":{"message":"synthetic rejection"}}"# };
            reader.get_mut().write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        });
        let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
        db.create_tables().unwrap();
        let id = db.add_llm_provider("Fixture", "openai_api", "", false, false).unwrap();
        db.add_llm_provider_config(id, "endpoint", &endpoint, "header", false).unwrap();
        db.add_llm_provider_config(id, "api_key", "synthetic-fixture-key", "header", false).unwrap();
        let configs = db.get_llm_provider_config(id).unwrap();
        let client = genai_client::create_client_with_config(None, &configs, "test", "openai_api",
            Some("chat_completions"), None, false, Some(3), false, &HashMap::new()).unwrap();
        let result = probe_request(&client, "test").await;
        server.await.unwrap();
        assert_eq!(result.err().map(|e| e.kind).unwrap_or("ok"), kind);
    }
}


#[test]
fn test_onboarding_import_reuses_only_matching_credentials_and_owns_only_new_drafts() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let data = |key: &str| ProviderShareData {
        name: "同名".into(), api_type: "openai_api".into(), endpoint: Some("https://test.invalid/v1".into()), api_key: key.into(),
    };
    let first = import_provider_record(&db, data("one"), None, false).unwrap();
    let second = import_provider_record(&db, data("two"), None, true).unwrap();
    let again = import_provider_record(&db, data("one"), None, true).unwrap();
    assert_eq!(again.provider.id, first.provider.id);
    assert!(again.already_exists && again.provider.is_enabled);
    assert_ne!(first.provider.id, second.provider.id);
    assert!(!second.provider.is_enabled && !second.already_exists);
    let owned = |id| db.conn.query_row("SELECT EXISTS(SELECT 1 FROM onboarding_draft_provider WHERE provider_id=?1)", [id], |r| r.get::<_, bool>(0)).unwrap();
    assert!(owned(second.provider.id));
    assert!(!owned(first.provider.id));
}

#[test]
fn test_onboarding_provider_errors_preserve_status_and_detail() {
    for (status, expected) in [(401, "auth"), (403, "auth"), (402, "insufficient_balance"), (404, "model_not_found"), (429,"rate_limited"), (504, "timeout")] {
        let error = classify_provider_error(Some(status), "原始错误".into());
        assert_eq!((error.kind, error.message.as_str()), (expected, "原始错误"));
    }
    assert_eq!(classify_provider_error(None, "connection refused".into()).kind, "network");
    assert_eq!(classify_provider_error(None, "request timed out".into()).kind, "timeout");
    assert_eq!(classify_provider_error(Some(400), "Your credit balance is too low".into()).kind, "insufficient_balance");
    assert_eq!(classify_provider_error(Some(429), "You exceeded your current quota".into()).kind, "insufficient_balance");
}


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

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
    for (status, kind) in [(200, "ok"), (401, "auth"), (402, "insufficient_balance"), (404, "ambiguous_not_found"), (429, "rate_limited")] {
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
    for (status, expected) in [(401, "auth"), (403, "auth"), (402, "insufficient_balance"), (404, "ambiguous_not_found"), (429,"rate_limited"), (504, "timeout")] {
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
        ("openai_api", vec!["chat_completions", "responses", "anthropic_messages", "openai_decisions"]),
        ("openai", vec!["chat_completions", "responses", "anthropic_messages", "openai_decisions"]),
        ("gemini", vec!["google_gemini", "gemini_decisions"]),
        ("typesafe_jev", vec!["jev_systemone"]),
    ] {
        let provider_id = db.add_llm_provider(api_type, api_type, "", false, true).unwrap();
        let provider = db.get_llm_provider(provider_id).unwrap();
        let (options, default_mode) = protocol_choice(&db, api_type, provider_id, "jev-latest", None).unwrap();
        assert_eq!(options.unwrap(), expected);
        assert_eq!(default_mode, expected[0]);
        for mode in ["chat_completions", "responses", "anthropic_messages", "google_gemini", "openai_decisions", "gemini_decisions", "jev_systemone"] {
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

const RESP: &str = r#"{"id":"resp_test","object":"response","status":"completed","model":"gpt-6.2-sol","output":[{"id":"msg_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]}]}"#;
const CHAT: &str = r#"{"id":"chat_test","model":"test","choices":[{"index":0,"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}]}"#;
const MSG: &str = r#"{"id":"msg_test","type":"message","role":"assistant","model":"claude-test","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;

#[tokio::test]
async fn test_onboarding_protocol_detection_and_persistence() {
    use tokio::io::AsyncWriteExt;
    let response_path = "/v1/responses";
    let chat_path = "/v1/chat/completions";
    let scenarios = [
        ("responses", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 200, RESP)], true),
        ("gateway rejects responses", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 400, r#"{"error":{"code":"unsupported_endpoint","message":"Unsupported endpoint"}}"#),(chat_path, 200, CHAT)], true),
        ("route absent", "openai_api", "opaque-id", None, vec![(response_path, 404, r#"{"error":{"message":"Cannot POST /v1/responses"}}"#),(chat_path, 200, CHAT)], true),
        ("wrong model", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 404, r#"{"error":{"code":"model_not_found","message":"Unknown model"}}"#)], false),
        ("ambiguous 404", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 404, r#"{"error":{"message":"Not found"}}"#)], false),
        ("invalid key", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 401, r#"{"error":{"message":"invalid key"}}"#)], false),
        ("no balance", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 402, r#"{"error":{"message":"insufficient balance"}}"#)], false),
        ("rate limit", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 429, r#"{"error":{"message":"rate limit"}}"#)], false),
        ("backend down", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 503, r#"{"error":{"message":"service unavailable"}}"#)], false),
        ("two failures", "openai_api", "gpt-6.2-sol", None, vec![(response_path, 405, r#"{"error":{"message":"Method not allowed"}}"#),(chat_path, 403, r#"{"error":{"message":"access denied"}}"#)], false),
        ("bad parameter", "openai_api", "opaque-id", None, vec![(response_path, 400, r#"{"error":{"message":"Unsupported parameter: max_output_tokens"}}"#)], false),
        ("timeout", "openai_api", "opaque-id", None, vec![(response_path, 504, r#"{"error":{"message":"gateway timeout"}}"#)], false),
        ("explicit Messages", "openai_api", "opaque-id", Some("anthropic_messages"), vec![("/v1/messages", 200, MSG)], true),
        ("explicit chat", "openai_api", "gpt-6.2-sol", Some("chat_completions"), vec![(chat_path, 200, CHAT)], true),
        ("native Anthropic", "anthropic", "claude-test", None, vec![("/v1/messages", 200, MSG)], true),
        ("native DeepSeek", "deepseek", "deepseek-chat", None, vec![(chat_path, 200, CHAT)], true),
    ];
    for (name, provider, code, saved, replies, ok) in scenarios {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            for (path, status, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (head, bytes) = crate::utils::test_http::read_request(&mut socket).await;
                let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(json["model"], code, "{name}");
                assert!(head.starts_with(&format!("POST {path} ")), "{name}: {head}");
                assert!(json.get(if path == "/v1/responses" { "input" } else { "messages" }).is_some());
                paths.push(path);
                socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            paths
        });
        let cfg = |name: &str, value: &str| crate::db::llm_db::LLMProviderConfig {
            id: 1, llm_provider_id: 1, name: name.into(), value: value.into(), append_location: "header".into(), is_addition: false,
        };
        let configs = [cfg("endpoint", &endpoint), cfg("api_key", "fixture-protocol-test-key")];
        let mut db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
        db.create_tables().unwrap();
        let id = db.add_llm_provider("Fixture", provider, "", false, true).unwrap();
        if let Some(mode) = saved { db.upsert_model_request_mode(id, code, mode).unwrap(); }
        let mut attempted = Vec::new();
        let result = probe_model_protocol(&mut db, id, provider, code, |mode| {
            attempted.push(mode.to_owned());
            genai_client::create_client_with_config(None, &configs, code, provider, Some(mode),
                None, false, Some(2), false, &HashMap::new()).map_err(|e| e.to_string())
        }).await;
        let paths = tokio::time::timeout(std::time::Duration::from_secs(3), server).await
            .unwrap_or_else(|_| panic!("{name}: missing expected request")).unwrap();
        assert_eq!(attempted.len(), paths.len(), "{name}");
        assert_eq!(result.is_ok(), ok, "{name}: {result:?}");
        if name == "ambiguous 404" { assert_eq!(result.as_ref().unwrap_err().kind, "choose_protocol"); }
        if name == "two failures" {
            let error = result.as_ref().unwrap_err();
            assert!(error.message.contains("responses") && error.message.contains("chat_completions"));
        }
        db.add_llm_model(code, id, code, "", false, false, false).unwrap();
        if ok {
            let detail = db.get_llm_model_detail(&id, &code.to_owned()).unwrap();
            assert_eq!(Some(&detail.model.request_mode), attempted.last(), "{name}");
        } else {
            assert_eq!(db.get_model_request_mode(id, code).unwrap().as_deref(), saved);
        }
    }
}

use super::*;

#[test]
fn test_opencode_go_raw_billing_preserves_zero_across_response_formats() {
    let usage = serde_json::json!({"cost":0,"opencode":{"billing_source":"go","allowance":0,"tokens":{"input":2,"output":3}}});
    for body in [
        serde_json::json!({"usage":usage}),
        serde_json::json!({"usageMetadata":usage}),
        serde_json::json!({"response":{"usage":usage}}),
        serde_json::json!({"message":{"usage":usage}}),
    ] {
        let raw = raw_billing(&body);
        let metadata = billing_metadata(&raw).unwrap();
        assert_eq!(metadata["cost"], 0);
        assert_eq!(metadata["allowance"], 0);
        assert_eq!(normalize_usage(Default::default(), &raw).total_tokens, Some(5));
        assert!(raw.get("response").is_none());
    }
    assert!(raw_billing(&serde_json::json!({"usage":{"total_tokens":5}})).is_null());
}
use rusqlite::Connection;

#[test]
fn test_catalog_protocols_and_media() {
    let model = |api| serde_json::from_value::<CatalogModel>(serde_json::json!({
        "id":"model", "api":api, "modalities":{"input":["text","image/png"]}
    })).unwrap().into_model(1, Some(&"responses".into()));
    let multiple = model(serde_json::json!({"messages": {}, "responses": {}, "unknown": {}}));
    assert_eq!(multiple.available_modes.unwrap(), ["anthropic_messages", "responses"]);
    assert_eq!(multiple.request_mode, "responses");
    assert!(multiple.vision_support);
    assert_eq!(model(serde_json::json!({"generate-content":{}})).request_mode,"google_gemini");
    assert_eq!(model(serde_json::json!({"future-api":{}})).request_mode,"unsupported");
    let text: CatalogModel = serde_json::from_value(serde_json::json!({
        "id":"glm", "type":"chat", "api":{"messages":{}}, "modalities":{"output":["text"]}
    })).unwrap();
    let text = text.into_model(1,None);
    assert_eq!(text.request_mode,"anthropic_messages");
    assert_eq!(text.available_modes.unwrap(), ["anthropic_messages","chat_completions","responses","google_gemini"]);
}

#[test]
fn test_seed_collision_upgrade_and_permanent_deletion() {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let initial_id = db.conn.query_row("SELECT id FROM llm_provider WHERE api_type=?1", [PROVIDER], |r| r.get(0)).unwrap();
    let initial = db.get_llm_provider(initial_id).unwrap();
    assert_eq!(initial.api_type, PROVIDER);
    assert!(!initial.is_enabled);
    db.conn.execute_batch("DELETE FROM llm_provider WHERE api_type='opencode_go';
        DELETE FROM builtin_provider_migrations;
        INSERT INTO llm_provider (id,name,api_type,description) VALUES (31,'mine','openai_api','');").unwrap();
    db.create_tables().unwrap();
    assert_eq!(db.get_llm_provider(31).unwrap().name, "mine");
    db.conn.execute("DELETE FROM llm_provider WHERE api_type=?1",[PROVIDER]).unwrap();
    db.create_tables().unwrap();
    assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM llm_provider WHERE api_type=?1",
        [PROVIDER], |r| r.get::<_,i64>(0)).unwrap(), 0);
}

#[test]
fn test_catalog_supersedes_stale_preferences() {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let id = db.conn.query_row("SELECT id FROM llm_provider WHERE api_type=?1", [PROVIDER], |r| r.get(0)).unwrap();
    db.upsert_model_request_mode(id, "example", "responses").unwrap();
    assert_eq!(db.effective_model_mode(PROVIDER, id, "example").unwrap(), "responses");
    db.conn.execute("INSERT INTO opencode_go_protocols(model,mode) VALUES ('example','anthropic_messages')", []).unwrap();
    assert_eq!(db.effective_model_mode(PROVIDER, id, "example").unwrap(), "anthropic_messages");
    assert_eq!(db.effective_model_mode(PROVIDER, id, "new").unwrap(), "unsupported");
}

#[test]
fn test_opencode_go_normalizes_billed_tokens_without_double_counting_reasoning() {
    let billing = serde_json::json!({
        "cost":0, "opencode":{"allowance":0,"tokens":{"input":11,"cache_read":28,"output":16,"reasoning":13}},
    });
    let usage = normalize_usage(Default::default(), &billing);
    assert_eq!((usage.prompt_tokens,usage.completion_tokens,usage.total_tokens), (Some(39),Some(16),Some(55)));
    let billing = billing_metadata(&billing).unwrap();
    assert_eq!(billing["allowance"],0);
    assert_eq!(billing["cost"],0);
    assert_eq!(usage.completion_tokens_details.unwrap().reasoning_tokens,Some(13));
}

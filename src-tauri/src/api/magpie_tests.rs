use super::*;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::Path;

#[test]
fn test_hello_requires_magpie_name() {
    assert_eq!(hello_name(r#"{"name":"magpie","version":"1"}"#).unwrap(), "magpie");
    assert!(hello_name(r#"{"name":"other"}"#).unwrap() != "magpie");
    assert!(hello_name("not-json").is_err());
}

#[test]
fn test_settings_path_prefers_xdg_then_home() {
    assert_eq!(
        settings_path(Some(Path::new("/tmp/xdg")), Some(Path::new("/home/me"))).unwrap(),
        Path::new("/tmp/xdg/magpie/settings.json")
    );
    assert_eq!(
        settings_path(Some(Path::new("")), Some(Path::new("/home/me"))).unwrap(),
        Path::new("/home/me/.config/magpie/settings.json")
    );
    assert!(settings_path(None, None).is_none());
}

#[test]
fn test_port_from_settings_reads_positive_port_only() {
    assert_eq!(port_from_settings(r#"{"port":3426,"theme":"dark"}"#), Some(3426));
    assert_eq!(port_from_settings("{}"), None);
    assert_eq!(port_from_settings(r#"{"port":0}"#), None);
    assert_eq!(port_from_settings("not json"), None);
}

#[test]
fn test_origin_strips_client_suffix_and_keeps_proxy_prefix() {
    assert_eq!(origin_from_endpoint("http://127.0.0.1:3425/v1/").unwrap(), "http://127.0.0.1:3425");
    assert_eq!(origin_from_endpoint("http://127.0.0.1:3425/v1beta/").unwrap(), "http://127.0.0.1:3425");
    assert_eq!(origin_from_endpoint("http://127.0.0.1:3425/").unwrap(), "http://127.0.0.1:3425");
    assert_eq!(origin_from_endpoint("https://gw.example.com/magpie/v1/").unwrap(), "https://gw.example.com/magpie");
    assert!(origin_is_loopback("http://127.0.0.1:3425"));
    assert!(origin_is_loopback("http://localhost:3425"));
    assert!(!origin_is_loopback("https://gw.example.com"));
}

#[test]
fn test_routes_hit_magpie_paths() {
    let origin = "http://127.0.0.1:3425";
    let (_, chat) = route(Some("chat_completions"), origin).unwrap();
    assert_eq!(reqwest::Url::parse(&chat).unwrap().join("chat/completions").unwrap().as_str(),
        "http://127.0.0.1:3425/v1/chat/completions");
    let (_, responses) = route(Some("responses"), origin).unwrap();
    assert_eq!(reqwest::Url::parse(&responses).unwrap().join("responses").unwrap().as_str(),
        "http://127.0.0.1:3425/v1/responses");
    let (anthropic, base) = route(Some("anthropic_messages"), origin).unwrap();
    assert_eq!(anthropic, AdapterKind::Anthropic);
    assert_eq!(format!("{base}messages"), "http://127.0.0.1:3425/v1/messages");
    let (gemini, base) = route(Some("google_gemini"), origin).unwrap();
    assert_eq!(gemini, AdapterKind::Gemini);
    assert_eq!(format!("{base}models/openai/gpt-5:generateContent"),
        "http://127.0.0.1:3425/v1beta/models/openai/gpt-5:generateContent");
    assert!(route(Some("future"), origin).is_err());
    assert_eq!(api_key_or_default(Some("  ")), APP_KEY);
    assert_eq!(api_key_or_default(Some("gateway-key")), "gateway-key");
}

#[test]
fn test_catalog_keeps_chat_models_and_native_protocol_first() {
    let body = r#"{"object":"list","data":[
        {"id":"anthropic/claude","display_name":"Claude","owned_by":"anthropic",
         "native_endpoints":["/v1/messages"],"modalities":{"input":["text","image"]}},
        {"id":"openai/gpt","display_name":"","owned_by":"openai","kind":"chat"},
        {"id":"draw","kind":"image","display_name":"Draw"},
        {"id":"","display_name":"blank"}
    ]}"#;
    let saved = HashMap::from([("openai/gpt".into(), "responses".into())]);
    let models = parse_catalog(body, 7, &saved).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].name, "Claude");
    assert_eq!(models[0].code, "anthropic/claude");
    assert!(models[0].vision_support);
    assert_eq!(models[0].request_mode, "anthropic_messages");
    assert_eq!(models[0].available_modes.as_ref().unwrap()[0], "anthropic_messages");
    assert_eq!(models[0].available_modes.as_ref().unwrap().len(), 4);
    assert_eq!(models[1].name, "openai/gpt");
    assert_eq!(models[1].request_mode, "responses");
    assert_eq!(models[1].description, "openai");
}

#[test]
fn test_seed_is_official_disabled_and_deletion_stays() {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let id: i64 = db.conn.query_row("SELECT id FROM llm_provider WHERE api_type=?1", [PROVIDER], |row| row.get(0)).unwrap();
    let provider = db.get_llm_provider(id).unwrap();
    assert_eq!(provider.name, "Magpie");
    assert!(!provider.is_enabled);
    assert!(provider.is_official);
    db.conn.execute("DELETE FROM llm_provider WHERE api_type=?1", [PROVIDER]).unwrap();
    db.create_tables().unwrap();
    assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM llm_provider WHERE api_type=?1",
        [PROVIDER], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_protocol_order_is_per_provider_and_defaults_before_refresh() {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let id: i64 = db.conn.query_row("SELECT id FROM llm_provider WHERE api_type=?1", [PROVIDER], |row| row.get(0)).unwrap();
    assert_eq!(db.effective_model_mode(PROVIDER, id, "anthropic/claude").unwrap(), "chat_completions");
    for (position, mode) in ["anthropic_messages", "chat_completions", "responses", "google_gemini"].iter().enumerate() {
        db.conn.execute("INSERT INTO magpie_protocols(provider_id,model,mode,position) VALUES (?1,'anthropic/claude',?2,?3)",
            params![id, mode, position as i64]).unwrap();
    }
    assert_eq!(db.effective_model_mode(PROVIDER, id, "anthropic/claude").unwrap(), "anthropic_messages");
    db.upsert_model_request_mode(id, "anthropic/claude", "responses").unwrap();
    assert_eq!(db.effective_model_mode(PROVIDER, id, "anthropic/claude").unwrap(), "responses");
    assert_eq!(available_modes(&db, id + 1, "anthropic/claude", None).unwrap()[0], "chat_completions");
}

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
    assert_eq!(models[0].name, "Claude · anthropic");
    assert_eq!(models[0].code, "anthropic/claude");
    assert!(models[0].vision_support);
    assert_eq!(models[0].request_mode, "anthropic_messages");
    assert_eq!(models[0].available_modes.as_ref().unwrap()[0], "anthropic_messages");
    assert_eq!(models[0].available_modes.as_ref().unwrap().len(), 4);
    assert_eq!(models[1].name, "openai/gpt");
    assert_eq!(models[1].request_mode, "responses");
    assert_eq!(models[1].description, "openai");
}

/// 同名模型优先使用网关提供的来源标签，路由组与直连提供商保持独立。
#[test]
fn test_catalog_distinguishes_providers_and_routing_groups_with_labels() {
    let body = r#"{"data":[
        {"id":"auto/claude","display_name":"Claude","magpie_label":"Claude · routing group","owned_by":"claude-mine"},
        {"id":"claude-mine/claude","display_name":"Claude","magpie_label":"Claude · 我的订阅","owned_by":"claude-mine"},
        {"id":"other/claude","display_name":"Claude","magpie_label":"Claude · 另一个渠道","owned_by":"other"}
    ]}"#;
    let saved = HashMap::from([("other/claude".into(), "responses".into())]);
    let models = parse_catalog(body, 7, &saved).unwrap();
    assert_eq!(models.iter().map(|model| model.name.as_str()).collect::<Vec<_>>(),
        ["Claude · routing group", "Claude · 我的订阅", "Claude · 另一个渠道"]);
    assert_eq!(models.iter().map(|model| model.code.as_str()).collect::<Vec<_>>(),
        ["auto/claude", "claude-mine/claude", "other/claude"]);
    assert_eq!(models[0].request_mode, "chat_completions");
    assert_eq!(models[1].request_mode, "chat_completions");
    assert_eq!(models[2].request_mode, "responses");
}

/// 旧版目录缺少来源标签时使用路由前缀；名称缺失时保留完整 ID。
#[test]
fn test_catalog_names_without_labels_keep_route_identity() {
    let body = r#"{"data":[
        {"id":"auto/claude","display_name":"Claude","owned_by":"mine"},
        {"id":"mine/claude","display_name":"Claude","magpie_label":"  ","owned_by":"mine"},
        {"id":"other/claude","display_name":"Claude"},
        {"id":"mine/gpt","display_name":"  ","owned_by":"mine"},
        {"id":"mine/plain","display_name":"mine/plain","owned_by":"mine"},
        {"id":"group","display_name":"My group"},
        {"id":"legacy","display_name":"Legacy","owned_by":"vendor"}
    ]}"#;
    let models = parse_catalog(body, 7, &HashMap::new()).unwrap();
    assert_eq!(models.iter().map(|model| model.name.as_str()).collect::<Vec<_>>(),
        ["Claude · auto", "Claude · mine", "Claude · other", "mine/gpt", "mine/plain", "My group", "Legacy · vendor"]);
}

/// 提供商名称被隐藏或多个渠道使用相同标签时，仍可按完整模型 ID 区分。
#[test]
fn test_catalog_duplicate_labels_include_full_model_ids() {
    let body = r#"{"data":[
        {"id":"one/claude","display_name":"Claude","magpie_label":"Claude"},
        {"id":"two/claude","display_name":"Claude","magpie_label":"Claude"},
        {"id":"one/gpt","display_name":"GPT","magpie_label":"GPT · One"}
    ]}"#;
    let models = parse_catalog(body, 7, &HashMap::new()).unwrap();
    assert_eq!(models.iter().map(|model| model.name.as_str()).collect::<Vec<_>>(),
        ["Claude (one/claude)", "Claude (two/claude)", "GPT · One"]);
}

/// 重新获取模型列表后更新显示名，同时保留模型数据库 ID 和已选协议。
#[test]
fn test_catalog_refresh_preserves_model_identity_and_protocol() {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let provider_id: i64 = db.conn.query_row("SELECT id FROM llm_provider WHERE api_type=?1", [PROVIDER], |row| row.get(0)).unwrap();
    db.add_llm_model("Claude", provider_id, "mine/claude", "mine", false, false, false).unwrap();
    let model_id = db.conn.last_insert_rowid();
    db.upsert_model_request_mode(provider_id, "mine/claude", "responses").unwrap();
    let saved = db.list_model_request_modes(provider_id).unwrap().into_iter().collect();
    let models = parse_catalog(r#"{"data":[
        {"id":"mine/claude","display_name":"Claude","magpie_label":"Claude · Mine","owned_by":"mine"}
    ]}"#, provider_id, &saved).unwrap();
    let persisted = models.into_iter().map(|model| crate::db::llm_db::LLMModel {
        id: model.id, name: model.name, llm_provider_id: model.llm_provider_id,
        code: model.code, description: model.description,
        vision_support: model.vision_support, audio_support: model.audio_support,
        video_support: model.video_support, request_mode: model.request_mode,
    }).collect::<Vec<_>>();
    db.replace_llm_models(provider_id, &persisted, true).unwrap();
    let rows = db.get_llm_models(provider_id.to_string()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, model_id);
    assert_eq!(rows[0].1, "Claude · Mine");
    assert_eq!(rows[0].3, "mine/claude");
    assert_eq!(db.effective_model_mode(PROVIDER, provider_id, "mine/claude").unwrap(), "responses");
    db.conn.execute("UPDATE llm_provider SET is_enabled=1 WHERE id=?1", [provider_id]).unwrap();
    let choices = db.get_models_for_select().unwrap();
    assert!(choices.iter().any(|choice| choice.0 == "Magpie / Claude · Mine" && choice.2 == model_id));
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

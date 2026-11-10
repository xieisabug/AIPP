use super::*;
use rusqlite::{params, Connection};
use std::collections::HashMap;

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

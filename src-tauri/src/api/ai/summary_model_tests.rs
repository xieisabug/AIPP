use super::*;

#[test]
fn test_onboarding_aux_summary_skips_absent_native_and_disabled_models() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    assert!(summary_model(&db, &HashMap::new()).unwrap().is_none());
    for (kind, enabled, available) in [("claude_sdk", true, false), ("anthropic", false, false), ("anthropic", true, true)] {
        let id = db.add_llm_provider(kind, kind, "", false, enabled).unwrap();
        db.add_llm_model("Chat", id, "chat", "", false, false, false).unwrap();
        let key = "default_light_model";
        let group = "conversation_summary";
        let value = FeatureConfig { id: None, feature_code: group.into(), key: key.into(),
            value: format!("chat%%{id}"), data_type: "string".into(), description: None };
        let config = HashMap::from([(group.into(), HashMap::from([(key.into(), value)]))]);
        assert_eq!(summary_model(&db, &config).unwrap().is_some(), available);
    }
    let agent_id = db.add_llm_provider("Codex", "codex_app_server", "", false, true).unwrap();
    let api_id = db.add_llm_provider("DeepSeek", "deepseek", "", false, true).unwrap();
    db.add_llm_model("Flash", api_id, "deepseek-flash", "", false, false, false).unwrap();
    let group = "conversation_summary";
    let config = HashMap::from([(group.into(), HashMap::from([
        ("conversation_summary_model".into(), FeatureConfig { id: None, feature_code: group.into(), key: "conversation_summary_model".into(), value: format!("acp%%{agent_id}"), data_type: "string".into(), description: None }),
        ("default_light_model".into(), FeatureConfig { id: None, feature_code: group.into(), key: "default_light_model".into(), value: format!("deepseek-flash%%{api_id}"), data_type: "string".into(), description: None }),
    ]))]);
    let resolved = summary_model(&db, &config).unwrap().unwrap();
    assert_eq!((resolved.provider.id, resolved.model.code.as_str()), (api_id, "deepseek-flash"));
}

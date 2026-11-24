use super::*;

#[test]
fn test_conversation_summary_selection_survives_unrelated_settings_saves() {
    let mut db = SystemDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    for group in ["experimental", "conversation_summary"] {
        let selection = HashMap::from([
            ("conversation_summary_model".to_string(), "shared-model".to_string()),
            ("conversation_summary_provider_id".to_string(), "7".to_string()),
        ]);
        persist_feature_settings(&mut db, group, selection, false).unwrap();
        let provider_before = db.get_feature_config(group, "conversation_summary_provider_id").unwrap().unwrap();
        let changes = if group == "experimental" {
            HashMap::from([("default_home_window".into(), "ask".into())])
        } else {
            HashMap::from([("title_summary_enabled".into(), "false".into())])
        };
        persist_feature_settings(&mut db, group, changes, true).unwrap();
        let provider = db.get_feature_config(group, "conversation_summary_provider_id").unwrap().unwrap();
        assert_eq!(provider.id, provider_before.id);
        assert_eq!(provider.value, "7");
        assert_eq!(db.get_feature_config(group, "conversation_summary_model").unwrap().unwrap().value, "shared-model");
    }
}

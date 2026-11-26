use super::*;

#[test]
fn test_onboarding_preset_ignores_unknown_entries_and_rejects_new_protocol() {
    let mut fixture: Value = serde_json::from_str(PACKAGED).unwrap();
    fixture["model_ranking"]["main"].as_array_mut().unwrap().push(serde_json::json!({"future_kind":true}));
    fixture["recommended_provider"]["key_guide"].as_array_mut().unwrap().push(serde_json::json!({"title":"invalid", "url":"file:///invalid"}));
    assert_eq!(validate(&fixture.to_string()).unwrap(), validate(PACKAGED).unwrap());
    fixture["schema_version"] = 7.into();
    assert_eq!(validate(&fixture.to_string()).unwrap_err(), INCOMPATIBLE);
}

#[test]
fn test_onboarding_budget_uses_exact_catalog_limits_and_offline_flash_defaults() {
    let (db, _) = super::super::state_tests::databases();
    let flash = selected_budget(&db, "deepseek", "deepseek-v4-flash").unwrap();
    assert_eq!((flash.context,flash.output), (512000,32000));
    let entry = serde_json::json!({"preset":{"model_limits":{"openai/small":{"context":16384,"output":2048}}}});
    db.conn.execute("INSERT INTO system_config(key,value) VALUES (?1,?2)", rusqlite::params![CACHE_KEY,entry.to_string()]).unwrap();
    let small = selected_budget(&db, "openai_api", "small").unwrap();
    assert_eq!((small.context, small.output), (8192, 2048));
    let other = selected_budget(&db, "deepseek", "small").unwrap();
    assert_eq!((other.context, other.output), (128000, 32000));
}

#[test]
fn test_onboarding_old_helper_uses_current_packaged_policy() {
    let mut legacy: Value = serde_json::from_str(PACKAGED).unwrap();
    legacy["revision"] = "2026-11-25.1".into();
    legacy["recommended_provider"]["models"]["main"] = "deepseek-v4-pro".into();
    assert_eq!(official_preset(&legacy.to_string()).unwrap()["recommended_provider"]["models"]["main"], "deepseek-flash");
}

use super::*;

#[test]
fn test_butler_context_codex_splits_without_truncation_and_keeps_user_input_separate() {
    let role = format!("角色约定\n{}\n工具约定", "文".repeat(4001));
    let context = codex_application_context(&role);
    assert!(context.len() > 2);
    let params = codex_turn_start_params("thread", &Default::default(), "用户原话", &context);
    assert_eq!(params["input"][0]["text"], "用户原话");
    let mut restored = String::new();
    for entry in context.values() {
        assert_eq!(entry["kind"], "application");
        let value = entry["value"].as_str().unwrap();
        assert!(value.len() <= 4000);
        restored.push_str(value);
    }
    assert_eq!(restored, role);
    assert_eq!(params["additionalContext"], json!(context));
    let ordinary = codex_turn_start_params("thread", &Default::default(), "hello", &Default::default());
    assert!(ordinary.get("additionalContext").is_none());
}

use super::resolve_new_assistant_model;

#[test]
fn test_new_assistant_model_preserves_explicit_provider() {
    let models = vec![("one".into(), "same".into(), 1, 10), ("two".into(), "same".into(), 2, 20)];
    assert_eq!(resolve_new_assistant_model(models, "same%%20", 0).unwrap(), (20, "same".into()));
}

#[test]
fn test_new_assistant_model_rejects_missing_selection_without_fallback() {
    let models = vec![("one".into(), "model".into(), 1, 10)];
    assert!(resolve_new_assistant_model(models, "missing%%10", 0).unwrap_err().contains("所选模型不可用"));
}

#[test]
fn test_new_assistant_model_agent_provider_keeps_cli_default() {
    let models = vec![("native".into(), "acp".into(), 0, 20)];
    assert_eq!(resolve_new_assistant_model(models, "acp%%20", 4).unwrap(), (20, String::new()));
}

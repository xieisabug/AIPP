use super::*;

#[test]
fn test_codex_activity_order_does_not_change_with_progress_or_completion() {
    let mut orders = HashMap::new();
    let a = json!({"id":"a","type":"commandExecution"});
    let b = json!({"id":"b","type":"commandExecution"});
    assert_eq!(activity_order(&mut orders, &a), Some(0));
    assert_eq!(activity_order(&mut orders, &b), Some(1));
    for _ in 0..20 { assert_eq!(activity_order(&mut orders, &a), Some(0)); }
    assert_eq!(activity_order(&mut orders, &b), Some(1));
    assert_eq!(activity_order(&mut orders, &json!({"type":"commandExecution"})), None);
}

#[test]
fn test_codex_activity_reasoning_uses_only_its_dedicated_message() {
    assert!(has_dedicated_message("reasoning"));
    assert!(has_dedicated_message("reasoning/summaryPartAdded"));
    assert!(!has_dedicated_message("command"));
    assert!(!has_dedicated_message("patch"));
    assert!(!has_dedicated_message("tool"));
}

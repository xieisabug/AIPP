use super::*;

#[test]
fn test_agent_output_focus_codex_uses_the_bubble_receiving_content() {
    let params = json!({"delta":"content","itemId":"plan-1"});
    let target = |method, params: &Value| codex_delta_focus(method, params, 100, Some(101),
        |item| (item == "plan-1").then_some(102));
    for method in ["item/reasoning/summaryTextDelta", "item/reasoning/textDelta"] {
        assert_eq!(target(method, &params), Some(101));
    }
    assert_eq!(target("item/plan/delta", &params), Some(102));
    assert_eq!(target("item/agentMessage/delta", &params), Some(100));
    assert_eq!(target("turn/started", &params), None);
    assert_eq!(target("item/plan/delta", &json!({"delta":"", "itemId":"plan-1"})), None);
    assert_eq!(target("item/plan/delta", &json!({"delta":"text", "itemId":"other"})), None);
}

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

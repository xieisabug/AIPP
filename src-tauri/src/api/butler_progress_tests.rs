use super::*;

fn call(id: i64, message_id: i64, tool: &str, params: &str, status: &str) -> MCPToolCall {
    MCPToolCall {
        id, conversation_id: 1, message_id: Some(message_id), assistant_message_id: Some(message_id),
        subtask_id: None, server_id: 1, server_name: "s".into(), tool_name: tool.into(),
        parameters: params.into(), status: status.into(), result: None, error: None,
        created_time: "0".into(), started_time: None, finished_time: None, llm_call_id: None,
    }
}

#[test]
fn test_step_label_prefers_intent_parameter() {
    assert_eq!(step_label(&call(1, 5, "web_search", r#"{"limit":3,"query":"竞品 定价"}"#, "success")), "web_search 竞品 定价");
    assert_eq!(step_label(&call(1, 5, "list_tasks", "{}", "success")), "list_tasks");
    assert_eq!(step_label(&call(1, 5, "x", "not json", "success")), "x");
}

#[test]
fn test_recent_steps_keep_current_turn_in_order() {
    // Database returns newest first.
    let calls = vec![
        call(3, 12, "c", "{}", "executing"),
        call(2, 11, "b", "{}", "success"),
        call(1, 9, "old", "{}", "success"),
    ];
    let steps = recent_steps(&calls, 10);
    assert_eq!(steps.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    assert_eq!(steps[1].status, "executing");
}

#[test]
fn test_clean_summary_strips_wrapping_and_clips() {
    assert_eq!(clean_summary("\n「正在对比三家报价。」\n多余"), "正在对比三家报价");
    assert!(clean_summary(&"长".repeat(80)).ends_with('…'));
    assert_eq!(parse_model("gpt%%7"), Some(("gpt".into(), 7)));
    assert_eq!(parse_model(""), None);
}

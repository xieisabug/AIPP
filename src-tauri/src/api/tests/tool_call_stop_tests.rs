use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy};
use crate::mcp::execution_api::{
    collect_required_tool_call_ids_from_message_list,
    collect_tool_result_ids_from_message_list,
};
use genai::chat::ChatRole;

fn mcp_tool_call_comment(call_id: u64) -> String {
    format!(
        r#"<!-- MCP_TOOL_CALL: {{"server_name":"test","tool_name":"search","parameters":"{{}}","call_id":{},"llm_call_id":"call_{}"}} -->"#,
        call_id, call_id
    )
}

fn response_with_tool_call(call_id: u64, content: &str) -> String {
    format!("{}\n\n{}", content, mcp_tool_call_comment(call_id))
}

fn stopped_tool_result_content(call_id: &str) -> String {
    format!(
        "Tool execution completed:\n\nTool Call ID: {}\nTool: search\nServer: test\nParameters: {{}}\nResult:\nError: Stopped by user",
        call_id
    )
}

#[test]
fn given_response_with_tool_call_when_collect_required_ids_then_includes_llm_call_id() {
    let message_list = vec![(
        "response".to_string(),
        response_with_tool_call(1, "call tool"),
        vec![],
    )];

    let ids = collect_required_tool_call_ids_from_message_list(&message_list);

    assert!(ids.contains("call_1"));
}

#[test]
fn given_tool_result_when_collect_existing_ids_then_returns_call_id() {
    let message_list = vec![(
        "tool_result".to_string(),
        stopped_tool_result_content("call_1"),
        vec![],
    )];

    let ids = collect_tool_result_ids_from_message_list(&message_list);

    assert_eq!(ids.len(), 1);
    assert!(ids.contains("call_1"));
}

#[test]
fn given_missing_tool_result_ids_when_diff_then_detects_gap() {
    let message_list = vec![(
        "response".to_string(),
        response_with_tool_call(1, "call tool"),
        vec![],
    )];

    let required = collect_required_tool_call_ids_from_message_list(&message_list);
    let existing = collect_tool_result_ids_from_message_list(&message_list);
    let missing: Vec<_> = required.difference(&existing).cloned().collect();

    assert!(missing.contains(&"call_1".to_string()));
}

#[test]
fn given_stopped_tool_result_when_build_chat_request_with_pairing_then_includes_tool_response() {
    let init_message_list = vec![
        ("system".to_string(), "system".to_string(), vec![]),
        ("user".to_string(), "question".to_string(), vec![]),
        ("reasoning".to_string(), "先思考".to_string(), vec![]),
        (
            "response".to_string(),
            response_with_tool_call(1, "call tool"),
            vec![],
        ),
        (
            "tool_result".to_string(),
            stopped_tool_result_content("call_1"),
            vec![],
        ),
    ];

    let result = build_chat_request_from_messages(
        &init_message_list,
        ToolCallStrategy::NativeWithToolResponsePairing,
        None,
    );
    let messages = result.chat_request.messages;

    assert_eq!(messages.len(), 4);
    assert!(matches!(&messages[2].role, ChatRole::Assistant));
    assert_eq!(messages[2].content.tool_calls().len(), 1);
    assert_eq!(
        messages[2].content.joined_reasoning_content().as_deref(),
        Some("先思考")
    );
    assert!(matches!(&messages[3].role, ChatRole::Tool));
}
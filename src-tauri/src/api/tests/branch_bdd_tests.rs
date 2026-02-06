use crate::api::ai::conversation::{
    build_chat_request_from_messages, build_message_list_from_db, filter_messages_for_parent_group,
    BranchSelection, ToolCallStrategy,
};
use crate::api::ai::summary::get_latest_branch_messages;
use crate::db::conversation_db::{Message, MessageAttachment};
use chrono::{Duration, TimeZone, Utc};
use genai::chat::ChatRole;

fn make_message(
    id: i64,
    message_type: &str,
    created_time: chrono::DateTime<Utc>,
    generation_group_id: Option<&str>,
    parent_group_id: Option<&str>,
    content: &str,
) -> Message {
    Message {
        id,
        parent_id: None,
        conversation_id: 1,
        message_type: message_type.to_string(),
        content: content.to_string(),
        llm_model_id: Some(1),
        llm_model_name: Some("test-model".to_string()),
        created_time,
        start_time: None,
        finish_time: None,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: generation_group_id.map(str::to_string),
        parent_group_id: parent_group_id.map(str::to_string),
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    }
}

fn wrap(message: Message) -> (Message, Option<MessageAttachment>) {
    (message, None)
}

fn tool_result_content(call_id: &str, result: &str) -> String {
    format!("Tool execution completed:\n\nTool Call ID: {}\nResult:\n{}", call_id, result)
}

fn mcp_tool_call_comment(call_id: u64) -> String {
    format!(
        r#"<!-- MCP_TOOL_CALL: {{"server_name":"test","tool_name":"search","parameters":"{{}}","call_id":{},"llm_call_id":"call_{}"}} -->"#,
        call_id, call_id
    )
}

fn response_with_tool_call(call_id: u64, content: &str) -> String {
    format!("{}\n\n{}", content, mcp_tool_call_comment(call_id))
}

#[test]
fn given_parent_group_when_latest_branch_then_truncates_parent_group() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();
    let t5 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 4).unwrap();
    let t6 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 5).unwrap();

    let messages = vec![
        wrap(make_message(1, "system", t1, None, None, "system")),
        wrap(make_message(2, "user", t2, None, None, "q1")),
        wrap(make_message(3, "response", t3, Some("g1"), None, "r1")),
        wrap(make_message(4, "user", t4, None, None, "q2")),
        wrap(make_message(5, "response", t5, Some("g2"), None, "r2")),
        wrap(make_message(6, "response", t6, Some("g2b"), Some("g2"), "r2b")),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 6]);
}

#[test]
fn given_missing_parent_group_when_latest_branch_then_keeps_messages() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();

    let messages = vec![
        wrap(make_message(1, "system", t1, None, None, "system")),
        wrap(make_message(2, "user", t2, None, None, "q1")),
        wrap(make_message(3, "response", t3, Some("g1"), None, "r1")),
        wrap(make_message(4, "response", t4, Some("g2"), Some("missing"), "r2")),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4]);
}

#[test]
fn given_same_generation_group_when_latest_branch_then_keeps_latest_only() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();

    let messages = vec![
        wrap(make_message(1, "system", t1, None, None, "system")),
        wrap(make_message(2, "user", t2, None, None, "q1")),
        wrap(make_message(3, "response", t3, Some("g1"), None, "r1")),
        wrap(make_message(4, "response", t4, Some("g1"), None, "r1b")),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 4]);
}

#[test]
fn given_parent_group_filter_when_applied_then_drops_group_and_tool_results() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();

    let messages = vec![
        wrap(make_message(1, "response", t1, Some("g1"), None, &response_with_tool_call(1, "r1"))),
        wrap(make_message(2, "tool_result", t2, None, None, &tool_result_content("1", "ok"))),
        wrap(make_message(3, "response", t3, Some("g2"), Some("g1"), "r2")),
    ];

    let filtered = filter_messages_for_parent_group(messages, Some("g1"));
    assert!(filtered.iter().all(|(msg, _)| msg.generation_group_id.as_deref() != Some("g1")));
    assert!(filtered.iter().all(|(msg, _)| msg.message_type != "tool_result"));
}

#[test]
fn given_latest_branch_with_tool_calls_when_build_message_list_then_unrelated_tool_results_dropped()
{
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();

    let messages = vec![
        wrap(make_message(1, "response", t1, Some("g1"), None, &response_with_tool_call(1, "r1"))),
        wrap(make_message(2, "tool_result", t2, None, None, &tool_result_content("1", "ok"))),
        wrap(make_message(3, "response", t3, Some("g2"), Some("g1"), "r2")),
    ];

    let list = build_message_list_from_db(&messages, BranchSelection::LatestBranch);
    let types: Vec<&str> = list.iter().map(|(t, _, _)| t.as_str()).collect();
    assert!(!types.contains(&"tool_result"));
    assert!(types.contains(&"response"));
}

#[test]
fn given_tool_call_response_and_tool_result_when_build_chat_request_then_includes_tool_call_and_tool_response(
) {
    let init_message_list = vec![
        ("system".to_string(), "system".to_string(), vec![]),
        ("user".to_string(), "question".to_string(), vec![]),
        ("response".to_string(), response_with_tool_call(1, "call tool"), vec![]),
        ("tool_result".to_string(), tool_result_content("1", "ok"), vec![]),
    ];

    let result =
        build_chat_request_from_messages(&init_message_list, ToolCallStrategy::Native, None);
    let messages = result.chat_request.messages;
    assert_eq!(messages.len(), 4);
    assert!(matches!(&messages[2].role, ChatRole::Assistant));
    assert_eq!(messages[2].content.tool_calls().len(), 1);
    assert!(matches!(&messages[3].role, ChatRole::Tool));
}

#[test]
fn given_tool_result_without_matching_call_when_build_chat_request_then_downgrades_to_user() {
    let init_message_list = vec![
        ("system".to_string(), "system".to_string(), vec![]),
        ("user".to_string(), "question".to_string(), vec![]),
        ("response".to_string(), "plain response".to_string(), vec![]),
        ("tool_result".to_string(), tool_result_content("1", "ok"), vec![]),
    ];

    let result =
        build_chat_request_from_messages(&init_message_list, ToolCallStrategy::Native, None);
    let messages = result.chat_request.messages;
    assert_eq!(messages.len(), 4);
    assert!(matches!(&messages[3].role, ChatRole::User));
}

#[test]
fn given_tool_result_missing_call_id_when_build_chat_request_then_downgrades_to_user() {
    let init_message_list = vec![
        ("system".to_string(), "system".to_string(), vec![]),
        ("user".to_string(), "question".to_string(), vec![]),
        ("response".to_string(), "plain response".to_string(), vec![]),
        (
            "tool_result".to_string(),
            "Tool execution completed:\n\nResult:\nmissing id".to_string(),
            vec![],
        ),
    ];

    let result =
        build_chat_request_from_messages(&init_message_list, ToolCallStrategy::Native, None);
    let messages = result.chat_request.messages;
    assert_eq!(messages.len(), 4);
    assert!(matches!(&messages[3].role, ChatRole::User));
}

#[test]
fn given_long_conversation_when_regenerating_last_then_truncates_tail() {
    let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut messages = Vec::new();
    messages.push(wrap(make_message(1, "system", base, None, None, "system")));

    let mut time = base;
    let mut id = 2;
    for i in 1..=10 {
        time = time + Duration::seconds(1);
        messages.push(wrap(make_message(id, "user", time, None, None, &format!("q{}", i))));
        id += 1;
        time = time + Duration::seconds(1);
        let group_id = format!("g{}", i);
        messages.push(wrap(make_message(
            id,
            "response",
            time,
            Some(&group_id),
            None,
            &format!("r{}", i),
        )));
        id += 1;
    }

    time = time + Duration::seconds(1);
    let regen_id = id;
    messages.push(wrap(make_message(regen_id, "response", time, Some("g5b"), Some("g5"), "r5b")));

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, regen_id]);
}

#[test]
fn given_long_conversation_when_regenerating_mid_then_truncates_tail() {
    let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut messages = Vec::new();
    messages.push(wrap(make_message(1, "system", base, None, None, "system")));

    let mut time = base;
    let mut id = 2;
    for i in 1..=10 {
        time = time + Duration::seconds(1);
        messages.push(wrap(make_message(id, "user", time, None, None, &format!("q{}", i))));
        id += 1;
        time = time + Duration::seconds(1);
        let group_id = format!("g{}", i);
        messages.push(wrap(make_message(
            id,
            "response",
            time,
            Some(&group_id),
            None,
            &format!("r{}", i),
        )));
        id += 1;
    }

    time = time + Duration::seconds(1);
    let regen_id = 5;
    messages.push(wrap(make_message(regen_id, "response", time, Some("g5b"), Some("g5"), "r5b")));

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, regen_id]);
}

#[test]
fn given_tool_call_retry_with_new_group_when_build_message_list_then_keeps_latest_tool_call_and_result(
) {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();

    let messages = vec![
        wrap(make_message(
            1,
            "response",
            t1,
            Some("g1"),
            None,
            &response_with_tool_call(1, "call tool"),
        )),
        wrap(make_message(2, "tool_result", t2, None, None, &tool_result_content("1", "error"))),
        wrap(make_message(
            3,
            "response",
            t3,
            Some("g1b"),
            Some("g1"),
            &response_with_tool_call(2, "retry"),
        )),
        wrap(make_message(4, "tool_result", t4, None, None, &tool_result_content("2", "ok"))),
    ];

    let list = build_message_list_from_db(&messages, BranchSelection::LatestBranch);
    let contents: Vec<String> = list.iter().map(|(_, content, _)| content.clone()).collect();
    assert!(contents.iter().any(|c| c.contains("\"call_2\"")));
    assert!(contents.iter().any(|c| c.contains("Tool Call ID: 2")));
    assert!(contents.iter().all(|c| !c.contains("\"call_1\"")));
    assert!(contents.iter().all(|c| !c.contains("Tool Call ID: 1")));

    let types: Vec<&str> = list.iter().map(|(t, _, _)| t.as_str()).collect();
    assert_eq!(types, vec!["response", "tool_result"]);
}

#[test]
fn given_regeneration_then_continue_when_latest_branch_then_keeps_new_tail() {
    let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let t2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 1).unwrap();
    let t3 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 2).unwrap();
    let t4 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 3).unwrap();
    let t5 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 4).unwrap();
    let t6 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 5).unwrap();
    let t7 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 6).unwrap();
    let t8 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 7).unwrap();

    let messages = vec![
        wrap(make_message(1, "system", t1, None, None, "system")),
        wrap(make_message(2, "user", t2, None, None, "q1")),
        wrap(make_message(3, "response", t3, Some("g1"), None, "r1")),
        wrap(make_message(4, "user", t4, None, None, "q2")),
        wrap(make_message(5, "response", t5, Some("g2"), None, "r2")),
        wrap(make_message(6, "response", t6, Some("g2b"), Some("g2"), "r2b")),
        wrap(make_message(7, "user", t7, None, None, "q3")),
        wrap(make_message(8, "response", t8, Some("g3"), None, "r3")),
    ];

    let result = get_latest_branch_messages(&messages);
    let ids: Vec<i64> = result.iter().map(|msg| msg.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 6, 7, 8]);
}

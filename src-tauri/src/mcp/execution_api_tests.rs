use super::*;

fn pairing_message(kind: &str, content: String) -> (String, String, Vec<crate::db::conversation_db::MessageAttachment>) {
    (kind.to_string(), content, Vec::new())
}

fn pairing_call(id: u64, llm_id: Option<&str>) -> String {
    format!("<!-- MCP_TOOL_CALL:{} -->", serde_json::json!({
        "call_id": id, "llm_call_id": llm_id,
        "server_name": "ui_interaction", "tool_name": "preview_code", "parameters": "{}"
    }))
}

#[test]
fn test_tool_result_pairing_native_id_does_not_require_database_aliases() {
    let messages = vec![
        pairing_message("response", pairing_call(2379, Some("call_preview"))),
        pairing_message("tool_result", "Tool Call ID: call_preview\nResult:\nError: Cancelled by user".into()),
        pairing_message("user", "Continue".into()),
    ];
    assert_eq!(collect_required_tool_call_ids_from_message_list(&messages), HashSet::from(["call_preview".to_string()]));
    validate_tool_result_pairing(772, &messages).unwrap();
    let built = crate::api::ai::conversation::build_chat_request_from_messages(
        &messages, crate::api::ai::conversation::ToolCallStrategy::NativeWithToolResponsePairing, None,
    );
    assert_eq!(built.chat_request.messages.len(), 3);
}

#[test]
fn test_tool_result_pairing_missing_one_result_reports_only_real_call() {
    let messages = vec![
        pairing_message("response", format!("{}{}", pairing_call(1, Some("call_one")), pairing_call(2, Some("call_two")))),
        pairing_message("tool_result", "Tool Call ID: call_one\nResult:\nok".into()),
    ];
    let error = validate_tool_result_pairing(772, &messages).unwrap_err().to_string();
    assert!(error.contains("call_two"));
    assert!(!error.contains("call_one"));
    assert!(!error.contains("mcp_tool_call_"));
}

#[test]
fn test_tool_result_pairing_legacy_ids_match_request_builder() {
    for llm_id in [None, Some("")] {
        for result_id in ["mcp_tool_call_23", "23"] {
            let messages = vec![
                pairing_message("response", pairing_call(23, llm_id)),
                pairing_message("tool_result", format!("Tool Call ID: {}\nResult:\nok", result_id)),
            ];
            assert_eq!(collect_required_tool_call_ids_from_message_list(&messages), HashSet::from(["mcp_tool_call_23".to_string()]));
            validate_tool_result_pairing(772, &messages).unwrap();
            let built = crate::api::ai::conversation::build_chat_request_from_messages(
                &messages, crate::api::ai::conversation::ToolCallStrategy::NativeWithToolResponsePairing, None,
            );
            assert_eq!(built.chat_request.messages.len(), 2);
            assert_eq!(built.chat_request.messages[0].content.tool_calls()[0].call_id, "mcp_tool_call_23");
        }
    }
}

#[test]
fn test_tool_result_pairing_incomplete_result_is_not_accepted() {
    let messages = vec![
        pairing_message("response", pairing_call(1, Some("call_one"))),
        pairing_message("tool_result", "Tool Call ID: call_one\ntruncated".into()),
    ];
    assert!(validate_tool_result_pairing(772, &messages).is_err());
}

fn call(id: i64, status: &str) -> MCPToolCall {
    MCPToolCall {
        id, conversation_id: 1, message_id: Some(10), assistant_message_id: Some(10),
        subtask_id: None, server_id: 1, server_name: "test".into(),
        tool_name: "test".into(), parameters: "{}".into(), status: status.into(),
        result: None, error: None, created_time: "0".into(), started_time: None,
        finished_time: None, llm_call_id: Some(format!("call_{id}")),
    }
}

#[test]
fn test_mixed_tool_round_waits_for_manual_and_running_calls() {
    for unfinished in ["pending", "executing", "unknown"] {
        for allow_error in [false, true] {
            for statuses in [["success", unfinished], [unfinished, "success"]] {
                let calls = vec![call(1, statuses[0]), call(2, statuses[1])];
                assert!(!tool_round_ready(&calls, allow_error));
            }
        }
    }
    assert!(tool_round_ready(&[call(1, "success"), call(2, "success")], false));
    assert!(!tool_round_ready(&[], true));
}

#[test]
fn test_mixed_tool_round_respects_error_policy() {
    let calls = vec![call(1, "success"), call(2, "failed")];
    assert!(!tool_round_ready(&calls, false));
    assert!(tool_round_ready(&calls, true));
    assert!(!tool_round_ready(&[call(1, "failed"), call(2, "pending")], true));
}

#[test]
fn test_mixed_tool_round_expands_auto_subset_without_other_rounds() {
    let db = MCPDatabase { conn: crate::db::connection::Connection::open_in_memory().unwrap() };
    db.conn.execute_batch("CREATE TABLE mcp_tool_call (
        id INTEGER PRIMARY KEY, conversation_id INTEGER, message_id INTEGER,
        server_id INTEGER, server_name TEXT, tool_name TEXT, parameters TEXT,
        status TEXT, result TEXT, error TEXT, created_time TEXT, started_time TEXT,
        finished_time TEXT, llm_call_id TEXT, assistant_message_id INTEGER, subtask_id INTEGER
    );
    INSERT INTO mcp_tool_call (id, conversation_id, message_id, server_id, server_name,
        tool_name, parameters, status, created_time, assistant_message_id)
    VALUES (1,1,10,1,'test','auto','{}','success','0',10),
           (2,1,10,1,'test','manual','{}','pending','0',10),
           (3,1,11,1,'test','other','{}','pending','0',11);").unwrap();
    let mut calls = vec![db.get_mcp_tool_call(1).unwrap()];
    expand_tool_call_round(&db, &mut calls).unwrap();
    assert_eq!(calls.iter().map(|call| call.id).collect::<Vec<_>>(), vec![1, 2]);
    assert!(!tool_round_ready(&calls, true));
    db.conn.execute("UPDATE mcp_tool_call SET status = 'success' WHERE id = 2", []).unwrap();
    let mut calls = vec![db.get_mcp_tool_call(2).unwrap()];
    expand_tool_call_round(&db, &mut calls).unwrap();
    assert!(tool_round_ready(&calls, false));
    expand_tool_call_round(&db, &mut calls).unwrap();
    assert_eq!(calls.len(), 2);
}

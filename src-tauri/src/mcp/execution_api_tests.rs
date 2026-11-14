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

#[test]
fn test_plugin_structured_content_is_preserved_without_host_private_metadata() {
    let response: rmcp::model::CallToolResult=serde_json::from_value(serde_json::json!({"content":[],"structuredContent":{"rows":[{"value":42}]},"_meta":{"private":"host-only"}})).unwrap();
    let output=super::serialize_tool_response(&response).unwrap();
    assert!(output.contains("42"));assert!(!output.contains("host-only"));
}

#[test]
fn test_mcp_response_redacts_before_structured_json_wrappers() {
    let secret = "synthetic-quoted\"value\\with\nnewline";
    let redactor = crate::mcp::http_transport::ResponseRedactor::new([secret.to_owned()].into_iter());
    let response: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
        "content": [{"type": "text", "text": secret}],
        "structuredContent": {"nested": [{"secret": secret}]}
    })).unwrap();
    let output = serialize_tool_response_with_redactor(&response, &redactor).unwrap();
    assert!(!output.contains("synthetic-quoted"));
    assert!(!output.contains("newline"));
    let content: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(content[0]["text"], "[REDACTED]");
    let structured: serde_json::Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(structured["nested"][0]["secret"], "[REDACTED]");
}

#[test]
fn test_mcp_error_redaction_combines_config_and_placeholder_credentials() {
    let config = "synthetic-config-token";
    let placeholder = "synthetic-placeholder\"token";
    let config_redactor = crate::vault::executor::redactor_for([config.to_owned()]);
    let call_redactor = crate::vault::executor::redactor_for([placeholder.to_owned()]);
    let redactor = config_redactor.merged(Some(&call_redactor));
    let response: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
        "isError": true, "content": [{"type": "text", "text": format!("{config} / {placeholder}")}]
    })).unwrap();
    let error = serialize_tool_response_with_redactor(&response, &redactor).unwrap_err();
    let error_for_log = redactor.clean(&format!("Tool execution failed: {error:#}"));
    assert!(!error_for_log.contains("synthetic-config"));
    assert!(!error_for_log.contains("synthetic-placeholder"));
    assert!(error_for_log.contains("工具执行错误"));
    assert!(error_for_log.contains("[REDACTED]"));
}


#[test]
fn test_mcp_error_json_text_is_redacted_before_error_wrapper() {
    let secret = "synthetic-error\"quoted\\value\nlast-line";
    let redactor = crate::vault::executor::redactor_for([secret.to_owned()]);
    let response: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
        "isError": true,
        "content": [{"type": "text", "text": serde_json::json!({"credential": secret}).to_string()}]
    })).unwrap();
    let error = serialize_tool_response_with_redactor(&response, &redactor).unwrap_err().to_string();
    assert!(!error.contains("synthetic-error"));
    assert!(!error.contains("last-line"));
    assert!(error.contains("[REDACTED]"));
}

#[test]
fn test_builtin_response_preserves_envelope_semantics_when_credentials_match_field_names() {
    for secret in ["content", "is_error", "isError"] {
        let redactor = crate::vault::executor::redactor_for([secret.to_owned()]);
        for flag in ["is_error", "isError"] {
            for expected_error in [false, true] {
                let raw = serde_json::json!({
                    (flag): expected_error,
                    "content": [{"type": "text", "text": format!("echo: {secret}")}]
                }).to_string();
                let (is_error, content) = builtin_response_payload(&raw, &redactor).unwrap();
                assert_eq!(is_error, expected_error);
                assert_eq!(content[0]["text"], "echo: [REDACTED]");
                assert!(!content.to_string().contains(secret));
            }
        }
    }
}

#[test]
fn test_builtin_response_still_redacts_nested_payload_keys_and_values() {
    let secret = "synthetic-secret\"with\\escaping\nlast-line";
    let redactor = crate::vault::executor::redactor_for([secret.to_owned()]);
    let raw = serde_json::json!({
        "is_error": true,
        "content": { (secret): [secret, {"text": serde_json::json!({"echo": secret}).to_string()}] }
    }).to_string();
    let (is_error, content) = builtin_response_payload(&raw, &redactor).unwrap();
    assert!(is_error);
    assert_eq!(content["[REDACTED]"][0], "[REDACTED]");
    let wrapped = format!("工具执行错误: {content}");
    assert!(!wrapped.contains("synthetic-secret"));
    assert!(!wrapped.contains("last-line"));
}

#[test]
fn test_stdio_response_redacts_plugin_connection_credentials_without_placeholders() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let secret = "synthetic-plugin-connection\"secret";
    // The process redactor includes credentials resolved by the plugin launcher, even with
    // no vault placeholder or vault:// environment reference on this call.
    let redactor = crate::vault::executor::redactor_for([secret.to_owned()]);
    for is_error in [false, true] {
        let response: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
            "isError": is_error,
            "content": [{"type": "text", "text": format!("literal {secret}; encoded {}", STANDARD.encode(secret))}],
            "structuredContent": {"credential": secret}
        })).unwrap();
        let output = match serialize_tool_response_with_redactor(&response, &redactor) {
            Ok(output) => { assert!(!is_error); output }
            Err(error) => { assert!(is_error); error.to_string() }
        };
        assert!(!output.contains("synthetic-plugin"));
        assert!(!output.contains(&STANDARD.encode(secret)));
        assert!(output.contains("[REDACTED]"));
    }
}

#[test]
fn test_mcp_response_preserves_content_schema_when_credentials_match_protocol() {
    // Inject the test secret explicitly to isolate schema handling. This does not change
    // redactor_for's short-value policy or prove complete short-credential leak protection.
    for secret in ["text", "type", "image", "resource", "t"] {
        let redactor = crate::mcp::http_transport::ResponseRedactor::new([secret.to_owned()].into_iter());
        for is_error in [false, true] {
            let response: rmcp::model::CallToolResult = serde_json::from_value(serde_json::json!({
                "isError": is_error,
                "content": [{"type":"text", "text":secret}],
                "structuredContent": {(secret): secret}
            })).unwrap();
            let output = match serialize_tool_response_with_redactor(&response, &redactor) {
                Ok(output) => {
                    assert!(!is_error);
                    // HTTP/stdio perform a second cleanup after serialization.
                    redactor.clean_tool_output(&output)
                }
                Err(error) => {
                    assert!(is_error);
                    error.to_string().strip_prefix("工具执行错误: ").unwrap().to_owned()
                }
            };
            let content: Vec<rmcp::model::Content> = serde_json::from_str(&output).unwrap();
            assert_eq!(content[0].as_text().unwrap().text, "[REDACTED]");
            if !is_error {
                let structured: serde_json::Value = serde_json::from_str(&content[1].as_text().unwrap().text).unwrap();
                assert_eq!(structured, serde_json::json!({"[REDACTED]":"[REDACTED]"}));
            }
        }
    }
}

#[test]
fn test_builtin_response_preserves_content_schema_when_credentials_match_protocol() {
    // Inject the test secret explicitly to isolate schema handling. This does not change
    // redactor_for's short-value policy or prove complete short-credential leak protection.
    for secret in ["text", "type", "content", "is_error", "isError", "t"] {
        let redactor = crate::mcp::http_transport::ResponseRedactor::new([secret.to_owned()].into_iter());
        for flag in ["is_error", "isError"] {
            for expected_error in [false, true] {
                let raw = serde_json::json!({
                    (flag):expected_error, "content":[{"type":"text", "text":secret}]
                }).to_string();
                let (is_error, content) = builtin_response_payload(&raw, &redactor).unwrap();
                assert_eq!(is_error, expected_error);
                let content: Vec<rmcp::model::Content> = serde_json::from_value(content).unwrap();
                assert_eq!(content[0].as_text().unwrap().text, "[REDACTED]");
            }
        }
    }
}

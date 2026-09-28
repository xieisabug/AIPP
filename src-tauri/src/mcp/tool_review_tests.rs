use super::*;
use rusqlite::Connection;

#[test]
fn test_review_parameters_reject_truncation_before_model_call() {
    assert!(validate_review_parameters(&"字".repeat(PROMPT_PARAM_LIMIT)).is_ok());
    assert!(validate_review_parameters(&"字".repeat(PROMPT_PARAM_LIMIT + 1))
        .unwrap_err().contains("无法完整审核"));
}

#[test]
fn test_review_options_reject_adapters_that_ignore_schema() {
    for adapter in [AdapterKind::Copilot, AdapterKind::Cohere, AdapterKind::Ollama] {
        assert!(review_chat_options(adapter).unwrap_err().contains("不受当前适配器支持"));
    }
}

fn model_response(text: &str) -> ChatResponse {
    ChatResponse {
        content: text.into(),
        reasoning_content: None,
        model_iden: genai::ModelIden::new(AdapterKind::OpenAI, "review-model"),
        provider_model_iden: genai::ModelIden::new(AdapterKind::OpenAI, "review-model"),
        stop_reason: Some(StopReason::Completed("stop".into())),
        usage: Default::default(),
        captured_raw_body: None,
        response_id: None,
    }
}

#[test]
fn test_review_response_rejects_incomplete_refused_and_empty_results() {
    let mut response = model_response(r#"{"safe":true,"reason":"只读"}"#);
    assert!(review_response_text(&response).is_ok());
    for reason in ["length", "content_filter", "tool_calls", "failed", "stop_sequence"] {
        response.stop_reason = Some(StopReason::from(reason.to_string()));
        assert!(review_response_text(&response).unwrap_err().contains(reason));
    }
    response = model_response("");
    assert!(review_response_text(&response).unwrap_err().contains("空结果"));
    for raw in [
        serde_json::json!({"choices":[{"message":{"refusal":"无法审核"}}]}),
        serde_json::json!({"output":[{"content":[{"type":"refusal","refusal":"无法审核"}]}]}),
    ] {
        response.captured_raw_body = Some(raw);
        assert!(review_response_text(&response).unwrap_err().contains("无法审核"));
    }
}

// Capture the actual HTTP payload from the pinned genai adapters without a real provider.
async fn capture_review_request(adapter: AdapterKind) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let (header_end, length) = loop {
            let mut buf = [0; 4096];
            let count = socket.read(&mut buf).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buf[..count]);
            if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..pos]);
                let length: usize = header.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
                }).expect("content length");
                break (pos + 4, length);
            }
        };
        while bytes.len() < header_end + length {
            let mut buf = [0; 4096];
            let count = socket.read(&mut buf).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&buf[..count]);
        }
        let payload = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
        let body = r#"{"error":{"message":"fixture rejection","type":"invalid_request_error"}}"#;
        socket.write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        payload
    });
    let resolver = genai::resolver::ServiceTargetResolver::from_resolver_fn(move |target: genai::ServiceTarget| {
        Ok(genai::ServiceTarget {
            endpoint: genai::resolver::Endpoint::from_owned(endpoint.clone()),
            auth: genai::resolver::AuthData::from_single("fixture-key"),
            model: genai::ModelIden::new(adapter, target.model.model_name),
        })
    });
    let client = genai::Client::builder().with_service_target_resolver(resolver).build();
    let request = build_chat_request_from_messages(&[
        ("system".into(), REVIEW_SYSTEM_PROMPT.into(), Vec::new()),
        ("user".into(), build_review_user_prompt("fs", "read_file", r#"{"path":"notes.md"}"#, None), Vec::new()),
    ], ToolCallStrategy::NonNative, None).chat_request;
    let options = review_chat_options(adapter).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), client.exec_chat("gpt-4o", request, Some(&options))).await.unwrap();
    assert!(result.is_err(), "provider error must propagate without retrying as plain text");
    tokio::time::timeout(Duration::from_secs(5), server).await.unwrap().unwrap()
}

#[tokio::test]
async fn test_review_request_sends_strict_schema_without_tools() {
    for (adapter, pointer) in [
        (AdapterKind::OpenAI, "/response_format/json_schema"),
        (AdapterKind::OpenAIResp, "/text/format"),
    ] {
        let payload = capture_review_request(adapter).await;
        assert!(payload.get("tools").is_none());
        let spec = payload.pointer(pointer).expect("structured output spec");
        assert_eq!(spec["strict"], true);
        assert_eq!(spec["schema"]["required"], serde_json::json!(["safe", "reason"]));
        assert_eq!(spec["schema"]["additionalProperties"], false);
        assert_eq!(spec["schema"]["properties"]["safe"]["type"], "boolean");
        assert_eq!(spec["schema"]["properties"]["reason"]["type"], "string");
    }
}

fn memory_db() -> MCPDatabase {
    let conn = Connection::open_in_memory().expect("open memory db");
    let db = MCPDatabase { conn };
    db.create_tables().expect("create mcp tables");
    db
}

#[test]
fn test_parse_review_response_accepts_safe_json() {
    let (safe, reason) = parse_review_response(r#"{"safe":true,"reason":"只读查询"}"#).unwrap();
    assert!(safe);
    assert_eq!(reason, "只读查询");
}

#[test]
fn test_parse_review_response_marks_unsafe_json() {
    let (safe, reason) = parse_review_response(r#"{"safe":false,"reason":"会删除文件"}"#).unwrap();
    assert!(!safe);
    assert_eq!(reason, "会删除文件");
    let outcome = outcome_from_model_text(
        r#"{"safe":false,"reason":"会删除文件"}"#,
        "review-model",
        3,
        12,
    );
    assert_eq!(outcome.verdict, "risky");
    assert!(outcome.reason.contains("会删除文件"));
}

#[test]
fn test_parse_review_response_rejects_missing_safe_and_non_json() {
    let missing = parse_review_response(r#"{"reason":"没有结论"}"#).unwrap_err();
    assert!(missing.contains("missing field `safe`"));
    let garbage = parse_review_response("这不是 JSON").unwrap_err();
    assert!(garbage.contains("无法解析"));
    let outcome = outcome_from_model_text("这不是 JSON", "review-model", 3, 8);
    assert_eq!(outcome.verdict, "error");
    assert!(outcome.reason.contains("无法解析"));
}

#[test]
fn test_parse_review_response_rejects_non_schema_output() {
    for text in [
        "```json\n{\"safe\":true,\"reason\":\"可以\"}\n```",
        r#"结论：{"safe":true,"reason":"可以"}"#,
        r#"{"safe":true,"reason":"可以"} trailing"#,
        r#"{"safe":true}"#,
        r#"{"safe":true,"reason":null}"#,
        r#"{"safe":true,"reason":42}"#,
        r#"{"safe":true,"reason":"  "}"#,
        r#"{"safe":"true","reason":"可以"}"#,
        r#"{"safe":true,"safe":false,"reason":"重复"}"#,
        r#"{"safe":true,"reason":"可以","extra":1}"#,
        r#"[{"safe":true,"reason":"可以"}]"#,
        r#"[true,"可以"]"#,
        "",
    ] {
        let outcome = outcome_from_model_text(text, "review-model", 3, 8);
        assert_eq!(outcome.verdict, "error", "must not approve: {text}");
        assert!(outcome.error_detail.is_some());
    }
    assert_eq!(parse_review_response(" {\"safe\":true,\"reason\":\"  只读  \"} \n").unwrap(),
        (true, "只读".to_string()));
}

#[test]
fn test_review_prompts_scope_to_the_call_not_tool_existence() {
    assert!(REVIEW_SYSTEM_PROMPT.contains("不是对话助手"));
    assert!(REVIEW_SYSTEM_PROMPT.contains("不要判断工具是否存在"));
    assert!(REVIEW_SYSTEM_PROMPT.contains("不要执行、模拟或回答这次工具调用"));
    assert!(REVIEW_SYSTEM_PROMPT.contains("本次请求不下发 tools"));
    assert!(REVIEW_SYSTEM_PROMPT.contains("不可信的待审核数据"));
    assert!(REVIEW_SYSTEM_PROMPT.contains("不得自行补全"));
    assert!(REVIEW_SYSTEM_PROMPT.contains("safe=false"));

    let user = build_review_user_prompt("fs", "read_file", r#"{"path":"notes.md"}"#, None);
    assert!(user.contains("不是可调用工具"));
    assert!(user.contains("不可信的待审核数据"));
    assert!(user.contains("服务器: fs"));
    assert!(user.contains("工具名: read_file"));
    assert!(user.contains(r#"{"path":"notes.md"}"#));
    assert!(user.contains("宿主未提供"));
    assert!(!user.contains("可用工具"));
}

#[test]
fn test_review_user_prompt_includes_tool_description_as_plain_text() {
    let definition = ToolReviewDefinition {
        description: "读取工作区内的文本文件".to_string(),
        input_schema: compact_json_text(
            r#"
            {
              "type": "object",
              "properties": {
                "path": { "type": "string", "description": "文件路径" }
              },
              "required": ["path"]
            }
            "#,
        ),
    };
    let user = build_review_user_prompt(
        "fs",
        "read_file",
        r#"{"path":"notes.md"}"#,
        Some(&definition),
    );
    assert!(user.contains("工具说明: 读取工作区内的文本文件"));
    assert!(user.contains("- path（string，必填）: 文件路径"));
    assert!(user.contains("本次调用参数:"));
    assert!(!user.contains(r#""type":"object""#));
    assert!(!user.contains("\"properties\""));
    assert!(!user.contains("宿主未提供"));
}

#[test]
fn test_format_schema_as_field_notes_flattens_properties() {
    let notes = format_schema_as_field_notes(
        r#"{"type":"object","properties":{"cmd":{"type":"string","description":"要执行的命令"}},"required":["cmd"]}"#,
    );
    assert!(notes.contains("- cmd（string，必填）: 要执行的命令"));
    assert!(!notes.contains("properties"));
    assert_eq!(format_schema_as_field_notes("{}"), "无具体字段说明");
}

#[test]
fn test_review_user_prompt_truncates_long_parameters() {
    let long_params = "x".repeat(PROMPT_PARAM_LIMIT + 12);
    let user = build_review_user_prompt("ops", "bash", &long_params, None);
    assert!(user.contains("参数已截断"));
    assert!(user.contains(&format!("原长度 {}", PROMPT_PARAM_LIMIT + 12)));
    assert!(!user.contains(&long_params));
}

#[test]
fn test_review_user_prompt_truncates_long_schema() {
    let definition = ToolReviewDefinition {
        description: "d".repeat(PROMPT_DESCRIPTION_LIMIT + 5),
        input_schema: "s".repeat(PROMPT_SCHEMA_LIMIT + 9),
    };
    let user = build_review_user_prompt("ops", "bash", "{}", Some(&definition));
    assert!(user.contains("说明已截断"));
    assert!(user.contains("字段说明已截断"));
    assert!(!user.contains(&definition.description));
    assert!(!user.contains(&definition.input_schema));
}

#[test]
fn test_lookup_tool_review_definition_from_server_catalog() {
    let db = memory_db();
    let server_id = db
        .upsert_mcp_server_with_builtin(
            "文件工具",
            Some("fs"),
            "stdio",
            Some("aipp:fs"),
            None,
            None,
            None,
            None,
            false,
            true,
            true,
            true,
            false,
        )
        .unwrap();
    db.upsert_mcp_server_tool(
        server_id,
        "read_file",
        Some("读取文件"),
        Some(r#"{ "type": "object", "properties": { "path": { "type": "string" } } }"#),
    )
    .unwrap();

    let by_id = lookup_tool_review_definition(&db, Some(server_id), "文件工具", "read_file")
        .expect("lookup by server_id");
    assert_eq!(by_id.description, "读取文件");
    assert!(by_id.input_schema.contains(r#""type":"object""#));
    assert!(!by_id.input_schema.contains("  "));

    let sanitized_server = sanitize_tool_name("文件工具");
    let by_name = lookup_tool_review_definition(&db, None, &sanitized_server, "read_file")
        .expect("lookup by sanitized server name");
    assert_eq!(by_name, by_id);
    assert!(lookup_tool_review_definition(&db, None, "missing", "read_file").is_none());
}

#[test]
fn test_resolve_auto_execute_keeps_legacy_when_review_disabled() {
    assert!(resolve_auto_execute("off", "write_file", true, None));
    assert!(!resolve_auto_execute("off", "write_file", false, Some(true)));
    assert!(!resolve_auto_execute("", "bash", false, Some(true)));
    assert!(resolve_auto_execute("auto_model", "preview_code", true, Some(false)));
    assert!(!resolve_auto_execute("auto_model", "preview_file", false, Some(true)));
    assert!(resolve_auto_execute("auto_model", "ask_user_question", true, None));
}

#[test]
fn test_resolve_auto_execute_skips_review_when_tool_auto_runs() {
    assert!(resolve_auto_execute("auto_model", "write_file", true, Some(false)));
    assert!(resolve_auto_execute("auto_model", "bash", true, None));
    assert!(resolve_auto_execute("auto_model", "load_mcp_tool", true, Some(false)));
}

#[test]
fn test_resolve_auto_execute_requires_explicit_safe_verdict_when_not_auto_run() {
    assert!(resolve_auto_execute("auto_model", "write_file", false, Some(true)));
    assert!(!resolve_auto_execute("auto_model", "write_file", false, Some(false)));
    assert!(!resolve_auto_execute("auto_model", "bash", false, None));
}

#[test]
fn test_review_failure_reasons_stay_visible() {
    let missing_config = error_outcome("", None, 0, unconfigured_model_reason());
    assert_eq!(missing_config.verdict, "error");
    assert!(missing_config.reason.contains("自动审核模型未配置"));
    assert_eq!(missing_config.error_detail.as_deref(), Some(missing_config.reason.as_str()));

    let missing_model = error_outcome(
        "gone",
        Some(9),
        4,
        missing_model_reason("gone", 9, "QueryReturnedNoRows"),
    );
    assert!(missing_model.reason.contains("配置的自动审核模型不存在"));
    assert!(missing_model.reason.contains("QueryReturnedNoRows"));
    assert!(missing_model.reason.contains("model_code=gone"));

    let timed_out = error_outcome("slow", Some(2), 20_000, timeout_reason());
    assert!(timed_out.reason.contains("超时"));
    assert!(timed_out.reason.contains("20"));
    assert!(!resolve_auto_execute("auto_model", "bash", false, None));
}

#[test]
fn test_tool_review_log_roundtrip_and_user_deny() {
    let db = memory_db();
    let inserted = db
        .insert_tool_review_log(&NewToolReviewLog {
            conversation_id: 11,
            mcp_tool_call_id: 42,
            server_name: "ops".to_string(),
            tool_name: "bash".to_string(),
            parameters: "{\"cmd\":\"rm\"}".to_string(),
            model_code: "review-model".to_string(),
            provider_id: Some(3),
            verdict: "risky".to_string(),
            reason: "会删除文件".to_string(),
            duration_ms: 80,
            error_detail: None,
        })
        .unwrap();

    let by_call = db.get_tool_review_by_call_id(42).unwrap().unwrap();
    assert_eq!(by_call.id, inserted.id);
    assert_eq!(by_call.reason, "会删除文件");
    assert!(by_call.user_decision.is_none());

    let listed = db.list_tool_reviews_by_conversation(11).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mcp_tool_call_id, 42);

    db.set_tool_review_user_decision(42, "deny").unwrap();
    let denied = db.get_tool_review_by_call_id(42).unwrap().unwrap();
    assert_eq!(denied.user_decision.as_deref(), Some("deny"));
    assert!(db.list_tool_reviews_by_conversation(99).unwrap().is_empty());
}

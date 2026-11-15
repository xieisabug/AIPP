use super::{prepare_empty_model_reply, EMPTY_MODEL_REPLY_NOTICE, REASONING_ONLY_MODEL_REPLY_NOTICE};

#[test]
fn test_http_diagnostics_reject_header_urls_and_surface_bad_request() {
    let message = "Unsupported parameter: 'temperature' is not supported with this model.";
    let body = serde_json::json!({"error": {"message": message, "param": "temperature"}});
    let error = format!("Stream processing failed: HTTP error.\nStatus: 400 Bad Request\nBody: {body}; transport: Internal(...) headers: {{\"server\": \"cloudflare\", \"report-to\": \"https://telemetry.example/report/503\"}}");
    let details = super::extract_http_details_from_anyhow(&anyhow::anyhow!(error.clone()));
    assert_eq!(details.status_code, Some(400));
    assert_eq!(details.endpoint, None);
    let friendly = super::friendly_http_error(&details, &error);
    assert!(friendly.contains(message));
    let payload = super::build_rich_error_payload_with_http_details(
        friendly, None, None, "stream", Some(1), error, Some(details),
    );
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["suggestions"], serde_json::json!(["检查 Base URL / 模型配置与请求参数格式"]));
}

#[test]
fn test_http_diagnostics_show_request_url_without_credentials() {
    let mut details = super::HttpErrorDetails::default();
    super::parse_status_from_string(
        "Cause: error sending request for url (http://user:secret@localhost:4000/v1/responses?key=secret)",
        &mut details,
    );
    assert_eq!(details.endpoint.as_deref(), Some("http://localhost:4000/v1/responses"));
    assert_eq!(details.status_code, None);
}

#[test]
fn test_empty_reply_classifies_bodyless_success_and_preserves_metadata() {
    for (reasoning, expected, kind) in [
        ("", EMPTY_MODEL_REPLY_NOTICE, "empty_model_reply"),
        ("  \n", EMPTY_MODEL_REPLY_NOTICE, "empty_model_reply"),
        ("先想一想", REASONING_ONLY_MODEL_REPLY_NOTICE, "reasoning_only_model_reply"),
    ] {
        let mut content = " \n".to_string();
        let metadata = prepare_empty_model_reply(
            &mut content, reasoning, 0, 0, Some(r#"{"response_id":"r1","stream_retry":{"attempt":1}}"#.into()),
        ).unwrap();
        assert_eq!(content, expected);
        let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(metadata["local_reply_notice"]["source"], "aipp");
        assert_eq!(metadata["local_reply_notice"]["kind"], kind);
        assert_eq!(metadata["response_id"], "r1");
        assert_eq!(metadata["stream_retry"]["attempt"], 1);
    }
}

#[test]
fn test_empty_reply_preserves_real_text_tools_and_binary_output() {
    for (body, tools, binaries) in [
        ("answer", 0, 0),
        (EMPTY_MODEL_REPLY_NOTICE, 0, 0),
        (REASONING_ONLY_MODEL_REPLY_NOTICE, 0, 0),
        ("", 1, 0),
        ("", 0, 1),
    ] {
        let mut content = body.to_string();
        let metadata = prepare_empty_model_reply(&mut content, "thinking", tools, binaries, None);
        assert_eq!(content, body);
        assert!(metadata.is_none());
    }
}

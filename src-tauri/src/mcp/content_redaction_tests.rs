use super::http_transport::ResponseRedactor;
use rmcp::model::Content;
use serde_json::{json, Value};

/// Protocol keywords can be credentials, but only their occurrences in data are sensitive.
#[test]
fn test_content_redaction_preserves_all_mcp_variants_and_protocol_fields() {
    // Inject the test secret explicitly to isolate schema handling. This does not change
    // redactor_for's short-value policy or prove complete short-credential leak protection.
    for secret in ["text", "type", "image", "audio", "resource", "resource_link", "data", "mimeType", "uri", "blob", "annotations", "audience", "user", "t"] {
        let redactor = crate::mcp::http_transport::ResponseRedactor::new([secret.to_owned()].into_iter());
        let raw = json!([
            {"type":"text", "text":secret, "annotations":{"audience":["user","assistant"], "priority":0.5}, "_meta":{(secret):secret}},
            {"type":"image", "data":secret, "mimeType":secret},
            {"type":"audio", "data":secret, "mimeType":secret},
            {"type":"resource", "resource":{"uri":secret, "text":secret, "mimeType":secret, "_meta":{(secret):secret}}},
            {"type":"resource", "resource":{"uri":secret, "blob":secret}},
            {"type":"resource_link", "uri":secret, "name":secret, "title":secret, "description":secret,
                "mimeType":secret, "icons":[{"src":secret, "mimeType":secret, "sizes":[secret]}]}
        ]);
        let output = redactor.clean_tool_output(&raw.to_string());
        let parsed: Vec<Content> = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.len(), 6, "credential: {secret}");
        let cleaned: Value = serde_json::from_str(&output).unwrap();
        for (index, kind) in ["text", "image", "audio", "resource", "resource", "resource_link"].into_iter().enumerate() {
            assert_eq!(cleaned[index]["type"], kind, "credential: {secret}");
        }
        assert_eq!(cleaned[0]["text"], "[REDACTED]");
        assert_eq!(cleaned[0]["annotations"]["audience"], json!(["user", "assistant"]));
        assert_eq!(cleaned[0]["_meta"], json!({"[REDACTED]":"[REDACTED]"}));
        for index in [1, 2] {
            assert_eq!(cleaned[index]["data"], "[REDACTED]");
            assert_eq!(cleaned[index]["mimeType"], "[REDACTED]");
        }
        assert_eq!(cleaned[3]["resource"]["text"], "[REDACTED]");
        assert_eq!(cleaned[3]["resource"]["uri"], "[REDACTED]");
        assert_eq!(cleaned[3]["resource"]["_meta"], json!({"[REDACTED]":"[REDACTED]"}));
        assert_eq!(cleaned[4]["resource"]["blob"], "[REDACTED]");
        for field in ["uri", "name", "title", "description", "mimeType"] {
            assert_eq!(cleaned[5][field], "[REDACTED]");
        }
        assert_eq!(cleaned[5]["icons"][0], json!({"src":"[REDACTED]", "mimeType":"[REDACTED]", "sizes":["[REDACTED]"]}));
    }
}

/// Arbitrary JSON in tool text and non-MCP builtin payloads must not receive schema exemptions.
#[test]
fn test_content_redaction_still_scrubs_arbitrary_payload_keys_and_values() {
    let redactor = ResponseRedactor::new(["text".to_owned()].into_iter());
    let payload = json!({"type":"text", "text":[{"text":"text"}]});
    let output = redactor.clean_tool_output(&payload.to_string());
    assert_eq!(serde_json::from_str::<Value>(&output).unwrap(), json!({"type":"[REDACTED]", "[REDACTED]":[{"[REDACTED]":"[REDACTED]"}]}));
    let content = json!([{"type":"text", "text":payload.to_string()}]);
    let output = redactor.clean_tool_output(&content.to_string());
    let content: Vec<Content> = serde_json::from_str(&output).unwrap();
    assert!(!content[0].as_text().unwrap().text.contains("text"));
    assert_eq!(redactor.clean_tool_output("plain text"), "plain [REDACTED]");
}

#[test]
fn test_content_redaction_omits_sensitive_timestamp_without_invalidating_annotations() {
    let redactor = ResponseRedactor::new(["2026-11-14".to_owned()].into_iter());
    let output = redactor.clean_tool_output(&json!([{
        "type":"text", "text":"safe",
        "annotations":{"audience":["user"], "lastModified":"2026-11-14T01:02:03Z"}
    }]).to_string());
    let content: Vec<Content> = serde_json::from_str(&output).unwrap();
    assert!(content[0].annotations.as_ref().unwrap().last_modified.is_none());
    assert!(!output.contains("2026-11-14"));
}

#[test]
fn test_content_redaction_preserves_and_scrubs_unknown_extension_payloads() {
    let redactor = ResponseRedactor::new(["text".to_owned()].into_iter());
    let output = redactor.clean_tool_output(&json!([{
        "type":"resource", "resource":{"uri":"safe", "text":"text", "extension":{"text":"text"}},
        "extension_text":{"type":"text", "text":"text"}
    }]).to_string());
    let parsed: Vec<Content> = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed.len(), 1);
    let content: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(content[0]["resource"]["extension"], json!({"[REDACTED]":"[REDACTED]"}));
    assert_eq!(content[0]["extension_[REDACTED]"], json!({"type":"[REDACTED]", "[REDACTED]":"[REDACTED]"}));
}

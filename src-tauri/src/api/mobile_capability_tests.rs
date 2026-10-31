use super::*;

#[test]
fn test_mobile_capability_distinguishes_transport_and_real_builtin() {
    assert!(tool_issue("stdio", Some("npx server"), None).is_some());
    assert!(tool_issue("stdio", Some("aipp:unknown"), None).is_some());
    assert!(tool_issue("stdio", Some("aipp:search --execute"), None).is_some());
    assert!(tool_issue("http", Some("aipp:search"), Some("search_web")).is_none());
    assert!(tool_issue("sse", None, None).is_none());
    assert!(tool_issue("builtin", Some("aipp:search"), Some("fetch_url")).is_none());
}

#[test]
fn test_mobile_capability_preserves_lightweight_tools() {
    assert!(tool_issue("stdio", Some("aipp:search"), Some("fetch_url")).is_none());
    assert!(tool_issue("stdio", Some("aipp:operation"), Some("read_file")).is_none());
    assert!(tool_issue("stdio", Some("aipp:search"), Some("search_web")).is_some());
    assert!(tool_issue("stdio", Some("aipp:operation"), Some("execute_bash")).is_some());
    assert!(assistant_issue(Some(4)).is_some());
    assert!(assistant_issue(Some(2)).is_some());
    assert!(assistant_issue(None).is_none());
}

#[test]
fn test_mobile_capability_error_keeps_stage_and_identity() {
    let error = assistant_issue(Some(4)).unwrap().error("send_preflight", "assistant_id=7; conversation_id=not_created");
    for expected in ["MOBILE_UNSUPPORTED", "local_cli_agent", "send_preflight", "assistant_id=7", "conversation_id=not_created", "CLI"] {
        assert!(error.contains(expected));
    }
}

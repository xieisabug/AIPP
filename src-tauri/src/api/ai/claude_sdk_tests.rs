use super::claude_sdk::{claude_usage_counts, confirm_claude_resume, ClaudeUsageCounts};

#[test]
fn test_claude_resume_confirms_matching_session_id() {
    let confirmation = confirm_claude_resume(Some("session-1"), "session-1", "run-1")
        .expect("matching session id should confirm resume");

    assert_eq!(
        confirmation,
        Some(("resume".to_string(), "run-1:resume".to_string()))
    );
}

#[test]
fn test_claude_resume_rejects_mismatched_session_id() {
    let error = confirm_claude_resume(Some("session-1"), "session-2", "run-1")
        .expect_err("mismatched session id must fail");

    assert!(error.contains("session_id=session-1"));
    assert!(error.contains("session_id=session-2"));
}

#[test]
fn test_claude_resume_ignores_fresh_session() {
    assert_eq!(
        confirm_claude_resume(None, "session-1", "run-1")
            .expect("fresh session should not be treated as resume"),
        None
    );
}

#[test]
fn test_claude_usage_counts_include_cache_in_input() {
    let usage = serde_json::json!({
        "input_tokens": 12,
        "cache_creation_input_tokens": 3000,
        "cache_read_input_tokens": 45000,
        "output_tokens": 800,
    });
    assert_eq!(
        claude_usage_counts(&usage),
        ClaudeUsageCounts { input: 48012, output: 800, total: 48812, cache_read: Some(45000), cache_write: Some(3000) }
    );
}

#[test]
fn test_claude_usage_counts_without_cache_fields_reports_unknown_cache() {
    let usage = serde_json::json!({ "input_tokens": 20, "output_tokens": 5 });
    assert_eq!(
        claude_usage_counts(&usage),
        ClaudeUsageCounts { input: 20, output: 5, total: 25, cache_read: None, cache_write: None }
    );
}

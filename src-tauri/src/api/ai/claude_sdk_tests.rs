use super::claude_sdk::confirm_claude_resume;

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

use super::*;

#[test]
fn test_feishu_journal_recovery_marks_interrupted_and_prevents_replay() {
    let conn = Connection::open_in_memory().unwrap();
    assert!(begin(&conn, "event:e1", "im.message.receive_v1", Some("m1")).unwrap());
    assert!(!begin(&conn, "event:e1", "im.message.receive_v1", Some("m1")).unwrap());
    assert_eq!(recover(&conn).unwrap(), 1);
    let problem = last_problem(&conn).unwrap().unwrap();
    for text in ["event:e1", "m1", "handle_payload", "unknown", "不会自动重发"] {
        assert!(problem.contains(text), "{problem}");
    }
    assert!(!begin(&conn, "event:e1", "im.message.receive_v1", Some("m1")).unwrap());
}

#[test]
fn test_feishu_journal_preserves_handler_error_and_completed_event() {
    let conn = Connection::open_in_memory().unwrap();
    begin(&conn, "event:card", "card.action.trigger", None).unwrap();
    finish(&conn, "event:card", "failed", Some("request=42; approval denied")).unwrap();
    begin(&conn, "event:next", "im.message.receive_v1", Some("m2")).unwrap();
    finish(&conn, "event:next", "complete", None).unwrap();
    let problem = last_problem(&conn).unwrap().unwrap();
    assert!(problem.contains("event:card") && problem.contains("request=42; approval denied"));
    assert_eq!(recover(&conn).unwrap(), 0);
}

#[test]
fn test_feishu_journal_uses_event_message_or_hash_for_stable_identity() {
    let payload = br#"{"header":{"event_type":"im.message.receive_v1","event_id":"e1"},"event":{"message":{"message_id":"m1"}}}"#;
    let (key, kind, message) = identity(payload);
    assert_eq!(key, "event:e1");
    assert_eq!(kind, "im.message.receive_v1");
    assert_eq!(message.as_deref(), Some("m1"));
    assert_eq!(identity(b"bad").0, identity(b"bad").0);
}

#[tokio::test]
async fn test_feishu_relay_stop_waits_for_inflight_and_rejects_new_permit() {
    let state = std::sync::Arc::new(super::super::types::FeishuButlerState::default());
    *state.relay_enabled.write().await = true;
    let permit = state.relay_permit().await.unwrap();
    let cloned = state.clone();
    let stop = tokio::spawn(async move { *cloned.relay_enabled.write().await = false; });
    tokio::task::yield_now().await;
    assert!(!stop.is_finished());
    drop(permit);
    stop.await.unwrap();
    assert!(state.relay_permit().await.is_none());
}

#[test]
fn test_feishu_runtime_error_merges_current_and_journal_without_duplicates() {
    use super::super::runtime::merge_runtime_errors;
    let tls = "TLS: certificate rejected".to_string();
    let journal = "event_id=e1; outcome=unknown".to_string();
    let merged = merge_runtime_errors(Some(tls.clone()), Some(journal.clone())).unwrap();
    assert!(merged.contains(&tls) && merged.contains(&journal));
    assert_eq!(merge_runtime_errors(Some(journal.clone()), Some(journal.clone())), Some(journal.clone()));
    assert_eq!(merge_runtime_errors(None, Some(journal.clone())), Some(journal));
    assert_eq!(merge_runtime_errors(Some(tls.clone()), None), Some(tls));
}

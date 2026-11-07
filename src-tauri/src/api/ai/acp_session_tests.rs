use super::*;

fn entry(conversation: i64, run: &str) -> (AcpSessionEntry, mpsc::UnboundedReceiver<AcpSessionCommand>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let handle = AcpSessionHandle { sender, run_id: run.into() };
    let mut entry = AcpSessionEntry::new(handle, conversation, "signature");
    entry.last_activity = Instant::now() - Duration::from_secs(60);
    (entry, receiver)
}

#[test]
fn test_session_reuse_matching_signature_touches_and_preserves_run() {
    let (mut entry, _receiver) = entry(1, "run-1");
    let before = entry.last_activity;
    entry.snapshot.has_active_prompt = true;
    let snapshot = serde_json::to_value(&entry.snapshot).unwrap();
    let handle = entry.reuse_handle("signature").expect("same configuration reuses session");
    assert!(entry.last_activity > before);
    assert_eq!(handle.run_id, "run-1");
    assert_eq!(entry.run_id, "run-1");
    assert!(handle.sender.same_channel(&entry.handle.sender));
    assert_eq!(entry.config_signature, "signature");
    assert_eq!(serde_json::to_value(&entry.snapshot).unwrap(), snapshot);
}

#[test]
fn test_session_reuse_mismatch_preserves_entry_without_touching() {
    let (mut entry, _receiver) = entry(1, "run-1");
    let before = entry.last_activity;
    let snapshot = serde_json::to_value(&entry.snapshot).unwrap();
    assert!(entry.reuse_handle("different").is_none());
    assert_eq!(entry.last_activity, before);
    assert_eq!(entry.run_id, "run-1");
    assert_eq!(entry.handle.run_id, "run-1");
    assert_eq!(entry.config_signature, "signature");
    assert_eq!(serde_json::to_value(&entry.snapshot).unwrap(), snapshot);
}

#[tokio::test]
async fn test_session_reuse_two_conversations_preserve_command_isolation() {
    let (mut first, mut first_rx) = entry(1, "run-1");
    let (second, mut second_rx) = entry(2, "run-2");
    let second_activity = second.last_activity;
    let handle = first.reuse_handle("signature").unwrap();
    let cancel = tokio::spawn(async move { handle.cancel_current_prompt().await });
    match tokio::time::timeout(Duration::from_secs(2), first_rx.recv())
        .await
        .expect("first session command arrives within deadline")
        .expect("first session receives command")
    {
        AcpSessionCommand::CancelCurrentPrompt { response } => response.send(Ok(())).unwrap(),
        _ => panic!("expected cancellation"),
    }
    tokio::time::timeout(Duration::from_secs(2), cancel)
        .await
        .expect("cancellation completes within deadline")
        .unwrap()
        .unwrap();
    assert!(matches!(second_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    assert_eq!(second.last_activity, second_activity);
    assert_eq!(second.snapshot.conversation_id, 2);
    assert_eq!(second.run_id, "run-2");
    assert!(!first.handle.sender.same_channel(&second.handle.sender));
}

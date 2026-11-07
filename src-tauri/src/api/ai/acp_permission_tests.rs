use super::*;
use tokio::sync::oneshot::error::TryRecvError;

async fn pending(state: &AcpPermissionState, id: &str, conversation: Option<i64>) -> oneshot::Receiver<AcpPermissionDecision> {
    let (tx, rx) = oneshot::channel();
    state.store_request(AcpPermissionRequestEvent {
        request_id: id.into(), conversation_id: conversation, agent_kind: Some("acp".into()),
        tool_call_id: format!("tool-{id}"), title: None, kind: None, parameters: None,
        options: vec![AcpPermissionOptionPayload { option_id: "deny".into(), name: "Deny".into(), kind: "reject_once".into() }],
    }, tx).await;
    rx
}

#[tokio::test]
async fn test_permission_cancel_isolates_conversations_and_unscoped_requests() {
    let state = AcpPermissionState::new();
    let a = pending(&state, "a", Some(1)).await;
    let a2 = pending(&state, "a2", Some(1)).await;
    let mut b = pending(&state, "b", Some(2)).await;
    let mut unscoped = pending(&state, "unscoped", None).await;
    let mut resolved = state.cancel_requests_for_conversation(1).await;
    resolved.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(resolved.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["a", "a2"]);
    assert!(resolved.iter().all(|r| r.1.delivered && r.1.conversation_id == Some(1)));
    assert!(matches!(a.await.unwrap(), AcpPermissionDecision::Cancelled));
    assert!(matches!(a2.await.unwrap(), AcpPermissionDecision::Cancelled));
    assert!(matches!(b.try_recv(), Err(TryRecvError::Empty)));
    assert!(matches!(unscoped.try_recv(), Err(TryRecvError::Empty)));
    assert!(!state.has_pending_permission_for_conversation(1).await);
    assert!(state.has_pending_permission_for_conversation(2).await);
    assert!(state.cancel_requests_for_conversation(1).await.is_empty());
    // A new turn may register a fresh request without inheriting cancellation.
    let mut next = pending(&state, "next", Some(1)).await;
    assert!(matches!(next.try_recv(), Err(TryRecvError::Empty)));
    assert!(state.resolve_request("a", AcpPermissionDecision::Selected("allow".into())).await.unwrap().is_none());
    assert!(matches!(next.try_recv(), Err(TryRecvError::Empty)));
    state.resolve_request("next", AcpPermissionDecision::Selected("deny".into())).await.unwrap().expect("request should resolve");
    assert!(matches!(next.await.unwrap(), AcpPermissionDecision::Selected(id) if id == "deny"));
}

#[tokio::test]
async fn test_permission_unknown_resolution_does_not_approve_pending_request() {
    let state = AcpPermissionState::new();
    let mut rx = pending(&state, "known", Some(1)).await;
    assert!(state.resolve_request("unknown", AcpPermissionDecision::Selected("allow".into())).await.unwrap().is_none());
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    assert!(state.get_request("known").await.is_some());
    state.resolve_request("known", AcpPermissionDecision::Selected("deny".into())).await.unwrap().expect("request should resolve");
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(id) if id == "deny"));
    assert!(state.resolve_request("known", AcpPermissionDecision::Selected("allow".into())).await.unwrap().is_none());
}

#[tokio::test]
async fn test_permission_cancel_cleans_disconnected_waiter_without_affecting_peer() {
    let state = AcpPermissionState::new();
    drop(pending(&state, "disconnected", Some(1)).await);
    let mut peer = pending(&state, "peer", Some(2)).await;
    let resolutions = state.cancel_requests_for_conversation(1).await;
    assert_eq!(resolutions.len(), 1);
    assert!(!resolutions[0].1.delivered);
    assert!(state.get_request("disconnected").await.is_none());
    assert!(matches!(peer.try_recv(), Err(TryRecvError::Empty)));
    state.resolve_request("peer", AcpPermissionDecision::Cancelled).await.unwrap().expect("request should resolve");
    assert!(matches!(peer.await.unwrap(), AcpPermissionDecision::Cancelled));
}

#[tokio::test]
async fn test_permission_unknown_option_rejected_without_consuming_request() {
    let state = AcpPermissionState::new();
    let mut rx = pending(&state, "known", Some(1)).await;
    let result = state.resolve_request("known", AcpPermissionDecision::Selected("allow".into())).await;
    let error = result.err().expect("unadvertised option must fail");
    assert!(error.contains("allow") && error.contains("known") && error.contains("not advertised"));
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    assert!(state.get_request("known").await.is_some());
    state.resolve_request("known", AcpPermissionDecision::Selected("deny".into()))
        .await.unwrap().expect("valid denial still resolves");
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(id) if id == "deny"));
    assert!(state.resolve_request("known", AcpPermissionDecision::Selected("deny".into()))
        .await.unwrap().is_none());
}

#[tokio::test]
async fn test_permission_advertised_extension_option_is_opaque_and_accepted() {
    let state = AcpPermissionState::new();
    let (tx, rx) = oneshot::channel();
    let id = "vendor:approval/one-use-v2";
    state.store_request(AcpPermissionRequestEvent {
        request_id: "extension".into(), conversation_id: Some(1), agent_kind: Some("acp".into()),
        tool_call_id: "tool-extension".into(), title: None, kind: None, parameters: None,
        options: vec![AcpPermissionOptionPayload { option_id: id.into(), name: "Vendor option".into(), kind: "allow_once".into() }],
    }, tx).await;
    assert!(state.resolve_request("extension", AcpPermissionDecision::Selected(id.into()))
        .await.unwrap().unwrap().delivered);
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(selected) if selected == id));
}

#[tokio::test]
async fn test_permission_invalid_option_keeps_request_cancellable() {
    let state = AcpPermissionState::new();
    let rx = pending(&state, "cancel-after-invalid", Some(1)).await;
    assert!(state.resolve_request("cancel-after-invalid", AcpPermissionDecision::Selected("unknown".into()))
        .await.is_err());
    let resolutions = state.cancel_requests_for_conversation(1).await;
    assert_eq!(resolutions.len(), 1);
    assert!(resolutions[0].1.delivered);
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Cancelled));
    assert!(state.get_request("cancel-after-invalid").await.is_none());
}

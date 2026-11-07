use super::*;

#[test]
fn test_codex_command_and_file_approvals_preserve_supported_native_variants() {
    for method in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval"] {
        for decision in ["accept", "acceptForSession", "decline", "cancel"] {
            assert_eq!(
                codex_approval_response(method, &json!({}), AcpPermissionDecision::Selected(decision.into())),
                json!({"decision": decision}),
                "{method}: {decision}",
            );
        }
        assert_eq!(
            codex_approval_response(method, &json!({}), AcpPermissionDecision::Cancelled),
            json!({"decision": "cancel"}),
        );
    }
}

#[test]
fn test_codex_command_and_file_approvals_cancel_unknown_and_unsupported_extensions() {
    for method in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval"] {
        for decision in [
            "", "approved", "acceptAlways", "Accept", "accept ", "futureApproval",
            "acceptWithExecpolicyAmendment", "applyNetworkPolicyAmendment",
            r#"{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["git"]}}"#,
            r#"{"applyNetworkPolicyAmendment":{"network_policy_amendment":{"host":"example.com","action":"allow"}}}"#,
        ] {
            // Even an advertised unknown string cannot bypass native validation.
            let params = json!({"availableDecisions": [decision]});
            assert_eq!(
                codex_approval_response(method, &params, AcpPermissionDecision::Selected(decision.into())),
                json!({"decision": "cancel"}),
                "{method}: {decision}",
            );
        }
    }
}

#[test]
fn test_codex_command_structured_extensions_do_not_replace_supported_string_options() {
    let params = json!({"availableDecisions": [
        "accept", "acceptForSession",
        {"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["git"]}},
        {"applyNetworkPolicyAmendment": {"network_policy_amendment": {"host": "example.com", "action": "allow"}}},
        "decline", "cancel"
    ]});
    let options = codex_permission_options("item/commandExecution/requestApproval", &params);
    assert_eq!(options.iter().map(|option| option.option_id.as_str()).collect::<Vec<_>>(),
        vec!["accept", "acceptForSession", "decline", "cancel"]);
}

#[test]
fn test_codex_permissions_approval_preserves_scope_and_denies_unknown_decisions() {
    let method = "item/permissions/requestApproval";
    let permissions = json!({"network": {"enabled": true}});
    let params = json!({"permissions": permissions});
    for (decision, scope) in [("accept", "turn"), ("acceptForSession", "session")] {
        assert_eq!(
            codex_approval_response(method, &params, AcpPermissionDecision::Selected(decision.into())),
            json!({"permissions": permissions, "scope": scope}),
        );
    }
    for decision in ["decline", "cancel", "acceptAlways", "futureApproval", "acceptWithExecpolicyAmendment"] {
        assert_eq!(
            codex_approval_response(method, &params, AcpPermissionDecision::Selected(decision.into())),
            json!({"permissions": {}}),
        );
    }
    assert_eq!(codex_approval_response(method, &params, AcpPermissionDecision::Cancelled), json!({"permissions": {}}));
}

#[test]
fn test_codex_unknown_approval_method_cannot_grant_permission() {
    assert_eq!(
        codex_approval_response("item/unknown/requestApproval", &json!({}), AcpPermissionDecision::Selected("accept".into())),
        json!({"decision": "cancel"}),
    );
}

async fn pending_native_permission(
    state: &AcpPermissionState,
    request_id: &str,
    conversation_id: i64,
) -> oneshot::Receiver<AcpPermissionDecision> {
    let (tx, rx) = oneshot::channel();
    state.store_request(AcpPermissionRequestEvent {
        request_id: request_id.into(), conversation_id: Some(conversation_id),
        agent_kind: Some(CODEX_APP_SERVER_API_TYPE.into()), tool_call_id: "command".into(),
        title: None, kind: Some("item/commandExecution/requestApproval".into()), parameters: None,
        options: codex_permission_options("item/commandExecution/requestApproval", &json!({})),
    }, tx).await;
    rx
}

/// Uses the unified facade and the same pending-decision wait as the native actor.
/// The actor cannot receive Cancel until its permission wait finishes.
#[tokio::test]
async fn test_codex_unified_cancel_unblocks_permission_wait_without_cancelling_peer() {
    use crate::api::ai::agent_driver::session::AgentSessionHandle;
    let state = AcpPermissionState::new();
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let cancellation = watch::channel(0).0;
    let context = CodexRequestCancellation { source_message_id: None, instance_id: None, signal: cancellation.clone(), generation: 0, run_id: "old-run".into() };
    let native = CodexSessionHandle {
        sender, cancellation, run_id: context.run_id.clone(),
        failure_context: std::sync::Arc::new(std::sync::Mutex::new(None)),
    };
    let run_id = native.run_id().to_string();
    let handle = AgentSessionHandle::new(native, run_id);
    let request_id = "codex:41:old-run:9";
    let rx = pending_native_permission(&state, request_id, 41).await;
    let mut replacement_rx = pending_native_permission(&state, "codex:41:new-run:9", 41).await;
    let mut peer_rx = pending_native_permission(&state, "codex:42:peer-run:9", 42).await;
    let result = {
    let actor = async {
        let (decision, cancelled) = wait_codex_approval_decision(&state, request_id, rx, Some(&context)).await;
        assert!(cancelled);
        assert!(matches!(decision, AcpPermissionDecision::Cancelled));
        assert_eq!(codex_approval_response("item/commandExecution/requestApproval", &json!({}), decision), json!({"decision":"cancel"}));
        let CodexSessionCommand::CancelCurrentPrompt { response } = receiver.recv().await.unwrap() else { panic!("expected original cancel command after permission wait") };
        response.send(Ok(())).unwrap();
    };
    tokio::pin!(actor);
    assert!(futures::poll!(&mut actor).is_pending());
    let (result, ()) = tokio::time::timeout(Duration::from_millis(500), async {
        tokio::join!(handle.cancel_current_prompt(), &mut actor)
    }).await.expect("unified cancel must unblock the production permission wait");
    result
    };
    result.unwrap();
    assert!(state.get_request(request_id).await.is_none());
    assert!(state.resolve_request(request_id, AcpPermissionDecision::Selected("accept".into())).await.unwrap().is_none());
    assert!(matches!(replacement_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    assert!(matches!(peer_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    assert!(receiver.try_recv().is_err(), "cancel must not duplicate or replace commands");
}

#[tokio::test]
async fn test_codex_permission_cancellation_is_latched_before_subscribe_and_beats_late_allow() {
    let state = AcpPermissionState::new();
    let cancellation = watch::channel(0).0;
    let context = CodexRequestCancellation { source_message_id: None, instance_id: None, signal: cancellation.clone(), generation: 0, run_id: "run".into() };
    for allow_already_ready in [false, true] {
        let request_id = format!("codex:41:run:{allow_already_ready}");
        let rx = pending_native_permission(&state, &request_id, 41).await;
        // Cancelling before wait subscribes must still cancel this prompt.
        cancellation.send_modify(|generation| *generation += 1);
        if allow_already_ready {
            state.resolve_request(&request_id, AcpPermissionDecision::Selected("accept".into())).await.unwrap().unwrap();
        }
        let (decision, cancelled) = tokio::time::timeout(Duration::from_millis(500),
            wait_codex_approval_decision(&state, &request_id, rx, Some(&context)),
        ).await.expect("pre-subscription cancellation must not be lost");
        assert!(cancelled);
        assert!(matches!(decision, AcpPermissionDecision::Cancelled));
        assert!(state.get_request(&request_id).await.is_none());
    }
}

#[tokio::test]
async fn test_codex_permission_new_prompt_generation_preserves_normal_decisions_after_idle_cancel() {
    let state = AcpPermissionState::new();
    let cancellation = watch::channel(0).0;
    // An earlier idle cancel is part of the new prompt's enqueue-time baseline.
    cancellation.send_modify(|generation| *generation += 1);
    let context = CodexRequestCancellation { source_message_id: None, instance_id: None,
        signal: cancellation.clone(), generation: *cancellation.borrow(), run_id: "run".into(),
    };
    for option in ["accept", "acceptForSession", "decline", "cancel"] {
        let request_id = format!("codex:41:run:{option}");
        let rx = pending_native_permission(&state, &request_id, 41).await;
        let wait = wait_codex_approval_decision(&state, &request_id, rx, Some(&context));
        tokio::pin!(wait);
        // Poll the production waiter once: no stale cancellation or fabricated answer.
        assert!(futures::poll!(&mut wait).is_pending());
        state.resolve_request(&request_id, AcpPermissionDecision::Selected(option.into())).await.unwrap().unwrap();
        let (decision, cancelled) = tokio::time::timeout(Duration::from_millis(500), &mut wait).await.unwrap();
        assert!(!cancelled);
        assert!(matches!(decision, AcpPermissionDecision::Selected(selected) if selected == option));
        assert!(state.get_request(&request_id).await.is_none());
    }
}

/// The production AskUserQuestion route uses this cancellable interaction wait.
/// A dropped real request has its existing PendingInteractionCleanup guard;
/// this headless test verifies future drop, not desktop/Feishu event delivery.
#[tokio::test]
async fn test_codex_user_input_wait_cancels_and_drops_only_its_pending_future() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    struct DropMarker(Arc<AtomicBool>);
    impl Drop for DropMarker {
        fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let peer_dropped = Arc::new(AtomicBool::new(false));
    let _peer = DropMarker(peer_dropped.clone());
    let cancellation = watch::channel(0).0;
    let context = CodexRequestCancellation { source_message_id: None, instance_id: None, signal: cancellation.clone(), generation: 0, run_id: "run".into() };
    let interaction = async {
        let _cleanup = DropMarker(dropped.clone());
        std::future::pending::<Result<HashMap<String, String>, String>>().await
    };
    let wait = wait_codex_interaction(interaction, Some(&context));
    tokio::pin!(wait);
    assert!(futures::poll!(&mut wait).is_pending());
    cancellation.send_modify(|generation| *generation += 1);
    let error = tokio::time::timeout(Duration::from_millis(500), &mut wait).await.unwrap().unwrap_err();
    assert!(error.contains("cancelled by Stop"));
    assert!(dropped.load(Ordering::SeqCst));
    assert!(!peer_dropped.load(Ordering::SeqCst));
}

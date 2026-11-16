use super::*;

fn approval_frame() -> Value {
    json!({"id": 7, "method": METHOD, "params": {
        "threadId": "thread", "turnId": "turn", "serverName": "aipp",
        "mode": "form", "message": "Allow aipp_t237_butler_memory?",
        "_meta": {"codex_approval_kind": "mcp_tool_call"},
        "requestedSchema": {"type": "object", "properties": {}}
    }})
}

#[test]
fn test_codex_mcp_approval_routes_to_guarded_permissions_and_native_response() {
    let frame = approval_frame();
    assert!(is_tool_approval(&frame).unwrap());
    let options = codex_permission_options(METHOD, &frame["params"]);
    assert_eq!(options.iter().map(|o| o.option_id.as_str()).collect::<Vec<_>>(), ["accept", "decline", "cancel"]);
    assert_eq!(options[0].kind, "allow_once");
    for action in ["accept", "decline", "cancel"] {
        assert_eq!(codex_approval_response(METHOD, &frame["params"], AcpPermissionDecision::Selected(action.into())),
            json!({"action": action, "content": null}));
    }
    for unknown in ["acceptForSession", "acceptAlways", "allow", ""] {
        assert_eq!(approval_response(AcpPermissionDecision::Selected(unknown.into()))["action"], "cancel");
    }
    assert_eq!(approval_response(AcpPermissionDecision::Cancelled)["action"], "cancel");
}

#[test]
fn test_codex_elicitation_distinguishes_forms_and_reports_unsupported_modes() {
    let mut frame = approval_frame();
    frame["params"]["_meta"] = Value::Null;
    assert!(!is_tool_approval(&frame).unwrap());
    for mode in ["url", "future-mode"] {
        frame["params"]["mode"] = json!(mode);
        let error = is_tool_approval(&frame).unwrap_err();
        assert!(error.contains(mode) && error.contains("thread") && error.contains("request=7"));
    }
    assert!(!is_tool_approval(&json!({"method": "item/tool/requestUserInput"})).unwrap());
}

#[test]
fn test_codex_form_preserves_typed_choices_and_native_answers() {
    let event = form_event(&json!({"message": "Pick", "requestedSchema": {"type": "object",
        "required": ["choice"], "properties": {
            "choice": {"type": "string", "oneOf": [{"const": "wire-value", "title": "Displayed label"}]},
            "many": {"type": "array", "items": {"anyOf": [{"const": "second", "title": "Second"}]}},
            "legacy": {"type": "string", "enum": ["old"], "enumNames": ["Old label"]}
        }}}), "request".into(), 42).unwrap();
    assert_eq!(event.schema["properties"]["choice"]["oneOf"][0]["value"], "wire-value");
    assert_eq!(event.schema["properties"]["many"]["items"]["anyOf"][0]["value"], "second");
    assert_eq!(event.schema["properties"]["legacy"]["oneOf"][0]["title"], "Old label");
    assert_eq!(event.schema["required"], json!(["choice"]));
    let content = [
        ("choice".into(), crate::api::ai::acp::json_to_acp_elicitation_value(json!("wire-value")).unwrap()),
        ("enabled".into(), crate::api::ai::acp::json_to_acp_elicitation_value(json!(true)).unwrap()),
    ].into_iter().collect();
    assert_eq!(form_response(AcpElicitationDecision::Accepted(content)),
        json!({"action":"accept", "content":{"choice":"wire-value", "enabled":true}}));
    assert_eq!(form_response(AcpElicitationDecision::Declined)["action"], "decline");
    assert!(form_event(&json!({"message":"missing schema"}), "bad".into(), 42).is_err());
}

#[tokio::test]
async fn test_codex_elicitation_stop_cancels_exact_form_and_keeps_peer() {
    let state = AcpElicitationState::new();
    let (sender, receiver) = oneshot::channel();
    let (peer_sender, mut peer_receiver) = oneshot::channel();
    state.store_request("current".into(), Some(42), sender).await;
    state.store_request("peer".into(), Some(43), peer_sender).await;
    let signal = watch::channel(0).0;
    let cancellation = CodexRequestCancellation {
        source_message_id: None, instance_id: Some(1), signal: signal.clone(), generation: 0, run_id: "run".into(),
    };
    signal.send_replace(1);
    let (response, resolution) = tokio::time::timeout(Duration::from_secs(1),
        wait_form(&state, "current", receiver, Some(&cancellation))).await.unwrap();
    assert_eq!(response["action"], "cancel");
    assert_eq!(resolution.unwrap().conversation_id, Some(42));
    assert!(!state.has_pending_request_for_conversation(42).await);
    assert!(state.has_pending_request_for_conversation(43).await);
    assert!(matches!(peer_receiver.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
}

#[tokio::test]
async fn test_codex_elicitation_closed_channel_cancels_instead_of_approving() {
    let state = AcpElicitationState::new();
    let (sender, receiver) = oneshot::channel();
    drop(sender);
    let (response, _) = wait_form(&state, "closed", receiver, None).await;
    assert_eq!(response["action"], "cancel");
}

use super::*;

fn form(id:&str,conversation:i64)->AcpElicitationRequestEvent {
    AcpElicitationRequestEvent {request_id:id.into(),conversation_id:Some(conversation),agent_kind:"acp".into(),
        message:"填写要求".into(),schema:serde_json::json!({"type":"object","properties":{"answer":{"type":"string"}}})}
}

#[tokio::test]
async fn test_group_elicitation_recovery_is_scoped_and_resolution_removes_form() {
    let state=AcpElicitationState::new();
    let (sender,receiver)=oneshot::channel();
    state.store_request("question".into(),Some(200),sender).await;
    assert!(state.attach_form(form("question",300)).await.is_err());
    state.attach_form(form("question",200)).await.unwrap();
    assert!(state.attach_form(form("question",200)).await.is_err());
    assert!(state.list_forms(300).await.is_empty());
    assert_eq!(state.list_forms(200).await[0].request_id,"question");
    assert!(state.has_pending_request_for_conversation(200).await);
    assert!(state.resolve_request("question",AcpElicitationDecision::Declined).await.unwrap().delivered);
    assert!(matches!(receiver.await.unwrap(),AcpElicitationDecision::Declined));
    assert!(state.list_forms(200).await.is_empty());
    assert!(!state.has_pending_request_for_conversation(200).await);
    assert!(state.attach_form(form("question",200)).await.is_err());
}

#[tokio::test]
async fn test_group_elicitation_cancel_keeps_other_execution_pending() {
    let state=AcpElicitationState::new();
    let (first,first_reply)=oneshot::channel();
    let (second,second_reply)=oneshot::channel();
    state.store_request("first".into(),Some(200),first).await;
    state.store_request("second".into(),Some(201),second).await;
    state.attach_form(form("first",200)).await.unwrap();
    state.attach_form(form("second",201)).await.unwrap();
    state.cancel_requests_for_conversation(200).await;
    assert!(matches!(first_reply.await.unwrap(),AcpElicitationDecision::Cancelled));
    assert!(state.list_forms(200).await.is_empty());
    assert_eq!(state.list_forms(201).await.len(),1);
    assert!(state.has_pending_request_for_conversation(201).await);
    state.remove_request("second").await;
    assert!(second_reply.await.is_err());
    assert!(state.list_forms(201).await.is_empty());
}

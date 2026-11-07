//! Exercises the production facade and authoritative pending stores. These are
//! not desktop window, live-CLI, or complete native-session lifecycle tests.
use super::*;
use crate::api::ai::acp::{AcpPermissionOptionPayload, AcpPermissionRequestEvent};
use crate::mcp::builtin_mcp::interaction::{
    AskUserQuestionItem, AskUserQuestionRequestEvent,
};
use tokio::sync::oneshot;

struct Fixture {
    permissions: AcpPermissionState,
    elicitations: AcpElicitationState,
    questions: InteractionState,
}

impl Fixture {
    fn new() -> Self {
        Self { permissions: AcpPermissionState::new(), elicitations: AcpElicitationState::new(),
            questions: InteractionState::new() }
    }
    async fn reply(&self, target: &AgentReplyTarget, decision: AgentInteractionReply)
        -> Result<AgentInteractionResolution, String>
    {
        resolve_reply(AgentInteractionStores {
            permissions: &self.permissions, elicitations: &self.elicitations, questions: &self.questions,
        }, target.clone(), decision).await
    }
    async fn permission(&self, target: &AgentReplyTarget) -> oneshot::Receiver<AcpPermissionDecision> {
        let (tx, rx) = oneshot::channel();
        self.permissions.store_request_scoped(permission_event(target), tx, target.scope.clone())
            .await.unwrap();
        rx
    }
    async fn elicitation(&self, target: &AgentReplyTarget) -> oneshot::Receiver<AcpElicitationDecision> {
        let (tx, rx) = oneshot::channel();
        self.elicitations.store_request_scoped(target.request_id.clone(), target.scope.conversation_id,
            tx, target.scope.clone()).await.unwrap();
        rx
    }
    async fn question(&self, target: &AgentReplyTarget)
        -> oneshot::Receiver<Result<HashMap<String, String>, String>>
    {
        let (tx, rx) = oneshot::channel();
        self.questions.store_ask_user_request_scoped(question_event(target), tx, target.scope.clone())
            .await.unwrap();
        rx
    }
}

fn target(id: &str) -> AgentReplyTarget {
    AgentReplyTarget { request_id: id.into(),
        scope: AgentRequestScope::native("acp", Some(13), 42, "native-run-7") }
}

fn permission_event(target: &AgentReplyTarget) -> AcpPermissionRequestEvent {
    AcpPermissionRequestEvent {
        request_id: target.request_id.clone(), conversation_id: target.scope.conversation_id,
        agent_kind: target.scope.driver_id.clone(), tool_call_id: "native-tool-2".into(),
        title: None, kind: None, parameters: None,
        options: vec![AcpPermissionOptionPayload {
            option_id: " vendor:approve/一次 ".into(), name: "Allow once".into(), kind: "allow_once".into(),
        }, AcpPermissionOptionPayload {
            option_id: "vendor:deny".into(), name: "Deny".into(), kind: "reject_once".into(),
        }],
    }
}

fn question_event(target: &AgentReplyTarget) -> AskUserQuestionRequestEvent {
    AskUserQuestionRequestEvent {
        request_id: target.request_id.clone(), conversation_id: target.scope.conversation_id,
        questions: vec![AskUserQuestionItem {
            question: "Which plan?".into(), header: "Plan".into(), options: vec![],
            multi_select: false, is_secret: false,
        }], metadata: None,
    }
}

fn select(option: &str) -> AgentInteractionReply {
    AgentInteractionReply::Permission(AgentPermissionReply::Select { option_id: option.into() })
}

fn answer() -> AgentInteractionReply {
    AgentInteractionReply::AskUserQuestion(AgentQuestionReply::Answer {
        answers: HashMap::from([("Which plan?".into(), "Other: a custom plan".into())]),
    })
}

fn mismatches(base: &AgentReplyTarget) -> Vec<(&'static str, AgentReplyTarget)> {
    let mut cases = Vec::new();
    let mut changed = base.clone(); changed.scope.driver_id = Some("other_driver".into());
    cases.push(("driver", changed));
    let mut changed = base.clone(); changed.scope.instance_id = Some(99);
    cases.push(("instance", changed));
    let mut changed = base.clone(); changed.scope.conversation_id = Some(43);
    cases.push(("conversation", changed));
    let mut changed = base.clone(); changed.scope.run_id = Some("replacement-run".into());
    cases.push(("run", changed));
    let mut changed = base.clone(); changed.scope.instance_id = None;
    cases.push(("instance", changed));
    let mut changed = base.clone(); changed.scope.run_id = None;
    cases.push(("run", changed));
    cases
}

/// Every driver, including a future registered driver, uses advertised opaque IDs.
#[tokio::test]
async fn test_shared_permission_reply_routes_all_drivers_without_whitelist() {
    let fixture = Fixture::new();
    for driver in ["acp", "codex_app_server", "claude_sdk", "custom_driver"] {
        let mut request = target(driver);
        request.scope.driver_id = Some(driver.into());
        let rx = fixture.permission(&request).await;
        let resolution = fixture.reply(&request, select(" vendor:approve/一次 ")).await.unwrap();
        assert!(resolution.delivered);
        assert_eq!(resolution.scope, request.scope);
        assert_eq!(resolution.kind, AgentInteractionKind::Permission);
        assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(value) if value == " vendor:approve/一次 "));
    }
}

/// Identity failures cannot consume the pending approval or wake its native waiter.
#[tokio::test]
async fn test_shared_permission_reply_rejects_each_scope_mismatch() {
    let fixture = Fixture::new(); let request = target("scope");
    let mut rx = fixture.permission(&request).await;
    for (field, wrong) in mismatches(&request) {
        let error = fixture.reply(&wrong, select("vendor:deny")).await.unwrap_err();
        assert!(error.contains(field), "{error}");
        assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
        assert!(fixture.permissions.get_request(&request.request_id).await.is_some());
    }
    fixture.reply(&request, select("vendor:deny")).await.unwrap();
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(value) if value == "vendor:deny"));
}

#[tokio::test]
async fn test_shared_permission_reply_rejects_unadvertised_and_normalized_options() {
    let fixture = Fixture::new(); let request = target("options");
    let mut rx = fixture.permission(&request).await;
    for wrong in ["allow", "acceptForSession", "vendor:approve/一次", ""] {
        assert!(fixture.reply(&request, select(wrong)).await.unwrap_err().contains("not advertised"));
        assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    }
    fixture.reply(&request, select(" vendor:approve/一次 ")).await.unwrap();
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(_)));
}

#[tokio::test]
async fn test_shared_permission_reply_duplicate_registration_unknown_and_late_reply() {
    let fixture = Fixture::new(); let request = target("duplicate");
    let rx = fixture.permission(&request).await;
    let (replacement_tx, replacement_rx) = oneshot::channel();
    assert!(fixture.permissions.store_request_scoped(permission_event(&request), replacement_tx,
        request.scope.clone()).await.unwrap_err().contains("already pending"));
    assert!(replacement_rx.await.is_err());
    let mut wrong_id = request.clone(); wrong_id.request_id = "not-pending".into();
    assert!(fixture.reply(&wrong_id, select("vendor:deny")).await.unwrap_err().contains("not found"));
    fixture.reply(&request, AgentInteractionReply::Permission(AgentPermissionReply::Cancel)).await.unwrap();
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Cancelled));
    assert!(fixture.reply(&request, select("vendor:deny")).await.unwrap_err().contains("already resolved"));
}

/// Cleanup must match provider, conversation and original run, not merely driver.
#[tokio::test]
async fn test_shared_permission_cleanup_isolates_replacement_run_instance_and_peer() {
    let fixture = Fixture::new(); let old = target("old");
    let mut replacement = target("new"); replacement.scope.run_id = Some("replacement".into());
    let mut instance = target("instance"); instance.scope.instance_id = Some(14);
    let mut peer = target("peer"); peer.scope.conversation_id = Some(43);
    let old_rx = fixture.permission(&old).await;
    let mut new_rx = fixture.permission(&replacement).await;
    let mut instance_rx = fixture.permission(&instance).await;
    let mut peer_rx = fixture.permission(&peer).await;
    let cancelled = fixture.permissions.cancel_requests_for_scope(&old.scope).await;
    assert_eq!(cancelled.len(), 1); assert_eq!(cancelled[0].0, "old");
    assert!(matches!(old_rx.await.unwrap(), AcpPermissionDecision::Cancelled));
    for rx in [&mut new_rx, &mut instance_rx, &mut peer_rx] {
        assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    }
    assert!(fixture.permissions.cancel_requests_for_scope(&old.scope).await.is_empty());
    assert!(fixture.reply(&old, select("vendor:deny")).await.is_err());
    for request in [&replacement, &instance, &peer] { fixture.reply(request, select("vendor:deny")).await.unwrap(); }
}

/// A raced answer and Stop have exactly one consumer; neither can affect a peer.
#[tokio::test]
async fn test_shared_permission_reply_cancel_race_has_one_winner() {
    for reply_first in [false, true] {
        let fixture = Fixture::new(); let request = target("race");
        let rx = fixture.permission(&request).await;
        let mut peer = target("peer"); peer.scope.conversation_id = Some(99);
        let mut peer_rx = fixture.permission(&peer).await;
        let (resolution, cancelled) = if reply_first {
            tokio::join!(biased;
                fixture.reply(&request, select("vendor:deny")),
                fixture.permissions.cancel_requests_for_scope(&request.scope))
        } else {
            let (cancelled, resolution) = tokio::join!(biased;
                fixture.permissions.cancel_requests_for_scope(&request.scope),
                fixture.reply(&request, select("vendor:deny")));
            (resolution, cancelled)
        };
        assert_eq!(usize::from(resolution.is_ok()) + cancelled.len(), 1);
        let decision = rx.await.unwrap();
        assert!(matches!(decision, AcpPermissionDecision::Selected(_) | AcpPermissionDecision::Cancelled));
        assert!(fixture.permissions.request_scope(&request.request_id).await.is_none());
        assert!(matches!(peer_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    }
}

#[tokio::test]
async fn test_shared_permission_reply_reports_dropped_receiver_without_retry() {
    let fixture = Fixture::new(); let request = target("dropped");
    drop(fixture.permission(&request).await);
    assert!(!fixture.reply(&request, select("vendor:deny")).await.unwrap().delivered);
    assert!(fixture.reply(&request, select("vendor:deny")).await.unwrap_err().contains("already resolved"));
}

#[tokio::test]
async fn test_shared_elicitation_reply_scopes_and_value_validation_preserve_pending() {
    let fixture = Fixture::new(); let request = target("elicitation");
    let mut rx = fixture.elicitation(&request).await;
    for (field, wrong) in mismatches(&request) {
        assert!(fixture.reply(&wrong, AgentInteractionReply::Elicitation(AgentElicitationReply::Decline))
            .await.unwrap_err().contains(field));
    }
    assert!(fixture.reply(&request, AgentInteractionReply::Elicitation(AgentElicitationReply::Accept {
        values: HashMap::from([("unsupported".into(), serde_json::json!({"nested": true}))]),
    })).await.unwrap_err().contains("invalid"));
    assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    let resolution = fixture.reply(&request, AgentInteractionReply::Elicitation(AgentElicitationReply::Accept {
        values: HashMap::from([("choice".into(), serde_json::json!(["a", "b"])),
            ("enabled".into(), serde_json::json!(true))]),
    })).await.unwrap();
    assert!(resolution.delivered);
    assert!(matches!(rx.await.unwrap(), AcpElicitationDecision::Accepted(values) if values.len() == 2));
    assert!(fixture.reply(&request, AgentInteractionReply::Elicitation(AgentElicitationReply::Cancel)).await.is_err());
}

#[tokio::test]
async fn test_shared_question_reply_scopes_empty_answers_and_atomic_event_cleanup() {
    let fixture = Fixture::new(); let request = target("question");
    let mut rx = fixture.question(&request).await;
    for (field, wrong) in mismatches(&request) {
        assert!(fixture.reply(&wrong, answer()).await.unwrap_err().contains(field));
    }
    assert!(fixture.reply(&request, AgentInteractionReply::AskUserQuestion(AgentQuestionReply::Answer {
        answers: HashMap::new(),
    })).await.unwrap_err().contains("cannot be empty"));
    assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
    assert_eq!(fixture.questions.list_requests_for_conversation(42).await.len(), 1);
    fixture.reply(&request, answer()).await.unwrap();
    assert_eq!(rx.await.unwrap().unwrap()["Which plan?"], "Other: a custom plan");
    assert!(fixture.questions.get_ask_user_request("question").await.is_none());
    assert!(fixture.questions.list_requests_for_conversation(42).await.is_empty());
    assert!(fixture.reply(&request, answer()).await.unwrap_err().contains("already resolved"));
}

/// Ordinary MCP questions have no driver identity and keep free-form answers.
#[tokio::test]
async fn test_shared_question_reply_preserves_unscoped_mcp_compatibility() {
    let fixture = Fixture::new();
    let request = AgentReplyTarget { request_id: "ordinary-mcp".into(),
        scope: AgentRequestScope::application(None) };
    let (tx, rx) = oneshot::channel();
    let event = question_event(&request);
    let old_payload = serde_json::to_value(&event).unwrap();
    fixture.questions.store_ask_user_request(event, tx).await;
    assert_eq!(fixture.questions.ask_user_request_scope(&request.request_id).await, Some(request.scope.clone()));
    assert_eq!(serde_json::to_value(fixture.questions.get_ask_user_request(&request.request_id).await.unwrap()).unwrap(), old_payload);
    assert!(old_payload.get("scope").is_none());
    fixture.reply(&request, answer()).await.unwrap();
    assert!(rx.await.unwrap().is_ok());
}

#[tokio::test]
async fn test_shared_question_and_elicitation_cleanup_isolates_replacement_and_peer() {
    let fixture = Fixture::new(); let old = target("old");
    let mut new = target("new"); new.scope.run_id = Some("new-run".into());
    let mut peer = target("peer"); peer.scope.conversation_id = Some(77);
    let old_q = fixture.question(&old).await; let old_e = fixture.elicitation(&old).await;
    let mut new_q = fixture.question(&new).await; let mut new_e = fixture.elicitation(&new).await;
    let mut peer_q = fixture.question(&peer).await; let mut peer_e = fixture.elicitation(&peer).await;
    assert_eq!(fixture.questions.cancel_requests_for_scope(&old.scope).await.len(), 1);
    assert_eq!(fixture.elicitations.cancel_requests_for_scope(&old.scope).await.len(), 1);
    assert!(old_q.await.unwrap().unwrap_err().contains("cancelled"));
    assert!(matches!(old_e.await.unwrap(), AcpElicitationDecision::Cancelled));
    for rx in [&mut new_q, &mut peer_q] { assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty))); }
    for rx in [&mut new_e, &mut peer_e] { assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty))); }
    assert!(fixture.reply(&old, answer()).await.is_err());
    assert!(fixture.reply(&old, AgentInteractionReply::Elicitation(AgentElicitationReply::Decline)).await.is_err());
    assert_eq!(fixture.questions.list_requests_for_conversation(42).await.len(), 1);
}

#[tokio::test]
async fn test_shared_registration_rejects_mismatched_creation_identity() {
    let fixture = Fixture::new(); let request = target("creation");
    let mut event = permission_event(&request); event.agent_kind = Some("wrong".into());
    let (tx, rx) = oneshot::channel();
    assert!(fixture.permissions.store_request_scoped(event, tx, request.scope.clone()).await.unwrap_err().contains("driver"));
    assert!(rx.await.is_err());
    let (tx, rx) = oneshot::channel();
    assert!(fixture.elicitations.store_request_scoped(request.request_id.clone(), Some(0), tx,
        request.scope.clone()).await.unwrap_err().contains("conversation"));
    assert!(rx.await.is_err());
    let (tx, rx) = oneshot::channel(); let mut event = question_event(&request); event.conversation_id = None;
    assert!(fixture.questions.store_ask_user_request_scoped(event, tx, request.scope).await.unwrap_err().contains("conversation"));
    assert!(rx.await.is_err());
}

#[tokio::test]
async fn test_shared_question_reply_cancel_race_removes_event_and_sender_together() {
    for reply_first in [false, true] {
        let fixture = Fixture::new(); let request = target("question-race");
        let rx = fixture.question(&request).await;
        let (resolution, cancelled) = if reply_first {
            tokio::join!(biased; fixture.reply(&request, answer()),
                fixture.questions.cancel_requests_for_scope(&request.scope))
        } else {
            let (cancelled, resolution) = tokio::join!(biased;
                fixture.questions.cancel_requests_for_scope(&request.scope),
                fixture.reply(&request, answer()));
            (resolution, cancelled)
        };
        assert_eq!(usize::from(resolution.is_ok()) + cancelled.len(), 1);
        assert_eq!(rx.await.unwrap().is_ok(), reply_first);
        assert!(fixture.questions.get_ask_user_request(&request.request_id).await.is_none());
        assert!(fixture.questions.ask_user_request_scope(&request.request_id).await.is_none());
        assert!(fixture.reply(&request, answer()).await.is_err());
    }
}

#[tokio::test]
async fn test_shared_elicitation_reply_cancel_race_has_one_winner() {
    for reply_first in [false, true] {
        let fixture = Fixture::new(); let request = target("elicitation-race");
        let rx = fixture.elicitation(&request).await;
        let reply = AgentInteractionReply::Elicitation(AgentElicitationReply::Decline);
        let (resolution, cancelled) = if reply_first {
            tokio::join!(biased; fixture.reply(&request, reply),
                fixture.elicitations.cancel_requests_for_scope(&request.scope))
        } else {
            let (cancelled, resolution) = tokio::join!(biased;
                fixture.elicitations.cancel_requests_for_scope(&request.scope),
                fixture.reply(&request, reply));
            (resolution, cancelled)
        };
        assert_eq!(usize::from(resolution.is_ok()) + cancelled.len(), 1);
        assert_eq!(matches!(rx.await.unwrap(), AcpElicitationDecision::Declined), reply_first);
        assert!(fixture.elicitations.request_scope(&request.request_id).await.is_none());
    }
}

#[tokio::test]
async fn test_shared_question_and_elicitation_duplicate_store_preserves_original_waiter() {
    let fixture = Fixture::new(); let request = target("duplicates");
    let original_q = fixture.question(&request).await;
    let original_e = fixture.elicitation(&request).await;
    let (tx, rx) = oneshot::channel();
    assert!(fixture.questions.store_ask_user_request_scoped(question_event(&request), tx,
        request.scope.clone()).await.unwrap_err().contains("already pending"));
    assert!(rx.await.is_err());
    let (tx, rx) = oneshot::channel();
    assert!(fixture.elicitations.store_request_scoped(request.request_id.clone(), Some(42), tx,
        request.scope.clone()).await.unwrap_err().contains("already pending"));
    assert!(rx.await.is_err());
    fixture.reply(&request, answer()).await.unwrap();
    fixture.reply(&request, AgentInteractionReply::Elicitation(AgentElicitationReply::Decline)).await.unwrap();
    assert!(original_q.await.unwrap().is_ok());
    assert!(matches!(original_e.await.unwrap(), AcpElicitationDecision::Declined));
}

#[tokio::test]
async fn test_shared_permission_legacy_scope_and_frontend_payload_stay_compatible() {
    let fixture = Fixture::new();
    let request = AgentReplyTarget { request_id: "legacy".into(),
        scope: AgentRequestScope::application(Some(42)) };
    let event = permission_event(&request);
    let old_payload = serde_json::to_value(&event).unwrap();
    let (tx, rx) = oneshot::channel();
    fixture.permissions.store_request(event, tx).await;
    assert_eq!(fixture.permissions.request_scope(&request.request_id).await, Some(request.scope.clone()));
    let snapshot = fixture.permissions.get_request(&request.request_id).await.unwrap();
    assert_eq!(serde_json::to_value(snapshot.event).unwrap(), old_payload);
    assert!(old_payload.get("scope").is_none());
    fixture.reply(&request, AgentInteractionReply::Permission(
        AgentPermissionReply::from_legacy(Some("vendor:deny".into()), false).unwrap(),
    )).await.unwrap();
    assert!(matches!(rx.await.unwrap(), AcpPermissionDecision::Selected(value) if value == "vendor:deny"));
    assert!(matches!(AgentPermissionReply::from_legacy(Some("ignored".into()), true).unwrap(), AgentPermissionReply::Cancel));
    assert!(AgentPermissionReply::from_legacy(None, false).is_err());
}

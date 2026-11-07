//! Wire-level tests of production client request dispatch and permission replies.
//! The SDK peers run over duplex channels, not the Tauri session actor. UI event
//! delivery, persistence, process teardown and AgentSessionHandle are not mocked
//! into an imitation actor and are deliberately outside these assertions.
use super::*;
use agent_client_protocol::{Channel, ConnectionTo, Responder};
use tokio::task::JoinHandle;

const DEADLINE: Duration = Duration::from_secs(3);

enum AgentAction {
    Permission(&'static str),
    Notification(&'static str),
    ReadFile,
}

type PermissionReply = (String, acp::Result<acp::RequestPermissionResponse>);

struct ProtocolFixture {
    actions: mpsc::UnboundedSender<AgentAction>,
    pending: mpsc::UnboundedReceiver<String>,
    replies: mpsc::UnboundedReceiver<PermissionReply>,
    notifications: mpsc::UnboundedReceiver<String>,
    file_replies: mpsc::UnboundedReceiver<acp::Result<acp::ReadTextFileResponse>>,
    stop: Option<oneshot::Sender<()>>,
    client: JoinHandle<acp::Result<()>>,
    agent: JoinHandle<acp::Result<()>>,
}

impl Drop for ProtocolFixture {
    fn drop(&mut self) {
        // Also clean up after a failed assertion or test timeout.
        self.client.abort();
        self.agent.abort();
    }
}

impl ProtocolFixture {
    fn new(state: Arc<AcpPermissionState>, conversation_id: i64) -> Self {
        let (client_channel, agent_channel) = Channel::duplex();
        let (pending_tx, pending) = mpsc::unbounded_channel();
        let (reply_tx, replies) = mpsc::unbounded_channel();
        let (notes_tx, notifications) = mpsc::unbounded_channel();
        let (file_tx, file_replies) = mpsc::unbounded_channel();
        let (actions, mut action_rx) = mpsc::unbounded_channel();
        let (stop, stopped) = oneshot::channel();
        let client = tokio::spawn(async move {
            agent_client_protocol::Client.builder()
                .name("aipp-permission-dispatch-test")
                .on_receive_request(
                    move |request: acp::RequestPermissionRequest,
                          responder: Responder<acp::RequestPermissionResponse>,
                          cx: ConnectionTo<agent_client_protocol::Agent>| {
                        let state = state.clone();
                        let pending_tx = pending_tx.clone();
                        async move {
                            spawn_acp_request_task(&cx, responder, async move {
                                let request_id = request.tool_call.tool_call_id.0.to_string();
                                let (tx, rx) = oneshot::channel();
                                // Desktop request construction/publication and actor
                                // controls are outside this fixture. Production state
                                // validates options and cancels waiters; production
                                // dispatch and response encoding handle every reply.
                                state.store_request(AcpPermissionRequestEvent {
                                    request_id: request_id.clone(),
                                    conversation_id: Some(conversation_id),
                                    agent_kind: Some("acp".into()),
                                    tool_call_id: request_id.clone(),
                                    title: request.tool_call.fields.title.clone(),
                                    kind: None,
                                    parameters: None,
                                    options: request.options.iter().map(|option| {
                                        AcpPermissionOptionPayload {
                                            option_id: option.option_id.0.to_string(),
                                            name: option.name.clone(),
                                            kind: permission_option_kind_to_string(&option.kind),
                                        }
                                    }).collect(),
                                }, tx).await;
                                pending_tx.send(request_id).unwrap();
                                Ok(wait_for_acp_permission_decision(rx).await)
                            })
                        }
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |_request: acp::ReadTextFileRequest,
                                responder: Responder<acp::ReadTextFileResponse>,
                                cx: ConnectionTo<agent_client_protocol::Agent>| {
                        spawn_acp_request_task(&cx, responder, async {
                            Err(acp::Error::internal_error().data(serde_json::json!({
                                "stage": "read-test-file", "reason": "fixture read denied"
                            })))
                        })
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_notification(
                    async move |notification: acp::SessionNotification, _cx| {
                        if let acp::SessionUpdate::AgentMessageChunk(chunk) = notification.update {
                            notes_tx.send(extract_content_text(&chunk.content)).unwrap();
                        }
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .connect_with(client_channel, async move |_cx| {
                    let _ = stopped.await;
                    Ok(())
                })
                .await
        });
        let agent = tokio::spawn(async move {
            agent_client_protocol::Agent.builder()
                .name("permission-wire-peer")
                .connect_with(agent_channel, async move |cx| {
                    loop {
                        tokio::select! {
                            _ = cx.incoming_closed() => break,
                            action = action_rx.recv() => match action {
                                Some(AgentAction::Permission(id)) => {
                                    let request: acp::RequestPermissionRequest = serde_json::from_value(
                                        serde_json::json!({
                                            "sessionId": "wire-session",
                                            "toolCall": { "toolCallId": id, "title": "Run test command", "kind": "execute" },
                                            "options": [
                                                { "optionId": "vendor:allow-once", "name": "Allow", "kind": "allow_once" },
                                                { "optionId": "deny", "name": "Deny", "kind": "reject_once" }
                                            ]
                                        })
                                    ).unwrap();
                                    let task_cx = cx.clone();
                                    let reply_tx = reply_tx.clone();
                                    cx.spawn(async move {
                                        let response = task_cx.send_request(request).block_task().await;
                                        let _ = reply_tx.send((id.to_string(), response));
                                        Ok(())
                                    })?;
                                }
                                Some(AgentAction::Notification(text)) => {
                                    cx.send_notification(acp::SessionNotification::new(
                                        "wire-session",
                                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(text.into())),
                                    ))?;
                                }
                                Some(AgentAction::ReadFile) => {
                                    let response = cx.send_request(acp::ReadTextFileRequest::new(
                                        "wire-session", "/fixture/not-a-real-read",
                                    )).block_task().await;
                                    let _ = file_tx.send(response);
                                }
                                None => break,
                            }
                        }
                    }
                    Ok(())
                })
                .await
        });
        Self { actions, pending, replies, notifications, file_replies, stop: Some(stop), client, agent }
    }

    async fn permission(&mut self, id: &'static str) {
        self.actions.send(AgentAction::Permission(id)).unwrap();
        assert_eq!(receive(&mut self.pending).await, id);
    }

    async fn reply(&mut self, expected_id: &str) -> serde_json::Value {
        let (id, response) = receive(&mut self.replies).await;
        assert_eq!(id, expected_id);
        serde_json::to_value(response.expect("permission response reaches peer")).unwrap()
    }

    async fn close(mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        tokio::time::timeout(DEADLINE, &mut self.client).await
            .expect("client closes within deadline").expect("client does not panic")
            .expect("client closes cleanly");
        tokio::time::timeout(DEADLINE, &mut self.agent).await
            .expect("peer observes disconnect within deadline").expect("peer does not panic")
            .expect("peer closes cleanly");
    }
}

async fn receive<T>(receiver: &mut mpsc::UnboundedReceiver<T>) -> T {
    tokio::time::timeout(DEADLINE, receiver.recv()).await
        .expect("protocol observation arrives within deadline")
        .expect("protocol observation channel remains open")
}

/// A blocked permission must not occupy the SDK dispatcher. A stream chunk and
/// a separate failing request both traverse it before the permission settles.
#[tokio::test]
async fn test_protocol_pending_permission_allows_notifications_and_error_responses() {
    let state = Arc::new(AcpPermissionState::new());
    let mut wire = ProtocolFixture::new(state.clone(), 1);
    wire.permission("pending").await;
    wire.actions.send(AgentAction::Notification("still streaming")).unwrap();
    wire.actions.send(AgentAction::ReadFile).unwrap();
    assert_eq!(receive(&mut wire.notifications).await, "still streaming");
    let error = receive(&mut wire.file_replies).await.expect_err("native error must remain an error");
    assert!(format!("{error:?}").contains("fixture read denied"));
    assert!(matches!(wire.replies.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    assert!(state.has_pending_permission_for_conversation(1).await);
    assert!(state.resolve_request("pending", AcpPermissionDecision::Selected("deny".into()))
        .await.unwrap().unwrap().delivered);
    assert_eq!(wire.reply("pending").await, serde_json::json!({
        "outcome": { "outcome": "selected", "optionId": "deny" }
    }));
    wire.close().await;
}

/// Cancellation produces a protocol cancellation, not an allow response. An old
/// approval cannot resolve the next request, and invalid option IDs stay pending.
#[tokio::test]
async fn test_protocol_cancelled_permission_drops_late_approval_and_preserves_next_request() {
    let state = Arc::new(AcpPermissionState::new());
    let mut wire = ProtocolFixture::new(state.clone(), 1);
    wire.permission("old").await;
    let resolutions = state.cancel_requests_for_conversation(1).await;
    assert_eq!(resolutions.len(), 1);
    assert!(resolutions[0].1.delivered);
    assert_eq!(wire.reply("old").await, serde_json::json!({ "outcome": { "outcome": "cancelled" } }));
    wire.permission("next").await;
    assert!(state.resolve_request("old", AcpPermissionDecision::Selected("vendor:allow-once".into()))
        .await.unwrap().is_none());
    let error = state.resolve_request("next", AcpPermissionDecision::Selected("invented".into()))
        .await.err().expect("unadvertised option must be rejected");
    assert!(error.contains("not advertised"));
    assert!(matches!(wire.replies.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    assert!(state.resolve_request("next", AcpPermissionDecision::Selected("vendor:allow-once".into()))
        .await.unwrap().unwrap().delivered);
    assert_eq!(wire.reply("next").await, serde_json::json!({
        "outcome": { "outcome": "selected", "optionId": "vendor:allow-once" }
    }));
    assert!(!state.has_pending_permission_for_conversation(1).await);
    wire.close().await;
}

/// Separate SDK connections can wait against one permission store. Cancelling
/// conversation A must leave B's wire request pending and B's stream usable.
#[tokio::test]
async fn test_protocol_concurrent_permission_connections_remain_isolated() {
    let state = Arc::new(AcpPermissionState::new());
    let mut first = ProtocolFixture::new(state.clone(), 1);
    let mut second = ProtocolFixture::new(state.clone(), 2);
    tokio::join!(first.permission("first"), second.permission("second"));
    assert_eq!(state.cancel_requests_for_conversation(1).await.len(), 1);
    assert_eq!(first.reply("first").await, serde_json::json!({ "outcome": { "outcome": "cancelled" } }));
    assert!(matches!(second.replies.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
    assert!(state.has_pending_permission_for_conversation(2).await);
    second.actions.send(AgentAction::Notification("second still active")).unwrap();
    assert_eq!(receive(&mut second.notifications).await, "second still active");
    state.resolve_request("second", AcpPermissionDecision::Selected("deny".into()))
        .await.unwrap().unwrap();
    assert_eq!(second.reply("second").await, serde_json::json!({
        "outcome": { "outcome": "selected", "optionId": "deny" }
    }));
    tokio::join!(first.close(), second.close());
}

/// Dropping an unresolved UI-side sender fails closed on the actual SDK wire.
#[tokio::test]
async fn test_protocol_removed_permission_waiter_returns_cancelled() {
    let state = Arc::new(AcpPermissionState::new());
    let mut wire = ProtocolFixture::new(state.clone(), 1);
    wire.permission("removed").await;
    state.remove_request("removed").await;
    assert_eq!(wire.reply("removed").await, serde_json::json!({ "outcome": { "outcome": "cancelled" } }));
    assert!(state.get_request("removed").await.is_none());
    wire.close().await;
}

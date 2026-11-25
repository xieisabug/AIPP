use super::*;
use crate::api::ai::agent_driver::next_permissions;
use agent_client_protocol::{Channel, ConnectionTo, Responder};
use serde_json::json;
use std::sync::Mutex;
use tokio::sync::Notify;

const SESSION: &str = "queued-permissions-session";
const PROVIDER: i64 = 3;

fn permission_options(value: &str) -> Vec<acp::SessionConfigOption> {
    serde_json::from_value(json!([{
        "id": "approval", "name": "Approval", "type": "select", "currentValue": value,
        "options": [{"value":"ask","name":"Ask"}, {"value":"allow","name":"Allow"}, {"value":"deny","name":"Deny"}]
    }])).unwrap()
}

async fn exercise_two_turns(reject_change: bool) {
    let (client_channel, agent_channel) = Channel::duplex();
    let (config_tx, mut config_rx) = mpsc::unbounded_channel();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let release_first = Arc::new(Notify::new());
    let new_requests = trace.clone();
    let config_requests = trace.clone();
    let prompts = trace.clone();
    let release = release_first.clone();
    let agent = agent_client_protocol::Agent.builder()
        .on_receive_request(async move |_: acp::NewSessionRequest, responder: Responder<acp::NewSessionResponse>, _| {
            new_requests.lock().unwrap().push(json!(["new", SESSION]));
            responder.respond(acp::NewSessionResponse::new(SESSION).config_options(permission_options("ask")))
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request(async move |req: acp::SetSessionConfigOptionRequest, responder: Responder<acp::SetSessionConfigOptionResponse>, _| {
            let wire = serde_json::to_value(req).unwrap();
            config_requests.lock().unwrap().push(json!(["set", wire["sessionId"], wire["configId"], wire["value"]]));
            config_tx.send((wire["value"].as_str().unwrap().to_owned(), responder)).unwrap();
            Ok(())
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request(async |_: acp::ListSessionsRequest, responder: Responder<acp::ListSessionsResponse>, _| {
            responder.respond(acp::ListSessionsResponse::new(vec![]))
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request(async move |req: acp::PromptRequest, responder: Responder<acp::PromptResponse>, cx: ConnectionTo<agent_client_protocol::Client>| {
            let text = extract_content_text(&req.prompt[0]);
            prompts.lock().unwrap().push(json!(["prompt", req.session_id, text]));
            if text == "first" {
                cx.send_notification(acp::SessionNotification::new(req.session_id,
                    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new("first is streaming".into()))))?;
                let release = release.clone();
                return cx.spawn(async move {
                    release.notified().await;
                    responder.respond(acp::PromptResponse::new(acp::StopReason::EndTurn))
                });
            }
            responder.respond(acp::PromptResponse::new(acp::StopReason::EndTurn))
        }, agent_client_protocol::on_receive_request!())
        .connect_with(agent_channel, async |cx| { cx.incoming_closed().await; Ok(()) });

    let streaming = Arc::new(Notify::new());
    let notice = streaming.clone();
    let expected_first = vec![json!(["new", SESSION]), json!(["prompt", SESSION, "first"])];
    let active_trace = trace.clone();
    let client = agent_client_protocol::Client.builder()
        .on_receive_notification(async move |update: acp::SessionNotification, _| {
            if let acp::SessionUpdate::AgentMessageChunk(chunk) = update.update {
                assert_eq!(extract_content_text(&chunk.content), "first is streaming");
                notice.notify_one();
            }
            Ok(())
        }, agent_client_protocol::on_receive_notification!())
        .connect_with(client_channel, async move |cx| {
            let opened = cx.send_request(acp::NewSessionRequest::new(std::env::temp_dir())).block_task().await?;
            let id = opened.session_id.to_string();
            let (db, keeper) = ConversationDatabase::shared_in_memory();
            db.create_tables().unwrap();
            keeper.execute("INSERT INTO conversation (name, created_time, updated_time) VALUES ('queue',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)", []).unwrap();
            let cid = keeper.last_insert_rowid();
            db.upsert_agent_session_id_for_instance(cid, "acp", Some(PROVIDER), &id).unwrap();

            let mut state = AcpConversationSessionState::default();
            apply_config_options_to_snapshot(&mut state, opened.config_options.as_deref());
            let state = Mutex::new(state);
            let output = tokio::sync::RwLock::new(None);
            let send = async |text: String| {
                dispatch_acp_prompt(&cx, &id, &output,
                    || next_permissions::read(&db.get_connection()?, cid, "acp", Some(PROVIDER), Some(&id)),
                    |options| {
                        let mut state = state.lock().unwrap();
                        if let Some(options) = options { apply_config_options_to_snapshot(&mut state, Some(&options)); }
                        std::future::ready(Some(state.clone()))
                    },
                    || build_acp_prompt_blocks(text, &[], &acp::PromptCapabilities::default(), None, None),
                ).await
            };
            let queue_and_edit = async {
                streaming.notified().await;
                let row = db.queued_message_repo().unwrap().enqueue(cid, "normal", "{}", "second", 1).unwrap();
                for value in ["allow", "deny"] {
                    next_permissions::save(&db.get_connection().unwrap(), cid, "acp", PROVIDER, &id, "approval", value).unwrap();
                }
                assert_eq!(*active_trace.lock().unwrap(), expected_first);
                assert_eq!(state.lock().unwrap().config_options[0].current_value, "ask");
                release_first.notify_one();
                row.id
            };
            let (first, queued_id) = tokio::join!(send("first".into()), queue_and_edit);
            assert_eq!(first.unwrap().1?.stop_reason, acp::StopReason::EndTurn);
            let queued = db.queued_message_repo().unwrap().take_next_for_dispatch(cid, false).unwrap().unwrap();
            assert_eq!(queued.id, queued_id);
            let acknowledge = async {
                let (requested, responder) = config_rx.recv().await.expect("permission RPC");
                // The peer is holding the config reply. Round-trip another request on the
                // same channel so an early second prompt would already appear in the trace.
                cx.send_request(acp::ListSessionsRequest::new()).block_task().await?;
                assert_eq!(requested, "deny");
                assert_eq!(*active_trace.lock().unwrap(), [
                    json!(["new", SESSION]), json!(["prompt", SESSION, "first"]), json!(["set", SESSION, "approval", "deny"])
                ]);
                assert_eq!(*output.read().await, None);
                if reject_change {
                    responder.respond_with_error(acp::Error::invalid_params().data("fixture rejected permission"))
                } else {
                    responder.respond(acp::SetSessionConfigOptionResponse::new(permission_options(&requested)))
                }
            };
            let (second, ack) = tokio::join!(send(queued.prompt), acknowledge);
            ack?;
            if reject_change {
                let Err(AcpPromptDispatchError::Permission(error)) = second else { panic!("expected a permission rejection"); };
                assert!(error.to_string().contains("fixture rejected permission"), "{error}");
                assert_eq!(state.lock().unwrap().config_options[0].current_value, "ask");
            } else {
                assert_eq!(second.unwrap().1?.stop_reason, acp::StopReason::EndTurn);
                assert_eq!(state.lock().unwrap().config_options[0].current_value, "deny");
            }
            Ok(())
        });

    tokio::time::timeout(Duration::from_secs(5), async { tokio::try_join!(client, agent) }).await
        .expect("both peers should finish without sleeps or a live model").unwrap();
    let mut expected = vec![json!(["new", SESSION]), json!(["prompt", SESSION, "first"]), json!(["set", SESSION, "approval", "deny"])];
    if !reject_change { expected.push(json!(["prompt", SESSION, "second"])); }
    assert_eq!(*trace.lock().unwrap(), expected);
}

#[tokio::test]
async fn test_acp_queued_permissions_apply_last_selection_before_second_prompt() {
    exercise_two_turns(false).await;
}

#[tokio::test]
async fn test_acp_queued_permissions_rejection_prevents_second_prompt() {
    exercise_two_turns(true).await;
}

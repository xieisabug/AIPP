//! Codex MCP approval and form requests share one native RPC, but approvals
//! must use the guarded permission store rather than the general question UI.
use super::*;
use crate::api::ai::acp::{AcpElicitationDecision, AcpElicitationRequestEvent, AcpElicitationState};

pub(super) const METHOD: &str = "mcpServer/elicitation/request";

pub(super) fn is_tool_approval(frame: &Value) -> Result<bool, String> {
    if frame["method"] != METHOD { return Ok(false); }
    let params = &frame["params"];
    let mode = params["mode"].as_str().unwrap_or("form");
    if !matches!(mode, "form" | "openai/form" | "openaiForm") {
        return Err(format!("Codex {METHOD}: unsupported elicitation mode '{mode}' [thread={}, turn={}, request={}]",
            params["threadId"], params["turnId"], frame["id"]));
    }
    Ok(params["_meta"]["codex_approval_kind"] == "mcp_tool_call")
}

pub(super) fn approval_response(decision: AcpPermissionDecision) -> Value {
    let action = match decision {
        AcpPermissionDecision::Selected(value) if value == "accept" => "accept",
        AcpPermissionDecision::Selected(value) if value == "decline" => "decline",
        _ => "cancel",
    };
    // No persistence metadata: accepting this request authorizes this call only.
    json!({"action": action, "content": null})
}

fn form_event(params: &Value, id: String, conversation: i64) -> Result<AcpElicitationRequestEvent, String> {
    let message = params["message"].as_str().ok_or("Codex elicitation omitted message")?;
    let mut schema = params["requestedSchema"].clone();
    let properties = schema.get_mut("properties").and_then(Value::as_object_mut)
        .ok_or("Codex elicitation requestedSchema must contain object properties")?;
    // MCP uses const; the shared ACP form UI uses value for titled choices.
    for field in properties.values_mut() {
        for pointer in ["/oneOf", "/items/anyOf"] {
            if let Some(options) = field.pointer_mut(pointer).and_then(Value::as_array_mut) {
                for option in options {
                    if let Some(value) = option.get("const").cloned() {
                        option.as_object_mut().ok_or("Invalid Codex elicitation choice")?.insert("value".into(), value);
                    }
                }
            }
        }
        if let Some(names) = field.get("enumNames").and_then(Value::as_array).cloned() {
            if let Some(values) = field.get("enum").and_then(Value::as_array) {
                let options = values.iter().enumerate().map(|(index, value)| json!({
                    "value": value, "title": names.get(index).unwrap_or(value)
                })).collect::<Vec<_>>();
                field.as_object_mut().unwrap().insert("oneOf".into(), json!(options));
            }
        }
    }
    Ok(AcpElicitationRequestEvent { request_id: id, conversation_id: Some(conversation),
        agent_kind: CODEX_APP_SERVER_API_TYPE.into(), message: message.into(), schema })
}

fn form_response(decision: AcpElicitationDecision) -> Value {
    match decision {
        AcpElicitationDecision::Accepted(content) => json!({"action": "accept", "content": content}),
        AcpElicitationDecision::Declined => json!({"action": "decline", "content": null}),
        AcpElicitationDecision::Cancelled => json!({"action": "cancel", "content": null}),
    }
}

async fn wait_form(
    state: &AcpElicitationState, id: &str, receiver: oneshot::Receiver<AcpElicitationDecision>,
    cancellation: Option<&CodexRequestCancellation>,
) -> (Value, Option<crate::api::ai::acp::AcpPermissionResolution>) {
    match wait_codex_interaction(async { Ok(receiver.await.unwrap_or(AcpElicitationDecision::Cancelled)) }, cancellation).await {
        Ok(decision) => (form_response(decision), None),
        Err(_) => (form_response(AcpElicitationDecision::Cancelled),
            state.resolve_request(id, AcpElicitationDecision::Cancelled).await),
    }
}

pub(super) async fn handle_form(
    app: &tauri::AppHandle, conversation: i64, stdin: &mut tokio::process::ChildStdin,
    frame: &Value, cancellation: Option<&CodexRequestCancellation>,
) -> Result<bool, String> {
    if frame["method"] != METHOD || is_tool_approval(frame)? { return Ok(false); }
    let rpc_id = frame.get("id").ok_or("Codex elicitation missing request id")?;
    let id = format!("codex:elicitation:{conversation}:{}:{}",
        cancellation.map(|context| context.run_id.as_str()).unwrap_or("startup"), rpc_id_key(rpc_id));
    let event = form_event(&frame["params"], id.clone(), conversation)?;
    let scope = crate::api::ai::agent_driver::approval::AgentRequestScope {
        driver_id: Some(CODEX_APP_SERVER_API_TYPE.into()),
        instance_id: cancellation.and_then(|context| context.instance_id),
        conversation_id: Some(conversation),
        run_id: cancellation.map(|context| context.run_id.clone()),
    };
    let state = app.state::<AcpElicitationState>();
    let (sender, receiver) = oneshot::channel();
    state.store_request_scoped(id.clone(), Some(conversation), sender, scope).await?;
    if let Err(error) = state.attach_form(event.clone()).await {
        state.remove_request(&id).await;
        return Err(error);
    }
    if let Err(error) = emit_permission_request_event(app,
        crate::api::operation_api::ACP_ELICITATION_REQUEST_EVENT, Some(conversation), &event) {
        state.remove_request(&id).await;
        return Err(error);
    }
    let (result, cancelled) = wait_form(&state, &id, receiver, cancellation).await;
    if let Some(resolution) = cancelled {
        crate::api::ai::acp::notify_cancelled_acp_elicitation_requests(app, vec![(id, resolution)]).await;
    }
    write_frame(stdin, &json!({"jsonrpc":"2.0", "id":rpc_id, "result":result})).await?;
    Ok(true)
}

#[cfg(test)]
#[path = "codex_app_server_elicitation_tests.rs"]
mod tests;

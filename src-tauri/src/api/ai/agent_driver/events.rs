//! One publication boundary for native agent conversation events.
//! Native actors retain their protocol and ordering. The compatibility adapter
//! deliberately preserves the existing event type and complete JSON payload.
use crate::api::ai::events::ConversationEvent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentEventIdentity {
    pub driver_id: String,
    /// Existing provider ID, not a new session or process identifier.
    pub instance_id: Option<String>,
    pub conversation_id: i64,
    pub native_session_id: Option<String>,
    pub run_id: Option<String>,
    pub turn_id: Option<String>,
    pub response_message_id: Option<i64>,
}

impl AgentEventIdentity {
    pub(crate) fn new(driver_id: impl Into<String>, conversation_id: i64) -> Self {
        Self { driver_id: driver_id.into(), instance_id: None, conversation_id,
            native_session_id: None, run_id: None, turn_id: None, response_message_id: None }
    }

    pub(crate) fn with_instance(mut self, instance_id: i64) -> Self {
        self.instance_id = Some(instance_id.to_string()); self
    }

    pub(crate) fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.native_session_id = Some(session_id.into()); self
    }

    pub(crate) fn with_run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into()); self
    }

    pub(crate) fn with_turn(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into()); self
    }

    pub(crate) fn with_response_message(mut self, message_id: i64) -> Self {
        self.response_message_id = Some(message_id); self
    }
}

/// Snapshot caches belong to the native actor run, never just a conversation.
/// An absent run cannot authorize mutation of an existing actor's snapshot.
pub(crate) fn matches_native_run(current_run_id: &str, event_run_id: Option<&str>) -> bool {
    event_run_id.is_some_and(|run_id| run_id == current_run_id)
}

pub(crate) fn can_publish_native_snapshot(current_run_id: Option<&str>, event_run_id: Option<&str>, has_state: bool) -> bool {
    if has_state { current_run_id.is_some_and(|run| matches_native_run(run, event_run_id)) }
    else { current_run_id.is_none() }
}

/// A stable semantic category, without restricting native extension payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentEventKind {
    MessageAdded, MessageUpdated, MessageMetadataUpdated, MessageTypeEnded,
    ToolUpdated, Activity, SessionSnapshot, StreamComplete, Extension,
}

impl AgentEventKind {
    fn from_compatibility_type(event_type: &str) -> Self {
        match event_type {
            "message_add" => Self::MessageAdded,
            "message_update" => Self::MessageUpdated,
            "message_metadata_update" => Self::MessageMetadataUpdated,
            "message_type_end" => Self::MessageTypeEnded,
            "mcp_tool_call_update" => Self::ToolUpdated,
            "agent_activity" => Self::Activity,
            "acp_session_state_snapshot" => Self::SessionSnapshot,
            "stream_complete" => Self::StreamComplete,
            _ => Self::Extension,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AgentEventEnvelope {
    pub schema_version: u32,
    pub identity: AgentEventIdentity,
    pub kind: AgentEventKind,
    pub compatibility_event_type: String,
    pub payload: Value,
    /// Preserve future envelope fields as well as native payload extensions.
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl AgentEventEnvelope {
    pub(crate) fn from_conversation_event(mut identity: AgentEventIdentity, event: ConversationEvent) -> Self {
        // Only identities carried by this event are inferred. Never consult the
        // mutable current conversation selection to label an old actor's event.
        if identity.response_message_id.is_none() {
            identity.response_message_id = event.data.get("response_message_id").and_then(Value::as_i64)
                .or_else(|| match event.data.get("message_type").and_then(Value::as_str) {
                    Some("response" | "error") => event.data.get("message_id").and_then(Value::as_i64),
                    _ => None,
                });
        }
        Self { schema_version: 1, identity, kind: AgentEventKind::from_compatibility_type(&event.r#type),
            compatibility_event_type: event.r#type, payload: event.data, extensions: BTreeMap::new() }
    }

    pub(crate) fn compatibility_event(&self) -> ConversationEvent {
        ConversationEvent { r#type: self.compatibility_event_type.clone(), data: self.payload.clone() }
    }
}

/// The only substituted boundary in tests is desktop delivery. Native event
/// classification, identity, compatibility serialization and order are real.
pub(crate) trait AgentEventSink {
    fn emit(&self, event_name: &str, payload: &Value) -> Result<(), String>;
    fn with_current_binding(&self, _identity: &AgentEventIdentity, publish: impl FnOnce()) -> Result<bool, String> {
        publish(); Ok(true)
    }
}

pub(crate) fn publish_event(sink: &impl AgentEventSink, envelope: &AgentEventEnvelope) -> Result<(), String> {
    if matches!(envelope.kind, AgentEventKind::SessionSnapshot | AgentEventKind::StreamComplete) {
        let mut result = Ok(());
        sink.with_current_binding(&envelope.identity, || { result = publish_event_unchecked(sink, envelope); })?;
        result
    } else {
        publish_event_unchecked(sink, envelope)
    }
}

fn publish_event_unchecked(sink: &impl AgentEventSink, envelope: &AgentEventEnvelope) -> Result<(), String> {
    let conversation_id = envelope.identity.conversation_id;
    let compatibility = serde_json::to_value(envelope.compatibility_event()).map_err(|error| error.to_string())?;
    let normalized = serde_json::to_value(envelope).map_err(|error| error.to_string())?;
    // Preserve legacy delivery even if the new consumer fails, and vice versa.
    // Never retry an uncertain emit; native actor ordering remains authoritative.
    let legacy_result = sink.emit(&format!("conversation_event_{conversation_id}"), &compatibility);
    let normalized_result = sink.emit(&format!("agent_event_{conversation_id}"), &normalized);
    match (legacy_result, normalized_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(format!("Agent compatibility event delivery failed: {error}")),
        (Ok(()), Err(error)) => Err(format!("Agent normalized event delivery failed: {error}")),
        (Err(legacy), Err(normalized)) => Err(format!("Agent event delivery failed: compatibility={legacy}; normalized={normalized}")),
    }
}

pub(crate) struct WindowEventSink<'a>(pub(crate) &'a tauri::Window);

impl AgentEventSink for WindowEventSink<'_> {
    fn with_current_binding(&self, identity: &AgentEventIdentity, publish: impl FnOnce()) -> Result<bool, String> {
        super::lifecycle::publish_if_current_binding(self.0.app_handle(), identity.conversation_id,
            &identity.driver_id, identity.run_id.as_deref(), publish).map_err(|error| error.to_string())
    }

    fn emit(&self, event_name: &str, payload: &Value) -> Result<(), String> {
        self.0.emit(event_name, payload).map_err(|error| error.to_string())
    }
}

pub(crate) struct ChatWindowsEventSink<'a>(pub(crate) &'a tauri::AppHandle);

impl AgentEventSink for ChatWindowsEventSink<'_> {
    fn with_current_binding(&self, identity: &AgentEventIdentity, publish: impl FnOnce()) -> Result<bool, String> {
        super::lifecycle::publish_if_current_binding(self.0, identity.conversation_id,
            &identity.driver_id, identity.run_id.as_deref(), publish).map_err(|error| error.to_string())
    }

    fn emit(&self, event_name: &str, payload: &Value) -> Result<(), String> {
        let mut failures = Vec::new();
        for label in ["ask", "chat_ui", "butler_experiment"] {
            if let Some(window) = self.0.get_webview_window(label) {
                if let Err(error) = window.emit(event_name, payload) {
                    failures.push(format!("{label}: {error}"));
                }
            }
        }
        if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
    }
}

pub(crate) fn emit_to_window(window: &tauri::Window, identity: AgentEventIdentity, event: ConversationEvent) -> Result<(), String> {
    let event=crate::api::ai::events::guard_goal_event(window.app_handle(),identity.conversation_id,event);
    publish_event(&WindowEventSink(window), &AgentEventEnvelope::from_conversation_event(identity, event))
}

pub(crate) fn emit_to_chat_windows(app: &tauri::AppHandle, identity: AgentEventIdentity, event: ConversationEvent) -> Result<(), String> {
    let event=crate::api::ai::events::guard_goal_event(app,identity.conversation_id,event);
    publish_event(&ChatWindowsEventSink(app), &AgentEventEnvelope::from_conversation_event(identity, event))
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;

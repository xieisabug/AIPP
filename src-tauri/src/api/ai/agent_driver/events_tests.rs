use super::*;
use serde_json::json;
use std::cell::RefCell;

#[derive(Default)]
struct RecordingSink {
    events: RefCell<Vec<(String, Value)>>,
    fail_legacy: bool,
}

impl AgentEventSink for RecordingSink {
    fn emit(&self, name: &str, payload: &Value) -> Result<(), String> {
        self.events.borrow_mut().push((name.into(), payload.clone()));
        if self.fail_legacy && name.starts_with("conversation_event_") { Err("closed compatibility window".into()) }
        else { Ok(()) }
    }
}

/// All native categories, including plan messages and unknown extensions, retain
/// their exact compatibility event and payload while carrying common identity.
#[test]
fn test_agent_event_sink_preserves_native_payloads_and_identity_for_all_drivers() {
    for driver in ["acp", "codex_app_server", "claude_sdk"] {
        let identity = AgentEventIdentity::new(driver, 42).with_instance(7)
            .with_session("native-session").with_run("run-A").with_turn("turn-A").with_response_message(21);
        let sink = RecordingSink::default();
        for (event_type, kind) in [
            ("message_add", AgentEventKind::MessageAdded),
            ("message_update", AgentEventKind::MessageUpdated),
            ("message_metadata_update", AgentEventKind::MessageMetadataUpdated),
            ("message_type_end", AgentEventKind::MessageTypeEnded),
            ("mcp_tool_call_update", AgentEventKind::ToolUpdated),
            ("agent_activity", AgentEventKind::Activity),
            ("acp_session_state_snapshot", AgentEventKind::SessionSnapshot),
            ("stream_complete", AgentEventKind::StreamComplete),
            ("vendor:new_extension", AgentEventKind::Extension),
        ] {
            let original = ConversationEvent { r#type: event_type.into(), data: json!({
                "message_id": 22, "message_type": "agent_plan", "content": "完整计划 🌱",
                "unknown": { "vendor": [null, false, 1, {"raw": "stderr\ntrace"}] }, "native_id": "opaque:id"
            }) };
            let envelope = AgentEventEnvelope::from_conversation_event(identity.clone(), original.clone());
            assert_eq!(envelope.kind, kind);
            publish_event(&sink, &envelope).unwrap();
            let events = sink.events.borrow();
            let pair = &events[events.len() - 2..];
            assert_eq!(pair[0].0, "conversation_event_42");
            assert_eq!(pair[0].1, serde_json::to_value(&original).unwrap());
            assert_eq!(pair[1].0, "agent_event_42");
            let normalized: AgentEventEnvelope = serde_json::from_value(pair[1].1.clone()).unwrap();
            assert_eq!(normalized.identity, identity);
            assert_eq!(serde_json::to_value(normalized.compatibility_event()).unwrap(), serde_json::to_value(original).unwrap());
        }
    }
}

/// Stream order is the caller's native order. The common sink does not reorder
/// reasoning/response updates or silently swallow agent-specific terminal data.
#[test]
fn test_agent_event_sink_preserves_native_stream_order() {
    let sink = RecordingSink::default();
    let identity = AgentEventIdentity::new("codex_app_server", 12).with_run("run");
    for (event_type, data) in [
        ("message_add", json!({"message_id": 1, "message_type": "response"})),
        ("message_update", json!({"message_id": 1, "message_type": "response", "content": "a", "is_done": false})),
        ("agent_activity", json!({"native_tool_id": "cmd-1", "status": "executing"})),
        ("message_update", json!({"message_id": 1, "message_type": "response", "content": "ab", "is_done": true})),
        ("stream_complete", json!({"response_message_id": 1, "has_plan": true, "custom_usage": {"cached": 3}})),
    ] {
        publish_event(&sink, &AgentEventEnvelope::from_conversation_event(identity.clone(), ConversationEvent {
            r#type: event_type.into(), data,
        })).unwrap();
    }
    let events = sink.events.borrow();
    assert_eq!(events.len(), 10);
    let kinds: Vec<_> = events.iter().step_by(2).map(|(_, value)| value["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["message_add", "message_update", "agent_activity", "message_update", "stream_complete"]);
    assert_eq!(events[8].1["data"]["custom_usage"]["cached"], 3);
    assert_eq!(events[9].1["payload"]["custom_usage"]["cached"], 3);
}

#[test]
fn test_agent_event_identity_does_not_invent_run_or_mislabel_reasoning_message() {
    let envelope = AgentEventEnvelope::from_conversation_event(AgentEventIdentity::new("claude_sdk", 3), ConversationEvent {
        r#type: "message_update".into(), data: json!({"message_id": 44, "message_type": "reasoning"}),
    });
    assert_eq!(envelope.identity.response_message_id, None);
    assert_eq!(envelope.identity.instance_id, None);
    assert_eq!(envelope.identity.run_id, None);
    assert_eq!(envelope.identity.native_session_id, None);
    assert_eq!(envelope.identity.turn_id, None);
}

#[test]
fn test_agent_event_envelope_roundtrip_retains_unknown_top_level_extension() {
    let envelope = AgentEventEnvelope::from_conversation_event(AgentEventIdentity::new("acp", 8), ConversationEvent {
        r#type: "future-native-event".into(), data: json!([{"unknown":"保留"}, null, 42]),
    });
    let mut encoded = serde_json::to_value(&envelope).unwrap();
    encoded["future_metadata"] = json!({"keys":["unknown", 3]});
    let decoded: AgentEventEnvelope = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), encoded);
}

#[test]
fn test_agent_event_sink_attempts_both_channels_once_when_legacy_delivery_fails() {
    let sink = RecordingSink { fail_legacy: true, ..Default::default() };
    let event = AgentEventEnvelope::from_conversation_event(AgentEventIdentity::new("acp", 1), ConversationEvent {
        r#type: "message_update".into(), data: json!({}),
    });
    assert!(publish_event(&sink, &event).unwrap_err().contains("closed compatibility window"));
    assert_eq!(sink.events.borrow().len(), 2);
    assert_eq!(sink.events.borrow()[1].0, "agent_event_1");
}

#[test]
fn test_agent_snapshot_guard_rejects_replaced_or_missing_run_and_accepts_owner() {
    assert!(matches_native_run("current-run", Some("current-run")));
    assert!(!matches_native_run("current-run", Some("old-run")));
    assert!(!matches_native_run("current-run", None));
    let mut snapshot = json!({"session_id": "new-session", "native_extra": {"preserve": true}});
    if matches_native_run("current-run", Some("old-run")) {
        snapshot = json!({"session_id":"old-session"});
    }
    assert_eq!(snapshot["session_id"], "new-session");
    assert_eq!(snapshot["native_extra"]["preserve"], true);
}

#[test]
fn test_agent_snapshot_absent_actor_cannot_resurrect_state_after_teardown() {
    assert!(!can_publish_native_snapshot(None, Some("removed-run"), true));
    assert!(!can_publish_native_snapshot(Some("new-run"), Some("old-run"), true));
    assert!(can_publish_native_snapshot(Some("new-run"), Some("new-run"), true));
    assert!(!can_publish_native_snapshot(Some("new-run"), Some("old-run"), false));
    assert!(can_publish_native_snapshot(None, Some("removed-run"), false));
}

#[test]
fn test_agent_sink_replaced_run_keeps_message_history_but_suppresses_global_terminal() {
    use super::super::lifecycle::{AgentSessionBinding, AgentSessionBindings};
    struct BoundSink { recording: RecordingSink, bindings: AgentSessionBindings }
    impl AgentEventSink for BoundSink {
        fn emit(&self, name: &str, payload: &Value) -> Result<(), String> { self.recording.emit(name, payload) }
        fn with_current_binding(&self, identity: &AgentEventIdentity, publish: impl FnOnce()) -> Result<bool, String> {
            self.bindings.publish_if_current(identity.conversation_id, &identity.driver_id, identity.run_id.as_deref(), publish)
                .map_err(|error| error.to_string())
        }
    }
    let sink = BoundSink { recording: RecordingSink::default(), bindings: AgentSessionBindings::default() };
    sink.bindings.bind(AgentSessionBinding { conversation_id: 42, provider_id: 9, driver_id: "codex_app_server".into(),
        config_fingerprint: "new-config".into(), run_id: "new-run".into() }).unwrap();
    let old = AgentEventIdentity::new("acp", 42).with_run("old-run").with_response_message(20);
    for event_type in ["message_update", "stream_complete", "acp_session_state_snapshot"] {
        publish_event(&sink, &AgentEventEnvelope::from_conversation_event(old.clone(), ConversationEvent {
            r#type: event_type.into(), data: json!({"message_id":20,"content":"old failure","state":null}),
        })).unwrap();
    }
    assert_eq!(sink.recording.events.borrow().len(), 2);
    assert_eq!(sink.recording.events.borrow()[0].1["type"], "message_update");
    let current = AgentEventIdentity::new("codex_app_server", 42).with_run("new-run");
    publish_event(&sink, &AgentEventEnvelope::from_conversation_event(current, ConversationEvent {
        r#type: "stream_complete".into(), data: json!({"response_message_id":21}),
    })).unwrap();
    assert_eq!(sink.recording.events.borrow().len(), 4);
}

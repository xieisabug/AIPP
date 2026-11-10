use super::*;

/// T3 Cursor lifecycle: a completed agent turn does not prove unfinished tools succeeded.
#[test]
fn test_native_agents_unfinished_tools_never_become_success_on_turn_completion() {
    for status in ["pending", "executing"] {
        let mut tool = json!({"kind":"tool", "status":status,"sequence":3});
        assert!(settle_activity(&mut tool, false, None));
        assert_eq!(tool["status"], "cancelled");
        assert_eq!(tool["sequence"], 4);
        assert!(tool["error"].as_str().unwrap().contains("未返回工具终态"));
    }
    let mut finished = json!({"kind":"tool", "status":"success","sequence":3});
    assert!(!settle_activity(&mut finished, false, Some("transport failed")));
    assert_eq!(finished["status"], "success");
}

#[test]
fn test_native_agents_terminal_activity_preserves_failure_and_cancellation() {
    let mut failed = json!({"kind":"tool", "status":"executing","sequence":1});
    assert!(settle_activity(&mut failed, false, Some("SDK transport closed")));
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"], "SDK transport closed");
    let mut reasoning = json!({"kind":"reasoning", "status":"executing","sequence":1});
    assert!(settle_activity(&mut reasoning, false, None));
    assert_eq!(reasoning["status"], "success");
    let mut cancelled = json!({"kind":"reasoning", "status":"executing","sequence":1});
    assert!(settle_activity(&mut cancelled, true, None));
    assert_eq!(cancelled["status"], "cancelled");
}

#[test]
fn test_native_agents_activity_updates_advance_sequence_and_keep_initial_position() {
    let first = merge_activity(None, &json!({"id":"tool", "kind":"tool", "status":"running", "input":{"path":"a"}, "output":"one"}), 2);
    let next = merge_activity(Some(&first), &json!({"id":"tool", "status":"success", "output":"two", "append":true}), 9);
    assert_eq!(first["status"], "executing");
    assert_eq!(next["sequence"], 2);
    assert_eq!(next["content_offset"], 2);
    assert_eq!(next["input"]["path"], "a");
    assert_eq!(next["output"], "onetwo");
    assert_eq!(next["status"], "success");
}

#[test]
fn test_native_agents_register_distinct_drivers_without_acp_aliases() {
    let registry = super::super::registry::AgentDriverRegistry::default();
    super::super::native::register_builtin_drivers(&registry).unwrap();
    for (index, id) in DRIVER_IDS.iter().enumerate() {
        let instance = registry.resolve_provider(index as i64 + 1, id, Vec::new()).unwrap();
        assert_eq!(instance.driver_id, *id);
        let driver = registry.driver(id).unwrap();
        assert!(!driver.matches_provider("acp", &[]));
        assert!(driver.metadata().operations.cancel_prompt);
        assert!(!driver.metadata().operations.structured_attachments);
        let caps = serde_json::to_value(driver.capabilities()).unwrap();
        assert_eq!(caps["sessions"]["resume"]["support"], "supported");
        assert_eq!(caps["attachments"]["image"]["support"], "unsupported");
    }
    assert!(registry.resolve_provider(9, "opencode_go", Vec::new()).is_err());
}

#[test]
fn test_native_agents_environment_preserves_values_and_rejects_malformed_input() {
    let json = parse_environment(r#"{"TOKEN":"contains=equals", "FLAG":"true"}"#).unwrap();
    let lines = parse_environment("# comment\nTOKEN=contains=equals\nFLAG=true\n").unwrap();
    assert_eq!(json, lines);
    assert!(parse_environment("MISSING_VALUE").is_err());
    assert!(parse_environment(r#"{"TOKEN":42}"#).is_err());
    assert!(parse_environment("=invalid").is_err());
}

#[tokio::test]
async fn test_native_agents_control_preserves_protocol_errors_without_retry() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let control = NativeControl { driver: NativeDriver("pi"), tx, busy: Arc::new(AtomicBool::new(false)) };
    let actor = tokio::spawn(async move {
        let Some(Command::Request { wire, reply }) = rx.recv().await else { panic!("missing configuration request") };
        assert_eq!(wire["key"], "model");
        assert_eq!(wire["value"], "provider/opaque/model");
        reply.send(Err("Pi set_model: credential expired".into())).unwrap();
        assert!(timeout(Duration::from_millis(30), rx.recv()).await.is_err());
    });
    let failure = control.set_config_option("model".into(), "provider/opaque/model".into()).await.unwrap_err();
    assert!(failure.to_string().contains("credential expired"));
    actor.await.unwrap();
    assert!(control.cancel_current_prompt().await.is_err());
}

use super::*;

/// T3 Cursor lifecycle: a completed agent turn does not prove unfinished tools succeeded.
#[test]
fn test_native_agents_unfinished_tools_never_become_success_on_turn_completion() {
    for (kind, status) in ["tool", "command", "patch", "file_read", "file_search", "file_write"].iter().flat_map(|kind| ["pending", "executing"].map(|status| (*kind, status))) {
        let mut tool = json!({"kind":kind, "status":status,"sequence":3});
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
    let first = merge_activity(None, &json!({"id":"tool", "kind":"tool", "status":"running", "input":{"path":"a"}, "output":"one"}), 2, 0);
    let next = merge_activity(Some(&first), &json!({"id":"tool", "status":"success", "output":"two", "append":true}), 9, 5);
    assert_eq!(first["status"], "executing");
    assert_eq!(next["sequence"], 2);
    assert_eq!(next["content_offset"], 2);
    assert_eq!(next["input"]["path"], "a");
    assert_eq!(next["output"], "onetwo");
    assert_eq!(next["status"], "success");
    assert_eq!(next["order"], 0);
    assert_eq!(next["started_at"], first["started_at"]);
    assert!(next["finished_at"].is_string());
}

#[test]
fn test_native_agents_activity_metadata_survives_partial_and_terminal_frames() {
    let first = merge_activity(None, &json!({"id":"edit", "kind":"patch", "status":"running", "input":{"path":"a.rs"}, "metadata":{"tool":"edit", "cwd":"/repo"}}), 2, 0);
    let done = merge_activity(Some(&first), &json!({"id":"edit", "status":"failed", "error":"file missing", "metadata":{"changes":[{"path":"a.rs", "diff":"+hello"}], "rawResult":{"content":[]}}}), 9, 1);
    assert_eq!(done["metadata"]["tool"], "edit");
    assert_eq!(done["metadata"]["cwd"], "/repo");
    assert_eq!(done["metadata"]["changes"][0]["diff"], "+hello");
    assert_eq!(done["metadata"]["rawResult"], json!({"content":[]}));
    assert_eq!(done["input"]["path"], "a.rs");
    assert_eq!(done["error"], "file missing");
    assert_eq!(done["content_offset"], 2);
    assert_eq!(done["kind"], "patch");
    assert_eq!(done["status"], "failed");
}

#[test]
fn test_native_agents_replay_order_tracks_first_seen_position_not_update_count() {
    let mut activities = HashMap::new();
    let thought = merge_activity(None, &json!({"id":"thinking:1","kind":"reasoning","status":"running","output":"think","append":true}), 0, 0);
    activities.insert("thinking:1".into(), thought);
    let command = merge_activity(None, &json!({"id":"command","kind":"command","status":"running"}), 0, 1);
    activities.insert("command".into(), command);
    for _ in 0..30 {
        let next = merge_activity(activities.get("thinking:1"), &json!({"id":"thinking:1","kind":"reasoning","status":"running","output":"more","append":true}), 0, 2);
        activities.insert("thinking:1".into(), next);
    }
    let saved = json!({"agent_activities":ordered_activities(&activities)});
    let restored: Value = serde_json::from_str(&saved.to_string()).unwrap();
    assert_eq!(restored["agent_activities"][0]["item_id"], "thinking:1");
    assert_eq!(restored["agent_activities"][1]["item_id"], "command");
    assert_eq!(restored["agent_activities"][0]["order"], 0);
    assert_eq!(restored["agent_activities"][0]["sequence"], 31);
    let thought = activities.get_mut("thinking:1").unwrap();
    assert!(settle_activity(thought, true, None));
    assert_eq!(thought["order"], 0);
    assert!(thought["finished_at"].is_string());
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

fn permission_option(id: &str, kind: &str) -> crate::api::ai::acp::AcpPermissionOptionPayload {
    crate::api::ai::acp::AcpPermissionOptionPayload { option_id: id.into(), name: id.into(), kind: kind.into() }
}

/// AIPP auto-review only ever answers with the one-time allow; everything else goes to the user with the reason.
#[test]
fn test_native_agents_review_outcome_allows_once_and_explains_holds() {
    let options = [permission_option("always", "allow_always"), permission_option("once", "allow_once"), permission_option("deny", "reject_once")];
    assert_eq!(review_outcome(&options, "safe", "只读查询"), ReviewOutcome::Allow("once".into()));
    match review_outcome(&options, "risky", "会删除工作区外的文件") {
        ReviewOutcome::Ask(reason) => assert!(reason.contains("会删除工作区外的文件")),
        other => panic!("risky requests must ask the user: {other:?}"),
    }
    match review_outcome(&options, "error", "自动审核模型未配置") {
        ReviewOutcome::Ask(reason) => assert!(reason.contains("未完成") && reason.contains("自动审核模型未配置")),
        other => panic!("failed reviews must ask the user: {other:?}"),
    }
    let only_always = [permission_option("always", "allow_always"), permission_option("deny", "reject_once")];
    match review_outcome(&only_always, "safe", "安全") {
        ReviewOutcome::Ask(reason) => assert!(reason.contains("没有提供「本次允许」")),
        other => panic!("a persistent allow is never chosen automatically: {other:?}"),
    }
}

#[test]
fn test_native_agents_review_and_permission_settings_follow_the_assistant() {
    use crate::db::assistant_db::AssistantModelConfig;
    let assistant = |name: &str, value: &str| AssistantModelConfig { id: 0, assistant_id: 1, assistant_model_id: 0, name: name.into(), value: Some(value.into()), value_type: "string".into() };
    let config = configuration(&[assistant("tool_review_mode", "auto_model"), assistant("pi_approval", "read_only")], &[], None).unwrap();
    assert!(config.review);
    assert_eq!(config.wire["pi_approval"], "read_only");
    assert_eq!(config.wire["aipp_review"], true, "the review mode is part of the session signature");
    let manual = configuration(&[assistant("tool_review_mode", "off")], &[], None).unwrap();
    assert!(!manual.review);
}

#[test]
fn test_native_agents_permission_parameters_unwrap_json_text() {
    assert_eq!(permission_parameters(&json!("{\"command\":\"ls -la\"}")), "{\"command\":\"ls -la\"}");
    assert_eq!(permission_parameters(&json!({"path":"a.txt"})), "{\"path\":\"a.txt\"}");
    assert_eq!(permission_parameters(&json!("plain text")), "plain text");
}

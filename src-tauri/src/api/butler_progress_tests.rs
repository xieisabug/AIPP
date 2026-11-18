use super::*;

fn event(kind: &str, data: Value) -> ConversationEvent {
    ConversationEvent { r#type: kind.into(), data }
}

#[test]
fn test_progress_window_only_contains_new_content_from_cumulative_events() {
    let mut buf = ProgressBuffer::default();
    for (event_type, data) in [
        ("message_update", serde_json::json!({"message_id":1,"message_type":"response","content":"已定位"})),
        ("agent_activity", serde_json::json!({"response_message_id":2,"item_id":"thought","kind":"reasoning","output":"核对"})),
        ("message_update", serde_json::json!({"message_id":3,"message_type":"agent_plan","content":"准备修改"})),
    ] {
        let event = event(event_type, data);
        buf.observe(&event);
        assert!(!buf.take().unwrap().text.is_empty());
        buf.observe(&event);
        assert!(buf.take().is_none());
    }
    buf.observe(&event("message_update",serde_json::json!({"message_id":1,"message_type":"response","content":"已定位文件"})));
    assert_eq!(buf.take().unwrap().text, "文件");
    buf.text("response:1".into(), "开始测试");
    assert_eq!(buf.take().unwrap().text, "开始测试");
    buf.text("response:1".into(), "开始测试 ");
    assert!(buf.take().is_none());
    assert_eq!(MIN_INTERVAL, Duration::from_secs(15));
}

#[test]
fn test_progress_batches_only_new_work_at_fifteen_second_intervals() {
    let start = Instant::now();
    let mut next = start + MIN_INTERVAL;
    let mut buffer = ProgressBuffer::default();
    buffer.text("a".into(), "检查配置");
    assert!(buffer.take_due(start + Duration::from_secs(14), &mut next).is_none());
    buffer.step("1".into(), "read config".into(), "success");
    let first = buffer.take_due(start + Duration::from_secs(15), &mut next).unwrap();
    assert_eq!(first.text, "检查配置");
    assert_eq!(first.steps.len(), 1);
    assert!(buffer.take_due(start + Duration::from_secs(30), &mut next).is_none());
    buffer.text("a".into(), "检查配置并核对结果");
    assert!(buffer.take_due(start + Duration::from_secs(44), &mut next).is_none());
    let second = buffer.take_due(start + Duration::from_secs(45), &mut next).unwrap();
    assert_eq!(second.text, "并核对结果");
    assert!(second.steps.is_empty());
}

#[test]
fn test_progress_steps_merge_updates_and_keep_latest_fifteen() {
    let mut buf = ProgressBuffer::default();
    for id in 1..=16 {
        buf.observe(&event("agent_activity", serde_json::json!({"response_message_id":18,"item_id":id.to_string(),"kind":"file_read","status":"executing","input":{"path":id.to_string()}})));
    }
    let mcp = event("mcp_tool_call_update", serde_json::json!({"call_id":4,"tool_name":"search","status":"success","parameters":"{\"query\":\"报价\"}","result":"private result"}));
    buf.observe(&mcp);
    let batch = buf.take().unwrap();
    assert_eq!(batch.steps.len(),15);
    assert_eq!(batch.steps[0].1.label,"file_read 3");
    assert_eq!(batch.steps[14].1.label,"search 报价");
    buf.observe(&mcp);
    assert!(buf.take().is_none());
    buf.observe(&event("mcp_tool_call_update", serde_json::json!({"call_id":4,"status":"failed"})));
    let update = buf.take().unwrap().steps;
    assert_eq!(update[0].1.label, "search 报价");
    assert_eq!(update[0].1.status, "failed");
    buf.step("mcp:5".into(),"read".into(),"executing");
    buf.step("mcp:5".into(),"read".into(),"success");
    assert_eq!(buf.take().unwrap().steps.len(),1);
}

#[test]
fn test_clean_summary_strips_wrapping_and_clips() {
    assert_eq!(clean_summary("\n「正在对比三家报价。」\n多余"), "正在对比三家报价");
    assert!(clean_summary(&"长".repeat(80)).ends_with('…'));
    assert_eq!(parse_model("gpt%%7"), Some(("gpt".into(), 7)));
    assert_eq!(parse_model(""), None);
}

#[test]
fn test_report_progress_requires_one_short_sentence() {
    for kind in ["butler_main", "butler_contact", "butler_task", "butler_group_member"] {
        assert!(crate::mcp::butler_tools::is_direct_tool(kind, Some("aipp:agent"), "report_progress"));
    }
    assert!(!crate::mcp::butler_tools::is_direct_tool("chat", Some("aipp:agent"), "report_progress"));
    let check = |text: &str| report_text(&serde_json::json!({"text": text}));
    assert_eq!(check(" 我先核对配置。 ").unwrap(), "我先核对配置。");
    for invalid in [" ", "第一行\n第二行", &"字".repeat(121)] {
        assert!(check(invalid).is_err());
    }
    assert!(report_text(&serde_json::json!({})).is_err());
}

#[test]
fn test_progress_snapshot_keeps_reports_and_helper_for_current_epoch() {
    let entry = |source, epoch, text: &str| ButlerProgressSummary {
        source, conversation_id: 42, epoch, text: text.into(), steps: Vec::new(),
    };
    let first = ProgressSnapshot::update(None, entry(ProgressSource::ReportProgress, 1, "开工"));
    let summary = ProgressSnapshot::update(Some(&first), entry(ProgressSource::HelperSummary, 1, "核对中"));
    let next = ProgressSnapshot::update(Some(&summary), entry(ProgressSource::ReportProgress, 1, "继续检查"));
    assert_eq!(summary.reports, ["开工"]);
    assert_eq!(next.reports, ["开工", "继续检查"]);
    assert_eq!(next.helper.unwrap().text, "核对中");
    let fresh = ProgressSnapshot::update(Some(&summary), entry(ProgressSource::ReportProgress, 2, "新任务"));
    assert_eq!(fresh.reports, ["新任务"]);
    assert!(fresh.helper.is_none());
}

use super::*;

fn display_config(key: &str, value: &str) -> HashMap<String, HashMap<String, FeatureConfig>> {
    HashMap::from([(
        "display".to_string(),
        HashMap::from([(
            key.to_string(),
            FeatureConfig {
                id: None,
                feature_code: "display".to_string(),
                key: key.to_string(),
                value: value.to_string(),
                data_type: "boolean".to_string(),
                description: None,
            },
        )]),
    )])
}

#[test]
fn test_approval_notification_enabled_by_default() {
    assert!(notification_enabled(&HashMap::new()));
    assert!(notification_enabled(&display_config("notification_on_permission", "true")));
    assert!(!notification_enabled(&display_config("notification_on_permission", "false")));
    // 完成通知开关与审批通知互不影响。
    assert!(notification_enabled(&display_config("notification_on_completion", "false")));
}

#[test]
fn test_approval_book_hold_and_release() {
    let mut book = AttentionBook::default();
    assert!(book.hold(10, 1));
    assert!(!book.hold(10, 1), "重复登记同一调用不再触发变化");
    assert!(book.hold(11, 1));
    assert_eq!(book.release(10), Some(1));
    assert_eq!(book.release(10), None, "没在等待确认的调用离开 pending 不触发变化");
    assert_eq!(book.held_calls(), vec![(11, 1)]);
}

#[test]
fn test_approval_surface_distinguishes_dialogs_and_tool_cards() {
    let mut book = AttentionBook::default();
    // 管家窗口为所有管家会话弹审批，但消息流只显示当前联系人。
    book.set_surface("butler_experiment", ApprovalSurface { conversations: vec![3], permissions: vec![1, 3, 7] });
    book.set_surface("chat_ui", ApprovalSurface { conversations: vec![20], permissions: vec![20] });

    assert_eq!(book.windows_showing(ApprovalKind::Permission, 7), vec!["butler_experiment".to_string()]);
    assert!(book.windows_showing(ApprovalKind::ToolCall, 7).is_empty());
    assert_eq!(book.windows_showing(ApprovalKind::ToolCall, 3), vec!["butler_experiment".to_string()]);
    assert_eq!(book.windows_showing(ApprovalKind::ToolCall, 20), vec!["chat_ui".to_string()]);
    assert!(book.windows_showing(ApprovalKind::Permission, 21).is_empty());

    // 切走会话后报告空范围，窗口不再算作展示了审批。
    book.set_surface("chat_ui", ApprovalSurface::default());
    assert!(book.windows_showing(ApprovalKind::ToolCall, 20).is_empty());
}

#[test]
fn test_approval_notify_cooldown_per_conversation() {
    let mut book = AttentionBook::default();
    let start = Instant::now();
    assert!(book.take_notify_slot(1, start));
    assert!(!book.take_notify_slot(1, start + Duration::from_secs(3)), "同一轮的多个审批只提醒一次");
    assert!(book.take_notify_slot(2, start + Duration::from_secs(3)), "其他会话不受影响");
    assert!(book.take_notify_slot(1, start + NOTIFY_COOLDOWN + Duration::from_secs(1)));
}

#[test]
fn test_approval_permission_detail_formats_each_source() {
    let operation = serde_json::json!({"request_id":"r","operation":"write_file","path":"C:/work/a.txt","conversation_id":1});
    assert_eq!(permission_detail(OPERATION_PERMISSION_REQUEST_EVENT, &operation), "写入文件 C:/work/a.txt");

    let admin_path = serde_json::json!({"操作":"删除助手","动作":"assistant.delete"}).to_string();
    let admin = serde_json::json!({"operation":"superadmin_execute","path":admin_path});
    assert_eq!(permission_detail(OPERATION_PERMISSION_REQUEST_EVENT, &admin), "管理操作：删除助手");

    let acp = serde_json::json!({"request_id":"r","tool_call_id":"t","title":"Run npm test","options":[]});
    assert_eq!(permission_detail(ACP_PERMISSION_REQUEST_EVENT, &acp), "请求权限：Run npm test");
    let untitled = serde_json::json!({"request_id":"r","tool_call_id":"t","options":[]});
    assert_eq!(permission_detail(ACP_PERMISSION_REQUEST_EVENT, &untitled), "请求工具权限");
}

#[test]
fn test_approval_tool_call_detail_reflects_review_outcome() {
    assert_eq!(tool_call_detail("execute_bash", None), "execute_bash 等待你确认执行");
    assert_eq!(
        tool_call_detail("execute_bash", Some(("risky", "会删除工作区外的文件"))),
        "execute_bash：自动审核未通过，会删除工作区外的文件"
    );
    assert_eq!(
        tool_call_detail("execute_bash", Some(("error", "timeout"))),
        "execute_bash：自动审核失败，等待你确认"
    );
}

#[test]
fn test_approval_only_permission_events_count() {
    assert!(is_permission_request_event(OPERATION_PERMISSION_REQUEST_EVENT));
    assert!(is_permission_request_event(ACP_PERMISSION_REQUEST_EVENT));
    assert!(!is_permission_request_event(crate::api::operation_api::ACP_ELICITATION_REQUEST_EVENT));
    assert!(is_permission_resolved_event(ACP_PERMISSION_RESOLVED_EVENT));
    assert!(!is_permission_resolved_event(crate::api::operation_api::ACP_ELICITATION_RESOLVED_EVENT));
}

#[test]
fn test_approval_truncate_collapses_whitespace() {
    assert_eq!(truncate_chars("a\n  b", 10), "a b");
    let long = "审".repeat(100);
    let truncated = truncate_chars(&long, 20);
    assert_eq!(truncated.chars().count(), 20);
    assert!(truncated.ends_with("..."));
}

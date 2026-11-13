use super::*;

fn row(kind: &str, content: &str) -> PreviewRow {
    PreviewRow {
        message_id: 42,
        conversation_id: 7,
        conversation_kind: kind.into(),
        conversation_name: "整理周报".into(),
        assistant_id: Some(2),
        message_type: "response".into(),
        content: content.into(),
        origin: None,
        created_time: "2026-11-12 08:00:00".into(),
        archived: false,
        scheduled_task_name: None,
    }
}

#[test]
fn test_viewer_allowed_only_in_butler_private_conversations() {
    for kind in ["butler_main", "butler_contact", "butler_task"] {
        assert!(viewer_allowed(kind).is_ok(), "{kind}");
    }
    assert!(viewer_allowed("butler_group_member").unwrap_err().contains("群聊"));
    assert!(viewer_allowed("normal").unwrap_err().contains("只在管家"));
    assert!(viewer_allowed("scheduled_run").is_err());
}

#[test]
fn test_display_content_strips_quote_marker_and_tool_calls() {
    let content = "[[QUOTE:帮我写周报]]\n周报如下：\n- 完成 A\n<!-- MCP_TOOL_CALL:{\"tool_name\":\"preview_file\"} -->";
    assert_eq!(display_content(&row("butler_contact", content)).unwrap(), "周报如下：\n- 完成 A");
}

#[test]
fn test_display_content_rejects_silent_or_tool_only_replies() {
    assert!(display_content(&row("butler_main", " [[NO_REPLY]] ")).unwrap_err().contains("静默回复"));
    let tool_only = "<!-- MCP_TOOL_CALL:{\"tool_name\":\"search_web\"} -->";
    assert!(display_content(&row("butler_main", tool_only)).unwrap_err().contains("只有工具调用"));
}

#[test]
fn test_source_label_names_where_the_reply_came_from() {
    assert_eq!(source_label(&row("butler_contact", ""), "写作"), "与写作的私聊");
    assert_eq!(source_label(&row("butler_contact_archive", ""), "写作"), "与写作的私聊（已归档）");
    assert_eq!(source_label(&row("butler_task", ""), "写作"), "独立任务《整理周报》");
    let archived_main = PreviewRow { archived: true, ..row("butler_main", "") };
    assert_eq!(source_label(&archived_main, "管家"), "总管家旧主会话");
    let scheduled = PreviewRow { scheduled_task_name: Some("日报".into()), ..row("scheduled_run", "") };
    assert_eq!(source_label(&scheduled, "写作"), "定时任务《日报》");
}

#[test]
fn test_excerpt_flattens_whitespace_and_truncates_by_chars() {
    assert_eq!(excerpt("第一行\n\n第二行"), "第一行 第二行");
    let long = "测".repeat(EXCERPT_CHARS + 5);
    let cut = excerpt(&long);
    assert_eq!(cut.chars().count(), EXCERPT_CHARS + 1);
    assert!(cut.ends_with('…'));
}

#[test]
fn test_message_id_arg_accepts_numeric_strings() {
    assert_eq!(message_id_arg(&json!({"message_id": 12})).unwrap(), Some(12));
    assert_eq!(message_id_arg(&json!({"message_id": "#34"})).unwrap(), Some(34));
    assert_eq!(message_id_arg(&json!({"work_ref": "task:1"})).unwrap(), None);
    assert!(message_id_arg(&json!({"message_id": "abc"})).is_err());
}

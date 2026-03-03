#[cfg(test)]
mod tests {
    use crate::api::scheduled_task_api::*;

    // ── normalize_tool_arguments ─────────────────────────────────────

    #[test]
    fn test_normalize_tool_arguments_object() {
        let val = serde_json::json!({"key": "value", "num": 42});
        let result = normalize_tool_arguments(&val);
        assert!(result.contains("\"key\""));
        assert!(result.contains("\"value\""));
        assert!(result.contains("42"));
    }

    #[test]
    fn test_normalize_tool_arguments_non_object() {
        assert_eq!(normalize_tool_arguments(&serde_json::json!("string")), "{}");
        assert_eq!(normalize_tool_arguments(&serde_json::json!(42)), "{}");
        assert_eq!(normalize_tool_arguments(&serde_json::json!(null)), "{}");
        assert_eq!(normalize_tool_arguments(&serde_json::json!([1, 2])), "{}");
    }

    #[test]
    fn test_normalize_tool_arguments_empty_object() {
        let val = serde_json::json!({});
        assert_eq!(normalize_tool_arguments(&val), "{}");
    }

    // ── parse_notify_bool_value ──────────────────────────────────────

    #[test]
    fn test_parse_notify_bool_value_booleans() {
        assert_eq!(parse_notify_bool_value(&serde_json::json!(true)), Some(true));
        assert_eq!(parse_notify_bool_value(&serde_json::json!(false)), Some(false));
    }

    #[test]
    fn test_parse_notify_bool_value_integers() {
        assert_eq!(parse_notify_bool_value(&serde_json::json!(1)), Some(true));
        assert_eq!(parse_notify_bool_value(&serde_json::json!(0)), Some(false));
        assert_eq!(parse_notify_bool_value(&serde_json::json!(-1)), Some(true));
    }

    #[test]
    fn test_parse_notify_bool_value_strings() {
        assert_eq!(parse_notify_bool_value(&serde_json::json!("true")), Some(true));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("false")), Some(false));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("yes")), Some(true));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("no")), Some(false));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("是")), Some(true));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("否")), Some(false));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("需要通知")), Some(true));
        assert_eq!(parse_notify_bool_value(&serde_json::json!("不通知")), Some(false));
    }

    #[test]
    fn test_parse_notify_bool_value_unknown() {
        assert_eq!(parse_notify_bool_value(&serde_json::json!("maybe")), None);
        assert_eq!(parse_notify_bool_value(&serde_json::json!(null)), None);
    }

    // ── normalize_task_state_value ───────────────────────────────────

    #[test]
    fn test_normalize_task_state_completed_variants() {
        for v in ["completed", "complete", "done", "finished", "success", "succeeded", "结束", "已结束", "已完成"] {
            assert_eq!(normalize_task_state_value(v), Some("completed".to_string()), "failed for '{}'", v);
        }
    }

    #[test]
    fn test_normalize_task_state_running_variants() {
        for v in ["running", "in_progress", "pending", "processing", "进行中", "未结束"] {
            assert_eq!(normalize_task_state_value(v), Some("running".to_string()), "failed for '{}'", v);
        }
    }

    #[test]
    fn test_normalize_task_state_failed_variants() {
        for v in ["failed", "error", "失败", "异常"] {
            assert_eq!(normalize_task_state_value(v), Some("failed".to_string()), "failed for '{}'", v);
        }
    }

    #[test]
    fn test_normalize_task_state_case_insensitive() {
        assert_eq!(normalize_task_state_value("COMPLETED"), Some("completed".to_string()));
        assert_eq!(normalize_task_state_value("Running"), Some("running".to_string()));
        assert_eq!(normalize_task_state_value(" Failed "), Some("failed".to_string()));
    }

    #[test]
    fn test_normalize_task_state_unknown() {
        assert_eq!(normalize_task_state_value("unknown"), None);
        assert_eq!(normalize_task_state_value(""), None);
    }

    // ── parse_notify_decision ────────────────────────────────────────

    #[test]
    fn test_parse_notify_decision_completed_with_notify() {
        let raw = r#"{"task_state":"completed","notify":true,"summary":"任务完成","reason":"有重要信息"}"#;
        let result = parse_notify_decision(raw).unwrap();
        assert_eq!(result.task_state, Some("completed".to_string()));
        assert!(result.notify);
        assert_eq!(result.summary, Some("任务完成".to_string()));
        assert_eq!(result.reason, Some("有重要信息".to_string()));
    }

    #[test]
    fn test_parse_notify_decision_completed_no_notify() {
        let raw = r#"{"task_state":"completed","notify":false,"summary":"","reason":"无重要内容"}"#;
        let result = parse_notify_decision(raw).unwrap();
        assert!(!result.notify);
        assert_eq!(result.summary, None); // empty string filtered to None
    }

    #[test]
    fn test_parse_notify_decision_running_rejected() {
        let raw = r#"{"task_state":"running","notify":false,"summary":"","reason":"任务进行中"}"#;
        let result = parse_notify_decision(raw);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("running"));
    }

    #[test]
    fn test_parse_notify_decision_notify_true_without_summary_rejected() {
        let raw = r#"{"task_state":"completed","notify":true,"summary":"","reason":"test"}"#;
        let result = parse_notify_decision(raw);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("summary"));
    }

    #[test]
    fn test_parse_notify_decision_markdown_json_block() {
        let raw = r#"好的，以下是判定结果：
```json
{"task_state":"completed","notify":true,"summary":"天气数据已获取","reason":"包含重要信息"}
```"#;
        let result = parse_notify_decision(raw).unwrap();
        assert!(result.notify);
        assert_eq!(result.summary, Some("天气数据已获取".to_string()));
    }

    #[test]
    fn test_parse_notify_decision_alternative_field_names() {
        let raw = r#"{"status":"done","should_notify":true,"message":"完成了","reason":"ok"}"#;
        let result = parse_notify_decision(raw).unwrap();
        assert_eq!(result.task_state, Some("completed".to_string()));
        assert!(result.notify);
        assert_eq!(result.summary, Some("完成了".to_string()));
    }

    #[test]
    fn test_parse_notify_decision_invalid_json() {
        let raw = "这不是 JSON";
        assert!(parse_notify_decision(raw).is_err());
    }

    // ── AgenticLoopStatus Display ────────────────────────────────────

    #[test]
    fn test_agentic_loop_status_display() {
        assert_eq!(format!("{}", AgenticLoopStatus::Completed), "completed");
        assert_eq!(format!("{}", AgenticLoopStatus::MaxRoundsReached), "max_rounds_reached");
        assert_eq!(format!("{}", AgenticLoopStatus::Timeout), "timeout");
        assert_eq!(format!("{}", AgenticLoopStatus::Error("test".into())), "error: test");
    }

    // ── parse_local_datetime ─────────────────────────────────────────

    #[test]
    fn test_parse_local_datetime_rfc3339() {
        let result = parse_local_datetime("2024-01-15T10:30:00+08:00");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_local_datetime_common_formats() {
        assert!(parse_local_datetime("2024-01-15 10:30:00").is_ok());
        assert!(parse_local_datetime("2024-01-15 10:30").is_ok());
        assert!(parse_local_datetime("2024-01-15T10:30:00").is_ok());
        assert!(parse_local_datetime("2024-01-15T10:30").is_ok());
    }

    #[test]
    fn test_parse_local_datetime_invalid() {
        assert!(parse_local_datetime("not a date").is_err());
        assert!(parse_local_datetime("").is_err());
    }
}

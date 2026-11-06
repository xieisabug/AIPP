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

    #[test]
    fn test_build_mcp_tool_call_ui_hint_with_call_id() {
        let hint = build_mcp_tool_call_ui_hint(
            "zentao",
            "get_product_bugs",
            r#"{"product_id":4}"#,
            Some(123),
            "llm-call-1",
        );
        assert!(hint.contains("\"call_id\":123"));
        assert!(hint.contains("\"llm_call_id\":\"llm-call-1\""));
    }

    #[test]
    fn test_build_mcp_tool_call_ui_hint_without_call_id() {
        let hint = build_mcp_tool_call_ui_hint(
            "zentao",
            "get_product_bugs",
            r#"{"product_id":4}"#,
            None,
            "llm-call-2",
        );
        assert!(!hint.contains("\"call_id\""));
        assert!(hint.contains("\"llm_call_id\":\"llm-call-2\""));
    }

    #[test]
    fn test_extract_prompt_tool_calls_basic() {
        let raw = r#"我来帮你查看项目 bug。
<mcp_tool_call>
  <server_name>zentao</server_name>
  <tool_name>get_product_bugs</tool_name>
  <parameters>{"product_id":4,"limit":100}</parameters>
</mcp_tool_call>"#;

        let (calls, sanitized) = extract_prompt_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].fn_name, "zentao__get_product_bugs");
        assert_eq!(calls[0].fn_arguments["product_id"], 4);
        assert_eq!(calls[0].fn_arguments["limit"], 100);
        assert!(!calls[0].call_id.is_empty());
        assert!(!sanitized.contains("<mcp_tool_call>"));
        assert!(!sanitized.contains("</mcp_tool_call>"));
    }

    #[test]
    fn test_extract_prompt_tool_calls_invalid_parameters_fallback_to_empty_object() {
        let raw = r#"<mcp_tool_call>
  <server_name>zentao</server_name>
  <tool_name>get_product_bugs</tool_name>
  <parameters>not-json</parameters>
</mcp_tool_call>"#;

        let (calls, sanitized) = extract_prompt_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].fn_arguments, serde_json::json!({}));
        assert!(!sanitized.contains("<mcp_tool_call>"));
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
        for v in [
            "completed",
            "complete",
            "done",
            "finished",
            "success",
            "succeeded",
            "结束",
            "已结束",
            "已完成",
        ] {
            assert_eq!(
                normalize_task_state_value(v),
                Some("completed".to_string()),
                "failed for '{}'",
                v
            );
        }
    }

    #[test]
    fn test_normalize_task_state_running_variants() {
        for v in ["running", "in_progress", "pending", "processing", "进行中", "未结束"] {
            assert_eq!(
                normalize_task_state_value(v),
                Some("running".to_string()),
                "failed for '{}'",
                v
            );
        }
    }

    #[test]
    fn test_normalize_task_state_failed_variants() {
        for v in ["failed", "error", "失败", "异常"] {
            assert_eq!(
                normalize_task_state_value(v),
                Some("failed".to_string()),
                "failed for '{}'",
                v
            );
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

    #[test]
    fn test_compute_next_run_at_interval_requires_explicit_unit_and_value() {
        let base_time = chrono::Utc::now();
        let result = compute_next_run_at_with_config(
            ScheduleConfig {
                schedule_type: "interval",
                interval_value: None,
                interval_unit: None,
                start_time: Some("09:00"),
                week_days: Some(vec![1, 2, 3, 4, 5]),
                month_days: None,
                run_at: None,
            },
            base_time,
        );

        let err = result.expect_err("expected validation error");
        assert!(err.contains("interval_value"));
        assert!(err.contains("interval_unit"));
        assert!(err.contains("week_days"));
        assert!(err.contains("interval_unit='week'"));
    }

    #[test]
    fn test_compute_next_run_at_week_days_rejects_non_week_unit() {
        let base_time = chrono::Utc::now();
        let result = compute_next_run_at_with_config(
            ScheduleConfig {
                schedule_type: "interval",
                interval_value: Some(1),
                interval_unit: Some("day"),
                start_time: Some("09:00"),
                week_days: Some(vec![1, 2, 3, 4, 5]),
                month_days: None,
                run_at: None,
            },
            base_time,
        );

        let err = result.expect_err("expected validation error");
        assert!(err.contains("week_days"));
        assert!(err.contains("interval_unit='week'"));
    }
}

#[cfg(test)]
mod run_outcome_tests {
    use crate::api::scheduled_task_api::*;
    use crate::db::scheduled_task_db::ScheduledTask;
    use chrono::Utc;
    use std::time::{Duration, Instant};
    use tokio_util::sync::CancellationToken;

    fn task(prompt: &str) -> ScheduledTask {
        ScheduledTask {
            id: 7, name: "日报".into(), is_enabled: false, schedule_type: "once".into(),
            interval_value: None, interval_unit: None, start_time: None, week_days: None,
            month_days: None, run_at: None, next_run_at: None, last_run_at: None,
            assistant_id: 1, task_prompt: prompt.into(), notify_prompt: String::new(),
            butler_conversation_id: Some(1), created_time: Utc::now(), updated_time: Utc::now(),
        }
    }

    #[test]
    fn test_loop_status_failure_maps_every_unfinished_reason() {
        assert!(loop_status_failure(&AgenticLoopStatus::Completed, 3, 1, 0).is_none());
        let (reason, msg) = loop_status_failure(&AgenticLoopStatus::Timeout, 4, 2, 1).unwrap();
        assert_eq!(reason, "timeout");
        assert!(msg.contains("超时") && msg.contains("无法撤回"));
        assert_eq!(loop_status_failure(&AgenticLoopStatus::MaxRoundsReached, 50, 0, 0).unwrap().0, "max_rounds_reached");
        let (reason, msg) = loop_status_failure(&AgenticLoopStatus::Cancelled, 1, 0, 0).unwrap();
        assert_eq!(reason, "cancelled");
        assert!(!msg.contains("无法撤回"));
        let (reason, msg) = loop_status_failure(&AgenticLoopStatus::Error("LLM 调用失败: 502".into()), 2, 0, 0).unwrap();
        assert_eq!(reason, "error");
        assert!(msg.contains("502"));
    }

    #[test]
    fn test_butler_result_message_reports_real_status_and_is_char_safe() {
        let long_cn = "测".repeat(3000); // 9000 字节，截断点落在多字节字符中间
        let outcome = ScheduledRunOutcome::failed("timeout", "执行超时".into(), Some(long_cn.clone()));
        let msg = build_butler_scheduled_task_result_message(&task(&"任".repeat(100)), "run-1", &outcome, "助手");
        assert!(msg.contains("status=failed"));
        assert!(msg.contains("end_reason=timeout"));
        assert!(msg.contains("error=执行超时"));
        assert!(msg.contains("…(truncated)"));
        let prompt = build_butler_scheduled_task_followup_prompt(&task("x"), &outcome);
        assert!(prompt.contains("超时未完成"));
        assert!(!prompt.contains("执行成功"));
    }

    #[test]
    fn test_scheduled_result_message_found_by_exact_run_id() {
        use crate::db::conversation_db::Message;
        let outcome = ScheduledRunOutcome::failed("timeout", "执行超时".into(), None);
        let content = build_butler_scheduled_task_result_message(&task("x"), "run-1", &outcome, "助手");
        let now = Utc::now();
        let message = |id: i64, content: String| Message {
            id, parent_id: None, conversation_id: 1, message_type: "system".into(), content,
            llm_model_id: None, llm_model_name: None, created_time: now, start_time: None,
            finish_time: Some(now), token_count: 0, input_token_count: 0, output_token_count: 0,
            generation_group_id: None, parent_group_id: None, tool_calls_json: None,
            metadata_json: None, first_token_time: None, ttft_ms: None,
        };
        let messages = vec![message(5, content)];
        assert_eq!(find_scheduled_result_message_id(&messages, "run-1"), Some(5));
        assert_eq!(find_scheduled_result_message_id(&messages, "run-"), None, "不能前缀误配");
        assert_eq!(find_scheduled_result_message_id(&messages, "run-2"), None);
    }

    #[test]
    fn test_scheduled_report_runs_once_per_run_id() {
        let first = claim_scheduled_report("run-dup").expect("first claim");
        assert!(claim_scheduled_report("run-dup").is_none(), "同一运行不并行回报");
        drop(first);
        assert!(claim_scheduled_report("run-dup").is_some(), "结束后可再次回报");
        assert_eq!(scheduled_followup_key("abc"), "run:abc");
    }

    #[test]
    fn test_outcome_from_recorded_run_keeps_real_status() {
        use crate::db::scheduled_task_db::ScheduledTaskRun;
        let run = ScheduledTaskRun {
            id: 1, task_id: 7, run_id: "r".into(), status: "success".into(), notify: true,
            summary: Some("结论".into()), error_message: None, started_time: Utc::now(),
            finished_time: Some(Utc::now()), butler_followup_status: Some("pending".into()),
        };
        let ok = outcome_from_recorded_run(&run);
        assert_eq!((ok.status, ok.end_reason, ok.summary.as_deref()), ("success", "completed", Some("结论")));
        let failed = outcome_from_recorded_run(&ScheduledTaskRun { status: "failed".into(), error_message: Some("超时".into()), ..run });
        assert_eq!((failed.status, failed.error.as_deref()), ("failed", Some("超时")));
    }

    #[tokio::test]
    async fn test_await_llm_or_stop_prefers_cancel_while_waiting_model() {
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        });
        let never = std::future::pending::<()>();
        let result = await_llm_or_stop(never, &token, Instant::now() + Duration::from_secs(30)).await;
        assert!(matches!(result, LlmWait::Cancelled));

        let fresh = CancellationToken::new();
        let result = await_llm_or_stop(std::future::pending::<()>(), &fresh, Instant::now() + Duration::from_millis(10)).await;
        assert!(matches!(result, LlmWait::Timeout));
        let result = await_llm_or_stop(async { 5 }, &fresh, Instant::now() + Duration::from_secs(1)).await;
        assert!(matches!(result, LlmWait::Done(5)));
    }
}

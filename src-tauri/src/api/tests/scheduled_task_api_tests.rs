#[cfg(test)]
mod tests {
    use crate::api::scheduled_task_api::*;
    use crate::db::scheduled_task_db::{ScheduledTask, DELIVER_OWNER_CHAT};
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use std::time::{Duration, Instant};
    use tokio_util::sync::CancellationToken;

    fn task(policy: &str) -> ScheduledTask {
        let now = Utc::now();
        ScheduledTask {
            id: 6, name: "周四休息提醒".into(), is_enabled: true, schedule_expr: "0 16 * * 4".into(),
            schedule_anchor: now, next_run_at: None, last_run_at: None, assistant_id: 40,
            task_prompt: "提醒用户今天休息".into(), deliver_policy: policy.into(), deliver_to: DELIVER_OWNER_CHAT.into(),
            deliver_conversation_id: None, created_in_conversation_id: Some(1520), butler_mode: true, ends_at: None,
            max_runs: None, run_count: 1, missed_policy: "run_once".into(), consecutive_failures: 0,
            paused_reason: None, created_time: now, updated_time: now,
        }
    }

    // ── 运行结果 ─────────────────────────────────────────────────────

    #[test]
    fn test_scheduled_outcome_roundtrip_preserves_complete_result() {
        let outcome = ScheduledRunOutcome {
            status: "delivered".into(),
            end_reason: "completed".into(),
            text: Some("完整成果\n".repeat(3000)),
            error: None,
            conversation_id: Some(30),
            final_message_id: Some(31),
            llm_model_id: Some(9),
            llm_model_name: Some("model-x".into()),
            notify: false,
        };
        let recovered: ScheduledRunOutcome = serde_json::from_str(&serde_json::to_string(&outcome).unwrap()).unwrap();
        assert_eq!(recovered, outcome);
    }

    #[test]
    fn test_scheduled_outcome_reads_previous_version_journals() {
        let legacy = r#"{"status":"success","end_reason":"completed","notify":true,"summary":"s","error":null,
            "detail":"完整产出","judged_task_state":null,"notify_error":null,"conversation_id":5}"#;
        let recovered: ScheduledRunOutcome = serde_json::from_str(legacy).unwrap();
        assert_eq!(recovered.text.as_deref(), Some("完整产出"), "旧字段 detail 映射为 text");
        assert_eq!(recovered.conversation_id, Some(5));
        assert!(recovered.notify);
    }

    #[test]
    fn test_classify_completion_handles_no_reply_policy_and_empty_output() {
        assert_eq!(classify_completion("今天休息，记得拉伸", "always"), ("delivered", "completed"));
        assert_eq!(classify_completion("  [NO_REPLY]  ", "if_needed"), ("quiet", "no_reply"));
        assert_eq!(classify_completion("`[NO_REPLY]`", "always"), ("quiet", "no_reply"));
        assert_eq!(classify_completion("有更新", "never"), ("quiet", "never"));
        assert_eq!(classify_completion("   ", "always"), ("failed", "empty_output"));
        assert_eq!(classify_completion("结果：[NO_REPLY] 之外还有内容", "if_needed").0, "delivered");
    }

    #[test]
    fn test_unattended_contract_explains_delivery_lateness_and_self_pause() {
        let scheduled: DateTime<Utc> = "2026-11-12T08:00:00Z".parse().unwrap();
        let on_time = unattended_contract(&task("always"), 3, scheduled, scheduled + ChronoDuration::seconds(30), "与健身教练的私聊", None);
        assert!(on_time.contains("第 3 次运行") && on_time.contains("与健身教练的私聊") && on_time.contains("每次都会发送"));
        assert!(on_time.contains("pause") && on_time.contains("task_id=6"));
        assert!(!on_time.contains("晚了"));
        let late = unattended_contract(&task("if_needed"), 3, scheduled, scheduled + ChronoDuration::hours(2), "x", Some("上一次运行：已发送"));
        assert!(late.contains("晚了约 120 分钟") && late.contains(NO_REPLY));
        assert!(late.contains("上一次运行：已发送"));
        assert!(late.contains("用户此刻不在场"));
    }

    // ── 工具调用辅助 ─────────────────────────────────────────────────

    #[test]
    fn test_normalize_tool_arguments() {
        let val = serde_json::json!({"key": "value", "num": 42});
        let result = normalize_tool_arguments(&val);
        assert!(result.contains("\"key\"") && result.contains("42"));
        for value in [serde_json::json!("string"), serde_json::json!(42), serde_json::json!(null), serde_json::json!([1, 2])] {
            assert_eq!(normalize_tool_arguments(&value), "{}");
        }
        assert_eq!(normalize_tool_arguments(&serde_json::json!({})), "{}");
    }

    #[test]
    fn test_build_mcp_tool_call_ui_hint() {
        let hint = build_mcp_tool_call_ui_hint("zentao", "get_product_bugs", r#"{"product_id":4}"#, Some(123), "llm-call-1");
        assert!(hint.contains("\"call_id\":123") && hint.contains("\"llm_call_id\":\"llm-call-1\""));
        let hint = build_mcp_tool_call_ui_hint("zentao", "get_product_bugs", r#"{"product_id":4}"#, None, "llm-call-2");
        assert!(!hint.contains("\"call_id\"") && hint.contains("\"llm_call_id\":\"llm-call-2\""));
    }

    #[test]
    fn test_extract_prompt_tool_calls() {
        let raw = r#"我来帮你查看项目 bug。
<mcp_tool_call>
  <server_name>zentao</server_name>
  <tool_name>get_product_bugs</tool_name>
  <parameters>{"product_id":4,"limit":100}</parameters>
</mcp_tool_call>"#;
        let (calls, sanitized) = extract_prompt_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].fn_name, "zentao__get_product_bugs");
        assert_eq!(calls[0].fn_arguments["limit"], 100);
        assert!(!sanitized.contains("<mcp_tool_call>"));
        let (calls, _) = extract_prompt_tool_calls("<mcp_tool_call><server_name>a</server_name><tool_name>b</tool_name><parameters>not-json</parameters></mcp_tool_call>");
        assert_eq!(calls[0].fn_arguments, serde_json::json!({}));
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
        let (reason, msg) = loop_status_failure(&AgenticLoopStatus::Error("模型调用失败: 502".into()), 2, 0, 0).unwrap();
        assert_eq!(reason, "error");
        assert!(msg.contains("502"));
        assert_eq!(format!("{}", AgenticLoopStatus::Error("test".into())), "error: test");
    }

    #[tokio::test]
    async fn test_await_llm_or_stop_prefers_cancel_while_waiting_model() {
        let token = CancellationToken::new();
        let cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel.cancel();
        });
        let result = await_llm_or_stop(std::future::pending::<()>(), &token, Instant::now() + Duration::from_secs(30)).await;
        assert!(matches!(result, LlmWait::Cancelled));
        let fresh = CancellationToken::new();
        let result = await_llm_or_stop(std::future::pending::<()>(), &fresh, Instant::now() + Duration::from_millis(10)).await;
        assert!(matches!(result, LlmWait::Timeout));
        let result = await_llm_or_stop(async { 5 }, &fresh, Instant::now() + Duration::from_secs(1)).await;
        assert!(matches!(result, LlmWait::Done(5)));
    }
}

// Prompt contracts: these tests do not claim that a live model follows the instructions.
// 完整的汇报约定只在系统提示词里出现一次；回调只给事实加一句简短引用。
fn assert_callback_contract(prompt: &str) {
    assert!(prompt.contains(super::CALLBACK_GUIDANCE));
    assert!(prompt.contains("不能因等待其他结果而隐瞒"));
    assert!(!prompt.contains("## 汇报约定"), "回调不再重复整段约定");
    assert!(!prompt.contains("全部完成后再一次性向用户交付"));
    assert!(!prompt.contains("结果回来后会汇总"));
}

#[test]
fn reporting_system_prompt_holds_the_full_policy_once() {
    let prompt = super::BUTLER_SYSTEM_PROMPT_BASE.replace("{BUTLER_NAME}", "总管家");
    for phrase in [
        "用户主动提问",
        "原请求如果还没有任何人交付",
        "不能把部分完成说成全部完成",
        "未经用户明确允许，不换成员",
        "`<scheduled_task_approval>`",
        "manual_review_required",
        "`<current_time>`",
        "`continue_work`",
        "`read_work_context`",
        "`preview_message`",
    ] {
        assert!(prompt.contains(phrase), "缺少：{phrase}");
    }
    assert!(!prompt.contains("{REPORTING_POLICY}") && !prompt.contains("!cd"), "日期改为每轮注入");
    assert!(!prompt.contains("全部完成→验收后汇总交付"));
}

#[test]
fn reporting_single_result_preserves_direct_delivery_failure_and_cancellation() {
    for direct in [true, false] {
        for status in [STATUS_SUCCEEDED, STATUS_FAILED, STATUS_CANCELLED] {
            for cancelled in [true, false] {
                let task = build_task_list_item(10, status, "测试工作");
                let prompt = super::build_butler_task_result_followup_prompt(&task, cancelled, direct);
                assert_callback_contract(&prompt);
                assert_eq!(prompt.contains("直接交付给用户，不要转述"), direct && status == STATUS_SUCCEEDED);
                assert_eq!(prompt.contains("错误与修复"), status != STATUS_SUCCEEDED);
                assert_eq!(prompt.contains("伴随过取消请求"), cancelled);
                assert_eq!(
                    prompt.contains("`preview_message`（work_ref=task:10）"),
                    !direct && status == STATUS_SUCCEEDED,
                    "只有交回给总管家的成功结果才引导原样展示"
                );
            }
        }
    }
}

#[test]
fn reporting_batches_distinguish_delivered_results_from_pending_delivery() {
    for direct_flags in [[true, true], [true, false], [false, false]] {
        for statuses in [[STATUS_SUCCEEDED, STATUS_SUCCEEDED], [STATUS_SUCCEEDED, STATUS_FAILED]] {
            let batch: Vec<_> = direct_flags.iter().zip(statuses).enumerate().map(|(i, (direct, status))| {
                let id = 10 + i as i64;
                let mut definition = build_task_definition(id);
                definition.result_handling_mode = Some(if *direct { RESULT_MODE_DIRECT_REPORT } else { RESULT_MODE_NOTIFY_ONLY }.to_string());
                FollowupBatchEntry {
                    task: build_task_list_item(id, status, &format!("工作{i}")),
                    definition, result: build_task_result(id),
                    cancel_requested: i == 1, inject_system_message: true,
                    handoff_message_id: None, claim: test_claim(),
                }
            }).collect();
            let prompt = build_batched_followup_prompt(&batch);
            assert_callback_contract(&prompt);
            assert!(prompt.contains("工作0") && prompt.contains("工作1"));
            let delivered = direct_flags.iter().zip(statuses).filter(|(direct, status)| **direct && *status == STATUS_SUCCEEDED).count();
            assert_eq!(prompt.matches("已由执行助手直接交付给用户").count(), delivered);
            assert_eq!(prompt.matches("伴随取消请求").count(), 1);
        }
    }
}

#[test]
fn reporting_member_prompts_share_core_and_keep_user_intervention_exception() {
    let core = super::BUTLER_MEMBER_CORE_PROMPT;
    assert!(core.contains("还有其他协作未返回且无需用户介入时"));
    assert!(core.contains("无法自行解决的错误与阻塞时应及时说明"));
    assert!(core.contains("不能代替用户授权") && core.contains("未经用户允许的内容"));
    assert!(core.contains("配置修复由总管家或用户决定"), "成员不自行修复配置");
    assert!(core.contains("生成真实文件"), "成员直接交付，也要遵守交付格式");
    assert!(core.contains("`preview_message`") && core.contains("不要转述"), "原样展示其他成员的答复");
    assert!(super::BUTLER_MEMBER_TASK_PROMPT.contains("等待答复时最终回复只写"));
    assert!(super::BUTLER_MEMBER_TASK_PROMPT.contains("默认是内部协作"));
    for specific in [super::BUTLER_MEMBER_CONTACT_PROMPT, super::BUTLER_MEMBER_TASK_PROMPT] {
        assert!(!specific.contains("不能代替用户授权"), "通用约定只写在共享部分");
    }
}

#[test]
fn reporting_attention_prompt_skips_pure_questions_without_reading_progress() {
    let task = build_task_list_item(10, super::STATUS_RUNNING, "测试工作");
    let prompt = super::build_butler_task_attention_followup_prompt(&task, "permission_requested");
    assert_callback_contract(&prompt);
    assert!(prompt.contains("只有成员向用户的提问时通常不处理"));
    assert!(prompt.contains("不查看或评论进度"));
    assert!(!prompt.contains("最新消息"), "不再既要求查看最新消息又禁止阅读任务内容");
}

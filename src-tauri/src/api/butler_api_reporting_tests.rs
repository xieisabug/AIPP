// Prompt contracts: these tests do not claim that a live model follows the instructions.
fn assert_reporting_contract(prompt: &str) {
    assert!(prompt.contains(super::REPORTING_POLICY));
    assert!(prompt.contains("用户主动提问"));
    assert!(prompt.contains("原请求仍无人交付"));
    assert!(prompt.contains("不能为了等待其他结果而隐瞒"));
    assert!(prompt.contains("未经用户明确允许不得换助手或自行代答"));
    assert!(!prompt.contains("全部完成后再一次性向用户交付"));
    assert!(!prompt.contains("结果回来后会汇总"));
}

#[test]
fn reporting_system_prompt_has_shared_policy_and_no_mandatory_summary() {
    let prompt = super::BUTLER_SYSTEM_PROMPT_BASE
        .replace("{BUTLER_NAME}", "总管家")
        .replace("{REPORTING_POLICY}", super::REPORTING_POLICY);
    assert_reporting_contract(&prompt);
    assert!(!prompt.contains("{REPORTING_POLICY}"));
    assert!(!prompt.contains("全部完成→验收后汇总交付"));
}

#[test]
fn reporting_single_result_preserves_direct_delivery_failure_and_cancellation() {
    for direct in [true, false] {
        for status in [STATUS_SUCCEEDED, STATUS_FAILED, STATUS_CANCELLED] {
            for cancelled in [true, false] {
                let task = build_task_list_item(10, status, "测试工作");
                let prompt = super::build_butler_task_result_followup_prompt(&task, cancelled, direct);
                assert_reporting_contract(&prompt);
                assert_eq!(prompt.contains("结果已经以"), direct && status == STATUS_SUCCEEDED);
                assert_eq!(prompt.contains("伴随过取消请求"), cancelled);
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
            assert_reporting_contract(&prompt);
            assert!(prompt.contains("工作0") && prompt.contains("工作1"));
            let delivered = direct_flags.iter().zip(statuses).filter(|(direct, status)| **direct && *status == STATUS_SUCCEEDED).count();
            assert_eq!(prompt.matches("结果已由执行助手直接展示给用户").count(), delivered);
            assert_eq!(prompt.matches("伴随取消请求").count(), 1);
        }
    }
}

#[test]
fn reporting_member_wait_guidance_keeps_user_intervention_exception() {
    assert!(super::BUTLER_MEMBER_CONTACT_PROMPT.contains("还有其他协作未返回且无需用户介入时"));
    assert!(super::BUTLER_MEMBER_CONTACT_PROMPT.contains("错误与阻塞应及时说明"));
    assert!(super::BUTLER_MEMBER_TASK_PROMPT.contains("等待答复且无需用户介入时"));
}

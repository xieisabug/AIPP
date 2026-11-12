#[test]
fn reporting_peer_callbacks_use_short_guidance_without_changing_request_delivery() {
    for direct in [false, true] {
        let ex = Exchange {
            report_to_user: direct, reply: Some("结果".into()),
            ..exchange("returning", false, None)
        };
        let callback = build_delivery_prompt(&ex, false, None, None).unwrap();
        assert!(callback.contains(super::CALLBACK_GUIDANCE));
        assert!(!callback.contains("总管家可用"), "回调可能发给成员，不能写总管家专属操作");
        assert_eq!(callback.contains("已由对方直接答复用户"), direct);
        let request = build_delivery_prompt(&ex, true, None, None).unwrap();
        assert!(!request.contains(super::CALLBACK_GUIDANCE), "协作请求仍须正常答复");
        assert_eq!(request.contains("本轮最终回复只回送给"), !direct);
    }
}

#[test]
fn reporting_nested_help_keeps_parent_target_and_user_action_exceptions() {
    let help = Exchange { reply: Some("意见".into()), ..exchange("returning", false, None) };
    for direct in [false, true] {
        let parent = Exchange { report_to_user: direct, ..exchange("waiting", false, None) };
        let prompt = build_delivery_prompt(&help, false, Some(&parent), None).unwrap();
        assert!(prompt.contains("给出该工作的最终交付"));
        assert!(prompt.contains("需要用户决策、授权或无法自行解决的阻塞及时说明"));
        assert_eq!(prompt.contains("最终交付会以你的名义展示给用户"), direct);
        assert_eq!(prompt.contains("最终交付只回送给"), !direct);
    }
}

#[test]
fn reporting_failure_and_stop_receipts_do_not_hide_blockers_while_waiting() {
    for (state, stopped) in [("failed", false), ("stopped", true)] {
        let prompt = build_receipt_prompt(&exchange(state, stopped, Some("通道不可用")), None);
        assert!(prompt.contains(super::CALLBACK_GUIDANCE));
        assert!(prompt.contains("不能因等待其他结果而隐瞒"));
        assert!(prompt.contains(if stopped { "不要重新派发" } else { "先征得用户同意" }));
        assert!(!prompt.contains("superadmin_execute"), "配置修复的具体做法只写在总管家系统提示词里");
    }
}

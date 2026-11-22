use super::*;

#[test]
fn test_group_relay_review_requires_an_explicit_verdict_and_reason_to_pause() {
    let review = RelayReview {
        origin: message(1, None, "完成复测"),
        history: vec![message(4, None, "请复测"), message(9, None, "请复测")],
        target: "测试".into(),
    };
    assert_eq!(parse_loop_review(r#"{"looping":false,"reason":"新版本待复测"}"#, &review), Ok(None));
    assert_eq!(parse_loop_review(r#"{"looping":true,"reason":"反复要求复测同一版本","repeated_seq":4}"#, &review),
        Ok(Some("反复要求复测同一版本（参见群消息 #4）".into())));
    for seq in [7, 9] {
        let text = serde_json::json!({"looping":true,"reason":"重复交接","repeated_seq":seq}).to_string();
        assert!(parse_loop_review(&text, &review).is_err());
    }
    assert!(parse_loop_review(r#"{"looping":true,"reason":" "}"#, &review).is_err());
    assert!(parse_loop_review("可能在循环", &review).is_err());
}

fn member(id: i64, name: &str) -> Member {
    Member { id, group_id: 1, assistant_id: id + 100, name: name.into(), conversation_id: id + 200, joined_after: 0, delivered_through: 0, active: true }
}

fn message(seq: i64, sender: Option<&Member>, body: &str) -> Message {
    Message {
        group_id: 1, seq, sender_member: sender.map(|m| m.id), sender_assistant: sender.map(|m| m.assistant_id),
        sender_name: sender.map(|m| m.name.clone()).unwrap_or_else(|| "我".into()), kind: "message".into(),
        body: body.into(), reply_to: None, created_at: String::new(), source_conversation_id: None, source_message_id: None,
    }
}

#[test]
fn test_group_router_parses_names_and_ignores_unknown() {
    let members = [member(1, "写作助手"), member(2, "Coder")];
    let candidates = members.iter().map(|m| Candidate { member: m, entry: None }).collect::<Vec<_>>();
    assert_eq!(parse_plan("```json\n{\"responders\":[\"@coder\",\"写作助手\",\"不存在\"]}\n```", &candidates, None).unwrap(),
        Plan { responders: vec![2, 1], rounds: 1 });
    assert!(parse_plan("{\"responders\":[\"路人\"]}", &candidates, None).unwrap_err().contains("不在群里"));
    assert!(parse_plan("让写作助手回答", &candidates, None).unwrap_err().contains("没有返回 JSON"));
}

#[test]
fn test_group_router_discussion_rounds_are_clamped_and_need_two_members() {
    let members = [member(1, "写作助手"), member(2, "Coder")];
    let candidates = members.iter().map(|m| Candidate { member: m, entry: None }).collect::<Vec<_>>();
    assert_eq!(parse_plan("{\"responders\":[\"写作助手\",\"Coder\"],\"rounds\":9}", &candidates, None).unwrap().rounds, MAX_DISCUSSION_ROUNDS);
    assert_eq!(parse_plan("{\"responders\":[\"Coder\"],\"rounds\":3}", &candidates, None).unwrap().rounds, 1);
    // 用户已经点名时名单不变，只采用轮数
    assert_eq!(parse_plan("{\"responders\":[\"Coder\"],\"rounds\":3}", &candidates, Some(&[1, 2])).unwrap(),
        Plan { responders: vec![1, 2], rounds: 3 });
    let latest = message(1, None, "@所有人 讨论下");
    let (system, _) = build_prompt("群", &candidates, &[latest.clone()], &latest, Some(&[1, 2]));
    assert!(system.contains("用户已经点名：写作助手、Coder"));
}

#[test]
fn test_group_router_prompt_lists_members_recent_chat_and_chief_fallback() {
    let members = [member(1, "写作助手"), member(2, "总管家")];
    let chief = RosterEntry { name: "总管家".into(), description: "总管家，熟悉整个助手团队".into(), chief: true };
    let writer = RosterEntry { name: "写作助手".into(), description: "擅长中文写作".into(), chief: false };
    let candidates = vec![Candidate { member: &members[0], entry: Some(&writer) }, Candidate { member: &members[1], entry: Some(&chief) }];
    let earlier = message(1, Some(&members[0]), "初稿写好了");
    let latest = message(2, None, "那再润色一下");
    let (system, user) = build_prompt("周报", &candidates, &[earlier, latest.clone()], &latest, None);
    assert!(system.contains("拿不准时选「总管家」"));
    assert!(user.contains("- 写作助手：擅长中文写作"));
    assert!(user.contains("最近的聊天：\n写作助手：初稿写好了"));
    assert!(user.ends_with("用户的最新消息：\n那再润色一下"));
}

#[test]
fn test_decision_group_router_selects_speakers_and_rounds_without_hallucination() {
    let members = [member(1, "写作助手"), member(2, "Coder"), member(3, "总管家")];
    let chief = RosterEntry { name: "总管家".into(), description: "统筹协调".into(), chief: true };
    let candidates = vec![
        Candidate { member: &members[0], entry: None },
        Candidate { member: &members[1], entry: None },
        Candidate { member: &members[2], entry: Some(&chief) },
    ];
    let latest = message(1, None, "请写作助手和 Coder 讨论一下方案");
    let req = build_decision_dispatch_request("jev-latest", "研发群", &candidates, &[latest.clone()], &latest, None);
    assert!(req.questions.iter().any(|q| q.key() == "primary_responder"));
    assert!(req.questions.iter().any(|q| q.key() == "speak_1"));
    assert!(req.questions.iter().any(|q| q.key() == "speak_2"));
    assert!(req.questions.iter().any(|q| q.key() == "discussion_rounds"));

    let resp = crate::decision::decode_response(
        crate::decision::DecisionProtocol::TypesafeSystemOne,
        &req,
        serde_json::json!({
            "answers": {
                "primary_responder": { "type": "choice", "choice": "m_2", "probabilities": { "m_1": 0.4, "m_2": 0.5, "m_3": 0.1 } },
                "speak_1": { "type": "noul", "noul": 0.85 },
                "speak_2": { "type": "noul", "noul": 0.92 },
                "speak_3": { "type": "noul", "noul": 0.12 },
                "discussion_rounds": { "type": "choice", "choice": "3" }
            }
        }),
        20,
    ).unwrap();
    let plan = plan_from_decision_response(&resp, &candidates, None).unwrap();
    assert_eq!(plan, Plan { responders: vec![2, 1], rounds: 3 });

    // 全部低于阈值时回退 primary_responder，且单人回复 rounds 强制为 1
    let low_resp = crate::decision::decode_response(
        crate::decision::DecisionProtocol::TypesafeSystemOne,
        &req,
        serde_json::json!({
            "answers": {
                "primary_responder": { "type": "choice", "choice": "m_3" },
                "speak_1": { "type": "noul", "noul": 0.2 },
                "speak_2": { "type": "noul", "noul": 0.1 },
                "speak_3": { "type": "noul", "noul": 0.3 },
                "discussion_rounds": { "type": "choice", "choice": "3" }
            }
        }),
        18,
    ).unwrap();
    let fallback_plan = plan_from_decision_response(&low_resp, &candidates, None).unwrap();
    assert_eq!(fallback_plan, Plan { responders: vec![3], rounds: 1 });

    // 用户已点名时保留点名名单，仅采用轮数
    let fixed_plan = plan_from_decision_response(&resp, &candidates, Some(&[1, 3])).unwrap();
    assert_eq!(fixed_plan, Plan { responders: vec![1, 3], rounds: 3 });
}

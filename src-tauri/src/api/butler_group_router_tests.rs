use super::*;

fn member(id: i64, name: &str) -> Member {
    Member { id, group_id: 1, assistant_id: id + 100, name: name.into(), conversation_id: id + 200, joined_after: 0, delivered_through: 0, active: true }
}

fn message(seq: i64, sender: Option<&Member>, body: &str) -> Message {
    Message {
        group_id: 1, seq, sender_member: sender.map(|m| m.id), sender_assistant: sender.map(|m| m.assistant_id),
        sender_name: sender.map(|m| m.name.clone()).unwrap_or_else(|| "我".into()), kind: "message".into(),
        body: body.into(), reply_to: None, created_at: String::new(),
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

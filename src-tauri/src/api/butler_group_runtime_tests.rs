use super::*;

fn turn(round: i64, rounds: i64) -> group::Turn {
    group::Turn {
        id: 1, group_id: 1, member_id: 1, trigger_seq: 1, origin_seq: 1, depth: 0, round, rounds, state: "running".into(),
        input_through: Some(1), input_message_id: None, reply_seq: None, error: None,
    }
}

#[test]
fn test_group_turn_situation_explains_concurrency_and_discussion_rounds() {
    let peers = vec!["李四".to_string(), "王五".to_string()];
    assert_eq!(turn_situation(&turn(1, 1), &[]), None);
    assert_eq!(turn_situation(&turn(1, 1), &peers).unwrap(), "本轮：李四、王五 也在同时回复，你看不到他们正在写的内容，只说你自己的看法。");
    let first = turn_situation(&turn(1, 3), &peers).unwrap();
    assert!(first.starts_with("讨论：第 1/3 轮。先亮出你的观点") && first.contains("李四、王五 也在同时回复"), "{first}");
    assert!(turn_situation(&turn(2, 3), &[]).unwrap().contains("没有新内容就只回复 [[NO_REPLY]]"));
    assert!(turn_situation(&turn(3, 3), &[]).unwrap().contains("这是最后一轮"));
}

#[test]
fn test_group_context_prefix_lists_members_and_style_every_turn() {
    let room = group::Group { id: 1, name: "周报".into(), seq: 3, created_at: String::new(), updated_at: String::new() };
    let member = |id: i64, name: &str| group::Member { id, group_id: 1, assistant_id: id + 10, name: name.into(),
        conversation_id: id + 100, joined_after: 0, delivered_through: 0, active: true };
    let members = vec![member(1, "张三"), member(2, "李四")];
    let mut roster = HashMap::new();
    roster.insert(12, RosterEntry { name: "李四".into(), description: "擅长写作".into(), chief: false });
    let prefix = context_prefix(&room, &members[0], &members, &roster, &turn(1, 1), &["李四".into()]);
    let expected = ["<group_chat name=\"周报\" you=\"张三\">", "当前成员：张三（你）、李四（擅长写作）", "本轮：李四 也在同时回复"].join("\n");
    assert!(prefix.starts_with(&expected), "{prefix}");
    assert!(prefix.contains("不要拆成 1、2、3 逐条列点"));
    assert!(prefix.ends_with("</group_chat>"));
}

fn member(id: i64, name: &str) -> group::Member {
    group::Member { id, group_id: 1, assistant_id: id + 10, name: name.into(), conversation_id: id + 100,
        joined_after: 0, delivered_through: 0, active: true }
}

fn message(seq: i64, sender: Option<i64>, body: &str) -> group::Message {
    group::Message { group_id: 1, seq, sender_member: sender, sender_assistant: sender.map(|id| id + 10),
        sender_name: if sender.is_some() { "成员".into() } else { "我".into() }, kind: "message".into(),
        body: body.into(), reply_to: None, created_at: String::new(), source_conversation_id: None, source_message_id: None }
}

#[test]
fn test_group_quick_route_sends_everyone_mentions_to_the_router_with_a_fixed_roster() {
    let members = vec![member(1, "总管家"), member(2, "写作助手"), member(3, "Coder")];
    // @所有人：名单固定为全部成员，只让调度判断要不要讨论
    assert_eq!(quick_route(&message(5, None, "@所有人 说说看法"), &members, None), Routing::Ask(Some(vec![1, 2, 3])));
    assert_eq!(quick_route(&message(5, None, "@写作助手 @Coder 你俩讨论下"), &members, None), Routing::Ask(Some(vec![2, 3])));
    assert_eq!(quick_route(&message(5, None, "@Coder 看下报错"), &members, None),
        Routing::Plan(Plan { responders: vec![3], rounds: 1 }));
}

#[test]
fn test_group_quick_route_uses_quote_author_single_member_or_router() {
    let members = vec![member(1, "总管家"), member(2, "写作助手")];
    let quoted = message(3, Some(2), "初稿");
    assert_eq!(quick_route(&message(5, None, "改短点"), &members, Some(&quoted)),
        Routing::Plan(Plan { responders: vec![2], rounds: 1 }));
    // 引用自己的消息或系统提示不算点名
    assert_eq!(quick_route(&message(5, None, "补充"), &members, Some(&message(4, None, "我说的"))), Routing::Ask(None));
    assert_eq!(quick_route(&message(5, None, "在吗"), &members[..1], None),
        Routing::Plan(Plan { responders: vec![1], rounds: 1 }));
    assert_eq!(quick_route(&message(5, None, "大家好"), &members, None), Routing::Ask(None));
}

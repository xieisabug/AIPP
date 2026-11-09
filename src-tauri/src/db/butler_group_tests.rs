use super::*;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,assistant_id INTEGER,
            created_time DATETIME,updated_time DATETIME,conversation_kind TEXT NOT NULL DEFAULT 'normal',
            parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE message(id INTEGER PRIMARY KEY AUTOINCREMENT,conversation_id INTEGER NOT NULL,message_type TEXT NOT NULL,
            content TEXT NOT NULL,llm_model_id INTEGER,llm_model_name TEXT,created_time DATETIME,token_count INTEGER,
            input_token_count INTEGER,output_token_count INTEGER,parent_id INTEGER,finish_time DATETIME,
            tool_calls_json TEXT,metadata_json TEXT);").unwrap();
    schema(&conn).unwrap();
    conn
}

fn group_with(conn: &mut Connection, names: &[&str]) -> (Group, Vec<Member>) {
    let invited = names.iter().enumerate().map(|(i, n)| (i as i64 + 10, n.to_string())).collect::<Vec<_>>();
    let group = create(conn, "产品讨论", &invited).unwrap();
    let members = active_members(conn, group.id).unwrap();
    (group, members)
}

fn reply(conn: &Connection, conversation: i64, body: &str) -> i64 {
    conn.execute("INSERT INTO message(conversation_id,message_type,content,finish_time) VALUES(?1,'response',?2,CURRENT_TIMESTAMP)",
        params![conversation, body]).unwrap();
    conn.last_insert_rowid()
}

fn run(conn: &mut Connection, id: i64) -> (Turn, Member, String) {
    match start_turn(conn, id).unwrap() {
        Start::Run { turn, member, input, .. } => {
            write_turn_input(conn, turn.id, member.conversation_id, &input, None, None).unwrap();
            (super::turn(conn, turn.id).unwrap(), member, input)
        }
        Start::Skipped => panic!("turn {id} unexpectedly skipped"),
    }
}

#[test]
fn test_group_create_gives_each_member_a_private_hidden_conversation() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    assert_eq!(members.len(), 2);
    let kinds: Vec<(String, i64, i64)> = collect(&conn, "SELECT conversation_kind,is_hidden_from_normal_chat_list,coalesce(parent_butler_conversation_id,0) FROM conversation ORDER BY id",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(kinds[0], (GROUP_KIND.to_string(), 1, 0));
    assert!(kinds[1..].iter().all(|(kind, hidden, parent)| kind == MEMBER_KIND && *hidden == 1 && *parent == group.id));
    let opening = last_message(&conn, group.id).unwrap().unwrap();
    assert_eq!(opening.kind, "notice");
    assert_eq!(opening.body, "你邀请 张三、李四 加入了群聊");
    // 成员只从加入后的消息开始接收
    assert!(members.iter().all(|m| m.joined_after == 0 && m.delivered_through == 0));
}

#[test]
fn test_group_post_user_is_idempotent_and_waits_for_routing() {
    let mut conn = setup();
    let (group, _) = group_with(&mut conn, &["张三", "李四"]);
    let first = post_user(&mut conn, group.id, "k1", "  大家好  ", None).unwrap();
    let again = post_user(&mut conn, group.id, "k1", "大家好", None).unwrap();
    assert_eq!(first, again);
    assert_eq!(first.body, "大家好");
    assert!(first.from_user());
    assert!(post_user(&mut conn, group.id, "k1", "别的内容", None).is_err());
    assert_eq!(pending_routes(&conn).unwrap().iter().map(|m| m.seq).collect::<Vec<_>>(), vec![first.seq]);
}

#[test]
fn test_group_multiple_responders_reply_concurrently_and_catch_up_later() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "@所有人 这个方案可行吗？", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id, members[1].id], 1).unwrap();
    assert!(pending_routes(&conn).unwrap().is_empty());
    // 两位成员同时开始，各自只看到用户的问题
    assert_eq!(runnable_turns(&conn).unwrap().iter().map(|t| t.id).collect::<Vec<_>>(), turns);
    let Start::Run { turn: first, member: zhang, input, peers, .. } = start_turn(&mut conn, turns[0]).unwrap() else { panic!() };
    assert_eq!(peers, vec!["李四".to_string()]);
    write_turn_input(&mut conn, first.id, zhang.conversation_id, &input, None, None).unwrap();
    let first = super::turn(&conn, first.id).unwrap();
    let (second, _, second_input) = run(&mut conn, turns[1]);
    assert_eq!(input, "群聊「产品讨论」的新消息：\n\n〔系统〕用户邀请 张三、李四 加入了群聊\n\n用户：@所有人 这个方案可行吗？");
    assert_eq!(second_input, input);
    reply(&conn, zhang.conversation_id, "可行，但要先压测。");
    assert_eq!(evidence(&conn, &first).unwrap(), Evidence::Answer("可行，但要先压测。".into()));
    let published = finish_turn(&mut conn, first.id, Outcome::Reply("可行，但要先压测。".into())).unwrap().unwrap().reply.unwrap();
    // 群里的发言指回成员执行会话中的那条最终回复（界面用它显示消息信息、预览代码）
    let source: i64 = conn.query_row("SELECT max(id) FROM message WHERE conversation_id=?1 AND message_type='response'",
        [zhang.conversation_id], |r| r.get(0)).unwrap();
    assert_eq!((published.source_conversation_id, published.source_message_id), (Some(zhang.conversation_id), Some(source)));
    let finished = finish_turn(&mut conn, second.id, Outcome::Reply("我担心成本。".into())).unwrap().unwrap();
    assert!(finished.relayed.is_empty(), "一轮回复不会自动续聊");
    // 下次轮到张三时补上李四的回复；张三自己的发言不会送回给他
    let next = post_user(&mut conn, group.id, "q2", "@张三 压测要多久？", None).unwrap();
    let again = route(&mut conn, group.id, next.seq, &[zhang.id], 1).unwrap();
    let (_, _, follow_up) = run(&mut conn, again[0]);
    assert_eq!(follow_up, "群聊「产品讨论」的新消息：\n\n李四：我担心成本。\n\n用户：@张三 压测要多久？");
}

/// 同时开始一轮里的所有回复，再逐个结束；返回之后排队的回复（即下一轮）。
fn finish_round(conn: &mut Connection, turns: &[i64], replies: &[Option<&str>]) -> Vec<i64> {
    let mut running = Vec::new();
    for (id, reply) in turns.iter().zip(replies) {
        if let Start::Run { turn, member, input, .. } = start_turn(conn, *id).unwrap() {
            write_turn_input(conn, turn.id, member.conversation_id, &input, None, None).unwrap();
            running.push((turn.id, *reply));
        }
    }
    for (id, reply) in running {
        let outcome = match reply { Some(body) => Outcome::Reply(body.to_string()), None => Outcome::Silent };
        finish_turn(conn, id, outcome).unwrap().unwrap();
    }
    collect(conn, "SELECT id FROM butler_chat_group_turn WHERE state='queued' ORDER BY id", [], |r| r.get(0)).unwrap()
}

#[test]
fn test_group_discussion_continues_rounds_until_quiet() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四", "王五"]);
    let question = post_user(&mut conn, group.id, "q", "你们讨论下用哪个数据库", None).unwrap();
    let ids = members.iter().map(|m| m.id).collect::<Vec<_>>();
    let round1 = route(&mut conn, group.id, question.seq, &ids, 3).unwrap();
    let round2 = finish_round(&mut conn, &round1, &[Some("PostgreSQL"), Some("SQLite 就够"), Some("看数据量")]);
    assert_eq!(round2.len(), 3, "所有人说完后进入第二轮");
    let second = super::turn(&conn, round2[0]).unwrap();
    assert_eq!((second.round, second.rounds), (2, 3));
    let Start::Run { input, .. } = start_turn(&mut conn, round2[0]).unwrap() else { panic!() };
    assert!(input.contains("李四：SQLite 就够") && input.contains("王五：看数据量") && !input.contains("张三：PostgreSQL"), "{input}");
    conn.execute("UPDATE butler_chat_group_turn SET state='queued' WHERE id=?1", [round2[0]]).unwrap();
    // 第二轮只有一人还有话说，仍进入第三轮
    let round3 = finish_round(&mut conn, &round2, &[None, Some("那就 SQLite"), None]);
    assert_eq!(round3.len(), 3);
    // 达到计划轮数后结束
    assert!(finish_round(&mut conn, &round3, &[Some("同意"), None, None]).is_empty());
}

#[test]
fn test_group_discussion_ends_when_everyone_passes_or_user_interjects() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let ids = members.iter().map(|m| m.id).collect::<Vec<_>>();
    let first = post_user(&mut conn, group.id, "a", "讨论下发布时间", None).unwrap();
    let round1 = route(&mut conn, group.id, first.seq, &ids, 4).unwrap();
    let round2 = finish_round(&mut conn, &round1, &[Some("周五"), Some("周一")]);
    assert!(finish_round(&mut conn, &round2, &[None, None]).is_empty(), "一轮里都没有新内容就结束");

    let second = post_user(&mut conn, group.id, "b", "再讨论下版本号", None).unwrap();
    let round1 = route(&mut conn, group.id, second.seq, &ids, 4).unwrap();
    let runs = round1.iter().map(|id| run(&mut conn, *id).0).collect::<Vec<_>>();
    post_user(&mut conn, group.id, "c", "先不讨论了", None).unwrap();
    for turn in runs {
        assert!(finish_turn(&mut conn, turn.id, Outcome::Reply("1.0".into())).unwrap().unwrap().relayed.is_empty(), "用户插话后不再续轮");
    }
    // 只有一位成员时没有讨论
    let single = post_user(&mut conn, group.id, "d", "@张三 你讨论下", None).unwrap();
    let only = route(&mut conn, group.id, single.seq, &[ids[0]], 3).unwrap();
    assert_eq!(super::turn(&conn, only[0]).unwrap().rounds, 1);
}

#[test]
fn test_group_turn_input_is_written_only_while_running_and_advances_cursor() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "你好", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id], 1).unwrap();
    let Start::Run { turn: started, member, input, .. } = start_turn(&mut conn, turns[0]).unwrap() else { panic!() };
    // 用户在写入前停止：成员不会收到消息，读取位置也不前进
    stop(&mut conn, group.id).unwrap();
    assert!(write_turn_input(&mut conn, started.id, member.conversation_id, &input, None, None).is_err());
    assert_eq!(super::member(&conn, member.id).unwrap().delivered_through, 0);
    let stopped = last_message(&conn, group.id).unwrap().unwrap();
    assert_eq!(stopped.body, "你停止了当前的回复");
}

#[test]
fn test_group_turn_for_already_delivered_trigger_is_skipped() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let first = post_user(&mut conn, group.id, "a", "第一条", None).unwrap();
    let second = post_user(&mut conn, group.id, "b", "第二条", None).unwrap();
    let t1 = route(&mut conn, group.id, first.seq, &[members[0].id], 1).unwrap();
    let t2 = route(&mut conn, group.id, second.seq, &[members[0].id], 1).unwrap();
    let (turn1, zhang, input) = run(&mut conn, t1[0]);
    assert!(input.contains("用户：第一条\n\n用户：第二条"));
    finish_turn(&mut conn, turn1.id, Outcome::Reply("两条都看到了".into())).unwrap();
    reply(&conn, zhang.conversation_id, "两条都看到了");
    assert!(matches!(start_turn(&mut conn, t2[0]).unwrap(), Start::Skipped));
    assert_eq!(super::turn(&conn, t2[0]).unwrap().state, "skipped");
}

#[test]
fn test_group_member_mentions_relay_with_depth_limit() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "开始", None).unwrap();
    let mut current = route(&mut conn, group.id, question.seq, &[members[0].id], 1).unwrap()[0];
    let mut speakers = Vec::new();
    for _ in 0..=MAX_RELAY_DEPTH {
        let (turn, member, _) = run(&mut conn, current);
        speakers.push(member.name.clone());
        let other = if member.name == "张三" { "李四" } else { "张三" };
        let finished = finish_turn(&mut conn, turn.id, Outcome::Reply(format!("@{other} 你来"))).unwrap().unwrap();
        match finished.relayed.first() {
            Some(next) => current = *next,
            None => break,
        }
    }
    assert_eq!(speakers, vec!["张三", "李四", "张三", "李四"]);
    let limit = last_message(&conn, group.id).unwrap().unwrap();
    assert_eq!(limit.kind, "notice");
    assert!(limit.body.contains("没有再请 张三 回复"), "{}", limit.body);
}

#[test]
fn test_group_failure_is_reported_in_chat_with_reason() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "你好", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[1].id], 1).unwrap();
    let (turn, member, _) = run(&mut conn, turns[0]);
    conn.execute("INSERT INTO message(conversation_id,message_type,content,finish_time) VALUES(?1,'error','401 invalid api key',CURRENT_TIMESTAMP)",
        [member.conversation_id]).unwrap();
    let Evidence::Failed(reason) = evidence(&conn, &turn).unwrap() else { panic!("expected failure") };
    assert!(reason.contains("401 invalid api key"));
    finish_turn(&mut conn, turn.id, Outcome::Failed(reason)).unwrap();
    let error = last_message(&conn, group.id).unwrap().unwrap();
    assert_eq!(error.kind, "error");
    assert!(error.body.starts_with("李四 没能回复：401 invalid api key"));
}

#[test]
fn test_group_evidence_waits_for_tool_calls_and_respects_silence() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "查一下", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id], 1).unwrap();
    let (turn, member, _) = run(&mut conn, turns[0]);
    assert_eq!(evidence(&conn, &turn).unwrap(), Evidence::Pending);
    let call = reply(&conn, member.conversation_id, "我先查资料");
    conn.execute("UPDATE message SET tool_calls_json='[{\"id\":\"c1\"}]' WHERE id=?1", [call]).unwrap();
    assert_eq!(evidence(&conn, &turn).unwrap(), Evidence::Pending);
    reply(&conn, member.conversation_id, "[[NO_REPLY]]");
    assert_eq!(evidence(&conn, &turn).unwrap(), Evidence::Silent);
}

#[test]
fn test_group_removed_member_stops_receiving_and_late_result_is_dropped() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "你好", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id], 1).unwrap();
    let (turn, _, _) = run(&mut conn, turns[0]);
    let (_, running) = remove_member(&mut conn, group.id, members[0].id, "张三 已被移出群聊").unwrap();
    assert_eq!(running.iter().map(|t| t.id).collect::<Vec<_>>(), vec![turn.id]);
    assert!(finish_turn(&mut conn, turn.id, Outcome::Reply("迟到的回复".into())).unwrap().is_none());
    assert_eq!(active_members(&conn, group.id).unwrap().len(), 1);
    assert!(messages(&conn, group.id, None, 50).unwrap().iter().all(|m| m.body != "迟到的回复"));
}

#[test]
fn test_group_recover_publishes_finished_answer_and_reports_unknown() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "你好", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id], 1).unwrap();
    let (_, zhang, _) = run(&mut conn, turns[0]);
    reply(&conn, zhang.conversation_id, "已经答完");
    let other = post_user(&mut conn, group.id, "q2", "@李四 在吗", None).unwrap();
    let pending = route(&mut conn, group.id, other.seq, &[members[1].id], 1).unwrap();
    run(&mut conn, pending[0]);
    assert_eq!(recover(&mut conn).unwrap(), vec![group.id]);
    let bodies = messages(&conn, group.id, None, 50).unwrap().into_iter().map(|m| m.body).collect::<Vec<_>>();
    assert!(bodies.contains(&"已经答完".to_string()));
    assert!(bodies.iter().any(|b| b.starts_with("李四 没能回复：应用重启")));
}

#[test]
fn test_group_parse_mentions_handles_chinese_names_everyone_and_code() {
    let member = |id: i64, name: &str| Member { id, group_id: 1, assistant_id: id, name: name.into(), conversation_id: id,
        joined_after: 0, delivered_through: 0, active: true };
    let members = vec![member(1, "小明"), member(2, "小明明"), member(3, "Writer")];
    assert_eq!(parse_mentions("@小明明你看下，@小明也说说", &members), vec![2, 1]);
    assert_eq!(parse_mentions("联系 me@Writer.com 或 `@小明`", &members), Vec::<i64>::new());
    assert_eq!(parse_mentions("```\n@Writer\n```\n＠Writer 请看", &members), vec![3]);
    assert_eq!(parse_mentions("@所有人 开会", &members), vec![1, 2, 3]);
    assert!(parse_mentions("小明你好", &members).is_empty(), "不带 @ 的名字由调度判断，不算点名");
}

#[test]
fn test_group_legacy_schema_migrates_public_messages_and_drops_queue_tables() {
    let conn = setup();
    conn.execute_batch("
        INSERT INTO conversation(id,name,conversation_kind) VALUES(100,'旧群','butler_group'),(101,'甲 · 旧群','butler_group_member');
        CREATE TABLE butler_group(id INTEGER PRIMARY KEY REFERENCES conversation(id),name TEXT,goal TEXT,chief_member INTEGER,seq INTEGER,state TEXT);
        CREATE TABLE butler_group_member(id INTEGER PRIMARY KEY,group_id INTEGER REFERENCES butler_group(id),assistant_id INTEGER,name TEXT,
            conversation_id INTEGER REFERENCES conversation(id),joined_after INTEGER,delivered_through INTEGER,active INTEGER);
        CREATE TABLE butler_group_packet(id INTEGER PRIMARY KEY,member_id INTEGER REFERENCES butler_group_member(id));
        CREATE TABLE butler_group_event(group_id INTEGER REFERENCES butler_group(id),seq INTEGER,sender_member INTEGER REFERENCES butler_group_member(id),
            sender_name TEXT,kind TEXT,body TEXT,reply_to INTEGER,created_at TEXT,sender_packet INTEGER REFERENCES butler_group_packet(id),PRIMARY KEY(group_id,seq));
        CREATE TABLE butler_group_inbox(id INTEGER PRIMARY KEY,group_id INTEGER REFERENCES butler_group(id),event_seq INTEGER);
        INSERT INTO butler_group VALUES(100,'旧群','目标',1,3,'active');
        INSERT INTO butler_group_member VALUES(1,100,7,'甲',101,0,1,1);
        INSERT INTO butler_group_packet VALUES(5,1);
        INSERT INTO butler_group_event VALUES(100,1,NULL,'用户','message','你好',NULL,'2026-11-09 01:00:00',NULL),
            (100,2,1,'甲','result','我在',NULL,'2026-11-09 01:00:05',5),(100,3,NULL,'用户','notice','乙 加入协作群',NULL,'2026-11-09 01:00:06',NULL);
        INSERT INTO butler_group_inbox VALUES(1,100,1);").unwrap();
    schema(&conn).unwrap();
    for table in LEGACY_TABLES {
        assert!(!table_exists(&conn, table).unwrap(), "{table} should be dropped");
    }
    let migrated = messages(&conn, 100, None, 10).unwrap();
    assert_eq!(migrated.iter().map(|m| (m.sender_name.as_str(), m.kind.as_str(), m.from_user())).collect::<Vec<_>>(),
        vec![("我", "message", true), ("甲", "message", false), ("系统", "notice", false)]);
    let member = member(&conn, 1).unwrap();
    assert_eq!((member.conversation_id, member.delivered_through), (101, 3), "旧积压消息不回放");
    // 再次初始化是幂等的
    schema(&conn).unwrap();
    assert_eq!(list(&conn).unwrap().len(), 1);
}

#[test]
fn test_group_turn_table_from_sequential_version_gains_round_columns() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,assistant_id INTEGER,
            created_time DATETIME,updated_time DATETIME,conversation_kind TEXT NOT NULL DEFAULT 'normal',
            parent_butler_conversation_id INTEGER,is_hidden_from_normal_chat_list INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE butler_chat_group_turn(id INTEGER PRIMARY KEY,group_id INTEGER NOT NULL,member_id INTEGER NOT NULL,
            trigger_seq INTEGER NOT NULL,origin_seq INTEGER NOT NULL,depth INTEGER NOT NULL DEFAULT 0,after_turn INTEGER,
            state TEXT NOT NULL DEFAULT 'queued',input_through INTEGER,input_message_id INTEGER,reply_seq INTEGER,error TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
        INSERT INTO butler_chat_group_turn(group_id,member_id,trigger_seq,origin_seq,state) VALUES(1,1,1,1,'done');").unwrap();
    schema(&conn).unwrap();
    schema(&conn).unwrap();
    let old = turn(&conn, 1).unwrap();
    assert_eq!((old.round, old.rounds), (1, 1));
}

#[test]
fn test_group_everyone_replies_concurrently_while_one_member_waits_for_the_user() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["总管家", "写作助手", "Coder"]);
    let ids = members.iter().map(|m| m.id).collect::<Vec<_>>();
    let question = post_user(&mut conn, group.id, "q", "@所有人 下周的计划怎么排", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &ids, 1).unwrap();
    // 三位成员同时开始
    assert_eq!(runnable_turns(&conn).unwrap().len(), 3);
    let runs = turns.iter().map(|id| run(&mut conn, *id)).collect::<Vec<_>>();
    assert!(runnable_turns(&conn).unwrap().is_empty());
    // Coder 调用 ask_user_question 等用户回答：执行还没结束，只有工具调用
    let (coder_turn, coder, _) = &runs[2];
    let asking = reply(&conn, coder.conversation_id, "我需要确认一下");
    conn.execute("UPDATE message SET tool_calls_json='[{\"id\":\"ask\"}]' WHERE id=?1", [asking]).unwrap();
    assert_eq!(evidence(&conn, coder_turn).unwrap(), Evidence::Pending);
    // 其他两位先说完，互不阻塞
    for (turn, member, _) in &runs[..2] {
        reply(&conn, member.conversation_id, &format!("{} 的安排", member.name));
        finish_turn(&mut conn, turn.id, Outcome::Reply(format!("{} 的安排", member.name))).unwrap().unwrap();
    }
    assert_eq!(open_turns(&conn, group.id).unwrap().iter().map(|t| t.member_id).collect::<Vec<_>>(), vec![coder.id]);
    // 用户回答后 Coder 给出最终回复
    reply(&conn, coder.conversation_id, "按两周迭代排");
    assert_eq!(evidence(&conn, coder_turn).unwrap(), Evidence::Answer("按两周迭代排".into()));
    finish_turn(&mut conn, coder_turn.id, Outcome::Reply("按两周迭代排".into())).unwrap().unwrap();
    assert!(open_turns(&conn, group.id).unwrap().is_empty());
    let bodies = messages(&conn, group.id, None, 20).unwrap().into_iter().map(|m| m.body).collect::<Vec<_>>();
    assert!(bodies.ends_with(&["总管家 的安排".into(), "写作助手 的安排".into(), "按两周迭代排".into()]));
}

#[test]
fn test_group_discussion_round_waits_for_a_member_still_answering() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let ids = members.iter().map(|m| m.id).collect::<Vec<_>>();
    let question = post_user(&mut conn, group.id, "q", "@所有人 讨论下要不要重构", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &ids, 3).unwrap();
    let first = run(&mut conn, turns[0]).0;
    let second = run(&mut conn, turns[1]).0;
    let early = finish_turn(&mut conn, first.id, Outcome::Reply("要重构".into())).unwrap().unwrap();
    assert!(early.relayed.is_empty(), "另一位还没说完，不进入下一轮");
    let late = finish_turn(&mut conn, second.id, Outcome::Failed("401 invalid api key".into())).unwrap().unwrap();
    assert_eq!(late.relayed.len(), 2, "失败也算这一轮结束，有人发过言就进入下一轮");
    // 停止后不再续轮，迟到的结果也不发出
    let next = late.relayed;
    let (running, _, _) = run(&mut conn, next[0]);
    stop(&mut conn, group.id).unwrap();
    assert!(finish_turn(&mut conn, running.id, Outcome::Reply("迟到".into())).unwrap().is_none());
    assert!(open_turns(&conn, group.id).unwrap().is_empty());
}

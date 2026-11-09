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
fn test_group_sequential_responders_see_earlier_replies() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "这个方案可行吗？", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id, members[1].id]).unwrap();
    assert!(pending_routes(&conn).unwrap().is_empty());
    // 第二位要等第一位说完
    assert_eq!(runnable_turns(&conn).unwrap().iter().map(|t| t.id).collect::<Vec<_>>(), vec![turns[0]]);
    let (first, zhang, input) = run(&mut conn, turns[0]);
    assert_eq!(input, "群聊「产品讨论」的新消息：\n\n〔系统〕用户邀请 张三、李四 加入了群聊\n\n用户：这个方案可行吗？");
    assert!(runnable_turns(&conn).unwrap().is_empty());
    reply(&conn, zhang.conversation_id, "可行，但要先压测。");
    let evidence = evidence(&conn, &first).unwrap();
    assert_eq!(evidence, Evidence::Answer("可行，但要先压测。".into()));
    finish_turn(&mut conn, first.id, Outcome::Reply("可行，但要先压测。".into())).unwrap().unwrap();
    let (_, _, second_input) = run(&mut conn, turns[1]);
    assert!(second_input.ends_with("用户：这个方案可行吗？\n\n张三：可行，但要先压测。"), "{second_input}");
    // 成员自己的发言不会再作为输入送回给它
    let next = post_user(&mut conn, group.id, "q2", "@张三 压测要多久？", None).unwrap();
    let again = route(&mut conn, group.id, next.seq, &[zhang.id]).unwrap();
    let (_, _, follow_up) = run(&mut conn, again[0]);
    assert_eq!(follow_up, "群聊「产品讨论」的新消息：\n\n用户：@张三 压测要多久？");
}

#[test]
fn test_group_turn_input_is_written_only_while_running_and_advances_cursor() {
    let mut conn = setup();
    let (group, members) = group_with(&mut conn, &["张三", "李四"]);
    let question = post_user(&mut conn, group.id, "q", "你好", None).unwrap();
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id]).unwrap();
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
    let t1 = route(&mut conn, group.id, first.seq, &[members[0].id]).unwrap();
    let t2 = route(&mut conn, group.id, second.seq, &[members[0].id]).unwrap();
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
    let mut current = route(&mut conn, group.id, question.seq, &[members[0].id]).unwrap()[0];
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
    let turns = route(&mut conn, group.id, question.seq, &[members[1].id]).unwrap();
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
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id]).unwrap();
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
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id]).unwrap();
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
    let turns = route(&mut conn, group.id, question.seq, &[members[0].id]).unwrap();
    let (_, zhang, _) = run(&mut conn, turns[0]);
    reply(&conn, zhang.conversation_id, "已经答完");
    let other = post_user(&mut conn, group.id, "q2", "@李四 在吗", None).unwrap();
    let pending = route(&mut conn, group.id, other.seq, &[members[1].id]).unwrap();
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

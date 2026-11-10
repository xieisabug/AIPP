use super::*;
use crate::db::butler_memory as memory;

/// 小A（助手 2）有私聊 20、派工任务 21，并在项目群 50 里（成员 1，会话 51）；小B（助手 3）有私聊 30，群里是成员 2（会话 52）。
fn fixture() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("
        CREATE TABLE conversation(id INTEGER PRIMARY KEY, name TEXT, assistant_id INTEGER, conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id INTEGER, parent_id INTEGER,
            message_type TEXT, content TEXT, metadata_json TEXT);
        INSERT INTO conversation VALUES
            (10,'总管家',1,'butler_main'),(20,'与小A的私聊',2,'butler_contact'),(21,'小A的任务',2,'butler_task'),
            (30,'与小B的私聊',3,'butler_contact'),(40,'普通聊天',2,'normal'),(50,'项目群',NULL,'butler_group'),
            (51,'小A在项目群',2,'butler_group_member'),(52,'小B在项目群',3,'butler_group_member');
        INSERT INTO message(id,conversation_id,parent_id,message_type,content,metadata_json) VALUES
            (1,20,NULL,'user','记住我喜欢中文',NULL),(2,20,1,'response','好的',NULL),
            (3,30,NULL,'user','你好',NULL),(4,30,3,'response','你好',NULL),
            (5,21,NULL,'user','派工内容',NULL),(6,21,5,'response','收到',NULL),
            (7,40,NULL,'user','普通',NULL),(8,40,7,'response','普通回复',NULL),
            (60,51,NULL,'user','我：大家记住周五交付','{\"origin\":\"butler_group\",\"group_id\":50,\"member_id\":1,\"turn_id\":100}'),
            (61,51,60,'response','收到',NULL),
            (70,52,NULL,'user','小A：@小B 记一下','{\"origin\":\"butler_group\",\"group_id\":50,\"member_id\":2,\"turn_id\":101}'),
            (71,52,70,'response','好',NULL);
    ").unwrap();
    memory::schema(&c).unwrap();
    crate::db::butler_group::schema(&c).unwrap();
    crate::db::butler_work::schema(&c).unwrap();
    c.execute_batch("
        INSERT INTO butler_chat_group(id,name,seq) VALUES(50,'项目群',2);
        INSERT INTO butler_chat_group_member(id,group_id,assistant_id,name,conversation_id,joined_after,delivered_through,active)
            VALUES(1,50,2,'小A',51,0,1,1),(2,50,3,'小B',52,0,2,1);
        INSERT INTO butler_chat_group_message(group_id,seq,sender_member,sender_assistant,sender_name,kind,body)
            VALUES(50,1,NULL,NULL,'我','message','大家记住周五交付'),(50,2,1,2,'小A','message','@小B 记一下');
        INSERT INTO butler_chat_group_turn(id,group_id,member_id,trigger_seq,origin_seq,depth,state,input_through,input_message_id)
            VALUES(100,50,1,1,1,0,'running',1,60),(101,50,2,2,1,1,'running',2,70);
    ").unwrap();
    c
}

fn run(c: &mut Connection, conversation: i64, anchor: Option<i64>, args: Value) -> Result<Value> {
    execute(c, conversation, anchor, &args)
}

fn remember_in(c: &mut Connection, conversation: i64, anchor: i64, content: &str) -> Value {
    run(c, conversation, Some(anchor), json!({"action":"remember","content":content})).unwrap()
}

fn recall_ids(c: &mut Connection, conversation: i64) -> Vec<i64> {
    run(c, conversation, None, json!({"action":"recall"})).unwrap()["memories"].as_array().unwrap()
        .iter().map(|m| m["id"].as_i64().unwrap()).collect()
}

fn memory_count(c: &Connection) -> i64 {
    c.query_row("SELECT count(*) FROM butler_memory", [], |r| r.get(0)).unwrap()
}

#[test]
fn test_memory_api_private_memory_follows_assistant_across_conversations() {
    let mut c = fixture();
    let saved = remember_in(&mut c, 20, 2, "回复使用中文");
    assert_eq!(saved["status"], "saved");
    assert_eq!(saved["memory"]["scope"], "assistant");
    let id = saved["memory"]["id"].as_i64().unwrap();
    assert_eq!(memory::get_any(&c, id).unwrap().unwrap().source(), Some(Source::Message(1)));
    assert_eq!(recall_ids(&mut c, 21), vec![id], "same assistant's task sees it");
    assert!(recall_ids(&mut c, 30).is_empty(), "another assistant does not");
    assert!(recall_ids(&mut c, 51).is_empty(), "private memory never enters a group");
    assert_eq!(remember_in(&mut c, 20, 2, "回复使用中文")["status"], "already_exists");
}

#[test]
fn test_memory_api_update_and_forget_by_id_with_optional_revision() {
    let mut c = fixture();
    let id = remember_in(&mut c, 20, 2, "回复使用中文")["memory"]["id"].as_i64().unwrap();
    let updated = run(&mut c, 20, Some(2), json!({"action":"update","id":id,"content":"回复简短"})).unwrap();
    assert_eq!(updated["memory"]["revision"], 2);
    assert!(run(&mut c, 20, Some(2), json!({"action":"forget","id":id,"revision":1})).is_err());
    assert!(run(&mut c, 30, Some(4), json!({"action":"update","id":id,"content":"越权"})).is_err());
    assert_eq!(run(&mut c, 30, Some(4), json!({"action":"forget","id":id})).unwrap()["status"], "not_found");
    assert_eq!(run(&mut c, 20, Some(2), json!({"action":"forget","id":id,"revision":2})).unwrap()["status"], "forgotten");
    assert_eq!(memory_count(&c), 0);
}

#[test]
fn test_memory_api_global_memory_is_shared_but_private_source_is_not() {
    let mut c = fixture();
    let saved = run(&mut c, 20, Some(2), json!({"action":"remember","scope":"global","content":"所有助手都用中文"})).unwrap();
    let id = saved["memory"]["id"].as_i64().unwrap();
    assert_eq!(recall_ids(&mut c, 30), vec![id]);
    assert_eq!(recall_ids(&mut c, 52), vec![id], "global memory reaches group members");
    assert_eq!(run(&mut c, 30, None, json!({"action":"source","id":id})).unwrap()["status"], "unavailable");
    let own = run(&mut c, 20, None, json!({"action":"source","id":id})).unwrap();
    assert_eq!((own["status"].as_str(), own["text"].as_str()), (Some("available"), Some("记住我喜欢中文")));
}

#[test]
fn test_memory_api_rejects_turns_not_sent_by_the_user() {
    for meta in [r#"{"origin":"internal"}"#, r#"{"origin":"butler_peer","leg":"request"}"#, r#"{"relayOrigin":"x"}"#] {
        let mut c = fixture();
        c.execute("UPDATE message SET metadata_json=?1 WHERE id=1", [meta]).unwrap();
        assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"不应保存"})).is_err(), "{meta}");
    }
    // continue_work 送进运行中任务的要求没有 origin 标记，要从续接记录识别。
    let mut c = fixture();
    c.execute_batch("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(9,21,NULL,'user','总管家转达的要求'),(10,21,9,'response','好');
        INSERT INTO butler_work_followup(id,work_ref,request_key,conversation_id,task_conversation_id,instruction,state,execution_task,execution_input_id)
            VALUES(1,'task:21','k',10,21,'记住这个','sending',21,9);").unwrap();
    let error = run(&mut c, 21, Some(10), json!({"action":"remember","content":"续接"})).unwrap_err().to_string();
    assert!(error.contains("续接"), "{error}");
    assert!(run(&mut c, 21, Some(6), json!({"action":"remember","content":"派工"})).unwrap_err().to_string().contains("派工"));
    assert_eq!(memory_count(&c), 0);
}

#[test]
fn test_memory_api_accepts_user_messages_in_task_after_dispatch() {
    let mut c = fixture();
    c.execute_batch("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(9,21,NULL,'user','记住输出用表格'),(10,21,9,'response','好');").unwrap();
    assert_eq!(remember_in(&mut c, 21, 10, "输出用表格")["status"], "saved");
}

#[test]
fn test_memory_api_older_turn_can_still_write_after_user_sends_more() {
    let mut c = fixture();
    // 普通模型允许运行中插队：用户又发了一条，上一轮里的“记住”仍然是用户本人的要求。
    c.execute("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(11,20,NULL,'user','另外一件事')", []).unwrap();
    assert_eq!(remember_in(&mut c, 20, 2, "回复使用中文")["status"], "saved");
}

#[test]
fn test_memory_api_requires_turn_identity_and_running_work() {
    let mut c = fixture();
    assert!(run(&mut c, 20, None, json!({"action":"remember","content":"无来源"})).is_err());
    assert!(run(&mut c, 20, Some(4), json!({"action":"remember","content":"别的会话"})).is_err());
    for action in ["pause", "cancel"] {
        let mut c = fixture();
        crate::db::butler_control::set(&c, 20, action).unwrap();
        assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"不得写入"})).is_err(), "{action}");
        assert_eq!(memory_count(&c), 0);
    }
}

#[test]
fn test_memory_api_unavailable_outside_butler_assistant_conversations() {
    let mut c = fixture();
    assert!(run(&mut c, 40, Some(8), json!({"action":"recall"})).is_err());
    assert!(run(&mut c, 50, None, json!({"action":"recall"})).is_err(), "the group itself is not an assistant");
    assert!(run(&mut c, 20, Some(2), json!({"action":"delete"})).is_err());
}

#[test]
fn test_memory_api_group_member_writes_shared_group_memory() {
    let mut c = fixture();
    let saved = remember_in(&mut c, 51, 61, "周五交付");
    assert_eq!(saved["memory"]["scope"], "group");
    let id = saved["memory"]["id"].as_i64().unwrap();
    let entry = memory::get_any(&c, id).unwrap().unwrap();
    assert_eq!((entry.scope_id, entry.source(), entry.author_assistant_id), (Some(50), Some(Source::GroupMessage { group: 50, seq: 1 }), Some(2)));
    assert_eq!(recall_ids(&mut c, 52), vec![id], "other members share group memory");
    assert!(recall_ids(&mut c, 20).is_empty(), "group memory stays in the group");
    let source = run(&mut c, 52, None, json!({"action":"source","id":id})).unwrap();
    assert_eq!(source["text"], "大家记住周五交付");
    assert_eq!(run(&mut c, 30, None, json!({"action":"source","id":id})).unwrap_err().to_string().contains("无权"), true);
}

#[test]
fn test_memory_api_group_rejects_relays_stopped_turns_and_private_scope() {
    let mut c = fixture();
    let error = run(&mut c, 52, Some(71), json!({"action":"remember","content":"接力"})).unwrap_err().to_string();
    assert!(error.contains("点名"), "{error}");
    assert!(run(&mut c, 51, Some(61), json!({"action":"remember","scope":"assistant","content":"私有"})).is_err());
    assert!(run(&mut c, 51, Some(61), json!({"action":"remember","content":"x","source_message_id":1})).is_err());
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","scope":"group","content":"x"})).is_err());
    c.execute("UPDATE butler_chat_group_turn SET state='cancelled' WHERE id=100", []).unwrap();
    assert!(run(&mut c, 51, Some(61), json!({"action":"remember","content":"迟到"})).is_err());
    c.execute("UPDATE butler_chat_group_member SET active=0 WHERE id=1", []).unwrap();
    assert!(run(&mut c, 51, None, json!({"action":"recall"})).is_err(), "removed members lose access");
    assert_eq!(memory_count(&c), 0);
}

#[test]
fn test_memory_api_explicit_source_must_be_own_direct_user_message() {
    let mut c = fixture();
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"x","source_message_id":3})).is_err());
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"x","source_message_id":2})).is_err());
    assert!(run(&mut c, 20, Some(2), json!({"action":"remember","content":"x","source_message_id":5})).is_err(), "task dispatch is not a user source");
    c.execute("INSERT INTO message(id,conversation_id,parent_id,message_type,content) VALUES(12,21,NULL,'user','以后周报用表格')", []).unwrap();
    let saved = run(&mut c, 20, Some(2), json!({"action":"remember","content":"周报用表格","source_message_id":12})).unwrap();
    let id = saved["memory"]["id"].as_i64().unwrap();
    assert_eq!(memory::get_any(&c, id).unwrap().unwrap().source(), Some(Source::Message(12)));
}

/// 一条带附加上下文的用户消息（附加上下文接在原话后面存储）。
fn user(context: &str) -> (String, String) {
    ("user".into(), crate::utils::turn_context::append("你好", crate::utils::turn_context::wrap([context]).as_deref()))
}

/// 记忆工具的结果消息，格式与 `mcp::execution_api` 一致（外面包着 MCP 的 content）。
fn tool_result(result: &Value) -> (String, String) {
    let wrapped = json!({"content":[{"type":"text","text":result.to_string()}],"isError":false});
    ("tool_result".into(), format!("Tool execution completed:\n\nTool Call ID: call_1\nTool: butler_memory\nServer: 智能体工具\nParameters: {{}}\nResult:\n{wrapped}"))
}

fn seen(history: &[(String, String)]) -> memory::Injected {
    seen_in_history(history.iter().map(|(kind, content)| (kind.as_str(), content.as_str())))
}

#[test]
fn test_memory_turn_block_lists_visible_memories_with_scope_tags() {
    let mut c = fixture();
    let own = remember_in(&mut c, 20, 2, "用户在做 AIPP 项目")["memory"]["id"].as_i64().unwrap();
    let global = run(&mut c, 20, Some(2), json!({"action":"remember","scope":"global","content":"回答使用中文"})).unwrap()["memory"]["id"].as_i64().unwrap();
    remember_in(&mut c, 51, 61, "周五交付");
    let none = memory::Injected::default();
    let block = turn_block(&c, 21, "AIPP 的进度", &none, 1).unwrap().unwrap();
    assert!(block.starts_with("<long_term_memory full=\"true\" as_of=\"1\">"), "{block}");
    assert!(block.contains(&format!("- #{own} v1：用户在做 AIPP 项目")), "{block}");
    assert!(block.contains(&format!("- #{global} v1（全局）：回答使用中文")), "{block}");
    assert!(!block.contains("周五交付"), "{block}");
    assert!(turn_block(&c, 52, "进度", &none, 1).unwrap().unwrap().contains("（本群）：周五交付"));
    assert!(turn_block(&c, 40, "普通聊天", &none, 1).unwrap().is_none());
    assert!(turn_block(&c, 50, "群聊本身", &none, 1).unwrap().is_none());
    c.execute("DELETE FROM butler_memory", []).unwrap();
    assert!(turn_block(&c, 20, "任意", &none, 1).unwrap().is_none(), "no memories costs no tokens");
}

#[test]
fn test_memory_turn_block_only_adds_changes_after_full_injection() {
    let mut c = fixture();
    let kept = remember_in(&mut c, 20, 2, "用户在做 AIPP 项目")["memory"]["id"].as_i64().unwrap();
    let edited = remember_in(&mut c, 20, 2, "回答先给结论")["memory"]["id"].as_i64().unwrap();
    let dropped = remember_in(&mut c, 20, 2, "周报用表格")["memory"]["id"].as_i64().unwrap();
    let first = turn_block(&c, 20, "你好", &seen(&[]), 1).unwrap().unwrap();
    // 存进历史的是整块附加上下文，解析时只看其中的记忆元素。
    let mut history = vec![user(&format!("<current_time>t</current_time>\n\n{first}")), ("response".into(), "好的".into())];
    assert!(turn_block(&c, 20, "你好", &seen(&history), 2).unwrap().is_none(), "已附带且没有变化的记忆不重复");

    // 都是在别处改的（界面、其他会话），本会话历史里没有对应的工具结果。
    memory::update(&c, None, edited, None, "回答先给结论，再给细节", None).unwrap();
    memory::forget(&c, None, dropped, None).unwrap();
    let added = run(&mut c, 20, Some(2), json!({"action":"remember","scope":"global","content":"所有助手用中文"})).unwrap()["memory"]["id"].as_i64().unwrap();
    let second = turn_block(&c, 20, "你好", &seen(&history), 3).unwrap().unwrap();
    assert!(second.starts_with("<long_term_memory as_of=\"3\">"), "{second}");
    assert!(second.contains(&format!("- #{dropped}（已删除，不再适用）")), "{second}");
    assert!(second.contains(&format!("- #{edited} v2（已更新，替换之前的版本）：回答先给结论，再给细节")), "{second}");
    assert!(second.contains(&format!("- #{added} v1（全局）：所有助手用中文")), "{second}");
    assert!(!second.contains(&format!("#{kept} ")), "没变的记忆不再附带：{second}");

    history.push(user(&second));
    assert!(turn_block(&c, 20, "你好", &seen(&history), 4).unwrap().is_none());
    // 早期内容被压缩且检查点里没有记忆：完整附带一次，但历史里仍看得到的不重复。
    let again = turn_block(&c, 20, "你好", &seen(&history[2..]), 5).unwrap().unwrap();
    assert!(again.contains("full=\"true\""), "{again}");
    assert!(again.contains(&format!("- #{kept} v1：")) && !again.contains("已删除"), "{again}");
    assert!(!again.contains(&format!("#{edited} ")) && !again.contains(&format!("#{added} ")), "{again}");
}

#[test]
fn test_memory_turn_block_skips_what_the_memory_tool_already_returned() {
    let mut c = fixture();
    let kept = remember_in(&mut c, 20, 2, "用户在做 AIPP 项目")["memory"]["id"].as_i64().unwrap();
    let first = turn_block(&c, 20, "你好", &seen(&[]), 1).unwrap().unwrap();
    let mut history = vec![user(&first)];
    // 这一轮模型用工具保存了一条：工具结果已在历史里，下一轮不再当成新增附带。
    let saved = run(&mut c, 20, Some(2), json!({"action":"remember","content":"回答先给结论"})).unwrap();
    let id = saved["memory"]["id"].as_i64().unwrap();
    history.push(tool_result(&saved));
    assert!(turn_block(&c, 20, "回答风格", &seen(&history), 2).unwrap().is_none());
    // 用户之后在界面里改了它：模型没看到新版本，照常附带更新。
    memory::update(&c, None, id, None, "回答先给结论，再给细节", None).unwrap();
    let block = turn_block(&c, 20, "回答风格", &seen(&history), 3).unwrap().unwrap();
    assert!(block.contains(&format!("- #{id} v2（已更新")), "{block}");
    history.push(user(&block));
    // 模型自己忘记的：不再告知删除。
    let forgotten = run(&mut c, 20, Some(2), json!({"action":"forget","id":id})).unwrap();
    history.push(tool_result(&forgotten));
    assert!(turn_block(&c, 20, "回答风格", &seen(&history), 4).unwrap().is_none());
    // 别处删掉的仍要告知。
    memory::forget(&c, None, kept, None).unwrap();
    let removed = turn_block(&c, 20, "你好", &seen(&history), 5).unwrap().unwrap();
    assert!(removed.contains(&format!("- #{kept}（已删除")), "{removed}");
}

#[test]
fn test_memory_seen_in_history_reads_blocks_checkpoint_and_tool_results() {
    let history = vec![
        ("system".to_string(), "系统提示 <long_term_memory full=\"true\" as_of=\"5\">\n- #8 v1：提示词里的不算\n</long_term_memory>".to_string()),
        ("system".to_string(), "<context_summary>\n摘要转述：<long_term_memory as_of=\"6\">\n- #7 v1：摘要里转述的不算\n</long_term_memory>\n</context_summary>\n\n<long_term_memory full=\"true\" as_of=\"10\">\n- #1 v1：甲\n</long_term_memory>\n说明".to_string()),
        user("<long_term_memory as_of=\"20\">\n- #2 v3（全局）：乙\n- #1 v2（已更新，替换之前的版本）：甲二\n</long_term_memory>"),
        tool_result(&json!({"status":"saved","memory":{"id":3,"revision":1,"scope":"assistant","content":"丙"}})),
        tool_result(&json!({"status":"forgotten","id":2})),
        tool_result(&json!({"memories":[{"id":4,"revision":2,"scope":"global","content":"丁"}],"visible_total":4})),
        ("tool_result".to_string(), "Tool execution completed:\n\nTool Call ID: c\nTool: other_tool\nServer: s\nParameters: {}\nResult:\n{\"status\":\"saved\",\"memory\":{\"id\":5,\"revision\":1}}".to_string()),
        ("tool_result".to_string(), "Tool execution completed:\n\nTool Call ID: c\nTool: butler_memory\nServer: s\nParameters: {}\nResult:\nError: 写入失败".to_string()),
    ];
    let state = seen(&history);
    assert!(state.full);
    assert_eq!(state.as_of, 20);
    assert_eq!(state.revisions, std::collections::HashMap::from([(1, Some(2)), (2, None), (3, Some(1)), (4, Some(2))]));
    assert!(!seen(&history[..1]).full, "系统提示词里的不算完整附带");
    let typed = "<long_term_memory full=\"true\" as_of=\"1\">\n- #1 v1：用户自己打的\n</long_term_memory>";
    assert_eq!(seen_in_history([("user", typed)]), memory::Injected::default(), "用户原话里的不算");
}

#[test]
fn test_memory_checkpoint_block_carries_memories_the_tail_does_not_show() {
    let mut c = fixture();
    let old = remember_in(&mut c, 20, 2, "用户在做 AIPP 项目")["memory"]["id"].as_i64().unwrap();
    let saved = run(&mut c, 20, Some(2), json!({"action":"remember","content":"回答先给结论"})).unwrap();
    let recent = saved["memory"]["id"].as_i64().unwrap();
    // 压缩后原样保留的近期消息里只有一次工具保存；早期的完整附带被摘要替换了。
    let tail = vec![("user".to_string(), "AIPP 进度怎样".to_string()), tool_result(&saved), ("response".to_string(), "好".to_string())];
    let pairs: Vec<(&str, &str)> = tail.iter().map(|(kind, content)| (kind.as_str(), content.as_str())).collect();
    let block = checkpoint_block(&c, 20, &pairs, 7).unwrap().unwrap();
    assert!(block.starts_with("<long_term_memory full=\"true\" as_of=\"7\">"), "{block}");
    assert!(block.contains(&format!("- #{old} v1：")) && !block.contains(&format!("#{recent} ")), "{block}");
    // 检查点接在摘要后面存成 system 消息：之后的用户消息只附带变化，不会整批重带。
    let mut history = vec![("system".to_string(), format!("<context_summary>\n摘要\n</context_summary>\n\n{block}"))];
    history.extend(tail);
    assert!(turn_block(&c, 20, "你好", &seen(&history), 8).unwrap().is_none());
    // 近期消息里已经有完整附带：检查点不用再带。
    let full = turn_block(&c, 20, "你好", &memory::Injected::default(), 1).unwrap().unwrap();
    let full_tail = [user(&full)];
    let pairs: Vec<(&str, &str)> = full_tail.iter().map(|(kind, content)| (kind.as_str(), content.as_str())).collect();
    assert!(checkpoint_block(&c, 20, &pairs, 9).unwrap().is_none());
    assert!(checkpoint_block(&c, 40, &[("user", "普通聊天")], 9).unwrap().is_none(), "普通会话没有长期记忆");
}

#[test]
fn test_memory_source_view_shows_label_and_deleted_state() {
    let mut c = fixture();
    let private = remember_in(&mut c, 20, 2, "回复使用中文")["memory"]["id"].as_i64().unwrap();
    let group = remember_in(&mut c, 51, 61, "周五交付")["memory"]["id"].as_i64().unwrap();
    let view = source_view(&c, private).unwrap();
    assert_eq!((view.status.as_str(), view.label.as_deref(), view.text.as_deref()), ("available", Some("与小A的私聊"), Some("记住我喜欢中文")));
    assert_eq!(source_view(&c, group).unwrap().label.as_deref(), Some("群聊「项目群」"));
    c.execute("DELETE FROM message WHERE id=1", []).unwrap();
    assert_eq!(source_view(&c, private).unwrap().status, "deleted");
    assert!(memory::get_any(&c, private).unwrap().is_some(), "deleting the chat keeps the memory");
}

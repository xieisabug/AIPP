use super::*;
use crate::db::butler_history;
use std::collections::HashMap;

pub(super) fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,assistant_id INTEGER,name TEXT,conversation_kind TEXT,updated_time TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,message_type TEXT,content TEXT,metadata_json TEXT,created_time TEXT DEFAULT CURRENT_TIMESTAMP,
            llm_model_id INTEGER,llm_model_name TEXT,token_count INTEGER,input_token_count INTEGER,output_token_count INTEGER,finish_time TEXT,tool_calls_json TEXT);
        CREATE TABLE conversation_summary(conversation_id INTEGER);
        CREATE TABLE butler_read_reply(id INTEGER PRIMARY KEY,message_id INTEGER UNIQUE);
        CREATE TABLE butler_main_state(butler_conversation_id INTEGER);
        INSERT INTO butler_main_state VALUES (10);
        INSERT INTO conversation VALUES (10,1,'总管家','butler_main',NULL),(20,2,'写作','butler_contact',NULL),
            (30,3,'审稿','butler_contact',NULL),(40,3,'审核任务','butler_task',NULL),(50,3,'普通聊天','normal',NULL);").unwrap();
    schema(&db).unwrap();
    db
}
fn message(conn: &Connection, conv: i64, kind: &str, text: &str) -> i64 {
    conn.execute("INSERT INTO message(conversation_id,message_type,content,finish_time) VALUES (?1,?2,?3,CURRENT_TIMESTAMP)",
        params![conv,kind,text]).unwrap();
    conn.last_insert_rowid()
}
pub(super) fn request<'a>(key: &'a str, body: &'a str) -> NewRequest<'a> {
    NewRequest {
        owner: 20,
        target: 30,
        sender: "写作",
        recipient: "审稿",
        key,
        body,
        tool_message_id: None,
        limit: 2,
        report_to_user: Some(false),
        watch_completion: None,
    }
}
fn reread(conn: &Connection, id: i64) -> Exchange {
    conn.query_row(&format!("{SELECT} WHERE id=?1"), [id], row).unwrap()
}

#[test]
fn ordinary_messages_finish_on_delivery_and_can_be_answered_later() {
    let mut db = database();
    message(&db, 20, "user", "让审稿助手知悉");
    let req = NewRequest { watch_completion: Some(false), ..request("note", "周五发布") };
    let note = enqueue(&mut db, req).unwrap();
    assert!(!has_open_owned(&db, 20).unwrap());
    assert!(claim(&db, &note).unwrap());
    write_input(&mut db, note.id, "request", 30, "周五发布", 1, "m").unwrap();
    assert_eq!(reread(&db, note.id).state, "delivered");
    assert!(pending_exchanges(&db).unwrap().is_empty());
    let back = enqueue(&mut db, NewRequest { owner: 30, target: 20,
        watch_completion: Some(false), ..request("detail", "我还有一个问题") }).unwrap();
    assert_eq!(back.target, 20);
    assert!(!has_open_children(&db, note.id).unwrap());
    message(&db, 30, "user", "先看另一个问题");
    assert!(notify_sender(&mut db, note.id, 10, 2, "result", "检查通过", None).is_err());
    assert!(notify_sender(&mut db, note.id, 30, 1, "result", "检查通过", None).is_err());
    let result = notify_sender(&mut db, note.id, 30, 2, "result", "检查通过", None).unwrap();
    assert!(claim(&db, &result).unwrap());
    write_input(&mut db, note.id, "reply", 20, "检查通过", 1, "m").unwrap();
    assert_eq!(notify_sender(&mut db, note.id, 30, 2, "result", "检查通过", None).unwrap().state, "delivered");
    let posts = crate::db::butler_timeline::list(&db, 30).unwrap();
    assert_eq!(posts.iter().filter(|p| p.outgoing && p.task_reply && p.text == "检查通过").count(), 1);
    assert_eq!(reply_message_id(&db, note.id).unwrap(), None);
    assert!(notify_sender(&mut db, note.id, 30, 2, "result", "不同结果", None).is_err());
}

#[test]
fn watched_message_has_one_completion_and_stopping_suppresses_late_notifications() {
    for manual_first in [true, false] {
        let mut db = database();
        message(&db, 20, "user", "我需要结果");
        let ex = enqueue(&mut db, NewRequest { watch_completion: Some(true), ..request("watch", "核对日期") }).unwrap();
        assert!(has_open_owned(&db, 20).unwrap());
        assert!(claim(&db, &ex).unwrap());
        write_input(&mut db, ex.id, "request", 30, "核对日期", 1, "m").unwrap();
        let waiting = reread(&db, ex.id);
        message(&db, 30, "response", "日期正确");
        if manual_first {
            notify_sender(&mut db, ex.id, 30, 2, "done", "日期正确", None).unwrap();
            assert!(!settle(&db, &waiting, false).unwrap());
        } else {
            assert!(settle(&db, &waiting, false).unwrap());
            notify_sender(&mut db, ex.id, 30, 2, "done", "日期正确", None).unwrap();
        }
        assert_eq!(pending_exchanges(&db).unwrap().len(), 1);
        assert!(claim(&db, &reread(&db, ex.id)).unwrap());
        stop_subtree(&db, ex.id).unwrap();
        assert!(write_input(&mut db, ex.id, "reply", 20, "迟到", 1, "m").is_err());
        assert!(notify_sender(&mut db, ex.id, 30, 2, "late", "迟到", None).is_err());
        assert!(pending_receipts(&db).unwrap().is_empty());
        assert!(!has_open_owned(&db, 20).unwrap());
    }
}

#[test]
fn background_watch_accepts_silent_completion_and_ignores_unrelated_replies() {
    for silent in [true, false] {
        let mut db = database();
        message(&db, 20, "user", "关注结果");
        let ex = enqueue(&mut db, NewRequest { watch_completion: Some(true), ..request("watch", "核对") }).unwrap();
        claim(&db, &ex).unwrap();
        write_input(&mut db, ex.id, "request", 30, "核对", 1, "m").unwrap();
        if silent { message(&db, 30, "response", "[[NO_REPLY]]"); }
        message(&db, 30, "user", "顺便问件事");
        message(&db, 30, "response", "另外一件事的回复");
        assert!(settle(&db, &reread(&db, ex.id), false).unwrap());
        let result = reread(&db, ex.id);
        if silent {
            assert_eq!(result.state, "returning");
            assert_eq!(result.reply.as_deref(), Some("收件助手已处理消息，没有附加说明。"));
        } else {
            assert_eq!(result.state, "failed");
            assert!(result.reply.is_none());
        }
    }
}

#[test]
fn nested_watch_resumes_original_work_after_user_interjection() {
    let mut db = database();
    message(&db, 10, "user", "请核对后汇总");
    let original = enqueue(&mut db, NewRequest { watch_completion: Some(true),
        ..delegate(10, 20, "verify", "核对日期") }).unwrap();
    claim(&db, &original).unwrap();
    write_input(&mut db, original.id, "request", 20, "核对日期", 1, "m").unwrap();
    let child = enqueue(&mut db, NewRequest { watch_completion: Some(true),
        ..delegate(20, 30, "source", "查原始日期") }).unwrap();
    message(&db, 20, "response", "[[NO_REPLY]]");
    message(&db, 20, "user", "先聊别的");
    message(&db, 20, "response", "答复新问题");
    claim(&db, &child).unwrap();
    write_input(&mut db, child.id, "request", 30, "查原始日期", 1, "m").unwrap();
    let reply = notify_sender(&mut db, child.id, 30, 2, "date", "周五", None).unwrap();
    assert!(claim(&db, &reply).unwrap());
    write_input(&mut db, child.id, "reply", 20, "周五", 1, "m").unwrap();
    assert!(!settle(&db, &original, false).unwrap());
    message(&db, 20, "response", "核对后确认为周五");
    assert!(settle(&db, &original, false).unwrap());
    assert_eq!(reread(&db, original.id).reply.as_deref(), Some("核对后确认为周五"));
}

#[test]
fn replaced_sender_goal_withdraws_a_queued_completion() {
    let mut db = database();
    db.execute_batch("ALTER TABLE message ADD COLUMN parent_id INTEGER").unwrap();
    let goal = crate::db::butler_goal::apply(&db, 20, "replace", "核对").unwrap().unwrap();
    let root = message(&db, 20, "user", "核对");
    crate::db::butler_goal::bind_turn(&db, 20, root, goal.revision).unwrap();
    let ex = enqueue(&mut db, NewRequest { watch_completion: Some(true), ..request("check", "核对") }).unwrap();
    claim(&db, &ex).unwrap();
    write_input(&mut db, ex.id, "request", 30, "核对", 1, "m").unwrap();
    let reply = notify_sender(&mut db, ex.id, 30, 2, "done", "完成", None).unwrap();
    claim(&db, &reply).unwrap();
    crate::db::butler_goal::apply(&db, 20, "replace", "新目标").unwrap();
    assert!(write_input(&mut db, ex.id, "reply", 20, "旧结果", 1, "m").is_err());
    let stopped = reread(&db, ex.id);
    assert!(stopped.stop_requested);
    assert!(pending_receipts(&db).unwrap().is_empty());
}
#[test]
fn test_butler_peer_round_trip_duplicate_limit_and_late_stop() {
    let mut db = database();
    let root = message(&db, 20, "user", "请审两轮");
    for (n, issue) in ["遗漏回滚步骤", "验收通过"].iter().enumerate() {
        let body = format!("稿件 v{}", n + 1);
        let ex = enqueue(&mut db, request(&body, &body)).unwrap();
        assert_eq!(enqueue(&mut db, request(&body, &body)).unwrap().id, ex.id);
        assert!(enqueue(&mut db, request(&body, "different payload")).is_err());
        assert_eq!((ex.root, ex.round_no), (root, n as i64 + 1));
        assert!(claim(&db, &ex).unwrap());
        assert!(!claim(&db, &ex).unwrap());
        let mid = write_input(&mut db, ex.id, "request", 30, &body, 1, "model").unwrap();
        assert!(write_input(&mut db, ex.id, "request", 30, &body, 1, "model").is_err());
        assert!(enqueue(
            &mut db,
            NewRequest { owner: 30, target: 20, ..request("reverse", "重复反向派工") }
        )
        .is_err());
        message(&db, 30, "response", issue);
        // An unrelated user and its reply cannot be mistaken for the peer answer.
        message(&db, 30, "user", "别的问题");
        message(&db, 30, "response", "无关内容");
        let waiting = reread(&db, ex.id);
        assert_eq!(waiting.input_id, Some(mid));
        settle(&db, &waiting, false).unwrap();
        let returning = reread(&db, ex.id);
        assert_eq!(returning.reply.as_deref(), Some(*issue));
        assert!(claim(&db, &returning).unwrap());
        write_input(&mut db, ex.id, "reply", 20, issue, 1, "model").unwrap();
    }
    assert!(enqueue(&mut db, request("third", "第三轮")).unwrap_err().to_string().contains("2 轮"));
    message(&db, 20, "user", "新委托");
    let ex = enqueue(&mut db, request("new", "再审")).unwrap();
    claim(&db, &ex).unwrap();
    assert_eq!(stop(&db, ex.root, 50).unwrap(), 0);
    assert_eq!(stop(&db, ex.root, 20).unwrap(), 1);
    assert!(write_input(&mut db, ex.id, "request", 30, "已停止", 1, "model").is_err());
}

#[test]
fn test_butler_peer_history_scope_and_user_anchors() {
    let mut db = database();
    let names = HashMap::from([(1, "总管家".into()), (2, "写作".into()), (3, "审稿".into())]);
    let human = message(&db, 30, "user", "以用户意图为准");
    for _ in 0..12 {
        message(&db, 30, "response", "处理记录");
    }
    let hit = message(&db, 30, "response", "V2: RollBack code");
    for _ in 0..12 {
        message(&db, 30, "assistant", "其他公开回复");
    }
    let later = message(&db, 30, "user", "请保留编号");
    let normal = message(&db, 50, "user", "Rollback 私聊");
    message(&db, 20, "user", "Rollback 客户端");
    message(&db, 40, "user", "Rollback 派工");
    let task = message(&db, 40, "response", "Rollback 任务结果");
    let peer = enqueue(&mut db, request("review", "协作")).unwrap();
    claim(&db, &peer).unwrap();
    let inbound =
        write_input(&mut db, peer.id, "request", 30, "Rollback 协作指令", 1, "model").unwrap();
    let hits = butler_history::search(&db, &names, "rollback", None, None).unwrap();
    assert_eq!(hits["matches"].as_array().unwrap().len(), 5);
    assert_eq!(hits["matches"][0]["bot_name"], "审稿");
    assert_eq!(hits["matches"][0]["speaker"], "写作");
    assert_eq!(
        butler_history::context(&db, &names, inbound, None).unwrap()["user_before"]["message_id"],
        later
    );
    let context = butler_history::context(&db, &names, hit, None).unwrap();
    assert_eq!(context["messages"].as_array().unwrap().len(), 15);
    assert_eq!(context["messages"][7]["message_id"], hit);
    assert_eq!(context["user_before"]["message_id"], human);
    assert_eq!(context["user_after"]["message_id"], later);
    assert!(butler_history::context(&db, &names, normal, None).is_err());
    assert!(butler_history::context(&db, &names, task, None).unwrap()["user_before"].is_null());
}

#[test]
fn test_butler_peer_missing_answer_and_cancellation_never_become_success() {
    let mut db = database();
    message(&db, 20, "user", "审核");
    let ex = enqueue(&mut db, request("review", "审稿")).unwrap();
    claim(&db, &ex).unwrap();
    write_input(&mut db, ex.id, "request", 30, "审稿", 1, "model").unwrap();
    message(&db, 30, "user", "用户插话");
    message(&db, 30, "response", "这个答复不能回流");
    settle(&db, &reread(&db, ex.id), false).unwrap();
    assert_eq!(reread(&db, ex.id).state, "failed");
}

#[test]
fn test_butler_peer_delayed_tool_respects_original_user_turn() {
    let mut db = database();
    let first = message(&db, 20, "user", "第一件事");
    let anchor = message(&db, 20, "response", "发起协作");
    let ex =
        enqueue(&mut db, NewRequest { tool_message_id: Some(anchor), ..request("review", "初稿") })
            .unwrap();
    message(&db, 20, "user", "第二件事");
    let replay =
        enqueue(&mut db, NewRequest { tool_message_id: Some(anchor), ..request("review", "初稿") })
            .unwrap();
    assert_eq!((replay.id, replay.root), (ex.id, first));
    assert!(enqueue(
        &mut db,
        NewRequest { tool_message_id: Some(anchor), ..request("new", "延迟执行") }
    )
    .is_err());
}

#[test]
fn test_butler_peer_history_pagination_and_sender_scope() {
    let db = database();
    for _ in 0..15 {
        message(&db, 30, "response", "release 归档");
    }
    message(&db, 20, "user", "release 写作任务");
    message(&db, 30, "reasoning", "release 不作为会话消息返回");
    let internal = message(&db, 30, "user", "release 内部回调");
    db.execute(
        "UPDATE message SET metadata_json=?1 WHERE id=?2",
        params![r#"{"origin":"internal"}"#, internal],
    )
    .unwrap();
    let names = HashMap::from([(2, "写作".into()), (3, "审稿".into())]);
    let page = butler_history::search(&db, &names, "ReLeAsE", Some(3), None).unwrap();
    let hits = page["matches"].as_array().unwrap();
    assert_eq!(hits.len(), 12);
    assert!(hits.iter().all(|h| h["bot_name"] == "审稿" && h["role"] == "response"));
    let next = butler_history::search(
        &db,
        &names,
        "ReLeAsE",
        Some(3),
        page["next_before_message_id"].as_i64(),
    )
    .unwrap();
    assert_eq!(next["matches"].as_array().unwrap().len(), 3);
    assert!(next["next_before_message_id"].is_null());
    assert!(butler_history::context(&db, &names, internal, None).is_err());
}

#[test]
fn test_butler_peer_pending_exchanges_preserves_delivery_queue() {
    let mut db = database();
    message(&db, 20, "user", "请审稿并核对资料");
    let first = enqueue(&mut db, request("review", "稿件正文")).unwrap();
    let second = enqueue(&mut db, NewRequest { target: 10, recipient: "总管家", ..request("check", "核对资料") }).unwrap();
    let pending = pending_exchanges(&db).unwrap();
    assert_eq!(pending.iter().map(|ex| ex.id).collect::<Vec<_>>(), vec![first.id, second.id]);
    assert_eq!(pending[0].body, "稿件正文");

    assert!(claim(&db, &first).unwrap());
    assert_eq!(pending_exchanges(&db).unwrap().len(), 1, "投递中不重复领取");
    write_input(&mut db, first.id, "request", 30, "稿件正文", 1, "model").unwrap();
    assert_eq!(pending_exchanges(&db).unwrap()[0].state, "waiting");
    fail(&db, first.id, "模型不可用").unwrap();
    let pending = pending_exchanges(&db).unwrap();
    assert_eq!(pending.len(), 1, "终态协作不重新进入投递队列");
    assert_eq!(pending[0].id, second.id);
}

#[test]
fn test_butler_peer_stop_preserves_delivered_round_and_blocks_new_one() {
    let mut db = database();
    let root = message(&db, 20, "user", "请审稿");
    let ex = enqueue(&mut db, request("rev1", "初稿")).unwrap();
    claim(&db, &ex).unwrap();
    write_input(&mut db, ex.id, "request", 30, "初稿", 1, "model").unwrap();
    message(&db, 30, "response", "修改建议");
    settle(&db, &reread(&db, ex.id), false).unwrap();
    claim(&db, &reread(&db, ex.id)).unwrap();
    write_input(&mut db, ex.id, "reply", 20, "修改建议", 1, "model").unwrap();
    stop(&db, root, 20).unwrap();
    let stored = reread(&db, ex.id);
    assert_eq!(stored.state, "delivered");
    assert!(stored.stop_requested);
    assert!(enqueue(&mut db, request("rev2", "新稿")).is_err());
    assert_eq!(stored.reply.as_deref(), Some("修改建议"));
    assert!(pending_exchanges(&db).unwrap().is_empty());
}

#[test]
fn test_butler_peer_timeline_both_chats_survive_backfill_and_retry() {
    use crate::db::butler_timeline;
    let mut db = database();
    message(&db, 20, "user", "请审阅");
    let ex = enqueue(&mut db, request("r", "修订稿")).unwrap();
    enqueue(&mut db, request("r", "修订稿")).unwrap();
    claim(&db, &ex).unwrap();
    let received = write_input(&mut db, ex.id, "request", 30, "内部投递指令", 1, "model").unwrap();
    let answer = message(&db, 30, "response", "复审通过");
    settle(&db, &reread(&db, ex.id), false).unwrap();
    claim(&db, &reread(&db, ex.id)).unwrap();
    write_input(&mut db, ex.id, "reply", 20, "内部答复指令", 1, "model").unwrap();
    for migrate in [false, true] {
        if migrate {
            db.execute_batch("DROP TABLE butler_peer_post").unwrap();
            butler_timeline::schema(&db).unwrap();
            butler_timeline::schema(&db).unwrap();
        }
        for (conv, peer_id, peer_name, legs) in [(20, 3, "审稿", [true,false]), (30, 2, "写作", [false,true])] {
            let mut posts = serde_json::to_value(butler_timeline::list(&db, conv).unwrap()).unwrap();
            // Backfilled event IDs reflect insert order, timeline position comes from anchors.
            posts.as_array_mut().unwrap().sort_by_key(|row| row["anchor"].as_i64());
            assert_eq!(posts.as_array().unwrap().len(), 2);
            for (i, text) in ["修订稿","复审通过"].iter().enumerate() {
                assert_eq!(posts[i]["text"], *text);
                assert_eq!(posts[i]["outgoing"], legs[i]);
                assert_eq!(posts[i]["peer"], peer_name);
                assert_eq!(posts[i]["peer_assistant"], peer_id);
            }
            if conv == 30 {
                assert_eq!(posts[0]["anchor"], received);
                assert_eq!(posts[1]["anchor"], answer);
            }
        }
    }
    let repo = crate::db::conversation_db::ConversationRepository::new(db);
    let previews = repo.butler_contact_previews().unwrap();
    assert!(previews.iter().any(|p| p["assistantId"] == 2 && p["preview"] == "来自 审稿：复审通过"));
}

#[test]
fn test_butler_previews_track_only_public_replies_in_current_conversations() {
    let conn = database();
    let reply = message(&conn, 10, "response", "收到");
    message(&conn, 10, "user", "再看一下");
    message(&conn, 20, "response", "[[NO_REPLY]]");
    let hidden_input = message(&conn, 20, "user", "内部协作");
    conn.execute("UPDATE message SET metadata_json=?1 WHERE id=?2", params![r#"{"origin":"butler_peer","audience":"peer"}"#, hidden_input]).unwrap();
    let hidden = message(&conn, 20, "response", "仅供助手参考");
    let repo = crate::db::conversation_db::ConversationRepository::new(conn);
    assert!(!repo.is_public_butler_reply(20, hidden).unwrap());
    assert!(repo.is_public_butler_reply(10, reply).unwrap());
    let rows = repo.butler_contact_previews().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.iter().find(|r| r["assistantId"] == 0).unwrap()["replyId"], reply);
    assert!(rows.iter().find(|r| r["assistantId"] == 2).unwrap()["replyId"].is_null());
}

#[test]
fn test_butler_peer_separate_recipients_can_receive_same_delegation() {
    let mut db = database();
    message(&db, 20, "user", "请同时联系两位助手");
    enqueue(&mut db, request("review", "审稿")).unwrap();
    assert!(enqueue(&mut db, request("repeat", "提前追问")).is_err());
    let second = enqueue(&mut db, NewRequest { target:10, recipient:"总管家", ..request("chief","确认") }).unwrap();
    assert_eq!(second.round_no, 2);
}

#[test]
fn test_butler_peer_long_hit_shows_original_keyword_in_context() {
    let db = database();
    let id = message(&db, 30, "response", &format!("{}验收代号", "甲".repeat(2500)));
    let context = butler_history::context(&db, &HashMap::new(), id, Some("验收代号")).unwrap();
    assert!(context["messages"][0]["text"].as_str().unwrap().contains("验收代号"));
    assert_eq!(context["messages"][0]["truncated"], true);
}

#[test]
fn test_butler_peer_backfilled_user_message_is_placed_in_chat_order() {
    let db = database();
    let hit = message(&db, 30, "assistant", "命中");
    let before = message(&db, 30, "user", "补录的原始意图");
    db.execute("UPDATE message SET created_time='2001-01-01' WHERE id=?1", [before]).unwrap();
    let context = butler_history::context(&db, &HashMap::new(), hit, None).unwrap();
    assert_eq!(context["messages"][0]["message_id"], before);
    assert_eq!(context["user_before"]["message_id"], before);
    assert!(context["user_after"].is_null());
}

#[tokio::test]
#[ignore = "opt-in model calls with synthetic Bot messages and in-memory SQLite"]
async fn test_live_peer_review_history() {
    use genai::chat::{ChatOptions, ChatRequest, Tool};
    let (client, model) = crate::api::live_test_client::configured_client();
    let catalog = crate::mcp::builtin_mcp::get_builtin_tools_for_command("aipp:agent");
    let call = async |name: &str, prompt: String| {
        let def = catalog.iter().find(|tool| tool.name == name).unwrap();
        let tool = Tool::new(name)
            .with_description(def.description.clone())
            .with_schema(def.input_schema.clone());
        let response = client
            .exec_chat(
                &model,
                ChatRequest::from_user(prompt).with_tools(vec![tool]),
                Some(&ChatOptions::default().with_max_tokens(1500).with_capture_tool_calls(true)),
            )
            .await
            .unwrap();
        let calls = response.tool_calls();
        assert_eq!(calls.len(), 1, "expected one {name} call");
        assert_eq!(calls[0].fn_name, name);
        calls[0].fn_arguments.clone()
    };
    let mut db = database();
    let names = HashMap::from([(1, "总管家".into()), (2, "写作".into()), (3, "审稿".into())]);
    message(&db, 10, "user", "发布说明必须包含回滚代号 JUNIPER-74。");
    let hit = message(&db, 10, "response", "ORCHID-RELEASE 已登记，可以交给写作助手。");
    message(&db, 20, "user", "写发布说明，交给审稿助手复审。");
    let search = call(
        "search_bot_history",
        "Use search_bot_history with the exact query ORCHID-RELEASE.".into(),
    )
    .await;
    let hits =
        butler_history::search(&db, &names, search["query"].as_str().unwrap(), None, None).unwrap();
    assert_eq!(hits["matches"][0]["bot_name"], "总管家");
    let lookup = call(
        "read_bot_message_context",
        format!("Use read_bot_message_context to read the matching message: {hits}"),
    )
    .await;
    assert_eq!(lookup["message_id"], hit);
    let context = butler_history::context(&db, &names, hit, None).unwrap();
    assert!(context["user_before"]["text"].as_str().unwrap().contains("JUNIPER-74"));
    let mut feedback = String::new();
    for round in 1..=2 {
        let prompt = if round == 1 {
            "你是写作助手。用 message_bot 向审稿发送初稿「发布新版」，本轮仅含初稿，不要补充回滚代号。最多两轮，request_id=draft1。".into()
        } else {
            format!("审稿反馈：{feedback}。历史用户验收要求：{}。用 message_bot 把修订稿发给审稿，包含具体回滚代号，request_id=draft2。",context["user_before"]["text"])
        };
        let args = call("message_bot", prompt).await;
        assert_eq!(args["to"], "审稿");
        let text = args["message"].as_str().unwrap();
        let ex = enqueue(&mut db, request(args["request_id"].as_str().unwrap(), text)).unwrap();
        assert!(claim(&db, &ex).unwrap());
        write_input(&mut db, ex.id, "request", 30, text, 1, &model).unwrap();
        let review=client.exec_chat(&model,ChatRequest::from_user(format!(
            "审核发布说明；只有草稿实际包含回滚代号 JUNIPER-74 才回复「通过」，否则要求补充回滚代号。草稿：{text}")),
            Some(&ChatOptions::default().with_max_tokens(1500))).await.unwrap();
        let answer = review.first_text().unwrap();
        message(&db, 30, "response", answer);
        settle(&db, &reread(&db, ex.id), false).unwrap();
        let returning = reread(&db, ex.id);
        assert!(claim(&db, &returning).unwrap());
        feedback = returning.reply.unwrap();
        write_input(&mut db, ex.id, "reply", 20, &feedback, 1, &model).unwrap();
        if round == 1 {
            assert!(!text.contains("JUNIPER-74"));
        } else {
            assert!(text.contains("JUNIPER-74"));
            assert!(feedback.contains("通过"));
        }
        println!("round {round}: {feedback}");
    }
}

#[test]
fn test_butler_peer_report_to_user_persists_and_is_part_of_idempotency() {
    let mut db = database();
    message(&db, 20, "user", "让审稿查完直接告诉我");
    let direct = NewRequest { report_to_user: Some(true), ..request("direct", "核查并直接汇报") };
    let ex = enqueue(&mut db, direct).unwrap();
    assert!(ex.report_to_user);
    assert!(reread(&db, ex.id).report_to_user);
    // 同一 request_id 改变汇报方式视为不同请求，不能静默复用旧协作。
    assert!(enqueue(&mut db, request("direct", "核查并直接汇报")).is_err());
    // 迁移可重复执行。
    schema(&db).unwrap();
}

/// 成员失败或被停止后，发起方收到且只收到一次回执；回执后发起方可继续处理
#[test]
fn test_butler_peer_failure_and_stop_send_single_receipt_to_owner() {
    let mut db = database();
    message(&db, 20, "user", "请审稿");
    let ex = enqueue(&mut db, request("r1", "稿件")).unwrap();
    assert!(claim(&db, &ex).unwrap());
    write_input(&mut db, ex.id, "request", 30, "稿件", 1, "model").unwrap();
    db.execute(
        "INSERT INTO message(conversation_id,message_type,content,finish_time) VALUES (30,'error','模型认证失败',CURRENT_TIMESTAMP)",
        [],
    )
    .unwrap();
    settle(&db, &reread(&db, ex.id), false).unwrap();
    let failed = reread(&db, ex.id);
    assert_eq!(failed.state, "failed");
    assert!(failed.error.as_deref().unwrap().contains("认证失败"), "保留失败原因");
    assert_eq!(failed.receipt_state.as_deref(), Some("pending"));

    let pending = pending_receipts(&db).unwrap();
    assert_eq!(pending.iter().map(|e| e.id).collect::<Vec<_>>(), vec![ex.id]);
    assert!(claim_receipt(&db, &failed).unwrap());
    assert!(!claim_receipt(&db, &failed).unwrap(), "同一回执只领取一次");
    assert!(write_input(&mut db, ex.id, "receipt", 30, "回执", 1, "model").is_err(), "回执只能送到发起方");
    write_input(&mut db, ex.id, "receipt", 20, "回执", 1, "model").unwrap();
    assert!(write_input(&mut db, ex.id, "receipt", 20, "回执", 1, "model").is_err(), "不会重复送达");
    assert_eq!(reread(&db, ex.id).receipt_state.as_deref(), Some("delivered"));
    assert!(pending_receipts(&db).unwrap().is_empty());

    // 收到回执后发起方可以继续发起下一轮（例如改派），不会被当作收件人派工拦截
    let next = enqueue(&mut db, NewRequest { target: 10, recipient: "总管家", ..request("r2", "改派") }).unwrap();
    assert_eq!(next.round_no, 2);

    // 进行中的协作被停止：同样回执一次
    assert!(claim(&db, &next).unwrap());
    assert_eq!(stop(&db, next.root, 20).unwrap(), 2);
    let stopped = reread(&db, next.id);
    assert_eq!((stopped.state.as_str(), stopped.receipt_state.as_deref()), ("stopped", Some("pending")));
    assert_eq!(reread(&db, ex.id).receipt_state.as_deref(), Some("delivered"), "已失败的不再重复回执");
}

/// 投递中退出：结果标记为未知且不重复投递；回执未写入时退回待送重发
#[test]
fn test_butler_peer_recover_marks_unknown_without_redelivery() {
    let mut db = database();
    message(&db, 20, "user", "请审稿");
    let ex = enqueue(&mut db, request("r1", "稿件")).unwrap();
    assert!(claim(&db, &ex).unwrap());
    recover(&db).unwrap();
    let failed = reread(&db, ex.id);
    assert_eq!(failed.state, "failed");
    assert!(failed.error.as_deref().unwrap().contains("未知"));
    assert!(claim_receipt(&db, &failed).unwrap());
    recover(&db).unwrap();
    assert_eq!(reread(&db, ex.id).receipt_state.as_deref(), Some("pending"), "回执未写入，重启后重发");
    assert_eq!(pending_receipts(&db).unwrap().len(), 1);
}

/// 回执投递失败会重试，达到上限后停止；已送达后失败不会重发
#[test]
fn test_butler_peer_receipt_retries_until_limit_without_duplicates() {
    let mut db = database();
    message(&db, 20, "user", "请审稿");
    let ex = enqueue(&mut db, request("r1", "稿件")).unwrap();
    fail(&db, ex.id, "接收方模型错误").unwrap();
    for attempt in 1..MAX_RECEIPT_ATTEMPTS {
        assert!(claim_receipt(&db, &reread(&db, ex.id)).unwrap(), "attempt {attempt}");
        fail_receipt(&db, ex.id).unwrap();
        assert_eq!(reread(&db, ex.id).receipt_state.as_deref(), Some("pending"));
    }
    assert!(claim_receipt(&db, &reread(&db, ex.id)).unwrap());
    fail_receipt(&db, ex.id).unwrap();
    assert_eq!(reread(&db, ex.id).receipt_state.as_deref(), Some("failed"));

    let second = enqueue(&mut db, NewRequest { target: 10, recipient: "总管家", ..request("r2", "改派") });
    let second = second.unwrap();
    fail(&db, second.id, "失败").unwrap();
    assert!(claim_receipt(&db, &reread(&db, second.id)).unwrap());
    write_input(&mut db, second.id, "receipt", 20, "回执", 1, "model").unwrap();
    fail_receipt(&db, second.id).unwrap();
    assert_eq!(reread(&db, second.id).receipt_state.as_deref(), Some("delivered"), "已送达不会重发");
}

/// 旧版本遗留的未送达回执（unknown、首次失败即放弃的 failed）在启动时恢复为待送
#[test]
fn test_butler_peer_recover_requeues_legacy_undelivered_receipts() {
    let mut db = database();
    message(&db, 20, "user", "请审稿");
    let a = enqueue(&mut db, request("a", "一")).unwrap();
    let b = enqueue(&mut db, NewRequest { target: 10, recipient: "总管家", ..request("b", "二") }).unwrap();
    db.execute("UPDATE butler_peer_exchange SET state='failed',receipt_state='unknown' WHERE id=?1", [a.id]).unwrap();
    db.execute("UPDATE butler_peer_exchange SET state='failed',receipt_state='failed',receipt_attempts=0 WHERE id=?1", [b.id]).unwrap();
    recover(&db).unwrap();
    assert_eq!(reread(&db, a.id).receipt_state.as_deref(), Some("pending"));
    assert_eq!(reread(&db, b.id).receipt_state.as_deref(), Some("pending"));
    // 新版本重试到上限的 failed 不再恢复
    db.execute("UPDATE butler_peer_exchange SET receipt_state='failed',receipt_attempts=?2 WHERE id=?1",
        params![b.id, MAX_RECEIPT_ATTEMPTS]).unwrap();
    recover(&db).unwrap();
    assert_eq!(reread(&db, b.id).receipt_state.as_deref(), Some("failed"));
}

fn meta(conn: &Connection, id: i64) -> serde_json::Value {
    let raw: String = conn.query_row("SELECT metadata_json FROM message WHERE id=?1", [id], |r| r.get(0)).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn chain_database() -> Connection {
    let db = database();
    db.execute_batch("INSERT INTO conversation VALUES (60,4,'资料','butler_contact',NULL),(70,5,'排版','butler_contact',NULL);").unwrap();
    db
}

fn delegate<'a>(owner: i64, target: i64, key: &'a str, body: &'a str) -> NewRequest<'a> {
    NewRequest { owner, target, limit: 6, report_to_user: None, ..request(key, body) }
}

/// 总管家派给甲（默认由甲直接答复用户）；甲在受托回合里请乙协助（默认内部协作），乙答复回到甲，
/// 甲继续工作后交付，上级协作才结束。
#[test]
fn test_butler_peer_delegate_requests_help_and_continues_after_reply() {
    let mut db = chain_database();
    message(&db, 10, "user", "让写作写一篇稿子");
    let top = enqueue(&mut db, delegate(10, 20, "write", "写稿")).unwrap();
    assert!(top.report_to_user, "直接受用户委托的工作默认由受托助手答复用户");
    assert_eq!((top.depth, top.parent), (1, None));
    assert!(claim(&db, &top).unwrap());
    let input = write_input(&mut db, top.id, "request", 20, "写稿", 1, "model").unwrap();
    assert_eq!(meta(&db, input)["audience"], "user");

    let help = enqueue(&mut db, delegate(20, 30, "review", "请审稿")).unwrap();
    assert_eq!((help.parent, help.depth, help.root), (Some(top.id), 2, top.root));
    assert!(!help.report_to_user, "受托期间请来的协助默认是内部协作");
    message(&db, 20, "response", "已请审稿协助");
    assert!(!settle(&db, &reread(&db, top.id), false).unwrap(), "下级未返回时上级不能结束");
    assert_eq!(reread(&db, top.id).state, "waiting");

    assert!(claim(&db, &help).unwrap());
    let inbound = write_input(&mut db, help.id, "request", 30, "请审稿", 1, "model").unwrap();
    assert_eq!(meta(&db, inbound)["audience"], "peer");
    message(&db, 30, "response", "意见：补充数据来源");
    assert!(settle(&db, &reread(&db, help.id), false).unwrap());
    let returning = reread(&db, help.id);
    assert!(claim(&db, &returning).unwrap(), "上级仍在等待时，下级答复也能送回受托方");
    let back = write_input(&mut db, help.id, "reply", 20, "意见", 1, "model").unwrap();
    assert_eq!(meta(&db, back)["audience"], "user", "甲继续的是面向用户的工作");
    assert_eq!(meta(&db, back)["parent"], top.id);

    message(&db, 20, "response", "终稿（已补充数据来源）");
    assert!(settle(&db, &reread(&db, top.id), false).unwrap());
    let done = reread(&db, top.id);
    assert_eq!((done.state.as_str(), done.reply.as_deref()), ("returning", Some("终稿（已补充数据来源）")));
}

#[test]
fn test_butler_peer_rejects_loops_depth_overflow_and_stopped_parent() {
    let mut db = chain_database();
    message(&db, 10, "user", "多层协作");
    let mut hops = Vec::new();
    for (owner, target, key) in [(10, 20, "a"), (20, 30, "b"), (30, 60, "c")] {
        let ex = enqueue(&mut db, delegate(owner, target, key, key)).unwrap();
        assert!(claim(&db, &ex).unwrap());
        write_input(&mut db, ex.id, "request", target, key, 1, "model").unwrap();
        hops.push(ex);
    }
    assert_eq!(hops[2].depth, MAX_DEPTH);
    let deep = enqueue(&mut db, delegate(60, 70, "d", "再转派")).unwrap_err().to_string();
    assert!(deep.contains("层上限"), "{deep}");
    // 协作链上已有的助手（含总管家）不能被拉回来。
    for target in [10, 20] {
        let looped = enqueue(&mut db, NewRequest { recipient: "链上助手", ..delegate(30, target, "loop", "回头问") })
            .unwrap_err().to_string();
        assert!(looped.contains("循环"), "{looped}");
    }
    // 停止整条协作后，受托方不能再为它联系其他助手。
    stop(&db, hops[0].root, 10).unwrap();
    assert!(enqueue(&mut db, delegate(30, 70, "late", "迟到的协助")).is_err());
}

#[test]
fn test_butler_peer_silent_reply_is_not_a_delivery() {
    let mut db = chain_database();
    message(&db, 10, "user", "任务");
    let ex = enqueue(&mut db, delegate(10, 20, "w", "写")).unwrap();
    claim(&db, &ex).unwrap();
    write_input(&mut db, ex.id, "request", 20, "写", 1, "model").unwrap();
    message(&db, 20, "response", "[[NO_REPLY]]");
    assert!(!settle(&db, &reread(&db, ex.id), false).unwrap());
    assert!(settle(&db, &reread(&db, ex.id), true).unwrap());
    assert_eq!(reread(&db, ex.id).state, "failed", "没有可确认的最终答复");
}

/// 归档联系人私聊前按受理方查找仍在进行的协作：只返回以该会话为接收方、尚未结束的协作。
#[test]
fn test_butler_peer_active_in_target_lists_only_open_requests_to_conversation() {
    let mut db = database();
    message(&db, 20, "user", "请审稿");
    let done = enqueue(&mut db, request("done", "初稿")).unwrap();
    fail(&db, done.id, "测试：已结束").unwrap();
    let open = enqueue(&mut db, request("open", "二稿")).unwrap();
    let ids: Vec<i64> = active_in_target(&db, 30).unwrap().iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![open.id]);
    assert!(active_in_target(&db, 20).unwrap().is_empty(), "发起方不是受理方");
}

/// 独立任务发起协作：未闭环时任务不收尾；整项停止只停本任务的协作，不给被取消的任务自己回执。
#[test]
fn test_butler_peer_task_owned_exchanges_block_finalize_and_stop_with_task() {
    let mut db = chain_database();
    message(&db, 40, "user", "任务说明：整理资料");
    message(&db, 20, "user", "与任务无关的私聊");
    let ex = enqueue(&mut db, delegate(40, 60, "help", "请提供资料")).unwrap();
    assert!(!ex.report_to_user, "任务请来的协助是任务资料，默认内部协作，由任务自己最终交付");
    assert!(has_open_owned(&db, 40).unwrap());
    assert!(!has_open_owned(&db, 20).unwrap());
    assert_eq!(stop_owned(&db, 40).unwrap(), vec![ex.id]);
    let stopped = reread(&db, ex.id);
    assert_eq!((stopped.state.as_str(), stopped.receipt_state), ("stopped", None));
    assert!(!has_open_owned(&db, 40).unwrap());
}

/// 同一助手从独立任务会话绕到联系人会话也构成循环：按助手身份判断，而不只比较会话 ID。
#[test]
fn test_butler_peer_loop_is_detected_by_assistant_identity_across_conversations() {
    let mut db = chain_database();
    // 任务会话 40 的执行助手是 3（审稿），联系人 30 也是审稿助手 3。
    message(&db, 40, "user", "任务说明：核对资料");
    let from_task = enqueue(&mut db, delegate(40, 60, "ask", "请提供资料")).unwrap();
    assert!(claim(&db, &from_task).unwrap());
    write_input(&mut db, from_task.id, "request", 60, "请提供资料", 1, "model").unwrap();
    let error = enqueue(&mut db, NewRequest { recipient: "审稿", ..delegate(60, 30, "back", "回头问审稿") })
        .unwrap_err()
        .to_string();
    assert!(error.contains("循环"), "{error}");
    // 链外的其他助手仍可协助。
    assert!(enqueue(&mut db, delegate(60, 70, "other", "请排版")).is_ok());
}

/// 旧版截止时间已过时仍可投递、回送并继续委托，独立任务沿用同一条链的轮数预算。
#[test]
fn test_butler_peer_continues_after_legacy_deadline_and_shares_round_budget() {
    let mut db = chain_database();
    let root = message(&db, 10, "user", "多轮工作");
    let first = enqueue(&mut db, NewRequest { limit: 3, ..delegate(10, 20, "r1", "第一轮") }).unwrap();
    db.execute_batch("ALTER TABLE butler_peer_exchange ADD COLUMN expires_at TEXT").unwrap();
    db.execute("UPDATE butler_peer_exchange SET expires_at='2000-01-01T00:00:00Z' WHERE id=?1", [first.id]).unwrap();
    assert!(claim(&db, &first).unwrap());
    write_input(&mut db, first.id, "request", 20, "第一轮", 1, "model").unwrap();
    message(&db, 20, "response", "第一轮结果");
    settle(&db, &reread(&db, first.id), false).unwrap();
    let back = reread(&db, first.id);
    assert!(claim(&db, &back).unwrap());
    write_input(&mut db, first.id, "reply", 10, "第一轮结果", 1, "model").unwrap();
    let second = enqueue(&mut db, delegate(10, 30, "r2", "第二轮")).unwrap();
    assert_eq!(second.chain, root);
    assert_eq!(second.round_no, 2);
    // 派出的独立任务属于同一项工作：任务里发起的协作计入同一条链的轮数预算。
    link_task(&db, 40, root, None).unwrap();
    message(&db, 40, "user", "任务说明");
    let from_task = enqueue(&mut db, delegate(40, 60, "t1", "任务里请资料")).unwrap();
    assert_eq!((from_task.chain, from_task.round_no), (root, 3));
    let over = enqueue(&mut db, delegate(40, 70, "t2", "超出预算")).unwrap_err().to_string();
    assert!(over.contains("3 轮"), "{over}");
    assert_eq!(linked_tasks(&db, root).unwrap(), vec![40]);
}

/// 当前这一轮属于哪项工作：请求方向是该协作，回送方向是继续处理的上级协作；只停止某项委托及其下级，不误停并行工作。
#[test]
fn test_butler_peer_turn_work_and_subtree_stop_spare_parallel_work() {
    let mut db = chain_database();
    message(&db, 10, "user", "同时交给写作和审稿");
    let to_writer = enqueue(&mut db, delegate(10, 20, "w", "写稿")).unwrap();
    let to_reviewer = enqueue(&mut db, delegate(10, 30, "r", "审另一份")).unwrap();
    for ex in [&to_writer, &to_reviewer] {
        assert!(claim(&db, ex).unwrap());
        write_input(&mut db, ex.id, "request", ex.target, "开始", 1, "model").unwrap();
    }
    assert_eq!(turn_work(&db, 20).unwrap(), Some(TurnWork { root: to_writer.root, exchange: Some(to_writer.id) }));
    let help = enqueue(&mut db, delegate(20, 60, "h", "请资料")).unwrap();
    assert!(claim(&db, &help).unwrap());
    write_input(&mut db, help.id, "request", 60, "请资料", 1, "model").unwrap();
    message(&db, 60, "response", "资料");
    settle(&db, &reread(&db, help.id), false).unwrap();
    assert!(claim(&db, &reread(&db, help.id)).unwrap());
    write_input(&mut db, help.id, "reply", 20, "资料", 1, "model").unwrap();
    assert_eq!(turn_work(&db, 20).unwrap().unwrap().exchange, Some(to_writer.id), "回送后仍在处理上级委托");

    let stopped = stop_subtree(&db, to_writer.id).unwrap();
    assert_eq!(stopped, vec![to_writer.id, help.id]);
    assert_eq!(reread(&db, to_writer.id).state, "stopped");
    assert_eq!(reread(&db, to_reviewer.id).state, "waiting", "同一轮并行交给审稿的工作不受影响");
    set_stop_note(&db, &stopped, "写作：已停止正在进行的回合").unwrap();
    assert_eq!(reread(&db, to_writer.id).stop_note.as_deref(), Some("写作：已停止正在进行的回合"));
    assert!(reread(&db, to_reviewer.id).stop_note.is_none());
    assert_eq!(chain_roots(&db, to_writer.chain).unwrap(), vec![to_writer.root]);
    let mut conversations = chain_conversations(&db, to_writer.chain).unwrap();
    conversations.sort();
    assert_eq!(conversations, vec![10, 20, 30, 60]);
}

#[test]
fn test_butler_peer_old_goal_output_cannot_be_settled_as_success() {
    let mut db=database();
    db.execute("ALTER TABLE message ADD COLUMN parent_id INTEGER",[]).unwrap();
    super::super::butler_goal::schema(&db).unwrap();
    message(&db,20,"user","review");
    let ex=enqueue(&mut db,request("old-goal","review old goal")).unwrap();
    assert!(claim(&db,&ex).unwrap());
    let input=write_input(&mut db,ex.id,"request",30,"old",1,"model").unwrap();
    super::super::butler_goal::bind_turn(&db,30,input,1).unwrap();
    let output=message(&db,30,"response","old answer");
    db.execute("UPDATE message SET parent_id=?1 WHERE id=?2",params![input,output]).unwrap();
    super::super::butler_goal::bind_output(&db,30,output).unwrap();
    db.execute("INSERT INTO butler_goal VALUES(30,2,'replacement',0)",[]).unwrap();
    assert!(settle(&db,&reread(&db,ex.id),false).unwrap());
    assert_eq!(reread(&db,ex.id).state,"failed");
    assert_eq!(db.query_row("SELECT content FROM message WHERE id=?1",[output],|r|r.get::<_,String>(0)).unwrap(),"old answer");
}

#[test]
fn test_queued_owner_controls_block_claim_and_delivery_without_hiding_replies() {
    for action in ["pause", "stop_delegation", "cancel"] {
        let mut db = database();
        message(&db, 20, "user", "审稿");
        let ex = enqueue(&mut db, request("controlled", "稿件")).unwrap();
        crate::db::butler_control::set(&db,20,action).unwrap();
        assert!(!claim(&db,&ex).unwrap(), "owner {action} must prevent queued delivery");
        // Simulate the interval between claiming and the final delivery transaction.
        db.execute("UPDATE butler_peer_exchange SET state='sending' WHERE id=?1",[ex.id]).unwrap();
        assert!(write_input(&mut db,ex.id,"request",30,"稿件",1,"model").is_err());
        let count:i64=db.query_row("SELECT count(*) FROM message WHERE conversation_id=30",[],|r|r.get(0)).unwrap();
        assert_eq!(count,0);
    }
    let mut db=database();
    message(&db,20,"user","审稿");
    let ex=enqueue(&mut db,request("reply","稿件")).unwrap();
    assert!(claim(&db,&ex).unwrap());
    write_input(&mut db,ex.id,"request",30,"稿件",1,"model").unwrap();
    crate::db::butler_control::set(&db,20,"stop_delegation").unwrap();
    db.execute("UPDATE butler_peer_exchange SET state='returning',reply='结论' WHERE id=?1",[ex.id]).unwrap();
    assert!(claim(&db,&reread(&db,ex.id)).unwrap());
    assert!(write_input(&mut db,ex.id,"reply",20,"结论",1,"model").is_ok());
}

#[test]
fn test_queued_owner_resume_and_unrelated_work_remain_eligible() {
    let mut db=database();message(&db,20,"user","审稿");
    let ex=enqueue(&mut db,request("resume","稿件")).unwrap();
    crate::db::butler_control::set(&db,40,"cancel").unwrap();
    crate::db::butler_control::set(&db,20,"pause").unwrap();
    assert!(!claim(&db,&ex).unwrap());
    crate::db::butler_control::set(&db,20,"resume").unwrap();
    assert!(claim(&db,&ex).unwrap());
    assert!(write_input(&mut db,ex.id,"request",30,"稿件",1,"model").is_ok());
}

#[test]
fn test_butler_peer_round_extension_is_bound_to_chain_and_budget() {
    let mut db = chain_database();
    let chain = message(&db, 10, "user", "协作");
    enqueue(&mut db, NewRequest { limit: 1, ..delegate(10, 20, "first", "分析") }).unwrap();
    let next = delegate(10, 30, "second", "复核");
    let boundary = *enqueue(&mut db, next).unwrap_err().downcast_ref::<RoundLimit>().unwrap();
    assert_eq!(boundary, RoundLimit { chain, limit: 1 });
    assert!(enqueue_reviewed(&mut db, next, Some(RoundLimit { chain: chain + 1, ..boundary })).is_err());
    let approved = enqueue_reviewed(&mut db, next, Some(boundary)).unwrap();
    assert_eq!((approved.round_no, approved.max_rounds), (2, 51));
    assert_eq!(enqueue(&mut db, next).unwrap().id, approved.id);
    let third = enqueue(&mut db, delegate(10, 60, "third", "补充")).unwrap();
    assert_eq!((third.round_no, third.max_rounds), (3, 51));
}

#[test]
fn test_butler_peer_reviews_each_block_of_fifty_rounds() {
    let mut db = chain_database();
    let chain = message(&db, 10, "user", "分批处理");
    let mut last_approval = None;
    for round in 1..=101 {
        let key = round.to_string();
        let request = NewRequest { limit: ROUND_BLOCK, ..delegate(10, 20, &key, "处理下一批") };
        let exchange = if round == 51 || round == 101 {
            let error = enqueue_reviewed(&mut db, request, last_approval).unwrap_err();
            let boundary = *error.downcast_ref::<RoundLimit>().unwrap();
            assert_eq!(boundary, RoundLimit { chain, limit: round - 1 });
            last_approval = Some(boundary);
            enqueue_reviewed(&mut db, request, last_approval).unwrap()
        } else {
            enqueue(&mut db, request).unwrap()
        };
        assert_eq!(exchange.round_no, round);
        assert_eq!(exchange.max_rounds, ((round - 1) / 50 + 1) * 50);
        db.execute("UPDATE butler_peer_exchange SET state='delivered' WHERE id=?1", [exchange.id]).unwrap();
    }
}

use super::*;

#[test]
fn test_scheduled_delivery_from_due_run_through_persisted_reply_and_replay() {
    use crate::db::{butler_goal, scheduled_task_db::{ClaimResult, OccurrenceClaim}};

    let (chat_db, keeper) = ConversationDatabase::shared_in_memory();
    chat_db.create_tables().unwrap();
    let mut db = ScheduledTaskDatabase::from_connection(keeper);
    db.create_tables().unwrap();
    let defaults = TaskDefaults {
        deliver_to: crate::db::scheduled_task_db::DELIVER_OWNER_CHAT.into(),
        butler_mode: true,
        ..Default::default()
    };
    let input = TaskInput {
        name: "测试".into(), schedule: "every 1h".into(), prompt: "汇总".into(), assistant_id: 40,
        deliver_policy: Some("always".into()), ..Default::default()
    };
    let task = db.create_task(&service::build_task(&input, &defaults, Utc::now()).unwrap()).unwrap();
    let now = task.next_run_at.unwrap();
    let run_id = "regression-delivery";
    let claim = OccurrenceClaim {
        run_id: run_id.into(), now, scheduled_for: now, next_run_at: Some(now + chrono::Duration::hours(1)),
        missed: Vec::new(), main_missed: false, skip_reason: None,
    };
    assert_eq!(db.claim_occurrence(task.id, now, &claim).unwrap(), ClaimResult::Execute);
    db.conn.execute("INSERT INTO conversation(name,conversation_kind,is_hidden_from_normal_chat_list)
        VALUES('run','scheduled_run',1)", []).unwrap();
    let run_chat = db.conn.last_insert_rowid();
    db.set_run_conversation(run_id, run_chat).unwrap();
    let target = chat_db.conversation_repo().unwrap().open_butler_contact(task.assistant_id, "执行助手").unwrap();
    let goal = butler_goal::apply(&db.conn, target, "replace", "正在处理另一项工作").unwrap();
    let cancel = CancellationToken::new();
    let response = |text: &str| genai::chat::ChatResponse {
        content: text.to_owned().into(), reasoning_content: None, usage: Default::default(),
        model_iden: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "model"),
        provider_model_iden: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "model"),
        stop_reason: Some(genai::chat::StopReason::Completed("stop".into())), captured_raw_body: None, response_id: None,
    };
    let call = response("<mcp_tool_call><server_name>test</server_name><tool_name>query</tool_name><parameters>{}</parameters></mcp_tool_call>");
    let intermediate = save_round_response(&chat_db, run_chat, &call, 9, "model", "group").unwrap();
    assert!(intermediate.next_step(&cancel, 1, 0, 0, 0).is_continue());

    let text = "报告中提到 mcp_tool_call 和 MCP_TOOL_CALL，均为正文。";
    let saved = save_round_response(&chat_db, run_chat, &response(text), 9, "model", "group").unwrap();
    let source = saved.message.id;
    let completed = saved.next_step(&cancel, 2, 1, 1, 0).break_value().expect("纯正文应结束执行");
    let outcome = completed.into_outcome(run_chat, 9, "model".into(), &task.deliver_policy);
    let entry = ScheduledOutcomeJournal { task_id: task.id, run_id: run_id.into(), finished_time: now, outcome };
    let journal = serde_json::to_string(&entry).unwrap();
    let mut delivered_ids = Vec::new();
    for _ in 0..2 {
        let entry: ScheduledOutcomeJournal = serde_json::from_str(&journal).unwrap();
        let (failures, _) = store_run_delivery(&db, &entry, |content| {
            let target = delivery::resolve_target(&chat_db, &task, || Ok(None), || "执行助手".into())?;
            delivery::deliver_to_chat(&chat_db, &task, content, target)
        }).unwrap();
        let run = db.read_run(run_id).unwrap().unwrap();
        assert_eq!(failures, 0);
        assert_eq!(run.status, RUN_DELIVERED, "{:?}", run.delivery_error);
        assert!(run.notify && run.delivery_error.is_none());
        assert_eq!((run.conversation_id, run.final_message_id), (Some(run_chat), Some(source)));
        assert_eq!(run.delivery_conversation_id, Some(target));
        delivered_ids.push(run.delivered_message_id.unwrap());
    }
    assert_eq!(delivered_ids[0], delivered_ids[1]);
    let repo = chat_db.message_repo().unwrap();
    let result = repo.read(delivered_ids[0]).unwrap().unwrap();
    let trigger = repo.read(result.parent_id.unwrap()).unwrap().unwrap();
    assert_eq!(trigger.conversation_id, target);
    assert_eq!(trigger.message_type, "user");
    assert_eq!(result.content, text);
    assert_eq!(repo.read(source).unwrap().unwrap().content, result.content);
    assert_eq!(repo.list_by_conversation_id(target).unwrap().len(), 2);
    assert_eq!(butler_goal::current(&db.conn, target).unwrap(), goal);
}

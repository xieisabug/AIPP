use super::*;
use rusqlite::Connection;

const BUDGET: ContextBudgetInfo = ContextBudgetInfo {
    enabled: true,
    context_window_size: 128_000,
    effective_input_limit: 115_712,
    compaction_trigger: 92_569,
};

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation(id INTEGER PRIMARY KEY);
         CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_type TEXT,
             content TEXT NOT NULL DEFAULT '', llm_model_id INTEGER, llm_model_name TEXT,
             token_count INTEGER, input_token_count INTEGER, output_token_count INTEGER,
             metadata_json TEXT, created_time TEXT, start_time TEXT, first_token_time TEXT,
             finish_time TEXT, ttft_ms INTEGER);
         CREATE TABLE butler_task_definition(id INTEGER PRIMARY KEY, butler_conversation_id INTEGER,
             task_conversation_id INTEGER);",
    )
    .unwrap();
    crate::db::butler_peer::schema(&conn).unwrap();
    schema(&conn).unwrap();
    conn
}

fn conversations(conn: &Connection, ids: &[i64]) {
    for id in ids {
        conn.execute("INSERT INTO conversation(id) VALUES (?1)", [id]).unwrap();
    }
}

/// 只带用量、不带时间的消息
fn msg(conn: &Connection, id: i64, conversation: i64, kind: &str, model: Option<(i64, &str)>, usage: (i64, i64, i64), metadata: Option<&str>) {
    conn.execute(
        "INSERT INTO message(id, conversation_id, message_type, llm_model_id, llm_model_name,
             token_count, input_token_count, output_token_count, metadata_json, created_time)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '2026-01-01T00:00:00.000Z')",
        rusqlite::params![id, conversation, kind, model.map(|m| m.0), model.map(|m| m.1),
            usage.0, usage.1, usage.2, metadata],
    )
    .unwrap();
}

fn exchange(conn: &Connection, id: i64, owner: i64, target: i64, input: Option<i64>, parent: Option<i64>) {
    conn.execute(
        "INSERT INTO butler_peer_exchange(id, root, owner, target, sender, recipient, request_key, body,
             state, input_id, round_no, max_rounds, parent)
         VALUES (?1, ?2, ?2, ?3, 'a', 'b', ?4, 'body', 'waiting', ?5, 1, 3, ?6)",
        rusqlite::params![id, owner, target, format!("k{id}"), input, parent],
    )
    .unwrap();
}

fn aux(conn: &Connection, conversation: i64, purpose: &'static str, model: (i64, &str), usage: (i64, i64, i64)) {
    record_auxiliary_usage(
        conn,
        &AuxiliaryUsageInput {
            conversation_id: conversation,
            purpose,
            model_id: Some(model.0),
            model_name: model.1.to_string(),
            total_tokens: usage.0,
            input_tokens: usage.1,
            output_tokens: usage.2,
            metadata_json: None,
        },
    )
    .unwrap();
}

fn stats(conn: &Connection, conversation: i64) -> ConversationTokenStats {
    token_stats(conn, StatsScope::Conversation(conversation), None).unwrap()
}

#[test]
fn test_token_stats_counts_tasks_and_delivered_peer_turns_only() {
    let conn = fixture();
    conversations(&conn, &[10, 20, 40, 50]);
    // 10 = 发起方；40 = 它委派的任务会话；50 = 别人的任务会话
    conn.execute_batch("INSERT INTO butler_task_definition VALUES (1,10,40),(2,99,50);").unwrap();
    msg(&conn, 1, 10, "user", None, (0, 0, 0), None);
    msg(&conn, 2, 10, "response", Some((1, "m1")), (100, 80, 20), None);
    msg(&conn, 3, 40, "user", None, (0, 0, 0), None);
    msg(&conn, 4, 40, "response", Some((2, "m2")), (1000, 900, 100), Some(r#"{"usage_source":"estimated"}"#));
    msg(&conn, 5, 40, "reasoning", Some((2, "m2")), (0, 0, 0), None);
    msg(&conn, 6, 50, "response", Some((2, "m2")), (7777, 7000, 777), None);
    // 20 是协作对象：8 之前与 11 之后是它自己的对话，不属于这次协作
    msg(&conn, 7, 20, "response", Some((1, "m1")), (3333, 3000, 333), None);
    msg(&conn, 8, 20, "user", None, (0, 0, 0), None);
    msg(&conn, 9, 20, "response", Some((1, "m1")), (50, 40, 10), None);
    msg(&conn, 11, 20, "user", None, (0, 0, 0), None);
    msg(&conn, 12, 20, "response", Some((1, "m1")), (4444, 4000, 444), None);
    exchange(&conn, 1, 10, 20, Some(8), None);
    // 排队中的请求还没有接收方回合
    exchange(&conn, 2, 10, 20, None, None);

    let result = stats(&conn, 10);
    assert_eq!(result.delegated.run_count, 2);
    assert_eq!(result.delegated.total_tokens, 1000 + 50);
    assert_eq!(result.delegated.estimated_message_count, 1);
    assert_eq!(result.total_tokens, 100 + 1000 + 50);
    assert_eq!(result.input_tokens, 80 + 900 + 40);
    assert_eq!(result.estimated_message_count, 1);
    // 消息数只统计本会话自身
    assert_eq!(result.message_count, 2);

    let m1 = result.by_model.iter().find(|m| m.model_id == Some(1)).unwrap();
    let m2 = result.by_model.iter().find(|m| m.model_id == Some(2)).unwrap();
    assert_eq!((m1.total_tokens, m1.request_count), (150, 2));
    // 不带用量的 reasoning 外壳不算一次调用
    assert_eq!((m2.total_tokens, m2.request_count), (1000, 1));
}

#[test]
fn test_token_stats_is_empty_without_delegation() {
    let conn = fixture();
    conversations(&conn, &[20]);
    let result = stats(&conn, 20);
    assert_eq!(result.delegated, DelegatedTokenUsage::default());
    assert!(result.by_model.is_empty());
    assert_eq!(result.total_tokens, 0);
}

#[test]
fn test_token_stats_follows_nested_delegation_without_double_counting() {
    let conn = fixture();
    conversations(&conn, &[10, 20, 21, 30, 40, 41]);
    // 10 → 任务 40 → 任务 41；10 请 20 协助，20 转请 30；任务 40 请 21 协助，又回头请 10
    conn.execute_batch("INSERT INTO butler_task_definition VALUES (1,10,40),(2,40,41);").unwrap();
    msg(&conn, 1, 10, "response", Some((1, "m1")), (1, 1, 0), None);
    msg(&conn, 2, 40, "response", Some((1, "m1")), (10, 8, 2), None);
    msg(&conn, 3, 41, "response", Some((1, "m1")), (100, 80, 20), None);
    msg(&conn, 4, 20, "user", None, (0, 0, 0), None);
    msg(&conn, 5, 20, "response", Some((1, "m1")), (1000, 800, 200), None);
    msg(&conn, 6, 30, "user", None, (0, 0, 0), None);
    msg(&conn, 7, 30, "response", Some((1, "m1")), (10_000, 8000, 2000), None);
    msg(&conn, 8, 21, "user", None, (0, 0, 0), None);
    msg(&conn, 9, 21, "response", Some((1, "m1")), (100_000, 80_000, 20_000), None);
    msg(&conn, 10, 10, "user", None, (0, 0, 0), None);
    msg(&conn, 11, 10, "response", Some((1, "m1")), (2, 2, 0), None);
    exchange(&conn, 1, 10, 20, Some(4), None);
    exchange(&conn, 2, 20, 30, Some(6), Some(1));
    exchange(&conn, 3, 40, 21, Some(8), None);
    // 任务回头请总管家：那段回合已在 10 自身里计入，不能再算一次委派
    exchange(&conn, 4, 40, 10, Some(10), None);

    let result = stats(&conn, 10);
    // 任务 40、任务 41、协作 1、下级协作 2、任务的协作 3
    assert_eq!(result.delegated.run_count, 5);
    assert_eq!(result.delegated.total_tokens, 10 + 100 + 1000 + 10_000 + 100_000);
    assert_eq!(result.total_tokens, 1 + 2 + 111_110);

    // 从协作对象 20 的视角：它自己的协作 2 计入，上级与无关工作不计入
    let peer = stats(&conn, 20);
    assert_eq!(peer.delegated.run_count, 1);
    assert_eq!(peer.total_tokens, 1000 + 10_000);
}

#[test]
fn test_token_stats_includes_auxiliary_usage_in_its_own_scope() {
    let conn = fixture();
    conversations(&conn, &[10, 20]);
    msg(&conn, 1, 10, "user", None, (0, 0, 0), None);
    msg(&conn, 2, 10, "response", Some((1, "m1")), (100, 90, 10), None);
    aux(&conn, 10, PURPOSE_CONTEXT_COMPACTION, (1, "m1"), (500, 450, 50));
    // 20 在协作前自己做过一次压缩，不属于这次协作
    msg(&conn, 3, 20, "response", Some((2, "m2")), (5, 5, 0), None);
    aux(&conn, 20, PURPOSE_CONTEXT_COMPACTION, (2, "m2"), (7000, 7000, 0));
    msg(&conn, 4, 20, "user", None, (0, 0, 0), None);
    // 协作回合内的压缩（锚点就是投递消息）
    aux(&conn, 20, PURPOSE_CONTEXT_COMPACTION, (2, "m2"), (300, 280, 20));
    msg(&conn, 5, 20, "response", Some((2, "m2")), (40, 30, 10), None);
    exchange(&conn, 1, 10, 20, Some(4), None);

    let result = stats(&conn, 10);
    assert_eq!(result.total_tokens, 100 + 500 + 300 + 40);
    assert_eq!(result.delegated.total_tokens, 300 + 40);
    assert_eq!(
        result.auxiliary,
        vec![AuxiliaryTokenUsage {
            purpose: PURPOSE_CONTEXT_COMPACTION.into(),
            call_count: 1,
            total_tokens: 500,
            input_tokens: 450,
            output_tokens: 50,
        }]
    );
    let m1 = result.by_model.iter().find(|m| m.model_id == Some(1)).unwrap();
    assert_eq!((m1.total_tokens, m1.request_count), (600, 2));
}

#[test]
fn test_token_stats_keeps_unknown_and_agent_models_in_breakdown() {
    let conn = fixture();
    conversations(&conn, &[10]);
    // 模型被删除后 llm_model_id 置空，名称仍在；Agent 通道统一用 0 占位
    msg(&conn, 1, 10, "response", None, (100, 90, 10), None);
    conn.execute("UPDATE message SET llm_model_name = 'old-model' WHERE id = 1", []).unwrap();
    msg(&conn, 2, 10, "response", Some((0, "claude-code")), (1000, 900, 100), None);
    msg(&conn, 3, 10, "response", Some((0, "codex-app-server")), (2000, 1800, 200), None);
    msg(&conn, 4, 10, "response", Some((5, "gpt")), (3000, 2900, 100), None);

    let result = stats(&conn, 10);
    let names: Vec<_> = result.by_model.iter().map(|m| m.model_name.as_str()).collect();
    assert_eq!(names, vec!["gpt", "Codex", "Claude Code", "old-model"]);
    assert_eq!(result.by_model.iter().map(|m| m.total_tokens).sum::<i64>(), result.total_tokens);
}

#[test]
fn test_token_stats_performance_only_uses_completed_model_requests() {
    let conn = fixture();
    conversations(&conn, &[10]);
    msg(&conn, 1, 10, "user", None, (0, 0, 0), None);
    let none = stats(&conn, 10);
    assert_eq!((none.avg_ttft_ms, none.avg_tps), (None, None));

    // 普通请求：1000 输出 / 2s 生成
    msg(&conn, 2, 10, "response", Some((1, "m1")), (1500, 500, 1000), Some(r#"{"generation_duration_ms":2000}"#));
    conn.execute("UPDATE message SET ttft_ms = 300 WHERE id = 2", []).unwrap();
    // 失败的半截回复：没有用量，首字时间接近 0，不应拉低平均
    msg(&conn, 3, 10, "response", Some((1, "m1")), (0, 0, 0), None);
    conn.execute("UPDATE message SET ttft_ms = 0 WHERE id = 3", []).unwrap();
    // Agent 回合跨越工具执行，参与首字延迟但不参与生成速度
    msg(&conn, 4, 10, "response", Some((0, "codex-app-server")), (900, 800, 100), None);
    conn.execute(
        "UPDATE message SET ttft_ms = 700, start_time = '2026-01-01T00:00:00.000Z',
             first_token_time = '2026-01-01T00:00:00.700Z', finish_time = '2026-01-01T00:01:40.000Z' WHERE id = 4",
        [],
    )
    .unwrap();

    let result = stats(&conn, 10);
    assert_eq!(result.ttft_sample_count, 2);
    assert_eq!(result.avg_ttft_ms, Some(500.0));
    assert_eq!(result.tps_sample_count, 1);
    assert_eq!(result.avg_tps, Some(500.0));
}

#[test]
fn test_token_stats_message_types_add_up_to_total() {
    let conn = fixture();
    conversations(&conn, &[10]);
    for (id, kind) in [(1, "system"), (2, "user"), (3, "reasoning"), (4, "response"), (5, "tool_result"),
        (6, "error"), (7, "assistant")] {
        msg(&conn, id, 10, kind, None, (0, 0, 0), None);
    }
    let result = stats(&conn, 10);
    assert_eq!(result.message_count, 7);
    assert_eq!(result.error_message_count, 1);
    assert_eq!(result.other_message_count, 1);
    let sum = result.system_message_count + result.user_message_count + result.response_message_count
        + result.reasoning_message_count + result.tool_result_message_count + result.error_message_count
        + result.other_message_count;
    assert_eq!(sum, result.message_count);
}

#[test]
fn test_token_stats_reports_context_usage_from_latest_model_request() {
    let conn = fixture();
    conversations(&conn, &[10]);
    msg(&conn, 1, 10, "response", Some((1, "m1")), (50_000, 49_000, 1000), None);
    conn.execute(
        "INSERT INTO message(id, conversation_id, message_type, content, token_count, input_token_count, output_token_count)
         VALUES (2, 10, 'system', '<!-- compacted_ids: {} -->\n<context_summary>s</context_summary>', 0, 0, 0)",
        [],
    )
    .unwrap();
    msg(&conn, 3, 10, "response", Some((1, "m1")), (61_000, 60_000, 1000), None);
    // 之后的 Agent 回合是多次请求累计，不代表上下文大小
    msg(&conn, 4, 10, "response", Some((0, "claude-code")), (900_000, 899_000, 1000), None);

    let result = token_stats(&conn, StatsScope::Conversation(10), Some(BUDGET)).unwrap();
    let context = result.context.unwrap();
    assert_eq!((context.latest_input_tokens, context.latest_message_id), (60_000, 3));
    assert_eq!(context.checkpoint_count, 1);
    assert_eq!(context.context_window_size, 128_000);

    // 只有 Agent 回合的会话不提供上下文占用
    conversations(&conn, &[11]);
    msg(&conn, 5, 11, "response", Some((0, "claude-code")), (10, 5, 5), None);
    assert!(token_stats(&conn, StatsScope::Conversation(11), Some(BUDGET)).unwrap().context.is_none());
}

#[test]
fn test_token_stats_group_scope_covers_members_and_dispatch() {
    let conn = fixture();
    conn.execute_batch(
        "CREATE TABLE butler_chat_group(id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE butler_chat_group_member(id INTEGER PRIMARY KEY, group_id INTEGER, conversation_id INTEGER, active INTEGER);",
    )
    .unwrap();
    conversations(&conn, &[100, 101, 102, 200]);
    conn.execute_batch(
        "INSERT INTO butler_chat_group VALUES (100, 'g');
         INSERT INTO butler_chat_group_member VALUES (1, 100, 101, 1), (2, 100, 102, 0);",
    )
    .unwrap();
    msg(&conn, 1, 101, "user", None, (0, 0, 0), None);
    msg(&conn, 2, 101, "response", Some((1, "m1")), (100, 90, 10), None);
    // 已移出的成员说过的话仍计入
    msg(&conn, 3, 102, "response", Some((2, "m2")), (200, 180, 20), None);
    msg(&conn, 4, 200, "response", Some((1, "m1")), (9999, 9999, 0), None);
    aux(&conn, 100, PURPOSE_GROUP_DISPATCH, (3, "helper"), (40, 38, 2));

    let result = token_stats(&conn, StatsScope::Group(100), Some(BUDGET)).unwrap();
    assert_eq!(result.total_tokens, 100 + 200 + 40);
    assert_eq!(result.message_count, 3);
    assert_eq!(result.auxiliary[0].purpose, PURPOSE_GROUP_DISPATCH);
    assert!(result.context.is_none());
    assert!(token_stats(&conn, StatsScope::Group(999), None).is_err());
}

#[test]
fn test_token_stats_totals_do_not_overflow_i32() {
    let conn = fixture();
    conversations(&conn, &[10]);
    msg(&conn, 1, 10, "response", Some((1, "m1")), (2_000_000_000, 1_999_000_000, 1_000_000), None);
    msg(&conn, 2, 10, "response", Some((1, "m1")), (2_000_000_000, 1_999_000_000, 1_000_000), None);
    let result = stats(&conn, 10);
    assert_eq!(result.total_tokens, 4_000_000_000);
    assert_eq!(result.by_model[0].total_tokens, result.total_tokens);
}

#[test]
fn test_token_stats_sums_opencode_go_allowance_across_delegation() {
    let conn = fixture();
    conversations(&conn, &[10, 40]);
    conn.execute_batch("INSERT INTO butler_task_definition VALUES (1,10,40);").unwrap();
    msg(&conn, 1, 10, "response", Some((1, "m1")), (10, 9, 1), Some(r#"{"opencode_go":{"allowance":0.25}}"#));
    msg(&conn, 2, 40, "response", Some((1, "m1")), (10, 9, 1), Some(r#"{"opencode_go":{"allowance":0.5}}"#));
    assert_eq!(stats(&conn, 10).opencode_go_allowance, Some(0.75));
}

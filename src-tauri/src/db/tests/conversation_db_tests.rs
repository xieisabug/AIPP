//! ConversationRepository 测试
//!
//! 对应源文件: `db/conversation_db.rs` 中的 `ConversationRepository`
//!
//! ## 测试隔离性
//! 所有测试使用 `Connection::open_in_memory()` 内存数据库，
//! 不会读写任何磁盘文件，确保与项目真实数据完全隔离。

use super::test_helpers::*;
use crate::db::conversation_db::*;
use chrono::Utc;
use rusqlite::Connection;

// ============================================================================
// ConversationRepository CRUD 测试
// ============================================================================

/// 测试对话的完整 CRUD 生命周期
///
/// 验证内容：
/// - Create: 创建对话后返回有效 ID
/// - Read: 能够根据 ID 读取完整对话信息
/// - Update: 修改对话名称后持久化成功
/// - Delete: 删除后无法再读取到该对话
#[test]
fn test_conversation_crud() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    // Test create
    let conversation = create_test_conversation(&repo);
    assert!(conversation.id > 0);
    assert_eq!(conversation.name, "Test Conversation");

    // Test read
    let read_conversation = repo.read(conversation.id).unwrap().unwrap();
    assert_eq!(read_conversation.id, conversation.id);
    assert_eq!(read_conversation.name, "Test Conversation");

    // Test update
    let mut updated_conversation = read_conversation.clone();
    updated_conversation.name = "Updated Conversation".to_string();
    repo.update(&updated_conversation).unwrap();

    let updated_read = repo.read(conversation.id).unwrap().unwrap();
    assert_eq!(updated_read.name, "Updated Conversation");

    // Test delete
    repo.delete(conversation.id).unwrap();
    let deleted_read = repo.read(conversation.id).unwrap();
    assert!(deleted_read.is_none());
}

/// 测试对话列表分页查询
///
/// 验证内容：
/// - 创建多个对话后，list 能正确返回所有对话
/// - 分页参数 (page, per_page) 正常工作
#[test]
fn test_conversation_list() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    // 创建多个对话
    for i in 1..=3 {
        let conversation = build_test_conversation(format!("Conversation {}", i), Some(i));
        repo.create(&conversation).unwrap();
    }

    // list 需要分页参数: page, per_page
    let conversations = repo.list(1, 10).unwrap();
    assert_eq!(conversations.len(), 3);
}

/// 测试不关联助手的对话
///
/// 验证内容：
/// - assistant_id 为 None 时能正确创建和读取
/// - 确保 nullable 字段处理正确
#[test]
fn test_conversation_with_none_assistant() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let conversation = build_test_conversation("No Assistant", None);
    let created = repo.create(&conversation).unwrap();

    let read = repo.read(created.id).unwrap().unwrap();
    assert!(read.assistant_id.is_none());
}

/// 测试对话名称的 Unicode 支持
///
/// 验证内容：
/// - 中文、emoji 等特殊字符能正确存储和读取
/// - UTF-8 编码处理正确
#[test]
fn test_conversation_unicode_name() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let conversation = build_test_conversation("中文对话名称 🎉", Some(1));
    let created = repo.create(&conversation).unwrap();

    let read = repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.name, "中文对话名称 🎉");
}

// ============================================================================
// ConversationRepository 特殊更新操作测试
// ============================================================================

/// 测试批量更新对话的 assistant_id
///
/// 验证内容：
/// - 当助手被删除时，所有关联该助手的对话需要更新 assistant_id
/// - update_assistant_id 能正确批量更新所有匹配的对话
#[test]
fn test_conversation_update_assistant_id() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    // 创建多个关联同一助手的对话
    for i in 1..=3 {
        let conversation = build_test_conversation(format!("Conversation {}", i), Some(100));
        repo.create(&conversation).unwrap();
    }

    // 批量更新 assistant_id: 100 -> None (模拟助手被删除)
    repo.update_assistant_id(100, None).unwrap();

    // 验证所有对话的 assistant_id 都已更新
    let conversations = repo.list(1, 10).unwrap();
    assert_eq!(conversations.len(), 3);
    for conv in conversations {
        assert!(conv.assistant_id.is_none());
    }
}

/// 测试单独更新对话名称
///
/// 验证内容：
/// - update_name 只更新名称，不影响其他字段
/// - 与 update 方法的区别：update_name 更轻量，只更新一个字段
#[test]
fn test_conversation_update_name() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let conversation = build_test_conversation("Original Name", Some(1));
    let created = repo.create(&conversation).unwrap();

    // 使用 update_name 只更新名称
    let mut updated = created.clone();
    updated.name = "New Name".to_string();
    repo.update_name(&updated).unwrap();

    let read = repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.name, "New Name");
    assert_eq!(read.assistant_id, Some(1)); // assistant_id 保持不变
}

// ============================================================================
// 异常情况和边界测试
// ============================================================================

/// 测试读取不存在的对话
///
/// 验证内容：
/// - 读取不存在的 ID 应返回 None，而不是错误
/// - 确保不会 panic
#[test]
fn test_conversation_read_nonexistent() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    // 读取不存在的 ID
    let result = repo.read(99999).unwrap();
    assert!(result.is_none());
}

/// 测试删除不存在的对话
///
/// 验证内容：
/// - 删除不存在的 ID 不应报错（SQLite DELETE 对不存在的行不报错）
/// - 操作应该是幂等的
#[test]
fn test_conversation_delete_nonexistent() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    // 删除不存在的 ID，不应报错
    let result = repo.delete(99999);
    assert!(result.is_ok());
}

/// 测试空名称的对话
///
/// 验证内容：
/// - 空字符串作为名称应该能正常存储
/// - 这是边界情况，UI 层应该阻止，但 DB 层应该能处理
#[test]
fn test_conversation_empty_name() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let conversation = build_test_conversation("", None);
    let created = repo.create(&conversation).unwrap();

    let read = repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.name, "");
}

/// 测试超长名称的对话
///
/// 验证内容：
/// - SQLite TEXT 类型没有长度限制
/// - 超长字符串应该能正常存储和读取
#[test]
fn test_conversation_very_long_name() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let long_name = "A".repeat(10000); // 10000 个字符
    let conversation = build_test_conversation(long_name.clone(), None);
    let created = repo.create(&conversation).unwrap();

    let read = repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.name.len(), 10000);
}

/// 测试分页边界情况
///
/// 验证内容：
/// - 空数据库查询应返回空列表
/// - 超出范围的页码应返回空列表
#[test]
fn test_conversation_list_empty_and_out_of_range() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    // 空数据库
    let empty_list = repo.list(1, 10).unwrap();
    assert!(empty_list.is_empty());

    // 创建一些数据
    for i in 1..=3 {
        let conversation = build_test_conversation(format!("Conversation {}", i), None);
        repo.create(&conversation).unwrap();
    }

    // 超出范围的页码
    let out_of_range = repo.list(100, 10).unwrap();
    assert!(out_of_range.is_empty());

    // 第一页应该有数据
    let first_page = repo.list(1, 2).unwrap();
    assert_eq!(first_page.len(), 2);

    // 第二页应该有剩余数据
    let second_page = repo.list(2, 2).unwrap();
    assert_eq!(second_page.len(), 1);
}

/// 测试特殊字符处理
///
/// 验证内容：
/// - SQL 注入尝试应被正确转义
/// - 特殊字符如引号、反斜杠应正确存储
#[test]
fn test_conversation_special_characters() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let special_names = vec![
        "Name with 'single quotes'",
        "Name with \"double quotes\"",
        "Name with \\ backslash",
        "Name with\nnewline",
        "Name with\ttab",
        "'; DROP TABLE conversation; --", // SQL 注入尝试
    ];

    for name in special_names {
        let conversation = build_test_conversation(name, None);
        let created = repo.create(&conversation).unwrap();
        let read = repo.read(created.id).unwrap().unwrap();
        assert_eq!(read.name, name);
    }
}

/// 测试隐藏的 butler 会话不会出现在普通会话列表中
#[test]
fn test_conversation_list_excludes_hidden_butler_conversations() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let visible = build_test_conversation("Visible Conversation", None);
    let visible = repo.create(&visible).unwrap();

    let mut hidden_task = build_test_conversation("Hidden Butler Task", None);
    hidden_task.conversation_kind = "butler_task".to_string();
    hidden_task.is_hidden_from_normal_chat_list = true;
    hidden_task.source_task_title = Some("Write release notes".to_string());
    repo.create(&hidden_task).unwrap();

    let conversations = repo.list(1, 10).unwrap();
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].id, visible.id);
    assert!(!conversations[0].is_hidden_from_normal_chat_list);
}

/// 测试按 butler 父会话查询时会返回隐藏的任务会话
#[test]
fn test_list_by_parent_butler_conversation_id_includes_hidden_tasks() {
    let conn = create_test_db();
    let repo = ConversationRepository::new(conn);

    let mut butler_main = build_test_conversation("Butler Main", None);
    butler_main.conversation_kind = "butler_main".to_string();
    butler_main.is_hidden_from_normal_chat_list = true;
    let butler_main = repo.create(&butler_main).unwrap();

    let mut task_a = build_test_conversation("Task A", None);
    task_a.conversation_kind = "butler_task".to_string();
    task_a.parent_butler_conversation_id = Some(butler_main.id);
    task_a.is_hidden_from_normal_chat_list = true;
    let task_a = repo.create(&task_a).unwrap();

    let mut task_b = build_test_conversation("Task B", None);
    task_b.conversation_kind = "butler_task".to_string();
    task_b.parent_butler_conversation_id = Some(butler_main.id);
    task_b.is_hidden_from_normal_chat_list = true;
    let task_b = repo.create(&task_b).unwrap();

    let mut other_main = build_test_conversation("Other Butler Main", None);
    other_main.conversation_kind = "butler_main".to_string();
    other_main.is_hidden_from_normal_chat_list = true;
    let other_main = repo.create(&other_main).unwrap();

    let mut other_task = build_test_conversation("Other Task", None);
    other_task.conversation_kind = "butler_task".to_string();
    other_task.parent_butler_conversation_id = Some(other_main.id);
    other_task.is_hidden_from_normal_chat_list = true;
    repo.create(&other_task).unwrap();

    let mut task_ids = repo
        .list_by_parent_butler_conversation_id(butler_main.id)
        .unwrap()
        .into_iter()
        .map(|conversation| {
            assert!(conversation.is_hidden_from_normal_chat_list);
            assert_eq!(conversation.parent_butler_conversation_id, Some(butler_main.id));
            conversation.id
        })
        .collect::<Vec<_>>();
    task_ids.sort_unstable();

    let mut expected = vec![task_a.id, task_b.id];
    expected.sort_unstable();
    assert_eq!(task_ids, expected);
}

#[test]
fn test_list_reconcilable_butler_task_conversation_ids_filters_terminal_tasks() {
    let conn = create_test_db();

    let mut running_task = build_test_conversation("Running Task", None);
    running_task.conversation_kind = "butler_task".to_string();
    running_task.butler_task_status = Some("running".to_string());

    let mut cancelled_without_result = build_test_conversation("Cancelled Task", None);
    cancelled_without_result.conversation_kind = "butler_task".to_string();
    cancelled_without_result.butler_task_status = Some("cancelled".to_string());
    cancelled_without_result.butler_task_finalized_at = Some(Utc::now());

    let mut finished_task = build_test_conversation("Finished Task", None);
    finished_task.conversation_kind = "butler_task".to_string();
    finished_task.butler_task_status = Some("succeeded".to_string());
    finished_task.butler_task_finalized_at = Some(Utc::now());

    let running_task = conn
        .execute(
            "INSERT INTO conversation (
                name, assistant_id, created_time, updated_time, conversation_kind,
                parent_butler_conversation_id, source_task_title, is_hidden_from_normal_chat_list,
                channel_source, butler_task_status, butler_task_summary, butler_task_finalized_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            (
                &running_task.name,
                &running_task.assistant_id,
                &running_task.created_time.to_rfc3339(),
                &running_task.updated_time.to_rfc3339(),
                &running_task.conversation_kind,
                &running_task.parent_butler_conversation_id,
                &running_task.source_task_title,
                &(running_task.is_hidden_from_normal_chat_list as i64),
                &running_task.channel_source,
                &running_task.butler_task_status,
                &running_task.butler_task_summary,
                &running_task.butler_task_finalized_at.map(|value| value.to_rfc3339()),
            ),
        )
        .unwrap();
    let running_task_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO conversation (
            name, assistant_id, created_time, updated_time, conversation_kind,
            parent_butler_conversation_id, source_task_title, is_hidden_from_normal_chat_list,
            channel_source, butler_task_status, butler_task_summary, butler_task_finalized_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        (
            &cancelled_without_result.name,
            &cancelled_without_result.assistant_id,
            &cancelled_without_result.created_time.to_rfc3339(),
            &cancelled_without_result.updated_time.to_rfc3339(),
            &cancelled_without_result.conversation_kind,
            &cancelled_without_result.parent_butler_conversation_id,
            &cancelled_without_result.source_task_title,
            &(cancelled_without_result.is_hidden_from_normal_chat_list as i64),
            &cancelled_without_result.channel_source,
            &cancelled_without_result.butler_task_status,
            &cancelled_without_result.butler_task_summary,
            &cancelled_without_result.butler_task_finalized_at.map(|value| value.to_rfc3339()),
        ),
    )
    .unwrap();
    let cancelled_task_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO conversation (
            name, assistant_id, created_time, updated_time, conversation_kind,
            parent_butler_conversation_id, source_task_title, is_hidden_from_normal_chat_list,
            channel_source, butler_task_status, butler_task_summary, butler_task_finalized_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        (
            &finished_task.name,
            &finished_task.assistant_id,
            &finished_task.created_time.to_rfc3339(),
            &finished_task.updated_time.to_rfc3339(),
            &finished_task.conversation_kind,
            &finished_task.parent_butler_conversation_id,
            &finished_task.source_task_title,
            &(finished_task.is_hidden_from_normal_chat_list as i64),
            &finished_task.channel_source,
            &finished_task.butler_task_status,
            &finished_task.butler_task_summary,
            &finished_task.butler_task_finalized_at.map(|value| value.to_rfc3339()),
        ),
    )
    .unwrap();
    let finished_task_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO butler_task_result (
            task_conversation_id, handoff_mode, payload_json, summary, structured_output_json,
            evidence_json, artifact_refs_json, followup_suggestions_json, final_message_id,
            created_time, updated_time
         ) VALUES (?1, NULL, NULL, ?2, NULL, NULL, NULL, NULL, NULL, ?3, ?3)",
        (finished_task_id, "done", Utc::now().to_rfc3339()),
    )
    .unwrap();

    let repo = ConversationRepository::new(conn);

    let mut ids = repo.list_reconcilable_butler_task_conversation_ids().unwrap();
    ids.sort_unstable();

    assert_eq!(ids, vec![cancelled_task_id, running_task_id]);
}

#[test]
fn test_list_butler_task_conversation_ids_pending_followup_only_returns_pending_rows() {
    let conn = create_test_db();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO butler_task_result (
            task_conversation_id, handoff_mode, payload_json, summary, structured_output_json,
            evidence_json, artifact_refs_json, followup_suggestions_json, followup_status,
            handoff_message_id, final_message_id, created_time, updated_time
         ) VALUES
            (101, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'pending', NULL, NULL, ?1, ?1),
            (102, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'handoff_injected', 9001, NULL, ?1, ?1),
            (103, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'enqueued', 9002, NULL, ?1, ?1),
            (104, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'dispatching', 9003, NULL, ?1, ?1)",
        [&now],
    )
    .unwrap();

    let repo = ConversationRepository::new(conn);
    let ids = repo.list_butler_task_conversation_ids_pending_followup().unwrap();

    assert_eq!(ids, vec![101, 102]);
}

#[test]
fn test_try_mark_task_result_followup_dispatching_only_claims_once() {
    let conn = create_test_db();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO butler_task_result (
            task_conversation_id, handoff_mode, payload_json, summary, structured_output_json,
            evidence_json, artifact_refs_json, followup_suggestions_json, followup_status,
            handoff_message_id, final_message_id, created_time, updated_time
         ) VALUES (?1, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'pending', NULL, NULL, ?2, ?2)",
        (201, &now),
    )
    .unwrap();

    let repo = ButlerRepository::new(conn);
    assert!(repo.try_mark_task_result_followup_dispatching(201, Some(9101)).unwrap());
    assert!(!repo.try_mark_task_result_followup_dispatching(201, Some(9102)).unwrap());

    let result = repo.get_task_result(201).unwrap().unwrap();
    assert_eq!(result.followup_status.as_deref(), Some("dispatching"));
    assert_eq!(result.handoff_message_id, Some(9101));
}

/// 测试旧版 conversation 表升级时不会使用 SQLite 不支持的非静态默认值
#[test]
fn test_ensure_conversation_table_migrates_legacy_schema_without_non_constant_default() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE conversation (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            assistant_id INTEGER,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation (name, assistant_id, created_time) VALUES (?1, ?2, ?3)",
        ("Legacy Conversation", Option::<i64>::None, "2026-03-13T13:37:48Z"),
    )
    .unwrap();

    ensure_conversation_table(&conn).unwrap();

    let updated_time: String = conn
        .query_row(
            "SELECT updated_time FROM conversation WHERE name = ?1",
            ["Legacy Conversation"],
            |row| row.get(0),
        )
        .unwrap();
    let conversation_kind: String = conn
        .query_row(
            "SELECT conversation_kind FROM conversation WHERE name = ?1",
            ["Legacy Conversation"],
            |row| row.get(0),
        )
        .unwrap();
    let is_hidden: i64 = conn
        .query_row(
            "SELECT is_hidden_from_normal_chat_list FROM conversation WHERE name = ?1",
            ["Legacy Conversation"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(updated_time, "2026-03-13T13:37:48Z");
    assert_eq!(conversation_kind, "normal");
    assert_eq!(is_hidden, 0);
}

#[test]
fn test_ensure_conversation_table_rewrites_butler_main_archive_kind() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        "CREATE TABLE conversation (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            assistant_id INTEGER,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            conversation_kind TEXT NOT NULL DEFAULT 'normal'
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation (name, assistant_id, created_time, updated_time, conversation_kind)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            "Archived Butler Main",
            Option::<i64>::None,
            "2026-04-11T00:00:00Z",
            "2026-04-11T00:10:00Z",
            "butler_main_archive",
        ),
    )
    .unwrap();

    ensure_conversation_table(&conn).unwrap();

    let conversation_kind: String = conn
        .query_row(
            "SELECT conversation_kind FROM conversation WHERE name = ?1",
            ["Archived Butler Main"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(conversation_kind, "butler_main");
}

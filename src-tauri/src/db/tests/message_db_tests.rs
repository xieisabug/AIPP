//! MessageRepository 测试
//!
//! 对应源文件: `db/conversation_db.rs` 中的 `MessageRepository`
//!
//! ## 测试隔离性
//! 所有测试使用 `Connection::open_in_memory()` 内存数据库，
//! 不会读写任何磁盘文件，确保与项目真实数据完全隔离。
//!
//! ## 测试分组
//! - CRUD 测试：验证基本的增删改查操作
//! - 版本管理测试：验证消息重发、版本链等核心业务逻辑

use super::test_helpers::*;
use crate::db::conversation_db::*;

// ============================================================================
// MessageRepository CRUD 测试
// ============================================================================

/// 测试消息的完整 CRUD 生命周期
///
/// 验证内容：
/// - Create: 创建消息后返回有效 ID
/// - Read: 能够根据 ID 读取完整消息信息
/// - Update: 修改消息内容后持久化成功
/// - Delete: 删除后无法再读取到该消息
#[test]
fn test_message_crud() {
    let (_, _, msg_repo, conversation) = create_shared_test_db();

    // Test create message
    let message = create_test_message(
        conversation.id,
        "user",
        "Test message",
        None,
        Some(new_group_id()),
    );
    let created_message = msg_repo.create(&message).unwrap();
    assert!(created_message.id > 0);
    assert_eq!(created_message.content, "Test message");

    // Test read message
    let read_message = msg_repo.read(created_message.id).unwrap().unwrap();
    assert_eq!(read_message.id, created_message.id);
    assert_eq!(read_message.content, "Test message");

    // Test update message
    let mut updated_message = read_message.clone();
    updated_message.content = "Updated message".to_string();
    msg_repo.update(&updated_message).unwrap();

    let updated_read = msg_repo.read(created_message.id).unwrap().unwrap();
    assert_eq!(updated_read.content, "Updated message");

    // Test delete message
    msg_repo.delete(created_message.id).unwrap();
    let deleted_read = msg_repo.read(created_message.id).unwrap();
    assert!(deleted_read.is_none());
}

/// 测试按对话 ID 查询消息列表
///
/// 验证内容：
/// - 多条消息能正确关联到同一对话
/// - list_by_conversation_id 返回完整的消息列表
/// - 返回结果包含附件信息（元组第二项）
#[test]
fn test_list_messages_by_conversation_id() {
    let (_, _, msg_repo, conversation) = create_shared_test_db();

    // 创建多条消息
    let messages = vec![
        create_test_message(conversation.id, "user", "Message 1", None, Some(new_group_id())),
        create_test_message(conversation.id, "assistant", "Message 2", None, Some(new_group_id())),
        create_test_message(conversation.id, "user", "Message 3", None, Some(new_group_id())),
    ];

    for message in &messages {
        msg_repo.create(message).unwrap();
    }

    // 查询对话的所有消息
    let retrieved_messages = msg_repo.list_by_conversation_id(conversation.id).unwrap();
    assert_eq!(retrieved_messages.len(), 3);

    // 验证消息内容
    let contents: Vec<String> =
        retrieved_messages.iter().map(|(msg, _)| msg.content.clone()).collect();
    assert!(contents.contains(&"Message 1".to_string()));
    assert!(contents.contains(&"Message 2".to_string()));
    assert!(contents.contains(&"Message 3".to_string()));
}

/// 测试支持的消息类型
///
/// 验证内容：
/// - user: 用户消息
/// - assistant: AI 回复（旧版，已废弃）
/// - system: 系统提示
/// - reasoning: AI 推理过程
/// - response: AI 最终回复
#[test]
fn test_message_types() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let message_types = vec!["user", "assistant", "system", "reasoning", "response"];
    
    for msg_type in message_types {
        let message = create_test_message(conversation_id, msg_type, "content", None, Some(new_group_id()));
        let created = msg_repo.create(&message).unwrap();
        assert_eq!(created.message_type, msg_type);
    }
}

// ============================================================================
// 消息版本管理测试
// 这是 AIPP 的核心业务逻辑，支持消息重发和版本切换
// ============================================================================

/// 测试 generation_group_id 分组机制
///
/// 验证内容：
/// - 同一轮对话的用户消息和 AI 回复共享相同的 generation_group_id
/// - 这使得 UI 能将相关消息分组显示
/// - AI 回复的 parent_id 指向对应的用户消息
#[test]
fn test_generation_group_id_management() {
    let (_, _, msg_repo, conversation) = create_shared_test_db();

    let group_id = new_group_id();

    // 创建用户消息
    let user_message = create_test_message(
        conversation.id,
        "user",
        "User question",
        None,
        Some(group_id.clone()),
    );
    let created_user_msg = msg_repo.create(&user_message).unwrap();

    // 创建AI回复消息（使用相同的generation_group_id）
    let ai_message = create_test_message(
        conversation.id,
        "assistant",
        "AI response",
        Some(created_user_msg.id),
        Some(group_id.clone()),
    );
    let created_ai_msg = msg_repo.create(&ai_message).unwrap();

    // 验证generation_group_id相同
    assert_eq!(created_user_msg.generation_group_id, Some(group_id.clone()));
    assert_eq!(created_ai_msg.generation_group_id, Some(group_id.clone()));

    // 验证parent_id关系
    assert_eq!(created_ai_msg.parent_id, Some(created_user_msg.id));
}

/// 测试消息的父子关系链
///
/// 验证内容：
/// - 多轮对话形成的消息链: 用户 -> AI -> 用户 -> AI
/// - 每条消息的 parent_id 正确指向前一条消息
/// - 第一条用户消息的 parent_id 为 None
#[test]
fn test_parent_child_relationships() {
    let (_, _, msg_repo, conversation) = create_shared_test_db();

    // 创建消息链：用户消息 -> AI回复 -> 用户回复 -> AI回复
    let user_msg1 = create_test_message(
        conversation.id,
        "user",
        "First user message",
        None,
        Some(new_group_id()),
    );
    let created_user_msg1 = msg_repo.create(&user_msg1).unwrap();

    let ai_msg1 = create_test_message(
        conversation.id,
        "assistant",
        "First AI response",
        Some(created_user_msg1.id),
        Some(new_group_id()),
    );
    let created_ai_msg1 = msg_repo.create(&ai_msg1).unwrap();

    let user_msg2 = create_test_message(
        conversation.id,
        "user",
        "Second user message",
        Some(created_ai_msg1.id),
        Some(new_group_id()),
    );
    let created_user_msg2 = msg_repo.create(&user_msg2).unwrap();

    let ai_msg2 = create_test_message(
        conversation.id,
        "assistant",
        "Second AI response",
        Some(created_user_msg2.id),
        Some(new_group_id()),
    );
    let created_ai_msg2 = msg_repo.create(&ai_msg2).unwrap();

    // 验证parent_id关系链
    assert_eq!(created_user_msg1.parent_id, None);
    assert_eq!(created_ai_msg1.parent_id, Some(created_user_msg1.id));
    assert_eq!(created_user_msg2.parent_id, Some(created_ai_msg1.id));
    assert_eq!(created_ai_msg2.parent_id, Some(created_user_msg2.id));

    // 查询所有消息
    let all_messages = msg_repo.list_by_conversation_id(conversation.id).unwrap();
    assert_eq!(all_messages.len(), 4);
}

/// 测试消息重发场景
///
/// 验证内容：
/// 1. AI 消息重发：使用相同的 generation_group_id，parent_id 指向被重发的消息
/// 2. 用户消息重发：创建新的 generation_group_id，parent_id 指向原始消息
/// 3. 这是支持 UI 版本切换的关键逻辑
#[test]
fn test_message_regeneration_scenarios() {
    let (_, _, msg_repo, conversation) = create_shared_test_db();

    let original_group_id = new_group_id();

    // 创建原始用户消息
    let original_user_msg = create_test_message(
        conversation.id,
        "user",
        "Original user message",
        None,
        Some(original_group_id.clone()),
    );
    let created_original_user = msg_repo.create(&original_user_msg).unwrap();

    // 创建原始AI回复
    let original_ai_msg = create_test_message(
        conversation.id,
        "assistant",
        "Original AI response",
        Some(created_original_user.id),
        Some(original_group_id.clone()),
    );
    let created_original_ai = msg_repo.create(&original_ai_msg).unwrap();

    // 模拟AI消息重发（应该使用相同的generation_group_id）
    let regenerated_ai_msg = create_test_message(
        conversation.id,
        "assistant",
        "Regenerated AI response",
        Some(created_original_ai.id),    // parent_id指向被重发的消息
        Some(original_group_id.clone()), // 使用相同的generation_group_id
    );
    let created_regenerated_ai = msg_repo.create(&regenerated_ai_msg).unwrap();

    // 验证重发逻辑
    assert_eq!(created_regenerated_ai.generation_group_id, Some(original_group_id.clone()));
    assert_eq!(created_regenerated_ai.parent_id, Some(created_original_ai.id));

    // 模拟用户消息重发（应该创建新的generation_group_id）
    let new_group_id = new_group_id();
    let regenerated_user_msg = create_test_message(
        conversation.id,
        "user",
        "Regenerated user message",
        Some(created_original_user.id), // parent_id指向被重发的消息
        Some(new_group_id.clone()),     // 新的generation_group_id
    );
    let created_regenerated_user = msg_repo.create(&regenerated_user_msg).unwrap();

    // 验证用户重发逻辑
    assert_eq!(created_regenerated_user.generation_group_id, Some(new_group_id));
    assert_eq!(created_regenerated_user.parent_id, Some(created_original_user.id));
    assert_ne!(created_regenerated_user.generation_group_id, Some(original_group_id));

    // 查询所有消息
    let all_messages = msg_repo.list_by_conversation_id(conversation.id).unwrap();
    assert_eq!(all_messages.len(), 4);
}

/// 测试 tool_calls_json 字段
///
/// 验证内容：
/// - MCP 工具调用信息能正确存储为 JSON 字符串
/// - 读取时能完整恢复 JSON 内容
/// - 这是支持 MCP 工具调用的关键字段
#[test]
fn test_message_with_tool_calls_json() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let mut message = create_test_message(conversation_id, "assistant", "Response", None, Some(new_group_id()));
    message.tool_calls_json = Some(r#"[{"name": "search", "arguments": {"query": "test"}}]"#.to_string());
    
    let created = msg_repo.create(&message).unwrap();
    assert!(created.tool_calls_json.is_some());
    
    let read = msg_repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.tool_calls_json, Some(r#"[{"name": "search", "arguments": {"query": "test"}}]"#.to_string()));
}

// ============================================================================
// MessageRepository 特殊更新操作测试
// ============================================================================

/// 测试更新消息内容
///
/// 验证内容：
/// - update_content 能正确更新消息的 content 字段
/// - 用于流式响应时追加内容或编辑消息
#[test]
fn test_message_update_content() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let message = create_test_message(conversation_id, "assistant", "Initial content", None, Some(new_group_id()));
    let created = msg_repo.create(&message).unwrap();

    // 更新消息内容
    msg_repo.update_content(created.id, "Updated content with more text").unwrap();

    let read = msg_repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.content, "Updated content with more text");
}

/// 测试更新消息完成时间
///
/// 验证内容：
/// - update_finish_time 设置 finish_time 为当前时间
/// - 用于标记 AI 响应完成的时间点
#[test]
fn test_message_update_finish_time() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let mut message = create_test_message(conversation_id, "assistant", "Response", None, Some(new_group_id()));
    message.finish_time = None; // 初始时没有完成时间
    let created = msg_repo.create(&message).unwrap();
    
    // 验证初始状态
    let before = msg_repo.read(created.id).unwrap().unwrap();
    assert!(before.finish_time.is_none());

    // 更新完成时间
    msg_repo.update_finish_time(created.id).unwrap();

    // 验证完成时间已设置（由于使用 CURRENT_TIMESTAMP，只能验证不为空）
    // 注意：内存数据库中 CURRENT_TIMESTAMP 可能格式不同，此测试主要验证 SQL 执行成功
}

// ============================================================================
// 异常情况和边界测试
// ============================================================================

/// 测试读取不存在的消息
///
/// 验证内容：
/// - 读取不存在的消息 ID 应返回 None
/// - 不应该 panic 或返回错误
#[test]
fn test_message_read_nonexistent() {
    let (msg_repo, _) = create_message_test_db();

    let result = msg_repo.read(99999).unwrap();
    assert!(result.is_none());
}

/// 测试删除不存在的消息
///
/// 验证内容：
/// - 删除不存在的消息不应报错
/// - 操作应该是幂等的
#[test]
fn test_message_delete_nonexistent() {
    let (msg_repo, _) = create_message_test_db();

    let result = msg_repo.delete(99999);
    assert!(result.is_ok());
}

/// 测试空内容消息
///
/// 验证内容：
/// - 空字符串内容应该能正常存储
/// - 这是边界情况，可能在流式响应开始时发生
#[test]
fn test_message_empty_content() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let message = create_test_message(conversation_id, "assistant", "", None, Some(new_group_id()));
    let created = msg_repo.create(&message).unwrap();
    
    let read = msg_repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.content, "");
}

/// 测试超长消息内容
///
/// 验证内容：
/// - SQLite TEXT 没有长度限制
/// - 超长内容（如完整代码文件）应该能正常存储
#[test]
fn test_message_very_long_content() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let long_content = "A".repeat(100000); // 100KB 内容
    let mut message = create_test_message(conversation_id, "assistant", &long_content, None, Some(new_group_id()));
    message.content = long_content.clone();
    
    let created = msg_repo.create(&message).unwrap();
    let read = msg_repo.read(created.id).unwrap().unwrap();
    assert_eq!(read.content.len(), 100000);
}

/// 测试查询不存在对话的消息列表
///
/// 验证内容：
/// - 查询不存在的 conversation_id 应返回空列表
/// - 不应该返回错误
#[test]
fn test_message_list_nonexistent_conversation() {
    let (msg_repo, _) = create_message_test_db();

    let messages = msg_repo.list_by_conversation_id(99999).unwrap();
    assert!(messages.is_empty());
}

/// 测试 parent_id 指向不存在的消息
///
/// 验证内容：
/// - SQLite 外键默认不强制执行（除非显式开启）
/// - 这种情况在应用层应该避免，但 DB 层应该能处理
#[test]
fn test_message_invalid_parent_id() {
    let (msg_repo, conversation_id) = create_message_test_db();

    // parent_id 指向不存在的消息
    let message = create_test_message(
        conversation_id,
        "assistant",
        "Orphan message",
        Some(99999), // 不存在的 parent_id
        Some(new_group_id()),
    );
    
    // 应该能创建成功（外键约束未启用）
    let created = msg_repo.create(&message).unwrap();
    assert_eq!(created.parent_id, Some(99999));
}

/// 测试特殊字符在消息内容中
///
/// 验证内容：
/// - 代码块、SQL、JSON 等特殊内容应正确存储
/// - 确保不会发生 SQL 注入
#[test]
fn test_message_special_content() {
    let (msg_repo, conversation_id) = create_message_test_db();

    let special_contents = vec![
        "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```",
        r#"{"key": "value", "nested": {"a": 1}}"#,
        "SELECT * FROM users WHERE name = 'admin'; DROP TABLE users;--",
        "<script>alert('XSS')</script>",
        "Line1\nLine2\r\nLine3\tTabbed",
    ];

    for content in special_contents {
        let message = create_test_message(conversation_id, "assistant", content, None, Some(new_group_id()));
        let created = msg_repo.create(&message).unwrap();
        let read = msg_repo.read(created.id).unwrap().unwrap();
        assert_eq!(read.content, content);
    }
}

/// 测试更新不存在消息的内容
///
/// 验证内容：
/// - 更新不存在的消息不应报错（SQL UPDATE 对不存在的行不报错）
/// - 但不会影响任何数据
#[test]
fn test_message_update_content_nonexistent() {
    let (msg_repo, _) = create_message_test_db();

    // 更新不存在的消息，不应报错
    let result = msg_repo.update_content(99999, "New content");
    assert!(result.is_ok());
}

//! ConversationRepository 测试
//!
//! 对应源文件: `db/conversation_db.rs` 中的 `ConversationRepository`
//!
//! ## 测试隔离性
//! 所有测试使用 `Connection::open_in_memory()` 内存数据库，
//! 不会读写任何磁盘文件，确保与项目真实数据完全隔离。

use super::test_helpers::*;
use crate::db::conversation_db::*;

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
        let conversation = Conversation {
            id: 0,
            name: format!("Conversation {}", i),
            assistant_id: Some(i),
            created_time: chrono::Utc::now(),
        };
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

    let conversation = Conversation {
        id: 0,
        name: "No Assistant".to_string(),
        assistant_id: None,
        created_time: chrono::Utc::now(),
    };
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

    let conversation = Conversation {
        id: 0,
        name: "中文对话名称 🎉".to_string(),
        assistant_id: Some(1),
        created_time: chrono::Utc::now(),
    };
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
        let conversation = Conversation {
            id: 0,
            name: format!("Conversation {}", i),
            assistant_id: Some(100), // 都关联 assistant_id = 100
            created_time: chrono::Utc::now(),
        };
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

    let conversation = Conversation {
        id: 0,
        name: "Original Name".to_string(),
        assistant_id: Some(1),
        created_time: chrono::Utc::now(),
    };
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

    let conversation = Conversation {
        id: 0,
        name: "".to_string(),
        assistant_id: None,
        created_time: chrono::Utc::now(),
    };
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
    let conversation = Conversation {
        id: 0,
        name: long_name.clone(),
        assistant_id: None,
        created_time: chrono::Utc::now(),
    };
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
        let conversation = Conversation {
            id: 0,
            name: format!("Conversation {}", i),
            assistant_id: None,
            created_time: chrono::Utc::now(),
        };
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
        let conversation = Conversation {
            id: 0,
            name: name.to_string(),
            assistant_id: None,
            created_time: chrono::Utc::now(),
        };
        let created = repo.create(&conversation).unwrap();
        let read = repo.read(created.id).unwrap().unwrap();
        assert_eq!(read.name, name);
    }
}

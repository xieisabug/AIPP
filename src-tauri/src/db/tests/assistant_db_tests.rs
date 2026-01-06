//! Assistant 数据库操作测试
//!
//! ## 测试范围
//! - Assistant CRUD 操作
//! - AssistantModel 关联操作
//! - AssistantPrompt 关联操作
//! - AssistantModelConfig 配置操作
//!
//! ## 测试隔离
//! 所有测试使用 `Connection::open_in_memory()` 创建内存数据库

use crate::db::assistant_db::*;
use rusqlite::Connection;

// ============================================================================
// 测试辅助函数
// ============================================================================

/// 创建测试用内存数据库并初始化 Assistant 相关表结构
///
/// **安全性**: 使用内存数据库，不会影响真实数据
fn create_assistant_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // 创建 assistant 表
    conn.execute(
        "CREATE TABLE assistant (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            description TEXT,
            assistant_type INTEGER,
            is_addition BOOLEAN NOT NULL DEFAULT 0,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();

    // 创建 assistant_model 表
    conn.execute(
        "CREATE TABLE assistant_model (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            assistant_id INTEGER NOT NULL,
            provider_id INTEGER NOT NULL,
            model_code TEXT NOT NULL,
            alias TEXT,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (assistant_id) REFERENCES assistant(id) ON DELETE CASCADE
        )",
        [],
    )
    .unwrap();

    // 创建 assistant_prompt 表
    conn.execute(
        "CREATE TABLE assistant_prompt (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            assistant_id INTEGER NOT NULL,
            prompt TEXT NOT NULL,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (assistant_id) REFERENCES assistant(id) ON DELETE CASCADE
        )",
        [],
    )
    .unwrap();

    // 创建 assistant_model_config 表
    conn.execute(
        "CREATE TABLE assistant_model_config (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            assistant_id INTEGER NOT NULL,
            assistant_model_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            value TEXT,
            value_type TEXT,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (assistant_id) REFERENCES assistant(id) ON DELETE CASCADE,
            UNIQUE(assistant_id, assistant_model_id, name)
        )",
        [],
    )
    .unwrap();

    conn
}

/// 创建 AssistantDatabase 实例用于测试
/// 注意：需要两个 Connection，一个用于 assistant，一个用于 mcp
fn create_assistant_db() -> AssistantDatabase {
    let conn = create_assistant_test_db();
    let mcp_conn = Connection::open_in_memory().unwrap();
    AssistantDatabase { conn, mcp_conn }
}

// ============================================================================
// 正常情况测试
// ============================================================================

/// 测试 Assistant 的完整 CRUD 生命周期
///
/// 验证内容：
/// - Create: 创建 Assistant 后返回有效 ID
/// - Read: 能够读取刚创建的 Assistant
/// - Update: 修改名称和描述后持久化成功
/// - Delete: 删除后无法再读取
#[test]
fn test_assistant_crud() {
    let db = create_assistant_db();

    // Create
    let id = db.add_assistant("Test Assistant", "Test Description", Some(1), false).unwrap();
    assert!(id > 0);

    // Read
    let assistant = db.get_assistant(id).unwrap();
    assert_eq!(assistant.id, id);
    assert_eq!(assistant.name, "Test Assistant");
    assert_eq!(assistant.description, Some("Test Description".to_string()));
    assert_eq!(assistant.assistant_type, Some(1));
    assert!(!assistant.is_addition);

    // Update
    db.update_assistant(id, "Updated Name", "Updated Description").unwrap();
    let updated = db.get_assistant(id).unwrap();
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.description, Some("Updated Description".to_string()));

    // Delete
    db.delete_assistant(id).unwrap();
    let result = db.get_assistant(id);
    assert!(result.is_err());
}

/// 测试获取所有 Assistant 列表
///
/// 验证内容：
/// - 空数据库返回空列表
/// - 添加多个 Assistant 后能正确返回
#[test]
fn test_assistant_get_all() {
    let db = create_assistant_db();

    // 空列表
    let list = db.get_assistants().unwrap();
    assert!(list.is_empty());

    // 添加多个
    db.add_assistant("Assistant 1", "Desc 1", Some(1), false).unwrap();
    db.add_assistant("Assistant 2", "Desc 2", Some(2), true).unwrap();

    let list = db.get_assistants().unwrap();
    assert_eq!(list.len(), 2);
}

/// 测试 Assistant Model 关联操作
///
/// 验证内容：
/// - 为 Assistant 添加关联 Model
/// - 获取 Assistant 的所有 Model
/// - 更新 Model 配置
#[test]
fn test_assistant_model_operations() {
    let db = create_assistant_db();

    // 创建 Assistant
    let assistant_id = db.add_assistant("Test", "Desc", None, false).unwrap();

    // 添加 Model
    let model_id = db.add_assistant_model(assistant_id, 1, "gpt-4", "GPT-4").unwrap();
    assert!(model_id > 0);

    // 获取 Models
    let models = db.get_assistant_model(assistant_id).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model_code, "gpt-4");
    assert_eq!(models[0].alias, "GPT-4");

    // 更新 Model
    db.update_assistant_model(model_id, 2, "claude-3", "Claude").unwrap();
    let updated_models = db.get_assistant_model(assistant_id).unwrap();
    assert_eq!(updated_models[0].model_code, "claude-3");
    assert_eq!(updated_models[0].provider_id, 2);
}

/// 测试 Assistant Prompt 关联操作
///
/// 验证内容：
/// - 为 Assistant 添加 Prompt
/// - 获取 Assistant 的所有 Prompt
/// - 更新 Prompt 内容
/// - 按 assistant_id 删除 Prompt
#[test]
fn test_assistant_prompt_operations() {
    let db = create_assistant_db();

    // 创建 Assistant
    let assistant_id = db.add_assistant("Test", "Desc", None, false).unwrap();

    // 添加 Prompt
    let prompt_id = db.add_assistant_prompt(assistant_id, "You are a helpful assistant.").unwrap();
    assert!(prompt_id > 0);

    // 获取 Prompts
    let prompts = db.get_assistant_prompt(assistant_id).unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].prompt, "You are a helpful assistant.");

    // 更新 Prompt
    db.update_assistant_prompt(prompt_id, "You are a code expert.").unwrap();
    let updated_prompts = db.get_assistant_prompt(assistant_id).unwrap();
    assert_eq!(updated_prompts[0].prompt, "You are a code expert.");

    // 删除所有 Prompt
    db.delete_assistant_prompt_by_assistant_id(assistant_id).unwrap();
    let empty_prompts = db.get_assistant_prompt(assistant_id).unwrap();
    assert!(empty_prompts.is_empty());
}

/// 测试 Assistant Model Config 配置操作
///
/// 验证内容：
/// - 为 Assistant Model 添加配置项
/// - 获取配置列表
/// - 更新配置值
/// - 按 assistant_id 删除所有配置
#[test]
fn test_assistant_model_config_operations() {
    let db = create_assistant_db();

    // 创建 Assistant 和 Model
    let assistant_id = db.add_assistant("Test", "Desc", None, false).unwrap();
    let model_id = db.add_assistant_model(assistant_id, 1, "gpt-4", "GPT-4").unwrap();

    // 添加配置
    let config_id =
        db.add_assistant_model_config(assistant_id, model_id, "temperature", "0.7", "float")
            .unwrap();
    assert!(config_id > 0);

    // 获取配置
    let configs = db.get_assistant_model_configs(assistant_id).unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "temperature");
    assert_eq!(configs[0].value, Some("0.7".to_string()));

    // 更新配置
    db.update_assistant_model_config(config_id, "temperature", "0.9").unwrap();
    let updated_configs = db.get_assistant_model_configs(assistant_id).unwrap();
    assert_eq!(updated_configs[0].value, Some("0.9".to_string()));

    // 删除所有配置
    db.delete_assistant_model_config_by_assistant_id(assistant_id).unwrap();
    let empty_configs = db.get_assistant_model_configs(assistant_id).unwrap();
    assert!(empty_configs.is_empty());
}

/// 测试不同 assistant_type 的创建
///
/// 验证内容：
/// - assistant_type 为 None 的情况
/// - assistant_type 为不同值的情况
/// - is_addition 标志的正确存储
#[test]
fn test_assistant_types() {
    let db = create_assistant_db();

    // type = None
    let id1 = db.add_assistant("No Type", "Desc", None, false).unwrap();
    let a1 = db.get_assistant(id1).unwrap();
    assert!(a1.assistant_type.is_none());

    // type = Some(1)
    let id2 = db.add_assistant("Type 1", "Desc", Some(1), false).unwrap();
    let a2 = db.get_assistant(id2).unwrap();
    assert_eq!(a2.assistant_type, Some(1));

    // type = Some(999), is_addition = true
    let id3 = db.add_assistant("Type 999", "Desc", Some(999), true).unwrap();
    let a3 = db.get_assistant(id3).unwrap();
    assert_eq!(a3.assistant_type, Some(999));
    assert!(a3.is_addition);
}

// ============================================================================
// 异常和边界情况测试
// ============================================================================

/// 测试读取不存在的 Assistant
///
/// 验证内容：
/// - 读取不存在的 ID 返回 QueryReturnedNoRows 错误
#[test]
fn test_assistant_read_nonexistent() {
    let db = create_assistant_db();

    let result = db.get_assistant(999);
    assert!(result.is_err());
    match result {
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        _ => panic!("Expected QueryReturnedNoRows error"),
    }
}

/// 测试删除不存在的 Assistant
///
/// 验证内容：
/// - 删除不存在的 ID 不会产生错误（SQLite 的 DELETE 行为）
/// - 实际上没有删除任何行
#[test]
fn test_assistant_delete_nonexistent() {
    let db = create_assistant_db();

    // DELETE 语句对不存在的行不会报错
    let result = db.delete_assistant(999);
    assert!(result.is_ok());
}

/// 测试空名称的 Assistant
///
/// 验证内容：
/// - 空名称仍可以成功创建（数据库层不做业务验证）
#[test]
fn test_assistant_empty_name() {
    let db = create_assistant_db();

    let id = db.add_assistant("", "", None, false).unwrap();
    let assistant = db.get_assistant(id).unwrap();
    assert_eq!(assistant.name, "");
    assert_eq!(assistant.description, Some("".to_string()));
}

/// 测试超长名称的 Assistant
///
/// 验证内容：
/// - 超长名称（10000字符）可以正确存储和读取
#[test]
fn test_assistant_very_long_name() {
    let db = create_assistant_db();

    let long_name = "A".repeat(10000);
    let long_desc = "B".repeat(10000);

    let id = db.add_assistant(&long_name, &long_desc, None, false).unwrap();
    let assistant = db.get_assistant(id).unwrap();
    assert_eq!(assistant.name.len(), 10000);
    assert_eq!(assistant.description.as_ref().unwrap().len(), 10000);
}

/// 测试特殊字符在 Assistant 中的处理
///
/// 验证内容：
/// - 中文、日文、Emoji 能正确存储
/// - SQL 注入字符被正确转义
#[test]
fn test_assistant_special_characters() {
    let db = create_assistant_db();

    // 中文和 Emoji
    let id1 = db.add_assistant("测试助手 🤖", "这是描述 ✨", None, false).unwrap();
    let a1 = db.get_assistant(id1).unwrap();
    assert_eq!(a1.name, "测试助手 🤖");
    assert_eq!(a1.description, Some("这是描述 ✨".to_string()));

    // SQL 注入尝试
    let id2 =
        db.add_assistant("'; DROP TABLE assistant; --", "Normal desc", None, false).unwrap();
    let a2 = db.get_assistant(id2).unwrap();
    assert_eq!(a2.name, "'; DROP TABLE assistant; --");

    // 确保表还存在
    let all = db.get_assistants().unwrap();
    assert_eq!(all.len(), 2);
}

/// 测试获取不存在 Assistant 的 Model
///
/// 验证内容：
/// - 查询不存在 assistant_id 的 Model 返回空列表
#[test]
fn test_assistant_model_nonexistent_assistant() {
    let db = create_assistant_db();

    let models = db.get_assistant_model(999).unwrap();
    assert!(models.is_empty());
}

/// 测试获取不存在 Assistant 的 Prompt
///
/// 验证内容：
/// - 查询不存在 assistant_id 的 Prompt 返回空列表
#[test]
fn test_assistant_prompt_nonexistent_assistant() {
    let db = create_assistant_db();

    let prompts = db.get_assistant_prompt(999).unwrap();
    assert!(prompts.is_empty());
}

/// 测试获取不存在 Assistant 的 Config
///
/// 验证内容：
/// - 查询不存在 assistant_id 的 Config 返回空列表
#[test]
fn test_assistant_config_nonexistent_assistant() {
    let db = create_assistant_db();

    let configs = db.get_assistant_model_configs(999).unwrap();
    assert!(configs.is_empty());
}

/// 测试更新不存在的 Assistant
///
/// 验证内容：
/// - UPDATE 语句对不存在的行不会报错
/// - 实际上没有更新任何行
#[test]
fn test_assistant_update_nonexistent() {
    let db = create_assistant_db();

    // UPDATE 语句对不存在的行不会报错
    let result = db.update_assistant(999, "New Name", "New Desc");
    assert!(result.is_ok());
}

/// 测试删除不存在 Assistant 的关联数据
///
/// 验证内容：
/// - 删除不存在 assistant_id 的 Prompt 不会报错
/// - 删除不存在 assistant_id 的 Config 不会报错
#[test]
fn test_assistant_delete_nonexistent_relations() {
    let db = create_assistant_db();

    // 删除不存在的 Prompt
    let result = db.delete_assistant_prompt_by_assistant_id(999);
    assert!(result.is_ok());

    // 删除不存在的 Config
    let result = db.delete_assistant_model_config_by_assistant_id(999);
    assert!(result.is_ok());
}

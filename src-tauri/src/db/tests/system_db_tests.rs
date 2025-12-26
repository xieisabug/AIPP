//! SystemDatabase 单元测试
//!
//! 测试 system_config 和 feature_config 的 CRUD 操作

use rusqlite::Connection;

// Test helper to create in-memory database and initialize tables
fn create_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // Create system_config table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS system_config (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            key TEXT NOT NULL UNIQUE,
            value TEXT NOT NULL,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();

    // Create feature_config table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS feature_config (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            feature_code TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT,
            data_type TEXT,
            description TEXT,
            UNIQUE(feature_code, key)
        )",
        [],
    )
    .unwrap();

    conn
}

// ============================================================================
// system_config Tests
// ============================================================================

/// 测试创建和读取系统配置
#[test]
fn test_add_and_get_system_config() {
    let conn = create_test_db();

    // Add config
    conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?, ?)",
        ["app_version", "1.0.0"],
    )
    .unwrap();

    // Get config
    let value: String = conn
        .query_row(
            "SELECT value FROM system_config WHERE key = ?",
            ["app_version"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(value, "1.0.0");
}

/// 测试系统配置 key 唯一性约束
#[test]
fn test_system_config_key_unique() {
    let conn = create_test_db();

    // Add first config
    conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?, ?)",
        ["unique_key", "value1"],
    )
    .unwrap();

    // Try to add duplicate key - should fail
    let result = conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?, ?)",
        ["unique_key", "value2"],
    );

    assert!(result.is_err());
}

/// 测试更新系统配置
#[test]
fn test_update_system_config() {
    let conn = create_test_db();

    conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?, ?)",
        ["update_key", "original"],
    )
    .unwrap();

    // Update
    conn.execute(
        "UPDATE system_config SET value = ? WHERE key = ?",
        ["updated", "update_key"],
    )
    .unwrap();

    // Verify
    let value: String = conn
        .query_row(
            "SELECT value FROM system_config WHERE key = ?",
            ["update_key"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(value, "updated");
}

/// 测试删除系统配置
#[test]
fn test_delete_system_config() {
    let conn = create_test_db();

    conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?, ?)",
        ["delete_key", "to_delete"],
    )
    .unwrap();

    // Delete
    conn.execute("DELETE FROM system_config WHERE key = ?", ["delete_key"])
        .unwrap();

    // Verify
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM system_config WHERE key = ?",
            ["delete_key"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 0);
}

/// 测试获取不存在的配置
#[test]
fn test_get_nonexistent_config() {
    let conn = create_test_db();

    let result: rusqlite::Result<String> = conn.query_row(
        "SELECT value FROM system_config WHERE key = ?",
        ["nonexistent"],
        |row| row.get(0),
    );

    assert!(result.is_err());
}

// ============================================================================
// feature_config Tests
// ============================================================================

/// 测试创建和读取 feature 配置
#[test]
fn test_add_and_get_feature_config() {
    let conn = create_test_db();

    // Add config
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            "conversation_summary",
            "summary_length",
            "100",
            "string",
            "对话总结使用长度"
        ],
    )
    .unwrap();

    // Get config
    let (value, data_type): (String, String) = conn
        .query_row(
            "SELECT value, data_type FROM feature_config WHERE feature_code = ? AND key = ?",
            ["conversation_summary", "summary_length"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(value, "100");
    assert_eq!(data_type, "string");
}

/// 测试 feature 配置复合唯一键约束
#[test]
fn test_feature_config_unique_constraint() {
    let conn = create_test_db();

    // Add first config
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["module1", "key1", "value1", "string", "desc"],
    )
    .unwrap();

    // Try to add duplicate (feature_code, key) - should fail
    let result = conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["module1", "key1", "value2", "string", "desc2"],
    );

    assert!(result.is_err());
}

/// 测试同一 feature 可以有多个 key
#[test]
fn test_multiple_keys_per_feature() {
    let conn = create_test_db();

    // Add multiple keys for same feature
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["feature1", "key1", "value1", "string", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["feature1", "key2", "value2", "number", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["feature1", "key3", "value3", "boolean", None::<String>],
    )
    .unwrap();

    // Count keys for feature
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feature_config WHERE feature_code = ?",
            ["feature1"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 3);
}

/// 测试不同 feature 可以有相同 key
#[test]
fn test_same_key_different_features() {
    let conn = create_test_db();

    // Add same key for different features
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["feature_a", "enabled", "true", "boolean", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["feature_b", "enabled", "false", "boolean", None::<String>],
    )
    .unwrap();

    // Verify both exist
    let value_a: String = conn
        .query_row(
            "SELECT value FROM feature_config WHERE feature_code = ? AND key = ?",
            ["feature_a", "enabled"],
            |row| row.get(0),
        )
        .unwrap();

    let value_b: String = conn
        .query_row(
            "SELECT value FROM feature_config WHERE feature_code = ? AND key = ?",
            ["feature_b", "enabled"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(value_a, "true");
    assert_eq!(value_b, "false");
}

/// 测试更新 feature 配置
#[test]
fn test_update_feature_config() {
    let conn = create_test_db();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["test_feature", "test_key", "old_value", "string", "old desc"],
    )
    .unwrap();

    // Update
    conn.execute(
        "UPDATE feature_config SET value = ?, description = ? WHERE feature_code = ? AND key = ?",
        rusqlite::params!["new_value", "new desc", "test_feature", "test_key"],
    )
    .unwrap();

    // Verify
    let (value, description): (String, String) = conn
        .query_row(
            "SELECT value, description FROM feature_config WHERE feature_code = ? AND key = ?",
            ["test_feature", "test_key"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(value, "new_value");
    assert_eq!(description, "new desc");
}

/// 测试按 feature_code 删除所有配置
#[test]
fn test_delete_feature_config_by_feature_code() {
    let conn = create_test_db();

    // Add configs for two features
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["to_delete", "key1", "value1", "string", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["to_delete", "key2", "value2", "string", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["to_keep", "key1", "value1", "string", None::<String>],
    )
    .unwrap();

    // Delete all for 'to_delete'
    conn.execute(
        "DELETE FROM feature_config WHERE feature_code = ?",
        ["to_delete"],
    )
    .unwrap();

    // Verify
    let deleted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feature_config WHERE feature_code = ?",
            ["to_delete"],
            |row| row.get(0),
        )
        .unwrap();

    let kept_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feature_config WHERE feature_code = ?",
            ["to_keep"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(deleted_count, 0);
    assert_eq!(kept_count, 1);
}

/// 测试获取所有 feature 配置
#[test]
fn test_get_all_feature_config() {
    let conn = create_test_db();

    // Add configs
    for i in 1..=5 {
        conn.execute(
            "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                format!("feature{}", i),
                "key",
                "value",
                "string",
                None::<String>
            ],
        )
        .unwrap();
    }

    // Get all
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM feature_config", [], |row| row.get(0))
        .unwrap();

    assert_eq!(count, 5);
}

/// 测试 NULL 值处理
#[test]
fn test_null_values() {
    let conn = create_test_db();

    // Add config with NULL description
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["null_test", "key1", "value", "string", None::<String>],
    )
    .unwrap();

    // Add config with NULL value
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["null_test", "key2", None::<String>, "string", "has desc"],
    )
    .unwrap();

    // Verify NULL handling
    let desc: Option<String> = conn
        .query_row(
            "SELECT description FROM feature_config WHERE feature_code = ? AND key = ?",
            ["null_test", "key1"],
            |row| row.get(0),
        )
        .unwrap();

    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM feature_config WHERE feature_code = ? AND key = ?",
            ["null_test", "key2"],
            |row| row.get(0),
        )
        .unwrap();

    assert!(desc.is_none());
    assert!(value.is_none());
}

/// 测试数据类型字段
#[test]
fn test_data_type_field() {
    let conn = create_test_db();

    // Add configs with different data types
    let types = ["string", "number", "boolean", "json", "array"];
    for (i, data_type) in types.iter().enumerate() {
        conn.execute(
            "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params!["type_test", format!("key{}", i), "value", *data_type, None::<String>],
        )
        .unwrap();
    }

    // Query by data type
    let string_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feature_config WHERE data_type = 'string'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let boolean_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feature_config WHERE data_type = 'boolean'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(string_count, 1);
    assert_eq!(boolean_count, 1);
}

/// 测试长文本值（如 prompt 配置）
#[test]
fn test_long_text_value() {
    let conn = create_test_db();

    let long_prompt = "请根据提供的大模型问答对话,总结一个简洁明了的标题。标题要求:\n\
        - 字数在5-15个字左右，必须是中文，不要包含标点符号\n\
        - 准确概括对话的核心主题，尽量贴近用户的提问\n\
        - 不要透露任何私人信息\n\
        - 用祈使句或陈述句";

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            "conversation_summary",
            "prompt",
            long_prompt,
            "string",
            "对话总结提示词"
        ],
    )
    .unwrap();

    // Read back
    let stored_value: String = conn
        .query_row(
            "SELECT value FROM feature_config WHERE feature_code = ? AND key = ?",
            ["conversation_summary", "prompt"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(stored_value, long_prompt);
}

/// 测试按模块查询配置
#[test]
fn test_get_feature_config_by_module() {
    let conn = create_test_db();

    // Add configs for multiple modules
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["module_a", "key1", "value1", "string", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["module_a", "key2", "value2", "string", None::<String>],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["module_b", "key1", "value1", "string", None::<String>],
    )
    .unwrap();

    // Query by module
    let module_a_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM feature_config WHERE feature_code = ?",
            ["module_a"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(module_a_count, 2);
}

/// 测试 ID 自增
#[test]
fn test_id_autoincrement() {
    let conn = create_test_db();

    // Insert multiple configs
    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["auto_id", "key1", "value1", "string", None::<String>],
    )
    .unwrap();
    let id1 = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["auto_id", "key2", "value2", "string", None::<String>],
    )
    .unwrap();
    let id2 = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO feature_config (feature_code, key, value, data_type, description) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["auto_id", "key3", "value3", "string", None::<String>],
    )
    .unwrap();
    let id3 = conn.last_insert_rowid();

    assert_eq!(id2, id1 + 1);
    assert_eq!(id3, id2 + 1);
}

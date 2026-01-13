//! PluginDatabase 单元测试
//!
//! 测试 Plugin, PluginStatus, PluginConfiguration, PluginData 的 CRUD 操作

use chrono::Utc;
use rusqlite::Connection;

// Test helper to create in-memory database and initialize tables
fn create_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // Create Plugins table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS Plugins (
            plugin_id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            folder_name TEXT NOT NULL,
            description TEXT,
            author TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();

    // Create PluginStatus table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS PluginStatus (
            status_id INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id INTEGER,
            is_active INTEGER DEFAULT 1,
            last_run TIMESTAMP,
            FOREIGN KEY (plugin_id) REFERENCES Plugins(plugin_id)
        )",
        [],
    )
    .unwrap();

    // Create PluginConfigurations table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS PluginConfigurations (
            config_id INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id INTEGER,
            config_key TEXT NOT NULL,
            config_value TEXT,
            FOREIGN KEY (plugin_id) REFERENCES Plugins(plugin_id)
        )",
        [],
    )
    .unwrap();

    // Create PluginData table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS PluginData (
            data_id INTEGER PRIMARY KEY AUTOINCREMENT,
            plugin_id INTEGER,
            session_id TEXT NOT NULL,
            data_key TEXT NOT NULL,
            data_value TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (plugin_id) REFERENCES Plugins(plugin_id)
        )",
        [],
    )
    .unwrap();

    conn
}

// ============================================================================
// Plugin CRUD Tests
// ============================================================================

/// 测试创建和读取插件
#[test]
fn test_add_and_get_plugin() {
    let conn = create_test_db();
    let now = Utc::now();

    // Add plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, description, author, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        rusqlite::params!["Test Plugin", "1.0.0", "test-plugin", "A test plugin", "Test Author", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Get plugin
    let (name, version, folder_name): (String, String, String) = conn
        .query_row(
            "SELECT name, version, folder_name FROM Plugins WHERE plugin_id = ?",
            [plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(name, "Test Plugin");
    assert_eq!(version, "1.0.0");
    assert_eq!(folder_name, "test-plugin");
}

/// 测试列出所有插件
#[test]
fn test_get_plugins() {
    let conn = create_test_db();
    let now = Utc::now();

    // Add multiple plugins
    for i in 1..=3 {
        conn.execute(
            "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![
                format!("Plugin {}", i),
                format!("{}.0.0", i),
                format!("plugin-{}", i),
                now,
                now
            ],
        )
        .unwrap();
    }

    // Count plugins
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM Plugins", [], |row| row.get(0)).unwrap();

    assert_eq!(count, 3);
}

/// 测试更新插件
#[test]
fn test_update_plugin() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, description, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params!["Original", "1.0.0", "original", "Old desc", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Update
    let updated = Utc::now();
    conn.execute(
        "UPDATE Plugins SET name = ?, version = ?, description = ?, updated_at = ? WHERE plugin_id = ?",
        rusqlite::params!["Updated", "2.0.0", "New desc", updated, plugin_id],
    )
    .unwrap();

    // Verify
    let (name, version, description): (String, String, String) = conn
        .query_row(
            "SELECT name, version, description FROM Plugins WHERE plugin_id = ?",
            [plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(name, "Updated");
    assert_eq!(version, "2.0.0");
    assert_eq!(description, "New desc");
}

/// 测试删除插件
#[test]
fn test_delete_plugin() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["To Delete", "1.0.0", "to-delete", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Delete
    conn.execute("DELETE FROM Plugins WHERE plugin_id = ?", [plugin_id]).unwrap();

    // Verify
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM Plugins WHERE plugin_id = ?", [plugin_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(count, 0);
}

/// 测试可选字段（description, author）
#[test]
fn test_optional_fields() {
    let conn = create_test_db();
    let now = Utc::now();

    // Add plugin without optional fields
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Minimal", "1.0.0", "minimal", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Get optional fields
    let (description, author): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT description, author FROM Plugins WHERE plugin_id = ?",
            [plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert!(description.is_none());
    assert!(author.is_none());
}

// ============================================================================
// PluginStatus Tests
// ============================================================================

/// 测试创建和获取插件状态
#[test]
fn test_plugin_status_crud() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin first
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Status Test", "1.0.0", "status-test", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add status
    conn.execute(
        "INSERT INTO PluginStatus (plugin_id, is_active, last_run) VALUES (?, ?, ?)",
        rusqlite::params![plugin_id, 1i64, now],
    )
    .unwrap();

    // Get status
    let (is_active, last_run): (i64, chrono::DateTime<Utc>) = conn
        .query_row(
            "SELECT is_active, last_run FROM PluginStatus WHERE plugin_id = ?",
            [plugin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(is_active, 1);
    assert!(last_run <= Utc::now());
}

/// 测试更新插件状态
#[test]
fn test_update_plugin_status() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin and status
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Status Update", "1.0.0", "status-update", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO PluginStatus (plugin_id, is_active, last_run) VALUES (?, ?, ?)",
        rusqlite::params![plugin_id, 1i64, now],
    )
    .unwrap();

    let status_id = conn.last_insert_rowid();

    // Update to inactive
    let new_run = Utc::now();
    conn.execute(
        "UPDATE PluginStatus SET is_active = ?, last_run = ? WHERE status_id = ?",
        rusqlite::params![0i64, new_run, status_id],
    )
    .unwrap();

    // Verify
    let is_active: i64 = conn
        .query_row("SELECT is_active FROM PluginStatus WHERE status_id = ?", [status_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(is_active, 0);
}

// ============================================================================
// PluginConfiguration Tests
// ============================================================================

/// 测试添加和获取插件配置
#[test]
fn test_plugin_configuration_crud() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Config Test", "1.0.0", "config-test", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add configuration
    conn.execute(
        "INSERT INTO PluginConfigurations (plugin_id, config_key, config_value) VALUES (?, ?, ?)",
        rusqlite::params![plugin_id, "api_key", "secret123"],
    )
    .unwrap();

    // Get configuration
    let config_value: String = conn
        .query_row(
            "SELECT config_value FROM PluginConfigurations WHERE plugin_id = ? AND config_key = ?",
            rusqlite::params![plugin_id, "api_key"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(config_value, "secret123");
}

/// 测试多个配置项
#[test]
fn test_multiple_configurations() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Multi Config", "1.0.0", "multi-config", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add multiple configurations
    for i in 1..=5 {
        conn.execute(
            "INSERT INTO PluginConfigurations (plugin_id, config_key, config_value) VALUES (?, ?, ?)",
            rusqlite::params![plugin_id, format!("key{}", i), format!("value{}", i)],
        )
        .unwrap();
    }

    // Count configurations
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM PluginConfigurations WHERE plugin_id = ?",
            [plugin_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 5);
}

/// 测试更新配置
#[test]
fn test_update_configuration() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Update Config", "1.0.0", "update-config", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add configuration
    conn.execute(
        "INSERT INTO PluginConfigurations (plugin_id, config_key, config_value) VALUES (?, ?, ?)",
        rusqlite::params![plugin_id, "setting", "old_value"],
    )
    .unwrap();

    let config_id = conn.last_insert_rowid();

    // Update
    conn.execute(
        "UPDATE PluginConfigurations SET config_value = ? WHERE config_id = ?",
        rusqlite::params!["new_value", config_id],
    )
    .unwrap();

    // Verify
    let config_value: String = conn
        .query_row(
            "SELECT config_value FROM PluginConfigurations WHERE config_id = ?",
            [config_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(config_value, "new_value");
}

/// 测试删除配置
#[test]
fn test_delete_configuration() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Delete Config", "1.0.0", "delete-config", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add configuration
    conn.execute(
        "INSERT INTO PluginConfigurations (plugin_id, config_key, config_value) VALUES (?, ?, ?)",
        rusqlite::params![plugin_id, "temp", "value"],
    )
    .unwrap();

    let config_id = conn.last_insert_rowid();

    // Delete
    conn.execute("DELETE FROM PluginConfigurations WHERE config_id = ?", [config_id]).unwrap();

    // Verify
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM PluginConfigurations WHERE config_id = ?",
            [config_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 0);
}

// ============================================================================
// PluginData Tests
// ============================================================================

/// 测试添加和获取插件数据
#[test]
fn test_plugin_data_crud() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Data Test", "1.0.0", "data-test", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add data
    conn.execute(
        "INSERT INTO PluginData (plugin_id, session_id, data_key, data_value, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![plugin_id, "session-123", "user_input", "Hello World", now, now],
    )
    .unwrap();

    let data_id = conn.last_insert_rowid();

    // Get data
    let (session_id, data_key, data_value): (String, String, String) = conn
        .query_row(
            "SELECT session_id, data_key, data_value FROM PluginData WHERE data_id = ?",
            [data_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(session_id, "session-123");
    assert_eq!(data_key, "user_input");
    assert_eq!(data_value, "Hello World");
}

/// 测试按 session 获取数据
#[test]
fn test_get_plugin_data_by_session() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Session Data", "1.0.0", "session-data", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add data for different sessions
    for session in ["session-a", "session-a", "session-b"] {
        conn.execute(
            "INSERT INTO PluginData (plugin_id, session_id, data_key, data_value, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![plugin_id, session, "key", "value", now, now],
        )
        .unwrap();
    }

    // Count by session
    let session_a_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM PluginData WHERE plugin_id = ? AND session_id = ?",
            rusqlite::params![plugin_id, "session-a"],
            |row| row.get(0),
        )
        .unwrap();

    let session_b_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM PluginData WHERE plugin_id = ? AND session_id = ?",
            rusqlite::params![plugin_id, "session-b"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(session_a_count, 2);
    assert_eq!(session_b_count, 1);
}

/// 测试更新插件数据
#[test]
fn test_update_plugin_data() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Update Data", "1.0.0", "update-data", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add data
    conn.execute(
        "INSERT INTO PluginData (plugin_id, session_id, data_key, data_value, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![plugin_id, "session", "key", "old", now, now],
    )
    .unwrap();

    let data_id = conn.last_insert_rowid();

    // Update
    let updated = Utc::now();
    conn.execute(
        "UPDATE PluginData SET data_value = ?, updated_at = ? WHERE data_id = ?",
        rusqlite::params!["new", updated, data_id],
    )
    .unwrap();

    // Verify
    let data_value: String = conn
        .query_row("SELECT data_value FROM PluginData WHERE data_id = ?", [data_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(data_value, "new");
}

/// 测试删除插件数据
#[test]
fn test_delete_plugin_data() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Delete Data", "1.0.0", "delete-data", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add data
    conn.execute(
        "INSERT INTO PluginData (plugin_id, session_id, data_key, data_value, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![plugin_id, "session", "key", "value", now, now],
    )
    .unwrap();

    let data_id = conn.last_insert_rowid();

    // Delete
    conn.execute("DELETE FROM PluginData WHERE data_id = ?", [data_id]).unwrap();

    // Verify
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM PluginData WHERE data_id = ?", [data_id], |row| row.get(0))
        .unwrap();

    assert_eq!(count, 0);
}

/// 测试 NULL 数据值
#[test]
fn test_null_data_value() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create plugin
    conn.execute(
        "INSERT INTO Plugins (name, version, folder_name, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params!["Null Data", "1.0.0", "null-data", now, now],
    )
    .unwrap();

    let plugin_id = conn.last_insert_rowid();

    // Add data with NULL value
    conn.execute(
        "INSERT INTO PluginData (plugin_id, session_id, data_key, data_value, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?)",
        rusqlite::params![plugin_id, "session", "key", None::<String>, now, now],
    )
    .unwrap();

    let data_id = conn.last_insert_rowid();

    // Verify NULL handling
    let data_value: Option<String> = conn
        .query_row("SELECT data_value FROM PluginData WHERE data_id = ?", [data_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert!(data_value.is_none());
}

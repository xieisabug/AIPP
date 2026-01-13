//! SubTaskDatabase 单元测试
//!
//! 测试 SubTaskDefinition 和 SubTaskExecution 的 CRUD 操作

use chrono::Utc;
use rusqlite::Connection;

// Test helper to create in-memory database and initialize tables
fn create_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // Create sub_task_definition table
    conn.execute(
        "CREATE TABLE sub_task_definition (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            code TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL,
            system_prompt TEXT NOT NULL,
            plugin_source TEXT NOT NULL,
            source_id INTEGER NOT NULL,
            is_enabled BOOLEAN NOT NULL DEFAULT 1,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .unwrap();

    // Create sub_task_execution table
    conn.execute(
        "CREATE TABLE sub_task_execution (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_definition_id INTEGER NOT NULL,
            task_code TEXT NOT NULL,
            task_name TEXT NOT NULL,
            task_prompt TEXT NOT NULL,
            parent_conversation_id INTEGER NOT NULL,
            parent_message_id INTEGER,
            status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'running', 'success', 'failed', 'cancelled')),
            result_content TEXT,
            error_message TEXT,
            mcp_result_json TEXT,
            llm_model_id INTEGER,
            llm_model_name TEXT,
            token_count INTEGER DEFAULT 0,
            input_token_count INTEGER DEFAULT 0,
            output_token_count INTEGER DEFAULT 0,
            started_time DATETIME,
            finished_time DATETIME,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (task_definition_id) REFERENCES sub_task_definition(id) ON DELETE CASCADE
        )",
        [],
    )
    .unwrap();

    conn
}

// ============================================================================
// SubTaskDefinition CRUD Tests
// ============================================================================

/// 测试创建和读取子任务定义
#[test]
fn test_create_and_read_definition() {
    let conn = create_test_db();
    let now = Utc::now();

    // Insert definition
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            "Test Task",
            "test_task",
            "A test task description",
            "You are a helpful assistant",
            "mcp",
            1i64,
            true,
            now,
            now
        ],
    )
    .unwrap();

    let id = conn.last_insert_rowid();

    // Read back
    let (name, code, description): (String, String, String) = conn
        .query_row(
            "SELECT name, code, description FROM sub_task_definition WHERE id = ?",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(name, "Test Task");
    assert_eq!(code, "test_task");
    assert_eq!(description, "A test task description");
}

/// 测试按代码查找定义
#[test]
fn test_find_definition_by_code() {
    let conn = create_test_db();
    let now = Utc::now();

    // Insert two definitions
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task A', 'task_a', 'Description A', 'Prompt A', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task B', 'task_b', 'Description B', 'Prompt B', 'plugin', 2, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    // Find by code
    let name: String = conn
        .query_row("SELECT name FROM sub_task_definition WHERE code = ?", ["task_b"], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(name, "Task B");
}

/// 测试代码唯一性约束
#[test]
fn test_definition_code_unique() {
    let conn = create_test_db();
    let now = Utc::now();

    // Insert first definition
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task 1', 'duplicate_code', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    // Try to insert duplicate code - should fail
    let result = conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task 2', 'duplicate_code', 'Desc', 'Prompt', 'mcp', 2, 1, ?1, ?1)",
        [now],
    );

    assert!(result.is_err());
}

/// 测试更新定义
#[test]
fn test_update_definition() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Original Name', 'update_test', 'Original Desc', 'Original Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let id = conn.last_insert_rowid();

    // Update
    conn.execute(
        "UPDATE sub_task_definition SET name = 'Updated Name', description = 'Updated Desc' WHERE id = ?",
        [id],
    )
    .unwrap();

    // Verify
    let (name, description): (String, String) = conn
        .query_row("SELECT name, description FROM sub_task_definition WHERE id = ?", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();

    assert_eq!(name, "Updated Name");
    assert_eq!(description, "Updated Desc");
}

/// 测试删除定义
#[test]
fn test_delete_definition() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('To Delete', 'delete_me', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let id = conn.last_insert_rowid();

    // Delete
    conn.execute("DELETE FROM sub_task_definition WHERE id = ?", [id]).unwrap();

    // Verify
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sub_task_definition WHERE id = ?", [id], |row| row.get(0))
        .unwrap();

    assert_eq!(count, 0);
}

/// 测试按 source 过滤列出定义
#[test]
fn test_list_definitions_by_source() {
    let conn = create_test_db();
    let now = Utc::now();

    // Insert definitions from different sources
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('MCP Task 1', 'mcp_1', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('MCP Task 2', 'mcp_2', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Plugin Task', 'plugin_1', 'Desc', 'Prompt', 'plugin', 2, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    // Count by source
    let mcp_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sub_task_definition WHERE plugin_source = 'mcp'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let plugin_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sub_task_definition WHERE plugin_source = 'plugin'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(mcp_count, 2);
    assert_eq!(plugin_count, 1);
}

/// 测试启用/禁用定义
#[test]
fn test_definition_enabled_status() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'enable_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let id = conn.last_insert_rowid();

    // Check initially enabled
    let enabled: bool = conn
        .query_row("SELECT is_enabled FROM sub_task_definition WHERE id = ?", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(enabled);

    // Disable
    conn.execute("UPDATE sub_task_definition SET is_enabled = 0 WHERE id = ?", [id]).unwrap();

    let enabled: bool = conn
        .query_row("SELECT is_enabled FROM sub_task_definition WHERE id = ?", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!enabled);

    // Filter by enabled
    let enabled_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sub_task_definition WHERE is_enabled = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(enabled_count, 0);
}

// ============================================================================
// SubTaskExecution CRUD Tests
// ============================================================================

/// 测试创建和读取执行记录
#[test]
fn test_create_and_read_execution() {
    let conn = create_test_db();
    let now = Utc::now();

    // First create a definition
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'exec_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    // Create execution
    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, parent_message_id, status, created_time) 
         VALUES (?1, 'exec_test', 'Task', 'Execute this task', 100, 50, 'pending', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Read back
    let (task_code, status, parent_conv_id): (String, String, i64) = conn
        .query_row(
            "SELECT task_code, status, parent_conversation_id FROM sub_task_execution WHERE id = ?",
            [exec_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(task_code, "exec_test");
    assert_eq!(status, "pending");
    assert_eq!(parent_conv_id, 100);
}

/// 测试执行状态更新
#[test]
fn test_update_execution_status() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create definition and execution
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'status_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
         VALUES (?1, 'status_test', 'Task', 'Prompt', 1, 'pending', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Update to running
    conn.execute(
        "UPDATE sub_task_execution SET status = 'running', started_time = ?1 WHERE id = ?2",
        rusqlite::params![now, exec_id],
    )
    .unwrap();

    let status: String = conn
        .query_row("SELECT status FROM sub_task_execution WHERE id = ?", [exec_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(status, "running");

    // Update to success
    conn.execute(
        "UPDATE sub_task_execution SET status = 'success', finished_time = ?1 WHERE id = ?2",
        rusqlite::params![now, exec_id],
    )
    .unwrap();

    let status: String = conn
        .query_row("SELECT status FROM sub_task_execution WHERE id = ?", [exec_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(status, "success");
}

/// 测试执行结果更新（包含 token 统计）
#[test]
fn test_update_execution_result() {
    let conn = create_test_db();
    let now = Utc::now();

    // Create definition and execution
    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'result_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
         VALUES (?1, 'result_test', 'Task', 'Prompt', 1, 'running', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Update with result
    conn.execute(
        "UPDATE sub_task_execution SET status = 'success', result_content = ?1, token_count = 100, input_token_count = 40, output_token_count = 60, finished_time = ?2 WHERE id = ?3",
        rusqlite::params!["Task completed successfully!", now, exec_id],
    )
    .unwrap();

    let (result, token_count, input_tokens, output_tokens): (String, i32, i32, i32) = conn
        .query_row(
            "SELECT result_content, token_count, input_token_count, output_token_count FROM sub_task_execution WHERE id = ?",
            [exec_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(result, "Task completed successfully!");
    assert_eq!(token_count, 100);
    assert_eq!(input_tokens, 40);
    assert_eq!(output_tokens, 60);
}

/// 测试执行失败记录
#[test]
fn test_execution_failure() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'fail_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
         VALUES (?1, 'fail_test', 'Task', 'Prompt', 1, 'running', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Update with error
    conn.execute(
        "UPDATE sub_task_execution SET status = 'failed', error_message = 'Connection timeout' WHERE id = ?",
        [exec_id],
    )
    .unwrap();

    let (status, error): (String, String) = conn
        .query_row(
            "SELECT status, error_message FROM sub_task_execution WHERE id = ?",
            [exec_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(status, "failed");
    assert_eq!(error, "Connection timeout");
}

/// 测试按对话 ID 列出执行记录
#[test]
fn test_list_executions_by_conversation() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'list_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    // Create executions for different conversations
    for conv_id in [100, 100, 100, 200, 200] {
        conn.execute(
            "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
             VALUES (?1, 'list_test', 'Task', 'Prompt', ?2, 'pending', ?3)",
            rusqlite::params![def_id, conv_id, now],
        )
        .unwrap();
    }

    // Count by conversation
    let count_100: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sub_task_execution WHERE parent_conversation_id = 100",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let count_200: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sub_task_execution WHERE parent_conversation_id = 200",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count_100, 3);
    assert_eq!(count_200, 2);
}

/// 测试按消息 ID 过滤执行记录
#[test]
fn test_list_executions_by_message() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'msg_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    // Create executions with different message IDs
    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, parent_message_id, status, created_time) 
         VALUES (?1, 'msg_test', 'Task', 'Prompt', 100, 10, 'pending', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, parent_message_id, status, created_time) 
         VALUES (?1, 'msg_test', 'Task', 'Prompt', 100, 20, 'pending', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, parent_message_id, status, created_time) 
         VALUES (?1, 'msg_test', 'Task', 'Prompt', 100, NULL, 'pending', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    // Count by message ID
    let count_msg_10: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sub_task_execution WHERE parent_message_id = 10",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let count_null_msg: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sub_task_execution WHERE parent_message_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count_msg_10, 1);
    assert_eq!(count_null_msg, 1);
}

/// 测试按状态过滤执行记录
#[test]
fn test_list_executions_by_status() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'status_filter', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    // Create executions with different statuses
    for status in ["pending", "running", "success", "failed", "cancelled"] {
        conn.execute(
            "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
             VALUES (?1, 'status_filter', 'Task', 'Prompt', 1, ?2, ?3)",
            rusqlite::params![def_id, status, now],
        )
        .unwrap();
    }

    // Count by status
    let pending_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sub_task_execution WHERE status = 'pending'", [], |row| {
            row.get(0)
        })
        .unwrap();

    let success_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sub_task_execution WHERE status = 'success'", [], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(pending_count, 1);
    assert_eq!(success_count, 1);
}

/// 测试状态约束
#[test]
fn test_execution_status_constraint() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'constraint_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    // Try to insert with invalid status
    let result = conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
         VALUES (?1, 'constraint_test', 'Task', 'Prompt', 1, 'invalid_status', ?2)",
        rusqlite::params![def_id, now],
    );

    assert!(result.is_err());
}

/// 测试删除执行记录
#[test]
fn test_delete_execution() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'delete_exec', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
         VALUES (?1, 'delete_exec', 'Task', 'Prompt', 1, 'pending', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Delete
    conn.execute("DELETE FROM sub_task_execution WHERE id = ?", [exec_id]).unwrap();

    // Verify
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sub_task_execution WHERE id = ?", [exec_id], |row| {
            row.get(0)
        })
        .unwrap();

    assert_eq!(count, 0);
}

/// 测试 MCP 结果 JSON 存储
#[test]
fn test_mcp_result_json() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'mcp_json', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
         VALUES (?1, 'mcp_json', 'Task', 'Prompt', 1, 'success', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Update with MCP result JSON
    let mcp_json = r#"{"final_text":"result","loops":2,"calls":[]}"#;
    conn.execute(
        "UPDATE sub_task_execution SET mcp_result_json = ?1 WHERE id = ?2",
        rusqlite::params![mcp_json, exec_id],
    )
    .unwrap();

    // Read back
    let stored_json: String = conn
        .query_row(
            "SELECT mcp_result_json FROM sub_task_execution WHERE id = ?",
            [exec_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(stored_json, mcp_json);
}

/// 测试分页查询
#[test]
fn test_pagination() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'page_test', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    // Create 15 executions
    for i in 1..=15 {
        conn.execute(
            "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, created_time) 
             VALUES (?1, 'page_test', ?2, 'Prompt', 1, 'pending', ?3)",
            rusqlite::params![def_id, format!("Task {}", i), now],
        )
        .unwrap();
    }

    // Page 1: 5 items
    let page1_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT * FROM sub_task_execution WHERE parent_conversation_id = 1 LIMIT 5 OFFSET 0)",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // Page 2: 5 items
    let page2_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT * FROM sub_task_execution WHERE parent_conversation_id = 1 LIMIT 5 OFFSET 5)",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // Page 3: 5 items
    let page3_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT * FROM sub_task_execution WHERE parent_conversation_id = 1 LIMIT 5 OFFSET 10)",
            [],
            |row| row.get(0),
        )
        .unwrap();

    // Page 4: 0 items
    let page4_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT * FROM sub_task_execution WHERE parent_conversation_id = 1 LIMIT 5 OFFSET 15)",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(page1_count, 5);
    assert_eq!(page2_count, 5);
    assert_eq!(page3_count, 5);
    assert_eq!(page4_count, 0);
}

/// 测试 LLM 模型信息存储
#[test]
fn test_llm_model_info() {
    let conn = create_test_db();
    let now = Utc::now();

    conn.execute(
        "INSERT INTO sub_task_definition (name, code, description, system_prompt, plugin_source, source_id, is_enabled, created_time, updated_time) 
         VALUES ('Task', 'llm_info', 'Desc', 'Prompt', 'mcp', 1, 1, ?1, ?1)",
        [now],
    )
    .unwrap();

    let def_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO sub_task_execution (task_definition_id, task_code, task_name, task_prompt, parent_conversation_id, status, llm_model_id, llm_model_name, created_time) 
         VALUES (?1, 'llm_info', 'Task', 'Prompt', 1, 'success', 42, 'gpt-4', ?2)",
        rusqlite::params![def_id, now],
    )
    .unwrap();

    let exec_id = conn.last_insert_rowid();

    // Read back
    let (model_id, model_name): (i64, String) = conn
        .query_row(
            "SELECT llm_model_id, llm_model_name FROM sub_task_execution WHERE id = ?",
            [exec_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(model_id, 42);
    assert_eq!(model_name, "gpt-4");
}

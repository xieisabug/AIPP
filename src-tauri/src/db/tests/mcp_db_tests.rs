//! MCP Server 和相关表的数据库操作测试
//!
//! ## 测试范围
//! - MCP Server CRUD 操作
//! - MCP Server Tool 操作
//! - MCP Server Resource 操作
//! - MCP Server Prompt 操作
//! - MCP Tool Call 历史记录操作
//!
//! ## 测试隔离
//! 所有测试使用 `Connection::open_in_memory()` 创建内存数据库

use crate::db::connection::{Connection, DbError};
use crate::db::mcp_db::*;

// ============================================================================
// 测试辅助函数
// ============================================================================

/// 创建测试用内存数据库并初始化 MCP 相关表结构
///
/// **安全性**: 使用内存数据库，不会影响真实数据
fn create_mcp_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // 创建 mcp_server 表
    conn.execute(
        "CREATE TABLE mcp_server (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            transport_type TEXT NOT NULL,
            command TEXT,
            environment_variables TEXT,
            headers TEXT,
            url TEXT,
            timeout INTEGER DEFAULT 30000,
            is_long_running BOOLEAN NOT NULL DEFAULT 0,
            is_enabled BOOLEAN NOT NULL DEFAULT 1,
            is_builtin BOOLEAN NOT NULL DEFAULT 0,
            is_deletable BOOLEAN NOT NULL DEFAULT 1,
            proxy_enabled BOOLEAN NOT NULL DEFAULT 0,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    )
    .unwrap();

    // 创建 mcp_server_tool 表
    conn.execute(
        "CREATE TABLE mcp_server_tool (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_id INTEGER NOT NULL,
            tool_name TEXT NOT NULL,
            tool_description TEXT,
            is_enabled BOOLEAN NOT NULL DEFAULT 1,
            is_auto_run BOOLEAN NOT NULL DEFAULT 0,
            parameters TEXT,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
            UNIQUE(server_id, tool_name)
        )",
        (),
    )
    .unwrap();

    // 创建 mcp_server_resource 表
    conn.execute(
        "CREATE TABLE mcp_server_resource (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_id INTEGER NOT NULL,
            resource_uri TEXT NOT NULL,
            resource_name TEXT NOT NULL,
            resource_type TEXT NOT NULL,
            resource_description TEXT,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
            UNIQUE(server_id, resource_uri)
        )",
        (),
    )
    .unwrap();

    // 创建 mcp_server_prompt 表
    conn.execute(
        "CREATE TABLE mcp_server_prompt (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_id INTEGER NOT NULL,
            prompt_name TEXT NOT NULL,
            prompt_description TEXT,
            is_enabled BOOLEAN NOT NULL DEFAULT 1,
            arguments TEXT,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
            UNIQUE(server_id, prompt_name)
        )",
        (),
    )
    .unwrap();

    // 创建 mcp_tool_call 表
    conn.execute(
        "CREATE TABLE mcp_tool_call (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            conversation_id INTEGER NOT NULL,
            message_id INTEGER,
            server_id INTEGER NOT NULL,
            server_name TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            parameters TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'executing', 'success', 'failed')),
            result TEXT,
            error TEXT,
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            started_time DATETIME,
            finished_time DATETIME,
            llm_call_id TEXT,
            assistant_message_id INTEGER,
            subtask_id INTEGER,
            FOREIGN KEY (server_id) REFERENCES mcp_server(id) ON DELETE CASCADE
        )",
        (),
    )
    .unwrap();

    conn
}

/// 创建 MCPDatabase 实例用于测试
fn create_mcp_db() -> MCPDatabase {
    let conn = create_mcp_test_db();
    MCPDatabase { conn }
}

/// 创建测试用的 MCP Server 并返回其 ID
fn create_test_server(db: &MCPDatabase) -> i64 {
    db.upsert_mcp_server_with_builtin(
        "test-server",
        Some("Test Server Description"),
        "stdio",
        Some("node server.js"),
        None, // environment_variables
        None, // headers
        None, // url
        Some(30000),
        false,
        true,
        false,
        true,  // is_deletable
        false, // proxy_enabled
    )
    .unwrap()
}

// ============================================================================
// 正常情况测试
// ============================================================================

/// 测试 MCP Server 的完整 CRUD 生命周期
///
/// 验证内容：
/// - Create: 通过 upsert 创建 Server
/// - Read: 能够读取 Server 信息
/// - Update: 修改 Server 配置后持久化成功
/// - Delete: 删除 Server
#[test]
fn test_mcp_server_crud() {
    let db = create_mcp_db();

    // Create (via upsert)
    let id = db
        .upsert_mcp_server_with_builtin(
            "my-server",
            Some("My Server"),
            "stdio",
            Some("node index.js"),
            Some("KEY=value"),
            None, // headers
            None, // url
            Some(60000),
            false,
            true,
            false,
            true,  // is_deletable
            false, // proxy_enabled
        )
        .unwrap();
    assert!(id > 0);

    // Read single
    let server = db.get_mcp_server(id).unwrap();
    assert_eq!(server.name, "my-server");
    assert_eq!(server.description, "My Server");
    assert_eq!(server.transport_type, "stdio");
    assert_eq!(server.command, Some("node index.js".to_string()));
    assert_eq!(server.environment_variables, Some("KEY=value".to_string()));
    assert_eq!(server.timeout, Some(60000));
    assert!(server.is_enabled);
    assert!(!server.is_builtin);

    // Read list
    let servers = db.get_mcp_servers().unwrap();
    assert_eq!(servers.len(), 1);

    // Update
    db.update_mcp_server_with_builtin(
        id,
        "my-server-updated",
        Some("Updated Desc"),
        "sse",
        None, // command
        None, // environment_variables
        None, // headers
        Some("http://localhost:3000"),
        Some(90000),
        true,
        false,
        true,
        false, // proxy_enabled
    )
    .unwrap();

    let updated = db.get_mcp_server(id).unwrap();
    assert_eq!(updated.name, "my-server-updated");
    assert_eq!(updated.transport_type, "sse");
    assert_eq!(updated.url, Some("http://localhost:3000".to_string()));
    assert!(updated.is_long_running);
    assert!(!updated.is_enabled);
    assert!(updated.is_builtin);

    // Delete
    db.delete_mcp_server(id).unwrap();
    let servers_after = db.get_mcp_servers().unwrap();
    assert!(servers_after.is_empty());
}

/// 测试 MCP Server upsert 语义
///
/// 验证内容：
/// - 新建时插入
/// - 同名时更新
#[test]
fn test_mcp_server_upsert() {
    let db = create_mcp_db();

    // 第一次创建
    let id1 = db
        .upsert_mcp_server_with_builtin(
            "upsert-test",
            Some("Original"),
            "stdio",
            Some("cmd1"),
            None,
            None,
            None,
            None,
            false,
            true,
            false,
            true,  // is_deletable
            false, // proxy_enabled
        )
        .unwrap();

    // 第二次同名 upsert
    let id2 = db
        .upsert_mcp_server_with_builtin(
            "upsert-test",
            Some("Updated"),
            "sse",
            Some("cmd2"),
            None,
            None,
            None,
            None,
            false,
            false,
            true,
            true,  // is_deletable
            false, // proxy_enabled
        )
        .unwrap();

    // 应该是同一个 ID
    assert_eq!(id1, id2);

    // 验证更新生效
    let server = db.get_mcp_server(id1).unwrap();
    assert_eq!(server.description, "Updated");
    assert_eq!(server.transport_type, "sse");
    assert!(!server.is_enabled);
    assert!(server.is_builtin);
}

/// 测试 MCP Server Tool 操作
///
/// 验证内容：
/// - 为 Server 添加 Tool
/// - 获取 Server 的 Tool 列表
/// - 更新 Tool 的 is_enabled 和 is_auto_run
/// - upsert 更新已有 Tool
#[test]
fn test_mcp_server_tool_operations() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    // 添加 Tool
    let tool_id = db
        .upsert_mcp_server_tool(
            server_id,
            "search",
            Some("Search the web"),
            Some(r#"{"query": "string"}"#),
        )
        .unwrap();
    assert!(tool_id > 0);

    // 获取 Tools
    let tools = db.get_mcp_server_tools(server_id).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "search");
    assert_eq!(tools[0].tool_description, Some("Search the web".to_string()));
    assert!(tools[0].is_enabled);
    assert!(!tools[0].is_auto_run);

    // 更新 Tool 设置
    db.update_mcp_server_tool(tool_id, false, true).unwrap();
    let updated_tools = db.get_mcp_server_tools(server_id).unwrap();
    assert!(!updated_tools[0].is_enabled);
    assert!(updated_tools[0].is_auto_run);

    // Upsert 更新描述（保留用户设置）
    let tool_id2 = db
        .upsert_mcp_server_tool(
            server_id,
            "search",
            Some("Updated description"),
            Some(r#"{"query": "string", "limit": "number"}"#),
        )
        .unwrap();
    assert_eq!(tool_id, tool_id2);

    let final_tools = db.get_mcp_server_tools(server_id).unwrap();
    assert_eq!(final_tools[0].tool_description, Some("Updated description".to_string()));
}

/// 测试 MCP Server Resource 操作
///
/// 验证内容：
/// - 为 Server 添加 Resource
/// - 获取 Server 的 Resource 列表
/// - upsert 更新已有 Resource
#[test]
fn test_mcp_server_resource_operations() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    // 添加 Resource
    let resource_id = db
        .upsert_mcp_server_resource(
            server_id,
            "file:///home/user/docs",
            "User Docs",
            "directory",
            Some("User documents folder"),
        )
        .unwrap();
    assert!(resource_id > 0);

    // 获取 Resources
    let resources = db.get_mcp_server_resources(server_id).unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].resource_uri, "file:///home/user/docs");
    assert_eq!(resources[0].resource_name, "User Docs");
    assert_eq!(resources[0].resource_type, "directory");

    // Upsert 更新
    let resource_id2 = db
        .upsert_mcp_server_resource(
            server_id,
            "file:///home/user/docs",
            "Updated Name",
            "folder",
            Some("Updated desc"),
        )
        .unwrap();
    assert_eq!(resource_id, resource_id2);

    let updated = db.get_mcp_server_resources(server_id).unwrap();
    assert_eq!(updated[0].resource_name, "Updated Name");
    assert_eq!(updated[0].resource_type, "folder");
}

/// 测试 MCP Server Prompt 操作
///
/// 验证内容：
/// - 为 Server 添加 Prompt
/// - 获取 Server 的 Prompt 列表
/// - 更新 Prompt 的 is_enabled
/// - upsert 更新已有 Prompt
#[test]
fn test_mcp_server_prompt_operations() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    // 添加 Prompt
    let prompt_id = db
        .upsert_mcp_server_prompt(
            server_id,
            "summarize",
            Some("Summarize content"),
            Some(r#"{"text": "string"}"#),
        )
        .unwrap();
    assert!(prompt_id > 0);

    // 获取 Prompts
    let prompts = db.get_mcp_server_prompts(server_id).unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].prompt_name, "summarize");
    assert!(prompts[0].is_enabled);

    // 更新 is_enabled
    db.update_mcp_server_prompt(prompt_id, false).unwrap();
    let updated = db.get_mcp_server_prompts(server_id).unwrap();
    assert!(!updated[0].is_enabled);

    // Upsert 更新描述
    db.upsert_mcp_server_prompt(server_id, "summarize", Some("New description"), None).unwrap();
    let final_prompts = db.get_mcp_server_prompts(server_id).unwrap();
    assert_eq!(final_prompts[0].prompt_description, Some("New description".to_string()));
}

/// 测试 MCP Tool Call 基本操作
///
/// 验证内容：
/// - 创建 Tool Call 记录
/// - 读取 Tool Call
/// - 更新状态
#[test]
fn test_mcp_tool_call_operations() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    // 创建 Tool Call
    let tool_call = db
        .create_mcp_tool_call(1, Some(10), server_id, "test-server", "search", r#"{"q":"test"}"#)
        .unwrap();

    assert!(tool_call.id > 0);
    assert_eq!(tool_call.conversation_id, 1);
    assert_eq!(tool_call.message_id, Some(10));
    assert_eq!(tool_call.server_id, server_id);
    assert_eq!(tool_call.tool_name, "search");
    assert_eq!(tool_call.status, "pending");
    assert!(tool_call.result.is_none());
    assert!(tool_call.error.is_none());

    // 读取 Tool Call
    let read = db.get_mcp_tool_call(tool_call.id).unwrap();
    assert_eq!(read.tool_name, "search");

    // 更新为 executing
    db.update_mcp_tool_call_status(tool_call.id, "executing", None, None).unwrap();
    let executing = db.get_mcp_tool_call(tool_call.id).unwrap();
    assert_eq!(executing.status, "executing");
    assert!(executing.started_time.is_some());

    // 更新为 success
    db.update_mcp_tool_call_status(tool_call.id, "success", Some(r#"{"result": "found"}"#), None)
        .unwrap();
    let success = db.get_mcp_tool_call(tool_call.id).unwrap();
    assert_eq!(success.status, "success");
    assert_eq!(success.result, Some(r#"{"result": "found"}"#.to_string()));
    assert!(success.finished_time.is_some());
}

/// 测试带 LLM ID 的 Tool Call 创建
///
/// 验证内容：
/// - 创建带 llm_call_id 和 assistant_message_id 的 Tool Call
#[test]
fn test_mcp_tool_call_with_llm_id() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    let tool_call = db
        .create_mcp_tool_call_with_llm_id(
            1,
            Some(10),
            server_id,
            "test-server",
            "search",
            r#"{"q":"test"}"#,
            Some("call_abc123"),
            Some(100),
        )
        .unwrap();

    assert_eq!(tool_call.llm_call_id, Some("call_abc123".to_string()));
    assert_eq!(tool_call.assistant_message_id, Some(100));
}

/// 测试按 Conversation 获取 Tool Calls
///
/// 验证内容：
/// - 获取指定 conversation 的所有 Tool Calls
/// - 结果按创建时间降序排列
#[test]
fn test_mcp_tool_calls_by_conversation() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    // 创建多个 Tool Calls
    db.create_mcp_tool_call(1, None, server_id, "server", "tool1", "{}").unwrap();
    db.create_mcp_tool_call(1, None, server_id, "server", "tool2", "{}").unwrap();
    db.create_mcp_tool_call(2, None, server_id, "server", "tool3", "{}").unwrap();

    // 获取 conversation 1 的 Tool Calls
    let calls = db.get_mcp_tool_calls_by_conversation(1).unwrap();
    assert_eq!(calls.len(), 2);

    // 获取 conversation 2 的 Tool Calls
    let calls2 = db.get_mcp_tool_calls_by_conversation(2).unwrap();
    assert_eq!(calls2.len(), 1);
}

/// 测试 Server 的 toggle 操作
///
/// 验证内容：
/// - 切换 Server 的 is_enabled 状态
#[test]
fn test_mcp_server_toggle() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    // 初始状态
    let server = db.get_mcp_server(server_id).unwrap();
    assert!(server.is_enabled);

    // 禁用
    db.toggle_mcp_server(server_id, false).unwrap();
    let disabled = db.get_mcp_server(server_id).unwrap();
    assert!(!disabled.is_enabled);

    // 启用
    db.toggle_mcp_server(server_id, true).unwrap();
    let enabled = db.get_mcp_server(server_id).unwrap();
    assert!(enabled.is_enabled);
}

/// 测试 Tool Call 的原子状态转换
///
/// 验证内容：
/// - mark_mcp_tool_call_executing_if_pending 的原子性
/// - 只有 pending/failed 状态可以转换
#[test]
fn test_mcp_tool_call_atomic_transition() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    let tool_call = db.create_mcp_tool_call(1, None, server_id, "server", "tool", "{}").unwrap();

    // pending -> executing
    let transitioned = db.mark_mcp_tool_call_executing_if_pending(tool_call.id).unwrap();
    assert!(transitioned);

    // 再次调用不应该转换
    let transitioned2 = db.mark_mcp_tool_call_executing_if_pending(tool_call.id).unwrap();
    assert!(!transitioned2);

    // 更新为 failed
    db.update_mcp_tool_call_status(tool_call.id, "failed", None, Some("Error")).unwrap();

    // failed -> executing 应该成功
    let transitioned3 = db.mark_mcp_tool_call_executing_if_pending(tool_call.id).unwrap();
    assert!(transitioned3);
}

// ============================================================================
// 异常和边界情况测试
// ============================================================================

/// 测试读取不存在的 Server
///
/// 验证内容：
/// - 读取不存在的 ID 返回 QueryReturnedNoRows 错误
#[test]
fn test_mcp_server_read_nonexistent() {
    let db = create_mcp_db();

    let result = db.get_mcp_server(999);
    assert!(result.is_err());
    match result {
        Err(DbError::QueryReturnedNoRows) => {}
        _ => panic!("Expected QueryReturnedNoRows error"),
    }
}

/// 测试删除不存在的 Server
///
/// 验证内容：
/// - 删除不存在的 ID 不会产生错误
#[test]
fn test_mcp_server_delete_nonexistent() {
    let db = create_mcp_db();

    let result = db.delete_mcp_server(999);
    assert!(result.is_ok());
}

/// 测试读取不存在的 Tool Call
///
/// 验证内容：
/// - 读取不存在的 Tool Call ID 返回错误
#[test]
fn test_mcp_tool_call_read_nonexistent() {
    let db = create_mcp_db();

    let result = db.get_mcp_tool_call(999);
    assert!(result.is_err());
}

/// 测试获取不存在 Server 的 Tools
///
/// 验证内容：
/// - 查询不存在 server_id 的 Tool 返回空列表
#[test]
fn test_mcp_tools_nonexistent_server() {
    let db = create_mcp_db();

    let tools = db.get_mcp_server_tools(999).unwrap();
    assert!(tools.is_empty());
}

/// 测试获取不存在 Server 的 Resources
///
/// 验证内容：
/// - 查询不存在 server_id 的 Resource 返回空列表
#[test]
fn test_mcp_resources_nonexistent_server() {
    let db = create_mcp_db();

    let resources = db.get_mcp_server_resources(999).unwrap();
    assert!(resources.is_empty());
}

/// 测试获取不存在 Server 的 Prompts
///
/// 验证内容：
/// - 查询不存在 server_id 的 Prompt 返回空列表
#[test]
fn test_mcp_prompts_nonexistent_server() {
    let db = create_mcp_db();

    let prompts = db.get_mcp_server_prompts(999).unwrap();
    assert!(prompts.is_empty());
}

/// 测试获取不存在 Conversation 的 Tool Calls
///
/// 验证内容：
/// - 查询不存在 conversation_id 的 Tool Call 返回空列表
#[test]
fn test_mcp_tool_calls_nonexistent_conversation() {
    let db = create_mcp_db();

    let calls = db.get_mcp_tool_calls_by_conversation(999).unwrap();
    assert!(calls.is_empty());
}

/// 测试空名称的 Server（名称唯一约束）
///
/// 验证内容：
/// - 空名称仍可创建
/// - 但同名会触发 upsert
#[test]
fn test_mcp_server_empty_name() {
    let db = create_mcp_db();

    let id1 = db
        .upsert_mcp_server_with_builtin(
            "",
            Some("Empty Name"),
            "stdio",
            None,
            None,
            None,
            None,
            None,
            false,
            true,
            false,
            true,
            false,
        )
        .unwrap();

    // 同名 upsert
    let id2 = db
        .upsert_mcp_server_with_builtin(
            "",
            Some("Updated"),
            "sse",
            None,
            None,
            None,
            None,
            None,
            false,
            false,
            false,
            true,
            false,
        )
        .unwrap();

    assert_eq!(id1, id2);
}

/// 测试超长名称和描述
///
/// 验证内容：
/// - 超长文本可以正确存储
#[test]
fn test_mcp_very_long_text() {
    let db = create_mcp_db();

    let long_name = "S".repeat(10000);
    let long_desc = "D".repeat(10000);

    let id = db
        .upsert_mcp_server_with_builtin(
            &long_name,
            Some(&long_desc),
            "stdio",
            None,
            None,
            None,
            None,
            None,
            false,
            true,
            false,
            true,
            false,
        )
        .unwrap();

    let server = db.get_mcp_server(id).unwrap();
    assert_eq!(server.name.len(), 10000);
    assert_eq!(server.description.len(), 10000);
}

/// 测试特殊字符
///
/// 验证内容：
/// - 中文、Emoji 能正确存储
/// - SQL 注入字符被正确转义
#[test]
fn test_mcp_special_characters() {
    let db = create_mcp_db();

    // 中文和 Emoji
    let id = db
        .upsert_mcp_server_with_builtin(
            "搜索服务 🔍",
            Some("网络搜索 ✨"),
            "stdio",
            None,
            None,
            None,
            None,
            None,
            false,
            true,
            false,
            true,
            false,
        )
        .unwrap();

    let server = db.get_mcp_server(id).unwrap();
    assert_eq!(server.name, "搜索服务 🔍");
    assert_eq!(server.description, "网络搜索 ✨");

    // SQL 注入尝试
    let id2 = db
        .upsert_mcp_server_with_builtin(
            "'; DROP TABLE mcp_server; --",
            Some("Injection test"),
            "stdio",
            None,
            None,
            None,
            None,
            None,
            false,
            true,
            false,
            true,
            false,
        )
        .unwrap();
    assert!(id2 > 0);

    // 确保表还存在
    let servers = db.get_mcp_servers().unwrap();
    assert_eq!(servers.len(), 2);
}

/// 测试 Tool Call 状态转换失败情况
///
/// 验证内容：
/// - 从 success 状态无法通过 mark_executing_if_pending 转换
#[test]
fn test_mcp_tool_call_invalid_transition() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    let tool_call = db.create_mcp_tool_call(1, None, server_id, "server", "tool", "{}").unwrap();

    // pending -> executing -> success
    db.update_mcp_tool_call_status(tool_call.id, "executing", None, None).unwrap();
    db.update_mcp_tool_call_status(tool_call.id, "success", Some("Result"), None).unwrap();

    // success -> executing 不应该成功
    let transitioned = db.mark_mcp_tool_call_executing_if_pending(tool_call.id).unwrap();
    assert!(!transitioned);

    // 状态应该仍然是 success
    let current = db.get_mcp_tool_call(tool_call.id).unwrap();
    assert_eq!(current.status, "success");
}

/// 测试不同 transport_type 的 Server
///
/// 验证内容：
/// - stdio, sse, http, builtin 等不同类型可以正确存储
#[test]
fn test_mcp_server_transport_types() {
    let db = create_mcp_db();

    let types = ["stdio", "sse", "http", "builtin"];

    for transport in &types {
        let id = db
            .upsert_mcp_server_with_builtin(
                &format!("server-{}", transport),
                Some(&format!("{} server", transport)), // 提供描述，避免 NULL
                transport,
                None,
                None,
                None,
                None,
                None,
                false,
                true,
                *transport == "builtin",
                true,  // is_deletable
                false, // proxy_enabled
            )
            .unwrap();

        let server = db.get_mcp_server(id).unwrap();
        assert_eq!(server.transport_type, *transport);
        assert_eq!(server.is_builtin, *transport == "builtin");
    }

    let servers = db.get_mcp_servers().unwrap();
    assert_eq!(servers.len(), 4);
}

/// 测试 Tool Call 的 error 字段
///
/// 验证内容：
/// - failed 状态可以携带 error 信息
#[test]
fn test_mcp_tool_call_error_handling() {
    let db = create_mcp_db();
    let server_id = create_test_server(&db);

    let tool_call = db.create_mcp_tool_call(1, None, server_id, "server", "tool", "{}").unwrap();

    // 更新为 failed 并携带错误信息
    db.update_mcp_tool_call_status(
        tool_call.id,
        "failed",
        None,
        Some("Connection timeout after 30000ms"),
    )
    .unwrap();

    let failed = db.get_mcp_tool_call(tool_call.id).unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.result.is_none());
    assert_eq!(failed.error, Some("Connection timeout after 30000ms".to_string()));
    assert!(failed.finished_time.is_some());
}

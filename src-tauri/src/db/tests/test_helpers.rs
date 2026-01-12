//! 测试辅助函数和共享的数据库初始化代码
//!
//! ## 重要：测试数据隔离
//!
//! 本模块所有辅助函数使用 `Connection::open_in_memory()` 创建内存数据库，
//! **不会读写任何磁盘文件**，确保测试与项目真实数据完全隔离。
//!
//! SQLite 内存数据库特性：
//! - 每次调用 `open_in_memory()` 创建独立的数据库实例
//! - 测试结束后自动销毁，无需清理
//! - 完全隔离，不同测试之间互不影响

use crate::db::conversation_db::*;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

/// 创建内存测试数据库并初始化表结构
///
/// **安全性**: 使用 `Connection::open_in_memory()` 创建纯内存数据库，
/// 不会创建任何磁盘文件，不会影响项目真实的 db 文件。
pub fn create_test_db() -> Connection {
    // 使用内存数据库，不会创建任何磁盘文件
    let conn = Connection::open_in_memory().unwrap();

    // 禁用外键约束检查，简化测试
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();

    // 创建对话表
    conn.execute(
        "CREATE TABLE conversation (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            assistant_id INTEGER,
            created_time TEXT NOT NULL
        )",
        [],
    )
    .unwrap();

    // 创建消息表
    conn.execute(
        "CREATE TABLE message (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER,
            conversation_id INTEGER NOT NULL,
            message_type TEXT NOT NULL,
            content TEXT NOT NULL,
            llm_model_id INTEGER,
            llm_model_name TEXT,
            created_time TEXT NOT NULL,
            start_time TEXT,
            finish_time TEXT,
            token_count INTEGER DEFAULT 0,
            input_token_count INTEGER DEFAULT 0,
            output_token_count INTEGER DEFAULT 0,
            generation_group_id TEXT,
            parent_group_id TEXT,
            tool_calls_json TEXT,
            first_token_time TEXT,
            ttft_ms INTEGER
        )",
        [],
    )
    .unwrap();

    // 创建消息附件表
    conn.execute(
        "CREATE TABLE message_attachment (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            message_id INTEGER NOT NULL,
            attachment_type INTEGER NOT NULL,
            attachment_url TEXT,
            attachment_content TEXT,
            attachment_hash TEXT,
            use_vector BOOLEAN DEFAULT 0,
            token_count INTEGER
        )",
        [],
    )
    .unwrap();

    conn
}

/// 创建测试用的对话数据
pub fn create_test_conversation(repo: &ConversationRepository) -> Conversation {
    let conversation = Conversation {
        id: 0,
        name: "Test Conversation".to_string(),
        assistant_id: Some(1),
        created_time: Utc::now(),
    };
    repo.create(&conversation).unwrap()
}

/// 创建测试用的消息数据
pub fn create_test_message(
    conversation_id: i64,
    message_type: &str,
    content: &str,
    parent_id: Option<i64>,
    generation_group_id: Option<String>,
) -> Message {
    Message {
        id: 0,
        parent_id,
        conversation_id,
        message_type: message_type.to_string(),
        content: content.to_string(),
        llm_model_id: Some(1),
        llm_model_name: Some("test-model".to_string()),
        created_time: Utc::now(),
        start_time: None,
        finish_time: None,
        token_count: 100,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id,
        parent_group_id: None,
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    }
}

/// 创建共享的测试数据库连接，包含对话和消息表
pub fn create_shared_test_db() -> (Connection, ConversationRepository, MessageRepository, Conversation) {
    let conn = create_test_db();

    // 创建一个共享的测试数据库用于对话和消息
    let shared_conn = create_test_db();

    // 在同一个连接中创建对话
    shared_conn
        .execute(
            "INSERT INTO conversation (name, assistant_id, created_time) VALUES (?, ?, ?)",
            (&"Test Conversation", &Some(1i64), &Utc::now().to_rfc3339()),
        )
        .unwrap();
    let conversation_id = shared_conn.last_insert_rowid();

    let conversation = Conversation {
        id: conversation_id,
        name: "Test Conversation".to_string(),
        assistant_id: Some(1),
        created_time: Utc::now(),
    };

    let conv_repo = ConversationRepository::new(Connection::open_in_memory().unwrap());
    let shared_msg_repo = MessageRepository::new(shared_conn);

    (conn, conv_repo, shared_msg_repo, conversation)
}

/// 创建带有消息的测试数据库
pub fn create_message_test_db() -> (MessageRepository, i64) {
    let conn = create_test_db();

    // 创建对话
    conn.execute(
        "INSERT INTO conversation (name, assistant_id, created_time) VALUES (?, ?, ?)",
        (&"Test Conversation", &Some(1i64), &Utc::now().to_rfc3339()),
    )
    .unwrap();
    let conversation_id = conn.last_insert_rowid();

    let msg_repo = MessageRepository::new(conn);
    (msg_repo, conversation_id)
}

/// 创建新的 group_id
pub fn new_group_id() -> String {
    Uuid::new_v4().to_string()
}

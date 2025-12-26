//! 数据库层测试模块
//!
//! ## 测试文件命名规范
//! 测试文件名 = 源文件名 + `_tests.rs`
//! 例如：`conversation_db.rs` -> `conversation_db_tests.rs`
//!
//! ## 测试文件结构
//! - test_helpers.rs: 共享的测试辅助函数和数据库初始化
//! - conversation_db_tests.rs: Conversation CRUD 测试
//! - message_db_tests.rs: Message CRUD 和版本管理测试
//! - attachment_db_tests.rs: MessageAttachment CRUD 测试
//! - assistant_db_tests.rs: Assistant 及其关联表测试
//! - llm_db_tests.rs: LLM Provider 和 Model 测试
//! - mcp_db_tests.rs: MCP Server 和 Tool 测试
//! - sub_task_db_tests.rs: SubTask Definition 和 Execution 测试
//! - system_db_tests.rs: SystemConfig 和 FeatureConfig 测试
//! - plugin_db_tests.rs: Plugin, PluginStatus, PluginConfiguration, PluginData 测试
//!
//! ## 重要：测试隔离性
//! 所有测试使用 `Connection::open_in_memory()` 创建内存数据库，
//! 不会影响项目真实的 db 文件。

mod test_helpers;

mod conversation_db_tests;
mod message_db_tests;
mod attachment_db_tests;
mod assistant_db_tests;
mod llm_db_tests;
mod mcp_db_tests;
mod sub_task_db_tests;
mod system_db_tests;
mod plugin_db_tests;

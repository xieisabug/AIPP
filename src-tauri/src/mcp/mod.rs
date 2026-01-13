// Central MCP module: prompt building, detection, execution API, and builtin wrappers

pub mod builtin_mcp;
pub mod detection;
pub mod execution_api;
pub mod prompt;
pub mod registry_api;
pub mod util;

// Re-exports for convenience to minimize callsite churn
pub use detection::detect_and_process_mcp_calls;
pub use prompt::{collect_mcp_info_for_assistant, format_mcp_prompt, MCPInfoForAssistant};

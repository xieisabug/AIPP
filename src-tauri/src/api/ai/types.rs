use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// MCP配置覆盖
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct McpOverrideConfig {
    // 覆盖所有工具的自动运行配置（优先级高于tool_auto_run）
    pub all_tool_auto_run: Option<bool>,
    // 覆盖特定工具的自动运行配置
    pub tool_auto_run: Option<HashMap<String, bool>>, // "serverId/toolName" -> autoRun
    // 覆盖是否使用原生工具调用
    pub use_native_toolcall: Option<bool>,
    // 自定义MCP工具调用超时时间
    pub tool_call_timeout: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiRequest {
    pub conversation_id: String,
    pub assistant_id: i64,
    pub prompt: String,
    pub model: Option<String>,
    pub override_model_id: Option<String>, // 覆盖助手的默认模型ID
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: Option<bool>,
    pub attachment_list: Option<Vec<i64>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AiResponse {
    pub conversation_id: i64,
    pub request_prompt_result_with_context: String,
    /// 对话已创建（用户消息已落库），但请求在调用模型前就失败（如助手未配置模型）。
    /// 错误同时已作为 error 消息写入对话；前端据此立即切到该对话并结束「响应中」状态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

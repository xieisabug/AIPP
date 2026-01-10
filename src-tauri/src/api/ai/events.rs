use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEvent {
    pub r#type: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAddEvent {
    pub message_id: i64,
    pub message_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUpdateEvent {
    pub message_id: i64,
    pub message_type: String,
    pub content: String,
    pub is_done: bool,
    // Token 计数（可选，仅在 is_done=true 时有值）
    pub token_count: Option<i32>,
    pub input_token_count: Option<i32>,
    pub output_token_count: Option<i32>,
    // 性能指标（可选，仅在 is_done=true 时有值）
    pub ttft_ms: Option<i64>, // Time to First Token (毫秒)
    pub tps: Option<f64>,     // Tokens Per Second
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTypeEndEvent {
    pub message_id: i64,
    pub message_type: String,
    pub duration_ms: i64,
    pub end_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolCallUpdateEvent {
    pub call_id: i64,
    pub conversation_id: i64,
    pub status: String, // pending, executing, success, failed
    pub result: Option<String>,
    pub error: Option<String>,
    pub started_time: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationCancelEvent {
    pub conversation_id: i64,
    pub cancelled_at: chrono::DateTime<chrono::Utc>,
}

/// 错误通知事件的 payload 结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorNotificationPayload {
    pub conversation_id: Option<i64>,
    pub error_message: String,
}

pub const TITLE_CHANGE_EVENT: &str = "title_change";
pub const ERROR_NOTIFICATION_EVENT: &str = "conversation-window-error-notification";

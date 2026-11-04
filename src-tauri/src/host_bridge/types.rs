//! Host Bridge wire types (placeholders for the realtime control plane).
//! Full WS / relay / E2EE is deferred; see docs/AIPP-mobile-sync-hostbridge-设计方案.md.

use serde::{Deserialize, Serialize};

/// Capability advertised by a Host during handshake.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    AskAi,
    ConfirmPermission,
    ListConversations,
    /// Placeholder for future desktop-only tools (fs / MCP / ACP).
    DesktopTools,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostHello {
    pub device_id: String,
    pub device_name: String,
    pub capabilities: Vec<Capability>,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: String,
    pub ok: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostEvent {
    MessageUpdate { conversation_id: i64, message_id: i64 },
    PermissionRequest { request_id: String },
    HostDisconnected,
}

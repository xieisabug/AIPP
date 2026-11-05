//! Mobile execution checks shared by chat preflight and direct tool calls.
use serde::Serialize;

use crate::api::assistant_api::{get_assistant, get_assistant_mcp_servers_with_tools};
use crate::db::mcp_db::MCPDatabase;

#[derive(Debug, Clone, Serialize)]
pub struct MobileCapabilityIssue {
    pub code: &'static str,
    pub capability: String,
    pub reason: String,
}

impl MobileCapabilityIssue {
    pub fn error(&self, stage: &str, context: &str) -> String {
        format!("{}: capability={}; stage={}; {}; {}", self.code, self.capability, stage, self.reason, context)
    }
}

fn issue(capability: &str, reason: &str) -> MobileCapabilityIssue {
    MobileCapabilityIssue {
        code: "MOBILE_UNSUPPORTED",
        capability: capability.to_string(),
        reason: reason.to_string(),
    }
}

pub fn assistant_issue(assistant_type: Option<i64>) -> Option<MobileCapabilityIssue> {
    match assistant_type.unwrap_or(0) {
        0 => None,
        4 => Some(issue("local_cli_agent", "本地 Codex/Claude/ACP 需要桌面 CLI 进程，移动端不能启动")),
        _ => Some(issue("plugin_assistant", "该插件助手尚未通过移动执行能力核验")),
    }
}

/// Platform projection only: retain desktop configuration in storage.
pub fn hidden_mobile_tool(transport: &str, command: Option<&str>, tool: &str) -> bool {
    matches!(transport, "stdio" | "builtin")
        && command.map(str::trim) == Some("aipp:search")
        && tool == "search_web"
}

pub fn tool_issue(transport: &str, command: Option<&str>, tool: Option<&str>) -> Option<MobileCapabilityIssue> {
    // Match the executor's routing, not the editable is_builtin metadata flag.
    if matches!(transport, "http" | "sse") {
        return None;
    }
    let command = command.unwrap_or("").trim();
    if !matches!(transport, "stdio" | "builtin") || !matches!(command,
        "aipp:search" | "aipp:operation" | "aipp:artifact" | "aipp:dynamic_mcp"
        | "aipp:ui_interaction" | "aipp:agent" | "aipp:superadmin") {
        return Some(issue("external_stdio_mcp", "该工具没有已核验的进程内或 HTTP 执行实现；移动端不启动外部进程"));
    }
    match (command, tool) {
        ("aipp:search", Some("search_web")) => Some(issue("browser_search", "当前搜索依赖桌面 Chromium；HTTP fetch_url 不受此限制")),
        ("aipp:operation", Some("execute_bash" | "get_bash_output" | "kill_bash")) => Some(issue("script_execution", "该工具依赖桌面 Shell 进程")),
        ("aipp:artifact", Some("capture_artifact_screenshot")) => Some(issue("artifact_screenshot", "制品截图依赖桌面浏览器，移动端暂不支持")),
        _ => None,
    }
}

#[derive(Serialize)]
pub struct AssistantExecutionCapability {
    pub available: bool,
    pub issues: Vec<MobileCapabilityIssue>,
}

/// Query is read-only. Callers retain all configuration when execution is denied.
#[tauri::command]
pub async fn get_assistant_execution_capability(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<AssistantExecutionCapability, String> {
    let mut issues = Vec::new();
    if cfg!(mobile) {
        let detail = get_assistant(app_handle.clone(), assistant_id)?;
        if let Some(reason) = assistant_issue(detail.assistant.assistant_type) {
            issues.push(reason);
        }
        let configured = get_assistant_mcp_servers_with_tools(app_handle.clone(), assistant_id).await?;
        let db = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        for configured_server in configured.into_iter().filter(|server| server.is_enabled) {
            let server = db.get_mcp_server(configured_server.id).map_err(|e| format!("assistant_id={assistant_id}; server_id={}; capability preflight: {e}", configured_server.id))?;
            if let Some(mut reason) = tool_issue(&server.transport_type, server.command.as_deref(), None) {
                reason.reason = format!("server_id={}; {}", server.id, reason.reason);
                issues.push(reason);
                continue;
            }
            for tool in configured_server.tools.into_iter().filter(|tool| tool.is_enabled) {
                if hidden_mobile_tool(&server.transport_type, server.command.as_deref(), &tool.name) {
                    continue;
                }
                if let Some(mut reason) = tool_issue(&server.transport_type, server.command.as_deref(), Some(&tool.name)) {
                    reason.reason = format!("server_id={}; tool={}; {}", server.id, tool.name, reason.reason);
                    issues.push(reason);
                }
            }
        }
    }
    Ok(AssistantExecutionCapability { available: issues.is_empty(), issues })
}

pub async fn ensure_assistant_execution(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
    stage: &str,
    conversation_id: &str,
) -> Result<(), String> {
    if !cfg!(mobile) {
        return Ok(());
    }
    let result = get_assistant_execution_capability(app_handle.clone(), assistant_id).await
        .map_err(|error| format!("capability preflight failed; stage={stage}; assistant_id={assistant_id}; conversation_id={conversation_id}; {error}"))?;
    if result.available {
        return Ok(());
    }
    let context = format!("assistant_id={assistant_id}; conversation_id={}", if conversation_id.is_empty() { "not_created" } else { conversation_id });
    Err(result.issues.iter().map(|issue| issue.error(stage, &context)).collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
#[path = "mobile_capability_tests.rs"]
mod tests;

//! Conversation-scoped tools. Never persist these overrides in assistant MCP settings.
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use super::builtin_mcp::templates::{get_builtin_tools_for_command, is_butler_conversation_kind};
use super::prompt::{collect_mcp_info_for_assistant, MCPInfoForAssistant};

pub fn conversation_kind(app: &tauri::AppHandle, id: Option<i64>) -> Result<String, AppError> {
    let Some(id) = id else { return Ok(String::new()) };
    Ok(ConversationDatabase::new(app)?.conversation_repo()?.read(id)?
        .map(|c| c.conversation_kind).unwrap_or_default())
}

pub fn is_direct_tool(kind: &str, command: Option<&str>, tool: &str) -> bool {
    cfg!(desktop) && is_butler_conversation_kind(kind) && match command {
        Some("aipp:superadmin") => matches!(tool,"superadmin_catalog"|"superadmin_inspect"|"superadmin_execute"|"superadmin_batch"|"superadmin_undo"|"superadmin_audit_query"),
        Some("aipp:agent") => crate::api::butler_peer::is_peer_tool(tool),
        // 受托助手在自己的会话里向用户提问，问题绑定当前回合（即当前这项委托或任务）。
        Some("aipp:ui_interaction") => tool == "ask_user_question",
        _ => false,
    }
}

pub fn merge_tools(servers: &mut Vec<MCPServerWithTools>, required: Vec<MCPServerWithTools>, kind: &str) {
    if !is_butler_conversation_kind(kind) {
        servers.retain(|s| s.command.as_deref() != Some("aipp:superadmin"));
    }
    for server in servers.iter_mut() {
        if server.command.as_deref() == Some("aipp:agent") && !is_butler_conversation_kind(kind) {
            server.tools.retain(|t| !super::builtin_mcp::templates::is_butler_only_agent_tool(&t.name));
        }
    }
    if !cfg!(desktop) || !is_butler_conversation_kind(kind) { return; }
    for mut source in required {
        source.tools.retain(|t| is_direct_tool(kind, source.command.as_deref(), &t.name));
        if source.tools.is_empty() { continue; }
        source.is_enabled = true;
        for tool in &mut source.tools { tool.is_enabled = true; }
        source.tools.sort_by(|a,b| a.name.cmp(&b.name));
        if let Some(target) = servers.iter_mut().find(|s| s.id == source.id) {
            target.is_enabled = true;
            for tool in source.tools {
                target.tools.retain(|t| t.name != tool.name);
                target.tools.push(tool);
            }
            target.tools.sort_by(|a,b| a.name.cmp(&b.name));
        } else {
            servers.push(source);
        }
    }
    servers.sort_by_key(|s| s.id);
}

pub fn inject(app: &tauri::AppHandle, servers: &mut Vec<MCPServerWithTools>, kind: &str) -> Result<(), AppError> {
    let mut required = Vec::new();
    if cfg!(desktop) && is_butler_conversation_kind(kind) {
        let db = MCPDatabase::new(app)?;
        let stored = db.get_mcp_servers()?;
        for command in ["aipp:agent", "aipp:superadmin", "aipp:ui_interaction"] {
            let server = stored.iter().find(|s| s.is_builtin && s.command.as_deref() == Some(command))
                .ok_or_else(|| AppError::UnknownError(format!("管家工具注入失败：内置工具集 {command} 未初始化")))?;
            let definitions = get_builtin_tools_for_command(command);
            let tools = db.get_mcp_server_tools(server.id)?;
            let mut direct = Vec::new();
            for definition in definitions.iter().filter(|t| is_direct_tool(kind, Some(command), &t.name)) {
                let stored = tools.iter().find(|t| t.tool_name == definition.name)
                    .ok_or_else(|| AppError::UnknownError(format!("管家工具注入失败：{command}::{} 未初始化", definition.name)))?;
                direct.push(MCPToolInfo { id: stored.id, name: definition.name.clone(),
                    description: definition.description.clone(), parameters: definition.input_schema.to_string(),
                    is_enabled: true, is_auto_run: stored.is_auto_run });
            }
            required.push(MCPServerWithTools { id: server.id, name: server.name.clone(), summary: String::new(),
                command: server.command.clone(), is_enabled: true, tools: direct });
        }
    }
    merge_tools(servers, required, kind);
    Ok(())
}

pub async fn collect_mcp_info_for_conversation(
    app: &tauri::AppHandle, assistant_id: i64,
    overrides: Option<&crate::api::ai::types::McpOverrideConfig>,
    filters: Option<&Vec<String>>, conversation_id: Option<i64>,
) -> Result<MCPInfoForAssistant, AppError> {
    let mut info = collect_mcp_info_for_assistant(app, assistant_id, overrides, filters).await?;
    let kind = conversation_kind(app, conversation_id)?;
    inject(app, &mut info.enabled_servers, &kind)?;
    Ok(info)
}

#[cfg(test)]
#[path = "butler_tools_tests.rs"]
mod tests;

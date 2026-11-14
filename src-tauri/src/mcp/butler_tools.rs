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
        Some("aipp:superadmin") => is_superadmin_tool(tool),
        // 管家模式下所有助手（含群成员）与总管家一致：长期记忆、成员协作与定时任务都直接提供。
        // 群里读写的是本群记忆，看不到成员的私有记忆。
        Some("aipp:agent") => crate::api::butler_memory::is_tool(tool)
            || crate::api::butler_peer::is_peer_tool(tool)
            || tool == "schedule_task",
        // 受托助手在自己的会话里向用户提问，问题绑定当前回合（即当前这项委托或任务）。
        Some("aipp:ui_interaction") => tool == "ask_user_question",
        _ => false,
    }
}

fn is_superadmin_tool(tool: &str) -> bool {
    matches!(tool, "superadmin_catalog" | "superadmin_inspect" | "superadmin_execute" | "superadmin_batch" | "superadmin_undo" | "superadmin_audit_query")
}

pub fn merge_tools(servers: &mut Vec<MCPServerWithTools>, required: Vec<MCPServerWithTools>, kind: &str) {
    if !is_butler_conversation_kind(kind) {
        servers.retain(|s| s.command.as_deref() != Some("aipp:superadmin"));
    }
    for server in servers.iter_mut() {
        if server.command.as_deref() == Some("aipp:agent") && !is_butler_conversation_kind(kind) {
            server.tools.retain(|t| !super::builtin_mcp::templates::is_butler_only_agent_tool(&t.name));
        }
        if server.command.as_deref() == Some("aipp:agent") && is_butler_conversation_kind(kind) {
            server.tools.retain(|t| super::builtin_mcp::templates::is_agent_tool_visible_in_butler(kind, &t.name));
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

/// 全局启用的 MCP 服务与工具（总管家的基线能力）。内置的管家/超管工具集由直接注入负责，不在这里。
fn globally_enabled_servers(db: &MCPDatabase) -> Result<Vec<MCPServerWithTools>, AppError> {
    let mut result = Vec::new();
    for server in db.get_mcp_servers()? {
        if !server.is_enabled || matches!(server.command.as_deref(), Some("aipp:superadmin")) {
            continue;
        }
        let tools: Vec<MCPToolInfo> = db
            .get_mcp_server_tools(server.id)?
            .into_iter()
            .filter(|tool| tool.is_enabled)
            .map(|tool| MCPToolInfo {
                id: tool.id,
                name: tool.tool_name,
                description: tool.tool_description.unwrap_or_default(),
                parameters: tool.parameters.unwrap_or_else(|| "{}".to_string()),
                is_enabled: true,
                is_auto_run: tool.is_auto_run,
            })
            .collect();
        if tools.is_empty() {
            continue;
        }
        result.push(MCPServerWithTools {
            id: server.id,
            name: server.name,
            summary: server.description,
            command: server.command,
            is_enabled: true,
            tools,
        });
    }
    Ok(result)
}

/// 把全局启用的服务与工具补进助手自己的配置（不覆盖已有条目，不写回助手设置）。
fn add_baseline(servers: &mut Vec<MCPServerWithTools>, baseline: Vec<MCPServerWithTools>) {
    for source in baseline {
        match servers.iter_mut().find(|s| s.id == source.id) {
            Some(target) => {
                target.is_enabled = true;
                for tool in source.tools {
                    match target.tools.iter_mut().find(|t| t.name == tool.name) {
                        Some(existing) => existing.is_enabled = true,
                        None => target.tools.push(tool),
                    }
                }
            }
            None => servers.push(source),
        }
    }
}

pub fn inject(app: &tauri::AppHandle, servers: &mut Vec<MCPServerWithTools>, kind: &str) -> Result<(), AppError> {
    let mut required = Vec::new();
    if cfg!(desktop) && is_butler_conversation_kind(kind) {
        // 管家模式下所有助手与总管家一致：全局启用的 MCP 服务都可用。
        add_baseline(servers, globally_enabled_servers(&MCPDatabase::new(app)?)?);
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
    crate::vault::tool::inject(app, servers);
    Ok(())
}

/// 定时任务运行的工具：管家模式创建的任务使用管家基线（全局 MCP、SuperAdmin、定时任务）；
/// 其余按助手自己的配置。运行无人在场，去掉向用户提问；运行中不做成员协作、不写长期记忆、不派独立任务。
pub fn prepare_scheduled_run_tools(app: &tauri::AppHandle, servers: &mut Vec<MCPServerWithTools>, butler_mode: bool) -> Result<(), AppError> {
    let db = MCPDatabase::new(app)?;
    if butler_mode {
        add_baseline(servers, globally_enabled_servers(&db)?);
    }
    let stored = db.get_mcp_servers()?;
    let mut wanted: Vec<(&str, Vec<&str>)> = vec![("aipp:agent", vec!["schedule_task"])];
    if butler_mode && cfg!(desktop) {
        wanted.push(("aipp:superadmin", vec!["superadmin_catalog", "superadmin_inspect", "superadmin_execute", "superadmin_batch", "superadmin_undo", "superadmin_audit_query"]));
    }
    for (command, names) in wanted {
        let Some(server) = stored.iter().find(|s| s.is_builtin && s.command.as_deref() == Some(command)) else { continue };
        let tools = db.get_mcp_server_tools(server.id)?;
        let definitions = get_builtin_tools_for_command(command);
        let direct: Vec<MCPToolInfo> = definitions
            .iter()
            .filter(|d| names.contains(&d.name.as_str()))
            .filter_map(|d| {
                let stored = tools.iter().find(|t| t.tool_name == d.name)?;
                Some(MCPToolInfo {
                    id: stored.id,
                    name: d.name.clone(),
                    description: d.description.clone(),
                    parameters: d.input_schema.to_string(),
                    is_enabled: true,
                    is_auto_run: stored.is_auto_run,
                })
            })
            .collect();
        add_baseline(servers, vec![MCPServerWithTools {
            id: server.id,
            name: server.name.clone(),
            summary: String::new(),
            command: server.command.clone(),
            is_enabled: true,
            tools: direct,
        }]);
    }
    for server in servers.iter_mut() {
        match server.command.as_deref() {
            Some("aipp:ui_interaction") => server.tools.retain(|t| t.name != "ask_user_question"),
            Some("aipp:agent") => server.tools.retain(|t| {
                !super::builtin_mcp::templates::is_butler_main_only_agent_tool(&t.name)
                    && !crate::api::butler_peer::is_peer_tool(&t.name)
                    && !crate::api::butler_memory::is_tool(&t.name)
            }),
            Some("aipp:superadmin") if !butler_mode => server.tools.clear(),
            _ => {}
        }
    }
    crate::vault::tool::inject(app, servers);
    servers.retain(|s| !s.tools.is_empty());
    servers.sort_by_key(|s| s.id);
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

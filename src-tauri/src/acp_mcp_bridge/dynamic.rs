//! Resolve the native bridge's dynamic-call wrapper before entering transport dispatch.
use crate::db::mcp_db::{MCPDatabase, MCPServer};
use crate::mcp::assistant_access::Restrictions;

pub(super) struct Target {
    pub server: MCPServer,
    pub tool_name: String,
    pub parameters: String,
}

/// Resolve synchronously so the transport future owns its target, not a SQLite borrow.
pub(super) fn dispatch<F, Fut>(
    db: &MCPDatabase, source: MCPServer, tool_name: &str, parameters: &str,
    conversation: i64, access: &Restrictions, execute: F,
) -> impl std::future::Future<Output = Result<String,String>>
where F: FnOnce(Target) -> Fut, Fut: std::future::Future<Output = Result<String,String>> {
    let target = resolve_target(db,source,tool_name,parameters,conversation,access);
    async move { execute(target?).await }
}

pub(super) fn resolve_target(
    db: &MCPDatabase, mut source: MCPServer, tool_name: &str, parameters: &str,
    conversation: i64, access: &Restrictions,
) -> Result<Target,String> {
    if !access.server(source.id) { return Err(format!("助手已禁用 MCP 服务 #{}",source.id)); }
    if source.command.as_deref()==Some("aipp:agent") && matches!(tool_name,"load_mcp_server"|"load_mcp_tool"|"call_mcp_tool") {
        source = db.get_mcp_server(source.id).map_err(|e|format!("动态 MCP 入口已不存在：{e}"))?;
        let entry = db.get_mcp_server_tools(source.id).map_err(|e|e.to_string())?.into_iter()
            .find(|t|t.tool_name==tool_name).ok_or_else(||format!("动态 MCP 入口 {tool_name} 已不存在"))?;
        if source.command.as_deref()!=Some("aipp:agent") || !source.is_enabled || !entry.is_enabled || !access.tool(source.id,entry.id) {
            return Err(format!("动态 MCP 入口 {tool_name} 已停用或重新配置，旧会话不能继续使用"));
        }
    }
    if source.command.as_deref()!=Some("aipp:agent") || tool_name!="call_mcp_tool" {
        let tools = db.get_mcp_server_tools(source.id).map_err(|e|e.to_string())?;
        if tools.iter().any(|t|(t.tool_name==tool_name || crate::api::ai_api::sanitize_tool_name(&t.tool_name)==tool_name) && !access.tool(source.id,t.id)) {
            return Err(format!("助手已禁用工具 {tool_name}"));
        }
        return Ok(Target {server:source,tool_name:tool_name.into(),parameters:parameters.into()});
    }
    let call: serde_json::Map<String, serde_json::Value> = serde_json::from_str(parameters)
        .map_err(|_| "call_mcp_tool 参数校验失败，未执行。顶层参数必须是 JSON 对象")?;
    let required_name = |key: &str| {
        call.get(key).and_then(|value| value.as_str()).map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("call_mcp_tool 参数校验失败，未执行。{key} 必须是加载结果中的非空字符串"))
    };
    let server_name = required_name("server_name")?;
    let tool_name = required_name("tool_name")?;
    let parameters = call.get("parameters").filter(|value| value.is_object())
        .ok_or("call_mcp_tool 参数校验失败，未执行。请传 parameters 对象，无参数时传 {}")?
        .to_string();
    let servers = db.get_mcp_servers().map_err(|e|e.to_string())?;
    let exact = servers.iter().filter(|s|s.name==server_name).collect::<Vec<_>>();
    let matches = if exact.is_empty() {
        servers.iter().filter(|s|crate::api::ai_api::sanitize_tool_name(&s.name)==server_name).collect::<Vec<_>>()
    } else {exact};
    let server = match matches.as_slice() {
        [server] => (*server).clone(),
        [] => return Err(format!("MCP 服务 {server_name} 不存在，未调用任何工具")),
        _ => return Err(format!("MCP 服务名 {server_name} 不唯一，请使用加载结果中的准确名称")),
    };
    if !server.is_enabled || !access.server(server.id) { return Err(format!("MCP 服务 {server_name} 已禁用")); }
    if matches!(server.command.as_deref(),Some("aipp:agent"|"aipp:ui_interaction"|"aipp:superadmin")) {
        return Err("Agent、UI 和 SuperAdmin 工具须直接调用，不能通过 call_mcp_tool 嵌套执行".into());
    }
    let tools = db.get_mcp_server_tools(server.id).map_err(|e|e.to_string())?;
    let exact = tools.iter().filter(|t|t.tool_name==tool_name).collect::<Vec<_>>();
    let matches = if exact.is_empty() {
        tools.iter().filter(|t|crate::api::ai_api::sanitize_tool_name(&t.tool_name)==tool_name).collect::<Vec<_>>()
    } else {exact};
    let tool = match matches.as_slice() {
        [tool] => *tool,
        [] => return Err(format!("MCP 工具 {server_name}::{tool_name} 不存在")),
        _ => return Err(format!("MCP 工具名 {tool_name} 不唯一，请使用加载结果中的准确名称")),
    };
    if !tool.is_enabled || !access.tool(server.id,tool.id) { return Err(format!("MCP 工具 {server_name}::{} 已禁用",tool.tool_name)); }
    db.refresh_conversation_loaded_tool_statuses(conversation).map_err(|e|e.to_string())?;
    if !db.is_tool_loaded_for_conversation(conversation,server.id,&tool.tool_name).map_err(|e|e.to_string())? {
        return Err(format!("MCP 工具 {server_name}::{} 尚未加载或已失效，请在当前会话重新 load_mcp_tool",tool.tool_name));
    }
    Ok(Target {server,tool_name:tool.tool_name.clone(),parameters})
}

#[cfg(test)]
#[path = "dynamic_tests.rs"]
mod tests;

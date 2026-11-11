use crate::db::connection::{params, Connection, OptionalExtension};
use crate::db::mcp_db::{MCPDatabase, MCPServer};

pub fn is_managed(conn: &Connection, id: i64) -> Result<bool, String> {
    super::init(conn)?;
    conn.query_row("SELECT EXISTS(SELECT 1 FROM app_connection WHERE server_id=?1)", [id], |r| r.get(0)).map_err(|e| e.to_string())
}

/// Re-read the existing permissions at dispatch time. A cached tool list is not authorization.
pub fn validate(
    db: &MCPDatabase, assistants: &Connection, assistant_id: i64,
    snapshot: &MCPServer, tool_name: &str,
) -> Result<(), String> {
    let server = db.get_mcp_server(snapshot.id).map_err(|e| format!("应用连接已不存在：{e}"))?;
    let status: String = db.conn.query_row("SELECT status FROM app_connection WHERE server_id=?1", [server.id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !server.is_enabled || ["connecting", "auth_required", "disconnected"].contains(&status.as_str()) {
        return Err(format!("应用连接 {} 已停用、断开或需要重新授权", server.id));
    }
    if server.url != snapshot.url || server.headers != snapshot.headers || server.transport_type != snapshot.transport_type || server.command != snapshot.command || server.environment_variables != snapshot.environment_variables {
        return Err(format!("应用连接 {} 已重新配置，旧请求不可继续使用", server.id));
    }
    let allowed: bool = assistants.query_row("SELECT is_enabled FROM assistant_mcp_config WHERE assistant_id=?1 AND mcp_server_id=?2", params![assistant_id,server.id], |r| r.get(0))
        .optional().map_err(|e| e.to_string())?.unwrap_or(false);
    if !allowed { return Err(format!("助手 {assistant_id} 未获准使用应用连接 {}", server.id)); }
    let tools = db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())?;
    let tool = tools.iter().find(|t| t.tool_name == tool_name)
        .or_else(|| tools.iter().find(|t| crate::api::ai_api::sanitize_tool_name(&t.tool_name) == tool_name))
        .ok_or_else(|| format!("应用连接 {} 中找不到工具 {tool_name}", server.id))?;
    let enabled: bool = assistants.query_row("SELECT is_enabled FROM assistant_mcp_tool_config WHERE assistant_id=?1 AND mcp_tool_id=?2", params![assistant_id,tool.id], |r| r.get(0))
        .optional().map_err(|e| e.to_string())?.unwrap_or(true);
    if !tool.is_enabled || !enabled { return Err(format!("工具 {tool_name} 已停用或未向助手 {assistant_id} 开放")); }
    Ok(())
}

pub fn validate_execution(app: &tauri::AppHandle, server: &MCPServer, tool_name: &str, conversation_id: Option<i64>) -> Result<(), String> {
    let db = super::db(app)?;
    if !is_managed(&db.conn, server.id)? { return Ok(()); }
    let conversation_id = conversation_id.ok_or("应用工具调用缺少会话身份，无法校验助手授权")?;
    let conversations = crate::db::conversation_db::ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    let conn = conversations.get_connection().map_err(|e| e.to_string())?;
    let assistant_id: Option<i64> = conn.query_row("SELECT assistant_id FROM conversation WHERE id=?1", [conversation_id], |r| r.get(0)).map_err(|e| format!("读取会话 {conversation_id} 的助手失败：{e}"))?;
    let assistant_id = assistant_id.ok_or("应用工具调用的会话未绑定助手")?;
    let assistants = crate::db::assistant_db::AssistantDatabase::new(app).map_err(|e| e.to_string())?;
    validate(&db, &assistants.conn, assistant_id, server, tool_name)
}

pub fn validate_call(conn: &Connection, server_id: i64, call_id: i64) -> Result<(), String> {
    if !is_managed(conn, server_id)? { return Ok(()); }
    let current: String = conn.query_row("SELECT request_id FROM app_connection WHERE server_id=?1", [server_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    let captured: Option<String> = conn.query_row("SELECT request_id FROM app_tool_call_binding WHERE call_id=?1", [call_id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    if captured.as_deref() != Some(&current) { return Err("应用连接已重新配置，原工具请求及批准已失效，请重新发起调用".into()); }
    Ok(())
}

#[cfg(test)]
#[path = "access_tests.rs"]
mod access_tests;

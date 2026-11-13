//! Explicit assistant denials also apply to dynamic discovery and already loaded tools.
use std::collections::HashSet;
use crate::db::assistant_db::AssistantDatabase;

#[derive(Default)]
pub(crate) struct Restrictions {
    servers: HashSet<i64>,
    tools: HashSet<i64>,
}
impl Restrictions {
    pub(crate) fn load(db: &AssistantDatabase, id: i64) -> Result<Self,String> {
        Ok(Self {
            servers:db.get_assistant_mcp_configs(id).map_err(|e|e.to_string())?.into_iter().filter(|c|!c.is_enabled).map(|c|c.mcp_server_id).collect(),
            tools:db.get_assistant_mcp_tool_configs(id).map_err(|e|e.to_string())?.into_iter().filter(|c|!c.is_enabled).map(|c|c.mcp_tool_id).collect(),
        })
    }
    pub(crate) fn conversation(app: &tauri::AppHandle, id: Option<i64>) -> Result<Self,String> {
        let Some(id) = id else {return Ok(Self::default())};
        let db = crate::db::conversation_db::ConversationDatabase::new(app).map_err(|e|e.to_string())?;
        let conn = db.get_connection().map_err(|e|e.to_string())?;
        let (assistant,kind):(Option<i64>,String) = conn.query_row("SELECT assistant_id,conversation_kind FROM conversation WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|format!("读取会话 #{id} 权限失败：{e}"))?;
        if cfg!(desktop) && super::builtin_mcp::templates::is_butler_conversation_kind(&kind) { return Ok(Self::default()); }
        let Some(assistant) = assistant else {return Ok(Self::default())};
        Self::load(&AssistantDatabase::new(app).map_err(|e|e.to_string())?,assistant)
    }
    pub(crate) fn server(&self, id: i64) -> bool { !self.servers.contains(&id) }
    pub(crate) fn tool(&self, server: i64, tool: i64) -> bool {self.server(server) && !self.tools.contains(&tool)}
}

#[cfg(test)]
#[path = "assistant_access_tests.rs"]
mod tests;

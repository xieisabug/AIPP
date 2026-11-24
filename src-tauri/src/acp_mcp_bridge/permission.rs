//! Resolve a native permission against the exact tool registration for this prompt.
use std::collections::HashMap;
use serde_json::Value;
use sha2::{Digest, Sha256};
use super::{acp_loaded_tool_name, dynamic, parse_selected_mcp_tools};
use crate::{db::mcp_db::MCPDatabase, mcp::assistant_access::Restrictions};

#[derive(Debug)]
pub(super) struct Registration { server: i64, id: i64, name: String }
pub(super) type Catalog = HashMap<String, Registration>;

pub(super) fn catalog(payload: &str) -> Result<Catalog, String> {
    Ok(parse_selected_mcp_tools(payload)?.into_iter().map(|tool| (
        acp_loaded_tool_name(tool.tool_id, &tool.tool_name),
        Registration { server: tool.server_id, id: tool.tool_id, name: tool.tool_name },
    )).collect())
}

fn exempt(target: &dynamic::Target) -> bool {
    target.server.transport_type == "stdio" && crate::mcp::tool_review::is_review_exempt_tool(
        target.server.is_builtin, target.server.command.as_deref(), &target.tool_name, &target.parameters)
}

pub(super) fn exempt_target(db: &MCPDatabase, tool: &Registration, args: &Value, cid: i64, kind: &str, access: &Restrictions) -> Option<dynamic::Target> {
    if !args.is_object() || !access.tool(tool.server, tool.id) { return None; }
    let server = db.get_mcp_server(tool.server).ok()?;
    if !server.is_builtin || server.transport_type != "stdio" { return None; }
    // These operations are filtered out of every native bridge registration.
    if server.command.as_deref() == Some("aipp:operation") && super::is_native_operation_duplicate(&tool.name) { return None; }
    let current = db.get_mcp_server_tools(tool.server).ok()?.into_iter()
        .find(|t| t.id == tool.id && t.tool_name == tool.name)?;
    if !(server.is_enabled && current.is_enabled)
        && !crate::mcp::butler_tools::is_direct_tool(kind, server.command.as_deref(), &tool.name) { return None; }
    let target = dynamic::resolve_target(db, server, &tool.name, &args.to_string(), cid, access).ok()?;
    exempt(&target).then_some(target)
}

// A dynamic call may resolve differently if the catalog changes after the native
// permission reply. Bind that reply to the original target for this prompt.
#[derive(Default, Debug)]
pub(super) struct Exemptions(HashMap<(i64, String, [u8; 32]), (i64, Option<String>, String)>);

fn call_key(server: i64, name: &str, args: &Value) -> (i64, String, [u8; 32]) {
    let mut args = args.clone();
    args.sort_all_objects();
    (server, name.into(), Sha256::digest(args.to_string().as_bytes()).into())
}

impl Exemptions {
    pub(super) fn record(&mut self, tool: &Registration, args: &Value, target: &dynamic::Target) {
        self.0.insert(call_key(tool.server, &tool.name, args),
            (target.server.id, target.server.command.clone(), target.tool_name.clone()));
    }
    pub(super) fn check(&self, server: i64, name: &str, args: &Value, target: &dynamic::Target) -> Result<(), String> {
        if self.0.is_empty() { return Ok(()); }
        if let Some((id, command, tool)) = self.0.get(&call_key(server, name, args)) {
            if *id != target.server.id || command != &target.server.command || tool != &target.tool_name || !exempt(target) {
                return Err(format!("工具 {name} 在本轮免审核确认后发生变化，请重新发起调用"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;

pub(crate) fn codex_call<'a>(method: &str, params: &'a Value) -> Option<(&'a str, &'a Value)> {
    let meta = &params["_meta"];
    if method != "mcpServer/elicitation/request" || params["serverName"] != "aipp"
        || meta["codex_approval_kind"] != "mcp_tool_call" { return None; }
    Some((meta["tool_name"].as_str()?, meta.get("tool_params")?))
}

pub(crate) fn claude_call(request: &Value) -> Option<(&str, &Value)> {
    if request["subtype"] != "can_use_tool" { return None; }
    Some((request["tool_name"].as_str()?.strip_prefix("mcp__aipp__")?, request.get("input")?))
}

/// Only adapter-supplied MCP identity is authoritative. rawInput is the tool's
/// arguments and can contain arbitrary user/model fields, including tool names.
pub(crate) fn acp_call<'a>(meta: &'a serde_json::Map<String, Value>, input: &'a Value, server: &str) -> Option<(&'a str, &'a Value)> {
    if let Some(mcp) = meta.get("mcp") {
        if mcp["serverName"] != server { return None; }
        return Some((mcp["toolName"].as_str()?, input));
    }
    let name = meta.get("claudeCode")?.get("toolName")?.as_str()?;
    let prefix = format!("mcp__{}__", server.replace(' ', "_"));
    Some((name.strip_prefix(&prefix)?, input))
}

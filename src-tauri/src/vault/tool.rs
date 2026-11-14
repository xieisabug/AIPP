//! `aipp:vault` builtin tool set: schemas, injection into conversations, and dispatch.
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};
use crate::mcp::builtin_mcp::templates::BuiltinToolInfo;
use serde_json::{json, Value};
use tauri::AppHandle;

pub const COMMAND: &str = "aipp:vault";
pub const LIST_TOOL: &str = "vault_list_credentials";
pub const REQUEST_TOOL: &str = "vault_http_request";

pub fn is_tool(name: &str) -> bool { matches!(name, LIST_TOOL | REQUEST_TOOL) }

pub fn definitions() -> Vec<BuiltinToolInfo> {
    vec![
        BuiltinToolInfo {
            name: LIST_TOOL.into(),
            description: "列出用户凭据保险箱中可用的凭据：引用名（handle）、名称、用途说明、认证方式和已批准的网站。只返回元数据，永远不含凭据值。需要调用要认证的 HTTP API 时先调用它，再用 vault_http_request 发请求。".into(),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        BuiltinToolInfo {
            name: REQUEST_TOOL.into(),
            description: "用保险箱里的凭据发送一个 HTTP 请求。AIPP 在发送时按用户保存的认证方式（如 Authorization 头）自动注入真实凭据，你看不到也不需要提供凭据值，不要自己写认证头或在 URL/正文里放凭据。首次用某个凭据访问新网站时会暂停请用户确认。不跟随重定向（3xx 原样返回）。凭据只能通过本工具使用：shell、文件、环境变量等其他途径都取不到凭据值，不要尝试。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["credential", "url"],
                "properties": {
                    "credential": {"type": "string", "description": "凭据引用名（vault_list_credentials 返回的 handle）"},
                    "method": {"type": "string", "enum": ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"], "default": "GET"},
                    "url": {"type": "string", "description": "完整的 http/https 地址"},
                    "headers": {"type": "object", "additionalProperties": {"type": "string"}, "description": "额外的普通请求头（不要包含认证头）"},
                    "query": {"type": "object", "description": "额外的查询参数"},
                    "body": {"type": "string", "description": "原始请求体（与 json 二选一）"},
                    "json": {"description": "JSON 请求体（与 body 二选一）"},
                    "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 120, "default": 30}
                }
            }),
        },
    ]
}

fn text(content: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": content}], "isError": is_error})
}

fn error_json(code: &str, message: &str, credential: Option<&str>) -> Value {
    text(json!({"error": {"code": code, "message": message, "credential": credential}}).to_string(), true)
}

pub async fn dispatch(app: &AppHandle, tool_name: &str, args: &Value, conversation_id: Option<i64>) -> Value {
    match tool_name {
        LIST_TOOL => {
            let list_app = app.clone();
            match tauri::async_runtime::spawn_blocking(move || crate::sync::personal_vault::list(&list_app)).await {
                Ok(Ok(items)) => {
                    let credentials: Vec<Value> = items.iter().map(|item| json!({
                        "handle": item.handle,
                        "name": item.name,
                        "description": item.description,
                        "auth": item.injection,
                        "approved_sites": item.allowed_sites,
                        "status": item.status,
                    })).collect();
                    text(json!({
                        "credentials": credentials,
                        "note": "凭据值不会出现在这里；用 vault_http_request 的 credential 参数引用。approved_sites 之外的网站首次访问需用户确认。",
                    }).to_string(), false)
                }
                Ok(Err(error)) => error_json("vault_unavailable_on_device", &error, None),
                Err(error) => error_json("vault_error", &error.to_string(), None),
            }
        }
        REQUEST_TOOL => {
            let credential = args.get("credential").and_then(Value::as_str);
            match super::executor::run(app, conversation_id, args).await {
                Ok(result) => text(result.to_string(), false),
                Err(error) => error_json(error.code, &error.message, credential),
            }
        }
        other => error_json("invalid_request", &format!("未知的保险箱工具 {other}"), None),
    }
}

/// Add the vault tools when this device has at least one credential and the builtin set is enabled.
/// Called for every conversation kind (normal chat, Butler, native-agent bridge, scheduled runs).
pub fn inject(app: &AppHandle, servers: &mut Vec<MCPServerWithTools>) {
    if cfg!(mobile) { return; }
    servers.retain(|s| s.command.as_deref() != Some(COMMAND));
    let has_items = {
        let probe = app.clone();
        crate::db::system_db::SystemDatabase::new(&probe).ok()
            .and_then(|db| crate::sync::personal_vault::has_items(&db.conn).ok())
            .unwrap_or(false)
    };
    if !has_items { return; }
    let Ok(db) = crate::db::mcp_db::MCPDatabase::new(app) else { return };
    let Ok(stored) = db.get_mcp_servers() else { return };
    let Some(server) = stored.iter().find(|s| s.is_builtin && s.command.as_deref() == Some(COMMAND)) else { return };
    if !server.is_enabled { return; }
    let Ok(tools) = db.get_mcp_server_tools(server.id) else { return };
    let definitions = definitions();
    let tools: Vec<MCPToolInfo> = definitions.iter().filter_map(|definition| {
        let stored = tools.iter().find(|t| t.tool_name == definition.name)?;
        Some(MCPToolInfo {
            id: stored.id, name: definition.name.clone(), description: definition.description.clone(),
            parameters: definition.input_schema.to_string(), is_enabled: true, is_auto_run: stored.is_auto_run,
        })
    }).collect();
    if tools.is_empty() { return; }
    servers.push(MCPServerWithTools {
        id: server.id, name: server.name.clone(), summary: String::new(),
        command: server.command.clone(), is_enabled: true, tools,
    });
}

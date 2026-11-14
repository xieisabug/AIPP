//! `aipp:vault` builtin tool set: schemas, injection into conversations, and dispatch.
//!
//! Using a value is not a tool here: the assistant writes `<vault:key>` in any tool's arguments and
//! `placeholder::prepare` handles it. These tools list entries, ask the user to fill in new ones,
//! and send plain HTTP requests.
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};
use crate::mcp::builtin_mcp::templates::BuiltinToolInfo;
use serde_json::{json, Value};
use tauri::AppHandle;

pub const COMMAND: &str = "aipp:vault";
pub const LIST_TOOL: &str = "vault_list";
pub const INPUT_TOOL: &str = "vault_request_input";
pub const REQUEST_TOOL: &str = "vault_http_request";

pub fn is_tool(name: &str) -> bool { matches!(name, LIST_TOOL | INPUT_TOOL | REQUEST_TOOL) }

const USAGE: &str = "在任何 AIPP 工具的参数里写占位符 <vault:键>，例如 Authorization 请求头写 \"Bearer <vault:github_token>\"，表单字段写 \"<vault:id_card>\"。执行前 AIPP 会请用户确认这一次使用，然后替换成真实值；返回内容中出现的值会被遮盖。格式（前缀、位置）由你拼好。Shell 命令里把占位符放在双引号中。占位符只在 AIPP 提供的工具里生效，原生 CLI 自带的 Bash、WebFetch 不会替换。不要试图输出、转换或推断这些值。";

pub fn definitions() -> Vec<BuiltinToolInfo> {
    vec![
        BuiltinToolInfo {
            name: LIST_TOOL.into(),
            description: format!("列出用户凭据保险箱里的条目：键、名称、说明、占位符写法、自动允许的网站。只返回元数据，永远不含值。\n\n{USAGE}\n\n缺少某个值时，用 vault_request_input 请用户填写，不要让用户把值直接发在聊天里。"),
            input_schema: json!({"type": "object", "properties": {}}),
        },
        BuiltinToolInfo {
            name: INPUT_TOOL.into(),
            description: "请用户填写你需要、但保险箱里还没有的敏感信息（如身份证号、手机号、API Key）。聊天中会出现填写卡片，用户可以拒绝。填写的值直接存进保险箱，你只会拿到占位符 <vault:键>；用户刚填写的值在 10 分钟内第一次使用不需要再次确认。键已存在时直接返回占位符，不会打扰用户。不要让用户在聊天里直接发送这些值。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["items", "purpose"],
                "properties": {
                    "items": {
                        "type": "array", "minItems": 1, "maxItems": 5,
                        "items": {
                            "type": "object",
                            "required": ["key", "name"],
                            "properties": {
                                "key": {"type": "string", "pattern": "^[a-z][a-z0-9_]{1,39}$", "description": "占位符里使用的键，如 id_card"},
                                "name": {"type": "string", "description": "给用户看的名称，如 身份证号"},
                                "description": {"type": "string", "description": "可选：这个值的用途或格式说明"}
                            }
                        }
                    },
                    "purpose": {"type": "string", "description": "要用这些值做什么，会显示给用户"}
                }
            }),
        },
        BuiltinToolInfo {
            name: REQUEST_TOOL.into(),
            description: "发送一个 HTTP 请求。参数里任何位置都可以使用占位符 <vault:键>（URL 的协议、主机和端口除外）；执行前 AIPP 请用户确认并替换为真实值。URL 查询参数请放在 query 里（替换后由工具编码），HTTP Basic 认证用 basic_auth。不跟随重定向（3xx 原样返回），丢弃 Set-Cookie，响应中出现的值会被遮盖。".into(),
            input_schema: json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "method": {"type": "string", "enum": ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"], "default": "GET"},
                    "url": {"type": "string", "description": "完整的 http/https 地址"},
                    "headers": {"type": "object", "additionalProperties": {"type": "string"}, "description": "请求头，例如 {\"Authorization\": \"Bearer <vault:github_token>\"}"},
                    "query": {"type": "object", "description": "查询参数，替换后由工具编码"},
                    "body": {"type": "string", "description": "原始请求体（与 json 二选一）"},
                    "json": {"description": "JSON 请求体（与 body 二选一）"},
                    "basic_auth": {
                        "type": "object", "required": ["username"],
                        "properties": {"username": {"type": "string"}, "password": {"type": "string"}},
                        "description": "HTTP Basic 认证，例如 {\"username\": \"<vault:nas_user>\", \"password\": \"<vault:nas_password>\"}"
                    },
                    "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 120, "default": 30}
                }
            }),
        },
    ]
}

fn text(content: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": content}], "isError": is_error})
}

fn error_json(code: &str, message: &str) -> Value {
    text(json!({"error": {"code": code, "message": message}}).to_string(), true)
}

pub async fn dispatch(app: &AppHandle, tool_name: &str, args: &Value, conversation_id: Option<i64>) -> Value {
    match tool_name {
        LIST_TOOL => {
            let list_app = app.clone();
            match tauri::async_runtime::spawn_blocking(move || crate::sync::personal_vault::list(&list_app)).await {
                Ok(Ok(items)) => {
                    let entries: Vec<Value> = items.iter().map(|item| json!({
                        "key": item.key,
                        "name": item.name,
                        "description": item.description,
                        "placeholder": format!("<vault:{}>", item.key),
                        "auto_allowed_sites": item.allowed_sites,
                        "status": item.status,
                    })).collect();
                    text(json!({"entries": entries, "usage": USAGE}).to_string(), false)
                }
                Ok(Err(error)) => error_json("vault_unavailable_on_device", &error),
                Err(error) => error_json("vault_error", &error.to_string()),
            }
        }
        INPUT_TOOL => match super::input::request(app, conversation_id, args).await {
            Ok(result) => text(result.to_string(), false),
            Err(error) => error_json(error.code, &error.message),
        },
        REQUEST_TOOL => match super::executor::run(app, args).await {
            Ok(result) => text(result.to_string(), false),
            Err(error) => error_json(error.code, &error.message),
        },
        other => error_json("invalid_request", &format!("未知的保险箱工具 {other}")),
    }
}

/// Add the vault tools when this device supports the vault and the builtin set is enabled. Entries
/// are not required: the assistant can ask the user to fill them in. Called for every conversation
/// kind (normal chat, Butler, native-agent bridge, scheduled runs).
pub fn inject(app: &AppHandle, servers: &mut Vec<MCPServerWithTools>) {
    if cfg!(mobile) { return; }
    servers.retain(|s| s.command.as_deref() != Some(COMMAND));
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

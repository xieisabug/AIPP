use crate::db::connection::Connection;
use crate::db::mcp_db::MCPDatabase;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::Manager;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub const ACP_MCP_BRIDGE_ARG: &str = "--aipp-acp-mcp-bridge";
pub const ACP_MCP_DB_PATH_ENV: &str = "AIPP_ACP_MCP_DB_PATH";
pub const ACP_MCP_CONVERSATION_ID_ENV: &str = "AIPP_ACP_CONVERSATION_ID";
pub const ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV: &str = "AIPP_ACP_NATIVE_DUPLICATE_FILTER";
pub const ACP_MCP_PROXY_ADDR_ENV: &str = "AIPP_ACP_MCP_PROXY_ADDR";
pub const ACP_MCP_PROXY_TOKEN_ENV: &str = "AIPP_ACP_MCP_PROXY_TOKEN";
pub const ACP_MCP_SELECTED_TOOLS_ENV: &str = "AIPP_ACP_SELECTED_MCP_TOOLS";

#[derive(Debug, Clone)]
pub struct AcpMcpProxyConfig {
    pub addr: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AcpSelectedMcpServer {
    #[serde(default)]
    server_id: i64,
    #[serde(default)]
    server_name: String,
    #[serde(default = "default_true")]
    is_enabled: bool,
    #[serde(default)]
    tools: Vec<AcpSelectedMcpTool>,
}

#[derive(Debug, Clone, Deserialize)]
struct AcpSelectedMcpTool {
    #[serde(default)]
    tool_id: i64,
    #[serde(default)]
    server_id: i64,
    #[serde(default)]
    server_name: String,
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    tool_description: String,
    #[serde(default = "default_tool_parameters")]
    parameters: String,
    #[serde(default = "default_true")]
    is_enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_tool_parameters() -> String {
    "{}".to_string()
}

static ACP_MCP_PROXY_CONFIG: OnceLock<AcpMcpProxyConfig> = OnceLock::new();

pub async fn ensure_proxy_server(
    app_handle: tauri::AppHandle,
) -> Result<AcpMcpProxyConfig, String> {
    if let Some(config) = ACP_MCP_PROXY_CONFIG.get() {
        return Ok(config.clone());
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("Failed to bind ACP MCP proxy server: {error}"))?;
    let addr = listener
        .local_addr()
        .map_err(|error| format!("Failed to read ACP MCP proxy address: {error}"))?
        .to_string();
    let config = AcpMcpProxyConfig {
        addr,
        token: uuid::Uuid::new_v4().to_string(),
    };

    if ACP_MCP_PROXY_CONFIG.set(config.clone()).is_err() {
        return ACP_MCP_PROXY_CONFIG
            .get()
            .cloned()
            .ok_or_else(|| "ACP MCP proxy server initialization raced without config".to_string());
    }
    let active_config = ACP_MCP_PROXY_CONFIG
        .get()
        .cloned()
        .unwrap_or_else(|| config.clone());
    let proxy_config = active_config.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let app_handle = app_handle.clone();
            let proxy_config = proxy_config.clone();
            tauri::async_runtime::spawn(async move {
                let _ = handle_proxy_stream(app_handle, proxy_config, stream).await;
            });
        }
    });

    Ok(active_config)
}

async fn handle_proxy_stream(
    app_handle: tauri::AppHandle,
    proxy_config: AcpMcpProxyConfig,
    stream: tokio::net::TcpStream,
) -> Result<(), String> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = tokio::io::BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
        if line.trim().is_empty() {
            continue;
        }
        let request: serde_json::Value =
            serde_json::from_str(&line).map_err(|error| error.to_string())?;
        let response = match execute_proxy_tool(&app_handle, &proxy_config, request).await {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
        let line = serde_json::to_string(&response).map_err(|error| error.to_string())?;
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        writer.write_all(b"\n").await.map_err(|error| error.to_string())?;
        writer.flush().await.map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn execute_proxy_tool(
    app_handle: &tauri::AppHandle,
    proxy_config: &AcpMcpProxyConfig,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let token = request
        .get("token")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "ACP MCP proxy request missing token".to_string())?;
    if token != proxy_config.token {
        return Err("ACP MCP proxy token mismatch".to_string());
    }

    let conversation_id = request
        .get("conversation_id")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| "ACP MCP proxy request missing conversation_id".to_string())?;
    let server_id = request
        .get("server_id")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| "ACP MCP proxy request missing server_id".to_string())?;
    let tool_name = request
        .get("tool_name")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "ACP MCP proxy request missing tool_name".to_string())?;
    let arguments = request
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let db = MCPDatabase::new(app_handle).map_err(|error| error.to_string())?;
    let server = db.get_mcp_server(server_id).map_err(|error| error.to_string())?;
    let parameters = serde_json::to_string(&arguments).map_err(|error| error.to_string())?;
    let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
    let result = crate::mcp::execution_api::execute_tool_by_transport(
        app_handle,
        &feature_config_state,
        &server,
        tool_name,
        &parameters,
        Some(conversation_id),
        None,
    )
    .await?;

    Ok(json!({
        "content": parse_tool_content(&result),
        "isError": false,
    }))
}

pub fn run_if_requested() -> bool {
    if !std::env::args().any(|arg| arg == ACP_MCP_BRIDGE_ARG) {
        return false;
    }

    if let Err(error) = run_bridge() {
        eprintln!("AIPP ACP MCP bridge failed: {error}");
    }
    true
}

fn run_bridge() -> Result<(), String> {
    let db_path = std::env::var(ACP_MCP_DB_PATH_ENV)
        .map(PathBuf::from)
        .map_err(|_| format!("{ACP_MCP_DB_PATH_ENV} is required"))?;
    let conversation_id = std::env::var(ACP_MCP_CONVERSATION_ID_ENV)
        .map_err(|_| format!("{ACP_MCP_CONVERSATION_ID_ENV} is required"))?
        .parse::<i64>()
        .map_err(|error| format!("Invalid {ACP_MCP_CONVERSATION_ID_ENV}: {error}"))?;
    let filter_acp_native_duplicates = std::env::var(ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV)
        .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    let proxy_addr = std::env::var(ACP_MCP_PROXY_ADDR_ENV).ok();
    let proxy_token = std::env::var(ACP_MCP_PROXY_TOKEN_ENV).ok();
    let selected_tools_payload =
        std::env::var(ACP_MCP_SELECTED_TOOLS_ENV).unwrap_or_else(|_| "[]".to_string());
    let selected_tools = parse_selected_mcp_tools(&selected_tools_payload)?;

    let db = open_mcp_db(db_path)?;
    let mut bridge = AcpManualMcpBridge {
        db,
        conversation_id,
        filter_acp_native_duplicates,
        proxy_addr,
        proxy_token,
        selected_tools,
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        let request: serde_json::Value =
            serde_json::from_str(&line).map_err(|error| format!("Invalid JSON-RPC: {error}"))?;
        for response in bridge.handle_json_rpc(request) {
            serde_json::to_writer(&mut stdout, &response).map_err(|error| error.to_string())?;
            stdout.write_all(b"\n").map_err(|error| error.to_string())?;
            stdout.flush().map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

fn open_mcp_db(db_path: PathBuf) -> Result<MCPDatabase, String> {
    let conn = Connection::open(db_path).map_err(|error| error.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;",
    )
    .map_err(|error| error.to_string())?;
    Ok(MCPDatabase { conn })
}

fn parse_selected_mcp_tools(raw: &str) -> Result<Vec<AcpSelectedMcpTool>, String> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    let servers: Vec<AcpSelectedMcpServer> =
        serde_json::from_str(raw).map_err(|error| format!("Invalid selected MCP tool payload: {error}"))?;
    let mut tools = Vec::new();
    for server in servers.into_iter().filter(|server| server.is_enabled) {
        for mut tool in server.tools.into_iter().filter(|tool| tool.is_enabled) {
            if tool.server_id == 0 {
                tool.server_id = server.server_id;
            }
            if tool.server_name.is_empty() {
                tool.server_name = server.server_name.clone();
            }
            if tool.tool_name.trim().is_empty() || tool.tool_id <= 0 || tool.server_id <= 0 {
                continue;
            }
            if matches!(tool.tool_name.as_str(), "load_mcp_server" | "load_mcp_tool") {
                continue;
            }
            tools.push(tool);
        }
    }
    tools.sort_by(|left, right| {
        left.tool_id
            .cmp(&right.tool_id)
            .then_with(|| left.server_name.cmp(&right.server_name))
            .then_with(|| left.tool_name.cmp(&right.tool_name))
    });
    tools.dedup_by_key(|tool| tool.tool_id);
    Ok(tools)
}

struct AcpManualMcpBridge {
    db: MCPDatabase,
    conversation_id: i64,
    filter_acp_native_duplicates: bool,
    proxy_addr: Option<String>,
    proxy_token: Option<String>,
    selected_tools: Vec<AcpSelectedMcpTool>,
}

impl AcpManualMcpBridge {
    fn handle_json_rpc(&mut self, request: serde_json::Value) -> Vec<serde_json::Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|value| value.as_str()).unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        let Some(id) = id else {
            return Vec::new();
        };

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "AIPP MCP Tools",
                    "version": "0.1.0"
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => self.list_tools(),
            "tools/call" => self.handle_tool_call(params),
            _ => Err(format!("Unsupported MCP method: {method}")),
        };

        vec![match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32603,
                    "message": error
                }
            }),
        }]
    }

    fn handle_tool_call(&mut self, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let tool_name = params
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "tools/call missing params.name".to_string())?;
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

        let payload = self.call_loaded_tool(tool_name, args)?;

        Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
                }
            ],
            "isError": false
        }))
    }

    fn list_tools(&mut self) -> Result<serde_json::Value, String> {
        let tools = self
            .loaded_mcp_tools()?
            .iter()
            .map(selected_tool_to_mcp_tool)
            .collect::<Vec<_>>();
        Ok(json!({ "tools": tools }))
    }

    fn call_loaded_tool(
        &mut self,
        acp_tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let tool = self
            .loaded_mcp_tools()?
            .into_iter()
            .find(|tool| acp_loaded_tool_name(tool.tool_id, &tool.tool_name) == acp_tool_name)
            .ok_or_else(|| format!("Unknown AIPP loaded MCP tool: {acp_tool_name}"))?;
        let proxy_addr = self
            .proxy_addr
            .as_deref()
            .ok_or_else(|| "AIPP ACP MCP proxy address is not configured".to_string())?;
        let proxy_token = self
            .proxy_token
            .as_deref()
            .ok_or_else(|| "AIPP ACP MCP proxy token is not configured".to_string())?;

        call_proxy_tool(
            proxy_addr,
            proxy_token,
            self.conversation_id,
            tool.server_id,
            &tool.tool_name,
            args,
        )
    }

    fn loaded_mcp_tools(&mut self) -> Result<Vec<AcpSelectedMcpTool>, String> {
        let operation_server_ids = self.acp_native_operation_server_ids()?;
        let mut tools = self
            .selected_tools
            .iter()
            .cloned()
            .into_iter()
            .filter(|tool| {
                !self.is_acp_native_duplicate_tool(
                    &operation_server_ids,
                    tool.server_id,
                    &tool.tool_name,
                )
            })
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| {
            left.tool_id
                .cmp(&right.tool_id)
                .then_with(|| left.server_name.cmp(&right.server_name))
                .then_with(|| left.tool_name.cmp(&right.tool_name))
        });
        Ok(tools)
    }

    fn acp_native_operation_server_ids(&self) -> Result<HashSet<i64>, String> {
        if !self.filter_acp_native_duplicates {
            return Ok(HashSet::new());
        }

        let servers = self
            .db
            .get_mcp_servers()
            .map_err(|error| format!("Failed to load ACP duplicate tool filter: {error}"))?;
        Ok(servers
            .into_iter()
            .filter(|server| {
                server.is_builtin && server.command.as_deref() == Some("aipp:operation")
            })
            .map(|server| server.id)
            .collect())
    }

    fn is_acp_native_duplicate_tool(
        &self,
        operation_server_ids: &HashSet<i64>,
        server_id: i64,
        tool_name: &str,
    ) -> bool {
        operation_server_ids.contains(&server_id)
            && matches!(
                tool_name,
                "read_file" | "write_file" | "execute_bash" | "get_bash_output"
            )
    }
}

fn call_proxy_tool(
    proxy_addr: &str,
    proxy_token: &str,
    conversation_id: i64,
    server_id: i64,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut stream = TcpStream::connect(proxy_addr)
        .map_err(|error| format!("Failed to connect AIPP MCP proxy: {error}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(600)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|error| error.to_string())?;

    let request = json!({
        "token": proxy_token,
        "conversation_id": conversation_id,
        "server_id": server_id,
        "tool_name": tool_name,
        "arguments": arguments,
    });
    serde_json::to_writer(&mut stream, &request).map_err(|error| error.to_string())?;
    stream.write_all(b"\n").map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|error| error.to_string())?;
    let response: serde_json::Value =
        serde_json::from_str(&line).map_err(|error| error.to_string())?;
    if response.get("ok").and_then(|value| value.as_bool()) == Some(true) {
        Ok(response.get("result").cloned().unwrap_or_else(|| json!({})))
    } else {
        Err(response
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("AIPP MCP proxy request failed")
            .to_string())
    }
}

fn parse_tool_content(raw: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Array(items)) if items.is_empty() => json!([{
            "type": "text",
            "text": "Tool completed successfully with no output."
        }]),
        Ok(value) if value.is_array() => value,
        Ok(value) => json!([{ "type": "json", "json": value }]),
        Err(_) if raw.trim().is_empty() => json!([{
            "type": "text",
            "text": "Tool completed successfully with no output."
        }]),
        Err(_) => json!([{ "type": "text", "text": raw }]),
    }
}

fn selected_tool_to_mcp_tool(tool: &AcpSelectedMcpTool) -> serde_json::Value {
    let parameters =
        serde_json::from_str(&tool.parameters).unwrap_or_else(|_| json!({ "type": "object" }));
    let description = if tool.tool_description.trim().is_empty() {
        format!("AIPP MCP tool from server '{}'.", tool.server_name)
    } else {
        format!("{} (AIPP MCP server: {})", tool.tool_description, tool.server_name)
    };
    json!({
        "name": acp_loaded_tool_name(tool.tool_id, &tool.tool_name),
        "description": description,
        "inputSchema": parameters,
    })
}

fn acp_loaded_tool_name(tool_id: i64, tool_name: &str) -> String {
    format!("aipp_t{}_{}", tool_id, sanitize_tool_name(tool_name))
}

fn sanitize_tool_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selected_mcp_tools_flattens_enabled_manual_selection() {
        let tools = parse_selected_mcp_tools(
            r#"[
                {
                    "server_id": 7,
                    "server_name": "Search",
                    "tools": [
                        {
                            "tool_id": 11,
                            "tool_name": "fetch_url",
                            "tool_description": "Fetch URL",
                            "parameters": "{\"type\":\"object\"}"
                        },
                        {
                            "tool_id": 12,
                            "tool_name": "disabled_tool",
                            "is_enabled": false
                        }
                    ]
                }
            ]"#,
        )
        .unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_id, 11);
        assert_eq!(tools[0].server_id, 7);
        assert_eq!(tools[0].server_name, "Search");
        assert_eq!(
            selected_tool_to_mcp_tool(&tools[0])["name"].as_str(),
            Some("aipp_t11_fetch_url")
        );
    }

    #[test]
    fn parse_tool_content_replaces_empty_results_with_text() {
        assert_eq!(
            parse_tool_content("[]"),
            json!([{
                "type": "text",
                "text": "Tool completed successfully with no output."
            }])
        );
        assert_eq!(
            parse_tool_content("   "),
            json!([{
                "type": "text",
                "text": "Tool completed successfully with no output."
            }])
        );
    }
}

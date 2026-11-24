pub(crate) mod provenance;
pub(crate) mod permission;
mod dynamic;

use crate::db::connection::Connection;
use crate::db::mcp_db::MCPDatabase;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub const ACP_MCP_BRIDGE_ARG: &str = "--aipp-acp-mcp-bridge";
pub const ACP_MCP_DB_PATH_ENV: &str = "AIPP_ACP_MCP_DB_PATH";
pub const ACP_MCP_CONVERSATION_ID_ENV: &str = "AIPP_ACP_CONVERSATION_ID";
pub const ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV: &str = "AIPP_ACP_NATIVE_DUPLICATE_FILTER";
pub const ACP_MCP_PROXY_ADDR_ENV: &str = "AIPP_ACP_MCP_PROXY_ADDR";
pub const ACP_MCP_PROXY_TOKEN_ENV: &str = "AIPP_ACP_MCP_PROXY_TOKEN";
pub const ACP_MCP_SELECTED_TOOLS_ENV: &str = "AIPP_ACP_SELECTED_MCP_TOOLS";
pub const ACP_MCP_SELECTED_TOOLS_FILE_ENV: &str = "AIPP_ACP_SELECTED_MCP_TOOLS_FILE";

#[derive(Debug)]
pub(crate) struct SelectedMcpToolsFile {
    path: PathBuf,
}

impl SelectedMcpToolsFile {
    pub(crate) fn write(content: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "aipp-mcp-selected-tools-{}.json",
            uuid::Uuid::new_v4()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|error| format!("Failed to create AIPP MCP selected tools file: {error}"))?;
        let guard = Self { path };
        file.write_all(content.as_bytes())
            .map_err(|error| format!("Failed to write AIPP MCP selected tools file: {error}"))?;
        file.flush()
            .map_err(|error| format!("Failed to flush AIPP MCP selected tools file: {error}"))?;
        Ok(guard)
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for SelectedMcpToolsFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn load_selected_mcp_tools_payload(
    file_path: Option<&str>,
    inline: Option<&str>,
) -> Result<String, String> {
    if let Some(path) = file_path.map(str::trim).filter(|path| !path.is_empty()) {
        return std::fs::read_to_string(path)
            .map_err(|error| format!("Failed to read {ACP_MCP_SELECTED_TOOLS_FILE_ENV} ({path}): {error}"));
    }
    Ok(inline.unwrap_or("[]").to_string())
}

#[derive(Clone)]
pub struct AcpMcpProxyConfig {
    pub addr: String,
    pub token: String,
}

impl std::fmt::Debug for AcpMcpProxyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpMcpProxyConfig").field("addr", &self.addr).field("token", &"[REDACTED]").finish()
    }
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
    #[serde(default)]
    dynamic_loading_enabled: bool,
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
    _proxy_config: &AcpMcpProxyConfig,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let token = request
        .get("token")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "ACP MCP proxy request missing token".to_string())?;
    let conversation_id = request
        .get("conversation_id")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| "ACP MCP proxy request missing conversation_id".to_string())?;
    // Resolve the runtime-owned lease before any asynchronous work. Source IDs
    // supplied in request JSON are intentionally ignored.
    let prompt = provenance::capture(token, conversation_id)?;
    if let Some(name) = request.get("review_registered_tool") {
        let exempt = name.as_str().zip(request.get("arguments"))
            .is_some_and(|call| prompt.review_exempt(app_handle, call));
        return Ok(json!({"exempt": exempt}));
    }
    let cancel = prompt.cancellation();
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
    let access = crate::mcp::assistant_access::Restrictions::conversation(app_handle,Some(conversation_id))?;
    {
        let db = crate::db::conversation_db::ConversationDatabase::new(app_handle).map_err(|error| error.to_string())?;
        let conn = db.get_connection().map_err(|error| error.to_string())?;
        prompt.validate_connection(&conn)?;
    }
    let is_proactive = crate::mcp::butler_tools::conversation_kind(app_handle, Some(conversation_id))
        .map(|k| k == crate::proactive::tools::PROACTIVE_RUN_KIND)
        .unwrap_or(false);
    prompt.ensure_active()?;
    let exempt_prompt = prompt.clone();
    let execute = dynamic::dispatch(&db, server, tool_name, &parameters, conversation_id, &access, |target| async move {
        exempt_prompt.check_exempt_target(server_id, tool_name, &arguments, &target)?;
        let audited_call_id = if is_proactive {
            crate::proactive::gate::admit_proxy_tool(
                app_handle,
                conversation_id,
                &target.server,
                &target.tool_name,
                &target.parameters,
            )
            .await?
        } else {
            None
        };
        let out = crate::mcp::execution_api::execute_tool_by_transport_on_large_stack(
            app_handle,
            &target.server,
            &target.tool_name,
            &target.parameters,
            Some(conversation_id),
            Some(cancel),
        )
        .await;
        if let Some(call_id) = audited_call_id {
            if let Ok(mcp_db) = MCPDatabase::new(app_handle) {
                match &out {
                    Ok(res) => {
                        let _ = mcp_db.update_mcp_tool_call_status(call_id, "success", Some(res), None);
                    }
                    Err(err) => {
                        let _ = mcp_db.update_mcp_tool_call_status(call_id, "failed", None, Some(err));
                    }
                }
            }
        }
        out
    });
    let result = prompt.run(execute).await?;

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
    let selected_tools_file = std::env::var(ACP_MCP_SELECTED_TOOLS_FILE_ENV).ok();
    let selected_tools_inline = std::env::var(ACP_MCP_SELECTED_TOOLS_ENV).ok();
    let selected_tools_payload = load_selected_mcp_tools_payload(
        selected_tools_file.as_deref(),
        selected_tools_inline.as_deref(),
    )?;
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

pub(crate) fn native_butler_instructions(role: &str, payload: &str) -> Result<String, String> {
    native_butler_instructions_with_catalog(role, payload, &[])
}

pub(crate) fn native_butler_instructions_with_catalog(
    role: &str,
    payload: &str,
    dynamic_catalog: &[(String, String)],
) -> Result<String, String> {
    let tools = parse_selected_mcp_tools(payload)?;
    let registered = |name: &str| {
        let mut matches = tools.iter().filter(|tool| tool.tool_name == name);
        let tool = matches.next()?;
        matches.next().is_none().then(|| acp_loaded_tool_name(tool.tool_id, name))
    };
    // These two product-authored rules require the transcript card. The native
    // tool catalog currently omits the card until the native UI can render it.
    let preview_available = registered("preview_message").is_some();
    let mut instructions = role.lines().filter(|line| {
        preview_available || !(line.starts_with("- 要把成员的答复、独立任务的最终答复或定时任务的产出原样交给用户时，用 `preview_message`")
            || line.starts_with("- 要把其他成员的答复原样交给用户时，用 `preview_message`"))
    }).collect::<Vec<_>>().join("\n");
    instructions.push_str("\n\n## AIPP 工具调用\n请使用当前 MCP 目录中的实际注册名，按 inputSchema 提交 JSON 对象。只在明确参数校验失败且未执行时修正重试。不可用的工具如实说明。");
    if let Some(progress) = registered("report_progress") {
        instructions.push_str(&format!(r#" 直接调用 {progress}，例如 {{"text":"我会核对任务记录。"}}；text 填本次实际要做的事。"#));
    }
    if let (Some(load), Some(call)) = (registered("load_mcp_tool"), registered("call_mcp_tool")) {
        if let Some(load_server) = registered("load_mcp_server") {
            instructions.push_str(&format!(" 动态工具可先用 {load_server} 查看工具集详情，再用 {load} 加载定义，最后用 {call} 调用：server_name 取加载结果的 server，tool_name 取 tool，parameters 按加载结果的 parameters 定义填写实际参数对象。"));
        } else {
            instructions.push_str(&format!(" 动态工具先用 {load} 加载定义，再用 {call} 调用：server_name 取加载结果的 server，tool_name 取 tool，parameters 按加载结果的 parameters 定义填写实际参数对象。"));
        }
    }
    if let (Some(inspect), Some(execute)) = (registered("superadmin_inspect"), registered("superadmin_execute")) {
        instructions.push_str(&format!(" 先从 {inspect} 读取 args_schema，调用 {execute} 时把对应动作字段放进 args 对象。"));
    }
    if !dynamic_catalog.is_empty() && registered("load_mcp_tool").is_some() && registered("call_mcp_tool").is_some() {
        instructions.push_str("\n\n## 可动态加载的 MCP 工具集目录\n");
        for (name, summary) in dynamic_catalog {
            let summary = if summary.trim().is_empty() { "暂无摘要" } else { summary.trim() };
            instructions.push_str(&format!("- `{name}`：{summary}\n"));
        }
    }
    Ok(instructions)
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
            if !server.dynamic_loading_enabled && matches!(tool.tool_name.as_str(), "load_mcp_server" | "load_mcp_tool") {
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

// The external clients expose a server-wide deadline. The bridge still applies the
// ordinary 600-second read deadline per noninteractive call.
pub(crate) const NATIVE_ANSWER_WINDOW_SECS: u64 = 24 * 60 * 60;

fn is_question_name(name: &str) -> bool {
    matches!(name, "ask_user_question" | "present_choices")
}

fn waits_for_answer(db: &MCPDatabase, tool: &AcpSelectedMcpTool) -> Result<bool, String> {
    if !is_question_name(&tool.tool_name) { return Ok(false); }
    let server = db.get_mcp_server(tool.server_id).map_err(|e| e.to_string())?;
    Ok(server.is_builtin && server.command.as_deref() == Some("aipp:ui_interaction"))
}

pub(crate) fn native_answer_window(app: &tauri::AppHandle, payload: &str) -> Result<Option<u64>, String> {
    let tools = parse_selected_mcp_tools(payload)?;
    let mut questions = tools.iter().filter(|tool| is_question_name(&tool.tool_name)).peekable();
    if questions.peek().is_some() {
        let db = MCPDatabase::new(app).map_err(|e| e.to_string())?;
        for tool in questions {
            if waits_for_answer(&db, tool)? { return Ok(Some(NATIVE_ANSWER_WINDOW_SECS)); }
        }
    }
    Ok(None)
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
            "aipp/reviewExemption" => self.review_exemption(&params),
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

    fn review_exemption(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        send_proxy_request(
            self.proxy_addr.as_deref().ok_or("AIPP MCP proxy address missing")?,
            false,
            json!({"token": self.proxy_token, "conversation_id": self.conversation_id,
                "review_registered_tool": params["name"], "arguments": params["arguments"]}),
        )
    }

    fn list_tools(&mut self) -> Result<serde_json::Value, String> {
        let mut tools = Vec::new();
        for tool in self.loaded_mcp_tools()? {
            let mut definition = selected_tool_to_mcp_tool(&tool);
            if waits_for_answer(&self.db, &tool)? {
                definition["_meta"] = json!({"aipp/waitsForAnswer": true});
            }
            tools.push(definition);
        }
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
            waits_for_answer(&self.db, &tool)?,
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
        operation_server_ids.contains(&server_id) && is_native_operation_duplicate(tool_name)
    }
}

fn is_native_operation_duplicate(name: &str) -> bool {
    matches!(name, "read_file" | "write_file" | "execute_bash" | "get_bash_output")
}

fn call_proxy_tool(
    proxy_addr: &str,
    proxy_token: &str,
    conversation_id: i64,
    server_id: i64,
    tool_name: &str,
    waits_for_answer: bool,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    send_proxy_request(proxy_addr, waits_for_answer, json!({
        "token": proxy_token, "conversation_id": conversation_id, "server_id": server_id,
        "tool_name": tool_name, "arguments": arguments,
    }))
}

fn send_proxy_request(proxy_addr: &str, waits_for_answer: bool, request: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut stream = TcpStream::connect(proxy_addr)
        .map_err(|error| format!("Failed to connect AIPP MCP proxy: {error}"))?;
    stream
        .set_read_timeout((!waits_for_answer).then(|| std::time::Duration::from_secs(600)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|error| error.to_string())?;

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
    fn test_choices_bridge_only_exempts_builtin_answer_tools_from_timeout() {
        let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
        db.create_tables().unwrap();
        let ui = db.upsert_mcp_server_with_builtin("ui", None, "stdio", Some("aipp:ui_interaction"), None, None, None, None, false, true, true, false, false).unwrap();
        let other = db.upsert_mcp_server_with_builtin("other", None, "http", None, None, None, Some("https://example.invalid"), None, false, true, false, false, false).unwrap();
        let make_tool = |id, server, name| serde_json::from_value(json!({
            "tool_id": id, "server_id": server, "tool_name": name
        })).unwrap();
        let mut bridge = AcpManualMcpBridge { db, conversation_id: 1, filter_acp_native_duplicates: false,
            proxy_addr: None, proxy_token: None, selected_tools: vec![
                make_tool(1, ui, "present_choices"), make_tool(2, ui, "ask_user_question"),
                make_tool(3, other, "present_choices"), make_tool(4, ui, "preview_code"),
            ] };
        let catalog = bridge.list_tools().unwrap();
        let flags: Vec<_> = catalog["tools"].as_array().unwrap().iter()
            .map(|tool| tool["_meta"]["aipp/waitsForAnswer"] == true).collect();
        assert_eq!(flags, [true, true, false, false]);
    }

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
    fn test_native_dynamic_loaders_require_explicit_mode_in_bridge_payload() {
        let tools = json!([{"tool_id":1,"tool_name":"load_mcp_tool"},{"tool_id":2,"tool_name":"load_mcp_server"},{"tool_id":3,"tool_name":"call_mcp_tool"}]);
        let legacy = json!([{"server_id":7,"server_name":"Agent","tools":tools}]);
        assert_eq!(parse_selected_mcp_tools(&legacy.to_string()).unwrap().len(),1);
        let explicit = json!([{"server_id":7,"server_name":"Agent","tools":tools,"dynamic_loading_enabled":true}]);
        assert_eq!(parse_selected_mcp_tools(&explicit.to_string()).unwrap().len(),3);
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

    #[test]
    fn load_selected_mcp_tools_payload_prefers_file_and_cleans_up_on_scope_drop() {
        let payload = format!("[{{\"server_id\":1,\"pad\":\"{}\"}}]", "x".repeat(40_000));
        let path = {
            let scope = provenance::NativeMcpScope::new(9).unwrap();
            let path = scope.attach_selected_tools_file(&payload).unwrap();
            assert!(path.exists());
            let loaded = load_selected_mcp_tools_payload(
                Some(path.to_str().unwrap()),
                Some("[{\"server_id\":999}]"),
            )
            .unwrap();
            assert_eq!(loaded, payload);
            path
        };
        assert!(!path.exists(), "scope drop should clean up temporary selected tools file");
        assert_eq!(
            load_selected_mcp_tools_payload(None, Some("[{\"server_id\":2}]")).unwrap(),
            "[{\"server_id\":2}]"
        );
        assert_eq!(load_selected_mcp_tools_payload(None, None).unwrap(), "[]");
    }
}

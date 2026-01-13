use crate::db::mcp_db::{
    MCPDatabase, MCPServer, MCPServerPrompt, MCPServerResource, MCPServerTool,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::Emitter;
use tracing::{info, instrument, warn};

// 超时常量集中定义，避免魔法数字分散
const STDIO_TEST_TIMEOUT: Duration = Duration::from_secs(5);
const CAPABILITY_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT_DEFAULT_MS: u64 = 30_000; // 连接阶段默认毫秒

// 简单的命令行解析，支持双引号/单引号与反斜杠转义，不依赖额外 crate
fn split_command_line(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for c in input.chars() {
        if escape {
            buf.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' => {
                escape = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' | '\n' if !in_single && !in_double => {
                if !buf.is_empty() {
                    parts.push(buf.clone());
                    buf.clear();
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        parts.push(buf);
    }
    parts
}

// 环境变量解析：忽略空行与以#开头的注释，只处理 KEY=VALUE 形式
fn parse_env_vars(env: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for line in env.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            if key.is_empty() {
                continue;
            }
            result.push((key.to_string(), v.trim().to_string()));
        }
    }
    result
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    #[serde(rename = "isAutoRun")]
    pub is_auto_run: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpProviderInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "transportType")]
    pub transport_type: String,
    #[serde(rename = "isEnabled")]
    pub is_enabled: bool,
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MCPServerRequest {
    pub name: String,
    pub description: Option<String>,
    pub transport_type: String,
    pub command: Option<String>,
    pub environment_variables: Option<String>,
    pub headers: Option<String>,
    pub url: Option<String>,
    pub timeout: Option<i32>,
    pub is_long_running: bool,
    pub is_enabled: bool,
    pub is_builtin: Option<bool>,
    pub proxy_enabled: bool,
}

// 打开数据库的辅助函数，减少重复样板代码
fn open_db(app_handle: &tauri::AppHandle) -> Result<MCPDatabase, String> {
    MCPDatabase::new(app_handle).map_err(|e: rusqlite::Error| e.to_string())
}

// 简化后的能力实体，用于统一持久化逻辑
struct SimpleTool {
    name: String,
    description: Option<String>,
    params_json: String,
}

struct SimpleResource {
    uri: String,
    name: String,
    mime_type: String,
    description: Option<String>,
}

struct SimplePrompt {
    name: String,
    description: Option<String>,
    args_json: String,
}

// 持久化工具/资源/提示，带删除缺失项；只在成功获取对应类别时才做删除，避免失败覆盖旧数据
fn persist_capability_sets(
    db: &MCPDatabase,
    server_id: i64,
    label: &str,
    tools_opt: Option<Vec<SimpleTool>>,
    resources_opt: Option<Vec<SimpleResource>>,
    prompts_opt: Option<Vec<SimplePrompt>>,
) -> Result<(), String> {
    if let Some(tools) = tools_opt {
        let remote_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
        info!(server_id, tool_count = remote_names.len(), label, "Persisting tools");
        for t in &tools {
            info!(server_id, tool_name = %t.name, label, "Fetched MCP tool");
            if let Err(e) = db.upsert_mcp_server_tool(
                server_id,
                &t.name,
                t.description.as_deref(),
                Some(&t.params_json),
            ) {
                warn!(server_id, tool = %t.name, error = %e, "Failed to upsert tool");
            }
        }
        if let Err(e) = db.delete_mcp_server_tools_not_in(server_id, &remote_names) {
            warn!(server_id, error = %e, label, "Failed to delete stale tools");
        }
    }

    if let Some(resources) = resources_opt {
        let remote_uris: Vec<String> = resources.iter().map(|r| r.uri.clone()).collect();
        info!(server_id, resource_count = remote_uris.len(), label, "Persisting resources");
        for r in &resources {
            if let Err(e) = db.upsert_mcp_server_resource(
                server_id,
                &r.uri,
                &r.name,
                &r.mime_type,
                r.description.as_deref(),
            ) {
                warn!(server_id, resource = %r.name, error = %e, "Failed to upsert resource");
            }
        }
        if let Err(e) = db.delete_mcp_server_resources_not_in(server_id, &remote_uris) {
            warn!(server_id, error = %e, label, "Failed to delete stale resources");
        }
    }

    if let Some(prompts) = prompts_opt {
        let remote_names: Vec<String> = prompts.iter().map(|p| p.name.clone()).collect();
        info!(server_id, prompt_count = remote_names.len(), label, "Persisting prompts");
        for p in &prompts {
            if let Err(e) = db.upsert_mcp_server_prompt(
                server_id,
                &p.name,
                p.description.as_deref(),
                Some(&p.args_json),
            ) {
                warn!(server_id, prompt = %p.name, error = %e, "Failed to upsert prompt");
            }
        }
        if let Err(e) = db.delete_mcp_server_prompts_not_in(server_id, &remote_names) {
            warn!(server_id, error = %e, label, "Failed to delete stale prompts");
        }
    }

    Ok(())
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle))]
pub async fn get_mcp_servers(app_handle: tauri::AppHandle) -> Result<Vec<MCPServer>, String> {
    let db = open_db(&app_handle)?;
    let servers = db.get_mcp_servers().map_err(|e| e.to_string())?;
    Ok(servers)
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(id))]
pub async fn get_mcp_server(app_handle: tauri::AppHandle, id: i64) -> Result<MCPServer, String> {
    let db = open_db(&app_handle)?;
    let server = db.get_mcp_server(id).map_err(|e| e.to_string())?;
    Ok(server)
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle, request), fields(name = %request.name, transport = %request.transport_type))]
pub async fn add_mcp_server(
    app_handle: tauri::AppHandle,
    request: MCPServerRequest,
) -> Result<i64, String> {
    let db = open_db(&app_handle)?;

    let server_id = db
        .upsert_mcp_server_with_builtin(
            &request.name,
            request.description.as_deref(),
            &request.transport_type,
            request.command.as_deref(),
            request.environment_variables.as_deref(),
            request.headers.as_deref(),
            request.url.as_deref(),
            request.timeout,
            request.is_long_running,
            request.is_enabled,
            request.is_builtin.unwrap_or(false),
            true, // is_deletable - 通过 API 添加的默认可删除
            request.proxy_enabled,
        )
        .map_err(|e| e.to_string())?;

    Ok(server_id)
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle, request), fields(id, name = %request.name))]
pub async fn update_mcp_server(
    app_handle: tauri::AppHandle,
    id: i64,
    request: MCPServerRequest,
) -> Result<(), String> {
    let db = open_db(&app_handle)?;

    db.update_mcp_server_with_builtin(
        id,
        &request.name,
        request.description.as_deref(),
        &request.transport_type,
        request.command.as_deref(),
        request.environment_variables.as_deref(),
        request.headers.as_deref(),
        request.url.as_deref(),
        request.timeout,
        request.is_long_running,
        request.is_enabled,
        request.is_builtin.unwrap_or(false),
        request.proxy_enabled,
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(id))]
pub async fn delete_mcp_server(app_handle: tauri::AppHandle, id: i64) -> Result<(), String> {
    let db = open_db(&app_handle)?;

    // 检查是否可删除（系统初始化的内置工具集不可删除）
    let server = db.get_mcp_server(id).map_err(|e| e.to_string())?;
    if !server.is_deletable {
        return Err("系统内置工具集不可删除".to_string());
    }

    db.delete_mcp_server(id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(id, is_enabled))]
pub async fn toggle_mcp_server(
    app_handle: tauri::AppHandle,
    id: i64,
    is_enabled: bool,
) -> Result<(), String> {
    let db = open_db(&app_handle)?;
    db.toggle_mcp_server(id, is_enabled).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(server_id))]
pub async fn get_mcp_server_tools(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<Vec<MCPServerTool>, String> {
    let db = open_db(&app_handle)?;
    let tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;
    Ok(tools)
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(tool_id, is_enabled, is_auto_run))]
pub async fn update_mcp_server_tool(
    app_handle: tauri::AppHandle,
    tool_id: i64,
    is_enabled: bool,
    is_auto_run: bool,
) -> Result<(), String> {
    let db = open_db(&app_handle)?;
    db.update_mcp_server_tool(tool_id, is_enabled, is_auto_run).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(server_id))]
pub async fn get_mcp_server_resources(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<Vec<MCPServerResource>, String> {
    let db = open_db(&app_handle)?;
    let resources = db.get_mcp_server_resources(server_id).map_err(|e| e.to_string())?;
    Ok(resources)
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(server_id))]
pub async fn test_mcp_connection(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<bool, String> {
    let db = open_db(&app_handle)?;
    let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;

    // 测试实际的MCP连接
    let test_result = match server.transport_type.as_str() {
        "stdio" => {
            if let Some(cmd) = &server.command {
                if crate::mcp::builtin_mcp::is_builtin_mcp_call(cmd) {
                    // 内置 aipp:* 不需要实际连接
                    Ok(())
                } else {
                    test_stdio_connection(&server).await
                }
            } else {
                test_stdio_connection(&server).await
            }
        }
        "sse" => test_sse_connection(&server).await,
        "http" => test_http_connection(&server).await,
        _ => Err(format!("Unsupported transport type: {}", server.transport_type)),
    };

    match test_result {
        Ok(_) => Ok(true),
        Err(e) => {
            warn!(error = %e, "MCP connection test failed");
            Ok(false)
        }
    }
}

// 测试stdio连接
async fn test_stdio_connection(server: &MCPServer) -> Result<(), String> {
    use rmcp::{
        transport::{ConfigureCommandExt, TokioChildProcess},
        ServiceExt,
    };
    use tokio::process::Command;

    let command = server.command.as_ref().ok_or("No command specified for stdio transport")?;
    let parts = split_command_line(command);
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    // 简短的连接测试，超时时间更短
    let client_result = tokio::time::timeout(STDIO_TEST_TIMEOUT, async {
        let client = ()
            .serve(TokioChildProcess::new(Command::new(&parts[0]).configure(|cmd| {
                if parts.len() > 1 {
                    cmd.args(&parts[1..]);
                }
                if let Some(env_vars) = &server.environment_variables {
                    for (k, v) in parse_env_vars(env_vars) {
                        cmd.env(k, v);
                    }
                }
            }))?)
            .await?;

        // 测试成功，取消连接
        client.cancel().await?;
        Ok::<(), anyhow::Error>(())
    })
    .await;

    match client_result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("Failed to create MCP client: {}", e)),
        Err(_) => Err("Timeout while connecting to MCP server".to_string()),
    }
}

// 测试SSE连接
async fn test_sse_connection(server: &MCPServer) -> Result<(), String> {
    use crate::mcp::util::parse_server_headers;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    use rmcp::{
        model::{ClientCapabilities, ClientInfo, Implementation},
        transport::{sse_client::SseClientConfig, SseClientTransport},
        ServiceExt,
    };

    let url = server.url.as_ref().ok_or("No URL specified for SSE transport")?;

    // 简短的连接测试，超时时间更短
    let client_result = tokio::time::timeout(STDIO_TEST_TIMEOUT, async {
        // Build client with default headers if configured
        let (_auth_header, all_headers) = parse_server_headers(server);
        let transport = if let Some(hdrs) = all_headers {
            let to_log = crate::mcp::util::sanitize_headers_for_log(&hdrs);
            info!(server_id = server.id, headers = ?to_log, "Testing SSE with headers");
            let mut header_map = HeaderMap::new();
            for (k, v) in hdrs.iter() {
                if let (Ok(name), Ok(value)) =
                    (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v.as_str()))
                {
                    header_map.insert(name, value);
                }
            }
            let client = reqwest::Client::builder().default_headers(header_map).build()?;
            SseClientTransport::start_with_client(
                client,
                SseClientConfig { sse_endpoint: url.as_str().into(), ..Default::default() },
            )
            .await?
        } else {
            SseClientTransport::start(url.as_str()).await?
        };
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "AIPP MCP SSE Test Client".to_string(),
                version: "0.1.0".to_string(),
                ..Default::default()
            },
        };
        let client = client_info.serve(transport).await?;

        // 测试成功，取消连接
        client.cancel().await?;
        Ok::<(), anyhow::Error>(())
    })
    .await;

    match client_result {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("Failed to create MCP SSE client: {}", e)),
        Err(_) => Err("Timeout while connecting to SSE server".to_string()),
    }
}

// 测试HTTP连接
async fn test_http_connection(server: &MCPServer) -> Result<(), String> {
    use crate::mcp::util::parse_server_headers;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    use rmcp::{
        model::{ClientCapabilities, ClientInfo, Implementation},
        transport::StreamableHttpClientTransport,
        ServiceExt,
    };

    let url = server.url.as_ref().ok_or("No URL specified for HTTP transport")?;

    // 创建StreamableHttpClientTransport传输，注入自定义 headers
    let (auth_header, all_headers) = parse_server_headers(server);
    let transport = {
        let mut header_map = HeaderMap::new();
        if let Some(hdrs) = all_headers.as_ref() {
            let to_log = crate::mcp::util::sanitize_headers_for_log(hdrs);
            info!(server_id = server.id, headers = ?to_log, "Testing HTTP with headers");
            for (k, v) in hdrs.iter() {
                if let (Ok(name), Ok(value)) =
                    (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v.as_str()))
                {
                    header_map.insert(name, value);
                }
            }
        }
        let client = reqwest::Client::builder()
            .default_headers(header_map)
            .build()
            .map_err(|e| e.to_string())?;
        let mut cfg =
            rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
                url.as_str(),
            );
        if let Some(auth) = auth_header.as_ref() {
            cfg = cfg.auth_header(auth.clone());
        }
        StreamableHttpClientTransport::with_client(client, cfg)
    };

    // 创建客户端信息
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "AIPP MCP Test Client".to_string(),
            version: "0.1.0".to_string(),
            ..Default::default()
        },
    };

    // 简短的连接测试，超时时间更短
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(server.timeout.unwrap_or(5000) as u64),
        async { client_info.serve(transport).await },
    )
    .await;

    match client_result {
        Ok(Ok(client)) => {
            // 测试成功，取消连接
            let _ = client.cancel().await;
            Ok(())
        }
        Ok(Err(e)) => Err(format!("Failed to create MCP client: {}", e)),
        Err(_) => Err("Timeout while connecting to HTTP server".to_string()),
    }
}

#[tauri::command]
pub async fn get_mcp_server_prompts(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<Vec<MCPServerPrompt>, String> {
    let db = open_db(&app_handle)?;
    let prompts = db.get_mcp_server_prompts(server_id).map_err(|e| e.to_string())?;
    Ok(prompts)
}

#[tauri::command]
pub async fn update_mcp_server_prompt(
    app_handle: tauri::AppHandle,
    prompt_id: i64,
    is_enabled: bool,
) -> Result<(), String> {
    let db = open_db(&app_handle)?;
    db.update_mcp_server_prompt(prompt_id, is_enabled).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn refresh_mcp_server_capabilities(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<(Vec<MCPServerTool>, Vec<MCPServerResource>, Vec<MCPServerPrompt>), String> {
    let db = open_db(&app_handle)?;
    let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;

    // Use incremental updates instead of clearing existing data

    // Try to connect to MCP server and get capabilities
    let result = match server.transport_type.as_str() {
        "stdio" => {
            // If aipp builtin server, register tools directly
            if let Some(cmd) = &server.command {
                if crate::mcp::builtin_mcp::is_builtin_mcp_call(cmd) {
                    let db = open_db(&app_handle)?;
                    for tool in crate::mcp::builtin_mcp::get_builtin_tools_for_command(cmd) {
                        let params_json = tool.input_schema.to_string();
                        let _ = db.upsert_mcp_server_tool(
                            server_id,
                            &tool.name,
                            Some(&tool.description),
                            Some(&params_json),
                        );
                    }
                    Ok(())
                } else {
                    get_stdio_capabilities(app_handle.clone(), server_id, server.clone()).await
                }
            } else {
                get_stdio_capabilities(app_handle.clone(), server_id, server.clone()).await
            }
        }
        "sse" => get_sse_capabilities(app_handle.clone(), server_id, server.clone()).await,
        "http" => get_http_capabilities(app_handle.clone(), server_id, server.clone()).await,
        _ => Err(format!("Unsupported transport type: {}", server.transport_type)),
    };

    match result {
        Ok(_) => {
            let tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;
            let resources = db.get_mcp_server_resources(server_id).map_err(|e| e.to_string())?;
            let prompts = db.get_mcp_server_prompts(server_id).map_err(|e| e.to_string())?;
            Ok((tools, resources, prompts))
        }
        Err(e) => {
            // 如果真实 MCP 连接失败，记录警告并返回错误（不再注入占位数据）
            warn!(error = %e, server_id, "MCP connection failed while fetching capabilities");
            Err(format!("获取 MCP 服务器工具错误: {}", e))
        }
    }
}

// Stdio transport implementation
#[instrument(level = "debug", skip(app_handle, server), fields(server_id))]
async fn get_stdio_capabilities(
    app_handle: tauri::AppHandle,
    server_id: i64,
    server: MCPServer,
) -> Result<(), String> {
    use rmcp::{
        transport::{ConfigureCommandExt, TokioChildProcess},
        ServiceExt,
    };
    use tokio::process::Command;

    let db = open_db(&app_handle)?;

    // 获取命令，如果没有则返回错误
    let command = server.command.ok_or("No command specified for stdio transport")?;

    // 解析命令和参数
    let parts = split_command_line(&command);
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    // 创建MCP客户端 - 使用正确的API模式
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(
            server.timeout.unwrap_or(CONNECT_TIMEOUT_DEFAULT_MS as i32) as u64,
        ),
        async {
            let client = ()
                .serve(TokioChildProcess::new(Command::new(&parts[0]).configure(|cmd| {
                    if parts.len() > 1 {
                        cmd.args(&parts[1..]);
                    }
                    if let Some(env_vars) = &server.environment_variables {
                        for (k, v) in parse_env_vars(env_vars) {
                            cmd.env(k, v);
                        }
                    }
                }))?)
                .await?;

            Ok::<_, anyhow::Error>(client)
        },
    )
    .await;

    let client = match client_result {
        Ok(Ok(client)) => client,
        Ok(Err(e)) => {
            return Err(format!("Failed to create MCP client: {}", e));
        }
        Err(_) => {
            return Err("Timeout while connecting to MCP server".to_string());
        }
    };

    // 获取服务器信息
    let _server_info = client.peer_info();

    // 获取能力 - 并发请求工具/资源/提示
    let capabilities_result = tokio::time::timeout(CAPABILITY_TIMEOUT, async {
        let (tools_result, resources_result, prompts_result) = tokio::join!(
            client.list_all_tools(),
            client.list_all_resources(),
            client.list_all_prompts()
        );
        (tools_result, resources_result, prompts_result)
    })
    .await;

    let (tools_result, resources_result, prompts_result) = match capabilities_result {
        Ok(results) => results,
        Err(_) => {
            return Err("Timeout while getting MCP server capabilities".to_string());
        }
    };

    // 转换结果为简化结构，失败则保持为 None（不做删除）
    let tools_simple = tools_result.ok().map(|tools| {
        tools
            .into_iter()
            .map(|tool| SimpleTool {
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(|d| d.to_string()),
                params_json: serde_json::to_string(&tool.input_schema)
                    .unwrap_or_else(|_| "{}".to_string()),
            })
            .collect::<Vec<_>>()
    });
    let resources_simple = resources_result.ok().map(|resources| {
        resources
            .into_iter()
            .map(|r| SimpleResource {
                uri: r.uri.to_string(),
                name: r.name.to_string(),
                mime_type: r.mime_type.as_deref().unwrap_or("unknown").to_string(),
                description: r.description.as_ref().map(|d| d.to_string()),
            })
            .collect::<Vec<_>>()
    });
    let prompts_simple = prompts_result.ok().map(|prompts| {
        prompts
            .into_iter()
            .map(|p| {
                let args_json = if let Some(args) = p.arguments {
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string())
                } else {
                    "{}".to_string()
                };
                SimplePrompt {
                    name: p.name.to_string(),
                    description: p.description.as_ref().map(|d| d.to_string()),
                    args_json,
                }
            })
            .collect::<Vec<_>>()
    });

    persist_capability_sets(
        &db,
        server_id,
        "stdio",
        tools_simple,
        resources_simple,
        prompts_simple,
    )?;

    // 取消客户端连接
    let _ = client.cancel().await;

    Ok(())
}

#[instrument(level = "debug", skip(app_handle, server), fields(server_id))]
async fn get_sse_capabilities(
    app_handle: tauri::AppHandle,
    server_id: i64,
    server: MCPServer,
) -> Result<(), String> {
    use crate::mcp::util::parse_server_headers;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    use rmcp::{
        model::{ClientCapabilities, ClientInfo, Implementation},
        transport::SseClientTransport,
        ServiceExt,
    };

    let db = open_db(&app_handle)?;

    // 获取URL，如果没有则返回错误
    let url = server.url.clone().ok_or("No URL specified for SSE transport")?;

    // 创建SSE传输和客户端（复用与 test_sse_connection 相同的模式，仅增加能力同步逻辑）
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(server.timeout.unwrap_or(CONNECT_TIMEOUT_DEFAULT_MS as i32) as u64),
        async {
            let (_auth_header, all_headers) = parse_server_headers(&server);
            let transport = if let Some(hdrs) = all_headers {
                let to_log = crate::mcp::util::sanitize_headers_for_log(&hdrs);
                info!(server_id = server.id, headers = ?to_log, "Fetching SSE capabilities with headers");
                let mut header_map = HeaderMap::new();
                for (k, v) in hdrs.iter() {
                    if let (Ok(name), Ok(value)) =
                        (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v.as_str()))
                    {
                        header_map.insert(name, value);
                    }
                }
                let client = reqwest::Client::builder().default_headers(header_map).build()?;
                SseClientTransport::start_with_client(
                    client,
                    rmcp::transport::sse_client::SseClientConfig {
                        sse_endpoint: url.as_str().into(),
                        ..Default::default()
                    },
                )
                .await?
            } else {
                SseClientTransport::start(url.as_str()).await?
            };

            let client_info = ClientInfo {
                protocol_version: Default::default(),
                capabilities: ClientCapabilities::default(),
                client_info: Implementation {
                    name: "AIPP MCP SSE Client".to_string(),
                    version: "0.1.0".to_string(),
                    ..Default::default()
                },
            };
            let client = client_info.serve(transport).await?;
            Ok::<_, anyhow::Error>(client)
        },
    )
    .await;

    let client = match client_result {
        Ok(Ok(client)) => client,
        Ok(Err(e)) => {
            return Err(format!("Failed to create MCP SSE client: {}", e));
        }
        Err(_) => {
            return Err("Timeout while connecting to SSE server".to_string());
        }
    };

    // 获取服务器信息
    let _server_info = client.peer_info();

    // 获取能力
    let capabilities_result = tokio::time::timeout(CAPABILITY_TIMEOUT, async {
        let (tools_result, resources_result, prompts_result) = tokio::join!(
            client.list_all_tools(),
            client.list_all_resources(),
            client.list_all_prompts()
        );
        (tools_result, resources_result, prompts_result)
    })
    .await;

    let (tools_result, resources_result, prompts_result) = match capabilities_result {
        Ok(results) => results,
        Err(_) => {
            return Err("Timeout while getting SSE server capabilities".to_string());
        }
    };

    let tools_simple = tools_result.ok().map(|tools| {
        tools
            .into_iter()
            .map(|tool| SimpleTool {
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(|d| d.to_string()),
                params_json: serde_json::to_string(&tool.input_schema)
                    .unwrap_or_else(|_| "{}".to_string()),
            })
            .collect::<Vec<_>>()
    });
    let resources_simple = resources_result.ok().map(|resources| {
        resources
            .into_iter()
            .map(|r| SimpleResource {
                uri: r.uri.to_string(),
                name: r.name.to_string(),
                mime_type: r.mime_type.as_deref().unwrap_or("unknown").to_string(),
                description: r.description.as_ref().map(|d| d.to_string()),
            })
            .collect::<Vec<_>>()
    });
    let prompts_simple = prompts_result.ok().map(|prompts| {
        prompts
            .into_iter()
            .map(|p| {
                let args_json = if let Some(args) = p.arguments {
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string())
                } else {
                    "{}".to_string()
                };
                SimplePrompt {
                    name: p.name.to_string(),
                    description: p.description.as_ref().map(|d| d.to_string()),
                    args_json,
                }
            })
            .collect::<Vec<_>>()
    });
    persist_capability_sets(&db, server_id, "sse", tools_simple, resources_simple, prompts_simple)?;

    // 取消客户端连接
    let _ = client.cancel().await;

    Ok(())
}

#[instrument(level = "debug", skip(app_handle, server), fields(server_id))]
async fn get_http_capabilities(
    app_handle: tauri::AppHandle,
    server_id: i64,
    server: MCPServer,
) -> Result<(), String> {
    use crate::mcp::util::parse_server_headers;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    use rmcp::{
        model::{ClientCapabilities, ClientInfo, Implementation},
        transport::StreamableHttpClientTransport,
        ServiceExt,
    };

    let db = open_db(&app_handle)?;

    // 获取URL，如果没有则返回错误
    let url = server.url.clone().ok_or("No URL specified for HTTP transport")?;

    // 创建HTTP传输和客户端（复用 test_http_connection 中的模式，仅增加能力同步逻辑）
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(server.timeout.unwrap_or(CONNECT_TIMEOUT_DEFAULT_MS as i32) as u64),
        async {
            let (auth_header, all_headers) = parse_server_headers(&server);
            let transport = {
                let mut header_map = HeaderMap::new();
                if let Some(hdrs) = all_headers.as_ref() {
                    let to_log = crate::mcp::util::sanitize_headers_for_log(hdrs);
                    info!(server_id = server.id, headers = ?to_log, "Fetching HTTP capabilities with headers");
                    for (k, v) in hdrs.iter() {
                        if let (Ok(name), Ok(value)) =
                            (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v.as_str()))
                        {
                            header_map.insert(name, value);
                        }
                    }
                }
                let client = reqwest::Client::builder()
                    .default_headers(header_map)
                    .build()
                    .map_err(|e| anyhow::anyhow!("Failed to build reqwest client for HTTP: {}", e))?;
                let mut cfg = rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url.as_str());
                if let Some(auth) = auth_header.as_ref() {
                    cfg = cfg.auth_header(auth.clone());
                }
                StreamableHttpClientTransport::with_client(client, cfg)
            };

            let client_info = ClientInfo {
                protocol_version: Default::default(),
                capabilities: ClientCapabilities::default(),
                client_info: Implementation {
                    name: "AIPP MCP HTTP Client".to_string(),
                    version: "0.1.0".to_string(),
                    ..Default::default()
                },
            };
            let client = client_info.serve(transport).await?;
            Ok::<_, anyhow::Error>(client)
        },
    )
    .await;

    let client = match client_result {
        Ok(Ok(client)) => client,
        Ok(Err(e)) => {
            return Err(format!("Failed to create MCP HTTP client: {}", e));
        }
        Err(_) => {
            return Err("Timeout while connecting to HTTP server".to_string());
        }
    };

    // 获取服务器信息
    let _server_info = client.peer_info();

    // 获取能力
    let capabilities_result = tokio::time::timeout(CAPABILITY_TIMEOUT, async {
        let (tools_result, resources_result, prompts_result) = tokio::join!(
            client.list_all_tools(),
            client.list_all_resources(),
            client.list_all_prompts()
        );
        (tools_result, resources_result, prompts_result)
    })
    .await;

    let (tools_result, resources_result, prompts_result) = match capabilities_result {
        Ok(results) => results,
        Err(_) => {
            return Err("Timeout while getting HTTP server capabilities".to_string());
        }
    };

    let tools_simple = tools_result.ok().map(|tools| {
        tools
            .into_iter()
            .map(|tool| SimpleTool {
                name: tool.name.to_string(),
                description: tool.description.as_ref().map(|d| d.to_string()),
                params_json: serde_json::to_string(&tool.input_schema)
                    .unwrap_or_else(|_| "{}".to_string()),
            })
            .collect::<Vec<_>>()
    });
    let resources_simple = resources_result.ok().map(|resources| {
        resources
            .into_iter()
            .map(|r| SimpleResource {
                uri: r.uri.to_string(),
                name: r.name.to_string(),
                mime_type: r.mime_type.as_deref().unwrap_or("unknown").to_string(),
                description: r.description.as_ref().map(|d| d.to_string()),
            })
            .collect::<Vec<_>>()
    });
    let prompts_simple = prompts_result.ok().map(|prompts| {
        prompts
            .into_iter()
            .map(|p| {
                let args_json = if let Some(args) = p.arguments {
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string())
                } else {
                    "{}".to_string()
                };
                SimplePrompt {
                    name: p.name.to_string(),
                    description: p.description.as_ref().map(|d| d.to_string()),
                    args_json,
                }
            })
            .collect::<Vec<_>>()
    });
    persist_capability_sets(
        &db,
        server_id,
        "http",
        tools_simple,
        resources_simple,
        prompts_simple,
    )?;

    // 取消客户端连接
    let _ = client.cancel().await;

    Ok(())
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(provider_id))]
pub async fn get_mcp_provider(
    app_handle: tauri::AppHandle,
    provider_id: String,
) -> Result<Option<McpProviderInfo>, String> {
    let db = open_db(&app_handle)?;

    // Parse provider_id as server ID
    let server_id: i64 =
        provider_id.parse().map_err(|_| "Invalid provider ID format".to_string())?;

    // Get server information
    let server = match db.get_mcp_server(server_id) {
        Ok(server) => server,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };

    // Get server tools
    let server_tools = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;

    // Convert to McpProviderInfo format
    let tools: Vec<McpToolInfo> = server_tools
        .into_iter()
        .map(|tool| McpToolInfo {
            name: tool.tool_name,
            description: tool.tool_description.unwrap_or_default(),
            parameters: tool.parameters.unwrap_or_else(|| "{}".to_string()),
            is_enabled: tool.is_enabled,
            is_auto_run: tool.is_auto_run,
        })
        .collect();

    let provider_info = McpProviderInfo {
        id: server.id.to_string(),
        name: server.name,
        description: if server.description.is_empty() { None } else { Some(server.description) },
        transport_type: server.transport_type,
        is_enabled: server.is_enabled,
        tools,
    };

    Ok(Some(provider_info))
}

#[tauri::command]
#[instrument(level = "debug", skip(app_handle, provider_ids), fields(provider_count = provider_ids.len()))]
pub async fn build_mcp_prompt(
    app_handle: tauri::AppHandle,
    provider_ids: Vec<String>,
) -> Result<String, String> {
    use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};
    use crate::mcp::format_mcp_prompt;

    let db = open_db(&app_handle)?;

    let mut enabled_servers = Vec::new();

    // Process each provider ID to build enabled servers list
    for provider_id in provider_ids {
        let server_id: i64 = match provider_id.parse() {
            Ok(id) => id,
            Err(_) => {
                warn!(provider_id = %provider_id, "Invalid provider ID format while building MCP prompt");
                continue;
            }
        };

        // Get server information
        let server = match db.get_mcp_server(server_id) {
            Ok(server) if server.is_enabled => server,
            _ => continue, // Skip disabled or non-existent servers
        };

        // Get server tools
        let server_tools = match db.get_mcp_server_tools(server_id) {
            Ok(tools) => tools,
            Err(_) => continue,
        };

        // Only include enabled tools and convert to the expected format
        let enabled_tools: Vec<MCPToolInfo> = server_tools
            .into_iter()
            .filter(|tool| tool.is_enabled)
            .map(|tool| MCPToolInfo {
                id: tool.id,
                name: tool.tool_name,
                description: tool.tool_description.unwrap_or_default(),
                is_enabled: tool.is_enabled,
                is_auto_run: tool.is_auto_run,
                parameters: tool.parameters.unwrap_or_else(|| "{}".to_string()),
            })
            .collect();

        if enabled_tools.is_empty() {
            continue;
        }

        // Build MCPServerWithTools
        let server_with_tools = MCPServerWithTools {
            id: server.id,
            name: server.name,
            command: server.command.clone(),
            is_enabled: server.is_enabled,
            tools: enabled_tools,
        };

        enabled_servers.push(server_with_tools);
    }

    if enabled_servers.is_empty() {
        return Ok("No MCP tools available for the specified providers.".to_string());
    }

    // Build MCPInfoForAssistant structure
    let mcp_info = crate::mcp::MCPInfoForAssistant {
        enabled_servers,
        use_native_toolcall: false, // For prompt generation, we use prompt-based mode
    };

    // Use existing format_mcp_prompt function
    let result = format_mcp_prompt("".to_string(), &mcp_info).await;
    Ok(result)
}

// =============================================================================
// Skills 与操作 MCP 联动校验 API
// =============================================================================

/// 操作 MCP 工具集的 command 标识
pub const OPERATION_MCP_COMMAND: &str = "aipp:operation";
/// Agent MCP 工具集 command
pub const AGENT_MCP_COMMAND: &str = "aipp:agent";
/// Agent 工具集中用于加载 Skill 的工具名
pub const AGENT_LOAD_SKILL_TOOL_NAME: &str = "load_skill";

/// 获取操作 MCP 工具集（用于校验）
fn get_operation_mcp_server(db: &MCPDatabase) -> Result<Option<MCPServer>, String> {
    let servers = db.get_mcp_servers().map_err(|e| e.to_string())?;
    Ok(servers.into_iter().find(|s| s.command.as_deref() == Some(OPERATION_MCP_COMMAND)))
}

/// 获取 Agent MCP 服务器（任意存在即可，支持用户自建）
fn get_agent_mcp_server(db: &MCPDatabase) -> Result<Option<MCPServer>, String> {
    let servers = db.get_mcp_servers().map_err(|e| e.to_string())?;
    // 优先返回已启用的 Agent 服务器，其次返回任意 Agent 服务器
    if let Some(enabled) = servers
        .iter()
        .find(|s| s.command.as_deref() == Some(AGENT_MCP_COMMAND) && s.is_enabled)
        .cloned()
    {
        return Ok(Some(enabled));
    }
    Ok(servers.into_iter().find(|s| s.command.as_deref() == Some(AGENT_MCP_COMMAND)))
}

/// 确保 Agent 的 load_skill 工具已启用（全局 + 助手级）
pub fn ensure_agent_load_skill_for_assistant(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> Result<(), String> {
    use crate::db::assistant_db::AssistantDatabase;

    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let agent_server =
        get_agent_mcp_server(&mcp_db)?.ok_or_else(|| "AGENT_LOAD_SKILL_REQUIRED".to_string())?;

    // 开启全局 Agent MCP
    if !agent_server.is_enabled {
        mcp_db.toggle_mcp_server(agent_server.id, true).map_err(|e| e.to_string())?;
    }

    // 找到 load_skill 工具并开启
    let tools = mcp_db.get_mcp_server_tools(agent_server.id).map_err(|e| e.to_string())?;
    let load_skill_tool = tools
        .into_iter()
        .find(|t| t.tool_name == AGENT_LOAD_SKILL_TOOL_NAME)
        .ok_or_else(|| "AGENT_LOAD_SKILL_REQUIRED".to_string())?;

    if !load_skill_tool.is_enabled {
        mcp_db
            .update_mcp_server_tool(load_skill_tool.id, true, load_skill_tool.is_auto_run)
            .map_err(|e| e.to_string())?;
    }

    // 开启助手级配置（服务器 + 工具）
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    assistant_db
        .upsert_assistant_mcp_config(assistant_id, agent_server.id, true)
        .map_err(|e| e.to_string())?;
    assistant_db
        .upsert_assistant_mcp_tool_config(
            assistant_id,
            load_skill_tool.id,
            true,
            load_skill_tool.is_auto_run,
        )
        .map_err(|e| e.to_string())?;

    // 通知前端刷新 MCP 状态
    let _ = app_handle.emit("mcp_state_changed", "agent_load_skill_enabled");

    Ok(())
}

/// 检查 Agent load_skill 是否可用（不做自动开启）
pub fn is_agent_load_skill_ready(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> Result<bool, String> {
    use crate::db::assistant_db::AssistantDatabase;
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let agent_server = match get_agent_mcp_server(&mcp_db)? {
        Some(s) => s,
        None => return Ok(false),
    };

    if !agent_server.is_enabled {
        return Ok(false);
    }

    let tools = mcp_db.get_mcp_server_tools(agent_server.id).map_err(|e| e.to_string())?;
    let load_skill = match tools.iter().find(|t| t.tool_name == AGENT_LOAD_SKILL_TOOL_NAME) {
        Some(t) if t.is_enabled => t,
        _ => return Ok(false),
    };

    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let assistant_mcp_configs =
        assistant_db.get_assistant_mcp_configs(assistant_id).map_err(|e| e.to_string())?;
    let assistant_enabled = assistant_mcp_configs
        .iter()
        .find(|c| c.mcp_server_id == agent_server.id)
        .map(|c| c.is_enabled)
        .unwrap_or(false);
    if !assistant_enabled {
        return Ok(false);
    }

    let assistant_tool_configs =
        assistant_db.get_assistant_mcp_tool_configs(assistant_id).map_err(|e| e.to_string())?;
    let load_skill_assistant_enabled = assistant_tool_configs
        .iter()
        .find(|c| c.mcp_tool_id == load_skill.id)
        .map(|c| c.is_enabled)
        .unwrap_or(false);

    Ok(load_skill_assistant_enabled)
}

/// 检查结果结构体
#[derive(Debug, Serialize, Deserialize)]
pub struct OperationMcpCheckResult {
    /// 操作 MCP 服务器 ID（如果存在）
    pub operation_mcp_id: Option<i64>,
    /// 操作 MCP 全局是否启用
    pub global_enabled: bool,
    /// 操作 MCP 助手级是否启用（如果指定了助手）
    pub assistant_enabled: bool,
    /// 助手当前启用的 Skills 数量
    pub enabled_skills_count: usize,
    /// Agent MCP 服务器 ID（如果存在）
    pub agent_mcp_id: Option<i64>,
    /// Agent MCP 是否全局启用
    pub agent_enabled: bool,
    /// Agent MCP 助手级是否启用
    pub agent_assistant_enabled: bool,
    /// Agent load_skill 工具是否全局启用
    pub agent_load_skill_enabled: bool,
    /// Agent load_skill 工具助手级是否启用
    pub agent_load_skill_assistant_enabled: bool,
    /// Agent load_skill 是否已就绪（全局 + 助手级）
    pub agent_ready: bool,
}

/// 检查操作 MCP 是否已启用（用于启用 Skills 前的校验）
/// 返回操作 MCP 的状态信息
#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(assistant_id))]
pub async fn check_operation_mcp_for_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<OperationMcpCheckResult, String> {
    let db = open_db(&app_handle)?;

    // 获取操作 MCP 服务器
    let operation_mcp = get_operation_mcp_server(&db)?;

    let (operation_mcp_id, global_enabled) = match &operation_mcp {
        Some(server) => (Some(server.id), server.is_enabled),
        None => (None, false),
    };

    // 检查助手级 MCP 配置
    use crate::db::assistant_db::AssistantDatabase;
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let assistant_enabled = if let Some(server) = &operation_mcp {
        let mcp_configs =
            assistant_db.get_assistant_mcp_configs(assistant_id).map_err(|e| e.to_string())?;
        mcp_configs
            .iter()
            .find(|c| c.mcp_server_id == server.id)
            .map(|c| c.is_enabled)
            .unwrap_or(false)
    } else {
        false
    };

    // Agent MCP 状态
    let agent_server = get_agent_mcp_server(&db)?;
    let (
        agent_mcp_id,
        agent_enabled,
        agent_assistant_enabled,
        agent_load_skill_enabled,
        agent_load_skill_assistant_enabled,
    ) = if let Some(agent) = &agent_server {
        let assistant_mcp_configs =
            assistant_db.get_assistant_mcp_configs(assistant_id).map_err(|e| e.to_string())?;
        let assistant_enabled_flag = assistant_mcp_configs
            .iter()
            .find(|c| c.mcp_server_id == agent.id)
            .map(|c| c.is_enabled)
            .unwrap_or(false);

        let tools = db.get_mcp_server_tools(agent.id).map_err(|e| e.to_string())?;
        let load_skill = tools.iter().find(|t| t.tool_name == AGENT_LOAD_SKILL_TOOL_NAME);
        let load_skill_enabled = load_skill.map(|t| t.is_enabled).unwrap_or(false);

        let assistant_tool_configs =
            assistant_db.get_assistant_mcp_tool_configs(assistant_id).map_err(|e| e.to_string())?;
        let load_skill_assistant_enabled = load_skill
            .and_then(|tool| assistant_tool_configs.iter().find(|c| c.mcp_tool_id == tool.id))
            .map(|c| c.is_enabled)
            .unwrap_or(false);

        (
            Some(agent.id),
            agent.is_enabled,
            assistant_enabled_flag,
            load_skill_enabled,
            load_skill_assistant_enabled,
        )
    } else {
        (None, false, false, false, false)
    };
    let agent_ready = agent_mcp_id.is_some()
        && agent_enabled
        && agent_assistant_enabled
        && agent_load_skill_enabled
        && agent_load_skill_assistant_enabled;

    // 获取助手启用的 Skills 数量
    use crate::db::skill_db::SkillDatabase;
    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let enabled_skills =
        skill_db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;

    Ok(OperationMcpCheckResult {
        operation_mcp_id,
        global_enabled,
        assistant_enabled,
        enabled_skills_count: enabled_skills.len(),
        agent_mcp_id,
        agent_enabled,
        agent_assistant_enabled,
        agent_load_skill_enabled,
        agent_load_skill_assistant_enabled,
        agent_ready,
    })
}

/// 启用操作 MCP 并启用指定的 Skill
/// 同时启用全局 MCP 和助手级 MCP 配置
#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(assistant_id, skill_identifier))]
pub async fn enable_operation_mcp_and_skill(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    skill_identifier: String,
    priority: i32,
) -> Result<(), String> {
    // 启用 Agent load_skill（全局 + 助手级）
    ensure_agent_load_skill_for_assistant(&app_handle, assistant_id)?;

    // 启用 Skill
    use crate::db::skill_db::SkillDatabase;
    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    skill_db
        .upsert_assistant_skill_config(assistant_id, &skill_identifier, true, priority)
        .map_err(|e| e.to_string())?;
    info!(assistant_id, skill_identifier = %skill_identifier, "Enabled skill after enabling agent load_skill");

    Ok(())
}

/// 批量启用操作 MCP 和多个 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle, skill_configs), fields(assistant_id))]
pub async fn enable_operation_mcp_and_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    skill_configs: Vec<(String, i32)>, // (skill_identifier, priority)
) -> Result<(), String> {
    // 启用 Agent load_skill（全局 + 助手级）
    ensure_agent_load_skill_for_assistant(&app_handle, assistant_id)?;

    // 批量启用 Skills
    use crate::db::skill_db::SkillDatabase;
    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    for (skill_identifier, priority) in skill_configs {
        skill_db
            .upsert_assistant_skill_config(assistant_id, &skill_identifier, true, priority)
            .map_err(|e| e.to_string())?;
    }

    info!(assistant_id, "Enabled agent load_skill and skills");
    Ok(())
}

/// 关闭操作 MCP 的校验结果
#[derive(Debug, Serialize, Deserialize)]
pub struct DisableOperationMcpCheckResult {
    /// 受影响的助手列表（有启用 Skills 的助手）
    pub affected_assistants: Vec<AffectedAssistantInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AffectedAssistantInfo {
    pub assistant_id: i64,
    pub assistant_name: String,
    pub enabled_skills_count: usize,
}

/// 检查关闭操作 MCP 会影响哪些助手的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle))]
pub async fn check_disable_operation_mcp(
    app_handle: tauri::AppHandle,
) -> Result<DisableOperationMcpCheckResult, String> {
    use crate::db::assistant_db::AssistantDatabase;
    use crate::db::skill_db::SkillDatabase;

    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // 获取所有助手
    let assistants = assistant_db.get_assistants().map_err(|e| e.to_string())?;

    let mut affected_assistants = Vec::new();

    for assistant in assistants {
        let enabled_skills =
            skill_db.get_enabled_skill_configs(assistant.id).map_err(|e| e.to_string())?;
        if !enabled_skills.is_empty() {
            affected_assistants.push(AffectedAssistantInfo {
                assistant_id: assistant.id,
                assistant_name: assistant.name,
                enabled_skills_count: enabled_skills.len(),
            });
        }
    }

    Ok(DisableOperationMcpCheckResult { affected_assistants })
}

/// 关闭操作 MCP 并同时关闭所有助手的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle))]
pub async fn disable_operation_mcp_with_skills(app_handle: tauri::AppHandle) -> Result<(), String> {
    use crate::db::assistant_db::AssistantDatabase;
    use crate::db::skill_db::SkillDatabase;

    let db = open_db(&app_handle)?;

    // 获取操作 MCP 服务器
    let operation_mcp = get_operation_mcp_server(&db)?;

    if let Some(server) = operation_mcp {
        // 1. 关闭所有助手的 Skills
        let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;

        let assistants = assistant_db.get_assistants().map_err(|e| e.to_string())?;
        for assistant in assistants {
            let enabled_skills =
                skill_db.get_enabled_skill_configs(assistant.id).map_err(|e| e.to_string())?;
            for skill_config in enabled_skills {
                skill_db
                    .update_skill_config_enabled(skill_config.id, false)
                    .map_err(|e| e.to_string())?;
            }
        }

        // 2. 关闭全局 MCP
        db.toggle_mcp_server(server.id, false).map_err(|e| e.to_string())?;

        info!(server_id = server.id, "Disabled operation MCP and all assistant skills");
        let _ = app_handle.emit("mcp_state_changed", "operation_disabled");
    }

    Ok(())
}

/// 检查关闭 Agent MCP 会影响哪些助手的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle))]
pub async fn check_disable_agent_mcp(
    app_handle: tauri::AppHandle,
) -> Result<DisableOperationMcpCheckResult, String> {
    use crate::db::assistant_db::AssistantDatabase;
    use crate::db::skill_db::SkillDatabase;

    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    let assistants = assistant_db.get_assistants().map_err(|e| e.to_string())?;
    let mut affected_assistants = Vec::new();

    for assistant in assistants {
        let enabled_skills =
            skill_db.get_enabled_skill_configs(assistant.id).map_err(|e| e.to_string())?;
        if !enabled_skills.is_empty() {
            affected_assistants.push(AffectedAssistantInfo {
                assistant_id: assistant.id,
                assistant_name: assistant.name,
                enabled_skills_count: enabled_skills.len(),
            });
        }
    }

    Ok(DisableOperationMcpCheckResult { affected_assistants })
}

/// 关闭 Agent MCP 并同时关闭所有助手的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle))]
pub async fn disable_agent_mcp_with_skills(app_handle: tauri::AppHandle) -> Result<(), String> {
    use crate::db::assistant_db::AssistantDatabase;
    use crate::db::skill_db::SkillDatabase;

    let db = open_db(&app_handle)?;

    // 获取 Agent MCP 服务器
    let agent_mcp = get_agent_mcp_server(&db)?;

    if let Some(server) = agent_mcp {
        let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;

        let assistants = assistant_db.get_assistants().map_err(|e| e.to_string())?;
        for assistant in assistants {
            let enabled_skills =
                skill_db.get_enabled_skill_configs(assistant.id).map_err(|e| e.to_string())?;
            for skill_config in enabled_skills {
                skill_db
                    .update_skill_config_enabled(skill_config.id, false)
                    .map_err(|e| e.to_string())?;
            }
            // 同时禁用助手级 Agent MCP 及 load_skill 工具
            let _ = assistant_db.upsert_assistant_mcp_config(assistant.id, server.id, false);
            let tools = db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())?;
            if let Some(load_skill) =
                tools.iter().find(|t| t.tool_name == AGENT_LOAD_SKILL_TOOL_NAME)
            {
                let _ = assistant_db.upsert_assistant_mcp_tool_config(
                    assistant.id,
                    load_skill.id,
                    false,
                    load_skill.is_auto_run,
                );
            }
        }

        // 关闭全局 Agent MCP 与 load_skill
        db.toggle_mcp_server(server.id, false).map_err(|e| e.to_string())?;
        let tools = db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())?;
        if let Some(load_skill) = tools.iter().find(|t| t.tool_name == AGENT_LOAD_SKILL_TOOL_NAME) {
            let _ = db.update_mcp_server_tool(load_skill.id, false, load_skill.is_auto_run);
        }

        info!(server_id = server.id, "Disabled agent MCP and all assistant skills");
        let _ = app_handle.emit("mcp_state_changed", "agent_disabled");
    }

    Ok(())
}

/// 检查关闭助手级操作 MCP 会影响的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(assistant_id))]
pub async fn check_disable_assistant_operation_mcp(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<usize, String> {
    use crate::db::skill_db::SkillDatabase;

    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let enabled_skills =
        skill_db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;

    Ok(enabled_skills.len())
}

/// 关闭助手级操作 MCP 并同时关闭该助手的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(assistant_id))]
pub async fn disable_assistant_operation_mcp_with_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<(), String> {
    use crate::db::assistant_db::AssistantDatabase;
    use crate::db::skill_db::SkillDatabase;

    let db = open_db(&app_handle)?;

    // 获取操作 MCP 服务器
    let operation_mcp = get_operation_mcp_server(&db)?;

    if let Some(server) = operation_mcp {
        // 1. 关闭该助手的所有 Skills
        let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let enabled_skills =
            skill_db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;
        for skill_config in enabled_skills {
            skill_db
                .update_skill_config_enabled(skill_config.id, false)
                .map_err(|e| e.to_string())?;
        }

        // 2. 关闭助手级 MCP 配置
        let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        assistant_db
            .upsert_assistant_mcp_config(assistant_id, server.id, false)
            .map_err(|e| e.to_string())?;

        info!(assistant_id, server_id = server.id, "Disabled assistant operation MCP and skills");
        let _ = app_handle.emit("mcp_state_changed", "assistant_operation_disabled");
    }

    Ok(())
}

/// 检查关闭助手级 Agent MCP 会影响的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(assistant_id))]
pub async fn check_disable_assistant_agent_mcp(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<usize, String> {
    use crate::db::skill_db::SkillDatabase;

    let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let enabled_skills =
        skill_db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;

    Ok(enabled_skills.len())
}

/// 关闭助手级 Agent MCP 并同时关闭该助手的 Skills
#[tauri::command]
#[instrument(level = "debug", skip(app_handle), fields(assistant_id))]
pub async fn disable_assistant_agent_mcp_with_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<(), String> {
    use crate::db::assistant_db::AssistantDatabase;
    use crate::db::skill_db::SkillDatabase;

    let db = open_db(&app_handle)?;
    let agent_mcp = get_agent_mcp_server(&db)?;

    if let Some(server) = agent_mcp {
        let skill_db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let enabled_skills =
            skill_db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;
        for skill_config in enabled_skills {
            skill_db
                .update_skill_config_enabled(skill_config.id, false)
                .map_err(|e| e.to_string())?;
        }

        let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        assistant_db
            .upsert_assistant_mcp_config(assistant_id, server.id, false)
            .map_err(|e| e.to_string())?;

        let tools = db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())?;
        if let Some(load_skill) = tools.iter().find(|t| t.tool_name == AGENT_LOAD_SKILL_TOOL_NAME) {
            assistant_db
                .upsert_assistant_mcp_tool_config(
                    assistant_id,
                    load_skill.id,
                    false,
                    load_skill.is_auto_run,
                )
                .map_err(|e| e.to_string())?;
        }

        info!(assistant_id, server_id = server.id, "Disabled assistant agent MCP and skills");
        let _ = app_handle.emit("mcp_state_changed", "assistant_agent_disabled");
    }

    Ok(())
}

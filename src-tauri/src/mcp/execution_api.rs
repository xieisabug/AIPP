//! MCP 工具调用执行与会话续写模块
//!
//! 该模块负责：
//! 1. 创建与查询 MCP 工具调用记录
//! 2. 根据不同传输类型（stdio / sse / http / builtin）执行工具
//! 3. 统一的参数解析、响应序列化与错误处理抽象
//! 4. 将执行结果写回数据库并触发前端事件
//! 5. 在工具成功后继续驱动 AI 对话（包含重试场景）
use crate::api::ai::events::{ConversationEvent, MCPToolCallUpdateEvent};
use crate::api::ai_api::tool_result_continue_ask_ai;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::mcp::builtin_mcp::{execute_aipp_builtin_tool, is_builtin_mcp_call};
use crate::mcp::mcp_db::{MCPDatabase, MCPServer, MCPToolCall};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::{
    model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation},
    transport::{
        sse_client::SseClientConfig, streamable_http_client::StreamableHttpClientTransportConfig,
        ConfigureCommandExt, SseClientTransport, StreamableHttpClientTransport, TokioChildProcess,
    },
    ServiceExt,
};
use serde_json::Map as JsonMap;
use tauri::Emitter;
use tokio::process::Command;
use tracing::{debug, error, info, instrument, warn};

// =============================
// 常量 & 公共类型
// =============================

/// 各种传输方式统一使用的默认超时时间（毫秒）
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

// =============================
// 公共辅助函数 (参数解析 / 请求构建 / 结果提取)
// =============================

/// 解析工具参数字符串为 JSON 对象 Map。允许带有 ``` 包裹并做容错。
fn parse_tool_arguments(parameters: &str) -> Result<JsonMap<String, serde_json::Value>> {
    let params_clean = normalize_parameters_json(parameters);
    let params_value: serde_json::Value =
        serde_json::from_str(&params_clean).with_context(|| "解析参数 JSON 失败")?;
    match params_value {
        serde_json::Value::Object(map) => Ok(map),
        _ => bail!("参数必须是 JSON 对象"),
    }
}

/// 构建 `CallToolRequestParam` 结构。
fn build_call_tool_request(
    tool_name: &str,
    arguments: JsonMap<String, serde_json::Value>,
) -> CallToolRequestParam {
    CallToolRequestParam { name: tool_name.to_string().into(), arguments: Some(arguments) }
}

/// 根据响应结构统一提取并序列化返回内容；若标记错误则返回 Err。
/// 注意：部分 MCP 服务器可能返回包含二进制/不可序列化字段的内容（例如嵌入资源、blob 等），
/// 这会导致直接 JSON 序列化失败。为保证稳健性：
/// 1) 优先尝试将 content 做 JSON 序列化；
/// 2) 若失败，则回退为 Debug 字符串拼接，并进行长度截断，避免超大结果引发内存压力。
fn serialize_tool_response(response: &rmcp::model::CallToolResult) -> Result<String> {
    // 处理服务器标记的错误
    if response.is_error.unwrap_or(false) {
        return Err(anyhow!("工具执行错误: {:?}", response.content));
    }

    // 优先尝试 JSON 序列化（与之前行为保持一致）
    match serde_json::to_string(&response.content) {
        Ok(s) => Ok(s),
        Err(json_err) => {
            // 回退：使用 Debug 格式串联各个 content part，确保始终有可显示的结果
            const MAX_LEN: usize = 100_000; // 保护上限，避免过大输出
            let joined = response
                .content
                .iter()
                .map(|part| format!("{:?}", part))
                .collect::<Vec<_>>()
                .join("\n");

            let truncated = if joined.len() > MAX_LEN {
                let mut s = joined;
                s.truncate(MAX_LEN);
                s.push_str("\n...[truncated]...");
                s
            } else {
                joined
            };

            // 记录一次警告日志，便于后续针对特定服务器/工具做更细化的适配
            warn!(error=%json_err, content_len=truncated.len(), "fallback to debug-string for tool response serialization");
            Ok(truncated)
        }
    }
}

// MCP Tool Execution API

// 发送MCP工具调用状态更新事件
/// 向前端发送工具调用状态更新事件（包括执行中 / 成功 / 失败）。
fn emit_mcp_tool_call_update(
    window: &tauri::Window,
    conversation_id: i64,
    tool_call: &MCPToolCall,
) {
    let update_event = ConversationEvent {
        r#type: "mcp_tool_call_update".to_string(),
        data: serde_json::to_value(MCPToolCallUpdateEvent {
            call_id: tool_call.id,
            conversation_id: tool_call.conversation_id,
            status: tool_call.status.clone(),
            result: tool_call.result.clone(),
            error: tool_call.error.clone(),
            started_time: tool_call.started_time.as_ref().map(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .unwrap_or_else(|_| chrono::Utc::now().into())
                    .with_timezone(&chrono::Utc)
            }),
            finished_time: tool_call.finished_time.as_ref().map(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .unwrap_or_else(|_| chrono::Utc::now().into())
                    .with_timezone(&chrono::Utc)
            }),
        })
        .unwrap(),
    };

    let _ = window.emit(format!("conversation_event_{}", conversation_id).as_str(), update_event);
}

// 验证工具调用是否可以执行
/// 校验当前工具调用是否允许执行；返回是否属于重试场景。
fn validate_tool_call_execution(tool_call: &MCPToolCall) -> Result<bool> {
    let is_retry = tool_call.status == "failed";
    if tool_call.status != "pending" && tool_call.status != "failed" {
        bail!("工具调用状态为 {} 时无法重新执行", tool_call.status);
    }
    Ok(is_retry)
}

// 验证服务器状态
/// 校验服务器是否启用。
fn validate_server_status(server: &MCPServer) -> Result<()> {
    if !server.is_enabled {
        bail!("MCP服务器已禁用");
    }
    Ok(())
}

// 处理工具执行结果
/// 根据执行结果更新状态并尝试触发会话续写。即使续写失败也不影响主执行成功标记。
#[instrument(skip(app_handle,state,feature_config_state,window,tool_call,execution_result), fields(call_id=call_id, conversation_id=?tool_call.conversation_id, retry=?is_retry))]
async fn handle_tool_execution_result(
    app_handle: &tauri::AppHandle,
    state: &tauri::State<'_, crate::AppState>,
    feature_config_state: &tauri::State<'_, crate::FeatureConfigState>,
    window: &tauri::Window,
    call_id: i64,
    mut tool_call: MCPToolCall,
    execution_result: std::result::Result<String, String>,
    is_retry: bool,
) -> Result<MCPToolCall, String> {
    let db = MCPDatabase::new(app_handle).map_err(|e| format!("初始化数据库失败: {}", e))?;

    match execution_result {
        Ok(result) => {
            info!(tool_call_id=tool_call.id, tool_name=%tool_call.tool_name, server=%tool_call.server_name, "工具执行成功");

            db.update_mcp_tool_call_status(call_id, "success", Some(&result), None)
                .map_err(|e| format!("更新工具调用状态失败: {}", e))?;

            tool_call.status = "success".to_string();
            tool_call.result = Some(result.clone());
            tool_call.error = None;

            emit_mcp_tool_call_update(window, tool_call.conversation_id, &tool_call);

            // 处理对话继续逻辑
            if let Err(e) = handle_tool_success_continuation(
                app_handle,
                state,
                feature_config_state,
                window,
                &tool_call,
                &result,
                is_retry,
            )
            .await
            {
                warn!(error=%e, "tool execution succeeded but continuation failed");
            }
        }
        Err(error) => {
            error!(tool_call_id=tool_call.id, tool_name=%tool_call.tool_name, server=%tool_call.server_name, %error, "tool execution failed");

            db.update_mcp_tool_call_status(call_id, "failed", None, Some(&error))
                .map_err(|e| format!("更新工具调用状态失败: {}", e))?;

            tool_call.status = "failed".to_string();
            tool_call.error = Some(error);
            tool_call.result = None;

            emit_mcp_tool_call_update(window, tool_call.conversation_id, &tool_call);
        }
    }

    Ok(tool_call)
}

/// 规范化从 LLM 返回的 parameters JSON，移除可能的 markdown 代码块包裹。
fn normalize_parameters_json(parameters: &str) -> String {
    let trimmed = parameters.trim();
    if trimmed.starts_with("```") {
        // 去掉首尾 ```，并移除可能的语言标识（如 ```json）
        let without_start = trimmed.trim_start_matches("```");
        // 可能存在语言标签，截到首个换行
        let without_lang = match without_start.find('\n') {
            Some(idx) => &without_start[idx + 1..],
            None => without_start,
        };
        let without_end = without_lang.trim_end_matches("```").trim();
        without_end.to_string()
    } else {
        trimmed.to_string()
    }
}

#[tauri::command]
/// 创建一条 MCP 工具调用记录。若提供 `llm_call_id` 或 `assistant_message_id` 会写入相应字段。
#[instrument(skip(app_handle, parameters), fields(server_name=%server_name, tool_name=%tool_name, conversation_id=conversation_id))]
pub async fn create_mcp_tool_call(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    message_id: Option<i64>,
    server_name: String,
    tool_name: String,
    parameters: String,
    llm_call_id: Option<String>,
    assistant_message_id: Option<i64>,
) -> std::result::Result<MCPToolCall, String> {
    let db = MCPDatabase::new(&app_handle).map_err(|e| format!("初始化数据库失败: {}", e))?;

    // 查找并验证服务器
    let servers = db.get_mcp_servers().map_err(|e| format!("获取MCP服务器列表失败: {}", e))?;
    let server = servers
        .iter()
        .find(|s| s.name == server_name && s.is_enabled)
        .ok_or_else(|| format!("服务器 '{}' 未找到或已禁用", server_name))?;

    // 根据是否提供 llm_call_id 选择相应的创建方法
    let tool_call = if llm_call_id.is_some() || assistant_message_id.is_some() {
        db.create_mcp_tool_call_with_llm_id(
            conversation_id,
            message_id,
            server.id,
            &server_name,
            &tool_name,
            &parameters,
            llm_call_id.as_deref(),
            assistant_message_id,
        )
    } else {
        db.create_mcp_tool_call(
            conversation_id,
            message_id,
            server.id,
            &server_name,
            &tool_name,
            &parameters,
        )
    };

    let result = tool_call.map_err(|e| format!("创建MCP工具调用失败: {}", e))?;

    Ok(result)
}

/// 兼容旧签名的创建函数，内部委托到新版实现。
#[instrument(skip(app_handle, parameters))]
pub async fn create_mcp_tool_call_with_llm_id(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    message_id: Option<i64>,
    server_name: String,
    tool_name: String,
    parameters: String,
    llm_call_id: Option<&str>,
    assistant_message_id: Option<i64>,
) -> std::result::Result<MCPToolCall, String> {
    create_mcp_tool_call(
        app_handle,
        conversation_id,
        message_id,
        server_name,
        tool_name,
        parameters,
        llm_call_id.map(|s| s.to_string()),
        assistant_message_id,
    )
    .await
}

#[tauri::command]
/// 执行指定 ID 的工具调用：
/// 1. 原子更新状态到 executing
/// 2. 按服务器传输类型派发执行
/// 3. 持久化结果并触发续写
#[instrument(skip(app_handle,state,feature_config_state,window), fields(call_id=call_id))]
pub async fn execute_mcp_tool_call(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    feature_config_state: tauri::State<'_, crate::FeatureConfigState>,
    window: tauri::Window,
    call_id: i64,
) -> std::result::Result<MCPToolCall, String> {
    let db = MCPDatabase::new(&app_handle).map_err(|e| format!("初始化数据库失败: {}", e))?;

    // 获取工具调用信息
    let mut tool_call =
        db.get_mcp_tool_call(call_id).map_err(|e| format!("获取工具调用信息失败: {}", e))?;
    debug!(tool_call_name=%tool_call.tool_name, tool_call_server=%tool_call.server_name, tool_call_params=%tool_call.parameters, "准备调用的mcp工具");

    // 验证工具调用状态
    let is_retry = validate_tool_call_execution(&tool_call).map_err(|e| e.to_string())?;
    debug!(retry=?is_retry, status=%tool_call.status, "validated tool call status");

    // 获取并验证服务器状态
    let server = db
        .get_mcp_server(tool_call.server_id)
        .map_err(|e| format!("获取MCP服务器信息失败: {}", e))?;
    validate_server_status(&server).map_err(|e| e.to_string())?;
    debug!(server_id=server.id, transport=%server.transport_type, "server validated");

    // 原子性地将状态转为执行中，避免并发重复执行
    if !db
        .mark_mcp_tool_call_executing_if_pending(call_id)
        .map_err(|e| format!("更新工具调用状态失败: {}", e))?
    {
        let current = db
            .get_mcp_tool_call(call_id)
            .map_err(|e| format!("获取当前工具调用状态失败: {}", e))?;
        return Ok(current);
    }

    // 重新加载工具调用以获取更新后的状态并发送事件
    tool_call =
        db.get_mcp_tool_call(call_id).map_err(|e| format!("重新加载工具调用信息失败: {}", e))?;
    emit_mcp_tool_call_update(&window, tool_call.conversation_id, &tool_call);
    debug!(call_id=call_id, status=%tool_call.status, "emitted executing status event");

    // 执行工具
    let execution_result = execute_tool_by_transport(
        &app_handle,
        &server,
        &tool_call.tool_name,
        &tool_call.parameters,
    )
    .await;

    // 处理执行结果
    handle_tool_execution_result(
        &app_handle,
        &state,
        &feature_config_state,
        &window,
        call_id,
        tool_call,
        execution_result,
        is_retry,
    )
    .await
}

#[tauri::command]
/// 获取单个工具调用。
#[instrument(skip(app_handle))]
pub async fn get_mcp_tool_call(
    app_handle: tauri::AppHandle,
    call_id: i64,
) -> std::result::Result<MCPToolCall, String> {
    let db = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.get_mcp_tool_call(call_id).map_err(|e| e.to_string())
}

#[tauri::command]
/// 根据会话 ID 获取全部工具调用记录。
#[instrument(skip(app_handle))]
pub async fn get_mcp_tool_calls_by_conversation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> std::result::Result<Vec<MCPToolCall>, String> {
    let db = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.get_mcp_tool_calls_by_conversation(conversation_id).map_err(|e| e.to_string())
}

/// 工具成功后的续写逻辑调度：区分首次与重试。
#[instrument(skip(app_handle,state,feature_config_state,window,tool_call,result), fields(call_id=tool_call.id, conversation_id=tool_call.conversation_id, retry=?is_retry))]
async fn handle_tool_success_continuation(
    app_handle: &tauri::AppHandle,
    state: &tauri::State<'_, crate::AppState>,
    feature_config_state: &tauri::State<'_, crate::FeatureConfigState>,
    window: &tauri::Window,
    tool_call: &MCPToolCall,
    result: &str,
    is_retry: bool,
) -> Result<()> {
    if is_retry {
        // For retries, we need to update the existing tool_result message instead of creating a new one
        handle_retry_success_continuation(
            app_handle,
            state,
            feature_config_state,
            window,
            tool_call,
            result,
        )
        .await
    } else {
        // For first-time execution, use the original logic
        trigger_conversation_continuation(
            app_handle,
            state,
            feature_config_state,
            window,
            tool_call,
            result,
        )
        .await
    }
}

/// 处理重试成功的情况：更新现有工具结果消息并触发新的AI响应
/// 重试成功：若存在旧的 tool_result 消息则更新其内容，然后统一触发续写。
#[instrument(skip(app_handle,state,feature_config_state,window,tool_call,result), fields(call_id=tool_call.id))]
async fn handle_retry_success_continuation(
    app_handle: &tauri::AppHandle,
    state: &tauri::State<'_, crate::AppState>,
    feature_config_state: &tauri::State<'_, crate::FeatureConfigState>,
    window: &tauri::Window,
    tool_call: &MCPToolCall,
    result: &str,
) -> Result<()> {
    let conversation_db = ConversationDatabase::new(app_handle).context("初始化对话数据库失败")?;

    // 更新现有的 tool_result 消消息在数据库中（用于记录保存）
    let messages = conversation_db
        .message_repo()
        .unwrap()
        .list_by_conversation_id(tool_call.conversation_id)
        .map_err(|e| anyhow!("获取对话消息列表失败: {}", e))?;

    // 查找与此工具调用匹配的现有 tool_result 消息
    let existing_tool_message = messages.into_iter().find(|(msg, _)| {
        msg.message_type == "tool_result"
            && msg.content.contains(&format!("Tool Call ID: {}", tool_call.id))
    });

    let updated_tool_result_content = format!(
        "Tool execution completed:\n\nTool Call ID: {}\nTool: {}\nServer: {}\nParameters: {}\nResult:\n{}",
        tool_call.id,
        tool_call.tool_name,
        tool_call.server_name,
        tool_call.parameters,
        result
    );

    match existing_tool_message {
        Some((mut existing_msg, _)) => {
            // 更新现有的 tool_result 消息在数据库中
            existing_msg.content = updated_tool_result_content;
            conversation_db
                .message_repo()
                .unwrap()
                .update(&existing_msg)
                .map_err(|e| anyhow!("更新工具结果消息失败: {}", e))?;
            debug!(call_id = tool_call.id, "updated existing tool_result message on retry");
        }
        None => {
            debug!(call_id = tool_call.id, "no existing tool_result message found on retry");
        }
    }
    // 统一在末尾触发对话继续
    trigger_conversation_continuation(
        app_handle,
        state,
        feature_config_state,
        window,
        tool_call,
        result,
    )
    .await
}

/// 触发会话继续：把工具结果作为 tool_result 语义传递给 AI 继续生成。
#[instrument(skip(app_handle,state,feature_config_state,window,tool_call,result), fields(call_id=tool_call.id, conversation_id=tool_call.conversation_id))]
async fn trigger_conversation_continuation(
    app_handle: &tauri::AppHandle,
    state: &tauri::State<'_, crate::AppState>,
    feature_config_state: &tauri::State<'_, crate::FeatureConfigState>,
    window: &tauri::Window,
    tool_call: &MCPToolCall,
    result: &str,
) -> Result<()> {
    let conversation_db = ConversationDatabase::new(app_handle).context("初始化对话数据库失败")?;

    // 获取对话详情
    let conversation = conversation_db
        .conversation_repo()
        .unwrap()
        .read(tool_call.conversation_id)
        .map_err(|e| anyhow!("获取对话信息失败: {}", e))?
        .ok_or_else(|| anyhow!("未找到对话"))?;

    let assistant_id = conversation.assistant_id.ok_or_else(|| anyhow!("对话未关联助手"))?;

    // 使用数据库中保存的 llm_call_id（若存在），否则退回到兼容格式
    let tool_call_id =
        tool_call.llm_call_id.clone().unwrap_or_else(|| format!("mcp_tool_call_{}", tool_call.id));

    // 调用 tool_result_continue_ask_ai 以工具结果继续对话
    tool_result_continue_ask_ai(
        app_handle.clone(),
        state.clone(),
        feature_config_state.clone(),
        window.clone(),
        tool_call.conversation_id.to_string(),
        assistant_id,
        tool_call_id,
        result.to_string(),
    )
    .await
    .map_err(|e| anyhow!("触发对话继续失败: {:?}", e))?;

    info!(
        call_id = tool_call.id,
        conversation_id = tool_call.conversation_id,
        "triggered conversation continuation"
    );
    Ok(())
}

/// 统一的工具执行函数，根据传输类型选择相应的执行策略（公开供子任务复用）
/// 根据服务器配置的传输类型选择执行方式。
#[instrument(skip(app_handle,server,parameters), fields(server_id=server.id, transport=%server.transport_type, tool_name=%tool_name))]
pub async fn execute_tool_by_transport(
    app_handle: &tauri::AppHandle,
    server: &MCPServer,
    tool_name: &str,
    parameters: &str,
) -> std::result::Result<String, String> {
    match server.transport_type.as_str() {
        // If stdio but command is aipp:*, route to builtin executor
        "stdio" => {
            if let Some(cmd) = &server.command {
                if crate::mcp::builtin_mcp::is_builtin_mcp_call(cmd) {
                    execute_builtin_tool(app_handle, server, tool_name, parameters)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    execute_stdio_tool(app_handle, server, tool_name, parameters)
                        .await
                        .map_err(|e| e.to_string())
                }
            } else {
                execute_stdio_tool(app_handle, server, tool_name, parameters)
                    .await
                    .map_err(|e| e.to_string())
            }
        }
        "sse" => execute_sse_tool(app_handle, server, tool_name, parameters)
            .await
            .map_err(|e| e.to_string()),
        "http" => execute_http_tool(app_handle, server, tool_name, parameters)
            .await
            .map_err(|e| e.to_string()),
        // Legacy builtin type is no longer used, but keep for backward compatibility
        "builtin" => execute_builtin_tool(app_handle, server, tool_name, parameters)
            .await
            .map_err(|e| e.to_string()),
        _ => {
            let error_msg = format!("不支持的传输类型: {}", server.transport_type);
            Err(error_msg)
        }
    }
}
// Reuse shared MCP header parsing utilities
use crate::mcp::util::{parse_server_headers, sanitize_headers_for_log};
/// 通过 stdio 传输执行工具（外部进程）。
#[instrument(skip(app_handle,server,parameters), fields(server_id=server.id, tool_name=%tool_name))]
async fn execute_stdio_tool(
    app_handle: &tauri::AppHandle,
    server: &MCPServer,
    tool_name: &str,
    parameters: &str,
) -> Result<String> {
    // builtin 透传
    if let Some(cmd) = &server.command {
        if crate::mcp::builtin_mcp::is_builtin_mcp_call(cmd) {
            return execute_builtin_tool(app_handle, server, tool_name, parameters).await;
        }
    }

    let command = server.command.as_ref().ok_or_else(|| anyhow!("未为 stdio 传输指定命令"))?;
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        bail!("命令为空");
    }

    let timeout_ms = server.timeout.map(|v| v as u64).unwrap_or(DEFAULT_TIMEOUT_MS);
    let start = std::time::Instant::now();
    let client_result = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let client = (())
            .serve(
                TokioChildProcess::new(Command::new(parts[0]).configure(|cmd| {
                    if parts.len() > 1 {
                        cmd.args(&parts[1..]);
                    }
                    if let Some(env_vars) = &server.environment_variables {
                        for line in env_vars.lines() {
                            if let Some((key, value)) = line.split_once('=') {
                                cmd.env(key.trim(), value.trim());
                            }
                        }
                    }
                }))
                .context("创建子进程失败")?,
            )
            .await
            .context("初始化客户端失败")?;

        let args = parse_tool_arguments(parameters).context("解析工具参数失败")?;
        let request_param = build_call_tool_request(tool_name, args);
        let response = client.call_tool(request_param).await.context("工具调用失败")?;
        debug!(is_error=?response.is_error, parts=?response.content.len(), "received stdio tool response");
        client.cancel().await.context("关闭客户端连接失败")?;
        // 不再包裹错误上下文，直接返回底层错误以保留真实原因（例如服务器返回的错误信息）
        serialize_tool_response(&response)
    })
    .await;

    match client_result {
        Ok(Ok(r)) => {
            info!(elapsed_ms=?start.elapsed().as_millis(), "stdio tool executed successfully");
            Ok(r)
        }
        Ok(Err(e)) => {
            error!(elapsed_ms=?start.elapsed().as_millis(), error=%e, "stdio tool execution failed");
            Err(anyhow!("工具执行失败: {}", e))
        }
        Err(_) => {
            error!(elapsed_ms=?start.elapsed().as_millis(), timeout_ms=timeout_ms, "stdio tool execution timeout");
            Err(anyhow!("工具执行超时"))
        }
    }
}

/// 通过 SSE 传输执行工具。
#[instrument(skip(server,parameters), fields(server_id=server.id, tool_name=%tool_name))]
async fn execute_sse_tool(
    _app_handle: &tauri::AppHandle,
    server: &MCPServer,
    tool_name: &str,
    parameters: &str,
) -> Result<String> {
    let url = server.url.as_ref().ok_or_else(|| anyhow!("No URL specified for SSE transport"))?;

    // Build SSE transport with a preconfigured reqwest client (propagate all headers)
    let (_auth_header, __all_headers_for_sse) = parse_server_headers(server);
    let sse_transport = if let Some(hdrs) = __all_headers_for_sse {
        // log sanitized headers
        let to_log = sanitize_headers_for_log(&hdrs);
        info!(server_id = server.id, headers = ?to_log, "Using SSE headers");
        let mut header_map = HeaderMap::new();
        for (k, v) in hdrs.iter() {
            if let (Ok(name), Ok(value)) =
                (HeaderName::try_from(k.as_str()), HeaderValue::from_str(v.as_str()))
            {
                header_map.insert(name, value);
            }
        }
        let client = reqwest::Client::builder()
            .default_headers(header_map)
            .build()
            .context("Failed to build reqwest client for SSE")?;
        // Requires rmcp feature `transport-sse-client-reqwest`
        SseClientTransport::start_with_client(
            client,
            SseClientConfig { sse_endpoint: url.as_str().into(), ..Default::default() },
        )
        .await
        .context("Failed to start SSE transport with client")?
    } else {
        SseClientTransport::start(url.as_str()).await.context("Failed to start SSE transport")?
    };

    // TODO: SSE 传输暂未支持将 Authorization 注入底层 rmcp 客户端（rmcp 默认 SseClientTransport::start 内部不暴露 token 参数）。
    // 后续可通过实现自定义 SseClient 并启用 transport-sse-client-reqwest 特性来完成。当前仅解析保留以便未来使用。
    let (_auth_header, _all) = parse_server_headers(server);

    let start = std::time::Instant::now();
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(
            server.timeout.map(|v| v as u64).unwrap_or(DEFAULT_TIMEOUT_MS),
        ),
        async move {
            let transport = sse_transport;
            let client_info = ClientInfo {
                protocol_version: Default::default(),
                capabilities: ClientCapabilities::default(),
                client_info: Implementation {
                    name: "AIPP MCP SSE Client".to_string(),
                    version: "0.1.0".to_string(),
                    ..Default::default()
                },
            };
            let client =
                client_info.serve(transport).await.context("Failed to initialize SSE client")?;
            let args = parse_tool_arguments(parameters).context("解析工具参数失败")?;
            let request_param = build_call_tool_request(tool_name, args);

            // TODO: rmcp 当前 SseClientTransport::send 未暴露 auth_token; 通过自定义 client 已用于初始化，后续调用暂不重复 header
            let response = client.call_tool(request_param).await.context("Tool call failed")?;
            debug!(is_error=?response.is_error, parts=?response.content.len(), "received sse tool response");

            // Cancel the client connection
            client.cancel().await.context("Failed to cancel client")?;

            // 不包裹序列化错误上下文，避免将服务器端错误误标为“序列化失败”
            serialize_tool_response(&response)
        },
    )
    .await;

    match client_result {
        Ok(Ok(result)) => {
            info!(elapsed_ms=?start.elapsed().as_millis(), "sse tool executed successfully");
            Ok(result)
        }
        Ok(Err(e)) => {
            error!(elapsed_ms=?start.elapsed().as_millis(), error=%e, "sse tool execution failed");
            Err(anyhow!("Tool execution failed: {}", e))
        }
        Err(_) => {
            error!(elapsed_ms=?start.elapsed().as_millis(), timeout_ms=server.timeout.map(|v| v as u64).unwrap_or(DEFAULT_TIMEOUT_MS), "sse tool execution timeout");
            Err(anyhow!("Timeout while executing tool"))
        }
    }
}

/// 执行内置（aipp:*）工具：不经网络，直接在本地实现。
#[instrument(skip(app_handle,server,parameters), fields(server_id=server.id, tool_name=%tool_name))]
async fn execute_builtin_tool(
    app_handle: &tauri::AppHandle,
    server: &MCPServer,
    tool_name: &str,
    parameters: &str,
) -> Result<String> {
    // 验证是否为内置工具调用
    let command = server.command.clone().unwrap_or_default();
    if !is_builtin_mcp_call(&command) {
        error!(command=%command, "invalid builtin tool command");
        bail!("Unknown builtin tool: {} for command: {}", tool_name, command);
    }

    // 通过 rmcp 的内置服务执行工具，并规范化返回为 content JSON 字符串
    // 注意：内置工具入口当前接受原始字符串，因此这里仍传入 normalize 后的原始 JSON 文本；
    // parse_tool_arguments 在 builtin 情况下不需要提前结构化（保持行为一致）。
    let raw = execute_aipp_builtin_tool(
        app_handle.clone(),
        command.clone(),
        tool_name.to_string(),
        normalize_parameters_json(parameters),
    )
    .await
    .map_err(|e| anyhow!(e))?; // map String error to anyhow

    // raw 是序列化后的 ToolResult，提取其中的 content 字段以与其他传输保持一致
    let v: serde_json::Value = serde_json::from_str(&raw).context("解析内置工具结果失败")?;
    let is_error =
        v.get("is_error").or_else(|| v.get("isError")).and_then(|x| x.as_bool()).unwrap_or(false);
    if is_error {
        error!(tool_name=%tool_name, "builtin tool returned error flag");
        bail!("工具执行错误: {}", v.get("content").unwrap_or(&serde_json::Value::Null));
    }
    let content = v.get("content").cloned().unwrap_or(serde_json::Value::Null);
    Ok(serde_json::to_string(&content).context("序列化结果失败")?)
}

/// 通过 HTTP (streamable) 传输执行工具。
#[instrument(skip(server,parameters), fields(server_id=server.id, tool_name=%tool_name))]
async fn execute_http_tool(
    _app_handle: &tauri::AppHandle,
    server: &MCPServer,
    tool_name: &str,
    parameters: &str,
) -> Result<String> {
    let url = server.url.as_ref().ok_or_else(|| anyhow!("No URL specified for HTTP transport"))?;

    // 解析自定义头，仅支持将 Authorization 传入 auth_header（其余头部未来可通过自定义 client 实现）
    let (auth_header, _all) = parse_server_headers(server);
    let mut config = StreamableHttpClientTransportConfig::with_uri(url.as_str());
    if let Some(auth) = auth_header.clone() {
        config = config.auth_header(auth);
    }
    // 使用新版 rmcp 提供的便捷方法（reqwest feature 已启用）
    let transport = {
        use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
        let (_auth_header, all_headers) = parse_server_headers(server);
        let mut header_map = HeaderMap::new();
        if let Some(hdrs) = all_headers.as_ref() {
            let to_log = sanitize_headers_for_log(hdrs);
            info!(server_id = server.id, headers = ?to_log, "Using HTTP headers");
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
            .context("Failed to build reqwest client for HTTP")?;
        StreamableHttpClientTransport::with_client(client, config)
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

    let start = std::time::Instant::now();
    let client_result = tokio::time::timeout(
        std::time::Duration::from_millis(
            server.timeout.map(|v| v as u64).unwrap_or(DEFAULT_TIMEOUT_MS),
        ),
        async move {
            let client =
                client_info.serve(transport).await.context("Failed to initialize HTTP client")?;
            let args = parse_tool_arguments(parameters).context("解析工具参数失败")?;
            let request_param = build_call_tool_request(tool_name, args);

            let response = client.call_tool(request_param).await.context("Tool call failed")?;
            debug!(is_error=?response.is_error, parts=?response.content.len(), "received http tool response");

            // Cancel the client connection
            client.cancel().await.context("Failed to cancel client")?;

            serialize_tool_response(&response)
        },
    )
    .await;

    match client_result {
        Ok(Ok(result)) => {
            info!(elapsed_ms=?start.elapsed().as_millis(), "http tool executed successfully");
            Ok(result)
        }
        Ok(Err(e)) => {
            error!(elapsed_ms=?start.elapsed().as_millis(), error=%e, "http tool execution failed");
            Err(anyhow!("Tool execution failed: {}", e))
        }
        Err(_) => {
            error!(elapsed_ms=?start.elapsed().as_millis(), timeout_ms=server.timeout.map(|v| v as u64).unwrap_or(DEFAULT_TIMEOUT_MS), "http tool execution timeout");
            Err(anyhow!("Timeout while executing tool"))
        }
    }
}

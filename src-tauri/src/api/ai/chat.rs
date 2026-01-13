use crate::api::ai::config::{calculate_retry_delay, get_retry_attempts_from_config};
use crate::api::ai::events::{ConversationEvent, MessageAddEvent, MessageUpdateEvent};
use crate::api::ai::types::McpOverrideConfig;
use crate::api::ai_api::{resolve_tool_name, sanitize_tool_name, ToolNameMapping};
use crate::db::assistant_db::Assistant;
use crate::db::conversation_db::{ConversationDatabase, Message, Repository};
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::utils::window_utils::send_error_to_appropriate_window;
use anyhow::Context as _;
use futures::StreamExt;
use genai::chat::ChatStreamEvent;
use genai::chat::{ChatOptions, ChatRequest, ToolCall};
use genai::Client;
use serde_json;
use std::collections::HashMap;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// HTTP 错误详情，包含状态码、响应体、端点等
#[derive(Debug, Clone, Default)]
pub struct HttpErrorDetails {
    pub status_code: Option<u16>,
    pub response_body: Option<String>,
    pub endpoint: Option<String>,
    pub request_id: Option<String>,
}

/// 从 genai::Error 中提取 HTTP 错误详情
fn extract_http_error_details(error: &genai::Error) -> HttpErrorDetails {
    let mut details = HttpErrorDetails::default();

    match error {
        // WebModelCall 和 WebAdapterCall 包含 webc_error
        genai::Error::WebModelCall { model_iden, webc_error } => {
            details.endpoint = Some(format!("{:?}", model_iden));
            extract_webc_error_details(webc_error, &mut details);
        }
        genai::Error::WebAdapterCall { adapter_kind, webc_error } => {
            details.endpoint = Some(format!("{:?}", adapter_kind));
            extract_webc_error_details(webc_error, &mut details);
        }
        // ChatResponse 包含 body (JSON Value)
        genai::Error::ChatResponse { model_iden, body } => {
            details.endpoint = Some(format!("{:?}", model_iden));
            details.response_body =
                Some(serde_json::to_string_pretty(body).unwrap_or_else(|_| body.to_string()));
            // 尝试从 body 中提取状态码和请求ID
            if let Some(error_obj) = body.get("error") {
                if let Some(code) = error_obj.get("code").and_then(|c| c.as_str()) {
                    // 常见的错误代码映射到 HTTP 状态码
                    details.status_code = match code {
                        "invalid_api_key" | "authentication_error" => Some(401),
                        "insufficient_quota" | "rate_limit_exceeded" => Some(429),
                        "model_not_found" => Some(404),
                        "context_length_exceeded" | "invalid_request_error" => Some(400),
                        "server_error" => Some(500),
                        _ => None,
                    };
                }
            }
            // 提取 request_id
            if let Some(req_id) = body.get("request_id").and_then(|r| r.as_str()) {
                details.request_id = Some(req_id.to_string());
            }
        }
        // WebStream 错误
        genai::Error::WebStream { model_iden, cause } => {
            details.endpoint = Some(format!("{:?}", model_iden));
            // 尝试从 cause 字符串中解析状态码
            parse_status_from_string(cause, &mut details);
        }
        // ReqwestEventSource 错误
        genai::Error::ReqwestEventSource(es_error) => {
            let error_str = format!("{:?}", es_error);
            parse_status_from_string(&error_str, &mut details);
        }
        _ => {
            // 对于其他错误，尝试从错误字符串解析
            let error_str = error.to_string();
            parse_status_from_string(&error_str, &mut details);
        }
    }

    details
}

/// 从 webc::Error 中提取详情
fn extract_webc_error_details(webc_error: &genai::webc::Error, details: &mut HttpErrorDetails) {
    match webc_error {
        genai::webc::Error::ResponseFailedStatus { status, body, headers: _ } => {
            details.status_code = Some(status.as_u16());
            details.response_body = Some(body.clone());
            // 尝试从响应体中解析更多信息
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                if let Some(req_id) = json.get("request_id").and_then(|r| r.as_str()) {
                    details.request_id = Some(req_id.to_string());
                }
            }
        }
        genai::webc::Error::ResponseFailedNotJson { content_type } => {
            details.response_body =
                Some(format!("响应不是有效的 JSON，Content-Type: {}", content_type));
        }
        genai::webc::Error::Reqwest(reqwest_error) => {
            if let Some(status) = reqwest_error.status() {
                details.status_code = Some(status.as_u16());
            }
            if let Some(url) = reqwest_error.url() {
                details.endpoint = Some(url.to_string());
            }
        }
        _ => {}
    }
}

/// 从错误字符串中解析状态码
fn parse_status_from_string(error_str: &str, details: &mut HttpErrorDetails) {
    // 尝试匹配常见的状态码模式
    let status_patterns = [
        ("400", 400u16),
        ("401", 401),
        ("403", 403),
        ("404", 404),
        ("429", 429),
        ("500", 500),
        ("502", 502),
        ("503", 503),
    ];

    for (pattern, code) in status_patterns {
        if error_str.contains(pattern) && details.status_code.is_none() {
            details.status_code = Some(code);
            break;
        }
    }

    // 尝试提取 URL
    if details.endpoint.is_none() {
        if let Some(start) = error_str.find("https://") {
            let remaining = &error_str[start..];
            let end = remaining
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '>')
                .unwrap_or(remaining.len());
            details.endpoint = Some(remaining[..end].to_string());
        }
    }

    // 尝试提取 JSON 响应体
    if details.response_body.is_none() {
        if let Some(start) = error_str.find('{') {
            if let Some(end) = error_str.rfind('}') {
                if end > start {
                    let json_str = &error_str[start..=end];
                    // 验证是否为有效 JSON
                    if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                        details.response_body = Some(json_str.to_string());
                    }
                }
            }
        }
    }
}

/// 从 anyhow::Error 链中尝试提取 HTTP 错误详情
fn extract_http_details_from_anyhow(error: &anyhow::Error) -> HttpErrorDetails {
    // 首先尝试直接转换为 genai::Error
    if let Some(genai_error) = error.downcast_ref::<genai::Error>() {
        return extract_http_error_details(genai_error);
    }

    // 遍历错误链
    let mut details = HttpErrorDetails::default();
    let error_string = error.to_string();

    // 从错误字符串中解析
    parse_status_from_string(&error_string, &mut details);

    // 遍历 source 链
    let mut current: Option<&(dyn std::error::Error + 'static)> = error.source();
    while let Some(source) = current {
        // 尝试转换为 genai::Error
        if let Some(genai_error) = source.downcast_ref::<genai::Error>() {
            let genai_details = extract_http_error_details(genai_error);
            // 合并详情（优先使用已有的非空值）
            if details.status_code.is_none() {
                details.status_code = genai_details.status_code;
            }
            if details.response_body.is_none() {
                details.response_body = genai_details.response_body;
            }
            if details.endpoint.is_none() {
                details.endpoint = genai_details.endpoint;
            }
            if details.request_id.is_none() {
                details.request_id = genai_details.request_id;
            }
            break;
        }

        // 尝试从 source 错误字符串中解析
        let source_str = source.to_string();
        if details.status_code.is_none() || details.response_body.is_none() {
            parse_status_from_string(&source_str, &mut details);
        }

        current = source.source();
    }

    details
}

/// 删除会话中最后一条错误消息（如果最后一条是 error）
async fn cleanup_last_error_message(
    conversation_db: &ConversationDatabase,
    conversation_id: i64,
) -> anyhow::Result<()> {
    // 读取该会话的所有消息（含附件信息）
    let messages = conversation_db
        .message_repo()
        .context("failed to get message_repo for cleanup")?
        .list_by_conversation_id(conversation_id)
        .context("failed to list messages for cleanup")?;

    // 找到 id 最大的消息
    if let Some((last_msg, _)) = messages.iter().max_by_key(|(m, _)| m.id).cloned() {
        if last_msg.message_type == "error" {
            // 删除该错误消息
            let _ = conversation_db
                .message_repo()
                .context("failed to get message_repo for delete")?
                .delete(last_msg.id);
        }
    }

    Ok(())
}

/// 发送消息完成通知
async fn send_completion_notification(
    app_handle: &tauri::AppHandle,
    content: &str,
    assistant_name: Option<String>,
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) {
    // 检查通知是否启用
    if let Some(display_config) = config_feature_map.get("display") {
        if let Some(FeatureConfig { value, .. }) = display_config.get("notification_on_completion")
        {
            if value == "true" {
                // 检查 chat 和 ask 窗口是否有任何一个聚焦
                // 如果有窗口聚焦，则不发送通知
                if crate::utils::window_utils::is_chat_or_ask_window_focused(app_handle) {
                    debug!("notification skipped because chat or ask window focused");
                    return;
                }

                // 准备通知内容
                let title = if let Some(name) = assistant_name {
                    format!("AI 消息完成 - {}", name)
                } else {
                    "AI 消息完成".to_string()
                };

                let body = if content.chars().count() > 60 {
                    let truncated: String = content.chars().take(57).collect();
                    format!("{}...", truncated)
                } else {
                    content.to_string()
                };

                // 发送系统通知
                if let Err(e) = app_handle.notification().builder().title(&title).body(&body).show()
                {
                    warn!(error = %e, "failed to send notification");
                }
            }
        }
    }
}

/// 输出消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputType {
    None,
    Reasoning,
    Response,
}

impl Default for OutputType {
    fn default() -> Self {
        OutputType::None
    }
}

/// 工具名分割助手
fn split_tool_name(fn_name: &str) -> (String, String) {
    if let Some((s, t)) = fn_name.split_once("__") {
        (s.to_string(), t.to_string())
    } else {
        (String::from("default"), fn_name.to_string())
    }
}

/// 创建并发出 message_add 事件，返回消息ID
async fn ensure_stream_message(
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    conversation_id: i64,
    message_type: &str,
    initial_content: &str,
    llm_model_id: i64,
    llm_model_name: &str,
    generation_group_id: &str,
    parent_group_id_override: Option<String>,
    start_time: Option<chrono::DateTime<chrono::Utc>>,
    first_token_time: Option<chrono::DateTime<chrono::Utc>>,
    ttft_ms: Option<i64>,
) -> anyhow::Result<i64> {
    let now = chrono::Utc::now();
    let message_start_time = start_time.unwrap_or(now);

    // 在创建新的 response 消息前，如果上一条是错误消息，则清理
    if message_type == "response" {
        let _ = cleanup_last_error_message(conversation_db, conversation_id).await;
    }

    let new_message = conversation_db
        .message_repo()
        .context("failed to get message_repo")?
        .create(&Message {
            id: 0,
            parent_id: None,
            conversation_id,
            message_type: message_type.to_string(),
            content: initial_content.to_string(),
            llm_model_id: Some(llm_model_id),
            llm_model_name: Some(llm_model_name.to_string()),
            created_time: message_start_time,
            start_time: Some(message_start_time),
            finish_time: None,
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: Some(generation_group_id.to_string()),
            parent_group_id: parent_group_id_override,
            tool_calls_json: None,
            first_token_time,
            ttft_ms,
        })
        .context("failed to create stream message")?;

    let add_event = ConversationEvent {
        r#type: "message_add".to_string(),
        data: serde_json::to_value(MessageAddEvent {
            message_id: new_message.id,
            message_type: message_type.to_string(),
        })
        .unwrap(),
    };
    let _ = window.emit(format!("conversation_event_{}", conversation_id).as_str(), add_event);

    Ok(new_message.id)
}

/// 更新消息内容并发出 message_update
fn persist_and_emit_update(
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    conversation_id: i64,
    msg_id: i64,
    message_type: &str,
    content: &str,
    is_done: bool,
) -> anyhow::Result<()> {
    if let Ok(Some(mut message)) =
        conversation_db.message_repo().context("failed to get message_repo for read")?.read(msg_id)
    {
        message.content = content.to_string();
        conversation_db
            .message_repo()
            .context("failed to get message_repo for update")?
            .update(&message)
            .ok();
    }

    let update_event = ConversationEvent {
        r#type: "message_update".to_string(),
        data: serde_json::to_value(MessageUpdateEvent {
            message_id: msg_id,
            message_type: message_type.to_string(),
            content: content.to_string(),
            is_done,
            token_count: None,
            input_token_count: None,
            output_token_count: None,
            ttft_ms: None,
            tps: None,
        })
        .unwrap(),
    };
    let _ = window.emit(format!("conversation_event_{}", conversation_id).as_str(), update_event);

    Ok(())
}

/// 统一处理捕获到的工具调用：创建DB记录、插入UI注释、更新消息、可选自动执行、可选向UI发事件
async fn handle_captured_tool_calls_common(
    app_handle: &tauri::AppHandle,
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    conversation_id: i64,
    response_message_id: i64,
    captured_tool_calls: &[ToolCall],
    response_content: &mut String,
    emit_tool_call_events: bool,
    mcp_override_config: Option<&McpOverrideConfig>,
    tool_name_mapping: &ToolNameMapping,
) -> anyhow::Result<()> {
    // 先将完整的 tool_calls JSON 覆盖保存到消息，保证数据源一致
    if let Ok(Some(mut msg)) = conversation_db
        .message_repo()
        .context("failed to get message_repo")?
        .read(response_message_id)
    {
        msg.tool_calls_json = serde_json::to_string(&captured_tool_calls).ok();
        let _ = conversation_db
            .message_repo()
            .context("failed to get message_repo for update")?
            .update(&msg);
    }

    for tool_call in captured_tool_calls {
        // 使用映射表还原原始名称，用于 UI 显示和数据库记录
        let (server_name, tool_name) = resolve_tool_name(&tool_call.fn_name, tool_name_mapping);
        let params_str = tool_call.fn_arguments.to_string();

        // 创建工具调用记录（使用原始名称）
        match crate::mcp::execution_api::create_mcp_tool_call_with_llm_id(
            app_handle.clone(),
            conversation_id,
            Some(response_message_id),
            server_name.clone(),
            tool_name.clone(),
            params_str.clone(),
            Some(&tool_call.call_id),
            Some(response_message_id),
        )
        .await
        {
            Ok(tool_call_record) => {
                // 追加 UI hint（使用原始名称）
                let ui_hint = format!(
                    "\n\n<!-- MCP_TOOL_CALL:{} -->\n",
                    serde_json::json!({
                        "server_name": server_name,
                        "tool_name": tool_name,
                        "parameters": params_str,
                        "call_id": tool_call_record.id,
                        "llm_call_id": tool_call.call_id.clone(),
                    })
                );
                response_content.push_str(&ui_hint);

                // 持久化内容并发出更新
                if let Ok(Some(mut msg)) = conversation_db
                    .message_repo()
                    .context("failed to get message_repo for read")?
                    .read(response_message_id)
                {
                    msg.content = response_content.clone();
                    // 覆盖保存 tool_calls JSON（再次确保一致）
                    msg.tool_calls_json = serde_json::to_string(&captured_tool_calls).ok();
                    let _ = conversation_db
                        .message_repo()
                        .context("failed to get message_repo for update")?
                        .update(&msg);

                    let update_event = ConversationEvent {
                        r#type: "message_update".to_string(),
                        data: serde_json::to_value(MessageUpdateEvent {
                            message_id: response_message_id,
                            message_type: "response".to_string(),
                            content: response_content.clone(),
                            is_done: false,
                            token_count: None,
                            input_token_count: None,
                            output_token_count: None,
                            ttft_ms: None,
                            tps: None,
                        })
                        .unwrap(),
                    };
                    let _ = window.emit(
                        format!("conversation_event_{}", conversation_id).as_str(),
                        update_event,
                    );
                }

                // 自动执行（若配置）
                if let Ok(conv) = conversation_db
                    .conversation_repo()
                    .context("failed to get conversation_repo")?
                    .read(conversation_id)
                {
                    if let Some(assistant_id) = conv.and_then(|c| c.assistant_id) {
                        if let Ok(servers) =
                            crate::api::assistant_api::get_assistant_mcp_servers_with_tools(
                                app_handle.clone(),
                                assistant_id,
                            )
                            .await
                        {
                            let mut should_auto_run = false;
                            for s in servers.iter() {
                                // 支持精确匹配和清理后名称匹配
                                let name_matches = s.name == server_name
                                    || sanitize_tool_name(&s.name) == server_name;
                                if name_matches && s.is_enabled {
                                    if let Some(t) =
                                        s.tools.iter().find(|t| t.name == tool_name && t.is_enabled)
                                    {
                                        // Check for override auto-run setting
                                        // Priority: all_tool_auto_run > tool_auto_run > default
                                        let tool_key = format!("{}/{}", server_name, tool_name);
                                        let auto_run = if let Some(all_auto_run) =
                                            mcp_override_config
                                                .and_then(|config| config.all_tool_auto_run)
                                        {
                                            // all_tool_auto_run has highest priority
                                            all_auto_run
                                        } else {
                                            // Check individual tool override
                                            *mcp_override_config
                                                .and_then(|config| config.tool_auto_run.as_ref())
                                                .and_then(|auto_run_map| {
                                                    auto_run_map.get(&tool_key)
                                                })
                                                .unwrap_or(&t.is_auto_run)
                                        };

                                        if auto_run {
                                            should_auto_run = true;
                                        }
                                    }
                                }
                            }
                            if should_auto_run {
                                let state = app_handle.state::<crate::AppState>();
                                let feature_config_state =
                                    app_handle.state::<crate::FeatureConfigState>();
                                if let Err(e) = crate::mcp::execution_api::execute_mcp_tool_call(
                                    app_handle.clone(),
                                    state,
                                    feature_config_state,
                                    window.clone(),
                                    tool_call_record.id,
                                )
                                .await
                                {
                                    warn!(
                                        "Auto-execute MCP tool failed (call_id={}): {}",
                                        tool_call_record.id, e
                                    );
                                }
                            }
                        }
                    }
                }

                if emit_tool_call_events {
                    let tool_call_event = serde_json::json!({
                        "type": "tool_call",
                        "data": {
                            "conversation_id": conversation_id,
                            "call_id": tool_call.call_id,
                            "function_name": tool_call.fn_name,
                            "arguments": tool_call.fn_arguments,
                            "response_message_id": response_message_id
                        }
                    });
                    let _ = window.emit(
                        format!("conversation_event_{}", conversation_id).as_str(),
                        tool_call_event,
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to create MCP tool call record");
            }
        }
    }

    Ok(())
}

/// 助手提及信息
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AssistantMention {
    pub assistant_id: i64,
    pub name: String,
    pub start_pos: usize,  // 字符位置（不是字节位置）
    pub end_pos: usize,    // 结束位置
    pub raw_match: String, // 原始匹配字符串 "@assistant_name"
}

/// 消息解析结果
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MessageParseResult {
    pub mentions: Vec<AssistantMention>,
    pub cleaned_content: String,           // 移除@mentions后的内容
    pub original_content: String,          // 原始内容
    pub primary_assistant_id: Option<i64>, // 主要助手ID（第一个匹配的）
}

/// 位置限制选项
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PositionRestriction {
    Anywhere,     // 任何位置
    StartOnly,    // 仅开头
    WordBoundary, // 单词边界（前面是空格或开头）
}

/// 解析选项
#[derive(Debug, Clone)]
pub struct ParseOptions {
    pub first_only: bool, // 只匹配第一个
    pub position_restriction: PositionRestriction,
    pub remove_mentions: bool,       // 是否从结果中移除@mentions
    pub case_sensitive: bool,        // 大小写敏感
    pub require_word_boundary: bool, // 要求词边界（替代 require_space_after）
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            first_only: true,
            position_restriction: PositionRestriction::Anywhere,
            remove_mentions: true,
            case_sensitive: false,
            require_word_boundary: true,
        }
    }
}

// 尝试获取HTTP错误的响应体（改进版，支持POST请求）
async fn try_fetch_error_body_advanced(
    url: &str,
    status: reqwest::StatusCode,
    is_chat_api: bool,
) -> Option<String> {
    if !status.is_client_error() && !status.is_server_error() {
        return None;
    }

    debug!(url, "attempting to fetch error body from url");

    // 创建一个简单的客户端来获取错误信息
    let client = reqwest::Client::new();

    if is_chat_api && url.contains("/chat/completions") {
        // 方法1: 发送一个故意错误的请求来获取错误响应
        let invalid_payload = serde_json::json!({
            "model": "invalid-model-name-that-does-not-exist",
            "messages": []
        });

        debug!("trying invalid payload method for error body fetch");
        match client.post(url).json(&invalid_payload).send().await {
            Ok(response) => {
                debug!(status = ?response.status(), "received error response status");
                if response.status().is_client_error() || response.status().is_server_error() {
                    match response.text().await {
                        Ok(body) => {
                            debug!(body = %body, "received error response body");
                            return Some(body);
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to read error body");
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "invalid payload request failed");
            }
        }

        // 方法2: 发送空的POST请求
        debug!("trying empty post method for error body fetch");
        match client.post(url).header("Content-Type", "application/json").body("{}").send().await {
            Ok(response) => {
                debug!(status = ?response.status(), "empty post response status");
                if response.status().is_client_error() || response.status().is_server_error() {
                    match response.text().await {
                        Ok(body) => {
                            debug!(body = %body, "empty post error body");
                            return Some(body);
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to read empty post body");
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "empty post request failed");
            }
        }

        // 方法3: 尝试用HEAD请求来获取一些信息
        debug!("trying head request method for error body fetch");
        match client.head(url).send().await {
            Ok(response) => {
                debug!(status = ?response.status(), headers = ?response.headers(), "head response info");
                // HEAD请求通常不会有响应体，但可能有有用的头信息
            }
            Err(e) => {
                warn!(error = %e, "head request failed");

                // 尝试从错误消息中提取有用信息
                let error_msg = e.to_string();
                if error_msg.contains("{") && error_msg.contains("}") {
                    // 尝试从错误消息中提取JSON
                    if let Some(start) = error_msg.find("{") {
                        if let Some(end) = error_msg.rfind("}") {
                            let json_part = &error_msg[start..=end];
                            debug!(json_part = %json_part, "extracted json from head error");
                            return Some(json_part.to_string());
                        }
                    }
                }
            }
        }
    } else {
        // 对于其他API，使用GET请求
        debug!("trying get request method for error body fetch");
        match client.get(url).send().await {
            Ok(response) => {
                if response.status().is_client_error() || response.status().is_server_error() {
                    match response.text().await {
                        Ok(body) => {
                            debug!(body = %body, "get error response body");
                            return Some(body);
                        }
                        Err(e) => {
                            warn!(error = %e, "failed to read get error body");
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "get request failed");
            }
        }
    }

    None
}

// 增强的错误处理函数（简化版，避免Send问题）
async fn enhanced_error_logging_v2<E: std::error::Error + 'static>(
    error: &E,
    context: &str,
) -> String {
    error!(context, error_type = ?error, error_display = %error, error_debug = ?error, "stream error encountered");

    // 收集错误链信息，不进行异步操作
    let mut current_error: Option<&dyn std::error::Error> = Some(error);
    let mut i = 0;
    let mut error_urls = Vec::new(); // 收集URL用于后续处理

    while let Some(err) = current_error {
        debug!(index = i, err = %err, err_type = std::any::type_name_of_val(err), "error chain element");

        // 检查错误字符串中是否包含有用信息
        let error_string = err.to_string();
        debug!(error_string = %error_string, "error string");

        // 尝试从错误字符串中提取URL和状态码
        if error_string.contains("400") && error_string.contains("https://") {
            if let Some(start) = error_string.find("https://") {
                if let Some(end) = error_string[start..].find("\"") {
                    let url = &error_string[start..start + end];
                    debug!(url = %url, "extracted url from error string");
                    error_urls.push((url.to_string(), reqwest::StatusCode::from_u16(400).unwrap()));
                }
            }
        }

        // 特别检查是否是 reqwest 错误并提取详细信息
        if let Some(reqwest_error) = err.downcast_ref::<reqwest::Error>() {
            debug!("reqwest error details");

            if let Some(status) = reqwest_error.status() {
                debug!(status = %status, is_client_error = status.is_client_error(), is_server_error = status.is_server_error(), "reqwest status details");

                if let Some(url) = reqwest_error.url() {
                    let url_str = url.to_string();
                    debug!(request_url = %url_str, "reqwest request url");

                    // 对于错误状态码，收集URL信息但不在这里执行异步操作
                    if status.is_client_error() || status.is_server_error() {
                        error_urls.push((url_str, status));
                    }
                }
            }

            debug!(
                is_timeout = reqwest_error.is_timeout(),
                is_connect = reqwest_error.is_connect(),
                is_request = reqwest_error.is_request(),
                is_body = reqwest_error.is_body(),
                is_decode = reqwest_error.is_decode(),
                "reqwest error flags"
            );
        } else {
            // 如果不是reqwest错误，尝试其他方式解析
            debug!("not reqwest error: attempting string parsing");

            // 检查是否是EventSource相关的错误
            if error_string.contains("EventSource") || error_string.contains("Invalid status code")
            {
                debug!("event source error detected");

                // 尝试从字符串中提取状态码
                if error_string.contains("400") {
                    debug!(detected_status_code = 400, "detected status code in error string");
                }

                // 尝试提取URL
                if let Some(start) = error_string.find("url: \"") {
                    let start = start + 6; // 跳过 'url: "'
                    if let Some(end) = error_string[start..].find("\"") {
                        let url = &error_string[start..start + end];
                        debug!(url = %url, "extracted url from string");
                        if !error_urls.iter().any(|(u, _)| u == url) {
                            error_urls.push((
                                url.to_string(),
                                reqwest::StatusCode::from_u16(400).unwrap(),
                            ));
                        }
                    }
                }
            }
        }

        current_error = err.source();
        i += 1;
    }

    // 现在在循环外处理URL（如果有的话）
    for (url_str, status) in error_urls {
        debug!(url = %url_str, %status, "processing extracted url");
        let is_chat_api = url_str.contains("/chat/completions");
        if let Some(error_body) = try_fetch_error_body_advanced(&url_str, status, is_chat_api).await
        {
            debug!(error_body = %error_body, "extracted error body");
            // 返回结构化错误信息
            return create_structured_error_message(error, Some(error_body));
        }
    }

    debug!(context, "end error details");

    // 如果没有提取到错误体，返回用户友好的错误信息
    create_structured_error_message(error, None)
}

/// 构建统一的、可被前端解析的富错误负载（JSON字符串）
/// 保留此函数以向后兼容，内部调用带 HTTP 详情的版本
#[allow(dead_code)]
fn build_rich_error_payload(
    main_message: String,
    details: Option<String>,
    model_name: Option<String>,
    phase: &str,
    attempts: Option<i32>,
    original_error: String,
) -> String {
    build_rich_error_payload_with_http_details(
        main_message,
        details,
        model_name,
        phase,
        attempts,
        original_error,
        None,
    )
}

/// 构建统一的、可被前端解析的富错误负载（JSON字符串），包含 HTTP 错误详情
fn build_rich_error_payload_with_http_details(
    main_message: String,
    details: Option<String>,
    model_name: Option<String>,
    phase: &str,
    attempts: Option<i32>,
    original_error: String,
    http_details: Option<HttpErrorDetails>,
) -> String {
    // 根据主要信息给出建议
    let mut suggestions: Vec<&str> = Vec::new();
    let lower = main_message.to_lowercase();
    let original_lower = original_error.to_lowercase();

    // 综合检查主消息和原始错误
    let check_str = format!("{} {}", lower, original_lower);

    if check_str.contains("网络")
        || check_str.contains("network")
        || check_str.contains("连接")
        || check_str.contains("connection")
    {
        suggestions.push("检查网络连接与代理设置");
    }
    if check_str.contains("认证")
        || check_str.contains("api密钥")
        || check_str.contains("unauthorized")
        || check_str.contains("401")
        || check_str.contains("api_key")
    {
        suggestions.push("检查 API Key 是否正确、是否过期");
    }
    if check_str.contains("权限") || check_str.contains("forbidden") || check_str.contains("403")
    {
        suggestions.push("检查账户或密钥是否有对应权限");
    }
    if check_str.contains("频繁")
        || check_str.contains("429")
        || check_str.contains("rate limit")
        || check_str.contains("rate_limit")
    {
        suggestions.push("降低调用频率或稍后再试");
    }
    if check_str.contains("服务器")
        || check_str.contains("503")
        || check_str.contains("502")
        || check_str.contains("500")
        || check_str.contains("server_error")
    {
        suggestions.push("服务端异常，稍后重试");
    }
    if check_str.contains("格式") || check_str.contains("json") || check_str.contains("parse") {
        suggestions.push("检查 Base URL / 模型配置与请求参数格式");
    }
    if check_str.contains("model")
        && (check_str.contains("not found")
            || check_str.contains("not_found")
            || check_str.contains("does not exist"))
    {
        suggestions.push("检查模型名称是否正确");
    }
    if check_str.contains("context_length")
        || check_str.contains("too long")
        || check_str.contains("token")
    {
        suggestions.push("尝试减少输入内容长度");
    }

    // 提取 HTTP 详情
    let (status_code, response_body, endpoint, request_id) = if let Some(ref http) = http_details {
        (
            http.status_code,
            http.response_body.clone(),
            http.endpoint.clone(),
            http.request_id.clone(),
        )
    } else {
        (None, None, None, None)
    };

    // 优先使用 HTTP 详情中的 response_body，否则使用 details
    let final_details = response_body.or(details);

    let payload = serde_json::json!({
        "message": main_message,
        "details": final_details,
        "model": model_name,
        "phase": phase,
        "attempts": attempts,
        "original_error": original_error,
        "suggestions": suggestions,
        "status": status_code,
        "endpoint": endpoint,
        "request_id": request_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    payload.to_string()
}

// 创建结构化错误消息
fn create_structured_error_message<E: std::fmt::Display>(
    error: &E,
    request_body: Option<String>,
) -> String {
    let user_friendly_message = get_user_friendly_error_message(error);

    if let Some(body) = request_body {
        // 使用特殊分隔符将主要信息和详情分开存储在content中
        format!("{}|||ERROR_DETAILS|||{}", user_friendly_message, body)
    } else {
        // 如果没有请求体信息，只返回主要消息
        user_friendly_message
    }
}

// 将错误信息转换为用户友好的中文提示
fn get_user_friendly_error_message<E: std::fmt::Display>(error: &E) -> String {
    let error_str = error.to_string().to_lowercase();

    if error_str.contains("network")
        || error_str.contains("connection")
        || error_str.contains("timeout")
    {
        "网络连接异常，请检查网络设置".to_string()
    } else if error_str.contains("unauthorized") || error_str.contains("401") {
        "身份认证失败，请检查API密钥".to_string()
    } else if error_str.contains("forbidden") || error_str.contains("403") {
        "访问被拒绝，请检查API权限".to_string()
    } else if error_str.contains("not found") || error_str.contains("404") {
        "请求的服务不存在，请检查配置".to_string()
    } else if error_str.contains("rate limit") || error_str.contains("429") {
        "请求过于频繁，请稍后重试".to_string()
    } else if error_str.contains("quota") || error_str.contains("exceeded") {
        "API配额已用完，请检查账户状态".to_string()
    } else if error_str.contains("server")
        || error_str.contains("500")
        || error_str.contains("502")
        || error_str.contains("503")
    {
        "服务器暂时不可用，请稍后重试".to_string()
    } else if error_str.contains("json") || error_str.contains("parse") {
        "响应数据格式异常".to_string()
    } else {
        "请求失败，请稍后重试".to_string()
    }
}

/// 检查指定位置是否为词边界结尾
fn is_word_boundary_end(chars: &[char], pos: usize) -> bool {
    if pos >= chars.len() {
        return true; // 字符串结尾是有效边界
    }

    let next_char = chars[pos];

    // 如果下一个字符不是字母、数字或某些连接符，就认为是边界
    // 这样可以自动处理所有标点符号、空格、中文标点等
    !next_char.is_alphanumeric() && next_char != '_' && next_char != '-'
}

/// 检查位置是否满足限制条件
fn check_position_restriction(
    chars: &[char],
    pos: usize,
    restriction: &PositionRestriction,
) -> bool {
    match restriction {
        PositionRestriction::Anywhere => true,
        PositionRestriction::StartOnly => pos == 0,
        PositionRestriction::WordBoundary => {
            pos == 0 || chars.get(pos.saturating_sub(1)) == Some(&' ')
        }
    }
}

/// 尝试在指定位置匹配特定助手
fn try_match_specific_assistant(
    assistant: &Assistant,
    chars: &[char],
    start_pos: usize,
    options: &ParseOptions,
) -> Option<AssistantMention> {
    if chars[start_pos] != '@' {
        return None;
    }

    let assistant_name = if options.case_sensitive {
        &assistant.name
    } else {
        // 对于不区分大小写，我们需要在比较时转换
        &assistant.name
    };

    let pattern_chars: Vec<char> = format!("@{}", assistant_name).chars().collect();

    // 检查是否有足够的字符来匹配
    if start_pos + pattern_chars.len() > chars.len() {
        return None;
    }

    // 执行字符匹配
    let match_slice = &chars[start_pos..start_pos + pattern_chars.len()];

    let matches = if options.case_sensitive {
        match_slice == &pattern_chars[..]
    } else {
        match_slice.iter().collect::<String>().to_lowercase()
            == pattern_chars.iter().collect::<String>().to_lowercase()
    };

    if !matches {
        return None;
    }

    let end_pos = start_pos + pattern_chars.len();

    // 使用改进的边界检测
    if options.require_word_boundary && !is_word_boundary_end(chars, end_pos) {
        return None;
    }

    // 即使不要求word boundary，我们也需要确保这是一个完整的助手名称匹配
    // 如果assistant name后面直接跟着字母数字字符，说明这不是一个完整匹配
    if !options.require_word_boundary && end_pos < chars.len() {
        let next_char = chars[end_pos];
        // 如果下一个字符是字母或数字，说明这不是完整匹配 (例如: @gpt4help 不应该匹配 gpt4)
        if next_char.is_alphanumeric() {
            return None;
        }
    }
    Some(AssistantMention {
        assistant_id: assistant.id,
        name: assistant.name.clone(),
        start_pos,
        end_pos,
        raw_match: pattern_chars.iter().collect(),
    })
}

/// 尝试在指定位置匹配任意助手
fn try_match_assistant_at_position(
    assistants: &Vec<Assistant>,
    chars: &[char],
    start_pos: usize,
    options: &ParseOptions,
) -> Option<AssistantMention> {
    if chars[start_pos] != '@' {
        return None;
    }

    // 检查位置限制
    if !check_position_restriction(chars, start_pos, &options.position_restriction) {
        return None;
    }

    // 按名称长度从长到短排序，优先匹配更长的名称（避免部分匹配问题）
    let mut sorted_assistants = assistants.clone();
    sorted_assistants.sort_by(|a, b| b.name.len().cmp(&a.name.len()));

    // 尝试匹配每个助手名称
    for assistant in &sorted_assistants {
        if let Some(mention) = try_match_specific_assistant(assistant, chars, start_pos, options) {
            return Some(mention);
        }
    }
    None
}

/// 从内容中移除@mentions
fn remove_mentions_from_content(content: &str, mentions: &[AssistantMention]) -> String {
    if mentions.is_empty() {
        return content.to_string();
    }

    let chars: Vec<char> = content.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;

    // 按开始位置排序
    let mut sorted_mentions = mentions.to_vec();
    sorted_mentions.sort_by(|a, b| a.start_pos.cmp(&b.start_pos));

    for mention in &sorted_mentions {
        // 添加mention之前的内容
        while i < mention.start_pos {
            result.push(chars[i]);
            i += 1;
        }

        // 跳过mention内容
        i = mention.end_pos;

        // 智能处理mention后的空格和标点符号
        if i < chars.len() {
            let next_char = chars[i];

            // 如果mention后面紧跟着逗号、句号等标点符号，跳过它们前面可能的空格
            if ",.!?;:，。！？；：".contains(next_char) {
                // 跳过标点符号
                i += 1;
                // 跳过标点符号后的空格
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
            } else if next_char.is_whitespace() {
                // 如果mention后面只是空格，跳过空格
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
            }
        }
    }

    // 添加剩余内容
    while i < chars.len() {
        result.push(chars[i]);
        i += 1;
    }

    let result_str = result.iter().collect::<String>();

    // 清理多余的空格
    result_str.split_whitespace().collect::<Vec<&str>>().join(" ").trim().to_string()
}

/// 解析消息中的助手提及
pub fn parse_assistant_mentions(
    assistants: &Vec<Assistant>,
    content: &str,
    options: &ParseOptions,
) -> Result<MessageParseResult, AppError> {
    let mut mentions = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '@' {
            if let Some(mention) = try_match_assistant_at_position(assistants, &chars, i, options) {
                mentions.push(mention.clone());
                i = mention.end_pos;

                // 如果只需要第一个匹配，直接退出
                if options.first_only {
                    break;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    // 根据配置处理结果
    let cleaned_content = if options.remove_mentions {
        remove_mentions_from_content(content, &mentions)
    } else {
        content.to_string()
    };

    Ok(MessageParseResult {
        primary_assistant_id: mentions.first().map(|m| m.assistant_id),
        mentions,
        cleaned_content,
        original_content: content.to_string(),
    })
}

/// 从消息中提取 @assistant_name 并返回处理后的消息和助手ID
/// 如果找到 @assistant_name，则返回对应的助手ID和清理后的消息
/// 如果没有找到或找不到对应助手，则返回原始助手ID和原始消息
///
/// 这个函数保持向后兼容，内部使用新的解析器实现
pub async fn extract_assistant_from_message(
    assistants: &Vec<Assistant>,
    prompt: &str,
    default_assistant_id: i64,
) -> Result<(i64, String), AppError> {
    // 使用默认选项，保持原有行为：只匹配开头的第一个@符号
    let options = ParseOptions::default();

    let result = parse_assistant_mentions(assistants, prompt, &options)?;

    Ok((result.primary_assistant_id.unwrap_or(default_assistant_id), result.cleaned_content))
}

pub async fn handle_stream_chat(
    client: &Client,
    model_name: &str,
    chat_request: &ChatRequest,
    chat_options: &ChatOptions,
    conversation_id: i64,
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    app_handle: &tauri::AppHandle,
    need_generate_title: bool,
    user_prompt: String,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    generation_group_id_override: Option<String>,
    parent_group_id_override: Option<String>,
    llm_model_id: i64,
    llm_model_name: String,
    mcp_override_config: Option<McpOverrideConfig>,
    tool_name_mapping: ToolNameMapping,
) -> Result<(), anyhow::Error> {
    let mut main_attempts = 0;
    let app_handle_clone = app_handle.clone();

    // 从配置中获取最大重试次数
    let max_retry_attempts = get_retry_attempts_from_config(&config_feature_map);

    // 外层重试循环，处理整个流式会话
    loop {
        main_attempts += 1;
        info!(attempt = main_attempts, max_attempts = max_retry_attempts, "stream chat attempt");

        let stream_result = attempt_stream_chat(
            client,
            model_name,
            chat_request,
            chat_options,
            conversation_id,
            conversation_db,
            window,
            &app_handle_clone,
            need_generate_title,
            user_prompt.clone(),
            config_feature_map.clone(),
            generation_group_id_override.clone(),
            parent_group_id_override.clone(),
            llm_model_id,
            llm_model_name.clone(),
            mcp_override_config.clone(),
            tool_name_mapping.clone(),
        )
        .await;

        match stream_result {
            Ok(_) => {
                info!(attempt = main_attempts, "stream chat completed");
                return Ok(());
            }
            Err(e) => {
                warn!(attempt = main_attempts, error = %e, "stream chat failed attempt");

                if main_attempts >= max_retry_attempts {
                    // 最终失败，提取 HTTP 错误详情并构建结构化错误
                    let http_details = extract_http_details_from_anyhow(&e);
                    let user_friendly = get_user_friendly_error_message(&e);

                    // 使用更友好的主消息
                    let final_main = format!("AI请求失败: {}", user_friendly);
                    let payload = build_rich_error_payload_with_http_details(
                        final_main,
                        None,
                        Some(llm_model_name.clone()),
                        "stream",
                        Some(main_attempts as i32),
                        e.to_string(),
                        Some(http_details),
                    );
                    error!(
                        "[[final_stream_error]]: 流式聊天在{}次尝试后失败: {}",
                        main_attempts, e
                    );

                    // 发送错误通知到合适的窗口
                    send_error_to_appropriate_window(
                        &window,
                        &user_friendly,
                        Some(conversation_id),
                    );

                    // 创建错误消息
                    create_error_message(
                        conversation_db,
                        conversation_id,
                        llm_model_id,
                        llm_model_name.clone(),
                        &payload,
                        generation_group_id_override.clone(),
                        parent_group_id_override.clone(),
                        window,
                    )
                    .await;

                    return Err(anyhow::anyhow!("AI stream failed after retries"));
                }

                let delay = calculate_retry_delay(main_attempts);
                debug!(delay_ms = delay, "retrying stream after delay");
                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
}

// 单次流式聊天尝试
async fn attempt_stream_chat(
    client: &Client,
    model_name: &str,
    chat_request: &ChatRequest,
    chat_options: &ChatOptions,
    conversation_id: i64,
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    app_handle: &tauri::AppHandle,
    need_generate_title: bool,
    user_prompt: String,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    generation_group_id_override: Option<String>,
    parent_group_id_override: Option<String>,
    llm_model_id: i64,
    llm_model_name: String,
    mcp_override_config: Option<McpOverrideConfig>,
    tool_name_mapping: ToolNameMapping,
) -> Result<(), anyhow::Error> {
    // 尝试建立流式连接
    info!(model_name, "establishing stream connection");

    debug!("stream chat_request messages");
    for (i, msg) in chat_request.messages.iter().enumerate() {
        debug!(message_index = i, role = ?msg.role, "stream message");
        match &msg.role {
            genai::chat::ChatRole::Assistant => {
                if let Some(text) = msg.content.first_text() {
                    debug!(preview = %text.chars().take(100).collect::<String>(), "assistant content");
                } else if msg.content.contains_tool_call() {
                    let tool_calls = msg.content.tool_calls();
                    debug!(tool_calls_count = tool_calls.len(), "assistant tool calls");
                    for tc in tool_calls {
                        debug!(call_id = %tc.call_id, fn_name = %tc.fn_name, "tool call item");
                    }
                } else {
                    debug!("assistant content other type");
                }
            }
            genai::chat::ChatRole::Tool => {
                if let Some(text) = msg.content.first_text() {
                    debug!(
                        "    Tool response content: {}",
                        text.chars().take(100).collect::<String>()
                    );
                } else {
                    debug!("tool response other type");
                }
            }
            _ => {
                if let Some(text) = msg.content.first_text() {
                    debug!(preview = %text.chars().take(100).collect::<String>(), "other content");
                } else {
                    debug!("other content other type");
                }
            }
        }
    }

    let stream_request_start_time = chrono::Utc::now();

    let chat_stream_response = match client
        .exec_chat_stream(model_name, chat_request.clone(), Some(&chat_options))
        .await
    {
        Ok(response) => {
            info!("stream connection established");
            response
        }
        Err(e) => {
            let _user_friendly_error = enhanced_error_logging_v2(&e, "Stream Connection").await;
            return Err(anyhow::anyhow!("Failed to establish stream connection: {}", e));
        }
    };

    let mut chat_stream = chat_stream_response.stream;
    let mut reasoning_content = String::new();
    let mut response_content = String::new();
    let mut reasoning_message_id: Option<i64> = None;
    let mut response_message_id: Option<i64> = None;
    let mut captured_tool_calls: Vec<ToolCall> = Vec::new();

    // Diagnostics: counters for stream content
    let mut response_chunk_count: usize = 0;
    let mut response_char_count: usize = 0;
    let mut reasoning_chunk_count: usize = 0;
    let mut reasoning_char_count: usize = 0;

    let generation_group_id =
        generation_group_id_override.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let is_regeneration = parent_group_id_override.is_some();
    let mut group_merge_event_emitted = false;

    let mut current_output_type: OutputType = OutputType::None;
    let mut reasoning_start_time: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut reasoning_end_time: Option<chrono::DateTime<chrono::Utc>> = None; // 记录 reasoning 结束时间
    let mut response_start_time: Option<chrono::DateTime<chrono::Utc>> = None; // 改为 None，在创建 response 时动态确定
    let mut response_first_token_time: Option<chrono::DateTime<chrono::Utc>> = None; // 首字到达时间
    let mut first_any_token_time: Option<chrono::DateTime<chrono::Utc>> = None; // 任意类型首字到达时间（用于 TPS 计算备用）

    loop {
        let stream_result = chat_stream.next().await;
        match stream_result {
            Some(Ok(stream_event)) => {
                match stream_event {
                    ChatStreamEvent::Start => {}
                    ChatStreamEvent::Chunk(chunk) => {
                        response_chunk_count += 1;
                        response_char_count += chunk.content.chars().count();

                        // 记录首字到达时间
                        if response_first_token_time.is_none() && !chunk.content.is_empty() {
                            let now = chrono::Utc::now();
                            response_first_token_time = Some(now);
                            // 同时更新任意类型首字时间（如果尚未设置）
                            if first_any_token_time.is_none() {
                                first_any_token_time = Some(now);
                            }
                        }

                        if current_output_type == OutputType::Reasoning {
                            if let (Some(msg_id), Some(start_time)) =
                                (reasoning_message_id, reasoning_start_time)
                            {
                                // 记录 reasoning 结束时间，用于后续 response 创建时使用
                                let now = chrono::Utc::now();
                                reasoning_end_time = Some(now);

                                if let Err(e) = super::conversation::handle_message_type_end(
                                    msg_id,
                                    "reasoning",
                                    &reasoning_content,
                                    start_time,
                                    &conversation_db,
                                    &window,
                                    conversation_id,
                                    &app_handle,
                                    false, // allow MCP detection
                                )
                                .await
                                {
                                    warn!(error = %e, "reasoning type end failed");
                                }
                            }
                        }

                        if current_output_type != OutputType::Response {
                            current_output_type = OutputType::Response;
                        }

                        response_content.push_str(&chunk.content);

                        if response_message_id.is_none() {
                            // 如果有 reasoning 结束时间，使用它作为 response 的 start_time
                            // 这样可以确保 reasoning 的 created_time 早于 response 的 created_time
                            let actual_start_time = reasoning_end_time.unwrap_or_else(|| {
                                response_start_time.get_or_insert(stream_request_start_time);
                                response_start_time.unwrap()
                            });
                            response_start_time = Some(actual_start_time);

                            let start_time = actual_start_time;
                            let ttft_ms = response_first_token_time.as_ref().map(|ft| {
                                (ft.timestamp_millis() - start_time.timestamp_millis()).max(0)
                            });

                            if let Ok(new_id) = ensure_stream_message(
                                &conversation_db,
                                &window,
                                conversation_id,
                                "response",
                                &response_content,
                                llm_model_id,
                                &llm_model_name,
                                &generation_group_id,
                                parent_group_id_override.clone(),
                                Some(start_time),
                                response_first_token_time.clone(),
                                ttft_ms,
                            )
                            .await
                            {
                                response_message_id = Some(new_id);

                                if is_regeneration && !group_merge_event_emitted {
                                    if let Some(ref parent_group_id) = parent_group_id_override {
                                        let group_merge_event = serde_json::json!({
                                            "type": "group_merge",
                                            "data": {
                                                "original_group_id": parent_group_id,
                                                "new_group_id": generation_group_id.clone(),
                                                "is_regeneration": true,
                                                "first_message_id": new_id,
                                                "conversation_id": conversation_id
                                            }
                                        });
                                        let _ = window.emit(
                                            format!("conversation_event_{}", conversation_id)
                                                .as_str(),
                                            group_merge_event,
                                        );
                                        group_merge_event_emitted = true;
                                    }
                                }
                            }
                        }

                        if let Some(msg_id) = response_message_id {
                            let _ = persist_and_emit_update(
                                &conversation_db,
                                &window,
                                conversation_id,
                                msg_id,
                                "response",
                                &response_content,
                                false,
                            );
                        }
                    }
                    ChatStreamEvent::ReasoningChunk(reasoning_chunk) => {
                        reasoning_chunk_count += 1;
                        reasoning_char_count += reasoning_chunk.content.chars().count();

                        // 记录任意类型首字到达时间（用于 TPS 计算备用）
                        if first_any_token_time.is_none() && !reasoning_chunk.content.is_empty() {
                            first_any_token_time = Some(chrono::Utc::now());
                        }

                        if current_output_type != OutputType::Reasoning {
                            current_output_type = OutputType::Reasoning;
                        }

                        reasoning_content.push_str(&reasoning_chunk.content);

                        if reasoning_message_id.is_none() {
                            let now = chrono::Utc::now();
                            reasoning_start_time = Some(now);

                            if let Ok(new_id) = ensure_stream_message(
                                &conversation_db,
                                &window,
                                conversation_id,
                                "reasoning",
                                &reasoning_content,
                                llm_model_id,
                                &llm_model_name,
                                &generation_group_id,
                                parent_group_id_override.clone(),
                                Some(now),
                                None,
                                None,
                            )
                            .await
                            {
                                reasoning_message_id = Some(new_id);
                            }
                        }

                        if let Some(msg_id) = reasoning_message_id {
                            let _ = persist_and_emit_update(
                                &conversation_db,
                                &window,
                                conversation_id,
                                msg_id,
                                "reasoning",
                                &reasoning_content,
                                false,
                            );
                        }
                    }
                    ChatStreamEvent::ToolCallChunk(tool_call_chunk) => {
                        debug!(?tool_call_chunk, "tool call chunk");
                    }
                    ChatStreamEvent::End(end_event) => {
                        debug!(?end_event, "end event");

                        // Extract and store token usage data before ownership is taken
                        let token_data = end_event.captured_usage.as_ref().map(|usage| {
                            let input_tokens = usage.prompt_tokens.unwrap_or(0);
                            let output_tokens = usage.completion_tokens.unwrap_or(0);
                            let total_tokens =
                                usage.total_tokens.unwrap_or(input_tokens + output_tokens);
                            (input_tokens, output_tokens, total_tokens)
                        });

                        // Capture tool calls if they exist (this takes ownership of end_event)
                        if let Some(tool_calls) = end_event.captured_into_tool_calls() {
                            captured_tool_calls = tool_calls;
                            debug!(?captured_tool_calls, "captured tool calls");
                        }

                        // Store the extracted token data
                        if let Some((input_tokens, output_tokens, total_tokens)) = token_data {
                            // Update the response or reasoning message with token data
                            // 优先更新 response 消息，如果没有则更新 reasoning 消息
                            let target_msg_id = response_message_id.or(reasoning_message_id);
                            let target_message_type = if response_message_id.is_some() {
                                "response"
                            } else {
                                "reasoning"
                            };

                            if let Some(msg_id) = target_msg_id {
                                if let Ok(repo) = conversation_db.message_repo() {
                                    if let Ok(Some(mut message)) = repo.read(msg_id) {
                                        message.input_token_count = input_tokens;
                                        message.output_token_count = output_tokens;
                                        message.token_count = total_tokens;

                                        // Ensure the message used for metrics reflects the whole assistant generation
                                        // (reasoning + response), not just the response segment.
                                        let request_start = stream_request_start_time;
                                        if message
                                            .start_time
                                            .map(|t| {
                                                t.timestamp_millis()
                                                    > request_start.timestamp_millis()
                                            })
                                            .unwrap_or(true)
                                        {
                                            message.start_time = Some(request_start);
                                        }
                                        if let Some(first_any) = first_any_token_time {
                                            if message
                                                .first_token_time
                                                .map(|t| {
                                                    t.timestamp_millis()
                                                        > first_any.timestamp_millis()
                                                })
                                                .unwrap_or(true)
                                            {
                                                message.first_token_time = Some(first_any);
                                            }
                                        }
                                        if let (Some(start), Some(first)) =
                                            (message.start_time, message.first_token_time)
                                        {
                                            let diff = (first.timestamp_millis()
                                                - start.timestamp_millis())
                                            .max(0);
                                            message.ttft_ms = Some(diff);
                                        }

                                        // 计算 TPS (Tokens Per Second)
                                        // TPS = output_tokens / (finish_time - first_token_time) * 1000
                                        // 优先使用 reasoning/response 的最早首 token 时间，避免 TPS 被高估；
                                        // 最后使用 stream_request_start_time 作为备用
                                        let tps = if output_tokens > 0 {
                                            let token_time = first_any_token_time
                                                .or(message.first_token_time)
                                                .or(message.start_time)
                                                .unwrap_or(stream_request_start_time);
                                            let end_time = chrono::Utc::now();
                                            let duration_ms = end_time.timestamp_millis()
                                                - token_time.timestamp_millis();
                                            if duration_ms > 0 {
                                                Some(
                                                    (output_tokens as f64) * 1000.0
                                                        / duration_ms as f64,
                                                )
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };

                                        match repo.update(&message) {
                                            Ok(_) => {
                                                info!(
                                                    message_id = msg_id,
                                                    message_type = target_message_type,
                                                    input_tokens,
                                                    output_tokens,
                                                    total_tokens,
                                                    ttft_ms = message.ttft_ms,
                                                    tps = ?tps,
                                                    "Token usage and performance metrics captured and stored for streaming message"
                                                );

                                                // Send message_update event to notify frontend
                                                let update_event = ConversationEvent {
                                                    r#type: "message_update".to_string(),
                                                    data: serde_json::to_value(
                                                        MessageUpdateEvent {
                                                            message_id: msg_id,
                                                            message_type: target_message_type
                                                                .to_string(),
                                                            content: message.content.clone(),
                                                            is_done: true,
                                                            token_count: Some(total_tokens),
                                                            input_token_count: Some(input_tokens),
                                                            output_token_count: Some(output_tokens),
                                                            ttft_ms: message.ttft_ms,
                                                            tps,
                                                        },
                                                    )
                                                    .unwrap(),
                                                };
                                                let _ = window.emit(
                                                    format!(
                                                        "conversation_event_{}",
                                                        conversation_id
                                                    )
                                                    .as_str(),
                                                    update_event,
                                                );
                                            }
                                            Err(e) => {
                                                warn!(
                                                    message_id = msg_id,
                                                    error = %e,
                                                    "Failed to update message with token usage"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            debug!("No usage data in End event - AI provider may not support it");
                        }

                        // Info summary for easier debugging / visibility
                        info!(
                            response_chunks = response_chunk_count,
                            response_chars = response_char_count,
                            reasoning_chunks = reasoning_chunk_count,
                            reasoning_chars = reasoning_char_count,
                            response_len = response_content.chars().count(),
                            reasoning_len = reasoning_content.chars().count(),
                            has_response_message = response_message_id.is_some(),
                            captured_tool_calls = captured_tool_calls.len(),
                            "stream summary"
                        );

                        // If native tool calls were captured, persist UI hints and DB records, and optionally auto-run
                        if !captured_tool_calls.is_empty() {
                            // Ensure we have a response message to attach UI hints
                            if response_message_id.is_none() {
                                // Create a minimal response message to host MCP UI hints
                                let now = chrono::Utc::now();
                                response_start_time.get_or_insert(now);
                                match ensure_stream_message(
                                    &conversation_db,
                                    &window,
                                    conversation_id,
                                    "response",
                                    "",
                                    llm_model_id,
                                    &llm_model_name,
                                    &generation_group_id,
                                    parent_group_id_override.clone(),
                                    response_start_time,
                                    response_first_token_time.clone(),
                                    None,
                                )
                                .await
                                {
                                    Ok(new_id) => response_message_id = Some(new_id),
                                    Err(e) => {
                                        warn!(
                                            "Failed to create response message for MCP hints: {}",
                                            e
                                        );
                                    }
                                }
                            }
                            if let Some(msg_id) = response_message_id {
                                let _ = handle_captured_tool_calls_common(
                                    &app_handle,
                                    &conversation_db,
                                    &window,
                                    conversation_id,
                                    msg_id,
                                    &captured_tool_calls,
                                    &mut response_content,
                                    true,
                                    mcp_override_config.as_ref(),
                                    &tool_name_mapping,
                                )
                                .await;
                            }
                        }

                        // 按当前输出类型收尾，确保 response 触发 MCP 检测与事件
                        match current_output_type {
                            OutputType::Reasoning => {
                                if let (Some(msg_id), Some(start_time)) =
                                    (reasoning_message_id, reasoning_start_time)
                                {
                                    // 记录 reasoning 结束时间
                                    if reasoning_end_time.is_none() {
                                        reasoning_end_time = Some(chrono::Utc::now());
                                    }

                                    if let Err(e) = super::conversation::handle_message_type_end(
                                        msg_id,
                                        "reasoning",
                                        &reasoning_content,
                                        start_time,
                                        &conversation_db,
                                        &window,
                                        conversation_id,
                                        &app_handle,
                                        false, // allow MCP detection
                                    )
                                    .await
                                    {
                                        warn!(error = %e, "reasoning type end failed");
                                    }
                                }
                            }
                            OutputType::Response => {
                                if let (Some(msg_id), Some(start_time)) =
                                    (response_message_id, response_start_time)
                                {
                                    if let Err(e) = super::conversation::handle_message_type_end(
                                        msg_id,
                                        "response",
                                        &response_content,
                                        start_time,
                                        &conversation_db,
                                        &window,
                                        conversation_id,
                                        &app_handle,
                                        false, // allow MCP detection
                                    )
                                    .await
                                    {
                                        warn!(error = %e, "response type end failed");
                                    }
                                } else {
                                    // 兜底：如果缺少 start_time 或 msg_id，至少完成事件更新
                                    super::conversation::finish_stream_messages(
                                        &conversation_db,
                                        reasoning_message_id,
                                        response_message_id,
                                        &reasoning_content,
                                        &response_content,
                                        &window,
                                        conversation_id,
                                    )?;
                                }
                            }
                            OutputType::None => {
                                // 无活跃类型时，走统一收尾
                                super::conversation::finish_stream_messages(
                                    &conversation_db,
                                    reasoning_message_id,
                                    response_message_id,
                                    &reasoning_content,
                                    &response_content,
                                    &window,
                                    conversation_id,
                                )?;

                                // 明确记录无内容结束的情况
                                if response_chunk_count == 0 && reasoning_chunk_count == 0 {
                                    info!("stream ended with no content chunks");
                                }
                            }
                        }

                        // 工具调用事件已在 handle_captured_tool_calls_common 中按需发出

                        if need_generate_title && !response_content.is_empty() {
                            let app_handle_clone = app_handle.clone();
                            let user_prompt_clone = user_prompt.clone();
                            let content_clone = response_content.clone();
                            let config_feature_map_clone = config_feature_map.clone();
                            let window_clone = window.clone();

                            tokio::spawn(async move {
                                if let Err(e) = crate::api::ai::title::generate_title(
                                    &app_handle_clone,
                                    conversation_id,
                                    user_prompt_clone,
                                    content_clone,
                                    config_feature_map_clone,
                                    window_clone,
                                )
                                .await
                                {
                                    warn!(error = %e, "title generation failed");
                                }
                            });
                        }

                        // 获取助手名称并发送完成通知
                        let assistant_name = if let Ok(Some(conv)) =
                            conversation_db.conversation_repo().unwrap().read(conversation_id)
                        {
                            if let Some(assistant_id) = conv.assistant_id {
                                if let Ok(assistant) = crate::api::assistant_api::get_assistant(
                                    app_handle.clone(),
                                    assistant_id,
                                ) {
                                    Some(assistant.assistant.name.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        send_completion_notification(
                            app_handle,
                            &response_content,
                            assistant_name,
                            &config_feature_map,
                        )
                        .await;

                        // 无论是否产生内容，都向前端发送一个流式完成事件，确保 UI 能正确退出接收状态
                        let stream_complete_event = ConversationEvent {
                            r#type: "stream_complete".to_string(),
                            data: serde_json::json!({
                                "conversation_id": conversation_id,
                                "response_message_id": response_message_id,
                                "reasoning_message_id": reasoning_message_id,
                                "has_response": response_message_id.is_some(),
                                "has_reasoning": reasoning_message_id.is_some(),
                                "response_length": response_content.len(),
                                "reasoning_length": reasoning_content.len(),
                            }),
                        };
                        let _ = window.emit(
                            format!("conversation_event_{}", conversation_id).as_str(),
                            stream_complete_event,
                        );

                        return Ok(());
                    }
                }
            }
            Some(Err(e)) => {
                let _user_friendly_error = enhanced_error_logging_v2(&e, "Stream Processing").await;
                return Err(anyhow::anyhow!("Stream processing failed: {}", e));
            }
            None => break,
        }
    }

    Ok(())
}

// 辅助函数：创建错误消息
async fn create_error_message(
    conversation_db: &ConversationDatabase,
    conversation_id: i64,
    llm_model_id: i64,
    llm_model_name: String,
    error_msg: &str,
    generation_group_id_override: Option<String>,
    parent_group_id_override: Option<String>,
    window: &tauri::Window,
) {
    let now = chrono::Utc::now();
    let generation_group_id =
        generation_group_id_override.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if let Ok(error_message) = conversation_db.message_repo().unwrap().create(&Message {
        id: 0,
        parent_id: None,
        conversation_id,
        message_type: "error".to_string(),
        content: error_msg.to_string(),
        llm_model_id: Some(llm_model_id),
        llm_model_name: Some(llm_model_name),
        created_time: now,
        start_time: Some(now),
        finish_time: Some(now),
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: Some(generation_group_id),
        parent_group_id: parent_group_id_override,
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    }) {
        let error_event = ConversationEvent {
            r#type: "message_add".to_string(),
            data: serde_json::to_value(MessageAddEvent {
                message_id: error_message.id,
                message_type: "error".to_string(),
            })
            .unwrap(),
        };
        let _ =
            window.emit(format!("conversation_event_{}", conversation_id).as_str(), error_event);

        let update_event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id: error_message.id,
                message_type: "error".to_string(),
                content: error_msg.to_string(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        };
        let _ =
            window.emit(format!("conversation_event_{}", conversation_id).as_str(), update_event);
    }
}

pub async fn handle_non_stream_chat(
    client: &Client,
    model_name: &str,
    chat_request: &ChatRequest,
    chat_options: &ChatOptions,
    conversation_id: i64,
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    app_handle: &tauri::AppHandle,
    need_generate_title: bool,
    user_prompt: String,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    generation_group_id_override: Option<String>,
    parent_group_id_override: Option<String>,
    llm_model_id: i64,
    llm_model_name: String,
    mcp_override_config: Option<McpOverrideConfig>,
    tool_name_mapping: ToolNameMapping,
) -> Result<(), anyhow::Error> {
    let generation_group_id =
        generation_group_id_override.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // 记录请求开始时间，用于估算非流式响应的 TTFT
    let request_start_time = chrono::Utc::now();

    // 从配置中获取最大重试次数
    let max_retry_attempts = get_retry_attempts_from_config(&config_feature_map);

    // 非流式：强制捕获工具调用，便于将工具以 UI 注释形式插入
    let non_stream_options = chat_options.clone().with_capture_tool_calls(true);

    debug!("non_stream chat_request messages");
    for (i, msg) in chat_request.messages.iter().enumerate() {
        debug!(message_index = i, role = ?msg.role, "non stream message");
        match &msg.role {
            genai::chat::ChatRole::Assistant => {
                if let Some(text) = msg.content.first_text() {
                    debug!(preview = %text.chars().take(100).collect::<String>(), "assistant content non stream");
                } else if msg.content.contains_tool_call() {
                    let tool_calls = msg.content.tool_calls();
                    debug!(tool_calls_count = tool_calls.len(), "assistant tool calls non stream");
                    for tc in tool_calls {
                        debug!(call_id = %tc.call_id, fn_name = %tc.fn_name, "tool call item non stream");
                    }
                } else {
                    debug!("assistant content other type non stream");
                }
            }
            genai::chat::ChatRole::Tool => {
                if let Some(text) = msg.content.first_text() {
                    debug!(
                        "    Tool response content: {}",
                        text.chars().take(100).collect::<String>()
                    );
                } else {
                    debug!("tool response other type non stream");
                }
            }
            _ => {
                if let Some(text) = msg.content.first_text() {
                    debug!(preview = %text.chars().take(100).collect::<String>(), "other content non stream");
                } else {
                    debug!("other content other type non stream");
                }
            }
        }
    }

    let chat_result = {
        let mut attempts = 0;
        loop {
            attempts += 1;

            info!(attempts, max_retry_attempts, "non stream chat attempt");

            match client
                .exec_chat(model_name, chat_request.clone(), Some(&non_stream_options))
                .await
            {
                Ok(response) => {
                    info!(attempts, "non stream chat succeeded attempt");
                    break Ok(response);
                }
                Err(e) => {
                    let _ = enhanced_error_logging_v2(
                        &e,
                        &format!("Non-Stream Chat (attempt {}/{})", attempts, max_retry_attempts),
                    )
                    .await;
                    if attempts >= max_retry_attempts {
                        let http_details = extract_http_error_details(&e);
                        let raw_error = e.to_string();
                        let final_error =
                            http_details.response_body.clone().unwrap_or_else(|| raw_error.clone());

                        error!(attempts, error = %e, final_error, "final non stream chat error");

                        // 发送错误通知到合适的窗口
                        send_error_to_appropriate_window(
                            &window,
                            &final_error,
                            Some(conversation_id),
                        );

                        break Err(anyhow::anyhow!("{}", final_error));
                    }

                    let delay = calculate_retry_delay(attempts);
                    debug!(delay_ms = delay, "retrying non-stream after delay");
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    };

    match chat_result {
        Ok(chat_response) => {
            // 在创建新的 response 消息前，如果上一条是错误消息，则清理
            let _ = cleanup_last_error_message(conversation_db, conversation_id).await;

            let mut content = chat_response.first_text().unwrap_or("").to_string();

            // Extract token usage data
            let usage = &chat_response.usage;
            let input_tokens = usage.prompt_tokens.unwrap_or(0);
            let output_tokens = usage.completion_tokens.unwrap_or(0);
            let total_tokens = usage.total_tokens.unwrap_or(input_tokens + output_tokens);

            // 现在才创建响应消息（在有实际内容后）
            let now = chrono::Utc::now();

            // 对于非流式响应，估算 TTFT 和 TPS
            // TTFT = 总响应时间 (无法区分首字时间)
            // TPS = output_tokens / 总响应时间
            let estimated_duration_ms =
                now.timestamp_millis() - request_start_time.timestamp_millis();
            let ttft_ms =
                if estimated_duration_ms > 0 { Some(estimated_duration_ms) } else { None };
            let tps = if output_tokens > 0 && estimated_duration_ms > 0 {
                Some((output_tokens as f64) * 1000.0 / estimated_duration_ms as f64)
            } else {
                None
            };

            let response_message = conversation_db
                .message_repo()
                .unwrap()
                .create(&Message {
                    id: 0,
                    parent_id: None,
                    conversation_id,
                    message_type: "response".to_string(),
                    content: content.clone(),
                    llm_model_id: Some(llm_model_id),
                    llm_model_name: Some(llm_model_name.clone()),
                    // Non-stream responses don't have per-token timestamps. Use request_start_time
                    // and the time we received the full response to make duration-based TPS sane.
                    created_time: now,
                    start_time: Some(request_start_time),
                    finish_time: Some(now),
                    token_count: total_tokens,
                    input_token_count: input_tokens,
                    output_token_count: output_tokens,
                    generation_group_id: Some(generation_group_id.clone()),
                    parent_group_id: parent_group_id_override.clone(),
                    tool_calls_json: None,
                    first_token_time: None, // non-stream: unknown, fallback to start_time
                    ttft_ms,
                })
                .unwrap();

            info!(
                message_id = response_message.id,
                input_tokens,
                output_tokens,
                total_tokens,
                "Token usage captured for non-streaming response"
            );
            let response_message_id = response_message.id;

            // 现在才发送 message_add 事件（消息有内容时）
            let add_event = ConversationEvent {
                r#type: "message_add".to_string(),
                data: serde_json::to_value(MessageAddEvent {
                    message_id: response_message_id,
                    message_type: "response".to_string(),
                })
                .unwrap(),
            };
            let _ =
                window.emit(format!("conversation_event_{}", conversation_id).as_str(), add_event);

            // 立即发送一个 is_done: false 的 message_update 事件，触发前端清理用户消息的 shine-border
            // 这与流式模式的行为保持一致
            let initial_update_event = ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(MessageUpdateEvent {
                    message_id: response_message_id,
                    message_type: "response".to_string(),
                    content: content.clone(),
                    is_done: false, // 关键：设置为 false 以触发前端的 shine-border 清理逻辑
                    token_count: None,
                    input_token_count: None,
                    output_token_count: None,
                    ttft_ms: None,
                    tps: None,
                })
                .unwrap(),
            };
            let _ = window.emit(
                format!("conversation_event_{}", conversation_id).as_str(),
                initial_update_event,
            );

            // 非流式：捕获原生 ToolCall 并处理（创建DB、UI注释、自动执行）
            let tool_calls: Vec<ToolCall> =
                chat_response.tool_calls().into_iter().map(|tc| tc.clone()).collect();

            if !tool_calls.is_empty() {
                debug!(tool_calls_count = tool_calls.len(), "non stream captured tool calls count");

                // 统一处理（会覆盖 tool_calls_json，插入 UI 注释，并 emit 事件）
                let _ = handle_captured_tool_calls_common(
                    app_handle,
                    conversation_db,
                    window,
                    conversation_id,
                    response_message_id,
                    &tool_calls,
                    &mut content,
                    true,
                    mcp_override_config.as_ref(),
                    &tool_name_mapping,
                )
                .await;
            }

            // 非流式场景下补充基于提示词的 MCP 检测，确保 prompt 模式生效
            match crate::mcp::detect_and_process_mcp_calls(
                app_handle,
                window,
                conversation_id,
                response_message_id,
                &content,
            )
            .await
            {
                Ok(updated) => {
                    if let Some(new_content) = updated {
                        content = new_content;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "failed to detect MCP calls in non-stream response");
                }
            }

            let mut message =
                conversation_db.message_repo().unwrap().read(response_message_id).unwrap().unwrap();
            message.content = content.clone();
            conversation_db.message_repo().unwrap().update(&message).unwrap();

            // finish_time already stored with ms precision at creation time for non-stream responses

            let update_event = ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(MessageUpdateEvent {
                    message_id: response_message_id,
                    message_type: "response".to_string(),
                    content: content.clone(),
                    is_done: true,
                    token_count: Some(total_tokens),
                    input_token_count: Some(input_tokens),
                    output_token_count: Some(output_tokens),
                    ttft_ms,
                    tps,
                })
                .unwrap(),
            };
            let _ = window
                .emit(format!("conversation_event_{}", conversation_id).as_str(), update_event);

            if need_generate_title && !content.is_empty() {
                let app_handle_clone = app_handle.clone();
                let user_prompt_clone = user_prompt.clone();
                let content_clone = content.clone();
                let config_feature_map_clone = config_feature_map.clone();
                let window_clone = window.clone();

                tokio::spawn(async move {
                    if let Err(e) = crate::api::ai::title::generate_title(
                        &app_handle_clone,
                        conversation_id,
                        user_prompt_clone,
                        content_clone,
                        config_feature_map_clone,
                        window_clone,
                    )
                    .await
                    {
                        warn!(error = %e, "title generation failed (non-stream)");
                    }
                });
            }

            // 获取助手名称并发送完成通知
            let assistant_name = if let Ok(Some(conv)) =
                conversation_db.conversation_repo().unwrap().read(conversation_id)
            {
                if let Some(assistant_id) = conv.assistant_id {
                    if let Ok(assistant) =
                        crate::api::assistant_api::get_assistant(app_handle.clone(), assistant_id)
                    {
                        Some(assistant.assistant.name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            send_completion_notification(app_handle, &content, assistant_name, &config_feature_map)
                .await;

            Ok(())
        }
        Err(e) => {
            // 提取 HTTP 错误详情
            let http_details = extract_http_details_from_anyhow(&e);
            let user_friendly_error = get_user_friendly_error_message(&e);

            let err_msg = build_rich_error_payload_with_http_details(
                format!("AI请求失败: {}", user_friendly_error),
                None,
                Some(llm_model_name.clone()),
                "non_stream",
                None,
                e.to_string(),
                Some(http_details),
            );
            let now = chrono::Utc::now();
            send_error_to_appropriate_window(&window, &user_friendly_error, Some(conversation_id));

            let generation_group_id = generation_group_id_override
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let error_message = conversation_db
                .message_repo()
                .unwrap()
                .create(&Message {
                    id: 0,
                    parent_id: None,
                    conversation_id,
                    message_type: "error".to_string(),
                    content: err_msg.clone(),
                    llm_model_id: Some(llm_model_id),
                    llm_model_name: Some(llm_model_name.clone()),
                    created_time: now,
                    start_time: Some(now),
                    finish_time: Some(now),
                    token_count: 0,
                    input_token_count: 0,
                    output_token_count: 0,
                    generation_group_id: Some(generation_group_id.clone()),
                    parent_group_id: parent_group_id_override.clone(),
                    tool_calls_json: None,
                    first_token_time: None,
                    ttft_ms: None,
                })
                .unwrap();

            let error_event = ConversationEvent {
                r#type: "message_add".to_string(),
                data: serde_json::to_value(MessageAddEvent {
                    message_id: error_message.id,
                    message_type: "error".to_string(),
                })
                .unwrap(),
            };
            let _ = window
                .emit(format!("conversation_event_{}", conversation_id).as_str(), error_event);

            let update_event = ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(MessageUpdateEvent {
                    message_id: error_message.id,
                    message_type: "error".to_string(),
                    content: err_msg.clone(),
                    is_done: true,
                    token_count: None,
                    input_token_count: None,
                    output_token_count: None,
                    ttft_ms: None,
                    tps: None,
                })
                .unwrap(),
            };
            let _ = window
                .emit(format!("conversation_event_{}", conversation_id).as_str(), update_event);

            error!(error = %e, "chat error");
            Err(anyhow::anyhow!("Chat error: {}", e))
        }
    }
}

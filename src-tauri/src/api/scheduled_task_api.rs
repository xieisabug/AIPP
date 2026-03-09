use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::api::ai::config::{
    get_network_proxy_from_config, get_request_timeout_from_config, ConfigBuilder,
};
use crate::api::ai::conversation::{
    build_chat_request_from_messages, ToolCallStrategy, ToolConfig,
};
use crate::api::ai::summary::extract_json_from_response;
use crate::api::ai_api::{build_tools_with_mapping, resolve_tool_name, ToolNameMapping};
use crate::api::assistant_api::get_assistant;
use crate::api::genai_client::create_client_with_config;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{
    ConversationDatabase, MessageAttachment, Repository as ConversationRepository,
};
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::MCPDatabase;
use crate::db::scheduled_task_db::{
    ScheduledTask, ScheduledTaskDatabase, ScheduledTaskLog, ScheduledTaskRun,
};
use crate::db::system_db::FeatureConfig;
use crate::mcp::{collect_mcp_info_for_assistant, format_mcp_prompt, MCPInfoForAssistant};
use crate::skills::{collect_skills_info_for_assistant, format_skills_prompt};
use crate::template_engine::build_template_engine;
use crate::{AppState, FeatureConfigState, NameCacheState};
use genai::chat::{ChatOptions, ToolCall};
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskDTO {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String,
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>,
    pub start_time: Option<String>,
    pub week_days: Option<Vec<i32>>,
    pub month_days: Option<Vec<i32>>,
    pub run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
    pub created_time: String,
    pub updated_time: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledTaskRequest {
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String, // 'once' | 'interval'
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>, // minute/hour/day/week/month
    pub start_time: Option<String>,    // HH:mm for day/week/month
    pub week_days: Option<Vec<i32>>,   // [0-6] for week
    pub month_days: Option<Vec<i32>>,  // [1-31] for month
    pub run_at: Option<String>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledTaskRequest {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String,
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>,
    pub start_time: Option<String>,
    pub week_days: Option<Vec<i32>>,
    pub month_days: Option<Vec<i32>>,
    pub run_at: Option<String>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunScheduledTaskResult {
    pub task_id: i64,
    pub success: bool,
    pub notify: bool,
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskLogDTO {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub message_type: String,
    pub content: String,
    pub created_time: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListScheduledTaskLogsResponse {
    pub logs: Vec<ScheduledTaskLogDTO>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskRunDTO {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub status: String,
    pub notify: bool,
    pub summary: Option<String>,
    pub error_message: Option<String>,
    pub started_time: String,
    pub finished_time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListScheduledTaskRunsResponse {
    pub runs: Vec<ScheduledTaskRunDTO>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ScheduledTaskRunUpdateEvent {
    pub run_id: String,
    pub status: String,
    pub notify: bool,
    pub summary: Option<String>,
    pub error_message: Option<String>,
    pub finished_time: Option<String>,
}

const AGENTIC_LOOP_MAX_ROUNDS: usize = 50;
const AGENTIC_LOOP_PER_TOOL_TIMEOUT_SECS: u64 = 120;
const AGENTIC_LOOP_TOTAL_TIMEOUT_SECS: u64 = 3000;
const SCHEDULED_TASK_LOG_ADDED_EVENT: &str = "scheduled_task_log_added";
const SCHEDULED_TASK_RUN_CREATED_EVENT: &str = "scheduled_task_run_created";
const SCHEDULED_TASK_RUN_UPDATED_EVENT: &str = "scheduled_task_run_updated";
type ScheduledRunCancelRegistry = Arc<Mutex<HashMap<String, CancellationToken>>>;
static SCHEDULED_RUN_CANCEL_REGISTRY: OnceLock<ScheduledRunCancelRegistry> = OnceLock::new();

fn scheduled_run_cancel_registry() -> &'static ScheduledRunCancelRegistry {
    SCHEDULED_RUN_CANCEL_REGISTRY.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

async fn register_scheduled_run_cancel_token(run_id: &str) -> CancellationToken {
    let token = CancellationToken::new();
    let mut registry = scheduled_run_cancel_registry().lock().await;
    registry.insert(run_id.to_string(), token.clone());
    token
}

async fn cancel_scheduled_run(run_id: &str) -> bool {
    let token_opt = {
        let registry = scheduled_run_cancel_registry().lock().await;
        registry.get(run_id).cloned()
    };
    if let Some(token) = token_opt {
        token.cancel();
        true
    } else {
        false
    }
}

async fn unregister_scheduled_run_cancel_token(run_id: &str) {
    let mut registry = scheduled_run_cancel_registry().lock().await;
    registry.remove(run_id);
}

// ── Agentic Loop Types ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum AgenticLoopStatus {
    Completed,
    Cancelled,
    MaxRoundsReached,
    Timeout,
    Error(String),
}

impl fmt::Display for AgenticLoopStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgenticLoopStatus::Completed => write!(f, "completed"),
            AgenticLoopStatus::Cancelled => write!(f, "cancelled"),
            AgenticLoopStatus::MaxRoundsReached => write!(f, "max_rounds_reached"),
            AgenticLoopStatus::Timeout => write!(f, "timeout"),
            AgenticLoopStatus::Error(e) => write!(f, "error: {}", e),
        }
    }
}

#[derive(Debug, Clone)]
struct AgenticLoopResult {
    final_text: String,
    rounds: usize,
    tool_calls_total: usize,
    tool_calls_success: usize,
    tool_calls_failed: usize,
    status: AgenticLoopStatus,
}

struct AgenticLoopContext<'a> {
    app_handle: &'a tauri::AppHandle,
    client: &'a genai::Client,
    model_name: &'a str,
    chat_options: &'a ChatOptions,
    conversation_id: i64,
    conversation_db: &'a ConversationDatabase,
    tool_name_mapping: &'a ToolNameMapping,
    tool_config: Option<ToolConfig>,
    tool_call_strategy: ToolCallStrategy,
    llm_model_id: i64,
    llm_model_code: String,
    task_id: i64,
    run_id: &'a str,
    cancel_token: CancellationToken,
}

#[derive(Debug, Clone)]
pub(crate) struct NotifyDecision {
    pub(crate) task_state: Option<String>,
    pub(crate) notify: bool,
    pub(crate) summary: Option<String>,
    pub(crate) reason: Option<String>,
}

fn format_dt(dt: Option<DateTime<Utc>>) -> Option<String> {
    dt.map(|v| v.to_rfc3339())
}

pub(crate) fn parse_local_datetime(input: &str) -> Result<DateTime<Utc>, String> {
    let trimmed = input.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc));
    }
    let formats = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"];
    for fmt in &formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            let local_dt = Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "无法解析本地时间".to_string())?;
            return Ok(local_dt.with_timezone(&Utc));
        }
    }
    Err("无法解析时间，请使用 YYYY-MM-DD HH:MM:SS 格式".to_string())
}

// ── Agentic Loop Implementation ─────────────────────────────────────────

/// 将 LLM ToolCall 的 arguments 归一化为 JSON 对象字符串
pub(crate) fn normalize_tool_arguments(arguments: &serde_json::Value) -> String {
    if arguments.is_object() {
        arguments.to_string()
    } else {
        "{}".to_string()
    }
}

/// 从 prompt 模式的 `<mcp_tool_call>` 标签中提取工具调用。
///
/// 返回值：
/// - `Vec<ToolCall>`: 解析出的工具调用（用于后续统一执行）
/// - `String`: 去除 `<mcp_tool_call>` 标签后的文本
pub(crate) fn extract_prompt_tool_calls(content: &str) -> (Vec<ToolCall>, String) {
    let mcp_regex = regex::Regex::new(r"<mcp_tool_call>\s*<server_name>([^<]*)</server_name>\s*<tool_name>([^<]*)</tool_name>\s*<parameters>([\s\S]*?)</parameters>\s*</mcp_tool_call>")
        .expect("valid mcp tool call regex");

    let mut tool_calls = Vec::new();
    for cap in mcp_regex.captures_iter(content) {
        let Some(server_name) = cap.get(1).map(|m| m.as_str().trim()).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(tool_name) = cap.get(2).map(|m| m.as_str().trim()).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let parameters_raw = cap.get(3).map(|m| m.as_str().trim()).unwrap_or("{}");
        let fn_arguments = serde_json::from_str::<serde_json::Value>(parameters_raw)
            .ok()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({}));

        tool_calls.push(ToolCall {
            call_id: Uuid::new_v4().to_string(),
            fn_name: crate::api::ai_api::build_tool_name(server_name, tool_name),
            fn_arguments,
            thought_signatures: None,
        });
    }

    if tool_calls.is_empty() {
        (tool_calls, content.to_string())
    } else {
        let sanitized_content = mcp_regex.replace_all(content, "").to_string();
        (tool_calls, sanitized_content)
    }
}

#[derive(Debug, Clone)]
struct PreparedAgenticToolCall {
    llm_call_id: String,
    call_db_id: Option<i64>,
    call_record_error: Option<String>,
    server_name: String,
    tool_name: String,
    params_str: String,
    params_json: serde_json::Value,
}

pub(crate) fn build_mcp_tool_call_ui_hint(
    server_name: &str,
    tool_name: &str,
    parameters: &str,
    call_db_id: Option<i64>,
    llm_call_id: &str,
) -> String {
    let payload = match call_db_id {
        Some(call_id) => serde_json::json!({
            "server_name": server_name,
            "tool_name": tool_name,
            "parameters": parameters,
            "call_id": call_id,
            "llm_call_id": llm_call_id,
        }),
        None => serde_json::json!({
            "server_name": server_name,
            "tool_name": tool_name,
            "parameters": parameters,
            "llm_call_id": llm_call_id,
        }),
    };
    format!("\n\n<!-- MCP_TOOL_CALL:{} -->\n", payload)
}

async fn create_scheduled_tool_call_record(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    response_message_id: Option<i64>,
    server_name: &str,
    tool_name: &str,
    parameters: &str,
    llm_call_id: &str,
) -> Result<i64, String> {
    crate::mcp::execution_api::create_mcp_tool_call_with_llm_id(
        app_handle.clone(),
        conversation_id,
        response_message_id,
        server_name.to_string(),
        tool_name.to_string(),
        parameters.to_string(),
        Some(llm_call_id),
        response_message_id,
    )
    .await
    .map(|record| record.id)
}

/// 执行单个工具调用，直接调用底层 execute_tool_by_transport
///
/// 返回 (is_success, result_or_error_text)
async fn execute_single_tool(
    app_handle: &tauri::AppHandle,
    call_db_id: i64,
    timeout: Duration,
    cancel_token: Option<CancellationToken>,
) -> (bool, String) {
    let mcp_db = match MCPDatabase::new(app_handle) {
        Ok(db) => db,
        Err(e) => {
            let err = format!("Error: 初始化 MCP 数据库失败: {}", e);
            return (false, err);
        }
    };

    let mut record = match mcp_db.get_mcp_tool_call(call_db_id) {
        Ok(record) => record,
        Err(e) => {
            let err = format!("Error: 获取工具调用记录失败: {}", e);
            return (false, err);
        }
    };

    // 2. Mark executing
    match mcp_db.mark_mcp_tool_call_executing_if_pending(call_db_id) {
        Ok(true) => {}
        Ok(false) => {
            if record.status == "success" {
                return (true, record.result.unwrap_or_default());
            }
            if record.status == "failed" {
                let err = record.error.unwrap_or_else(|| "工具执行失败".to_string());
                return (false, format!("Error: {}", err));
            }
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "executing", None, None);
        }
        Err(e) => {
            let err = format!("Error: 更新工具状态失败: {}", e);
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "failed", None, Some(&err));
            return (false, err);
        }
    }

    if let Ok(latest_record) = mcp_db.get_mcp_tool_call(call_db_id) {
        record = latest_record;
    }

    // 3. 获取 MCPServer 并执行工具
    let server = match mcp_db.get_mcp_server(record.server_id) {
        Ok(s) => s,
        Err(e) => {
            let err = format!("Error: 获取 MCP 服务器失败: {}", e);
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "failed", None, Some(&err));
            return (false, err);
        }
    };

    let feature_config_state = app_handle.state::<FeatureConfigState>();
    if let Some(token) = cancel_token.as_ref() {
        if token.is_cancelled() {
            let err = "Error: 工具执行已取消".to_string();
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "failed", None, Some(&err));
            return (false, err);
        }
    }
    let exec_result = tokio::time::timeout(
        timeout,
        crate::mcp::execution_api::execute_tool_by_transport(
            app_handle,
            &feature_config_state,
            &server,
            &record.tool_name,
            &record.parameters,
            Some(record.conversation_id),
            cancel_token,
        ),
    )
    .await;

    // 4. 处理结果
    match exec_result {
        Ok(Ok(result)) => {
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "success", Some(&result), None);
            (true, result)
        }
        Ok(Err(e)) => {
            let err_str = format!("Error: {}", e);
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "failed", None, Some(&err_str));
            (false, err_str)
        }
        Err(_) => {
            let err_str = format!("Error: 工具执行超时 ({}s)", timeout.as_secs());
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "failed", None, Some(&err_str));
            (false, err_str)
        }
    }
}

/// Agentic Loop 核心：同步多轮 LLM + 工具调用循环
///
/// 在循环内部控制整个流程，无需外部轮询或状态机。
/// - LLM 返回 tool_calls → 执行工具 → 结果追加到消息 → 下一轮
/// - LLM 返回纯文本 → 循环结束，final_text 就是最终结果
async fn run_task_agentic_loop(ctx: &AgenticLoopContext<'_>) -> AgenticLoopResult {
    let mut messages: Vec<(String, String, Vec<MessageAttachment>)> = Vec::new();
    let mut round: usize = 0;
    let mut tool_calls_total: usize = 0;
    let mut tool_calls_success: usize = 0;
    let mut tool_calls_failed: usize = 0;
    let mut final_text = String::new();
    let deadline = Instant::now() + Duration::from_secs(AGENTIC_LOOP_TOTAL_TIMEOUT_SECS);

    // 从 DB 中读取初始消息（system + user）构建内存消息列表
    let all_db_messages =
        match ctx.conversation_db.message_repo().map_err(|e| e.to_string()).and_then(|repo| {
            repo.list_by_conversation_id(ctx.conversation_id).map_err(|e| e.to_string())
        }) {
            Ok(msgs) => msgs,
            Err(e) => {
                return AgenticLoopResult {
                    final_text: String::new(),
                    rounds: 0,
                    tool_calls_total: 0,
                    tool_calls_success: 0,
                    tool_calls_failed: 0,
                    status: AgenticLoopStatus::Error(format!("读取初始消息失败: {}", e)),
                };
            }
        };
    for (msg, attachment_opt) in &all_db_messages {
        let attachments = attachment_opt.iter().cloned().collect::<Vec<_>>();
        messages.push((msg.message_type.clone(), msg.content.clone(), attachments));
    }

    let generation_group_id = Uuid::new_v4().to_string();

    loop {
        if ctx.cancel_token.is_cancelled() {
            info!(task_id = ctx.task_id, round, "agentic loop cancelled by user");
            log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "cancel", "任务已停止");
            return AgenticLoopResult {
                final_text,
                rounds: round,
                tool_calls_total,
                tool_calls_success,
                tool_calls_failed,
                status: AgenticLoopStatus::Cancelled,
            };
        }

        // 超时检查
        if Instant::now() >= deadline {
            warn!(task_id = ctx.task_id, round, "agentic loop total timeout");
            log_task_message(
                ctx.app_handle,
                ctx.task_id,
                ctx.run_id,
                "timeout",
                format!("总超时 ({}s)，已完成 {} 轮", AGENTIC_LOOP_TOTAL_TIMEOUT_SECS, round),
            );
            return AgenticLoopResult {
                final_text,
                rounds: round,
                tool_calls_total,
                tool_calls_success,
                tool_calls_failed,
                status: AgenticLoopStatus::Timeout,
            };
        }

        // 轮次检查
        if round >= AGENTIC_LOOP_MAX_ROUNDS {
            warn!(task_id = ctx.task_id, round, "agentic loop max rounds reached");
            log_task_message(
                ctx.app_handle,
                ctx.task_id,
                ctx.run_id,
                "max_rounds",
                format!("达到最大轮次 ({})", AGENTIC_LOOP_MAX_ROUNDS),
            );
            return AgenticLoopResult {
                final_text,
                rounds: round,
                tool_calls_total,
                tool_calls_success,
                tool_calls_failed,
                status: AgenticLoopStatus::MaxRoundsReached,
            };
        }

        round += 1;
        info!(task_id = ctx.task_id, round, "agentic loop round start");

        // 构建 ChatRequest
        let build_result = build_chat_request_from_messages(
            &messages,
            ctx.tool_call_strategy,
            ctx.tool_config.clone(),
        );
        let chat_request = build_result.chat_request;

        // 调用 LLM
        let chat_response = match ctx
            .client
            .exec_chat(ctx.model_name, chat_request, Some(ctx.chat_options))
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                error!(task_id = ctx.task_id, round, error = %e, "LLM call failed in agentic loop");
                // 重试一次
                log_task_message(
                    ctx.app_handle,
                    ctx.task_id,
                    ctx.run_id,
                    "llm_retry",
                    format!("第 {} 轮 LLM 调用失败，重试: {}", round, e),
                );
                let retry_request = build_chat_request_from_messages(
                    &messages,
                    ctx.tool_call_strategy,
                    ctx.tool_config.clone(),
                );
                match ctx
                    .client
                    .exec_chat(ctx.model_name, retry_request.chat_request, Some(ctx.chat_options))
                    .await
                {
                    Ok(resp) => resp,
                    Err(e2) => {
                        return AgenticLoopResult {
                            final_text,
                            rounds: round,
                            tool_calls_total,
                            tool_calls_success,
                            tool_calls_failed,
                            status: AgenticLoopStatus::Error(format!("LLM 调用失败: {}", e2)),
                        };
                    }
                }
            }
        };

        // 提取文本和 tool_calls
        let mut response_text = chat_response.first_text().unwrap_or("").to_string();
        let mut captured_tool_calls: Vec<ToolCall> =
            chat_response.tool_calls().into_iter().cloned().collect();
        if captured_tool_calls.is_empty() {
            let (prompt_tool_calls, sanitized_text) = extract_prompt_tool_calls(&response_text);
            if !prompt_tool_calls.is_empty() {
                debug!(
                    task_id = ctx.task_id,
                    round,
                    prompt_tool_calls_count = prompt_tool_calls.len(),
                    "agentic loop parsed prompt mode mcp calls"
                );
                captured_tool_calls = prompt_tool_calls;
                response_text = sanitized_text;
            }
        }

        debug!(
            task_id = ctx.task_id,
            round,
            text_len = response_text.len(),
            tool_calls_count = captured_tool_calls.len(),
            "agentic loop LLM response"
        );

        // 保存 response 消息到 DB（含 MCP_TOOL_CALL 注释）
        let mut response_content = response_text.clone();
        let response_message = crate::api::ai_api::add_message(
            ctx.app_handle,
            None,
            ctx.conversation_id,
            "response".to_string(),
            response_content.clone(),
            Some(ctx.llm_model_id),
            Some(ctx.llm_model_code.clone()),
            Some(Utc::now()),
            if captured_tool_calls.is_empty() { Some(Utc::now()) } else { None },
            0,
            Some(generation_group_id.clone()),
            None,
        );
        let response_message_id = match response_message {
            Ok(msg) => msg.id,
            Err(e) => {
                warn!(error = %e, "failed to save response message in agentic loop");
                0
            }
        };

        // 无工具调用 → 循环结束
        if captured_tool_calls.is_empty() {
            final_text = response_text;
            info!(task_id = ctx.task_id, round, "agentic loop completed (no tool calls)");
            return AgenticLoopResult {
                final_text,
                rounds: round,
                tool_calls_total,
                tool_calls_success,
                tool_calls_failed,
                status: AgenticLoopStatus::Completed,
            };
        }

        // 有工具调用 → 逐个执行
        log_task_message(
            ctx.app_handle,
            ctx.task_id,
            ctx.run_id,
            "tool_round",
            format!("第 {} 轮：{} 个工具调用", round, captured_tool_calls.len()),
        );

        // 先创建 tool_call 记录并写入带 call_id 的 MCP_TOOL_CALL 注释，保证后续可追踪完整状态
        let mut prepared_tool_calls: Vec<PreparedAgenticToolCall> = Vec::new();
        for tc in &captured_tool_calls {
            let (server_name, tool_name) = resolve_tool_name(&tc.fn_name, ctx.tool_name_mapping);
            let params_str = normalize_tool_arguments(&tc.fn_arguments);
            let params_json = serde_json::from_str::<serde_json::Value>(&params_str)
                .unwrap_or_else(|_| serde_json::Value::String(params_str.clone()));

            let response_message_id_opt =
                if response_message_id > 0 { Some(response_message_id) } else { None };
            let (call_db_id, call_record_error) = match create_scheduled_tool_call_record(
                ctx.app_handle,
                ctx.conversation_id,
                response_message_id_opt,
                &server_name,
                &tool_name,
                &params_str,
                &tc.call_id,
            )
            .await
            {
                Ok(call_id) => (Some(call_id), None),
                Err(e) => {
                    warn!(
                        task_id = ctx.task_id,
                        round,
                        llm_call_id = %tc.call_id,
                        server = %server_name,
                        tool = %tool_name,
                        error = %e,
                        "failed to create mcp tool call record for scheduled task round"
                    );
                    (None, Some(format!("Error: failed to create tool call record: {}", e)))
                }
            };

            response_content.push_str(&build_mcp_tool_call_ui_hint(
                &server_name,
                &tool_name,
                &params_str,
                call_db_id,
                &tc.call_id,
            ));
            prepared_tool_calls.push(PreparedAgenticToolCall {
                llm_call_id: tc.call_id.clone(),
                call_db_id,
                call_record_error,
                server_name,
                tool_name,
                params_str,
                params_json,
            });
        }

        // 保存含 tool_calls 注释的完整 response 内容和 tool_calls_json
        if response_message_id > 0 {
            if let Ok(repo) = ctx.conversation_db.message_repo() {
                if let Ok(Some(mut msg)) = repo.read(response_message_id) {
                    msg.content = response_content.clone();
                    msg.tool_calls_json = serde_json::to_string(&captured_tool_calls).ok();
                    let _ = repo.update(&msg);
                }
            }
        }

        // 把 assistant response（带 tool_calls 注释）加入内存消息
        messages.push(("response".to_string(), response_content.clone(), Vec::new()));

        // 执行每个工具并收集结果
        for tool_call in &prepared_tool_calls {
            if ctx.cancel_token.is_cancelled() {
                info!(task_id = ctx.task_id, round, "agentic loop cancelled before tool execution");
                log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "cancel", "任务已停止");
                return AgenticLoopResult {
                    final_text,
                    rounds: round,
                    tool_calls_total,
                    tool_calls_success,
                    tool_calls_failed,
                    status: AgenticLoopStatus::Cancelled,
                };
            }

            tool_calls_total += 1;
            info!(
                task_id = ctx.task_id, round,
                server = %tool_call.server_name, tool = %tool_call.tool_name,
                llm_call_id = %tool_call.llm_call_id,
                call_db_id = ?tool_call.call_db_id,
                "agentic loop executing tool"
            );
            log_task_message(
                ctx.app_handle,
                ctx.task_id,
                ctx.run_id,
                "tool_call",
                serde_json::json!({
                    "callId": tool_call.llm_call_id.clone(),
                    "callDbId": tool_call.call_db_id,
                    "serverName": tool_call.server_name.clone(),
                    "toolName": tool_call.tool_name.clone(),
                    "parameters": tool_call.params_json.clone(),
                })
                .to_string(),
            );

            let (success, result_text) = match tool_call.call_db_id {
                Some(call_db_id) => {
                    execute_single_tool(
                        ctx.app_handle,
                        call_db_id,
                        Duration::from_secs(AGENTIC_LOOP_PER_TOOL_TIMEOUT_SECS),
                        Some(ctx.cancel_token.clone()),
                    )
                    .await
                }
                None => (
                    false,
                    tool_call.call_record_error.clone().unwrap_or_else(|| {
                        "Error: failed to create tool call record before execution".to_string()
                    }),
                ),
            };

            if success {
                tool_calls_success += 1;
            } else {
                tool_calls_failed += 1;
            }

            let tool_result_call_id = if tool_call.llm_call_id.trim().is_empty() {
                tool_call
                    .call_db_id
                    .map(|id| format!("mcp_tool_call_{}", id))
                    .unwrap_or_else(|| Uuid::new_v4().to_string())
            } else {
                tool_call.llm_call_id.clone()
            };
            // 构建 tool_result 消息内容（与 trigger_conversation_continuation_batch 格式一致）
            let tool_result_content = format!(
                "Tool execution completed:\n\nTool Call ID: {}\nTool: {}\nServer: {}\nParameters: {}\nResult:\n{}",
                tool_result_call_id.as_str(),
                tool_call.tool_name.as_str(),
                tool_call.server_name.as_str(),
                tool_call.params_str.as_str(),
                result_text
            );

            // 保存 tool_result 到 DB
            let _ = crate::api::ai_api::add_message(
                ctx.app_handle,
                None,
                ctx.conversation_id,
                "tool_result".to_string(),
                tool_result_content.clone(),
                Some(ctx.llm_model_id),
                Some(ctx.llm_model_code.clone()),
                Some(Utc::now()),
                Some(Utc::now()),
                0,
                Some(generation_group_id.clone()),
                None,
            );

            // 追加到内存消息
            messages.push(("tool_result".to_string(), tool_result_content, Vec::new()));
            log_task_message(
                ctx.app_handle,
                ctx.task_id,
                ctx.run_id,
                "tool_result",
                serde_json::json!({
                    "callId": tool_result_call_id,
                    "callDbId": tool_call.call_db_id,
                    "serverName": tool_call.server_name.clone(),
                    "toolName": tool_call.tool_name.clone(),
                    "success": success,
                    "parameters": tool_call.params_json.clone(),
                    "result": result_text,
                })
                .to_string(),
            );
        }

        // 更新最后的 response message finish_time
        if response_message_id > 0 {
            if let Ok(repo) = ctx.conversation_db.message_repo() {
                if let Ok(Some(mut msg)) = repo.read(response_message_id) {
                    msg.finish_time = Some(Utc::now());
                    let _ = repo.update(&msg);
                }
            }
        }

        // 继续下一轮
    }
}

pub(crate) fn parse_notify_bool_value(value: &serde_json::Value) -> Option<bool> {
    if let Some(v) = value.as_bool() {
        return Some(v);
    }
    if let Some(v) = value.as_i64() {
        return Some(v != 0);
    }
    let s = value.as_str()?.trim().to_lowercase();
    match s.as_str() {
        "true" | "1" | "yes" | "notify" | "需要通知" | "是" => Some(true),
        "false" | "0" | "no" | "skip" | "不通知" | "否" => Some(false),
        _ => None,
    }
}

pub(crate) fn normalize_task_state_value(value: &str) -> Option<String> {
    let normalized = value.trim().to_lowercase();
    let mapped = match normalized.as_str() {
        "completed" | "complete" | "done" | "finished" | "success" | "succeeded" | "结束"
        | "已结束" | "已完成" => "completed",
        "running" | "in_progress" | "pending" | "processing" | "进行中" | "未结束" => {
            "running"
        }
        "failed" | "error" | "失败" | "异常" => "failed",
        _ => return None,
    };
    Some(mapped.to_string())
}

pub(crate) fn parse_notify_decision(raw: &str) -> Result<NotifyDecision, String> {
    let value = extract_json_from_response(raw)
        .ok_or_else(|| "通知判定返回格式不正确，无法提取 JSON 对象".to_string())?;
    let object = value.as_object().ok_or_else(|| "通知判定返回必须是 JSON 对象".to_string())?;

    let task_state = object
        .get("task_state")
        .or_else(|| object.get("status"))
        .or_else(|| object.get("state"))
        .and_then(|v| v.as_str())
        .and_then(normalize_task_state_value);

    if matches!(task_state.as_deref(), Some("running")) {
        return Err("通知判定返回 task_state=running，表示任务尚未结束".to_string());
    }

    let notify = object
        .get("notify")
        .or_else(|| object.get("should_notify"))
        .and_then(parse_notify_bool_value)
        .unwrap_or(false);

    let summary = object
        .get("summary")
        .or_else(|| object.get("message"))
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let reason = object
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    if notify && summary.is_none() {
        return Err("通知判定返回 notify=true 时必须提供 summary".to_string());
    }

    Ok(NotifyDecision { task_state, notify, summary, reason })
}

fn validate_assistant_type(app_handle: &tauri::AppHandle, assistant_id: i64) -> Result<(), String> {
    let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let assistant = db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    if assistant.assistant_type.unwrap_or(0) != 0 {
        return Err("只能选择普通对话助手".to_string());
    }
    Ok(())
}

fn build_tool_config_for_mcp(
    mcp_info: &MCPInfoForAssistant,
    enable_tools: bool,
) -> Option<ToolConfig> {
    if !enable_tools {
        return None;
    }
    let (tools, tool_name_mapping) = build_tools_with_mapping(&mcp_info.enabled_servers);
    Some(ToolConfig { tools, tool_name_mapping })
}

fn suppress_completion_notification(
    config_feature_map: &mut HashMap<String, HashMap<String, FeatureConfig>>,
) {
    let display_config =
        config_feature_map.entry("display".to_string()).or_insert_with(HashMap::new);
    display_config.insert(
        "notification_on_completion".to_string(),
        FeatureConfig {
            id: None,
            feature_code: "display".to_string(),
            key: "notification_on_completion".to_string(),
            value: "false".to_string(),
            data_type: "string".to_string(),
            description: Some("scheduled task override".to_string()),
        },
    );
}

fn write_task_log(
    app_handle: &tauri::AppHandle,
    task_id: i64,
    run_id: &str,
    message_type: &str,
    content: impl Into<String>,
) -> Result<(), String> {
    let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let log = ScheduledTaskLog {
        id: 0,
        task_id,
        run_id: run_id.to_string(),
        message_type: message_type.to_string(),
        content: content.into(),
        created_time: Utc::now(),
    };
    let created = db.add_log(&log).map_err(|e| e.to_string())?;
    let _ = app_handle.emit(SCHEDULED_TASK_LOG_ADDED_EVENT, log_to_dto(created));
    Ok(())
}

fn log_task_message(
    app_handle: &tauri::AppHandle,
    task_id: i64,
    run_id: &str,
    message_type: &str,
    content: impl Into<String>,
) {
    let content = content.into();
    let _ = write_task_log(app_handle, task_id, run_id, message_type, content);
}

fn update_task_run(
    app_handle: &tauri::AppHandle,
    run_id: &str,
    status: &str,
    notify: bool,
    summary: Option<&str>,
    error_message: Option<&str>,
    finished_time: Option<DateTime<Utc>>,
) {
    let finished_time_for_event = finished_time.clone();
    let result = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string()).and_then(|db| {
        db.update_run_result(run_id, status, notify, summary, error_message, finished_time)
            .map_err(|e| e.to_string())
    });
    if result.is_ok() {
        let event_payload = ScheduledTaskRunUpdateEvent {
            run_id: run_id.to_string(),
            status: status.to_string(),
            notify,
            summary: summary.map(|v| v.to_string()),
            error_message: error_message.map(|v| v.to_string()),
            finished_time: finished_time_for_event.map(|v| v.to_rfc3339()),
        };
        let _ = app_handle.emit(SCHEDULED_TASK_RUN_UPDATED_EVENT, event_payload);
    }
}

fn cleanup_conversation(app_handle: &tauri::AppHandle, conversation_id: i64) -> Result<(), String> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = conversation_db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM message_attachment WHERE message_id IN (SELECT id FROM message WHERE conversation_id = ?)",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM message WHERE conversation_id = ?", params![conversation_id])
        .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM conversation_summary WHERE conversation_id = ?",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM conversation WHERE id = ?", params![conversation_id])
        .map_err(|e| e.to_string())?;

    if let Ok(mcp_db) = MCPDatabase::new(app_handle) {
        let _ = mcp_db.conn.execute(
            "DELETE FROM mcp_tool_call WHERE conversation_id = ?",
            params![conversation_id],
        );
    }

    Ok(())
}

/// Configuration for schedule calculation
pub struct ScheduleConfig<'a> {
    pub schedule_type: &'a str,
    pub interval_value: Option<i64>,
    pub interval_unit: Option<&'a str>,
    pub start_time: Option<&'a str>,  // HH:mm
    pub week_days: Option<Vec<i32>>,  // 0=Sun, 1=Mon, ..., 6=Sat
    pub month_days: Option<Vec<i32>>, // 1-31
    pub run_at: Option<DateTime<Utc>>,
}

fn parse_start_time(start_time: Option<&str>) -> Option<(u32, u32)> {
    start_time.and_then(|s| {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            let hour = parts[0].parse::<u32>().ok()?;
            let minute = parts[1].parse::<u32>().ok()?;
            Some((hour, minute))
        } else {
            None
        }
    })
}

pub fn compute_next_run_at(
    schedule_type: &str,
    interval_value: Option<i64>,
    interval_unit: Option<&str>,
    run_at: Option<DateTime<Utc>>,
    base_time: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, String> {
    compute_next_run_at_with_config(
        ScheduleConfig {
            schedule_type,
            interval_value,
            interval_unit,
            start_time: None,
            week_days: None,
            month_days: None,
            run_at,
        },
        base_time,
    )
}

pub fn compute_next_run_at_with_config(
    config: ScheduleConfig,
    base_time: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, String> {
    use chrono::{Datelike, Timelike, Weekday};

    if config.schedule_type == "once" {
        return Ok(config.run_at);
    }
    if config.schedule_type != "interval" {
        return Err("不支持的 schedule_type".to_string());
    }

    let value = config.interval_value.ok_or_else(|| "缺少 interval_value".to_string())?;
    let unit = config.interval_unit.ok_or_else(|| "缺少 interval_unit".to_string())?;
    if value <= 0 {
        return Err("interval_value 需要大于 0".to_string());
    }

    let local_base = base_time.with_timezone(&Local);
    let (target_hour, target_minute) =
        parse_start_time(config.start_time).unwrap_or((local_base.hour(), local_base.minute()));

    match unit {
        "minute" => {
            let next = base_time + chrono::Duration::minutes(value);
            Ok(Some(next))
        }
        "hour" => {
            let next = base_time + chrono::Duration::hours(value);
            Ok(Some(next))
        }
        "day" => {
            // Every N days at start_time
            let mut candidate = Local
                .with_ymd_and_hms(
                    local_base.year(),
                    local_base.month(),
                    local_base.day(),
                    target_hour,
                    target_minute,
                    0,
                )
                .single()
                .ok_or_else(|| "无法构造日期".to_string())?;

            if candidate <= local_base {
                candidate = candidate + chrono::Duration::days(value);
            }
            Ok(Some(candidate.with_timezone(&Utc)))
        }
        "week" => {
            // Every N weeks on specified week_days at start_time
            let week_days = config
                .week_days
                .clone()
                .unwrap_or_else(|| vec![local_base.weekday().num_days_from_sunday() as i32]);
            if week_days.is_empty() {
                return Err("请至少选择一个星期几".to_string());
            }

            let mut week_days_sorted: Vec<u32> = week_days
                .iter()
                .filter_map(|&d| if d >= 0 && d <= 6 { Some(d as u32) } else { None })
                .collect();
            week_days_sorted.sort();
            week_days_sorted.dedup();

            if week_days_sorted.is_empty() {
                return Err("无效的星期几配置".to_string());
            }

            let current_weekday = local_base.weekday().num_days_from_sunday();

            // Find next valid day in this week or next weeks
            let mut candidate: Option<DateTime<Local>> = None;
            for week_offset in 0..=(value as i64 * 2) {
                let week_start = local_base + chrono::Duration::weeks(week_offset);
                for &wd in &week_days_sorted {
                    let days_from_week_start =
                        (wd as i64 + 7 - week_start.weekday().num_days_from_sunday() as i64) % 7;
                    let target_date =
                        week_start.date_naive() + chrono::Duration::days(days_from_week_start);
                    let target_dt = Local
                        .with_ymd_and_hms(
                            target_date.year(),
                            target_date.month(),
                            target_date.day(),
                            target_hour,
                            target_minute,
                            0,
                        )
                        .single();

                    if let Some(dt) = target_dt {
                        if dt > local_base {
                            if candidate.is_none() || dt < candidate.unwrap() {
                                candidate = Some(dt);
                            }
                        }
                    }
                }
                if candidate.is_some() {
                    break;
                }
            }

            match candidate {
                Some(dt) => Ok(Some(dt.with_timezone(&Utc))),
                None => Err("无法计算下次执行时间".to_string()),
            }
        }
        "month" => {
            // Every N months on specified month_days at start_time
            let month_days =
                config.month_days.clone().unwrap_or_else(|| vec![local_base.day() as i32]);
            if month_days.is_empty() {
                return Err("请至少选择一天".to_string());
            }

            let mut month_days_sorted: Vec<u32> = month_days
                .iter()
                .filter_map(|&d| if d >= 1 && d <= 31 { Some(d as u32) } else { None })
                .collect();
            month_days_sorted.sort();
            month_days_sorted.dedup();

            if month_days_sorted.is_empty() {
                return Err("无效的日期配置".to_string());
            }

            // Find next valid day in current month or future months
            let mut candidate: Option<DateTime<Local>> = None;
            let mut check_year = local_base.year();
            let mut check_month = local_base.month();

            for _ in 0..24 {
                for &day in &month_days_sorted {
                    let target_dt = Local
                        .with_ymd_and_hms(
                            check_year,
                            check_month,
                            day,
                            target_hour,
                            target_minute,
                            0,
                        )
                        .single();

                    if let Some(dt) = target_dt {
                        if dt > local_base {
                            if candidate.is_none() || dt < candidate.unwrap() {
                                candidate = Some(dt);
                            }
                        }
                    }
                }
                if candidate.is_some() {
                    break;
                }
                // Move to next month
                check_month += 1;
                if check_month > 12 {
                    check_month = 1;
                    check_year += 1;
                }
            }

            match candidate {
                Some(dt) => Ok(Some(dt.with_timezone(&Utc))),
                None => Err("无法计算下次执行时间".to_string()),
            }
        }
        _ => Err("不支持的 interval_unit".to_string()),
    }
}

fn parse_json_array(s: &Option<String>) -> Option<Vec<i32>> {
    s.as_ref().and_then(|v| serde_json::from_str(v).ok())
}

fn to_dto(task: ScheduledTask) -> ScheduledTaskDTO {
    ScheduledTaskDTO {
        id: task.id,
        name: task.name,
        is_enabled: task.is_enabled,
        schedule_type: task.schedule_type,
        interval_value: task.interval_value,
        interval_unit: task.interval_unit,
        start_time: task.start_time,
        week_days: parse_json_array(&task.week_days),
        month_days: parse_json_array(&task.month_days),
        run_at: format_dt(task.run_at),
        next_run_at: format_dt(task.next_run_at),
        last_run_at: format_dt(task.last_run_at),
        assistant_id: task.assistant_id,
        task_prompt: task.task_prompt,
        notify_prompt: task.notify_prompt,
        created_time: task.created_time.to_rfc3339(),
        updated_time: task.updated_time.to_rfc3339(),
    }
}

fn log_to_dto(log: ScheduledTaskLog) -> ScheduledTaskLogDTO {
    ScheduledTaskLogDTO {
        id: log.id,
        task_id: log.task_id,
        run_id: log.run_id,
        message_type: log.message_type,
        content: log.content,
        created_time: log.created_time.to_rfc3339(),
    }
}

fn run_to_dto(run: ScheduledTaskRun) -> ScheduledTaskRunDTO {
    ScheduledTaskRunDTO {
        id: run.id,
        task_id: run.task_id,
        run_id: run.run_id,
        status: run.status,
        notify: run.notify,
        summary: run.summary,
        error_message: run.error_message,
        started_time: run.started_time.to_rfc3339(),
        finished_time: run.finished_time.map(|v| v.to_rfc3339()),
    }
}

#[tauri::command]
pub async fn list_scheduled_tasks(
    app_handle: tauri::AppHandle,
) -> Result<Vec<ScheduledTaskDTO>, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let tasks = db.list_tasks().map_err(|e| e.to_string())?;
    Ok(tasks.into_iter().map(to_dto).collect())
}

#[tauri::command]
pub async fn list_scheduled_task_logs(
    app_handle: tauri::AppHandle,
    task_id: i64,
    run_id: Option<String>,
    limit: Option<u32>,
) -> Result<ListScheduledTaskLogsResponse, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let limit_value = limit.unwrap_or(200).min(1000);
    let logs = match run_id {
        Some(run_id) => {
            db.list_logs_by_run(task_id, &run_id, limit_value).map_err(|e| e.to_string())?
        }
        None => db.list_logs_by_task(task_id, limit_value).map_err(|e| e.to_string())?,
    };
    Ok(ListScheduledTaskLogsResponse { logs: logs.into_iter().map(log_to_dto).collect() })
}

#[tauri::command]
pub async fn list_scheduled_task_runs(
    app_handle: tauri::AppHandle,
    task_id: i64,
    limit: Option<u32>,
) -> Result<ListScheduledTaskRunsResponse, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let limit_value = limit.unwrap_or(50).min(200);
    let runs = db.list_runs_by_task(task_id, limit_value).map_err(|e| e.to_string())?;
    Ok(ListScheduledTaskRunsResponse { runs: runs.into_iter().map(run_to_dto).collect() })
}

fn serialize_json_array(arr: &Option<Vec<i32>>) -> Option<String> {
    arr.as_ref().map(|v| serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string()))
}

#[tauri::command]
pub async fn create_scheduled_task(
    app_handle: tauri::AppHandle,
    request: CreateScheduledTaskRequest,
) -> Result<ScheduledTaskDTO, String> {
    validate_assistant_type(&app_handle, request.assistant_id)?;
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let now = Utc::now();
    let run_at = match &request.run_at {
        Some(value) => Some(parse_local_datetime(value)?),
        None => None,
    };
    if request.schedule_type == "once" && run_at.is_none() {
        return Err("一次性任务需要设置执行时间".to_string());
    }
    let next_run_at = compute_next_run_at_with_config(
        ScheduleConfig {
            schedule_type: &request.schedule_type,
            interval_value: request.interval_value,
            interval_unit: request.interval_unit.as_deref(),
            start_time: request.start_time.as_deref(),
            week_days: request.week_days.clone(),
            month_days: request.month_days.clone(),
            run_at,
        },
        now,
    )?;

    let task = ScheduledTask {
        id: 0,
        name: request.name,
        is_enabled: request.is_enabled,
        schedule_type: request.schedule_type,
        interval_value: request.interval_value,
        interval_unit: request.interval_unit,
        start_time: request.start_time,
        week_days: serialize_json_array(&request.week_days),
        month_days: serialize_json_array(&request.month_days),
        run_at,
        next_run_at,
        last_run_at: None,
        assistant_id: request.assistant_id,
        task_prompt: request.task_prompt,
        notify_prompt: request.notify_prompt,
        created_time: now,
        updated_time: now,
    };
    let created = db.create_task(&task).map_err(|e| e.to_string())?;
    Ok(to_dto(created))
}

#[tauri::command]
pub async fn update_scheduled_task(
    app_handle: tauri::AppHandle,
    request: UpdateScheduledTaskRequest,
) -> Result<ScheduledTaskDTO, String> {
    validate_assistant_type(&app_handle, request.assistant_id)?;
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let existing = db
        .read_task(request.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在".to_string())?;
    let now = Utc::now();
    let run_at = match &request.run_at {
        Some(value) => Some(parse_local_datetime(value)?),
        None => None,
    };
    if request.schedule_type == "once" && run_at.is_none() {
        return Err("一次性任务需要设置执行时间".to_string());
    }
    let next_run_at = compute_next_run_at_with_config(
        ScheduleConfig {
            schedule_type: &request.schedule_type,
            interval_value: request.interval_value,
            interval_unit: request.interval_unit.as_deref(),
            start_time: request.start_time.as_deref(),
            week_days: request.week_days.clone(),
            month_days: request.month_days.clone(),
            run_at,
        },
        now,
    )?;
    let updated = ScheduledTask {
        id: existing.id,
        name: request.name,
        is_enabled: request.is_enabled,
        schedule_type: request.schedule_type,
        interval_value: request.interval_value,
        interval_unit: request.interval_unit,
        start_time: request.start_time,
        week_days: serialize_json_array(&request.week_days),
        month_days: serialize_json_array(&request.month_days),
        run_at,
        next_run_at,
        last_run_at: existing.last_run_at,
        assistant_id: request.assistant_id,
        task_prompt: request.task_prompt,
        notify_prompt: request.notify_prompt,
        created_time: existing.created_time,
        updated_time: now,
    };
    db.update_task(&updated).map_err(|e| e.to_string())?;
    Ok(to_dto(updated))
}

#[tauri::command]
pub async fn delete_scheduled_task(
    app_handle: tauri::AppHandle,
    task_id: i64,
) -> Result<(), String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.delete_task(task_id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn run_scheduled_task_now(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    task_id: i64,
) -> Result<RunScheduledTaskResult, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let task = db
        .read_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在".to_string())?;
    let now = Utc::now();
    let next_run_at = if task.schedule_type == "once" {
        None
    } else {
        compute_next_run_at(
            &task.schedule_type,
            task.interval_value,
            task.interval_unit.as_deref(),
            task.run_at,
            now,
        )?
    };
    let updated = ScheduledTask {
        is_enabled: if task.schedule_type == "once" { false } else { task.is_enabled },
        last_run_at: Some(now),
        next_run_at,
        updated_time: now,
        ..task.clone()
    };
    db.update_task(&updated).map_err(|e| e.to_string())?;

    match execute_scheduled_task(&app_handle, &feature_config_state, &updated).await {
        Ok(result) => Ok(result),
        Err(e) => Ok(RunScheduledTaskResult {
            task_id,
            success: false,
            notify: false,
            summary: None,
            error: Some(e),
        }),
    }
}

#[tauri::command]
pub async fn stop_scheduled_task_run(
    app_handle: tauri::AppHandle,
    task_id: i64,
    run_id: Option<String>,
) -> Result<bool, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let runs = db.list_runs_by_task(task_id, 50).map_err(|e| e.to_string())?;

    let target_run = match run_id {
        Some(target_run_id) => {
            runs.into_iter().find(|run| run.run_id == target_run_id && run.status == "running")
        }
        None => runs.into_iter().find(|run| run.status == "running"),
    }
    .ok_or_else(|| "当前任务没有正在运行的执行记录".to_string())?;

    let cancelled = cancel_scheduled_run(&target_run.run_id).await;
    if !cancelled {
        return Err("当前运行任务不可停止或已结束".to_string());
    }

    log_task_message(
        &app_handle,
        task_id,
        &target_run.run_id,
        "cancel_request",
        "收到停止请求，正在尝试终止执行",
    );

    Ok(true)
}

pub async fn execute_scheduled_task(
    app_handle: &tauri::AppHandle,
    feature_config_state: &FeatureConfigState,
    task: &ScheduledTask,
) -> Result<RunScheduledTaskResult, String> {
    let run_id = Uuid::new_v4().to_string();
    let run_cancel_token = register_scheduled_run_cancel_token(&run_id).await;
    let run_started_at = Utc::now();
    {
        let log_db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let running = ScheduledTaskRun {
            id: 0,
            task_id: task.id,
            run_id: run_id.clone(),
            status: "running".to_string(),
            notify: false,
            summary: None,
            error_message: None,
            started_time: run_started_at,
            finished_time: None,
        };
        if let Ok(created_run) = log_db.create_run(&running) {
            let _ = app_handle.emit(SCHEDULED_TASK_RUN_CREATED_EVENT, run_to_dto(created_run));
        }
    }
    log_task_message(app_handle, task.id, &run_id, "start", "开始执行定时任务");
    let run_result = execute_scheduled_task_inner(
        app_handle,
        feature_config_state,
        task,
        &run_id,
        run_cancel_token,
    )
    .await;

    if let Err(err) = &run_result {
        update_task_run(app_handle, &run_id, "failed", false, None, Some(err), Some(Utc::now()));
    }
    unregister_scheduled_run_cancel_token(&run_id).await;
    run_result
}

async fn execute_scheduled_task_inner(
    app_handle: &tauri::AppHandle,
    feature_config_state: &FeatureConfigState,
    task: &ScheduledTask,
    run_id: &str,
    run_cancel_token: CancellationToken,
) -> Result<RunScheduledTaskResult, String> {
    // ── 1. 验证助手配置 ──────────────────────────────────────────────
    let assistant_detail = get_assistant(app_handle.clone(), task.assistant_id)
        .map_err(|e| format!("Failed to get assistant: {}", e))?;
    if assistant_detail.assistant.assistant_type.unwrap_or(0) != 0 {
        log_task_message(app_handle, task.id, run_id, "error", "只能选择普通对话助手");
        update_task_run(
            app_handle,
            run_id,
            "failed",
            false,
            None,
            Some("只能选择普通对话助手"),
            Some(Utc::now()),
        );
        return Err("只能选择普通对话助手".to_string());
    }
    if assistant_detail.model.is_empty() {
        log_task_message(app_handle, task.id, run_id, "error", "助手未配置模型");
        update_task_run(
            app_handle,
            run_id,
            "failed",
            false,
            None,
            Some("助手未配置模型"),
            Some(Utc::now()),
        );
        return Err("助手未配置模型".to_string());
    }
    let system_prompt = assistant_detail
        .prompts
        .first()
        .map(|p| p.prompt.clone())
        .ok_or_else(|| "助手未配置系统提示词".to_string())?;

    // ── 2. 渲染提示词 ──────────────────────────────────────────────
    let template_engine = build_template_engine(app_handle).map_err(|e| e.to_string())?;
    let mut template_context = HashMap::new();
    if let Some(state) = app_handle.try_state::<AppState>() {
        let selected_text = state.selected_text.lock().await.clone();
        template_context.insert("selected_text".to_string(), selected_text);
    }
    let system_prompt_rendered = template_engine.parse(&system_prompt, &template_context).await;
    let task_prompt_rendered = template_engine.parse(&task.task_prompt, &template_context).await;
    log_task_message(app_handle, task.id, run_id, "task_prompt", task_prompt_rendered.clone());

    // ── 3. 收集 MCP 工具信息 & skills ────────────────────────────────
    let mcp_info = collect_mcp_info_for_assistant(app_handle, task.assistant_id, None, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut assistant_prompt_result =
        if !mcp_info.enabled_servers.is_empty() && !mcp_info.use_native_toolcall {
            format_mcp_prompt(system_prompt_rendered.clone(), &mcp_info).await
        } else {
            system_prompt_rendered.clone()
        };
    let skills_info = collect_skills_info_for_assistant(app_handle, task.assistant_id)
        .await
        .map_err(|e| e.to_string())?;
    if !skills_info.enabled_skills.is_empty() {
        assistant_prompt_result =
            format_skills_prompt(app_handle, assistant_prompt_result, &skills_info).await;
    }

    // ── 4. 构建 LLM 客户端 & 选项 ─────────────────────────────────
    let llm_db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let model_info = &assistant_detail.model[0];
    let model_detail = llm_db
        .get_llm_model_detail(&model_info.provider_id, &model_info.model_code)
        .map_err(|e| format!("Failed to get LLM model: {}", e))?;

    let mut config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    suppress_completion_notification(&mut config_feature_map);
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let request_timeout = get_request_timeout_from_config(&config_feature_map);
    let proxy_enabled = model_detail
        .configs
        .iter()
        .find(|config| config.name == "proxy_enabled")
        .and_then(|config| config.value.parse::<bool>().ok())
        .unwrap_or(false);

    let client = create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
        false,
        &config_feature_map,
    )
    .map_err(|e| format!("Failed to create AI client: {}", e))?;
    log_task_message(
        app_handle,
        task.id,
        run_id,
        "assistant",
        format!("助手: {} / 模型: {}", assistant_detail.assistant.name, model_detail.model.code),
    );

    let model_config_clone = ConfigBuilder::merge_model_configs(
        assistant_detail.model_configs.clone(),
        &model_detail,
        None,
    );
    let config_map: HashMap<String, String> = model_config_clone
        .iter()
        .filter_map(|config| {
            config.value.as_ref().map(|value| (config.name.clone(), value.clone()))
        })
        .collect();
    let model_name =
        config_map.get("model").cloned().unwrap_or_else(|| model_detail.model.code.clone());
    let provider_api_type_lc = model_detail.provider.api_type.to_lowercase();
    let model_code_lc = model_detail.model.code.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let is_gemini = model_code_lc.contains("gemini");
    let capture_usage = !(is_openai_like && is_gemini);
    let has_available_tools = mcp_info.use_native_toolcall && !mcp_info.enabled_servers.is_empty();
    let chat_options = ConfigBuilder::build_chat_options(&config_map)
        .with_normalize_reasoning_content(true)
        .with_capture_usage(capture_usage)
        .with_capture_tool_calls(has_available_tools);

    let tool_call_strategy =
        if has_available_tools { ToolCallStrategy::Native } else { ToolCallStrategy::NonNative };
    let tool_config = build_tool_config_for_mcp(&mcp_info, has_available_tools);
    let tool_name_mapping =
        tool_config.as_ref().map(|c| c.tool_name_mapping.clone()).unwrap_or_default();

    // ── 5. 创建对话并初始化消息 ─────────────────────────────────────
    let init_messages = vec![
        ("system".to_string(), assistant_prompt_result.clone(), Vec::new()),
        ("user".to_string(), task_prompt_rendered.clone(), Vec::new()),
    ];
    let (mut conversation, _) = crate::api::ai::conversation::init_conversation(
        app_handle,
        task.assistant_id,
        model_detail.model.id,
        model_detail.model.code.clone(),
        &init_messages,
    )
    .map_err(|e| e.to_string())?;
    let conversation_id = conversation.id;
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;

    // ── 6. 执行 Agentic Loop ────────────────────────────────────────
    let loop_ctx = AgenticLoopContext {
        app_handle,
        client: &client,
        model_name: &model_name,
        chat_options: &chat_options,
        conversation_id,
        conversation_db: &conversation_db,
        tool_name_mapping: &tool_name_mapping,
        tool_config,
        tool_call_strategy,
        llm_model_id: model_detail.model.id,
        llm_model_code: model_detail.model.code.clone(),
        task_id: task.id,
        run_id,
        cancel_token: run_cancel_token.clone(),
    };
    let loop_result = run_task_agentic_loop(&loop_ctx).await;

    let task_result = &loop_result.final_text;
    let status_str = loop_result.status.to_string();
    log_task_message(
        app_handle,
        task.id,
        run_id,
        "loop_done",
        format!(
            "status={}, rounds={}, tools={}/{}/{}",
            status_str,
            loop_result.rounds,
            loop_result.tool_calls_success,
            loop_result.tool_calls_failed,
            loop_result.tool_calls_total
        ),
    );

    if task_result.is_empty() {
        log_task_message(app_handle, task.id, run_id, "response", "任务执行未返回内容");
    } else {
        log_task_message(app_handle, task.id, run_id, "response", task_result.clone());
    }

    // 非正常结束直接标记失败
    match &loop_result.status {
        AgenticLoopStatus::Error(e) => {
            update_task_run(app_handle, run_id, "failed", false, None, Some(e), Some(Utc::now()));
            let _ = cleanup_conversation(app_handle, conversation_id);
            return Err(e.clone());
        }
        AgenticLoopStatus::Cancelled => {
            let cancelled_error = "任务已手动停止".to_string();
            log_task_message(app_handle, task.id, run_id, "cancel", &cancelled_error);
            update_task_run(
                app_handle,
                run_id,
                "failed",
                false,
                None,
                Some(&cancelled_error),
                Some(Utc::now()),
            );
            let _ = cleanup_conversation(app_handle, conversation_id);
            return Err(cancelled_error);
        }
        _ => {}
    }

    // ── 7. 通知判定 ──────────────────────────────────────────────────
    let execution_report = format!(
        "执行状态: {}\n执行轮次: {}\n工具调用: 总计 {}, 成功 {}, 失败 {}\n\n最终输出:\n{}",
        status_str,
        loop_result.rounds,
        loop_result.tool_calls_total,
        loop_result.tool_calls_success,
        loop_result.tool_calls_failed,
        task_result
    );

    let notify_prompt_full = format!(
        "以下是定时任务的执行报告，请判断是否需要通知用户。判定规则如下：\n{}\n\n请严格返回 JSON 对象（可放在```json代码块中，不要返回其他解释文本）：\n{{\"task_state\":\"completed|running|failed\",\"notify\":true|false,\"summary\":\"notify=true时必填\",\"reason\":\"判定依据\"}}\n约束：\n1) task_state=running 时 notify 必须为 false 且 summary 置空；\n2) task_state=completed 或 failed 时，再决定 notify；\n3) notify=true 时 summary 必须是简洁结论。",
        if task.notify_prompt.trim().is_empty() {
            "如果任务结果包含重要信息或需要用户关注的内容则通知".to_string()
        } else {
            task.notify_prompt.clone()
        }
    );

    let notify_request = build_chat_request_from_messages(
        &[
            ("system".to_string(), system_prompt_rendered.clone(), Vec::new()),
            ("user".to_string(), notify_prompt_full, Vec::new()),
            ("assistant".to_string(), execution_report, Vec::new()),
        ],
        ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;

    let notify_options = chat_options.clone().with_capture_tool_calls(false);
    let notify_response = client
        .exec_chat(&model_name, notify_request, Some(&notify_options))
        .await
        .map_err(|e| {
            log_task_message(
                app_handle,
                task.id,
                run_id,
                "error",
                format!("通知判定执行失败: {}", e),
            );
            update_task_run(
                app_handle,
                run_id,
                "failed",
                false,
                None,
                Some(&format!("通知判定执行失败: {}", e)),
                Some(Utc::now()),
            );
            format!("通知判定执行失败: {}", e)
        })?;
    let notify_text = notify_response.content.into_joined_texts().unwrap_or_default();
    log_task_message(app_handle, task.id, run_id, "notify_raw", notify_text.clone());

    let notify_decision = parse_notify_decision(&notify_text).map_err(|e| {
        error!(error = %e, raw = %notify_text, "invalid notify decision payload");
        log_task_message(
            app_handle,
            task.id,
            run_id,
            "error",
            format!("通知判定返回不合法: {}", e),
        );
        update_task_run(
            app_handle,
            run_id,
            "failed",
            false,
            None,
            Some(&format!("通知判定返回不合法: {}", e)),
            Some(Utc::now()),
        );
        format!("通知判定返回不合法: {}", e)
    })?;
    let notify = notify_decision.notify;
    let summary = notify_decision.summary.clone();
    log_task_message(
        app_handle,
        task.id,
        run_id,
        "notify_result",
        format!(
            "task_state={}, notify={}, summary={}, reason={}",
            notify_decision.task_state.unwrap_or_else(|| "unknown".to_string()),
            notify,
            summary.clone().unwrap_or_default(),
            notify_decision.reason.unwrap_or_default()
        ),
    );

    // ── 8. 通知或清理 ─────────────────────────────────────────────
    if notify {
        conversation.name = format!("计划任务: {}", task.name);
        let _ =
            conversation_db.conversation_repo().map_err(|e| e.to_string())?.update(&conversation);
        if let Some(cache_state) = app_handle.try_state::<NameCacheState>() {
            let mut assistant_name_cache = cache_state.assistant_names.lock().await;
            assistant_name_cache.insert(task.assistant_id, assistant_detail.assistant.name.clone());
        }
        let _ = app_handle.emit("conversation_created", conversation.id);

        let _ = app_handle
            .notification()
            .builder()
            .title("定时任务完成")
            .body("定时任务完成，请查看对话")
            .show();
        log_task_message(app_handle, task.id, run_id, "notify", "已发送系统通知");
        update_task_run(
            app_handle,
            run_id,
            "success",
            notify,
            summary.as_deref(),
            None,
            Some(Utc::now()),
        );
    } else {
        cleanup_conversation(app_handle, conversation_id)?;
        log_task_message(app_handle, task.id, run_id, "cleanup", "未通知，已清理对话记录");
        update_task_run(
            app_handle,
            run_id,
            "success",
            notify,
            summary.as_deref(),
            None,
            Some(Utc::now()),
        );
    }

    Ok(RunScheduledTaskResult {
        task_id: task.id,
        success: true,
        notify,
        summary: if notify { summary } else { None },
        error: None,
    })
}

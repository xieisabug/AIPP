use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, State};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::api::ai::config::{
    get_network_proxy_from_config, get_request_timeout_from_config, ConfigBuilder,
};
use crate::api::ai::conversation::{
    build_chat_request_from_messages, ToolCallStrategy, ToolConfig,
};
use crate::api::ai::summary::extract_json_from_response;
use crate::api::ai::types::AiRequest;
use crate::api::ai_api::{
    add_message, ask_ai, build_tools_with_mapping, resolve_tool_name, ToolNameMapping,
};
use crate::api::assistant_api::get_assistant;
use crate::api::butler_api::{
    active_butler_main_conversation_id, followup_turn_progress, get_butler_main_continuation_lock,
    is_butler_main_busy_error, is_butler_main_running, is_current_butler_main_conversation,
    resolve_or_create_butler_execution_window, wait_for_butler_main_to_be_idle, ButlerMainBusy,
    FollowupTurnProgress, FOLLOWUP_KEYS, FOLLOWUP_MAX_CONTINUATION_ATTEMPTS,
};
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
use crate::skills::{
    build_active_skill_attachments, collect_skills_info_for_assistant,
    compose_user_message_with_active_skills, format_skills_prompt,
};
use crate::slash::parse_slash_prompt;
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
    pub butler_conversation_id: Option<i64>,
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
    pub butler_conversation_id: Option<i64>,
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
    /// True only when this process owns the cancellation token for this run.
    pub local_running: bool,
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
const NOTIFY_DECISION_TIMEOUT_SECS: u64 = 300;
const SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY: Duration = Duration::from_secs(5);
/// 续跑开始后等待主会话空闲的上限；超过后保持 continuing，由下次启动补查。
const SCHEDULED_FOLLOWUP_TURN_WAIT: Duration = Duration::from_secs(3600);
const SCHEDULED_TASK_LOG_ADDED_EVENT: &str = "scheduled_task_log_added";
const SCHEDULED_TASK_RUN_CREATED_EVENT: &str = "scheduled_task_run_created";
const SCHEDULED_TASK_RUN_UPDATED_EVENT: &str = "scheduled_task_run_updated";
type ScheduledRunCancelRegistry = Arc<std::sync::Mutex<HashMap<String, CancellationToken>>>;
static SCHEDULED_RUN_CANCEL_REGISTRY: OnceLock<ScheduledRunCancelRegistry> = OnceLock::new();

fn scheduled_run_cancel_registry() -> &'static ScheduledRunCancelRegistry {
    SCHEDULED_RUN_CANCEL_REGISTRY.get_or_init(|| Arc::new(std::sync::Mutex::new(HashMap::new())))
}

struct ScheduledRunRegistration { run_id: String, token: CancellationToken }

impl Drop for ScheduledRunRegistration {
    fn drop(&mut self) {
        scheduled_run_cancel_registry().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.run_id);
    }
}

fn register_scheduled_run_cancel_token(run_id: &str) -> ScheduledRunRegistration {
    let token = CancellationToken::new();
    let mut registry = scheduled_run_cancel_registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.insert(run_id.to_string(), token.clone());
    ScheduledRunRegistration { run_id: run_id.to_string(), token }
}

async fn cancel_scheduled_run(run_id: &str) -> bool {
    let token_opt = {
        let registry = scheduled_run_cancel_registry().lock().unwrap_or_else(|e| e.into_inner());
        registry.get(run_id).cloned()
    };
    if let Some(token) = token_opt {
        token.cancel();
        true
    } else {
        false
    }
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
pub(crate) enum LlmWait<T> {
    Done(T),
    Cancelled,
    Timeout,
}

/// 等待模型回复时同时监听停止信号与总时限；停止优先，保证停止后不会再按完成处理。
pub(crate) async fn await_llm_or_stop<F: std::future::Future>(
    fut: F,
    cancel_token: &CancellationToken,
    deadline: Instant,
) -> LlmWait<F::Output> {
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => LlmWait::Cancelled,
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => LlmWait::Timeout,
        out = fut => LlmWait::Done(out),
    }
}

/// 一次运行的真实结束结果。执行状态只由执行本身决定，通知判定与管家回报的失败不会改写它。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ScheduledRunOutcome {
    /// 入库状态：success / failed
    pub(crate) status: &'static str,
    /// 具体结束原因：completed / cancelled / timeout / max_rounds_reached / error / interrupted
    pub(crate) end_reason: &'static str,
    pub(crate) notify: bool,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) detail: Option<String>,
    /// 通知判定模型给出的任务状态，仅作参考，不改写执行状态
    pub(crate) judged_task_state: Option<String>,
    /// 通知判定失败原因（不改写执行状态）
    pub(crate) notify_error: Option<String>,
}

impl<'de> Deserialize<'de> for ScheduledRunOutcome {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Persisted JSON owns its text. Decode into owned fields first, then map
        // the validated discriminants to static literals without borrowing the
        // input buffer (or leaking strings to manufacture a static lifetime).
        #[derive(Deserialize)]
        struct StoredOutcome {
            status: String,
            end_reason: String,
            notify: bool,
            summary: Option<String>,
            error: Option<String>,
            detail: Option<String>,
            judged_task_state: Option<String>,
            notify_error: Option<String>,
        }
        let stored = StoredOutcome::deserialize(deserializer)?;
        let status = match stored.status.as_str() {
            "success" => "success",
            "failed" => "failed",
            other => return Err(serde::de::Error::custom(format!("invalid scheduled outcome status: {other}"))),
        };
        let end_reason = match stored.end_reason.as_str() {
            "completed" => "completed",
            "cancelled" => "cancelled",
            "timeout" => "timeout",
            "max_rounds_reached" => "max_rounds_reached",
            "error" => "error",
            "interrupted" => "interrupted",
            other => return Err(serde::de::Error::custom(format!("invalid scheduled outcome end reason: {other}"))),
        };
        Ok(Self {
            status, end_reason, notify: stored.notify, summary: stored.summary,
            error: stored.error, detail: stored.detail,
            judged_task_state: stored.judged_task_state, notify_error: stored.notify_error,
        })
    }
}

// A separate, synced outbox survives a transient SQLite write failure. It is removed
// only after the complete outcome and terminal run state commit together.
#[derive(Serialize, Deserialize)]
struct ScheduledOutcomeJournal {
    task_id: i64,
    run_id: String,
    finished_time: DateTime<Utc>,
    outcome: ScheduledRunOutcome,
}

fn scheduled_outbox_dir(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app_handle.path().app_data_dir()
        .map(|path| path.join("scheduled-outcome-outbox"))
        .map_err(|e| e.to_string())
}

fn scheduled_outbox_path(app_handle: &tauri::AppHandle, run_id: &str) -> Result<std::path::PathBuf, String> {
    let id = Uuid::parse_str(run_id).map_err(|e| format!("invalid run_id: {e}"))?;
    Ok(scheduled_outbox_dir(app_handle)?.join(format!("{id}.json")))
}

fn write_outcome_journal(app_handle: &tauri::AppHandle, entry: &ScheduledOutcomeJournal) -> Result<(), String> {
    use std::io::Write;
    let path = scheduled_outbox_path(app_handle, &entry.run_id)?;
    let dir = path.parent().ok_or("outcome outbox has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Each run has one final outcome. Never replace an already durable journal.
    if path.exists() {
        return Ok(());
    }
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::fs::File::open(dir).and_then(|file| file.sync_all()).map_err(|e| e.to_string())?;
    Ok(())
}

fn remove_outcome_journal(app_handle: &tauri::AppHandle, run_id: &str) {
    if let Ok(path) = scheduled_outbox_path(app_handle, run_id) {
        for path in [path.clone(), path.with_extension("tmp")] {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(run_id, error = %error, "failed to remove committed outcome journal");
                }
            }
        }
    }
}

pub(crate) fn scheduled_followup_needs_retry(status: Option<&str>) -> bool {
    matches!(status, Some("pending" | "continuing" | "failed" | "retry"))
}

fn persist_scheduled_outcome(app_handle: &tauri::AppHandle, entry: &ScheduledOutcomeJournal) -> Result<(), String> {
    let journal_error = write_outcome_journal(app_handle, entry).err();
    let outcome = &entry.outcome;
    let payload = serde_json::to_string(outcome).map_err(|e| e.to_string())?;
    let saved = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string()).and_then(|db| {
        db.save_run_outcome(&entry.run_id, outcome.status, outcome.notify,
            outcome.summary.as_deref(), outcome.error.as_deref(), Some(entry.finished_time), &payload)
            .map_err(|e| e.to_string())
    });
    if let Err(error) = saved {
        return Err(match journal_error {
            Some(journal_error) => format!("SQLite: {error}; durable outbox also failed: {journal_error}; outcome retained only in memory"),
            None => format!("SQLite: {error}; complete outcome retained in durable outbox"),
        });
    }
    remove_outcome_journal(app_handle, &entry.run_id);
    let _ = app_handle.emit(SCHEDULED_TASK_RUN_UPDATED_EVENT, ScheduledTaskRunUpdateEvent {
        run_id: entry.run_id.clone(), status: outcome.status.to_string(), notify: outcome.notify,
        summary: outcome.summary.clone(), error_message: outcome.error.clone(),
        finished_time: Some(entry.finished_time.to_rfc3339()),
    });
    Ok(())
}

impl ScheduledRunOutcome {
    pub(crate) fn failed(end_reason: &'static str, error: String, detail: Option<String>) -> Self {
        Self {
            status: "failed",
            end_reason,
            notify: false,
            summary: None,
            error: Some(error),
            detail,
            judged_task_state: None,
            notify_error: None,
        }
    }
}

/// 将非正常结束的循环状态映射为失败结果；Completed 返回 None。
pub(crate) fn loop_status_failure(
    status: &AgenticLoopStatus,
    rounds: usize,
    tool_calls_success: usize,
    tool_calls_failed: usize,
) -> Option<(&'static str, String)> {
    let executed = if tool_calls_success + tool_calls_failed > 0 {
        format!(
            "；停止前已执行工具 {} 次（成功 {}，失败 {}），已产生的外部操作无法撤回",
            tool_calls_success + tool_calls_failed,
            tool_calls_success,
            tool_calls_failed
        )
    } else {
        String::new()
    };
    match status {
        AgenticLoopStatus::Completed => None,
        AgenticLoopStatus::Cancelled => Some(("cancelled", format!("任务已手动停止（已进行 {rounds} 轮）{executed}"))),
        AgenticLoopStatus::Timeout => Some((
            "timeout",
            format!("执行超时：超过总时限 {AGENTIC_LOOP_TOTAL_TIMEOUT_SECS}s，任务未完成（已进行 {rounds} 轮）{executed}"),
        )),
        AgenticLoopStatus::MaxRoundsReached => Some((
            "max_rounds_reached",
            format!("达到最大执行轮次 {AGENTIC_LOOP_MAX_ROUNDS}，任务未完成{executed}"),
        )),
        AgenticLoopStatus::Error(e) => Some(("error", format!("{e}（已进行 {rounds} 轮）{executed}"))),
    }
}

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

        // 调用 LLM（与停止信号、总时限竞争，等待模型期间也能停止）
        let stop_result = |status: AgenticLoopStatus| AgenticLoopResult {
            final_text: String::new(),
            rounds: round,
            tool_calls_total,
            tool_calls_success,
            tool_calls_failed,
            status,
        };
        let first_attempt = match await_llm_or_stop(
            ctx.client.exec_chat(ctx.model_name, chat_request, Some(ctx.chat_options)),
            &ctx.cancel_token,
            deadline,
        )
        .await
        {
            LlmWait::Done(result) => result,
            LlmWait::Cancelled => {
                log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "cancel", "等待模型回复时任务已停止");
                return stop_result(AgenticLoopStatus::Cancelled);
            }
            LlmWait::Timeout => {
                log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "timeout",
                    format!("等待模型回复时达到总超时 ({}s)，已进入第 {} 轮", AGENTIC_LOOP_TOTAL_TIMEOUT_SECS, round));
                return stop_result(AgenticLoopStatus::Timeout);
            }
        };
        let chat_response = match first_attempt {
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
                let retry_result = match await_llm_or_stop(
                    ctx.client.exec_chat(ctx.model_name, retry_request.chat_request, Some(ctx.chat_options)),
                    &ctx.cancel_token,
                    deadline,
                )
                .await
                {
                    LlmWait::Done(result) => result,
                    LlmWait::Cancelled => {
                        log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "cancel", "重试等待模型回复时任务已停止");
                        return stop_result(AgenticLoopStatus::Cancelled);
                    }
                    LlmWait::Timeout => {
                        log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "timeout",
                            format!("重试等待模型回复时达到总超时 ({}s)", AGENTIC_LOOP_TOTAL_TIMEOUT_SECS));
                        return stop_result(AgenticLoopStatus::Timeout);
                    }
                };
                match retry_result {
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

        // 无工具调用 → 循环结束（停止请求优先于完成，避免停止后仍报成功）
        if captured_tool_calls.is_empty() {
            final_text = response_text;
            if ctx.cancel_token.is_cancelled() {
                log_task_message(ctx.app_handle, ctx.task_id, ctx.run_id, "cancel", "模型已回复但任务已停止，不按完成处理");
                return AgenticLoopResult {
                    final_text,
                    rounds: round,
                    tool_calls_total,
                    tool_calls_success,
                    tool_calls_failed,
                    status: AgenticLoopStatus::Cancelled,
                };
            }
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
    use crate::mcp::builtin_mcp::templates::{
        is_butler_only_builtin_command, is_dynamic_loading_fixed_tool,
    };

    if !enable_tools {
        return None;
    }
    let servers_for_injection = if mcp_info.dynamic_loading_enabled {
        let mut filtered = Vec::new();
        for server in &mcp_info.enabled_servers {
            if server.command.as_deref().map_or(false, is_butler_only_builtin_command) {
                continue;
            }
            let tools: Vec<_> = server
                .tools
                .iter()
                .filter(|tool| {
                    is_dynamic_loading_fixed_tool(server.command.as_deref(), &tool.name, false)
                })
                .cloned()
                .collect();
            if !tools.is_empty() {
                let mut server_cloned = server.clone();
                server_cloned.tools = tools;
                filtered.push(server_cloned);
            }
        }
        filtered
    } else {
        mcp_info.enabled_servers.clone()
    };
    let (tools, tool_name_mapping) = build_tools_with_mapping(&servers_for_injection);
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
) -> Result<(), String> {
    let finished_time_for_event = finished_time.clone();
    let result = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string()).and_then(|db| {
        db.update_run_result(run_id, status, notify, summary, error_message, finished_time)
            .map_err(|e| e.to_string())
    });
    if let Err(error) = &result {
        error!(run_id, status, error = %error, "failed to persist scheduled task run result");
    }
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
    result
}

fn cleanup_conversation(app_handle: &tauri::AppHandle, conversation_id: i64) -> Result<(), String> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    conversation_db
        .with_write_connection(|conn| {
            crate::db::conversation_db::cascade_delete_conversation(conn, conversation_id)
                .map_err(crate::errors::AppError::from)?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;


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

pub fn compute_next_run_at_with_config(
    config: ScheduleConfig,
    base_time: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, String> {
    use chrono::{Datelike, Timelike};

    if config.schedule_type == "once" {
        return Ok(config.run_at);
    }
    if config.schedule_type != "interval" {
        return Err("不支持的 schedule_type".to_string());
    }

    let interval_usage_hint = "schedule_type='interval' 时必须显式提供 interval_value 和 interval_unit。常见写法：每天 22:00 => interval_unit='day', interval_value=1, start_time='22:00'；工作日 09:00 => interval_unit='week', interval_value=1, week_days=[1,2,3,4,5], start_time='09:00'。";
    let has_week_days = config.week_days.as_ref().is_some_and(|days| !days.is_empty());
    let has_month_days = config.month_days.as_ref().is_some_and(|days| !days.is_empty());
    let value = config.interval_value.ok_or_else(|| {
        if has_week_days {
            format!(
                "{} 你当前提供了 week_days，但 week_days 不能单独使用，必须与 interval_unit='week' 和 interval_value 一起传入。",
                interval_usage_hint
            )
        } else if has_month_days {
            format!(
                "{} 你当前提供了 month_days，但 month_days 不能单独使用，必须与 interval_unit='month' 和 interval_value 一起传入。",
                interval_usage_hint
            )
        } else {
            interval_usage_hint.to_string()
        }
    })?;
    let unit = config.interval_unit.ok_or_else(|| interval_usage_hint.to_string())?;
    if value <= 0 {
        return Err("interval_value 需要大于 0".to_string());
    }
    if has_week_days && unit != "week" {
        return Err(format!(
            "week_days 仅在 interval_unit='week' 时有效，当前 interval_unit='{}'。正确示例：interval_unit='week', interval_value=1, week_days=[1,2,3,4,5], start_time='09:00'。",
            unit
        ));
    }
    if has_month_days && unit != "month" {
        return Err(format!(
            "month_days 仅在 interval_unit='month' 时有效，当前 interval_unit='{}'。正确示例：interval_unit='month', interval_value=1, month_days=[1,15], start_time='09:00'。",
            unit
        ));
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

            let _current_weekday = local_base.weekday().num_days_from_sunday();

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

pub(crate) fn parse_json_array(s: &Option<String>) -> Option<Vec<i32>> {
    s.as_ref().and_then(|v| serde_json::from_str(v).ok())
}

pub(crate) fn to_dto(task: ScheduledTask) -> ScheduledTaskDTO {
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
        butler_conversation_id: task.butler_conversation_id,
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
        local_running: false,
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
pub async fn list_butler_scheduled_tasks(
    app_handle: tauri::AppHandle,
    _butler_conversation_id: i64,
) -> Result<Vec<ScheduledTaskDTO>, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let tasks = db.list_butler_tasks().map_err(|e| e.to_string())?;
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
    let registry = scheduled_run_cancel_registry().lock().unwrap_or_else(|e| e.into_inner());
    let runs = runs.into_iter().map(|run| {
        let mut dto = run_to_dto(run);
        dto.local_running = registry.contains_key(&dto.run_id);
        dto
    }).collect();
    Ok(ListScheduledTaskRunsResponse { runs })
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
    if let Some(butler_conversation_id) = request.butler_conversation_id {
        let conversation_db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let conversation_repo = conversation_db.conversation_repo().map_err(|e| e.to_string())?;
        let conversation = conversation_repo
            .read(butler_conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "总管家主会话不存在".to_string())?;
        if conversation.conversation_kind != "butler_main" {
            return Err("只能绑定总管家主会话".to_string());
        }
        if !is_current_butler_main_conversation(&app_handle, &conversation)? {
            return Err("总管家主会话已归档，无法绑定新的定时任务".to_string());
        }
    }
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
        butler_conversation_id: request.butler_conversation_id,
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
        butler_conversation_id: existing.butler_conversation_id,
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
    let scheduler = app_handle.state::<crate::scheduler::SchedulerState>();
    let _claim = scheduler.claim_scheduled_task(task_id)
        .ok_or_else(|| format!("定时任务 {task_id} 已在本机执行"))?;
    run_scheduled_task_now_claimed(&app_handle, &feature_config_state, task_id).await
}

async fn run_scheduled_task_now_claimed(
    app_handle: &tauri::AppHandle,
    feature_config_state: &FeatureConfigState,
    task_id: i64,
) -> Result<RunScheduledTaskResult, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let task = db
        .read_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在".to_string())?;
    crate::api::mobile_capability::ensure_assistant_execution(
        app_handle, task.assistant_id, "scheduled_manual_preflight", "not_created",
    ).await.map_err(|e| format!("task_id={task_id}; {e}"))?;
    let now = Utc::now();
    let next_run_at = if task.schedule_type == "once" {
        None
    } else {
        compute_next_run_at_with_config(
            ScheduleConfig {
                schedule_type: &task.schedule_type,
                interval_value: task.interval_value,
                interval_unit: task.interval_unit.as_deref(),
                start_time: task.start_time.as_deref(),
                week_days: parse_json_array(&task.week_days),
                month_days: parse_json_array(&task.month_days),
                run_at: task.run_at,
            },
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

    match execute_scheduled_task(app_handle, feature_config_state, &task, &updated).await {
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

fn create_scheduled_run_atomically(
    conn: &mut rusqlite::Connection,
    original: &ScheduledTask,
    updated: &ScheduledTask,
    run: &ScheduledTaskRun,
) -> Result<ScheduledTaskRun, String> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("task_id={}; run_id={}; 创建事务失败: {e}", original.id, run.run_id))?;
    let current: Option<DateTime<Utc>> = tx.query_row(
        "SELECT updated_time FROM scheduled_task WHERE id=?1", [original.id],
        |row| crate::utils::db_utils::get_required_datetime_from_row(row, 0, "updated_time"),
    ).optional().map_err(|e| format!("task_id={}; run_id={}; 读取计划失败: {e}", original.id, run.run_id))?;
    if current != Some(original.updated_time) {
        return Err(format!("task_id={}; run_id={}; 任务已变更或删除，请刷新后重试", original.id, run.run_id));
    }
    let changed = tx.execute(
        "UPDATE scheduled_task SET is_enabled=?1, next_run_at=?2, last_run_at=?3, updated_time=?4 WHERE id=?5",
        rusqlite::params![updated.is_enabled, updated.next_run_at, updated.last_run_at, updated.updated_time, original.id],
    ).map_err(|e| format!("task_id={}; run_id={}; 更新计划失败: {e}", original.id, run.run_id))?;
    if changed != 1 {
        return Err(format!("task_id={}; run_id={}; 任务已变更或删除，请刷新后重试", original.id, run.run_id));
    }
    tx.execute(
        "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, summary, error_message, started_time, finished_time, butler_followup_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![run.task_id, run.run_id, run.status, run.notify, run.summary, run.error_message, run.started_time, run.finished_time, run.butler_followup_status],
    ).map_err(|e| format!("task_id={}; run_id={}; 创建运行记录失败: {e}", original.id, run.run_id))?;
    let id = tx.last_insert_rowid();
    tx.commit().map_err(|e| format!("task_id={}; run_id={}; 提交运行记录失败: {e}", original.id, run.run_id))?;
    Ok(ScheduledTaskRun { id, ..run.clone() })
}

#[cfg(test)]
#[path = "scheduled_task_atomic_tests.rs"]
mod atomic_tests;

pub async fn execute_scheduled_task(
    app_handle: &tauri::AppHandle,
    feature_config_state: &FeatureConfigState,
    original: &ScheduledTask,
    task: &ScheduledTask,
) -> Result<RunScheduledTaskResult, String> {
    let run_id = Uuid::new_v4().to_string();
    let mut log_db = ScheduledTaskDatabase::new(app_handle)
        .map_err(|e| format!("task_id={}; run_id={run_id}; 打开运行记录数据库失败: {e}", task.id))?;
    let run_started_at = Utc::now();
    {
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
            butler_followup_status: if task.butler_conversation_id.is_some() {
                Some("pending".to_string())
            } else {
                None
            },
        };
        // 先登记本进程运行，再写入 running 记录，避免启动收尾把刚创建的运行误判为遗留
        let registration = register_scheduled_run_cancel_token(&run_id);
        let created_run = create_scheduled_run_atomically(&mut log_db.conn, original, task, &running)?;
        let mut dto = run_to_dto(created_run);
        dto.local_running = true;
        let _ = app_handle.emit(SCHEDULED_TASK_RUN_CREATED_EVENT, dto);
        log_task_message(app_handle, task.id, &run_id, "start", "开始执行定时任务");
        let run_result = match crate::api::mobile_capability::ensure_assistant_execution(
            app_handle, task.assistant_id, "scheduled_run_preflight", "not_created",
        ).await {
            Ok(()) => execute_scheduled_task_inner(
                app_handle, feature_config_state, task, &run_id, registration.token.clone(),
            ).await,
            Err(error) => Err(format!("task_id={}; run_id={run_id}; {error}", task.id)),
        };

        // 所有结束原因（含启动前置失败）统一在这里入库，再回报管家；回报失败不改写执行结果。
        let outcome = match run_result {
            Ok(outcome) => outcome,
            Err(err) => ScheduledRunOutcome::failed("error", err, None),
        };
        let entry = ScheduledOutcomeJournal {
            task_id: task.id, run_id: run_id.clone(), finished_time: Utc::now(), outcome: outcome.clone(),
        };
        let saved = persist_scheduled_outcome(app_handle, &entry);
        // 结果已入库后再注销本地运行登记（回报管家可能需要等待，不应继续显示为执行中）
        drop(registration);
        if let Err(error) = saved {
            // SQLite 保存失败时保留持久化 outbox，后台及重启均继续保存完整结果。
            log_task_message(app_handle, task.id, &run_id, "error",
                format!("保存运行结果失败，结果已保留并在后台重试保存: {error}"));
            spawn_run_result_save_retry(app_handle, task, entry);
            return Err(format!(
                "task_id={}; run_id={run_id}; 运行已结束（status={}, end_reason={}），但保存结果失败：{error}；结果已保留，正在后台重试保存",
                task.id, outcome.status, outcome.end_reason
            ));
        }
        if task.butler_conversation_id.is_some() {
            // 回报可能需要长时间排队等待主会话空闲，放到后台，不占用调度
            spawn_scheduled_run_report(app_handle, task, &run_id, &outcome);
        }
        if outcome.status == "success" {
            Ok(RunScheduledTaskResult {
                task_id: task.id,
                success: true,
                notify: outcome.notify,
                summary: if outcome.notify { outcome.summary } else { None },
                error: None,
            })
        } else {
            Err(outcome.error.unwrap_or_else(|| format!("task_id={}; run_id={run_id}; 执行失败但缺少底层原因", task.id)))
        }
    }
}

// Only a usable complete outcome can protect an old running record. A partial
// .tmp file is crash evidence, not evidence that result recovery can finish.
fn accept_scheduled_outcome_journal(
    run_id: &str,
    bytes: &[u8],
    protected: &mut Vec<String>,
) -> Result<ScheduledOutcomeJournal, String> {
    let entry: ScheduledOutcomeJournal = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if entry.run_id != run_id {
        return Err("outcome journal run_id does not match filename".to_string());
    }
    if !protected.iter().any(|id| id == run_id) {
        protected.push(run_id.to_string());
    }
    Ok(entry)
}

#[cfg(test)]
#[path = "scheduled_task_journal_tests.rs"]
mod journal_tests;

/// Replay the write-ahead outbox before deciding which old runs were interrupted.
/// A valid outcome whose persistence fails remains protected; malformed journals
/// do not prevent an abandoned running record from being marked interrupted.
fn recover_scheduled_outcome_journals(app_handle: &tauri::AppHandle) -> Result<Vec<String>, String> {
    let dir = scheduled_outbox_dir(app_handle)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取定时结果 outbox 失败: {error}")),
    };
    let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let mut protected = Vec::new();
    let mut processed = Vec::new();
    let mut paths = entries.map(|file| file.map(|file| file.path()))
        .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    // Prefer the committed name, but allow a valid synced temporary file to
    // rescue a corrupt journal instead of hiding it due to directory order.
    paths.sort_by_key(|path| path.extension().and_then(|ext| ext.to_str()) != Some("json"));
    for path in paths {
        if !matches!(path.extension().and_then(|ext| ext.to_str()), Some("json" | "tmp")) {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str())
            .and_then(|stem| Uuid::parse_str(stem).ok()).map(|id| id.to_string()) else {
                warn!(path = %path.display(), "invalid outcome journal filename; retained for repair");
                continue;
            };
        if processed.contains(&id) { continue; }
        let entry = std::fs::read(&path).map_err(|e| e.to_string())
            .and_then(|bytes| accept_scheduled_outcome_journal(&id, &bytes, &mut protected));
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(run_id = %id, error = %error, "cannot recover outcome journal; retained for repair, running record will be reconciled as interrupted unless a valid journal is found");
                continue;
            }
        };
        processed.push(id.clone());
        let Some(task) = db.read_task(entry.task_id).map_err(|e| e.to_string())? else {
            // The user removed the task. Do not recreate it or report deleted work.
            remove_outcome_journal(app_handle, &id);
            continue;
        };
        if let Err(error) = persist_scheduled_outcome(app_handle, &entry) {
            warn!(run_id = %id, error = %error, "replaying outcome journal failed; retrying in background");
            spawn_run_result_save_retry(app_handle, &task, entry);
        }
    }
    Ok(protected)
}

/// 启动时核对上一个进程已启动但未结束的运行（含已停用的一次性任务）：
/// 如实标记为中断（结果未知，不自动重跑），管家绑定的任务再回报总管家主会话。
pub async fn recover_interrupted_scheduled_runs(app_handle: &tauri::AppHandle) -> Result<usize, String> {
    let mut local_run_ids: Vec<String> = scheduled_run_cancel_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    local_run_ids.extend(recover_scheduled_outcome_journals(app_handle)?);
    let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| format!("打开运行记录数据库失败: {e}"))?;
    let interrupted = db
        .mark_interrupted_runs(
            &local_run_ids,
            "应用在任务执行过程中退出，运行被中断，执行结果未知；未自动重新执行",
            Utc::now(),
        )
        .map_err(|e| format!("收尾中断的运行记录失败: {e}"))?;
    for run in &interrupted {
        log_task_message(app_handle, run.task_id, &run.run_id, "interrupted", run.error_message.clone().unwrap_or_default());
        let _ = app_handle.emit(
            SCHEDULED_TASK_RUN_UPDATED_EVENT,
            ScheduledTaskRunUpdateEvent {
                run_id: run.run_id.clone(),
                status: run.status.clone(),
                notify: false,
                summary: None,
                error_message: run.error_message.clone(),
                finished_time: run.finished_time.map(|t| t.to_rfc3339()),
            },
        );
        if run.butler_followup_status.as_deref() != Some("pending") {
            continue;
        }
        let task = match db.read_task(run.task_id) {
            Ok(Some(task)) if task.butler_conversation_id.is_some() => task,
            Ok(_) => continue,
            Err(e) => {
                log_task_message(app_handle, run.task_id, &run.run_id, "butler_followup_error", format!("读取任务失败，无法回报总管家: {e}"));
                continue;
            }
        };
        let outcome = ScheduledRunOutcome::failed(
            "interrupted",
            run.error_message.clone().unwrap_or_default(),
            None,
        );
        spawn_scheduled_run_report(app_handle, &task, &run.run_id, &outcome);
    }

    // 已结束但回报未确认完成的运行（回报前退出、续跑中途退出）：补报
    let awaiting = db
        .list_runs_awaiting_butler_followup()
        .map_err(|e| format!("读取待回报运行失败: {e}"))?;
    for run in awaiting {
        if interrupted.iter().any(|done| done.run_id == run.run_id) {
            continue;
        }
        let task = match db.read_task(run.task_id) {
            Ok(Some(task)) if task.butler_conversation_id.is_some() => task,
            Ok(_) => continue,
            Err(e) => {
                log_task_message(app_handle, run.task_id, &run.run_id, "butler_followup_error", format!("读取任务失败，无法补报总管家: {e}"));
                continue;
            }
        };
        log_task_message(app_handle, run.task_id, &run.run_id, "butler_followup", "上次退出前回报未确认完成，重新回报总管家");
        let outcome = match db.read_run_outcome(&run.run_id).map_err(|e| e.to_string())? {
            Some(payload) => match serde_json::from_str::<ScheduledRunOutcome>(&payload) {
                Ok(outcome) => outcome,
                Err(error) => {
                    log_task_message(app_handle, task.id, &run.run_id, "butler_followup_error", format!("无法恢复完整结果，保留原始记录待修复: {error}"));
                    continue;
                }
            },
            None => outcome_from_recorded_run(&run),
        };
        spawn_scheduled_run_report(app_handle, &task, &run.run_id, &outcome);
    }
    Ok(interrupted.len())
}

/// 兼容旧版本未保存完整 outcome 的运行；新运行优先从 outcome 表恢复。
pub(crate) fn outcome_from_recorded_run(run: &ScheduledTaskRun) -> ScheduledRunOutcome {
    if run.status == "success" {
        ScheduledRunOutcome {
            status: "success",
            end_reason: "completed",
            notify: run.notify,
            summary: run.summary.clone(),
            error: None,
            detail: None,
            judged_task_state: None,
            notify_error: None,
        }
    } else {
        ScheduledRunOutcome::failed(
            "error",
            run.error_message
                .clone()
                .unwrap_or_else(|| format!("run_id={} 记录为失败但缺少原因", run.run_id)),
            None,
        )
    }
}

const RUN_RESULT_SAVE_RETRY_DELAY: Duration = Duration::from_secs(5);

/// 结果保存失败后在后台重试；保存成功后才回报总管家，保证回报与记录一致。
fn spawn_run_result_save_retry(
    app_handle: &tauri::AppHandle,
    task: &ScheduledTask,
    entry: ScheduledOutcomeJournal,
) {
    let Some(guard) = claim_scheduled_report(&format!("save:{}", entry.run_id)) else { return; };
    let app_handle = app_handle.clone();
    let task = task.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        loop {
            tokio::time::sleep(RUN_RESULT_SAVE_RETRY_DELAY).await;
            match ScheduledTaskDatabase::new(&app_handle).and_then(|db| {
                Ok(db.read_task(task.id)?.is_some() && db.read_run(&entry.run_id)?.is_some())
            }) {
                Ok(false) => { remove_outcome_journal(&app_handle, &entry.run_id); return; }
                Err(error) => { warn!(run_id = %entry.run_id, error = %error, "cannot check outcome ownership; retrying"); continue; }
                Ok(true) => {}
            }
            match persist_scheduled_outcome(&app_handle, &entry) {
                Ok(()) => {
                    log_task_message(&app_handle, task.id, &entry.run_id, "result_saved", "完整运行结果已保存");
                    if task.butler_conversation_id.is_some() {
                        spawn_scheduled_run_report(&app_handle, &task, &entry.run_id, &entry.outcome);
                    }
                    return;
                }
                Err(error) => warn!(run_id = %entry.run_id, error = %error, "retry saving scheduled run result failed"),
            }
        }
    });
}

/// 本进程正在回报的运行；同一运行只允许一个回报流程，启动补报不会与新任务的回报重叠。
fn scheduled_reports_in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static IN_FLIGHT: OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

pub(crate) struct ScheduledReportGuard(String);

impl Drop for ScheduledReportGuard {
    fn drop(&mut self) {
        scheduled_reports_in_flight().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

pub(crate) fn claim_scheduled_report(run_id: &str) -> Option<ScheduledReportGuard> {
    scheduled_reports_in_flight()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(run_id.to_string())
        .then(|| ScheduledReportGuard(run_id.to_string()))
}

fn spawn_scheduled_run_report(
    app_handle: &tauri::AppHandle,
    task: &ScheduledTask,
    run_id: &str,
    outcome: &ScheduledRunOutcome,
) {
    let app_handle = app_handle.clone();
    let task = task.clone();
    let run_id = run_id.to_string();
    let outcome = outcome.clone();
    let Some(guard) = claim_scheduled_report(&run_id) else {
        debug!(run_id = %run_id, "scheduled run report already in flight");
        return;
    };
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        report_scheduled_run_to_butler(&app_handle, &task, &run_id, &outcome).await;
    });
}

fn resolve_assistant_name(app_handle: &tauri::AppHandle, assistant_id: i64) -> String {
    match get_assistant(app_handle.clone(), assistant_id) {
        Ok(detail) => detail.assistant.name,
        Err(e) => format!("assistant_id={assistant_id}（读取名称失败: {e}）"),
    }
}

fn set_scheduled_followup_status(app_handle: &tauri::AppHandle, task: &ScheduledTask, run_id: &str, status: &str) -> Result<(), String> {
    let persisted = ScheduledTaskDatabase::new(app_handle)
        .map_err(|e| e.to_string())
        .and_then(|sdb| sdb.update_run_butler_followup_status(run_id, status).map_err(|e| e.to_string()));
    if let Err(e) = &persisted {
        log_task_message(app_handle, task.id, run_id, "butler_followup_error", format!("记录回流状态 {status} 失败: {e}"));
    }
    persisted
}

// Once a terminal delivery decision is known, retry only its state write. Never
// redispatch another turn because SQLite temporarily cannot record completion.
async fn persist_terminal_followup_status(app_handle: &tauri::AppHandle, task: &ScheduledTask, run_id: &str, status: &str) {
    while set_scheduled_followup_status(app_handle, task, run_id, status).is_err() {
        tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
    }
}

/// 续跑开始后等待主会话本轮结束；超时返回 false。
async fn wait_for_followup_turn_to_end(app_handle: &tauri::AppHandle, conversation_id: i64) -> bool {
    let started = Instant::now();
    // 给 ask_ai 留出登记运行状态的时间
    tokio::time::sleep(Duration::from_secs(1)).await;
    while is_butler_main_running(app_handle, conversation_id).await {
        if started.elapsed() > SCHEDULED_FOLLOWUP_TURN_WAIT {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    true
}

/// 把一次运行的真实结果回报给总管家主会话，并记录回报状态。
/// 只有主会话对该结果出现自然完成的回复才记为 done；忙碌时持续排队，中途退出由启动补报。
async fn report_scheduled_run_to_butler(
    app_handle: &tauri::AppHandle,
    task: &ScheduledTask,
    run_id: &str,
    outcome: &ScheduledRunOutcome,
) {
    let assistant_name = resolve_assistant_name(app_handle, task.assistant_id);
    loop {
        match ScheduledTaskDatabase::new(app_handle).and_then(|db| {
            if db.read_task(task.id)?.is_none() { return Ok(None); }
            db.read_run(run_id)
        }) {
            Ok(Some(run)) if scheduled_followup_needs_retry(run.butler_followup_status.as_deref()) => {}
            Ok(_) => return, // Deleted, completed or explicitly exhausted: never re-inject.
            Err(error) => {
                warn!(run_id, error = %error, "cannot read report state; retrying without dispatch");
                tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
                continue;
            }
        }
        let conversation_id = match active_butler_main_conversation_id(app_handle) {
            Ok(Some(id)) => id,
            Ok(None) => {
                let _ = set_scheduled_followup_status(app_handle, task, run_id, "retry");
                log_task_message(app_handle, task.id, run_id, "butler_followup_error", "当前不存在总管家主会话，保留结果等待重试");
                tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
                continue;
            }
            Err(e) => {
                let _ = set_scheduled_followup_status(app_handle, task, run_id, "retry");
                log_task_message(app_handle, task.id, run_id, "butler_followup_error", format!("读取总管家主会话失败，保留结果等待重试: {e}"));
                tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
                continue;
            }
        };
        match enqueue_scheduled_task_butler_followup(app_handle, conversation_id, task, run_id, outcome, &assistant_name).await {
            Err(e) if is_butler_main_busy_error(&e) => {
                // 主会话忙碌未获得执行权：保持排队，空闲后再回报，不抢占会话
                log_task_message(app_handle, task.id, run_id, "butler_followup_wait", format!("回流等待主会话空闲，稍后重试: {e}"));
                tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
            }
            Err(e) => {
                let _ = set_scheduled_followup_status(app_handle, task, run_id, "retry");
                log_task_message(app_handle, task.id, run_id, "butler_followup_error", format!("回流总管家失败，保留结果等待重试: {e}"));
                tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
            }
            Ok(ScheduledFollowupStep::Exhausted(reason)) => {
                persist_terminal_followup_status(app_handle, task, run_id, "exhausted").await;
                log_task_message(app_handle, task.id, run_id, "butler_followup_error", reason);
                return;
            }
            Ok(ScheduledFollowupStep::Completed) => {
                persist_terminal_followup_status(app_handle, task, run_id, "done").await;
                log_task_message(app_handle, task.id, run_id, "butler_followup", "总管家已完成对该结果的处理");
                return;
            }
            Ok(ScheduledFollowupStep::Started) => {
                let _ = set_scheduled_followup_status(app_handle, task, run_id, "continuing");
                log_task_message(app_handle, task.id, run_id, "butler_followup", "已回流至总管家主会话，等待续跑完成");
                if !wait_for_followup_turn_to_end(app_handle, conversation_id).await {
                    log_task_message(app_handle, task.id, run_id, "butler_followup_wait", "续跑长时间未结束，保持待确认并继续检查");
                    tokio::time::sleep(SCHEDULED_FOLLOWUP_BUSY_RETRY_DELAY).await;
                }
                // 回到循环开头按完成证据判定：完成则 done，否则在上限内重新续跑
            }
        }
    }
}

async fn execute_scheduled_task_inner(
    app_handle: &tauri::AppHandle,
    feature_config_state: &FeatureConfigState,
    task: &ScheduledTask,
    run_id: &str,
    run_cancel_token: CancellationToken,
) -> Result<ScheduledRunOutcome, String> {
    // ── 1. 验证助手配置 ──────────────────────────────────────────────
    let assistant_detail = get_assistant(app_handle.clone(), task.assistant_id)
        .map_err(|e| format!("Failed to get assistant: {}", e))?;
    if assistant_detail.assistant.assistant_type.unwrap_or(0) != 0 {
        log_task_message(app_handle, task.id, run_id, "error", "只能选择普通对话助手");
        // 外层会统一写入最终结果
        let _ = update_task_run(
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
        // 外层会统一写入最终结果
        let _ = update_task_run(
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
    let slash_parse_result =
        parse_slash_prompt(app_handle, &task.task_prompt).await.map_err(|e| e.to_string())?;
    let system_prompt_rendered = template_engine.parse(&system_prompt, &template_context).await;
    let task_prompt_rendered =
        template_engine.parse(&slash_parse_result.runtime_user_prompt, &template_context).await;
    let task_prompt_display =
        template_engine.parse(&slash_parse_result.display_prompt, &template_context).await;
    log_task_message(app_handle, task.id, run_id, "task_prompt", task_prompt_display.clone());

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
    let task_prompt_with_skills = compose_user_message_with_active_skills(
        &task_prompt_rendered,
        &slash_parse_result.active_skills,
    );
    let active_skill_attachments =
        build_active_skill_attachments(&slash_parse_result.active_skills)
            .map_err(|e| e.to_string())?;

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
        None,
        &crate::api::copilot_token_manager::prepare_provider_configs(
            app_handle,
            &model_detail.provider.api_type,
            &model_detail.configs,
            network_proxy.as_deref(),
        )
        .await
        .map_err(|e| format!("Copilot token exchange failed: {}", e))?,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        Some(&model_detail.model.request_mode),
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
    let chat_options = ConfigBuilder::build_chat_options(&config_map, None, None);
    let capture_reasoning_content =
        is_openai_like && has_available_tools && config_map.contains_key("reasoning_effort");
    let chat_options = chat_options
        .with_normalize_reasoning_content(true)
        .with_capture_reasoning_content(capture_reasoning_content)
        .with_capture_usage(capture_usage)
        .with_capture_tool_calls(has_available_tools);

    let tool_call_strategy =
        if has_available_tools { ToolCallStrategy::Native } else { ToolCallStrategy::NonNative };
    let tool_config = build_tool_config_for_mcp(&mcp_info, has_available_tools);
    let tool_name_mapping =
        tool_config.as_ref().map(|c| c.tool_name_mapping.clone()).unwrap_or_default();

    // ── 5. 创建对话并初始化消息 ─────────────────────────────────────
    let mut init_messages =
        vec![("system".to_string(), assistant_prompt_result.clone(), Vec::new())];
    init_messages.push((
        "user".to_string(),
        task_prompt_with_skills.clone(),
        active_skill_attachments,
    ));
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

    // 非正常结束（错误 / 停止 / 超时 / 轮次耗尽）直接按真实原因判失败，不再进入通知判定
    let detail = (!task_result.trim().is_empty()).then(|| task_result.clone());
    if let Some((end_reason, error)) = loop_status_failure(
        &loop_result.status,
        loop_result.rounds,
        loop_result.tool_calls_success,
        loop_result.tool_calls_failed,
    ) {
        log_task_message(app_handle, task.id, run_id, end_reason, &error);
        let outcome = ScheduledRunOutcome::failed(end_reason, error, detail);
        let journal = ScheduledOutcomeJournal {
            task_id: task.id, run_id: run_id.to_string(), finished_time: Utc::now(), outcome: outcome.clone(),
        };
        match write_outcome_journal(app_handle, &journal) {
            Ok(()) => {
                if let Err(e) = cleanup_conversation(app_handle, conversation_id) {
                    log_task_message(app_handle, task.id, run_id, "cleanup_error", format!("清理对话记录失败: {e}"));
                }
            }
            Err(e) => log_task_message(app_handle, task.id, run_id, "result_save_error", format!("持久化结果失败，保留原会话: {e}")),
        }
        return Ok(outcome);
    }

    // ── 7. 通知判定（失败只影响是否通知，不改写执行结果） ────────────────
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
    let notify_decision: Result<NotifyDecision, String> = match await_llm_or_stop(
        client.exec_chat(&model_name, notify_request, Some(&notify_options)),
        &run_cancel_token,
        Instant::now() + Duration::from_secs(NOTIFY_DECISION_TIMEOUT_SECS),
    )
    .await
    {
        LlmWait::Done(Ok(response)) => {
            let notify_text = response.content.into_joined_texts().unwrap_or_default();
            log_task_message(app_handle, task.id, run_id, "notify_raw", notify_text.clone());
            parse_notify_decision(&notify_text).map_err(|e| {
                error!(error = %e, raw = %notify_text, "invalid notify decision payload");
                format!("通知判定返回不合法: {}", e)
            })
        }
        LlmWait::Done(Err(e)) => Err(format!("通知判定执行失败: {}", e)),
        LlmWait::Cancelled => Err("通知判定期间收到停止请求，已跳过通知判定（任务主体已完成）".to_string()),
        LlmWait::Timeout => Err(format!("通知判定超时（{}s）", NOTIFY_DECISION_TIMEOUT_SECS)),
    };

    let (notify, summary, judged_task_state, notify_error) = match notify_decision {
        Ok(decision) => {
            log_task_message(
                app_handle,
                task.id,
                run_id,
                "notify_result",
                format!(
                    "task_state={}, notify={}, summary={}, reason={}",
                    decision.task_state.clone().unwrap_or_else(|| "unknown".to_string()),
                    decision.notify,
                    decision.summary.clone().unwrap_or_default(),
                    decision.reason.clone().unwrap_or_default()
                ),
            );
            (decision.notify, decision.summary, decision.task_state, None)
        }
        Err(e) => {
            log_task_message(app_handle, task.id, run_id, "error", &e);
            (false, None, None, Some(e))
        }
    };

    let outcome = ScheduledRunOutcome {
        status: "success", end_reason: "completed", notify, summary: summary.clone(), error: None,
        detail, judged_task_state, notify_error: notify_error.clone(),
    };
    let journal = ScheduledOutcomeJournal {
        task_id: task.id, run_id: run_id.to_string(), finished_time: Utc::now(), outcome: outcome.clone(),
    };
    let journal_saved = match write_outcome_journal(app_handle, &journal) {
        Ok(()) => true,
        Err(e) => {
            log_task_message(app_handle, task.id, run_id, "result_save_error", format!("持久化结果失败，保留原会话: {e}"));
            false
        }
    };

    // ── 8. 通知或清理 ─────────────────────────────────────────────
    if notify {
        conversation.name = format!("计划任务: {}", task.name);
        if let Err(e) = conversation_db
            .conversation_repo()
            .map_err(|e| e.to_string())
            .and_then(|repo| repo.update(&conversation).map_err(|e| e.to_string()))
        {
            log_task_message(app_handle, task.id, run_id, "error", format!("更新对话名称失败: {e}"));
        }
        if let Some(cache_state) = app_handle.try_state::<NameCacheState>() {
            let mut assistant_name_cache = cache_state.assistant_names.lock().await;
            assistant_name_cache.insert(task.assistant_id, assistant_detail.assistant.name.clone());
        }
        let _ = app_handle.emit("conversation_created", conversation.id);

        match app_handle
            .notification()
            .builder()
            .title("定时任务完成")
            .body("定时任务完成，请查看对话")
            .show()
        {
            Ok(()) => log_task_message(app_handle, task.id, run_id, "notify", "已发送系统通知"),
            Err(e) => log_task_message(app_handle, task.id, run_id, "error", format!("发送系统通知失败: {e}")),
        }
    } else if notify_error.is_some() || !journal_saved {
        // 通知判定或结果持久化失败时保留对话，方便用户查看执行结果
        log_task_message(app_handle, task.id, run_id, "cleanup", "通知判定失败，保留对话记录");
    } else {
        match cleanup_conversation(app_handle, conversation_id) {
            Ok(()) => log_task_message(app_handle, task.id, run_id, "cleanup", "未通知，已清理对话记录"),
            Err(e) => log_task_message(app_handle, task.id, run_id, "cleanup_error", format!("清理对话记录失败: {e}")),
        }
    }

    Ok(outcome)
}

// ────────────────────────────────────────────────────────────────────────────
// Butler backflow: inject scheduled task result into Butler main conversation
// ────────────────────────────────────────────────────────────────────────────

const SCHEDULED_TASK_RESULT_DETAIL_LIMIT: usize = 4000;

fn truncate_at_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut cut = max_bytes;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

pub(crate) fn build_butler_scheduled_task_result_message(
    task: &ScheduledTask,
    run_id: &str,
    outcome: &ScheduledRunOutcome,
    assistant_name: &str,
) -> String {
    let summary_text = outcome.summary.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("无");
    let detail_text = outcome
        .detail
        .as_deref()
        .filter(|d| !d.trim().is_empty())
        .map(|d| {
            let cut = truncate_at_char_boundary(d, SCHEDULED_TASK_RESULT_DETAIL_LIMIT);
            if cut.len() < d.len() { format!("{cut}…(truncated)") } else { d.to_string() }
        })
        .unwrap_or_else(|| "无".to_string());
    let task_prompt = {
        let cut = truncate_at_char_boundary(&task.task_prompt, 200);
        if cut.len() < task.task_prompt.len() { format!("{cut}…") } else { task.task_prompt.clone() }
    };

    format!(
        "<butler_scheduled_task_result>
status={status}
end_reason={end_reason}
error={error}
scheduled_task_id={task_id}
scheduled_task_name={name}
run_id={run_id}
assistant_name={assistant_name}
task_prompt={task_prompt}
notify_decision={notify}
judged_task_state={judged}
notify_error={notify_error}
summary={summary}
detail_excerpt={detail}
</butler_scheduled_task_result>",
        status = outcome.status,
        end_reason = outcome.end_reason,
        error = outcome.error.as_deref().unwrap_or("无"),
        task_id = task.id,
        name = task.name,
        run_id = run_id,
        assistant_name = assistant_name,
        task_prompt = task_prompt,
        notify = outcome.notify,
        judged = outcome.judged_task_state.as_deref().unwrap_or("无"),
        notify_error = outcome.notify_error.as_deref().unwrap_or("无"),
        summary = summary_text,
        detail = detail_text,
    )
}

pub(crate) fn build_butler_scheduled_task_followup_prompt(task: &ScheduledTask, outcome: &ScheduledRunOutcome) -> String {
    let status_desc = match (outcome.status, outcome.end_reason) {
        ("success", _) => "成功".to_string(),
        (_, "cancelled") => "已被停止".to_string(),
        (_, "timeout") => "超时未完成".to_string(),
        (_, "max_rounds_reached") => "达到最大轮次未完成".to_string(),
        (_, "interrupted") => "因应用退出而中断，结果未知".to_string(),
        _ => "失败".to_string(),
    };
    format!(
        "系统定时任务回流：定时任务《{name}》执行{status_desc}。这不是终端用户的新需求，而是你之前安排的定时任务执行回调。请基于最新注入的 <butler_scheduled_task_result> 信息判断下一步：成功且结果重要时向用户汇报；失败、停止、超时或中断时如实告知用户原因与已完成部分，不要把未完成的任务说成已完成，也不要自动重做可能已产生外部影响的操作。",
        name = task.name,
    )
}

pub(crate) enum ScheduledFollowupStep {
    /// 主会话已对该结果完成一轮自然结束的回复
    Completed,
    /// 已开始一轮续跑，需等待其结束后再判定
    Started,
    /// 模型续跑已达到上限；保留结果，不自动重做可能有副作用的操作。
    Exhausted(String),
}

/// 主会话里注入过的该次运行结果消息（按 run_id 识别，避免重复注入）。
pub(crate) fn find_scheduled_result_message_id(
    messages: &[crate::db::conversation_db::Message],
    run_id: &str,
) -> Option<i64> {
    let run_line = format!("run_id={run_id}");
    messages.iter().rev().find_map(|message| {
        (message.message_type == "system"
            && message.content.contains("<butler_scheduled_task_result>")
            && message.content.lines().any(|line| line == run_line))
        .then_some(message.id)
    })
}

async fn enqueue_scheduled_task_butler_followup(
    app_handle: &tauri::AppHandle,
    butler_conversation_id: i64,
    task: &ScheduledTask,
    run_id: &str,
    outcome: &ScheduledRunOutcome,
    assistant_name: &str,
) -> Result<ScheduledFollowupStep, String> {
    let continuation_lock = get_butler_main_continuation_lock(butler_conversation_id).await;
    let _guard = continuation_lock.lock().await;
    wait_for_butler_main_to_be_idle(app_handle, butler_conversation_id)
        .await
        .map_err(ButlerMainBusy::into_error_string)?;

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;

    let main_conversation = conversation_repo
        .read(butler_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "总管家主会话不存在".to_string())?;

    if main_conversation.conversation_kind != "butler_main" {
        return Err("总管家主会话已归档，无法回流定时任务结果".to_string());
    }
    if !is_current_butler_main_conversation(app_handle, &main_conversation)? {
        return Err("总管家主会话已归档，无法回流定时任务结果".to_string());
    }
    let assistant_id =
        main_conversation.assistant_id.ok_or_else(|| "总管家主会话缺少 assistant".to_string())?;

    let main_messages: Vec<_> = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .list_by_conversation_id(butler_conversation_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(message, _)| message)
        .collect();
    if let Some(anchor) = find_scheduled_result_message_id(&main_messages, run_id) {
        match followup_turn_progress(&main_messages, anchor, &scheduled_followup_key(run_id)) {
            FollowupTurnProgress::Completed => return Ok(ScheduledFollowupStep::Completed),
            FollowupTurnProgress::Incomplete { attempts } if attempts >= FOLLOWUP_MAX_CONTINUATION_ATTEMPTS => {
                return Ok(ScheduledFollowupStep::Exhausted(format!("结果已注入主会话（message_id={anchor}），但总管家续跑 {attempts} 次都未完成，停止自动重试")));
            }
            // 已注入但续跑未完成：不重复注入，只重新续跑
            _ => {}
        }
    } else {
        let system_message_content =
            build_butler_scheduled_task_result_message(task, run_id, outcome, assistant_name);
        add_message(
            app_handle,
            None,
            butler_conversation_id,
            "system".to_string(),
            system_message_content,
            None,
            None,
            None,
            None,
            0,
            None,
            None,
        )
        .map_err(|e| e.to_string())?;
    }

    let prompt = build_butler_scheduled_task_followup_prompt(task, outcome);
    let window = resolve_or_create_butler_execution_window(app_handle)?;
    let ai_request = AiRequest {
        conversation_id: butler_conversation_id.to_string(),
        assistant_id,
        prompt,
        model: None,
        override_model_id: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: Some(true),
        attachment_list: None,
        quote: None,
    };

    FOLLOWUP_KEYS.scope(vec![scheduled_followup_key(run_id)], ask_ai(
        app_handle.clone(),
        app_handle.state::<crate::AppState>(),
        app_handle.state::<crate::AcpSessionState>(),
        app_handle.state::<crate::FeatureConfigState>(),
        app_handle.state::<crate::state::message_token::MessageTokenManager>(),
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>(),
        window,
        ai_request,
        None,
        None,
        None,
        None,
        Some("internal".to_string()),
    ))
    .await
    .map_err(|e| e.to_string())?;

    Ok(ScheduledFollowupStep::Started)
}

pub(crate) fn scheduled_followup_key(run_id: &str) -> String {
    format!("run:{run_id}")
}

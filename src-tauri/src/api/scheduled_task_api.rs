//! 定时任务的执行与界面命令。
//!
//! 一次运行：调度器登记运行（见 `scheduler::scheduled_task`）→ 在隐藏的运行会话里执行
//! （每个工具调用先过无人值守准入，见 `scheduler::gate`）→ 结果先写结果日志（outbox）
//! → 按任务设置投递给用户（见 `scheduler::delivery`）→ 结果与投递信息一起入库。
//! 崩溃恢复重放结果日志时投递幂等，不会重复发消息。
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::api::ai::config::{
    get_network_proxy_from_config, get_request_timeout_from_config, ConfigBuilder,
};
use crate::api::ai::conversation::ToolCallStrategy;
#[allow(unused_imports)]
pub(crate) use crate::api::ai::unattended_loop::{
    await_llm_or_stop, build_mcp_tool_call_ui_hint, build_tool_config_for_mcp,
    extract_prompt_tool_calls, normalize_tool_arguments, run_unattended_loop, save_round_response,
    AgenticLoopResult, AgenticLoopStatus, LlmWait, LoopLimits, UnattendedLoopContext,
    UnattendedLoopObserver, UnattendedToolGate,
};
use crate::api::assistant_api::get_assistant;
use crate::api::genai_client::create_client_with_config;
use crate::db::conversation_db::{ConversationDatabase, Repository as ConversationRepository};
use crate::db::llm_db::LLMDatabase;
use crate::db::scheduled_task_db::{
    RunFinish, ScheduledTask, ScheduledTaskDatabase, ScheduledTaskLog, DELIVER_RUN_CONVERSATION,
    RUN_CANCELLED, RUN_DELIVERED, RUN_FAILED, RUN_QUIET, RUN_SKIPPED,
};
use crate::db::system_db::FeatureConfig;
use crate::mcp::{collect_mcp_info_for_assistant, format_mcp_prompt};
use crate::scheduler::delivery::{self, DeliveryContent};
use crate::scheduler::gate::{self, Admission, GateContext};
use crate::scheduler::service::{self, RunView, SchedulePreview, TaskDefaults, TaskInput, TaskPatch, TaskView};
use crate::scheduler::{expr, ScheduledRunKind};
use crate::skills::{
    build_active_skill_attachments, collect_skills_info_for_assistant,
    compose_user_message_with_active_skills, format_skills_prompt,
};
use crate::slash::parse_slash_prompt;
use crate::template_engine::build_template_engine;
use crate::{AppState, FeatureConfigState};
use uuid::Uuid;

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
struct ScheduledTaskRunUpdateEvent {
    pub task_id: i64,
    pub run_id: String,
    pub status: String,
}

const AGENTIC_LOOP_MAX_ROUNDS: usize = 50;
const AGENTIC_LOOP_PER_TOOL_TIMEOUT_SECS: u64 = 120;
const AGENTIC_LOOP_TOTAL_TIMEOUT_SECS: u64 = 3000;
/// 模型接口的临时错误在本次运行内重试的等待时间。
const LLM_RETRY_DELAYS_SECS: [u64; 3] = [30, 120, 300];
/// 模型没有值得告诉用户的内容时的回复。
pub const NO_REPLY: &str = "[NO_REPLY]";
const SUMMARY_LIMIT: usize = 200;
const PREVIOUS_RUN_LIMIT: usize = 1500;
/// 实际开始比计划晚这么多时，在运行提示里说明延迟。
const LATE_NOTICE_AFTER: chrono::Duration = chrono::Duration::minutes(2);
/// 每个任务保留的隐藏运行会话数。
const SCHEDULED_RUN_HIDDEN_KEEP: u32 = 20;
/// 连续失败达到这个次数时，在投递会话里提醒一次。
pub const REPAIR_NOTICE_AFTER: i64 = 2;
/// 连续失败达到这个次数时自动暂停。
pub const AUTO_PAUSE_AFTER: i64 = 5;

pub const SCHEDULED_TASK_LOG_ADDED_EVENT: &str = "scheduled_task_log_added";
pub const SCHEDULED_TASK_RUN_CREATED_EVENT: &str = "scheduled_task_run_created";
pub const SCHEDULED_TASK_RUN_UPDATED_EVENT: &str = "scheduled_task_run_updated";
pub const SCHEDULED_TASK_CHANGED_EVENT: &str = "scheduled_task_changed";

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

/// 本进程是否持有这次运行的停止令牌（界面据此显示「停止」）。
pub fn is_run_local(run_id: &str) -> bool {
    scheduled_run_cancel_registry().lock().unwrap_or_else(|e| e.into_inner()).contains_key(run_id)
}

fn cancel_scheduled_run(run_id: &str) -> bool {
    let token = scheduled_run_cancel_registry().lock().unwrap_or_else(|e| e.into_inner()).get(run_id).cloned();
    match token {
        Some(token) => {
            token.cancel();
            true
        }
        None => false,
    }
}

// ── Agentic Loop Adapter ────────────────────────────────────────────────

struct ScheduledToolGate<'a> {
    ctx: GateContext<'a>,
}

#[async_trait::async_trait]
impl UnattendedToolGate for ScheduledToolGate<'_> {
    async fn admit(&self, call_db_id: i64, progress: &str) -> Admission {
        gate::admit(&self.ctx, call_db_id, progress).await
    }
}

struct ScheduledLoopObserver<'a> {
    app_handle: &'a tauri::AppHandle,
    task_id: i64,
    run_id: &'a str,
}

impl UnattendedLoopObserver for ScheduledLoopObserver<'_> {
    fn on_log(&self, kind: &str, content: String) {
        log_task_message(self.app_handle, self.task_id, self.run_id, kind, content);
    }
}

/// 将非正常结束的循环状态映射为失败结果；Completed 返回 None。
pub(crate) fn loop_status_failure(
    status: &AgenticLoopStatus,
    rounds: usize,
    tool_calls_success: usize,
    tool_calls_failed: usize,
) -> Option<(&'static str, String)> {
    crate::api::ai::unattended_loop::loop_status_failure_with_limits(
        status,
        rounds,
        tool_calls_success,
        tool_calls_failed,
        AGENTIC_LOOP_TOTAL_TIMEOUT_SECS,
        AGENTIC_LOOP_MAX_ROUNDS,
    )
}

// ── 运行结果与结果日志 ──────────────────────────────────────────────────

/// 一次运行的真实结束结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ScheduledRunOutcome {
    /// delivered（待投递/已投递）| quiet | failed | cancelled | skipped
    pub(crate) status: String,
    /// completed / no_reply / never / cancelled / timeout / max_rounds_reached / error / interrupted / preflight
    pub(crate) end_reason: String,
    /// 交给用户的内容（或失败前的部分产出）。
    #[serde(default, alias = "detail")]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) conversation_id: Option<i64>,
    #[serde(default)]
    pub(crate) final_message_id: Option<i64>,
    #[serde(default)]
    pub(crate) llm_model_id: Option<i64>,
    #[serde(default)]
    pub(crate) llm_model_name: Option<String>,
    /// 旧版本结果日志的通知标记，用于把 success 映射为 delivered/quiet。
    #[serde(default, skip_serializing)]
    pub(crate) notify: bool,
}

impl ScheduledRunOutcome {
    pub(crate) fn failed(end_reason: &str, error: String, text: Option<String>) -> Self {
        Self {
            status: RUN_FAILED.to_string(),
            end_reason: end_reason.to_string(),
            text,
            error: Some(error),
            conversation_id: None,
            final_message_id: None,
            llm_model_id: None,
            llm_model_name: None,
            notify: false,
        }
    }

    pub(crate) fn skipped(reason: String) -> Self {
        Self { status: RUN_SKIPPED.to_string(), end_reason: "preflight".to_string(), ..Self::failed("preflight", reason, None) }
    }

    /// 旧版本的 success 按当时是否通知映射。
    fn normalized(mut self) -> Self {
        if self.status == "success" {
            self.status = if self.notify { RUN_DELIVERED } else { RUN_QUIET }.to_string();
        }
        self
    }
}

/// 根据执行结果、最终输出与投递策略决定状态（不含投递本身）。
pub(crate) fn classify_completion(final_text: &str, deliver_policy: &str) -> (&'static str, &'static str) {
    let text = final_text.trim();
    if text.is_empty() {
        return (RUN_FAILED, "empty_output");
    }
    if text == NO_REPLY || text.trim_matches(|c: char| c == '`' || c.is_whitespace()) == NO_REPLY {
        return (RUN_QUIET, "no_reply");
    }
    if deliver_policy == service::DELIVER_NEVER {
        return (RUN_QUIET, "never");
    }
    (RUN_DELIVERED, "completed")
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

fn summary_of(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    if text.is_empty() || text == NO_REPLY {
        return None;
    }
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(if flat.chars().count() > SUMMARY_LIMIT {
        format!("{}…", flat.chars().take(SUMMARY_LIMIT).collect::<String>())
    } else {
        flat
    })
}

/// 投递并入库。返回更新后的连续失败次数。
fn persist_scheduled_outcome(app_handle: &tauri::AppHandle, entry: &ScheduledOutcomeJournal) -> Result<i64, String> {
    let journal_error = write_outcome_journal(app_handle, entry).err();
    let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let task = db.read_task(entry.task_id).map_err(|e| e.to_string())?
        .ok_or_else(|| format!("定时任务 #{} 已删除，不再保存运行 {} 的结果", entry.task_id, entry.run_id))?;
    let (failures, finish) = store_run_delivery(&db, entry, |content| delivery::deliver(app_handle, &task, content))
        .map_err(|error| match journal_error {
            Some(journal_error) => format!("SQLite: {error}; durable outbox also failed: {journal_error}; outcome retained only in memory"),
            None => format!("SQLite: {error}; complete outcome retained in durable outbox"),
        })?;
    if let Some(error) = &finish.delivery_error {
        log_task_message(app_handle, task.id, &entry.run_id, "delivery_error", error);
    } else if let (Some(chat), Some(message)) = (finish.delivery_conversation_id, finish.delivered_message_id) {
        log_task_message(app_handle, task.id, &entry.run_id, "delivered", format!("已发送到会话 #{chat}（消息 #{message}）"));
    }
    remove_outcome_journal(app_handle, &entry.run_id);
    let _ = app_handle.emit(SCHEDULED_TASK_RUN_UPDATED_EVENT, ScheduledTaskRunUpdateEvent {
        task_id: task.id,
        run_id: entry.run_id.clone(),
        status: finish.status.clone(),
    });
    let _ = app_handle.emit(SCHEDULED_TASK_CHANGED_EVENT, serde_json::json!({ "taskId": task.id }));
    Ok(failures)
}

fn store_run_delivery(
    db: &ScheduledTaskDatabase, entry: &ScheduledOutcomeJournal,
    deliver: impl FnOnce(&DeliveryContent<'_>) -> Result<delivery::Delivered, String>,
) -> Result<(i64, RunFinish), String> {
    let outcome = entry.outcome.clone().normalized();
    let mut finish = RunFinish {
        status: outcome.status.clone(),
        notify: false,
        summary: summary_of(outcome.text.as_deref()),
        error_message: outcome.error.clone(),
        finished_time: Some(entry.finished_time),
        conversation_id: outcome.conversation_id,
        final_message_id: outcome.final_message_id,
        ..Default::default()
    };
    if outcome.status == RUN_DELIVERED {
        let scheduled_for = db.read_run(&entry.run_id).map_err(|e| e.to_string())?.and_then(|run| run.scheduled_for);
        let content = DeliveryContent {
            run_id: &entry.run_id,
            scheduled_for,
            run_conversation_id: outcome.conversation_id.unwrap_or_default(),
            final_message_id: outcome.final_message_id,
            text: outcome.text.as_deref().unwrap_or_default(),
            llm_model_id: outcome.llm_model_id,
            llm_model_name: outcome.llm_model_name.clone(),
        };
        match deliver(&content) {
            Ok(delivered) => {
                finish.notify = true;
                finish.delivered_message_id = Some(delivered.message_id);
                finish.delivery_conversation_id = Some(delivered.conversation_id);
            }
            Err(error) => {
                finish.status = RUN_FAILED.to_string();
                finish.error_message = Some(format!("已完成，但发送给用户失败：{error}"));
                finish.delivery_error = Some(error);
            }
        }
    }
    let outcome_json = serde_json::to_string(&outcome).map_err(|e| e.to_string())?;
    let failures = db.save_run_outcome(&entry.run_id, &finish, &outcome_json).map_err(|e| e.to_string())?;
    Ok((failures, finish))
}

fn after_finish(app_handle: &tauri::AppHandle, task_id: i64, run_id: &str, failures: i64) {
    let Ok(db) = ScheduledTaskDatabase::new(app_handle) else { return };
    let Ok(Some(task)) = db.read_task(task_id) else { return };
    let Ok(Some(run)) = db.read_run(run_id) else { return };
    let reason = run.error_message.clone().unwrap_or_else(|| "未记录原因".to_string());
    let notice = if failures >= AUTO_PAUSE_AFTER && task.is_enabled {
        let pause_reason = format!("连续失败 {failures} 次，已自动暂停");
        if db.pause_task(task.id, &pause_reason, Utc::now()).is_err() {
            return;
        }
        let _ = app_handle.emit(SCHEDULED_TASK_CHANGED_EVENT, serde_json::json!({ "taskId": task.id }));
        Some(format!(
            "定时任务《{}》已经连续 {failures} 次没有完成，我先把它暂停了。最近一次的原因：{reason}\n\n需要的话告诉我怎么调整，或者在「定时任务」里修改后恢复。",
            task.name
        ))
    } else if failures == REPAIR_NOTICE_AFTER {
        Some(format!(
            "定时任务《{}》最近 {failures} 次都没有完成。最近一次的原因：{reason}\n\n需要的话告诉我怎么调整（比如改时间、改指令或换执行助手），也可以在「定时任务」里查看每次运行的详情。",
            task.name
        ))
    } else {
        None
    };
    let Some(text) = notice else { return };
    // 修复提醒总是发到执行助手能接着处理的地方；单独会话模式没有固定会话，改投执行助手的私聊。
    let target = if task.deliver_to == DELIVER_RUN_CONVERSATION {
        ScheduledTask { deliver_to: crate::db::scheduled_task_db::DELIVER_OWNER_CHAT.to_string(), ..task.clone() }
    } else {
        task.clone()
    };
    let notice_id = format!("{run_id}:notice");
    let content = DeliveryContent {
        run_id: &notice_id,
        scheduled_for: run.scheduled_for,
        run_conversation_id: run.conversation_id.unwrap_or_default(),
        final_message_id: None,
        text: &text,
        llm_model_id: None,
        llm_model_name: None,
    };
    if let Err(error) = delivery::deliver(app_handle, &target, &content) {
        log_task_message(app_handle, task.id, run_id, "delivery_error", format!("发送失败提醒失败：{error}"));
    }
}

// ── 运行 ────────────────────────────────────────────────────────────────

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

pub(crate) fn log_task_message(
    app_handle: &tauri::AppHandle,
    task_id: i64,
    run_id: &str,
    message_type: &str,
    content: impl Into<String>,
) {
    let _ = write_task_log(app_handle, task_id, run_id, message_type, content);
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

fn cleanup_conversation(app_handle: &tauri::AppHandle, conversation_id: i64) -> Result<(), String> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    conversation_db
        .with_write_connection(|conn| {
            crate::db::conversation_db::cascade_delete_conversation(conn, conversation_id)
                .map_err(crate::errors::AppError::from)?;
            Ok(())
        })
        .map_err(|e| e.to_string())
}

/// 每个任务只保留最近若干个隐藏运行会话。
fn prune_hidden_run_conversations(app_handle: &tauri::AppHandle, task_id: i64, run_id: &str) {
    let stale = ConversationDatabase::new(app_handle)
        .map_err(|e| e.to_string())
        .and_then(|db| db.get_connection().map_err(|e| e.to_string()))
        .and_then(|conn| {
            crate::db::message_preview::scheduled_run_conversation_ids_beyond(&conn, task_id, SCHEDULED_RUN_HIDDEN_KEEP)
                .map_err(|e| e.to_string())
        });
    match stale {
        Ok(ids) => {
            for id in ids {
                if let Err(e) = cleanup_conversation(app_handle, id) {
                    log_task_message(app_handle, task_id, run_id, "cleanup_error", format!("清理更早的运行会话 {id} 失败: {e}"));
                }
            }
        }
        Err(e) => log_task_message(app_handle, task_id, run_id, "cleanup_error", format!("查询更早的运行会话失败: {e}")),
    }
}

fn suppress_completion_notification(config_feature_map: &mut HashMap<String, HashMap<String, FeatureConfig>>) {
    let display_config = config_feature_map.entry("display".to_string()).or_default();
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

/// 上一次结束的运行（给下一次运行参考，便于「有变化才告诉我」）。
fn previous_run_note(db: &ScheduledTaskDatabase, task_id: i64, run_id: &str) -> Option<String> {
    let previous = db
        .list_runs_by_task(task_id, 10)
        .ok()?
        .into_iter()
        .find(|run| run.run_id != run_id && matches!(run.status.as_str(), RUN_DELIVERED | RUN_QUIET | RUN_FAILED | "success"))?;
    let at = previous.finished_time.unwrap_or(previous.started_time);
    let status = service::status_text(&previous.status, previous.notify);
    let text = db
        .read_run_outcome(&previous.run_id)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<ScheduledRunOutcome>(&json).ok())
        .and_then(|outcome| outcome.text.or(outcome.error))
        .or(previous.summary)
        .or(previous.error_message)
        .unwrap_or_default();
    let text = if text.chars().count() > PREVIOUS_RUN_LIMIT {
        format!("{}…（已截断）", text.chars().take(PREVIOUS_RUN_LIMIT).collect::<String>())
    } else {
        text
    };
    Some(format!("上一次运行：{}，{status}。\n{text}", expr::format_local(at)))
}

/// 每次运行附加在系统提示末尾的无人值守约定。
pub(crate) fn unattended_contract(
    task: &ScheduledTask,
    run_number: i64,
    scheduled_for: DateTime<Utc>,
    started_at: DateTime<Utc>,
    deliver_target: &str,
    previous: Option<&str>,
) -> String {
    let late = started_at - scheduled_for;
    let timing = if late > LATE_NOTICE_AFTER {
        format!(
            "计划 {}，实际 {}（晚了约 {} 分钟，因为应用当时没有运行）。如果内容已经不合时宜，请据实调整，或回复 {NO_REPLY}",
            expr::format_local(scheduled_for),
            expr::format_local(started_at),
            late.num_minutes()
        )
    } else {
        format!("时间 {}", expr::format_local(started_at))
    };
    let delivery = match task.deliver_policy.as_str() {
        service::DELIVER_NEVER => "这次的结果只记录、不发送给用户。".to_string(),
        service::DELIVER_IF_NEEDED => format!(
            "你的最终回复会以你的身份发到{deliver_target}。没有值得告诉用户的新内容时，最终回复只写 {NO_REPLY}，这次就不会打扰用户。"
        ),
        _ => format!("你的最终回复会以你的身份发到{deliver_target}，每次都会发送。"),
    };
    let previous = previous.map(|p| format!("\n{p}")).unwrap_or_default();
    format!(
        "\n\n<scheduled_task_run>\n现在是定时任务《{name}》（task_id={id}）的第 {run_number} 次运行，{timing}。\n\
- 用户此刻不在场，不能向用户提问或等待回答；需要用户决定的事写进最终回复。\n\
- {delivery}直接写给用户，不要复述这段说明。\n\
- 自动审核没通过的操作会交给总管家裁决；被拒绝的操作不要反复尝试，在最终回复里说明卡在哪一步。\n\
- 任务已经完成、条件已经达成、不需要再运行时，用 schedule_task 的 pause 暂停本任务（task_id={id}）。{previous}\n</scheduled_task_run>",
        name = task.name,
        id = task.id,
    )
}

/// 执行一次已登记的运行，结束后入库、投递并处理连续失败。
pub(crate) async fn execute_claimed_run(
    app_handle: tauri::AppHandle,
    task: ScheduledTask,
    run_id: String,
    scheduled_for: DateTime<Utc>,
    kind: ScheduledRunKind,
) {
    let registration = register_scheduled_run_cancel_token(&run_id);
    let _ = app_handle.emit(
        SCHEDULED_TASK_RUN_CREATED_EVENT,
        ScheduledTaskRunUpdateEvent { task_id: task.id, run_id: run_id.clone(), status: "running".to_string() },
    );
    log_task_message(&app_handle, task.id, &run_id, "start",
        if kind == ScheduledRunKind::Manual { "手动运行开始" } else { "开始执行定时任务" });

    let outcome = match execute_run_inner(&app_handle, &task, &run_id, scheduled_for, registration.token.clone()).await {
        Ok(outcome) => outcome,
        Err(RunError::Preflight(reason)) => {
            log_task_message(&app_handle, task.id, &run_id, "skipped", &reason);
            ScheduledRunOutcome::skipped(reason)
        }
        Err(RunError::Failed(error)) => {
            log_task_message(&app_handle, task.id, &run_id, "error", &error);
            ScheduledRunOutcome::failed("error", format!("task_id={}; run_id={run_id}; {error}", task.id), None)
        }
    };
    let entry = ScheduledOutcomeJournal { task_id: task.id, run_id: run_id.clone(), finished_time: Utc::now(), outcome };
    let saved = persist_scheduled_outcome(&app_handle, &entry);
    drop(registration);
    match saved {
        Ok(failures) => after_finish(&app_handle, task.id, &run_id, failures),
        Err(error) => {
            log_task_message(&app_handle, task.id, &run_id, "error", format!("保存运行结果失败，结果已保留并在后台重试保存: {error}"));
            spawn_run_result_save_retry(&app_handle, entry);
        }
    }
    prune_hidden_run_conversations(&app_handle, task.id, &run_id);
}

enum RunError {
    /// 开始前的检查没通过（记为跳过）。
    Preflight(String),
    Failed(String),
}

async fn execute_run_inner(
    app_handle: &tauri::AppHandle,
    task: &ScheduledTask,
    run_id: &str,
    scheduled_for: DateTime<Utc>,
    cancel_token: CancellationToken,
) -> Result<ScheduledRunOutcome, RunError> {
    // ── 1. 开始前检查 ────────────────────────────────────────────────
    let assistant_detail = get_assistant(app_handle.clone(), task.assistant_id)
        .map_err(|e| RunError::Preflight(format!("执行助手 #{} 不存在或读取失败：{e}", task.assistant_id)))?;
    service::validate_executor_type(assistant_detail.assistant.assistant_type, &assistant_detail.assistant.name)
        .map_err(RunError::Preflight)?;
    if assistant_detail.model.is_empty() {
        return Err(RunError::Preflight(format!("执行助手「{}」没有配置模型", assistant_detail.assistant.name)));
    }
    let assistant_name = assistant_detail.assistant.name.clone();
    let system_prompt = assistant_detail
        .prompts
        .first()
        .map(|p| p.prompt.clone())
        .ok_or_else(|| RunError::Preflight(format!("执行助手「{assistant_name}」没有系统提示词")))?;
    crate::api::mobile_capability::ensure_assistant_execution(app_handle, task.assistant_id, "scheduled_run_preflight", "not_created")
        .await
        .map_err(RunError::Preflight)?;
    let llm_db = LLMDatabase::new(app_handle).map_err(|e| RunError::Failed(e.to_string()))?;
    let model_info = &assistant_detail.model[0];
    let model_detail = llm_db
        .get_model_for_request(&model_info.provider_id, &model_info.model_code)
        .map_err(|e| RunError::Preflight(format!("执行助手配置的模型不可用（{} / provider {}）：{e}", model_info.model_code, model_info.provider_id)))?;

    // ── 2. 提示词 ─────────────────────────────────────────────────────
    let template_engine = build_template_engine(app_handle).map_err(|e| RunError::Failed(e.to_string()))?;
    let mut template_context = HashMap::new();
    if !task.butler_mode {
        if let Some(state) = app_handle.try_state::<AppState>() {
            let selected_text = state.selected_text.lock().await.clone();
            template_context.insert("selected_text".to_string(), selected_text);
        }
    }
    let slash = if task.butler_mode {
        crate::slash::types::SlashParseResult {
            display_prompt: task.task_prompt.clone(),
            runtime_user_prompt: task.task_prompt.clone(),
            active_skills: Vec::new(),
        }
    } else {
        parse_slash_prompt(app_handle, &task.task_prompt).await.map_err(|e| RunError::Failed(e.to_string()))?
    };
    let system_prompt_rendered = if task.butler_mode {
        system_prompt
    } else {
        template_engine.parse(&system_prompt, &template_context).await
    };
    let task_prompt_rendered = if task.butler_mode {
        slash.runtime_user_prompt.clone()
    } else {
        template_engine.parse(&slash.runtime_user_prompt, &template_context).await
    };
    log_task_message(app_handle, task.id, run_id, "task_prompt", slash.display_prompt.clone());

    // ── 3. 工具与 Skills ─────────────────────────────────────────────
    let mut mcp_info = collect_mcp_info_for_assistant(app_handle, task.assistant_id, None, None)
        .await
        .map_err(|e| RunError::Failed(e.to_string()))?;
    crate::mcp::butler_tools::prepare_scheduled_run_tools(app_handle, &mut mcp_info.enabled_servers, task.butler_mode)
        .map_err(|e| RunError::Failed(e.to_string()))?;
    let mut assistant_prompt = if !mcp_info.enabled_servers.is_empty() && !mcp_info.use_native_toolcall {
        format_mcp_prompt(system_prompt_rendered.clone(), &mcp_info).await
    } else {
        system_prompt_rendered.clone()
    };
    let skills_info = if task.butler_mode {
        crate::skills::collect_all_skills_for_assistant(app_handle, task.assistant_id, &[]).await
    } else {
        collect_skills_info_for_assistant(app_handle, task.assistant_id).await
    }
    .map_err(|e| RunError::Failed(e.to_string()))?;
    if !skills_info.enabled_skills.is_empty() {
        assistant_prompt = format_skills_prompt(app_handle, assistant_prompt, &skills_info).await;
    }
    if task.butler_mode {
        // 管家模式的运行与管家会话共用交付约定；用户不在场、运行会话不可见，只保留文件交付。
        assistant_prompt.push_str("\n\n");
        assistant_prompt.push_str(&crate::api::butler_delivery::delivery_prompt(
            crate::api::butler_delivery::DeliveryTools::from_servers(&mcp_info.enabled_servers),
            true,
        ));
    }
    let deliver_target = {
        let names = service::view_names(app_handle, std::slice::from_ref(task)).unwrap_or_default();
        service::deliver_target_text(task, &names)
    };
    let (run_number, previous) = {
        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| RunError::Failed(e.to_string()))?;
        let count = db.read_task(task.id).ok().flatten().map(|t| t.run_count).unwrap_or(task.run_count).max(1);
        (count, previous_run_note(&db, task.id, run_id))
    };
    assistant_prompt.push_str(&unattended_contract(task, run_number, scheduled_for, Utc::now(), &deliver_target, previous.as_deref()));
    let task_prompt_with_skills = compose_user_message_with_active_skills(&task_prompt_rendered, &slash.active_skills);
    let active_skill_attachments = build_active_skill_attachments(&slash.active_skills).map_err(|e| RunError::Failed(e.to_string()))?;

    // ── 4. 模型客户端 ─────────────────────────────────────────────────
    let feature_config_state = app_handle.state::<FeatureConfigState>();
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
        .map_err(|e| RunError::Failed(format!("Copilot token exchange failed: {}", e)))?,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        Some(&model_detail.model.request_mode),
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
        false,
        &config_feature_map,
    )
    .map_err(|e| RunError::Failed(format!("创建模型客户端失败: {}", e)))?;
    log_task_message(app_handle, task.id, run_id, "assistant", format!("助手: {} / 模型: {}", assistant_name, model_detail.model.code));

    let model_config_clone = ConfigBuilder::merge_model_configs(assistant_detail.model_configs.clone(), &model_detail, None);
    let config_map: HashMap<String, String> = model_config_clone
        .iter()
        .filter_map(|config| config.value.as_ref().map(|value| (config.name.clone(), value.clone())))
        .collect();
    let model_name = config_map.get("model").cloned().unwrap_or_else(|| model_detail.model.code.clone());
    let provider_api_type_lc = model_detail.provider.api_type.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let has_available_tools = mcp_info.use_native_toolcall && !mcp_info.enabled_servers.is_empty();
    let capture_reasoning_content = is_openai_like && has_available_tools && config_map.contains_key("reasoning_effort");
    let chat_options = ConfigBuilder::build_chat_options(&config_map, None, None)
        .with_normalize_reasoning_content(true)
        .with_capture_reasoning_content(capture_reasoning_content)
        .with_capture_tool_calls(has_available_tools);
    let tool_call_strategy = if has_available_tools { ToolCallStrategy::Native } else { ToolCallStrategy::NonNative };
    let tool_config = build_tool_config_for_mcp(&mcp_info, has_available_tools);
    let tool_name_mapping = tool_config.as_ref().map(|c| c.tool_name_mapping.clone()).unwrap_or_default();

    // ── 5. 隐藏的运行会话 ─────────────────────────────────────────────
    let init_messages = vec![
        ("system".to_string(), assistant_prompt, Vec::new()),
        ("user".to_string(), task_prompt_with_skills, active_skill_attachments),
    ];
    let (mut conversation, _) = crate::api::ai::conversation::init_conversation(
        app_handle,
        task.assistant_id,
        model_detail.model.id,
        model_detail.model.code.clone(),
        &init_messages,
        None,
    )
    .map_err(|e| RunError::Failed(e.to_string()))?;
    let conversation_id = conversation.id;
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| RunError::Failed(e.to_string()))?;
    conversation.name = format!("定时任务：{} · {}", task.name, expr::format_local(scheduled_for));
    conversation.conversation_kind = crate::db::message_preview::SCHEDULED_RUN_KIND.to_string();
    conversation.is_hidden_from_normal_chat_list = true;
    conversation_db
        .conversation_repo()
        .map_err(|e| RunError::Failed(e.to_string()))?
        .update(&conversation)
        .map_err(|e| RunError::Failed(format!("登记运行会话失败: {e}")))?;
    ScheduledTaskDatabase::new(app_handle)
        .and_then(|db| db.set_run_conversation(run_id, conversation_id))
        .map_err(|e| RunError::Failed(format!("关联运行 {run_id} 与会话 {conversation_id} 失败：{e}")))?;

    // ── 6. 执行 ───────────────────────────────────────────────────────
    let gate_adapter = ScheduledToolGate {
        ctx: GateContext {
            app: app_handle,
            task,
            assistant_name: &assistant_name,
            run_id,
            conversation_id,
            cancel: &cancel_token,
        },
    };
    let observer = ScheduledLoopObserver { app_handle, task_id: task.id, run_id };
    let loop_ctx = UnattendedLoopContext {
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
        run_label: run_id,
        limits: LoopLimits {
            max_rounds: AGENTIC_LOOP_MAX_ROUNDS,
            per_tool_timeout_secs: AGENTIC_LOOP_PER_TOOL_TIMEOUT_SECS,
            total_timeout_secs: AGENTIC_LOOP_TOTAL_TIMEOUT_SECS,
            retry_delays_secs: &LLM_RETRY_DELAYS_SECS,
        },
        cancel_token: cancel_token.clone(),
        gate: &gate_adapter,
        observer: &observer,
    };
    let loop_result = run_unattended_loop(&loop_ctx).await;
    log_task_message(
        app_handle,
        task.id,
        run_id,
        "loop_done",
        format!(
            "status={}, rounds={}, tools={}/{}/{}",
            loop_result.status, loop_result.rounds, loop_result.tool_calls_success, loop_result.tool_calls_failed, loop_result.tool_calls_total
        ),
    );
    let final_text = loop_result.final_text.trim().to_string();
    if !final_text.is_empty() {
        log_task_message(app_handle, task.id, run_id, "response", final_text.clone());
    }
    Ok(loop_result.into_outcome(conversation_id, model_detail.model.id, model_detail.model.code.clone(), &task.deliver_policy))
}

impl AgenticLoopResult {
    fn into_outcome(self, conversation_id: i64, model_id: i64, model_name: String, policy: &str) -> ScheduledRunOutcome {
        let final_text = self.final_text.trim().to_string();
        let final_message_id = match self.status {
            AgenticLoopStatus::Completed { message_id } => Some(message_id),
            _ => None,
        };
        let base = ScheduledRunOutcome {
            conversation_id: Some(conversation_id),
            final_message_id,
            llm_model_id: Some(model_id),
            llm_model_name: Some(model_name),
            ..ScheduledRunOutcome::failed("error", String::new(), None)
        };
        if let Some((end_reason, error)) = loop_status_failure(
            &self.status,
            self.rounds,
            self.tool_calls_success,
            self.tool_calls_failed,
        ) {
            let status = if end_reason == "cancelled" { RUN_CANCELLED } else { RUN_FAILED };
            return ScheduledRunOutcome {
                status: status.to_string(),
                end_reason: end_reason.to_string(),
                text: (!final_text.is_empty()).then_some(final_text),
                error: Some(error),
                ..base
            };
        }
        let (status, end_reason) = classify_completion(&final_text, policy);
        ScheduledRunOutcome {
            status: status.to_string(),
            end_reason: end_reason.to_string(),
            error: (status == RUN_FAILED).then(|| "执行完成，但没有产出任何内容".to_string()),
            text: (!final_text.is_empty()).then_some(final_text),
            ..base
        }
    }
}

const RUN_RESULT_SAVE_RETRY_DELAY: Duration = Duration::from_secs(5);

fn save_retries_in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static IN_FLIGHT: OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// 结果保存失败后在后台重试；同一运行只有一个重试流程。
fn spawn_run_result_save_retry(app_handle: &tauri::AppHandle, entry: ScheduledOutcomeJournal) {
    if !save_retries_in_flight().lock().unwrap_or_else(|e| e.into_inner()).insert(entry.run_id.clone()) {
        return;
    }
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(RUN_RESULT_SAVE_RETRY_DELAY).await;
            match ScheduledTaskDatabase::new(&app_handle).and_then(|db| {
                Ok(db.read_task(entry.task_id)?.is_some() && db.read_run(&entry.run_id)?.is_some())
            }) {
                Ok(false) => {
                    remove_outcome_journal(&app_handle, &entry.run_id);
                    break;
                }
                Err(error) => {
                    warn!(run_id = %entry.run_id, error = %error, "cannot check outcome ownership; retrying");
                    continue;
                }
                Ok(true) => {}
            }
            match persist_scheduled_outcome(&app_handle, &entry) {
                Ok(failures) => {
                    log_task_message(&app_handle, entry.task_id, &entry.run_id, "result_saved", "完整运行结果已保存");
                    after_finish(&app_handle, entry.task_id, &entry.run_id, failures);
                    break;
                }
                Err(error) => warn!(run_id = %entry.run_id, error = %error, "retry saving scheduled run result failed"),
            }
        }
        save_retries_in_flight().lock().unwrap_or_else(|e| e.into_inner()).remove(&entry.run_id);
    });
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

/// Replay the write-ahead outbox before deciding which old runs were interrupted.
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
                warn!(run_id = %id, error = %error, "cannot recover outcome journal; retained for repair");
                continue;
            }
        };
        processed.push(id.clone());
        if db.read_task(entry.task_id).map_err(|e| e.to_string())?.is_none() {
            remove_outcome_journal(app_handle, &id);
            continue;
        }
        match persist_scheduled_outcome(app_handle, &entry) {
            Ok(failures) => after_finish(app_handle, entry.task_id, &entry.run_id, failures),
            Err(error) => {
                warn!(run_id = %id, error = %error, "replaying outcome journal failed; retrying in background");
                spawn_run_result_save_retry(app_handle, entry);
            }
        }
    }
    Ok(protected)
}

/// 启动时核对上一个进程已启动但未结束的运行：如实标记为中断（结果未知，不自动重跑）。
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
        .mark_interrupted_runs(&local_run_ids, "应用在任务执行过程中退出，运行被中断，执行结果未知；未自动重新执行", Utc::now())
        .map_err(|e| format!("收尾中断的运行记录失败: {e}"))?;
    for run in &interrupted {
        log_task_message(app_handle, run.task_id, &run.run_id, "interrupted", run.error_message.clone().unwrap_or_default());
        let _ = app_handle.emit(SCHEDULED_TASK_RUN_UPDATED_EVENT, ScheduledTaskRunUpdateEvent {
            task_id: run.task_id,
            run_id: run.run_id.clone(),
            status: run.status.clone(),
        });
    }
    Ok(interrupted.len())
}

// ── 界面命令 ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_scheduled_tasks(app_handle: tauri::AppHandle) -> Result<Vec<TaskView>, String> {
    service::list(&app_handle)
}

#[tauri::command]
pub async fn get_scheduled_task(app_handle: tauri::AppHandle, task_id: i64) -> Result<TaskView, String> {
    service::get(&app_handle, task_id)
}

#[tauri::command]
pub async fn preview_schedule(schedule: String) -> Result<SchedulePreview, String> {
    service::preview_schedule(&schedule, Utc::now())
}

/// 界面创建：投递位置由用户选择，默认发到执行助手的管家私聊。
#[tauri::command]
pub async fn create_scheduled_task(app_handle: tauri::AppHandle, input: TaskInput) -> Result<TaskView, String> {
    let deliver_to = input.deliver_to.clone().unwrap_or_else(|| crate::db::scheduled_task_db::DELIVER_OWNER_CHAT.to_string());
    let defaults = TaskDefaults {
        created_in_conversation_id: None,
        butler_mode: deliver_to == crate::db::scheduled_task_db::DELIVER_OWNER_CHAT,
        deliver_to,
        deliver_conversation_id: input.deliver_conversation_id,
    };
    service::create(&app_handle, &input, &defaults)
}

#[tauri::command]
pub async fn update_scheduled_task(app_handle: tauri::AppHandle, task_id: i64, patch: TaskPatch) -> Result<TaskView, String> {
    service::update(&app_handle, task_id, &patch)
}

#[tauri::command]
pub async fn set_scheduled_task_enabled(app_handle: tauri::AppHandle, task_id: i64, enabled: bool) -> Result<TaskView, String> {
    service::set_enabled(&app_handle, task_id, enabled)
}

#[tauri::command]
pub async fn delete_scheduled_task(app_handle: tauri::AppHandle, task_id: i64) -> Result<(), String> {
    service::delete(&app_handle, task_id)
}

/// 立即开始一次运行（不改变计划时间），返回运行编号；结果按任务设置投递。
#[tauri::command]
pub async fn run_scheduled_task_now(app_handle: tauri::AppHandle, task_id: i64) -> Result<String, String> {
    crate::scheduler::run_now(&app_handle, task_id).await
}

#[tauri::command]
pub async fn stop_scheduled_task_run(app_handle: tauri::AppHandle, task_id: i64, run_id: Option<String>) -> Result<bool, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let target = db
        .list_runs_by_task(task_id, 50)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|run| run.status == "running" && run_id.as_ref().is_none_or(|id| id == &run.run_id))
        .ok_or_else(|| "当前任务没有正在运行的执行记录".to_string())?;
    if !cancel_scheduled_run(&target.run_id) {
        return Err("这次运行不在本机执行或已经结束，无法停止".to_string());
    }
    log_task_message(&app_handle, task_id, &target.run_id, "cancel_request", "收到停止请求，正在终止执行");
    Ok(true)
}

#[tauri::command]
pub async fn list_scheduled_task_runs(app_handle: tauri::AppHandle, task_id: i64, limit: Option<u32>) -> Result<Vec<RunView>, String> {
    service::list_runs(&app_handle, task_id, limit.unwrap_or(30).clamp(1, 200))
}

#[tauri::command]
pub async fn list_scheduled_task_logs(
    app_handle: tauri::AppHandle,
    task_id: i64,
    run_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ScheduledTaskLogDTO>, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(500).clamp(1, 2000);
    let logs = match run_id {
        Some(run_id) => db.list_logs_by_run(task_id, &run_id, limit),
        None => db.list_logs_by_task(task_id, limit),
    }
    .map_err(|e| e.to_string())?;
    Ok(logs.into_iter().map(log_to_dto).collect())
}

#[cfg(test)]
#[path = "scheduled_task_journal_tests.rs"]
mod journal_tests;

#[cfg(test)]
#[path = "tests/scheduled_delivery_flow_tests.rs"]
mod scheduled_delivery_flow_tests;

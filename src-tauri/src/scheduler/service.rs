//! 定时任务服务层：界面命令、模型工具（`schedule_task`）与 SuperAdmin 统一走这里。
//! 校验、下一次时间计算与展示视图只在这里实现一份。
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::scheduled_task_db::{
    ScheduledTask, ScheduledTaskDatabase, ScheduledTaskRun, DELIVER_CONVERSATION, DELIVER_OWNER_CHAT,
    DELIVER_RUN_CONVERSATION, RUN_CANCELLED, RUN_DELIVERED, RUN_FAILED, RUN_LEGACY_SUCCESS, RUN_MISSED,
    RUN_QUIET, RUN_RUNNING, RUN_SKIPPED,
};
use crate::scheduler::expr::{self, Schedule};

pub const DELIVER_ALWAYS: &str = "always";
pub const DELIVER_IF_NEEDED: &str = "if_needed";
pub const DELIVER_NEVER: &str = "never";
pub const MISSED_RUN_ONCE: &str = "run_once";
pub const MISSED_SKIP: &str = "skip";
/// 任务名称与指令的长度上限（字符）。
const NAME_LIMIT: usize = 100;
const UPCOMING_COUNT: usize = 3;

/// 创建任务的输入。缺省字段由调用方按场景补齐（见 [`TaskDefaults`]）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    pub assistant_id: i64,
    pub deliver_policy: Option<String>,
    pub deliver_to: Option<String>,
    pub deliver_conversation_id: Option<i64>,
    pub ends_at: Option<String>,
    pub max_runs: Option<i64>,
    pub missed_policy: Option<String>,
    pub enabled: Option<bool>,
}

/// 修改任务：只改出现的字段。`ends_at`/`max_runs` 传空字符串或 0 表示清除。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    pub name: Option<String>,
    pub schedule: Option<String>,
    pub prompt: Option<String>,
    pub assistant_id: Option<i64>,
    pub deliver_policy: Option<String>,
    pub deliver_to: Option<String>,
    pub deliver_conversation_id: Option<i64>,
    pub ends_at: Option<String>,
    pub max_runs: Option<i64>,
    pub missed_policy: Option<String>,
    pub enabled: Option<bool>,
}

/// 创建场景决定的默认值。
#[derive(Debug, Clone, Default)]
pub struct TaskDefaults {
    pub created_in_conversation_id: Option<i64>,
    pub butler_mode: bool,
    pub deliver_to: String,
    pub deliver_conversation_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run_id: String,
    pub status: String,
    pub status_text: String,
    pub reason: Option<String>,
    pub summary: Option<String>,
    pub scheduled_for: Option<String>,
    pub started_time: String,
    pub finished_time: Option<String>,
    pub conversation_id: Option<i64>,
    pub final_message_id: Option<i64>,
    pub delivered_message_id: Option<i64>,
    pub delivery_conversation_id: Option<i64>,
    pub delivery_error: Option<String>,
    /// 本进程持有这次运行的停止令牌。
    pub local_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub schedule: String,
    pub schedule_text: String,
    pub upcoming: Vec<String>,
    pub upcoming_text: Vec<String>,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub assistant_id: i64,
    pub assistant_name: String,
    pub prompt: String,
    pub deliver_policy: String,
    pub deliver_to: String,
    pub deliver_conversation_id: Option<i64>,
    pub deliver_target_text: String,
    pub created_in_conversation_id: Option<i64>,
    pub butler_mode: bool,
    pub ends_at: Option<String>,
    pub max_runs: Option<i64>,
    pub run_count: i64,
    pub missed_policy: String,
    pub consecutive_failures: i64,
    pub paused_reason: Option<String>,
    pub created_time: String,
    pub updated_time: String,
    pub last_run: Option<RunView>,
    /// 需要用户处理：自动暂停、连续失败、或迁移后需要确认时间。
    pub needs_attention: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePreview {
    pub schedule: String,
    pub schedule_text: String,
    pub upcoming: Vec<String>,
    pub upcoming_text: Vec<String>,
}

fn non_empty(value: &str, label: &str, limit: Option<usize>) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label}不能为空"));
    }
    if let Some(limit) = limit {
        if value.chars().count() > limit {
            return Err(format!("{label}不能超过 {limit} 个字"));
        }
    }
    Ok(value.to_string())
}

fn validate_choice(value: &str, allowed: &[&str], label: &str) -> Result<String, String> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!("{label}只能是 {}，收到「{value}」", allowed.join(" / ")))
    }
}

fn parse_ends_at(value: &str, now: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    match expr::parse(value, now)? {
        Schedule::Once(at) if at > now => Ok(Some(at)),
        Schedule::Once(at) => Err(format!("结束时间 {} 已经过去", expr::format_local(at))),
        _ => Err(format!("结束时间应为具体时间，例如 2026-12-31 23:59，收到「{value}」")),
    }
}

/// 解析并规范化时间；一次性时间必须在将来。返回（规范表达，计划，锚点）。
pub fn normalize_schedule(text: &str, now: DateTime<Utc>) -> Result<(String, Schedule), String> {
    let schedule = expr::parse(text, now)?;
    if let Schedule::Once(at) = &schedule {
        if *at <= now {
            return Err(format!("一次性时间 {} 已经过去，请给一个将来的时间", expr::format_local(*at)));
        }
    }
    Ok((schedule.to_expr(), schedule))
}

/// 在 `after` 之后的下一次运行，考虑结束时间与最多次数。
pub fn next_run(task: &ScheduledTask, schedule: &Schedule, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if task.max_runs.is_some_and(|max| task.run_count >= max) {
        return None;
    }
    let next = schedule.next_after(after, task.schedule_anchor)?;
    match task.ends_at {
        Some(ends_at) if next > ends_at => None,
        _ => Some(next),
    }
}

pub fn preview_schedule(text: &str, now: DateTime<Utc>) -> Result<SchedulePreview, String> {
    let (expr_text, schedule) = normalize_schedule(text, now)?;
    let upcoming = schedule.upcoming(now, now, UPCOMING_COUNT);
    Ok(SchedulePreview {
        schedule: expr_text,
        schedule_text: schedule.describe(now),
        upcoming: upcoming.iter().map(|t| t.to_rfc3339()).collect(),
        upcoming_text: upcoming.into_iter().map(expr::format_local).collect(),
    })
}

/// 校验输入并构造新任务（不写库）。
pub fn build_task(input: &TaskInput, defaults: &TaskDefaults, now: DateTime<Utc>) -> Result<ScheduledTask, String> {
    let name = non_empty(&input.name, "任务名称", Some(NAME_LIMIT))?;
    let prompt = non_empty(&input.prompt, "任务指令", None)?;
    let (schedule_expr, schedule) = normalize_schedule(&input.schedule, now)?;
    if input.assistant_id <= 0 {
        return Err("缺少执行助手".to_string());
    }
    let deliver_policy = validate_choice(
        input.deliver_policy.as_deref().unwrap_or(DELIVER_ALWAYS),
        &[DELIVER_ALWAYS, DELIVER_IF_NEEDED, DELIVER_NEVER],
        "投递策略 deliver",
    )?;
    let deliver_to = validate_choice(
        input.deliver_to.as_deref().unwrap_or(&defaults.deliver_to),
        &[DELIVER_OWNER_CHAT, DELIVER_CONVERSATION, DELIVER_RUN_CONVERSATION],
        "投递位置 deliver_to",
    )?;
    let deliver_conversation_id = if deliver_to == DELIVER_CONVERSATION {
        Some(input.deliver_conversation_id.or(defaults.deliver_conversation_id).ok_or("投递到会话时需要 deliver_conversation_id")?)
    } else {
        None
    };
    let missed_policy = validate_choice(
        input.missed_policy.as_deref().unwrap_or(MISSED_RUN_ONCE),
        &[MISSED_RUN_ONCE, MISSED_SKIP],
        "错过策略 missed",
    )?;
    let ends_at = parse_ends_at(input.ends_at.as_deref().unwrap_or(""), now)?;
    let max_runs = match input.max_runs {
        Some(n) if n < 1 => return Err("max_runs 至少为 1".to_string()),
        other => other,
    };
    let mut task = ScheduledTask {
        id: 0,
        name,
        is_enabled: input.enabled.unwrap_or(true),
        schedule_expr,
        schedule_anchor: now,
        next_run_at: None,
        last_run_at: None,
        assistant_id: input.assistant_id,
        task_prompt: prompt,
        deliver_policy,
        deliver_to,
        deliver_conversation_id,
        created_in_conversation_id: defaults.created_in_conversation_id,
        butler_mode: defaults.butler_mode,
        ends_at,
        max_runs,
        run_count: 0,
        missed_policy,
        consecutive_failures: 0,
        paused_reason: None,
        created_time: now,
        updated_time: now,
    };
    task.next_run_at = next_run(&task, &schedule, now);
    if task.next_run_at.is_none() {
        return Err("按这个时间设置，结束前不会再运行，请检查时间与结束条件".to_string());
    }
    Ok(task)
}

/// 应用修改（不写库）。时间或启用状态变化时从现在重新计算下一次，恢复时不补跑。
pub fn apply_patch(mut task: ScheduledTask, patch: &TaskPatch, now: DateTime<Utc>) -> Result<ScheduledTask, String> {
    if let Some(name) = &patch.name {
        task.name = non_empty(name, "任务名称", Some(NAME_LIMIT))?;
    }
    if let Some(prompt) = &patch.prompt {
        task.task_prompt = non_empty(prompt, "任务指令", None)?;
    }
    if let Some(assistant_id) = patch.assistant_id {
        if assistant_id <= 0 {
            return Err("缺少执行助手".to_string());
        }
        task.assistant_id = assistant_id;
    }
    if let Some(policy) = &patch.deliver_policy {
        task.deliver_policy = validate_choice(policy, &[DELIVER_ALWAYS, DELIVER_IF_NEEDED, DELIVER_NEVER], "投递策略 deliver")?;
    }
    if let Some(deliver_to) = &patch.deliver_to {
        task.deliver_to = validate_choice(
            deliver_to,
            &[DELIVER_OWNER_CHAT, DELIVER_CONVERSATION, DELIVER_RUN_CONVERSATION],
            "投递位置 deliver_to",
        )?;
    }
    if let Some(id) = patch.deliver_conversation_id {
        task.deliver_conversation_id = Some(id);
    }
    if task.deliver_to == DELIVER_CONVERSATION && task.deliver_conversation_id.is_none() {
        return Err("投递到会话时需要 deliver_conversation_id".to_string());
    }
    if task.deliver_to != DELIVER_CONVERSATION {
        task.deliver_conversation_id = None;
    }
    if let Some(policy) = &patch.missed_policy {
        task.missed_policy = validate_choice(policy, &[MISSED_RUN_ONCE, MISSED_SKIP], "错过策略 missed")?;
    }
    if let Some(ends_at) = &patch.ends_at {
        task.ends_at = parse_ends_at(ends_at, now)?;
    }
    if let Some(max_runs) = patch.max_runs {
        task.max_runs = (max_runs > 0).then_some(max_runs);
    }
    let schedule_changed = match &patch.schedule {
        Some(text) => {
            let (expr_text, _) = normalize_schedule(text, now)?;
            let changed = expr_text != task.schedule_expr;
            if changed {
                task.schedule_expr = expr_text;
                task.schedule_anchor = now;
            }
            changed
        }
        None => false,
    };
    let enabling = patch.enabled == Some(true) && !task.is_enabled;
    if let Some(enabled) = patch.enabled {
        task.is_enabled = enabled;
    }
    if enabling {
        task.paused_reason = None;
        task.consecutive_failures = 0;
    }
    let schedule = task.schedule()?;
    if schedule_changed || enabling || patch.ends_at.is_some() || patch.max_runs.is_some() || task.next_run_at.is_none() {
        task.next_run_at = next_run(&task, &schedule, now);
    }
    if task.is_enabled && task.next_run_at.is_none() {
        return Err("按这个时间设置，结束前不会再运行；请修改时间或结束条件后再启用".to_string());
    }
    task.updated_time = now;
    Ok(task)
}

pub fn status_text(status: &str, notify: bool) -> &'static str {
    match status {
        RUN_RUNNING => "运行中",
        RUN_DELIVERED => "已发送",
        RUN_QUIET => "无新内容",
        RUN_FAILED => "失败",
        RUN_SKIPPED => "跳过",
        RUN_MISSED => "错过",
        RUN_CANCELLED => "已停止",
        RUN_LEGACY_SUCCESS if notify => "已发送",
        RUN_LEGACY_SUCCESS => "无新内容",
        _ => "未知",
    }
}

pub fn run_view(run: ScheduledTaskRun, local_running: bool) -> RunView {
    RunView {
        status_text: status_text(&run.status, run.notify).to_string(),
        local_running: local_running && run.status == RUN_RUNNING,
        run_id: run.run_id,
        status: run.status,
        reason: run.error_message,
        summary: run.summary,
        scheduled_for: run.scheduled_for.map(|t| t.to_rfc3339()),
        started_time: run.started_time.to_rfc3339(),
        finished_time: run.finished_time.map(|t| t.to_rfc3339()),
        conversation_id: run.conversation_id,
        final_message_id: run.final_message_id,
        delivered_message_id: run.delivered_message_id,
        delivery_conversation_id: run.delivery_conversation_id,
        delivery_error: run.delivery_error,
    }
}

/// 视图需要的外部名称（助手名、投递会话名）。
#[derive(Debug, Default, Clone)]
pub struct ViewNames {
    pub assistants: HashMap<i64, String>,
    pub conversations: HashMap<i64, String>,
    pub butler_assistant_id: Option<i64>,
}

pub fn deliver_target_text(task: &ScheduledTask, names: &ViewNames) -> String {
    let assistant = names.assistants.get(&task.assistant_id).cloned().unwrap_or_else(|| format!("助手 #{}", task.assistant_id));
    match task.deliver_to.as_str() {
        DELIVER_OWNER_CHAT if names.butler_assistant_id == Some(task.assistant_id) => "总管家主会话".to_string(),
        DELIVER_OWNER_CHAT => format!("与{assistant}的私聊"),
        DELIVER_CONVERSATION => task
            .deliver_conversation_id
            .map(|id| names.conversations.get(&id).cloned().unwrap_or_else(|| format!("会话 #{id}")))
            .map(|name| format!("会话「{name}」"))
            .unwrap_or_else(|| "未指定会话".to_string()),
        _ => "单独的会话（出现在聊天列表）".to_string(),
    }
}

pub fn task_view(task: ScheduledTask, last_run: Option<ScheduledTaskRun>, names: &ViewNames, now: DateTime<Utc>, local_running: &dyn Fn(&str) -> bool) -> TaskView {
    let schedule = task.schedule();
    let (schedule_text, upcoming) = match &schedule {
        Ok(schedule) => {
            let upcoming = if task.is_enabled {
                let mut times = Vec::new();
                let mut cursor = task.next_run_at;
                while let Some(at) = cursor {
                    if times.len() >= UPCOMING_COUNT {
                        break;
                    }
                    times.push(at);
                    let probe = ScheduledTask { run_count: task.run_count + times.len() as i64, ..task.clone() };
                    cursor = next_run(&probe, schedule, at);
                }
                times
            } else {
                Vec::new()
            };
            (schedule.describe(task.schedule_anchor), upcoming)
        }
        Err(error) => (format!("时间设置无效：{error}"), Vec::new()),
    };
    let _ = now;
    let deliver_target_text = deliver_target_text(&task, names);
    let needs_attention = task.consecutive_failures >= 2
        || (!task.is_enabled && task.paused_reason.as_deref().is_some_and(|r| r != "已完成" && r != "已手动暂停"));
    TaskView {
        id: task.id,
        assistant_name: names.assistants.get(&task.assistant_id).cloned().unwrap_or_else(|| format!("助手 #{}（已删除）", task.assistant_id)),
        name: task.name,
        enabled: task.is_enabled,
        schedule: task.schedule_expr,
        schedule_text,
        upcoming: upcoming.iter().map(|t| t.to_rfc3339()).collect(),
        upcoming_text: upcoming.into_iter().map(expr::format_local).collect(),
        next_run_at: task.next_run_at.map(|t| t.to_rfc3339()),
        last_run_at: task.last_run_at.map(|t| t.to_rfc3339()),
        assistant_id: task.assistant_id,
        prompt: task.task_prompt,
        deliver_policy: task.deliver_policy,
        deliver_to: task.deliver_to,
        deliver_conversation_id: task.deliver_conversation_id,
        deliver_target_text,
        created_in_conversation_id: task.created_in_conversation_id,
        butler_mode: task.butler_mode,
        ends_at: task.ends_at.map(|t| t.to_rfc3339()),
        max_runs: task.max_runs,
        run_count: task.run_count,
        missed_policy: task.missed_policy,
        consecutive_failures: task.consecutive_failures,
        paused_reason: task.paused_reason,
        created_time: task.created_time.to_rfc3339(),
        updated_time: task.updated_time.to_rfc3339(),
        last_run: last_run.map(|run| {
            let local = local_running(&run.run_id);
            run_view(run, local)
        }),
        needs_attention,
    }
}

// ── 需要应用状态的操作 ──────────────────────────────────────────────────

fn assistant_names(app: &tauri::AppHandle) -> Result<HashMap<i64, String>, String> {
    Ok(AssistantDatabase::new(app)
        .map_err(|e| e.to_string())?
        .get_assistants()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|a| (a.id, a.name))
        .collect())
}

pub fn view_names(app: &tauri::AppHandle, tasks: &[ScheduledTask]) -> Result<ViewNames, String> {
    let mut conversations = HashMap::new();
    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    let repo = db.conversation_repo().map_err(|e| e.to_string())?;
    for id in tasks.iter().filter_map(|t| t.deliver_conversation_id) {
        if let Ok(Some(conversation)) = repo.read(id) {
            conversations.insert(id, conversation.name);
        }
    }
    let butler_assistant_id = crate::api::butler_api::active_butler_main_conversation_id(app)
        .ok()
        .flatten()
        .and_then(|id| repo.read(id).ok().flatten())
        .and_then(|c| c.assistant_id);
    Ok(ViewNames { assistants: assistant_names(app)?, conversations, butler_assistant_id })
}

/// 执行助手必须存在且是普通对话助手（多模型对比、工作流、ACP 等助手不能无人值守运行）。
pub fn validate_assistant(app: &tauri::AppHandle, assistant_id: i64) -> Result<String, String> {
    let assistant = AssistantDatabase::new(app)
        .map_err(|e| e.to_string())?
        .get_assistant(assistant_id)
        .map_err(|_| format!("执行助手 #{assistant_id} 不存在"))?;
    if assistant.assistant_type.unwrap_or(0) != 0 {
        return Err(format!("「{}」不是普通对话助手，不能执行定时任务", assistant.name));
    }
    Ok(assistant.name)
}

/// 按名称或编号找助手（模型常只知道名字）。
pub fn resolve_assistant(app: &tauri::AppHandle, value: &serde_json::Value) -> Result<i64, String> {
    if let Some(id) = value.as_i64() {
        return Ok(id);
    }
    let name = value.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("assistant 应为助手名称或编号")?;
    if let Ok(id) = name.trim_start_matches('#').parse::<i64>() {
        return Ok(id);
    }
    let assistants = AssistantDatabase::new(app).map_err(|e| e.to_string())?.get_assistants().map_err(|e| e.to_string())?;
    let matches: Vec<_> = assistants.iter().filter(|a| a.name == name).collect();
    match matches.as_slice() {
        [one] => Ok(one.id),
        [] => Err(format!("找不到名为「{name}」的助手；可用 superadmin 的 assistant.list 查看助手列表")),
        _ => Err(format!("有多个名为「{name}」的助手，请改用助手编号")),
    }
}

fn local_running(run_id: &str) -> bool {
    crate::api::scheduled_task_api::is_run_local(run_id)
}

pub fn list(app: &tauri::AppHandle) -> Result<Vec<TaskView>, String> {
    let db = ScheduledTaskDatabase::new(app).map_err(|e| e.to_string())?;
    let tasks = db.list_tasks().map_err(|e| e.to_string())?;
    let mut last_runs = db.latest_runs().map_err(|e| e.to_string())?;
    let names = view_names(app, &tasks)?;
    let now = Utc::now();
    Ok(tasks
        .into_iter()
        .map(|task| {
            let last = last_runs.remove(&task.id);
            task_view(task, last, &names, now, &local_running)
        })
        .collect())
}

pub fn get(app: &tauri::AppHandle, id: i64) -> Result<TaskView, String> {
    let db = ScheduledTaskDatabase::new(app).map_err(|e| e.to_string())?;
    let task = db.read_task(id).map_err(|e| e.to_string())?.ok_or_else(|| format!("定时任务 #{id} 不存在"))?;
    let last = db.list_runs_by_task(id, 1).map_err(|e| e.to_string())?.into_iter().next();
    let names = view_names(app, std::slice::from_ref(&task))?;
    Ok(task_view(task, last, &names, Utc::now(), &local_running))
}

pub fn list_runs(app: &tauri::AppHandle, id: i64, limit: u32) -> Result<Vec<RunView>, String> {
    let db = ScheduledTaskDatabase::new(app).map_err(|e| e.to_string())?;
    Ok(db
        .list_runs_by_task(id, limit)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|run| {
            let local = local_running(&run.run_id);
            run_view(run, local)
        })
        .collect())
}

fn emit_changed(app: &tauri::AppHandle, task_id: i64) {
    use tauri::Emitter;
    let _ = app.emit(crate::api::scheduled_task_api::SCHEDULED_TASK_CHANGED_EVENT, serde_json::json!({ "taskId": task_id }));
}

pub fn create(app: &tauri::AppHandle, input: &TaskInput, defaults: &TaskDefaults) -> Result<TaskView, String> {
    validate_assistant(app, input.assistant_id)?;
    if input.deliver_to.as_deref().unwrap_or(&defaults.deliver_to) == DELIVER_CONVERSATION {
        let id = input.deliver_conversation_id.or(defaults.deliver_conversation_id).ok_or("投递到会话时需要 deliver_conversation_id")?;
        ensure_conversation_exists(app, id)?;
    }
    let task = build_task(input, defaults, Utc::now())?;
    let created = ScheduledTaskDatabase::new(app)
        .map_err(|e| e.to_string())?
        .create_task(&task)
        .map_err(|e| e.to_string())?;
    emit_changed(app, created.id);
    get(app, created.id)
}

fn ensure_conversation_exists(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    db.conversation_repo()
        .map_err(|e| e.to_string())?
        .read(id)
        .map_err(|e| e.to_string())?
        .map(|_| ())
        .ok_or_else(|| format!("投递会话 #{id} 不存在"))
}

pub fn update(app: &tauri::AppHandle, id: i64, patch: &TaskPatch) -> Result<TaskView, String> {
    let db = ScheduledTaskDatabase::new(app).map_err(|e| e.to_string())?;
    let task = db.read_task(id).map_err(|e| e.to_string())?.ok_or_else(|| format!("定时任务 #{id} 不存在"))?;
    if let Some(assistant_id) = patch.assistant_id {
        validate_assistant(app, assistant_id)?;
    }
    if let Some(conversation_id) = patch.deliver_conversation_id {
        ensure_conversation_exists(app, conversation_id)?;
    }
    let mut updated = apply_patch(task, patch, Utc::now())?;
    if patch.enabled == Some(false) && updated.paused_reason.is_none() {
        updated.paused_reason = Some("已手动暂停".to_string());
    }
    db.update_task(&updated).map_err(|e| e.to_string())?;
    emit_changed(app, id);
    get(app, id)
}

pub fn set_enabled(app: &tauri::AppHandle, id: i64, enabled: bool) -> Result<TaskView, String> {
    update(app, id, &TaskPatch { enabled: Some(enabled), ..Default::default() })
}

pub fn delete(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    let db = ScheduledTaskDatabase::new(app).map_err(|e| e.to_string())?;
    db.read_task(id).map_err(|e| e.to_string())?.ok_or_else(|| format!("定时任务 #{id} 不存在"))?;
    db.delete_task(id).map_err(|e| e.to_string())?;
    emit_changed(app, id);
    Ok(())
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

//! SuperAdmin 的 `schedule.*` 动作。与界面、`schedule_task` 工具走同一服务层（`scheduler::service`），
//! 校验与下一次时间的计算只有一份。SuperAdmin 只在管家会话可用，创建的任务按管家模式投递到执行助手的私聊。
use async_trait::async_trait;
use serde_json::json;
use tauri::AppHandle;

use crate::db::scheduled_task_db::{ScheduledTask, ScheduledTaskDatabase, DELIVER_OWNER_CHAT};
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;
use crate::scheduler::service::{self, TaskDefaults, TaskInput, TaskPatch};
use crate::scheduler::tool::task_card;

struct ScheduleListHandler;
struct ScheduleGetHandler;
struct ScheduleCreateHandler;
struct ScheduleUpdateHandler;
struct ScheduleRunNowHandler;
struct ScheduleDeleteHandler;

fn task_id(args: &serde_json::Value) -> Result<i64, String> {
    args.get("task_id").and_then(|v| v.as_i64()).ok_or_else(|| "Missing required parameter: task_id".to_string())
}

fn string_arg(args: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| args.get(*key).and_then(|v| v.as_str())).map(str::to_string)
}

fn input_from(app: &AppHandle, args: &serde_json::Value) -> Result<TaskInput, String> {
    let assistant = args.get("assistant").or_else(|| args.get("assistant_id")).ok_or("Missing required parameter: assistant")?;
    Ok(TaskInput {
        name: string_arg(args, &["name"]).unwrap_or_default(),
        schedule: string_arg(args, &["schedule"]).unwrap_or_default(),
        prompt: string_arg(args, &["prompt", "task_prompt"]).unwrap_or_default(),
        assistant_id: service::resolve_assistant(app, assistant)?,
        deliver_policy: string_arg(args, &["deliver"]),
        deliver_to: None,
        deliver_conversation_id: None,
        ends_at: string_arg(args, &["ends_at"]),
        max_runs: args.get("max_runs").and_then(|v| v.as_i64()),
        missed_policy: string_arg(args, &["missed"]),
        enabled: Some(true),
    })
}

fn butler_defaults() -> TaskDefaults {
    TaskDefaults { created_in_conversation_id: None, butler_mode: true, deliver_to: DELIVER_OWNER_CHAT.to_string(), deliver_conversation_id: None }
}

#[async_trait]
impl ActionHandler for ScheduleListHandler {
    async fn execute(&self, app_handle: &AppHandle, _args: serde_json::Value, _dry_run: bool) -> Result<serde_json::Value, String> {
        let items: Vec<serde_json::Value> = service::list(app_handle)?.iter().map(task_card).collect();
        Ok(json!({ "count": items.len(), "tasks": items }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleGetHandler {
    async fn execute(&self, app_handle: &AppHandle, args: serde_json::Value, _dry_run: bool) -> Result<serde_json::Value, String> {
        let id = task_id(&args)?;
        let view = service::get(app_handle, id)?;
        let mut card = task_card(&view);
        card["prompt"] = json!(view.prompt);
        card["recent_runs"] = json!(service::list_runs(app_handle, id, 5)?);
        Ok(card)
    }
}

#[async_trait]
impl ActionHandler for ScheduleCreateHandler {
    async fn execute(&self, app_handle: &AppHandle, args: serde_json::Value, dry_run: bool) -> Result<serde_json::Value, String> {
        let input = input_from(app_handle, &args)?;
        if dry_run {
            let task = service::build_task(&input, &butler_defaults(), chrono::Utc::now())?;
            let preview = service::preview_schedule(&task.schedule_expr, chrono::Utc::now())?;
            return Ok(json!({ "dry_run": true, "would_create": { "name": task.name, "schedule_text": preview.schedule_text, "next_runs": preview.upcoming_text } }));
        }
        service::validate_assistant(app_handle, input.assistant_id)?;
        let view = service::create(app_handle, &input, &butler_defaults())?;
        Ok(json!({ "task_id": view.id, "created": task_card(&view) }))
    }

    async fn snapshot_before(&self, _app_handle: &AppHandle, _args: &serde_json::Value) -> Option<serde_json::Value> {
        Some(json!({ "_type": "schedule.create", "existed": false }))
    }

    async fn undo(&self, app_handle: &AppHandle, _snapshot: &serde_json::Value, original_args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let name = string_arg(original_args, &["name"]).ok_or("缺少任务名称，无法定位要撤销的任务")?;
        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let task = db
            .list_tasks()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|task| task.name == name)
            .ok_or_else(|| format!("找不到名为「{name}」的定时任务"))?;
        service::delete(app_handle, task.id)?;
        Ok(json!({ "undone": true, "deleted_task_id": task.id }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleUpdateHandler {
    async fn execute(&self, app_handle: &AppHandle, args: serde_json::Value, dry_run: bool) -> Result<serde_json::Value, String> {
        let id = task_id(&args)?;
        let patch = TaskPatch {
            name: string_arg(&args, &["name"]),
            schedule: string_arg(&args, &["schedule"]),
            prompt: string_arg(&args, &["prompt", "task_prompt"]),
            assistant_id: args.get("assistant").map(|v| service::resolve_assistant(app_handle, v)).transpose()?,
            deliver_policy: string_arg(&args, &["deliver"]),
            ends_at: string_arg(&args, &["ends_at"]),
            max_runs: args.get("max_runs").and_then(|v| v.as_i64()),
            missed_policy: string_arg(&args, &["missed"]),
            enabled: args.get("is_enabled").or_else(|| args.get("enabled")).and_then(|v| v.as_bool()),
            ..Default::default()
        };
        if dry_run {
            let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
            let task = db.read_task(id).map_err(|e| e.to_string())?.ok_or_else(|| format!("Scheduled task not found: {id}"))?;
            let updated = service::apply_patch(task, &patch, chrono::Utc::now())?;
            return Ok(json!({ "dry_run": true, "task_id": id, "would_update": { "name": updated.name, "schedule": updated.schedule_expr, "enabled": updated.is_enabled } }));
        }
        let view = service::update(app_handle, id, &patch)?;
        Ok(json!({ "task_id": id, "updated": task_card(&view) }))
    }

    async fn snapshot_before(&self, app_handle: &AppHandle, args: &serde_json::Value) -> Option<serde_json::Value> {
        let id = args.get("task_id")?.as_i64()?;
        let task = ScheduledTaskDatabase::new(app_handle).ok()?.read_task(id).ok()??;
        Some(json!({ "_type": "schedule.update", "task": serde_json::to_value(task).ok()? }))
    }

    async fn undo(&self, app_handle: &AppHandle, snapshot: &serde_json::Value, _original_args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let before: ScheduledTask = serde_json::from_value(snapshot.get("task").cloned().ok_or("快照缺少任务")?)
            .map_err(|e| format!("快照无法解析: {e}"))?;
        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.read_task(before.id).map_err(|e| e.to_string())?.ok_or_else(|| format!("Task {} no longer exists", before.id))?;
        let restored = ScheduledTask { updated_time: chrono::Utc::now(), ..before };
        db.update_task(&restored).map_err(|e| e.to_string())?;
        Ok(json!({ "undone": true, "task_id": restored.id, "restored": "schedule config" }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleRunNowHandler {
    async fn execute(&self, app_handle: &AppHandle, args: serde_json::Value, dry_run: bool) -> Result<serde_json::Value, String> {
        let id = task_id(&args)?;
        if dry_run {
            return Ok(json!({ "dry_run": true, "task_id": id, "would_run_immediately": true }));
        }
        let run_id = crate::scheduler::run_now(app_handle, id).await?;
        Ok(json!({ "task_id": id, "run_id": run_id, "started": true }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleDeleteHandler {
    async fn execute(&self, app_handle: &AppHandle, args: serde_json::Value, dry_run: bool) -> Result<serde_json::Value, String> {
        let id = task_id(&args)?;
        let view = service::get(app_handle, id)?;
        if dry_run {
            return Ok(json!({ "dry_run": true, "task_id": id, "task_name": view.name, "would_delete": true }));
        }
        service::delete(app_handle, id)?;
        Ok(json!({ "task_id": id, "deleted": true }))
    }

    async fn execute_approved(&self, app_handle: &AppHandle, args: serde_json::Value, snapshot: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
        let id = task_id(&args)?;
        let expected = snapshot.and_then(|v| v.get("approved_resource")).ok_or("缺少批准对象快照")?;
        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.delete_approved_task(id, expected).map_err(|e| e.to_string())?;
        Ok(json!({ "task_id": id, "deleted": true }))
    }

    async fn snapshot_before(&self, app_handle: &AppHandle, args: &serde_json::Value) -> Option<serde_json::Value> {
        let id = args.get("task_id")?.as_i64()?;
        let task = ScheduledTaskDatabase::new(app_handle).ok()?.read_task(id).ok()??;
        Some(json!({
            "_type": "schedule.delete",
            "approved_resource": serde_json::to_value(&task).ok()?,
            "task_id": task.id,
            "name": task.name,
        }))
    }

    async fn undo(&self, app_handle: &AppHandle, snapshot: &serde_json::Value, _original_args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let before: ScheduledTask = serde_json::from_value(snapshot.get("approved_resource").cloned().ok_or("快照缺少任务")?)
            .map_err(|e| format!("快照无法解析: {e}"))?;
        let now = chrono::Utc::now();
        let mut task = ScheduledTask { id: 0, run_count: 0, consecutive_failures: 0, created_time: now, updated_time: now, ..before };
        if let Ok(schedule) = task.schedule() {
            task.next_run_at = service::next_run(&task, &schedule, now);
        }
        task.is_enabled = task.is_enabled && task.next_run_at.is_some();
        let created = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?.create_task(&task).map_err(|e| e.to_string())?;
        Ok(json!({
            "undone": true,
            "new_task_id": created.id,
            "name": created.name,
            "note": "定时任务已按快照重新创建，原任务的运行记录不会恢复。",
        }))
    }
}

fn meta(action_id: &str, summary: &str, description: &str, risk: RiskLevel, approval: ApprovalPolicy, tags: &[&str], args_schema: serde_json::Value, dry_run: bool, rollback: Option<&str>) -> ActionMeta {
    ActionMeta {
        action_id: action_id.into(),
        domain: "schedule".into(),
        summary: summary.into(),
        description: description.into(),
        risk_level: risk,
        requires_approval: matches!(approval, ApprovalPolicy::UserApprovalRequired),
        approval_policy: approval,
        allowed_scopes: vec![ActionScope::App],
        tags: tags.iter().map(|t| t.to_string()).collect(),
        args_schema,
        result_schema: json!({ "type": "object" }),
        supports_dry_run: dry_run,
        rollback_hint: rollback.map(str::to_string),
    }
}

pub fn register(registry: &mut ActionRegistry) {
    let task_id_only = json!({ "type": "object", "properties": { "task_id": { "type": "integer", "description": "定时任务 ID" } }, "required": ["task_id"] });
    let fields = json!({
        "name": { "type": "string", "description": "任务名称" },
        "schedule": { "type": "string", "description": "时间，一个字符串：ISO 时间（2026-11-13T16:00+08:00）、相对时间（in 30m）、every 30m / every 1d at 09:00、或 5 段 cron（0 16 * * 4）" },
        "prompt": { "type": "string", "description": "每次运行的自包含指令（运行时看不到当前对话）" },
        "assistant": { "type": ["string", "integer"], "description": "执行助手名称或 ID" },
        "deliver": { "type": "string", "enum": ["always", "if_needed", "never"], "description": "always 每次发送；if_needed 无事可报时不打扰；never 只记录" },
        "ends_at": { "type": "string", "description": "可选，结束时间" },
        "max_runs": { "type": "integer", "description": "可选，最多运行次数" },
        "missed": { "type": "string", "enum": ["run_once", "skip"], "description": "应用未运行而错过时：补跑最近一次或只记录" }
    });
    registry.register(
        meta("schedule.list", "列出定时任务", "返回全部定时任务：人话时间、接下来的运行时间、执行助手、投递位置与上次结果。", RiskLevel::SAFE, ApprovalPolicy::AutoAllow, &["schedule", "read", "list"], json!({ "type": "object", "properties": {}, "required": [] }), false, None),
        Box::new(ScheduleListHandler),
    );
    registry.register(
        meta("schedule.get", "获取定时任务详情", "获取指定定时任务的设置与最近运行记录。", RiskLevel::SAFE, ApprovalPolicy::AutoAllow, &["schedule", "read", "detail"], task_id_only.clone(), false, None),
        Box::new(ScheduleGetHandler),
    );
    let mut create_props = fields.clone();
    create_props["assistant"] = json!({ "type": ["string", "integer"], "description": "执行助手名称或 ID（必填）" });
    registry.register(
        meta("schedule.create", "创建定时任务", "创建定时任务，创建即生效；结果以执行助手的身份发到它与用户的管家私聊。与 schedule_task 的 create 等价。", RiskLevel::MEDIUM, ApprovalPolicy::AllowInScope, &["schedule", "write", "create"],
            json!({ "type": "object", "properties": create_props, "required": ["name", "schedule", "prompt", "assistant"] }), true, Some("可通过 schedule.delete 删除创建的任务")),
        Box::new(ScheduleCreateHandler),
    );
    let mut update_props = fields;
    update_props["task_id"] = json!({ "type": "integer", "description": "定时任务 ID" });
    update_props["enabled"] = json!({ "type": "boolean", "description": "启用或暂停；恢复时不补跑暂停期间的时段" });
    registry.register(
        meta("schedule.update", "修改定时任务", "修改定时任务的名称、时间、指令、执行助手、投递策略或启用状态；时间变化会重新计算下一次运行。", RiskLevel::MEDIUM, ApprovalPolicy::AllowInScope, &["schedule", "write", "update"],
            json!({ "type": "object", "properties": update_props, "required": ["task_id"] }), true, Some("可通过 superadmin_undo 恢复修改前的设置")),
        Box::new(ScheduleUpdateHandler),
    );
    registry.register(
        meta("schedule.run_now", "立即运行定时任务", "立即开始一次运行，不改变计划时间；结果按任务设置投递。", RiskLevel::MEDIUM, ApprovalPolicy::AllowInScope, &["schedule", "execute"], task_id_only.clone(), true, None),
        Box::new(ScheduleRunNowHandler),
    );
    registry.register(
        meta("schedule.delete", "删除定时任务", "永久删除定时任务及其运行记录。此操作不可逆。", RiskLevel::HIGH, ApprovalPolicy::UserApprovalRequired, &["schedule", "write", "delete", "destructive"], task_id_only, true, Some("可通过 superadmin_undo 按快照重新创建任务（不恢复运行记录）")),
        Box::new(ScheduleDeleteHandler),
    );
}

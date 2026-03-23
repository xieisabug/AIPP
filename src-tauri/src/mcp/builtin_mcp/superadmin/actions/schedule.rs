use async_trait::async_trait;
use serde_json::json;
use tauri::AppHandle;

use crate::db::scheduled_task_db::ScheduledTaskDatabase;
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

struct ScheduleListHandler;
struct ScheduleGetHandler;
struct ScheduleCreateHandler;
struct ScheduleUpdateHandler;
struct ScheduleRunNowHandler;
struct ScheduleDeleteHandler;

fn task_to_json(t: &crate::db::scheduled_task_db::ScheduledTask) -> serde_json::Value {
    json!({
        "id": t.id,
        "name": t.name,
        "is_enabled": t.is_enabled,
        "schedule_type": t.schedule_type,
        "interval_value": t.interval_value,
        "interval_unit": t.interval_unit,
        "start_time": t.start_time,
        "assistant_id": t.assistant_id,
        "task_prompt": t.task_prompt,
        "next_run_at": t.next_run_at.map(|d| d.to_rfc3339()),
        "last_run_at": t.last_run_at.map(|d| d.to_rfc3339()),
        "created_time": t.created_time.to_rfc3339(),
    })
}

#[async_trait]
impl ActionHandler for ScheduleListHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        _args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let tasks = db.list_tasks().map_err(|e| e.to_string())?;

        let items: Vec<serde_json::Value> = tasks.iter().map(task_to_json).collect();
        Ok(json!({ "tasks": items, "count": items.len() }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleGetHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: task_id")?;

        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let task = db
            .read_task(task_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Scheduled task not found: {}", task_id))?;

        // Also get recent runs
        let runs = db.list_runs_by_task(task_id, 5).map_err(|e| e.to_string())?;
        let run_items: Vec<serde_json::Value> = runs
            .iter()
            .map(|r| {
                json!({
                    "run_id": r.run_id,
                    "status": r.status,
                    "notify": r.notify,
                    "summary": r.summary,
                    "error_message": r.error_message,
                    "started_time": r.started_time.to_rfc3339(),
                    "finished_time": r.finished_time.map(|d| d.to_rfc3339()),
                })
            })
            .collect();

        let mut result = task_to_json(&task);
        result["notify_prompt"] = json!(task.notify_prompt);
        result["recent_runs"] = json!(run_items);
        Ok(result)
    }
}

#[async_trait]
impl ActionHandler for ScheduleCreateHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: name")?;
        let schedule_type = args
            .get("schedule_type")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: schedule_type")?;
        let assistant_id = args
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: assistant_id")?;
        let task_prompt = args
            .get("task_prompt")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: task_prompt")?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_create": {
                    "name": name,
                    "schedule_type": schedule_type,
                    "assistant_id": assistant_id,
                }
            }));
        }

        let interval_value = args.get("interval_value").and_then(|v| v.as_i64());
        let interval_unit = args
            .get("interval_unit")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let start_time = args
            .get("start_time")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let notify_prompt = args
            .get("notify_prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        use crate::db::scheduled_task_db::ScheduledTask;
        use chrono::Utc;

        let task = ScheduledTask {
            id: 0,
            name: name.to_string(),
            is_enabled: true,
            schedule_type: schedule_type.to_string(),
            interval_value,
            interval_unit,
            start_time,
            week_days: args.get("week_days").and_then(|v| v.as_str()).map(|s| s.to_string()),
            month_days: args.get("month_days").and_then(|v| v.as_str()).map(|s| s.to_string()),
            run_at: None,
            next_run_at: None,
            last_run_at: None,
            assistant_id,
            task_prompt: task_prompt.to_string(),
            notify_prompt: notify_prompt.to_string(),
            created_time: Utc::now(),
            updated_time: Utc::now(),
        };

        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let created = db.create_task(&task).map_err(|e| e.to_string())?;

        Ok(json!({
            "task_id": created.id,
            "name": created.name,
        }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleUpdateHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: task_id")?;

        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let mut task = db
            .read_task(task_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Scheduled task not found: {}", task_id))?;

        // Apply updates from args
        if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
            task.name = name.to_string();
        }
        if let Some(enabled) = args.get("is_enabled").and_then(|v| v.as_bool()) {
            task.is_enabled = enabled;
        }
        if let Some(prompt) = args.get("task_prompt").and_then(|v| v.as_str()) {
            task.task_prompt = prompt.to_string();
        }
        if let Some(notify) = args.get("notify_prompt").and_then(|v| v.as_str()) {
            task.notify_prompt = notify.to_string();
        }
        if let Some(interval_value) = args.get("interval_value").and_then(|v| v.as_i64()) {
            task.interval_value = Some(interval_value);
        }
        if let Some(interval_unit) = args.get("interval_unit").and_then(|v| v.as_str()) {
            task.interval_unit = Some(interval_unit.to_string());
        }

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "task_id": task_id,
                "would_update": task_to_json(&task),
            }));
        }

        task.updated_time = chrono::Utc::now();
        db.update_task(&task).map_err(|e| e.to_string())?;

        Ok(json!({
            "task_id": task_id,
            "updated_fields": "applied",
        }))
    }
}

#[async_trait]
impl ActionHandler for ScheduleRunNowHandler {
    async fn execute(
        &self,
        _app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: task_id")?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "task_id": task_id,
                "would_run_immediately": true,
            }));
        }

        // NOTE: Running a scheduled task immediately requires FeatureConfigState
        // and triggers async AI execution. The existing run_scheduled_task_now
        // Tauri command is the proper entry point.
        Err(
            "schedule.run_now is not directly executable in Phase 1. \
             Use the existing run_scheduled_task_now Tauri command instead. \
             This action is registered for catalog/inspect discoverability."
                .to_string(),
        )
    }
}

#[async_trait]
impl ActionHandler for ScheduleDeleteHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let task_id = args
            .get("task_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: task_id")?;

        let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let task = db
            .read_task(task_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Scheduled task not found: {}", task_id))?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "task_id": task_id,
                "task_name": task.name,
                "would_delete": true,
            }));
        }

        db.delete_task(task_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "task_id": task_id,
            "deleted": true,
        }))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut ActionRegistry) {
    registry.register(
        ActionMeta {
            action_id: "schedule.list".into(),
            domain: "schedule".into(),
            summary: "列出定时任务".into(),
            description: "返回所有已配置的定时任务列表。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["schedule".into(), "read".into(), "list".into()],
            args_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "tasks": { "type": "array" },
                    "count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(ScheduleListHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "schedule.get".into(),
            domain: "schedule".into(),
            summary: "获取定时任务详情".into(),
            description: "获取指定定时任务的完整信息，包括最近运行记录。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["schedule".into(), "read".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer", "description": "定时任务 ID" }
                },
                "required": ["task_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "recent_runs": { "type": "array" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(ScheduleGetHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "schedule.create".into(),
            domain: "schedule".into(),
            summary: "创建定时任务".into(),
            description: "创建新的定时任务，支持一次性和周期性调度。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["schedule".into(), "write".into(), "create".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "任务名称" },
                    "schedule_type": { "type": "string", "enum": ["once", "interval"], "description": "调度类型" },
                    "interval_value": { "type": "integer", "description": "间隔值" },
                    "interval_unit": { "type": "string", "enum": ["minute", "hour", "day", "week", "month"], "description": "间隔单位" },
                    "start_time": { "type": "string", "description": "开始时间 (HH:mm 格式)" },
                    "assistant_id": { "type": "integer", "description": "执行助手 ID" },
                    "task_prompt": { "type": "string", "description": "任务提示词" },
                    "notify_prompt": { "type": "string", "description": "通知判断提示词" }
                },
                "required": ["name", "schedule_type", "assistant_id", "task_prompt"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer" },
                    "name": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 schedule.delete 删除创建的任务".into()),
        },
        Box::new(ScheduleCreateHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "schedule.update".into(),
            domain: "schedule".into(),
            summary: "更新定时任务".into(),
            description: "修改定时任务的配置，如名称、提示词、调度参数等。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["schedule".into(), "write".into(), "update".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer", "description": "定时任务 ID" },
                    "name": { "type": "string", "description": "新名称（可选）" },
                    "is_enabled": { "type": "boolean", "description": "是否启用（可选）" },
                    "task_prompt": { "type": "string", "description": "新任务提示词（可选）" },
                    "notify_prompt": { "type": "string", "description": "新通知提示词（可选）" },
                    "interval_value": { "type": "integer", "description": "新间隔值（可选）" },
                    "interval_unit": { "type": "string", "description": "新间隔单位（可选）" }
                },
                "required": ["task_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer" },
                    "updated_fields": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可再次调用 schedule.update 恢复旧配置".into()),
        },
        Box::new(ScheduleUpdateHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "schedule.run_now".into(),
            domain: "schedule".into(),
            summary: "立即运行定时任务".into(),
            description: "立即触发一次定时任务的执行。注意：Phase 1 中需使用现有 run_scheduled_task_now 命令。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["schedule".into(), "write".into(), "run".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer", "description": "定时任务 ID" }
                },
                "required": ["task_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer" },
                    "status": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: None,
        },
        Box::new(ScheduleRunNowHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "schedule.delete".into(),
            domain: "schedule".into(),
            summary: "删除定时任务".into(),
            description: "永久删除定时任务及其所有运行日志。此操作不可逆。".into(),
            risk_level: RiskLevel::HIGH,
            requires_approval: true,
            approval_policy: ApprovalPolicy::UserApprovalRequired,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["schedule".into(), "write".into(), "delete".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer", "description": "定时任务 ID" }
                },
                "required": ["task_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "integer" },
                    "deleted": { "type": "boolean" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: None,
        },
        Box::new(ScheduleDeleteHandler),
    );
}

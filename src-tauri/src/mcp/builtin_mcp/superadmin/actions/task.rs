use async_trait::async_trait;
use serde_json::json;
use tauri::AppHandle;

use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

struct TaskListHandler;
struct TaskGetHandler;
struct TaskSpawnHandler;
struct TaskCancelHandler;

#[async_trait]
impl ActionHandler for TaskListHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let butler_conversation_id = args
            .get("butler_conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: butler_conversation_id")?;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let tasks = repo
            .list_by_parent_butler_conversation_id(butler_conversation_id)
            .map_err(|e| e.to_string())?;

        let items: Vec<serde_json::Value> = tasks
            .iter()
            .map(|t| {
                json!({
                    "conversation_id": t.id,
                    "name": t.name,
                    "source_task_title": t.source_task_title,
                    "assistant_id": t.assistant_id,
                    "butler_task_status": t.butler_task_status,
                    "butler_task_summary": t.butler_task_summary,
                    "created_time": t.created_time.to_rfc3339(),
                })
            })
            .collect();

        Ok(json!({ "tasks": items, "count": items.len() }))
    }
}

#[async_trait]
impl ActionHandler for TaskGetHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let task_conversation_id = args
            .get("task_conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: task_conversation_id")?;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let conversation = repo
            .read(task_conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Task conversation not found: {}", task_conversation_id))?;

        if conversation.conversation_kind != "butler_task" {
            return Err(format!(
                "Conversation {} is not a butler task (kind={})",
                task_conversation_id, conversation.conversation_kind
            ));
        }

        // Get task definition and result from butler repo
        let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;

        let task_def = butler_repo
            .get_task_definition_by_task_conversation_id(task_conversation_id)
            .map_err(|e| e.to_string())?;

        let task_result = butler_repo
            .get_task_result(task_conversation_id)
            .map_err(|e| e.to_string())?;

        let mut result = json!({
            "conversation_id": conversation.id,
            "name": conversation.name,
            "source_task_title": conversation.source_task_title,
            "assistant_id": conversation.assistant_id,
            "butler_task_status": conversation.butler_task_status,
            "butler_task_summary": conversation.butler_task_summary,
            "created_time": conversation.created_time.to_rfc3339(),
        });

        if let Some(def) = task_def {
            result["definition"] = json!({
                "title": def.title,
                "goal": def.goal,
                "executor_assistant_id": def.executor_assistant_id,
            });
        }

        if let Some(res) = task_result {
            result["result"] = json!({
                "summary": res.summary,
                "followup_status": res.followup_status,
                "structured_output_json": res.structured_output_json,
            });
        }

        Ok(result)
    }
}

#[async_trait]
impl ActionHandler for TaskSpawnHandler {
    async fn execute(
        &self,
        _app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let butler_conversation_id = args
            .get("butler_conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: butler_conversation_id")?;
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: title")?;
        let goal = args
            .get("goal")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: goal")?;
        let executor_assistant_id = args.get("executor_assistant_id").and_then(|v| v.as_i64());
        let executor_assistant_name = args
            .get("executor_assistant_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_spawn": {
                    "butler_conversation_id": butler_conversation_id,
                    "title": title,
                    "goal_length": goal.len(),
                    "executor_assistant_id": executor_assistant_id,
                    "executor_assistant_name": executor_assistant_name,
                }
            }));
        }

        // NOTE: Actually spawning a butler task requires a Window handle and
        // triggers async AI execution. The existing spawn_task_conversation
        // agent tool in aipp:agent is the proper entry point.
        // This action provides a structured wrapper that documents the intent,
        // but for Phase 1 it delegates to the agent tool guidance.
        Err(
            "task.spawn is not directly executable in Phase 1. \
             Use the existing aipp:agent spawn_task_conversation tool instead, \
             which handles window context and AI execution. \
             This action is registered for catalog/inspect discoverability."
                .to_string(),
        )
    }
}

#[async_trait]
impl ActionHandler for TaskCancelHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let task_conversation_id = args
            .get("task_conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: task_conversation_id")?;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let mut conversation = repo
            .read(task_conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Task conversation not found: {}", task_conversation_id))?;

        if conversation.conversation_kind != "butler_task" {
            return Err(format!(
                "Conversation {} is not a butler task",
                task_conversation_id
            ));
        }

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "task_conversation_id": task_conversation_id,
                "current_status": conversation.butler_task_status,
            }));
        }

        // Mark as cancelled
        conversation.butler_task_status = Some("cancelled".to_string());
        conversation.butler_task_finalized_at = Some(chrono::Utc::now());
        repo.update(&conversation).map_err(|e| e.to_string())?;

        Ok(json!({
            "task_conversation_id": task_conversation_id,
            "status": "cancelled",
        }))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut ActionRegistry) {
    registry.register(
        ActionMeta {
            action_id: "task.list".into(),
            domain: "task".into(),
            summary: "列出 Butler 子任务".into(),
            description: "列出指定 Butler 主会话下的所有子任务。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["task".into(), "butler".into(), "read".into(), "list".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "butler_conversation_id": { "type": "integer", "description": "Butler 主会话 ID" }
                },
                "required": ["butler_conversation_id"]
            }),
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
        Box::new(TaskListHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "task.get".into(),
            domain: "task".into(),
            summary: "获取 Butler 任务详情".into(),
            description: "获取指定 Butler 子任务的完整信息，包括定义和结果。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::Conversation],
            tags: vec!["task".into(), "butler".into(), "read".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "task_conversation_id": { "type": "integer", "description": "任务会话 ID" }
                },
                "required": ["task_conversation_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer" },
                    "name": { "type": "string" },
                    "butler_task_status": { "type": "string" },
                    "definition": { "type": "object" },
                    "result": { "type": "object" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(TaskGetHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "task.spawn".into(),
            domain: "task".into(),
            summary: "派发 Butler 子任务".into(),
            description: "创建并启动一个新的 Butler 子任务。注意：Phase 1 中请使用 aipp:agent 的 spawn_task_conversation 工具，本 action 仅用于目录发现。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["task".into(), "butler".into(), "write".into(), "spawn".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "butler_conversation_id": { "type": "integer", "description": "Butler 主会话 ID" },
                    "title": { "type": "string", "description": "任务标题" },
                    "goal": { "type": "string", "description": "任务目标（作为提示词发送给执行助手）" },
                    "executor_assistant_id": { "type": "integer", "description": "执行助手 ID（可选）" },
                    "executor_assistant_name": { "type": "string", "description": "执行助手名称（可选，与 ID 二选一）" }
                },
                "required": ["butler_conversation_id", "title", "goal"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "task_conversation_id": { "type": "integer" },
                    "status": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 task.cancel 取消任务".into()),
        },
        Box::new(TaskSpawnHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "task.cancel".into(),
            domain: "task".into(),
            summary: "取消 Butler 子任务".into(),
            description: "将指定的 Butler 子任务标记为已取消。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Conversation],
            tags: vec!["task".into(), "butler".into(), "write".into(), "cancel".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "task_conversation_id": { "type": "integer", "description": "任务会话 ID" }
                },
                "required": ["task_conversation_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "task_conversation_id": { "type": "integer" },
                    "status": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: None,
        },
        Box::new(TaskCancelHandler),
    );
}

//! 定时任务运行中每个工具调用的准入：
//! 1. 运行只能管理自己的任务，不能创建、删除、立即运行其他任务；
//! 2. 必须人工的操作（rm/删除类命令、需要用户批准的 SuperAdmin 动作）直接拒绝，结果里写明需要用户亲自处理；
//! 3. 免审核工具直接执行；其余用审核模型判断（审核时带上任务指令作为用户意图）；
//! 4. 审核没过不挂起，转交总管家裁决；没有总管家或总管家不处理时按拒绝处理。
use tokio_util::sync::CancellationToken;

use crate::db::mcp_db::MCPDatabase;
use crate::db::scheduled_task_db::ScheduledTask;
use crate::mcp::tool_review::{self, UnattendedReview};
use crate::scheduler::escalation::{self, Decision};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// 可以执行；附带放行依据（审核理由或总管家的裁决），写进运行日志。
    Run(String),
    /// 不执行；原因会作为工具结果交给模型。
    Refuse(String),
}

pub struct GateContext<'a> {
    pub app: &'a tauri::AppHandle,
    pub task: &'a ScheduledTask,
    pub assistant_name: &'a str,
    pub run_id: &'a str,
    pub conversation_id: i64,
    pub cancel: &'a CancellationToken,
}

/// 运行中对 `schedule_task` 的限制：只能查看任务，或管理正在运行的这个任务本身。
pub(crate) fn scheduled_tool_restriction(command: Option<&str>, tool: &str, parameters: &str, own_task_id: i64) -> Option<String> {
    if command != Some("aipp:agent") || tool != "schedule_task" {
        return None;
    }
    let args: serde_json::Value = serde_json::from_str(parameters).unwrap_or_default();
    match args["action"].as_str() {
        Some("list" | "get" | "runs") => None,
        Some("pause" | "update") if args["task_id"].as_i64() == Some(own_task_id) => None,
        Some("pause" | "update") => Some("定时任务运行中只能暂停或修改正在运行的这个任务本身".to_string()),
        _ => Some("定时任务运行中不能创建、删除、恢复或立即运行定时任务，也不能裁决其他操作".to_string()),
    }
}

/// 必须由用户本人处理的操作，总管家也不能代批。
pub(crate) fn human_only_reason(command: Option<&str>, tool: &str, parameters: &str) -> Option<String> {
    match command {
        Some("aipp:operation") => crate::mcp::builtin_mcp::contains_manual_review_command_keyword(parameters)
            .then(|| "包含 rm/删除类命令".to_string()),
        Some("aipp:superadmin") => {
            let args: serde_json::Value = serde_json::from_str(parameters).unwrap_or_default();
            let actions: Vec<&serde_json::Value> = match tool {
                "superadmin_execute" => vec![&args],
                "superadmin_batch" => args["actions"].as_array().map(|a| a.iter().collect()).unwrap_or_default(),
                _ => Vec::new(),
            };
            actions.into_iter().find_map(|action| {
                let id = action["action_id"].as_str()?;
                let dry_run = action["dry_run"].as_bool().unwrap_or(false);
                crate::mcp::builtin_mcp::superadmin::action_approval_gate(id, dry_run).map(|_| format!("{id} 需要用户批准"))
            })
        }
        _ => None,
    }
}

fn log(ctx: &GateContext<'_>, kind: &str, content: String) {
    crate::api::scheduled_task_api::log_task_message(ctx.app, ctx.task.id, ctx.run_id, kind, content);
}

pub async fn admit(ctx: &GateContext<'_>, call_id: i64, progress: &str) -> Admission {
    let resolved = MCPDatabase::new(ctx.app)
        .map_err(|e| e.to_string())
        .and_then(|db| {
            let call = db.get_mcp_tool_call(call_id).map_err(|e| e.to_string())?;
            let server = db.get_mcp_server(call.server_id).map_err(|e| e.to_string())?;
            Ok((call, server))
        });
    let (call, server) = match resolved {
        Ok(resolved) => resolved,
        Err(error) => return Admission::Refuse(format!("读取工具调用失败，未执行：{error}")),
    };
    let command = server.command.as_deref().filter(|_| server.is_builtin);
    if let Some(reason) = scheduled_tool_restriction(command, &call.tool_name, &call.parameters, ctx.task.id) {
        return Admission::Refuse(reason);
    }
    if let Some(reason) = human_only_reason(command, &call.tool_name, &call.parameters) {
        log(ctx, "needs_user", format!("{} / {}：{reason}，已拒绝", call.server_name, call.tool_name));
        return Admission::Refuse(format!(
            "该操作{reason}，必须由用户亲自处理，定时任务不会执行。请在最终回复里告诉用户需要他亲自完成哪一步。"
        ));
    }
    let reason = match tool_review::review_unattended(ctx.app, ctx.conversation_id, call_id).await {
        UnattendedReview::Exempt => return Admission::Run("免审核工具".to_string()),
        UnattendedReview::Safe(reason) => return Admission::Run(format!("自动审核通过：{reason}")),
        UnattendedReview::NotSafe(reason) => reason,
    };
    log(ctx, "review_held", format!("{} / {} 自动审核未通过（{reason}），转交总管家裁决", call.server_name, call.tool_name));
    let request = escalation::Request {
        task_id: ctx.task.id,
        task_name: ctx.task.name.clone(),
        task_prompt: ctx.task.task_prompt.clone(),
        assistant_name: ctx.assistant_name.to_string(),
        run_id: ctx.run_id.to_string(),
        server_name: call.server_name.clone(),
        tool_name: call.tool_name.clone(),
        parameters: call.parameters.clone(),
        review_reason: reason,
        progress: progress.to_string(),
    };
    match escalation::escalate(ctx.app, request, ctx.cancel).await {
        Decision::Allow(reason) => {
            tool_review::record_butler_decision(ctx.app, call_id, true);
            log(ctx, "butler_decision", format!("总管家允许：{reason}"));
            Admission::Run(format!("总管家允许：{reason}"))
        }
        Decision::Deny(reason) => {
            tool_review::record_butler_decision(ctx.app, call_id, false);
            log(ctx, "butler_decision", format!("未执行：{reason}"));
            Admission::Refuse(format!("该操作未执行：{reason}。如果它是完成任务必需的，请在最终回复里说明卡在哪一步。"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_can_only_manage_its_own_task() {
        let check = |args: serde_json::Value| scheduled_tool_restriction(Some("aipp:agent"), "schedule_task", &args.to_string(), 6);
        assert!(check(serde_json::json!({"action": "list"})).is_none());
        assert!(check(serde_json::json!({"action": "get", "task_id": 9})).is_none());
        assert!(check(serde_json::json!({"action": "pause", "task_id": 6})).is_none());
        assert!(check(serde_json::json!({"action": "update", "task_id": 6, "schedule": "every 2h"})).is_none());
        assert!(check(serde_json::json!({"action": "pause", "task_id": 7})).is_some());
        for action in ["create", "delete", "resume", "run_now", "decide"] {
            assert!(check(serde_json::json!({"action": action, "task_id": 6})).is_some(), "{action}");
        }
        assert!(scheduled_tool_restriction(Some("aipp:agent"), "load_skill", "{}", 6).is_none());
        assert!(scheduled_tool_restriction(None, "schedule_task", r#"{"action":"create"}"#, 6).is_none(), "外部同名工具不按内置处理");
    }

    #[test]
    fn test_human_only_operations_are_never_delegated() {
        assert!(human_only_reason(Some("aipp:operation"), "execute_bash", r#"{"command":"rm -rf build"}"#).is_some());
        assert!(human_only_reason(Some("aipp:operation"), "execute_bash", r#"{"command":"Remove-Item x"}"#).is_some());
        assert!(human_only_reason(Some("aipp:operation"), "execute_bash", r#"{"command":"ls -la"}"#).is_none());
        let delete = r#"{"action_id":"schedule.delete","args":{"task_id":1}}"#;
        assert!(human_only_reason(Some("aipp:superadmin"), "superadmin_execute", delete).is_some());
        let batch = r#"{"actions":[{"action_id":"schedule.list","args":{}},{"action_id":"schedule.delete","args":{"task_id":1}}]}"#;
        assert!(human_only_reason(Some("aipp:superadmin"), "superadmin_batch", batch).is_some());
        assert!(human_only_reason(Some("aipp:superadmin"), "superadmin_execute", r#"{"action_id":"schedule.list","args":{}}"#).is_none());
        assert!(human_only_reason(None, "execute_bash", r#"{"command":"rm x"}"#).is_none());
    }
}

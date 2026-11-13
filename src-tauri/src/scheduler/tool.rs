//! 模型使用的 `schedule_task` 工具。所有写操作都走 [`super::service`]，与界面规则一致。
use serde_json::{json, Value};

use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::message_preview::SCHEDULED_RUN_KIND;
use crate::db::scheduled_task_db::{DELIVER_CONVERSATION, DELIVER_OWNER_CHAT, DELIVER_RUN_CONVERSATION};
use crate::mcp::builtin_mcp::templates::is_butler_conversation_kind;
use crate::scheduler::service::{self, TaskDefaults, TaskInput, TaskPatch, TaskView};

const LIST_LIMIT: usize = 50;

fn ok(value: Value) -> Value {
    json!({ "content": [{ "type": "json", "json": value }], "isError": false })
}

fn err(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }], "isError": true })
}

/// 调用方所在会话：类型与会话助手。
#[derive(Debug, Clone, Default)]
pub(crate) struct Caller {
    pub conversation_id: Option<i64>,
    pub kind: String,
    pub assistant_id: Option<i64>,
}

fn caller(app: &tauri::AppHandle, conversation_id: Option<i64>) -> Result<Caller, String> {
    let Some(id) = conversation_id else { return Ok(Caller::default()) };
    let conversation = ConversationDatabase::new(app)
        .map_err(|e| e.to_string())?
        .conversation_repo()
        .map_err(|e| e.to_string())?
        .read(id)
        .map_err(|e| e.to_string())?;
    Ok(match conversation {
        Some(c) => Caller { conversation_id: Some(id), kind: c.conversation_kind, assistant_id: c.assistant_id },
        None => Caller { conversation_id: Some(id), ..Default::default() },
    })
}

/// 按创建场景决定默认投递位置：管家会话投到执行助手的私聊；普通会话投回当前会话；无会话时结果单独成会话。
pub(crate) fn defaults_for(caller: &Caller, owner: i64, deliver_to: Option<&str>) -> Result<TaskDefaults, String> {
    let butler = is_butler_conversation_kind(&caller.kind);
    let this_conversation = |caller: &Caller| -> Result<(String, Option<i64>), String> {
        if butler && caller.assistant_id == Some(owner) {
            // 私聊会被「重开」：投到助手当前的私聊，而不是固定的会话编号。
            return Ok((DELIVER_OWNER_CHAT.to_string(), None));
        }
        let id = caller.conversation_id.ok_or("当前没有会话，不能投递到 this_conversation")?;
        Ok((DELIVER_CONVERSATION.to_string(), Some(id)))
    };
    let (deliver_to, deliver_conversation_id) = match deliver_to {
        Some("owner_chat") => (DELIVER_OWNER_CHAT.to_string(), None),
        Some("this_conversation") => this_conversation(caller)?,
        Some(other) => return Err(format!("deliver_to 只能是 owner_chat 或 this_conversation，收到「{other}」")),
        None if butler => (DELIVER_OWNER_CHAT.to_string(), None),
        None if caller.conversation_id.is_some() => this_conversation(caller)?,
        None => (DELIVER_RUN_CONVERSATION.to_string(), None),
    };
    Ok(TaskDefaults {
        created_in_conversation_id: caller.conversation_id,
        butler_mode: butler,
        deliver_to,
        deliver_conversation_id,
    })
}

fn string_arg(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| args.get(*key).and_then(Value::as_str)).map(str::to_string)
}

fn task_id(args: &Value) -> Result<i64, String> {
    args.get("task_id").and_then(Value::as_i64).ok_or_else(|| "缺少 task_id".to_string())
}

/// 给模型的任务摘要：人话时间、接下来几次、投递位置，便于它核对后原样告诉用户。
pub(crate) fn task_card(view: &TaskView) -> Value {
    json!({
        "task_id": view.id,
        "name": view.name,
        "enabled": view.enabled,
        "assistant": view.assistant_name,
        "schedule": view.schedule,
        "schedule_text": view.schedule_text,
        "next_runs": view.upcoming_text,
        "deliver": view.deliver_policy,
        "deliver_to": view.deliver_target_text,
        "ends_at": view.ends_at,
        "max_runs": view.max_runs,
        "run_count": view.run_count,
        "paused_reason": view.paused_reason,
        "last_run": view.last_run.as_ref().map(|run| json!({
            "status": run.status_text,
            "reason": run.reason,
            "at": run.started_time,
        })),
    })
}

fn list_row(view: &TaskView) -> Value {
    json!({
        "task_id": view.id,
        "name": view.name,
        "enabled": view.enabled,
        "assistant": view.assistant_name,
        "schedule_text": view.schedule_text,
        "next_run": view.upcoming_text.first(),
        "last_status": view.last_run.as_ref().map(|run| run.status_text.clone()),
        "last_reason": view.last_run.as_ref().and_then(|run| run.reason.clone()),
        "paused_reason": view.paused_reason,
    })
}

fn patch_from(app: &tauri::AppHandle, args: &Value) -> Result<TaskPatch, String> {
    Ok(TaskPatch {
        name: string_arg(args, &["name"]),
        schedule: string_arg(args, &["schedule"]),
        prompt: string_arg(args, &["prompt", "task_prompt"]),
        assistant_id: args.get("assistant").map(|v| service::resolve_assistant(app, v)).transpose()?,
        deliver_policy: string_arg(args, &["deliver"]),
        deliver_to: None,
        deliver_conversation_id: None,
        ends_at: string_arg(args, &["ends_at"]),
        max_runs: args.get("max_runs").and_then(Value::as_i64),
        missed_policy: string_arg(args, &["missed"]),
        enabled: None,
    })
}

pub async fn handle(app: &tauri::AppHandle, conversation_id: Option<i64>, args: &Value) -> Value {
    match handle_inner(app, conversation_id, args).await {
        Ok(value) => ok(value),
        Err(error) => err(error),
    }
}

async fn handle_inner(app: &tauri::AppHandle, conversation_id: Option<i64>, args: &Value) -> Result<Value, String> {
    let caller = caller(app, conversation_id)?;
    let action = args.get("action").and_then(Value::as_str).unwrap_or("");
    match action {
        "create" => {
            if caller.kind == SCHEDULED_RUN_KIND {
                return Err("定时任务运行中不能再创建定时任务".to_string());
            }
            let owner = match args.get("assistant") {
                Some(value) => service::resolve_assistant(app, value)?,
                None => caller.assistant_id.ok_or("没有可用的执行助手，请用 assistant 指定")?,
            };
            let mut defaults = defaults_for(&caller, owner, args.get("deliver_to").and_then(Value::as_str))?;
            if defaults.deliver_to == DELIVER_CONVERSATION && caller.kind == SCHEDULED_RUN_KIND {
                defaults.deliver_to = DELIVER_OWNER_CHAT.to_string();
            }
            let input = TaskInput {
                name: string_arg(args, &["name"]).unwrap_or_default(),
                schedule: string_arg(args, &["schedule"]).unwrap_or_default(),
                prompt: string_arg(args, &["prompt", "task_prompt"]).unwrap_or_default(),
                assistant_id: owner,
                deliver_policy: string_arg(args, &["deliver"]),
                deliver_to: None,
                deliver_conversation_id: None,
                ends_at: string_arg(args, &["ends_at"]),
                max_runs: args.get("max_runs").and_then(Value::as_i64),
                missed_policy: string_arg(args, &["missed"]),
                enabled: Some(true),
            };
            let view = service::create(app, &input, &defaults)?;
            Ok(json!({
                "created": task_card(&view),
                "note": "任务已生效。请把名称、执行时间（schedule_text 与 next_runs）和结果会发到哪里告诉用户。",
            }))
        }
        "list" => {
            let mine = args.get("scope").and_then(Value::as_str) != Some("all");
            let views: Vec<Value> = service::list(app)?
                .iter()
                .filter(|view| {
                    !mine
                        || caller.assistant_id == Some(view.assistant_id)
                        || (caller.conversation_id.is_some() && view.created_in_conversation_id == caller.conversation_id)
                })
                .take(LIST_LIMIT)
                .map(list_row)
                .collect();
            Ok(json!({ "tasks": views, "count": views.len(), "scope": if mine { "mine" } else { "all" } }))
        }
        "get" => {
            let id = task_id(args)?;
            let view = service::get(app, id)?;
            let runs = service::list_runs(app, id, 5)?;
            let mut card = task_card(&view);
            card["prompt"] = json!(view.prompt);
            card["recent_runs"] = json!(runs
                .iter()
                .map(|run| json!({
                    "run_id": run.run_id,
                    "status": run.status_text,
                    "reason": run.reason,
                    "summary": run.summary,
                    "scheduled_for": run.scheduled_for,
                    "work_ref": run.final_message_id.map(|_| format!("scheduled_run:{}", run.run_id)),
                }))
                .collect::<Vec<_>>());
            Ok(card)
        }
        "runs" => {
            let id = task_id(args)?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10).clamp(1, 50) as u32;
            Ok(json!({ "runs": service::list_runs(app, id, limit)? }))
        }
        "update" => {
            let id = task_id(args)?;
            let mut patch = patch_from(app, args)?;
            if let Some(target) = args.get("deliver_to").and_then(Value::as_str) {
                let task = service::get(app, id)?;
                let defaults = defaults_for(&caller, patch.assistant_id.unwrap_or(task.assistant_id), Some(target))?;
                patch.deliver_to = Some(defaults.deliver_to);
                patch.deliver_conversation_id = defaults.deliver_conversation_id;
            }
            let view = service::update(app, id, &patch)?;
            Ok(json!({ "updated": task_card(&view) }))
        }
        "pause" => Ok(json!({ "paused": task_card(&service::set_enabled(app, task_id(args)?, false)?) })),
        "resume" => Ok(json!({ "resumed": task_card(&service::set_enabled(app, task_id(args)?, true)?) })),
        "run_now" => {
            let id = task_id(args)?;
            let run_id = crate::scheduler::run_now(app, id).await?;
            Ok(json!({ "started": true, "task_id": id, "run_id": run_id, "note": "已开始运行，结果会按任务设置投递；可用 runs 查看。" }))
        }
        "delete" => {
            let id = task_id(args)?;
            let view = service::get(app, id)?;
            service::delete(app, id)?;
            Ok(json!({ "deleted": true, "task_id": id, "name": view.name }))
        }
        "decide" => {
            if !is_butler_conversation_kind(&caller.kind) {
                return Err("只有管家会话可以裁决定时任务的操作".to_string());
            }
            let request_id = string_arg(args, &["request_id"]).ok_or("缺少 request_id")?;
            let allow = match args.get("decision").and_then(Value::as_str) {
                Some("allow") => true,
                Some("deny") => false,
                other => return Err(format!("decision 只能是 allow 或 deny，收到 {other:?}")),
            };
            let reason = string_arg(args, &["reason"]).unwrap_or_default();
            Ok(json!({ "result": crate::scheduler::escalation::decide(&request_id, allow, &reason)? }))
        }
        "" => Err("缺少 action".to_string()),
        other => Err(format!("未知的 action「{other}」，可用：create、list、get、runs、update、pause、resume、run_now、delete、decide")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(kind: &str, conversation_id: Option<i64>, assistant_id: Option<i64>) -> Caller {
        Caller { conversation_id, kind: kind.to_string(), assistant_id }
    }

    #[test]
    fn test_butler_conversations_deliver_to_owner_private_chat() {
        let contact = caller("butler_contact", Some(1529), Some(40));
        let defaults = defaults_for(&contact, 40, None).unwrap();
        assert_eq!((defaults.deliver_to.as_str(), defaults.butler_mode), (DELIVER_OWNER_CHAT, true));
        // 总管家替教练建的任务：结果发到教练的私聊
        let main = caller("butler_main", Some(1520), Some(1));
        assert_eq!(defaults_for(&main, 40, None).unwrap().deliver_to, DELIVER_OWNER_CHAT);
        // 群成员建的任务也走管家模式
        let member = caller("butler_group_member", Some(88), Some(40));
        assert!(defaults_for(&member, 40, None).unwrap().butler_mode);
    }

    #[test]
    fn test_this_conversation_follows_private_chat_resets_but_pins_other_conversations() {
        let contact = caller("butler_contact", Some(1529), Some(40));
        assert_eq!(defaults_for(&contact, 40, Some("this_conversation")).unwrap().deliver_to, DELIVER_OWNER_CHAT);
        let main = caller("butler_main", Some(1520), Some(1));
        let pinned = defaults_for(&main, 40, Some("this_conversation")).unwrap();
        assert_eq!((pinned.deliver_to.as_str(), pinned.deliver_conversation_id), (DELIVER_CONVERSATION, Some(1520)));
    }

    #[test]
    fn test_normal_chat_delivers_back_and_ui_defaults_to_own_conversation() {
        let normal = caller("normal", Some(12), Some(3));
        let defaults = defaults_for(&normal, 3, None).unwrap();
        assert_eq!((defaults.deliver_to.as_str(), defaults.deliver_conversation_id, defaults.butler_mode), (DELIVER_CONVERSATION, Some(12), false));
        assert_eq!(defaults_for(&Caller::default(), 3, None).unwrap().deliver_to, DELIVER_RUN_CONVERSATION);
        assert!(defaults_for(&normal, 3, Some("somewhere")).is_err());
    }
}

//! 管家工作引用、续接与任务提问：
//! - 用户引用某份结果时，按真实记录身份关联原工作，带入必要上下文（不恢复执行、不恢复授权）；
//! - 未引用时给出近期工作清单，指代不明时由助手一句话请用户选择；
//! - `continue_work` 按原任务状态有序续接（运行中排队送达，已结束则建立关联的后续任务，不继承临时授权）；
//! - 独立任务里的提问显示在执行助手自己的联系人窗口，答案回到原问题与原执行。
use super::ai::types::{AiRequest, WorkQuote};
use super::ai_api::ask_ai;
use super::butler_api::{
    is_current_butler_main_conversation, load_or_create_butler_main_internal,
    resolve_or_create_butler_execution_window, SpawnButlerTaskRequest,
    RESULT_MODE_DIRECT_REPORT,
};
use crate::db::{
    butler_peer as peer,
    butler_work::{self as work, clip, Followup, WorkView},
    conversation_db::{ConversationDatabase, Repository},
    mcp_db::MCPDatabase,
};
use crate::mcp::builtin_mcp::interaction::InteractionState;
use crate::state::activity_state::ConversationActivityManager;
use anyhow::{anyhow, ensure, Context};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, Manager};

#[cfg(test)]
#[path = "butler_work_tests.rs"]
mod tests;

// Serialize followup admission with cancellation; release before stopping live runtimes.
static FOLLOWUP_EXECUTION_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub(crate) async fn execution_gate() -> tokio::sync::MutexGuard<'static, ()> {
    FOLLOWUP_EXECUTION_GATE.lock().await
}

fn connection(app: &AppHandle) -> anyhow::Result<Connection> {
    Ok(ConversationDatabase::new(app)?.get_connection()?)
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, "continue_work" | "read_work_context")
}

/// 本轮发给模型的补充上下文与要随用户消息保存的引用身份。
#[derive(Debug, Default, PartialEq)]
pub(crate) struct TurnContext {
    pub prefix: Option<String>,
    pub metadata: Option<Value>,
}

pub(crate) fn quote_prompt(view: &WorkView) -> String {
    let mut text = format!(
        "<quoted_work ref=\"{}\">\n类型：{}｜状态：{}\n",
        view.work_ref,
        match view.kind.as_str() {
            "task" => "独立任务",
            "exchange" => "助手间委托",
            _ => "本会话对话",
        },
        view.status
    );
    if let Some(requester) = &view.requester {
        text.push_str(&format!("来源：{}（{}）\n", view.label, requester));
    } else {
        text.push_str(&format!("来源：{}\n", view.label));
    }
    text.push_str(&format!("目标：{}\n", clip(&view.title, 600)));
    match (&view.excerpt, &view.result) {
        (Some(excerpt), _) => text.push_str(&format!("用户引用的内容：\n{}\n", clip(excerpt, 1500))),
        (None, Some(result)) => text.push_str(&format!("结果：\n{}\n", clip(result, 1500))),
        _ => {}
    }
    text.push_str("</quoted_work>\n");
    text.push_str("用户这条消息引用了上面这项工作。只是讨论、解释或评价结果时，直接在当前聊天回答，不要重新执行。");
    if view.kind == "task" {
        text.push_str(&format!(
            "用户明确要求继续执行或修改时，调用 `continue_work`（work_ref=\"{}\"）续接：原任务仍在运行会按顺序送达，已结束会建立关联的后续任务。",
            view.work_ref
        ));
    } else {
        text.push_str("用户要求继续修改时，在当前会话里基于这项工作继续即可，不要另起重复执行。");
    }
    text.push_str("引用不会恢复原工作的临时授权，也不代替用户批准；需要更多细节时用 `read_work_context` 按需读取，不要猜测或编造。");
    text
}

pub(crate) fn recent_prompt(views: &[WorkView]) -> Option<String> {
    if views.is_empty() {
        return None;
    }
    if views.len() == 1 {
        let view = &views[0];
        let continuation = if view.kind == "task" {
            "只有用户明确要求继续这项独立任务时才调用 `continue_work`"
        } else {
            "用户要求修改时在当前会话继续，不要为普通对话创建独立任务"
        };
        // A projected task result is not part of this contact's model history.
        // Supply a bounded reference, not an implicit quote or execution grant.
        return Some(format!(
            "<available_work ref=\"{}\">\n来源：{}\n状态：{}\n目标：{}\n结果摘录：{}\n</available_work>\n用户没有引用具体工作。上面只是当前会话可访问的唯一近期工作，供明确承接该话题时参考，不代表用户这句话一定指它。普通聊天和新话题照常回答；如果指代无法判断，请简短询问。解释或讨论结果不要重新执行；{continuation}，需要更多上下文用 `read_work_context`。这些资料不恢复任何旧权限，也不代替用户批准。",
            view.work_ref,
            clip(&view.label, 160),
            view.status,
            clip(&view.title, 600),
            view.result.as_deref().map(|s| clip(s, 1500)).unwrap_or_else(|| "（尚无结果）".into()),
        ));
    }
    let lines: Vec<String> = views
        .iter()
        .map(|v| format!("- {} {}：{}（{}）", v.work_ref, v.label, clip(&v.title, 60), v.status))
        .collect();
    Some(format!(
        "<recent_work>\n{}\n</recent_work>\n用户没有引用具体工作。当前话题明确时自然承接；如果可能指上面的多项工作而无法判断，用一句话请用户选择，不要猜测，也不要要求用户提供任务编号。需要续接独立任务时用 `continue_work`。",
        lines.join("\n")
    ))
}

/// 计算一轮用户消息的工作上下文。只用于管家联系人与总管家会话。
pub(crate) fn turn_context_with(conn: &Connection, conversation: i64, quote: Option<&WorkQuote>) -> anyhow::Result<TurnContext> {
    let kind: Option<String> = conn
        .query_row("SELECT conversation_kind FROM conversation WHERE id=?1", [conversation], |r| r.get(0))
        .optional()?;
    if !matches!(kind.as_deref(), Some("butler_contact" | "butler_main")) {
        return Ok(TurnContext::default());
    }
    if let Some(quote) = quote {
        return Ok(match work::resolve(conn, conversation, &quote.kind, quote.id) {
            Ok(view) => TurnContext {
                prefix: Some(quote_prompt(&view)),
                metadata: Some(json!({"quote": {"ref": view.work_ref, "kind": quote.kind, "id": quote.id, "label": view.label}})),
            },
            Err(error) => TurnContext {
                prefix: Some(format!(
                    "<quoted_work unavailable=\"true\">用户引用的记录不可用：{error:#}</quoted_work>\n请如实告诉用户这条引用已无法读取（已删除或无权访问），不要编造其内容；需要时请用户说明或重新引用。"
                )),
                metadata: Some(json!({"quote": {"kind": quote.kind, "id": quote.id, "unavailable": format!("{error:#}")}})),
            },
        });
    }
    Ok(TurnContext { prefix: recent_prompt(&work::recent(conn, conversation, 5)?), metadata: None })
}

pub(crate) fn turn_context(app: &AppHandle, conversation: i64, quote: Option<&WorkQuote>) -> anyhow::Result<TurnContext> {
    turn_context_with(&connection(app)?, conversation, quote)
}

pub async fn tool(app: &AppHandle, current: i64, name: &str, args: &Value) -> anyhow::Result<Value> {
    let work_ref = args["work_ref"].as_str().map(str::trim).filter(|s| !s.is_empty()).context("需要 work_ref")?;
    let (kind, id) = work::parse_ref(work_ref)?;
    match name {
        "read_work_context" => {
            let conn = connection(app)?;
            let view = work::resolve(&conn, current, kind, id)?;
            let mut value = serde_json::to_value(&view)?;
            for key in ["title", "result", "excerpt"] {
                if let Some(text) = value[key].as_str() {
                    value[key] = json!(clip(text, 6000));
                }
            }
            if let Some(task) = view.task_conversation_id {
                let latest: Option<String> = conn
                    .query_row(
                        "SELECT content FROM message WHERE conversation_id=?1 AND message_type IN ('response','assistant')
                           AND trim(content)!='' ORDER BY id DESC LIMIT 1",
                        [task],
                        |r| r.get(0),
                    )
                    .optional()?;
                value["latest_reply"] = json!(latest.map(|s| clip(&s, 3000)));
            }
            Ok(value)
        }
        "continue_work" => {
            ensure!(kind == "task", "只有独立任务需要续接；其他工作请直接在当前会话里继续，或用 message_bot 请对应助手处理");
            let instruction = args["instruction"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= 12000)
                .context("instruction 必须为 1 至 12000 字符")?;
            let key = args["request_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .context("需要 1 至 128 字节的稳定 request_id")?;
            let mut conn = connection(app)?;
            let view = work::resolve(&conn, current, kind, id)?;
            let chain = match peer::turn_work(&conn, current)? {
                Some(turn) => Some(peer::chain_of(&conn, turn.root, current)?),
                None => None,
            };
            let (followup, created) = work::enqueue_followup(&mut conn, current, id, key, instruction, chain)?;
            drop(conn);
            super::butler_peer::wake();
            Ok(json!({
                "followup_id": followup.id, "state": followup.state, "deduplicated": !created,
                "result_task_conversation_id": followup.result_task,
                "instruction": if view.finished {
                    "原任务已结束：将建立关联的后续任务继续执行（不恢复原任务的临时授权，需要的权限会重新请求）。结果会以执行助手的名义交付，本轮无需等待或轮询。"
                } else {
                    "原任务仍在进行：续接要求会在它空闲时按顺序送达同一执行，不会同时启动第二个执行。本轮无需等待或轮询。"
                },
            }))
        }
        _ => Err(anyhow!("未知工作工具 {name}")),
    }
}

fn followup_goal(original_title: &str, goal: &str, result: Option<&str>, instruction: &str) -> String {
    format!(
        "这是任务《{original_title}》的后续执行。\n原目标：\n{}\n\n原结果（摘录）：\n{}\n\n用户的新要求：\n{instruction}\n\n请在原结果的基础上完成新要求。原任务的临时授权不会自动恢复，需要的权限按运行时审批重新申请。",
        clip(goal, 4000),
        result.map(|r| clip(r, 3000)).unwrap_or_else(|| "（原任务没有记录结果正文）".into()),
    )
}

/// 把用户的新要求送进仍在执行的任务会话（与任务内其他回合串行）。
async fn send_to_task(app: &AppHandle, task: i64, assistant_id: i64, text: String, followup_id: i64) -> anyhow::Result<()> {
    let window = resolve_or_create_butler_execution_window(app).map_err(anyhow::Error::msg)?;
    let result = ask_ai(
        app.clone(),
        app.state(),
        app.state(),
        app.state(),
        app.state(),
        app.state(),
        window,
        AiRequest {
            conversation_id: task.to_string(),
            assistant_id,
            prompt: text,
            model: None,
            override_model_id: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: Some(true),
            attachment_list: None,
            quote: None,
        },
        None,
        None,
        None,
        None,
        Some(format!("butler_followup/{followup_id}")),
    )
    .await?;
    if let Some(error) = result.error_message {
        return Err(anyhow!(error));
    }
    Ok(())
}

async fn process_followup(app: &AppHandle, followup: &Followup) -> anyhow::Result<(String, Option<i64>)> {
    let db = ConversationDatabase::new(app)?;
    anyhow::ensure!(work::followup_is_sending(&connection(app)?, followup.id)?, "续接已停止，未开始执行");
    let target = work::followup_target(&connection(app)?, followup)?;
    let task = db
        .conversation_repo()?
        .read(target)?
        .ok_or_else(|| anyhow!("原任务 #{} 已删除，无法续接", followup.task_conversation_id))?;
    let definition = db
        .butler_repo()?
        .get_task_definition_by_task_conversation_id(task.id)?
        .ok_or_else(|| anyhow!("原任务 #{} 缺少任务定义，无法续接", task.id))?;
    let terminal = task.butler_task_finalized_at.is_some()
        || matches!(task.butler_task_status.as_deref(), Some("succeeded" | "failed" | "cancelled" | "interrupted"));
    if !terminal {
        let assistant = task.assistant_id.context("任务会话缺少执行助手")?;
        let _admission = execution_gate().await;
        anyhow::ensure!(work::followup_is_sending(&connection(app)?, followup.id)?, "续接已停止，未开始执行");
        send_to_task(
            app,
            task.id,
            assistant,
            format!(
                "[用户补充要求 · 来自执行助手会话]\n{}\n\n这是用户对本任务的后续要求，请在当前任务中继续完成；仍遵循原任务的权限与审批约束。",
                followup.instruction
            ),
            followup.id,
        )
        .await?;
        return Ok(("delivered".into(), None));
    }
    let result: Option<String> = connection(app)?
        .query_row(
            "SELECT coalesce(nullif(m.content,''),r.summary) FROM butler_task_result r LEFT JOIN message m ON m.id=r.final_message_id
             WHERE r.task_conversation_id=?1",
            [task.id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let owner = db.conversation_repo()?.read(definition.butler_conversation_id)?;
    let butler_conversation_id = match owner {
        Some(owner) if is_current_butler_main_conversation(app, &owner).map_err(anyhow::Error::msg)? => owner.id,
        _ => load_or_create_butler_main_internal(app).await.map_err(anyhow::Error::msg)?.id,
    };
    let window = resolve_or_create_butler_execution_window(app).map_err(anyhow::Error::msg)?;
    let spawned = super::butler_api::spawn_butler_followup_with_window(
        app,
        &window,
        SpawnButlerTaskRequest {
            butler_conversation_id,
            title: format!("续：{}", definition.title),
            goal: followup_goal(&definition.title, &definition.goal, result.as_deref(), &followup.instruction),
            executor_assistant_id: Some(definition.executor_assistant_id),
            executor_assistant_name: None,
            handoff_contract_json: None,
            result_handling_mode: Some(RESULT_MODE_DIRECT_REPORT.into()),
            notification_policy: definition.notification_policy.clone(),
            // 不恢复原任务的临时授权。
            temporary_trusted_paths: Vec::new(),
            temporary_skill_identifiers: Vec::new(),
            followup_of: Some(task.id),
            link_chain: followup.chain,
        },
        followup.id,
    )
    .await
    .map_err(anyhow::Error::msg)?;
    Ok(("spawned".into(), Some(spawned.task_conversation_id)))
}

/// 送达排队的续接：同一任务严格按序；运行中的任务等空闲再送；已结束的建立关联后续任务。
pub(crate) async fn tick(app: &AppHandle, recovering: bool) -> anyhow::Result<bool> {
    let conn = connection(app)?;
    if recovering {
        work::recover_followups(&conn)?;
    }
    let pending = work::next_followups(&conn)?;
    drop(conn);
    for followup in pending {
        let target = work::followup_target(&connection(app)?, &followup)?;
        if super::butler_peer::busy(app, target).await? {
            continue;
        }
        if !work::claim_followup(&connection(app)?, followup.id)? {
            continue;
        }
        match process_followup(app, &followup).await {
            Ok((state, task)) => work::finish_followup(&connection(app)?, followup.id, &state, task, None)?,
            Err(error) => {
                let detail = format!("续接任务 #{} 失败：{error:#}", followup.task_conversation_id);
                tracing::warn!(followup = followup.id, error = %detail, "butler work follow-up failed");
                work::finish_followup(&connection(app)?, followup.id, "failed", None, Some(&detail))?;
            }
        }
        super::butler_peer::updated(app);
    }
    Ok(work::pending_followups(&connection(app)?)? > 0)
}

#[derive(Debug, Serialize)]
pub struct TaskQuestion {
    pub task_conversation_id: i64,
    pub task_title: String,
    /// 仍有执行在等待：在窗口里直接回答即回到原执行。
    pub request_id: Option<String>,
    /// 原执行已因应用重启等中断：回答会作为续接要求送回这项任务。
    pub call_id: Option<i64>,
    pub stale: bool,
    pub questions: Value,
    pub metadata: Value,
    pub history_key: Option<String>,
    pub answers: Option<HashMap<String, String>>,
    pub cancelled: bool,
    pub delivery_status: Option<String>,
}

fn executor_tasks(conn: &Connection, contact: i64) -> anyhow::Result<Vec<(i64, String)>> {
    let assistant: Option<i64> = conn
        .query_row(
            "SELECT assistant_id FROM conversation WHERE id=?1 AND conversation_kind='butler_contact'",
            [contact],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let assistant = assistant.ok_or_else(|| anyhow!("会话 #{contact} 不是管家联系人会话"))?;
    Ok(conn
        .prepare("SELECT d.task_conversation_id,d.title FROM butler_task_definition d WHERE d.executor_assistant_id=?1
                  ORDER BY d.id DESC LIMIT 50")?
        .query_map([assistant], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

/// 执行助手在独立任务里向用户提出、尚未回答的问题。
#[tauri::command]
pub async fn list_butler_contact_task_questions(app_handle: AppHandle, conversation_id: i64) -> Result<Vec<TaskQuestion>, String> {
    let tasks = executor_tasks(&connection(&app_handle).map_err(|e| e.to_string())?, conversation_id)
        .map_err(|e| format!("读取联系人 #{conversation_id} 的任务失败：{e:#}"))?;
    let interactions = app_handle.state::<InteractionState>();
    let activity = app_handle.state::<ConversationActivityManager>();
    let mcp = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for (task, title) in tasks {
        let mut uncertain_delivery = false;
        for record in crate::db::butler_question_history::list(&connection(&app_handle).map_err(|e| e.to_string())?, task)
            .map_err(|e| format!("读取任务 #{task} 问答记录失败：{e:#}"))? {
            uncertain_delivery |= record.delivery_status == "pending";
            result.push(TaskQuestion {
                task_conversation_id: task, task_title: title.clone(), request_id: None, call_id: None,
                stale: false, questions: record.questions, metadata: record.metadata,
                history_key: Some(record.key), answers: Some(record.answers), cancelled: record.cancelled, delivery_status: Some(record.delivery_status),
            });
        }
        let live = interactions.list_requests_for_conversation(task).await;
        for request in &live {
            result.push(TaskQuestion {
                task_conversation_id: task,
                task_title: title.clone(),
                request_id: Some(request.request_id.clone()),
                call_id: None,
                stale: false,
                questions: serde_json::to_value(&request.questions).unwrap_or_default(),
                metadata: serde_json::to_value(&request.metadata).unwrap_or_default(),
                history_key: None, answers: None, cancelled: false, delivery_status: None,
            });
        }
        // An unconfirmed persisted reply must not become a new continuation after restart.
        if uncertain_delivery || !live.is_empty() || activity.get_runtime_state(task).await.is_running {
            continue;
        }
        // 没有执行在等待、但提问调用仍未结束：原执行已中断（如应用重启），问题仍需回答。
        for call in mcp.get_mcp_tool_calls_by_conversation(task).map_err(|e| e.to_string())? {
            if call.tool_name != "ask_user_question" || !matches!(call.status.as_str(), "pending" | "executing") {
                continue;
            }
            let parameters: Value = serde_json::from_str(&call.parameters).unwrap_or_default();
            result.push(TaskQuestion {
                task_conversation_id: task,
                task_title: title.clone(),
                request_id: None,
                call_id: Some(call.id),
                stale: true,
                questions: parameters["questions"].clone(),
                metadata: parameters["metadata"].clone(),
                history_key: None, answers: None, cancelled: false, delivery_status: None,
            });
        }
    }
    Ok(result)
}

pub(crate) fn stale_answer_instruction(questions: &Value, answers: &HashMap<String, String>) -> String {
    let mut lines = vec!["[用户回答你在本任务中提出的问题 · 原执行已中断，改在会话中回答]".to_string()];
    if let Some(items) = questions.as_array() {
        for item in items {
            if let Some(question) = item["question"].as_str() {
                lines.push(format!("问：{question}"));
            }
        }
    }
    let mut keys: Vec<_> = answers.keys().collect();
    keys.sort();
    for key in keys {
        lines.push(format!("答（{key}）：{}", answers[key]));
    }
    lines.push("请据此继续完成任务。".into());
    lines.join("\n")
}

// The two databases cannot commit together. Serialize stale handoffs so the saved
// intent cannot be replaced by a competing answer/cancel before its MCP claim.
async fn stale_question_reply_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(())).lock().await
}

/// 回答原执行已中断的任务提问：关闭原提问调用，并把答案作为续接要求送回这项任务（运行中按序送达，已结束则建立关联后续任务）。
#[tauri::command]
pub async fn answer_butler_task_question(
    app_handle: AppHandle,
    conversation_id: i64,
    task_conversation_id: i64,
    call_id: i64,
    answers: HashMap<String, String>,
) -> Result<Followup, String> {
    let _stale_reply_guard = stale_question_reply_guard().await;
    let fail = |e: anyhow::Error| format!("回答任务 #{task_conversation_id} 的问题失败：{e:#}");
    let mut conn = connection(&app_handle).map_err(fail)?;
    crate::db::butler_question_history::validate_owner(&conn, conversation_id, task_conversation_id).map_err(fail)?;
    let mcp = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let call = mcp.get_mcp_tool_call(call_id).map_err(|e| format!("提问记录 #{call_id} 已删除：{e}"))?;
    if call.conversation_id != task_conversation_id || call.tool_name != "ask_user_question" {
        return Err(format!("提问记录 #{call_id} 不属于任务 #{task_conversation_id}"));
    }
    if !matches!(call.status.as_str(), "pending" | "executing") {
        return Err("问题已结束或已取消，请刷新后重试；未重新启动任务".into());
    }
    if answers.is_empty() { return Err("缺少回答内容".into()); }
    if !app_handle.state::<InteractionState>().list_requests_for_conversation(task_conversation_id).await.is_empty()
        || app_handle.state::<ConversationActivityManager>().get_runtime_state(task_conversation_id).await.is_running {
        return Err("任务仍在运行，请刷新后回答当前问题".into());
    }
    let parameters: Value = serde_json::from_str(&call.parameters).map_err(|e| format!("提问内容无法读取：{e}"))?;
    if parameters["questions"].as_array().is_some_and(|items| items.iter().any(|q|
        q["isSecret"].as_bool().unwrap_or(false) || q["is_secret"].as_bool().unwrap_or(false))) {
        return Err("敏感问题的原执行已中断，请让助手重新提问；不会将敏感答案保存到续接队列".into());
    }
    let instruction = stale_answer_instruction(&parameters["questions"], &answers);
    let chain = peer::task_link(&conn, task_conversation_id).map_err(|e| fail(e.into()))?.map(|(chain, _)| chain);
    crate::db::butler_question_history::save(&conn, task_conversation_id,
        crate::db::butler_question_history::AnswerRecord {
            key: format!("call:{call_id}"), questions: parameters["questions"].clone(),
            metadata: parameters["metadata"].clone(), answers: answers.clone(), cancelled: false, delivery_status: "pending".into(),
        }).map_err(|e| format!("保存问答记录失败，未加入续接队列：{e:#}"))?;
    // Persist the redacted intent before claiming the call. A crash at any later
    // point leaves a visible unconfirmed record, never a silently lost answer.
    crate::db::butler_question_history::claim_stale(&mcp.conn, task_conversation_id, call_id, false)
        .map_err(|e| format!("答案未加入续接队列：{e:#}。已保留待核对记录，请勿自动重试。"))?;
    let (followup, _) = work::enqueue_followup(
        &mut conn,
        conversation_id,
        task_conversation_id,
        &format!("answer:{call_id}"),
        &instruction,
        chain,
    )
    .map_err(|e| format!("问题已关闭，但答案未能加入续接队列：{e:#}。请核对任务状态，不要自动重试。"))?;
    if matches!(call.status.as_str(), "pending" | "executing") {
        mcp.update_mcp_tool_call_status(call_id, "failed", None, Some("原执行已中断，问题已改在执行助手会话中回答并作为续接要求送回任务"))
            .map_err(|e| e.to_string())?;
    }
    crate::db::butler_question_history::finish_delivery(&conn, task_conversation_id, &format!("call:{call_id}"))
        .map_err(|e| format!("答案已排队，但记录状态未确认，请勿重复提交：{e:#}"))?;
    super::butler_peer::wake();
    super::butler_peer::updated(&app_handle);
    Ok(followup)
}

/// Contact-window reply: bind both live and recovered questions to the current contact and task.
#[tauri::command]
pub async fn respond_butler_contact_task_question(
    app_handle: AppHandle, conversation_id: i64, task_conversation_id: i64,
    request_id: Option<String>, call_id: Option<i64>, answers: Option<HashMap<String, String>>, cancelled: bool,
) -> Result<(), String> {
    crate::db::butler_question_history::validate_owner(
        &connection(&app_handle).map_err(|e| e.to_string())?, conversation_id, task_conversation_id,
    ).map_err(|e| e.to_string())?;
    if request_id.is_some() == call_id.is_some() {
        return Err("必须指定唯一的任务问题".into());
    }
    if let Some(request_id) = request_id {
        let state = app_handle.state::<InteractionState>();
        let request = state.get_ask_user_request(&request_id).await.ok_or("问题已结束或已失效，请刷新后重试")?;
        if request.conversation_id != Some(task_conversation_id) {
            return Err("问题不属于当前任务，未提交答案".into());
        }
        let delivered = crate::mcp::builtin_mcp::interaction::resolve_ask_user_question_response(
            &app_handle, &request_id, answers.clone(), cancelled,
        ).await?;
        if !delivered { return Err("原执行已结束，答案未送达".into()); }
    } else if let Some(call_id) = call_id {
        if !cancelled {
            answer_butler_task_question(app_handle.clone(), conversation_id, task_conversation_id, call_id,
                answers.ok_or("缺少回答内容")?).await?;
        } else {
            let _stale_reply_guard = stale_question_reply_guard().await;
            if !app_handle.state::<InteractionState>().list_requests_for_conversation(task_conversation_id).await.is_empty()
                || app_handle.state::<ConversationActivityManager>().get_runtime_state(task_conversation_id).await.is_running {
                return Err("任务仍在运行，请刷新后取消当前问题".into());
            }
            let mcp = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
            let call = mcp.get_mcp_tool_call(call_id).map_err(|e| e.to_string())?;
            if call.conversation_id != task_conversation_id || call.tool_name != "ask_user_question" {
                return Err("问题不属于当前任务，未取消".into());
            }
            if !matches!(call.status.as_str(), "pending" | "executing") {
                return Err("问题已结束，请刷新后重试".into());
            }
            let params: Value = serde_json::from_str(&call.parameters).map_err(|e| e.to_string())?;
            let conn = connection(&app_handle).map_err(|e| e.to_string())?;
            crate::db::butler_question_history::save(&conn, task_conversation_id,
                crate::db::butler_question_history::AnswerRecord {
                    key: format!("call:{call_id}"), questions: params["questions"].clone(), metadata: params["metadata"].clone(),
                    answers: HashMap::new(), cancelled: true, delivery_status: "pending".into(),
                }).map_err(|e| format!("保存取消记录失败，尚未取消：{e:#}"))?;
            crate::db::butler_question_history::claim_stale(&mcp.conn, task_conversation_id, call_id, true)
                .map_err(|e| format!("未取消问题：{e:#}。已保留待核对记录。"))?;
            crate::db::butler_question_history::finish_delivery(&conn, task_conversation_id, &format!("call:{call_id}"))
                .map_err(|e| format!("问题已取消，但记录状态未确认，请勿重复提交：{e:#}"))?;
        }
    }
    super::butler_peer::updated(&app_handle);
    Ok(())
}

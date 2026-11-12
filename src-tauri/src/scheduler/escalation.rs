//! 无人值守运行中自动审核没通过的操作：不挂起等用户，转交总管家裁决。
//!
//! 总管家在主会话里收到一条裁决请求（附任务指令、待执行操作、审核意见和运行进展），
//! 结合它掌握的上下文用 `schedule_task` 的 `decide` 给出允许或拒绝。等待有上限；
//! 没有总管家、总管家迟迟不处理或运行被停止时按拒绝处理，并把原因交给运行中的模型。
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::Manager;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::api::ai::types::AiRequest;
use crate::api::ai_api::{add_message, ask_ai};
use crate::api::butler_api::{
    active_butler_main_conversation_id, get_butler_main_continuation_lock, is_butler_main_busy_error,
    resolve_or_create_butler_execution_window, wait_for_butler_main_to_be_idle, ButlerMainBusy,
};
use crate::db::conversation_db::{ConversationDatabase, Repository};

/// 等待总管家裁决的上限。
pub const DECISION_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const BUSY_RETRY: Duration = Duration::from_secs(5);
const PROMPT_LIMIT: usize = 2000;
const PARAMS_LIMIT: usize = 3000;
const PROGRESS_LIMIT: usize = 1500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow(String),
    Deny(String),
}

struct Pending {
    sender: oneshot::Sender<Decision>,
    task_name: String,
}

fn pending() -> &'static Mutex<HashMap<String, Pending>> {
    static PENDING: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 一次需要裁决的操作。
#[derive(Debug, Clone)]
pub struct Request {
    pub task_id: i64,
    pub task_name: String,
    pub task_prompt: String,
    pub assistant_name: String,
    pub run_id: String,
    pub server_name: String,
    pub tool_name: String,
    pub parameters: String,
    pub review_reason: String,
    pub progress: String,
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}…（已截断）")
}

pub(crate) fn request_message(id: &str, request: &Request) -> String {
    format!(
        "<scheduled_task_approval request_id=\"{id}\">\n定时任务《{name}》（#{task_id}，执行助手：{assistant}，运行 {run_id}）正在无人值守运行，想执行下面这个操作，自动审核没有通过，需要你裁决。\n\n任务指令（用户交代的事，代表用户授权的范围）：\n{prompt}\n\n待执行操作：{server} / {tool}\n参数：\n{params}\n\n自动审核意见：{reason}\n\n运行进展：\n{progress}\n</scheduled_task_approval>",
        name = request.task_name,
        task_id = request.task_id,
        assistant = request.assistant_name,
        run_id = request.run_id,
        prompt = truncate(&request.task_prompt, PROMPT_LIMIT),
        server = request.server_name,
        tool = request.tool_name,
        params = truncate(&request.parameters, PARAMS_LIMIT),
        reason = request.review_reason,
        progress = if request.progress.trim().is_empty() { "（还没有输出）".to_string() } else { truncate(&request.progress, PROGRESS_LIMIT) },
    )
}

pub(crate) fn request_prompt(id: &str, task_name: &str) -> String {
    format!(
        "定时任务《{task_name}》运行中有一个操作等你裁决（request_id={id}，详情见上面的 scheduled_task_approval）。用户不在场，不要向用户提问。\
请结合任务指令和你掌握的上下文判断：在用户交代的范围内、目标明确且风险可控就允许，超出范围或风险不清就拒绝。\
用 schedule_task 的 decide 操作给出结论（request_id、decision=allow 或 deny、reason 写一句理由），然后简短说明即可。"
    )
}

/// 总管家调用 `schedule_task.decide` 时交付裁决。
pub fn decide(id: &str, allow: bool, reason: &str) -> Result<String, String> {
    let entry = pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id)
        .ok_or_else(|| format!("裁决请求 {id} 不存在或已结束（可能已超时或运行已停止）"))?;
    let reason = if reason.trim().is_empty() { "总管家未说明理由".to_string() } else { reason.trim().to_string() };
    let decision = if allow { Decision::Allow(reason) } else { Decision::Deny(reason) };
    entry
        .sender
        .send(decision)
        .map_err(|_| format!("定时任务《{}》的运行已结束，裁决未生效", entry.task_name))?;
    Ok(format!("已{}定时任务《{}》的操作", if allow { "允许" } else { "拒绝" }, entry.task_name))
}

struct PendingGuard(String);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        pending().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

/// 转交总管家裁决并等待结果。没有总管家主会话时立即拒绝。
pub async fn escalate(app: &tauri::AppHandle, request: Request, cancel: &CancellationToken) -> Decision {
    let main_id = match active_butler_main_conversation_id(app) {
        Ok(Some(id)) => id,
        Ok(None) => return Decision::Deny("自动审核未通过，且当前没有总管家主会话可以转交裁决".to_string()),
        Err(error) => return Decision::Deny(format!("自动审核未通过，读取总管家主会话失败：{error}")),
    };
    let id = format!("sched-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let (sender, receiver) = oneshot::channel();
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id.clone(), Pending { sender, task_name: request.task_name.clone() });
    let _guard = PendingGuard(id.clone());

    let wake = {
        let app = app.clone();
        let id = id.clone();
        let request = request.clone();
        let cancel = cancel.clone();
        tauri::async_runtime::spawn(async move {
            let mut injected = false;
            loop {
                if cancel.is_cancelled() {
                    return;
                }
                match wake_butler(&app, main_id, &id, &request, &mut injected).await {
                    Ok(()) => return,
                    Err(error) if is_butler_main_busy_error(&error) => tokio::time::sleep(BUSY_RETRY).await,
                    Err(error) => {
                        warn!(request_id = %id, error = %error, "failed to hand scheduled approval to butler");
                        if let Some(entry) = pending().lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                            let _ = entry.sender.send(Decision::Deny(format!("自动审核未通过，转交总管家失败：{error}")));
                        }
                        return;
                    }
                }
            }
        })
    };

    let decision = tokio::select! {
        biased;
        _ = cancel.cancelled() => Decision::Deny("运行已停止，未执行该操作".to_string()),
        result = tokio::time::timeout(DECISION_TIMEOUT, receiver) => match result {
            Ok(Ok(decision)) => decision,
            Ok(Err(_)) => Decision::Deny("裁决请求已失效，未执行该操作".to_string()),
            Err(_) => Decision::Deny(format!("总管家未在 {} 分钟内裁决，按拒绝处理", DECISION_TIMEOUT.as_secs() / 60)),
        },
    };
    wake.abort();
    decision
}

/// 在总管家主会话里注入裁决请求并开始一轮内部回合（与用户回合互斥，主会话忙时返回忙碌错误）。
async fn wake_butler(app: &tauri::AppHandle, main_id: i64, id: &str, request: &Request, injected: &mut bool) -> Result<(), String> {
    let lock = get_butler_main_continuation_lock(main_id).await;
    let _guard = lock.lock().await;
    wait_for_butler_main_to_be_idle(app, main_id).await.map_err(ButlerMainBusy::into_error_string)?;
    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    let conversation = db
        .conversation_repo()
        .map_err(|e| e.to_string())?
        .read(main_id)
        .map_err(|e| e.to_string())?
        .ok_or("总管家主会话不存在")?;
    let assistant_id = conversation.assistant_id.ok_or("总管家主会话缺少助手")?;
    if !*injected {
        add_message(app, None, main_id, "system".to_string(), request_message(id, request), None, None, None, None, 0, None, None)
            .map_err(|e| e.to_string())?;
        *injected = true;
    }
    let window = resolve_or_create_butler_execution_window(app)?;
    let ai_request = AiRequest {
        conversation_id: main_id.to_string(),
        assistant_id,
        prompt: request_prompt(id, &request.task_name),
        model: None,
        override_model_id: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: Some(true),
        attachment_list: None,
        quote: None,
        butler_intent: None,
        butler_goal_revision: None,
    };
    ask_ai(
        app.clone(),
        app.state::<crate::AppState>(),
        app.state::<crate::AcpSessionState>(),
        app.state::<crate::FeatureConfigState>(),
        app.state::<crate::state::message_token::MessageTokenManager>(),
        app.state::<crate::state::activity_state::ConversationActivityManager>(),
        window,
        ai_request,
        None,
        None,
        None,
        None,
        Some("internal".to_string()),
    )
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "escalation_tests.rs"]
mod tests;

use super::{conversations, host::Host, store, types::*};
use crate::{
    api::{ai::types::AiRequest, ai_api, butler_api},
    db::conversation_db::ConversationDatabase,
    state::activity_state::ConversationActivityManager,
};
use anyhow::{ensure, Context, Result};
use futures::FutureExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock, Weak},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use uuid::Uuid;

static PROCESS: LazyLock<String> = LazyLock::new(|| Uuid::new_v4().to_string());
// Only conversations with a remote turn need an owner. Entries expire on the next
// desktop send/regeneration. The lock also prevents a stale stop overtaking it.
type TurnOwner = Arc<Mutex<Option<Uuid>>>;
static OWNERS: LazyLock<std::sync::Mutex<HashMap<i64, TurnOwner>>> =
    LazyLock::new(Default::default);
static WATCHERS: LazyLock<std::sync::Mutex<HashMap<i64, Weak<tokio::sync::Notify>>>> =
    LazyLock::new(Default::default);
pub fn changed(id: i64) {
    let watchers = WATCHERS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(signal) = watchers.get(&id).and_then(Weak::upgrade) {
        signal.notify_waiters();
    }
}
pub async fn wait_for_change(id: i64) {
    let signal = {
        let mut watchers = WATCHERS.lock().unwrap_or_else(|e| e.into_inner());
        watchers.retain(|_, v| v.strong_count() > 0);
        if let Some(v) = watchers.get(&id).and_then(Weak::upgrade) {
            v
        } else {
            let v = Arc::new(tokio::sync::Notify::new());
            watchers.insert(id, Arc::downgrade(&v));
            v
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(3), signal.notified()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
}
struct Scope {
    device: String,
    destination: Destination,
    turn: Uuid,
}
tokio::task_local! { static REMOTE: Scope; }
static STARTING: LazyLock<std::sync::Mutex<HashSet<i64>>> = LazyLock::new(Default::default);
struct Starting(Option<i64>);
impl Starting {
    fn claim(destination: &Destination) -> Result<Self> {
        if let Destination::Existing { conversation } = destination {
            ensure!(
                STARTING.lock().unwrap_or_else(|e| e.into_inner()).insert(*conversation),
                "电脑正在接收这条对话的另一条消息，请稍后再发送"
            );
            Ok(Self(Some(*conversation)))
        } else {
            Ok(Self(None))
        }
    }
}
impl Drop for Starting {
    fn drop(&mut self) {
        if let Some(id) = self.0 {
            STARTING.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        }
    }
}

pub fn is_remote() -> bool {
    REMOTE.try_with(|_| ()).is_ok()
}

pub async fn begin_turn(id: i64) {
    let turn = REMOTE.try_with(|scope| scope.turn).ok();
    let slot = {
        let mut owners = OWNERS.lock().unwrap_or_else(|e| e.into_inner());
        if turn.is_some() {
            owners.entry(id).or_default().clone()
        } else if let Some(slot) = owners.get(&id) {
            slot.clone()
        } else {
            return;
        }
    };
    let mut current = slot.lock().await;
    *current = turn;
    if turn.is_none() {
        release_owner(id, &slot);
    }
}

fn release_owner(id: i64, slot: &TurnOwner) {
    let mut owners = OWNERS.lock().unwrap_or_else(|e| e.into_inner());
    if Arc::strong_count(slot) == 2
        && owners.get(&id).is_some_and(|current| Arc::ptr_eq(current, slot))
    {
        owners.remove(&id);
    }
}

fn authorize(app: &AppHandle, grant: &Access, destination: &Destination) -> Result<(String, i64)> {
    ensure!(grant.chat, "手机没有对话权限");
    match destination {
        Destination::New { assistant } => {
            ensure!(grant.assistants.contains(assistant), "此助手未授权");
            Ok(("normal".into(), *assistant))
        }
        Destination::Existing { conversation } => conversations::target(
            &ConversationDatabase::new(app)?.get_connection()?,
            grant,
            *conversation,
        ),
    }
}

async fn current_grant(app: &AppHandle, device: &str) -> Result<Access> {
    ensure!(app.state::<Host>().0.lock().await.is_some(), "电脑已关闭设备互联");
    let raw: String = store::open(app)?
        .query_row("SELECT access FROM host_bridge_peer WHERE id=?1", [device], |r| r.get(0))
        .optional()?
        .context("手机配对已撤销")?;
    let grant: Access = serde_json::from_str(&raw)?;
    ensure!(grant.chat, "电脑已取消这部手机的对话权限");
    Ok(grant)
}

pub async fn check_before_execution(app: &AppHandle, assistant: i64) -> Result<()> {
    let Ok((device, destination)) = REMOTE.try_with(|s| (s.device.clone(), s.destination.clone()))
    else {
        return Ok(());
    };
    ensure!(
        authorize(app, &current_grant(app,&device).await?, &destination)?.1 == assistant,
        "对话的执行助手已改变，请重新打开"
    );
    Ok(())
}

pub(super) fn reserve(
    db: &Connection,
    device: &str,
    request: Uuid,
    fingerprint: &[u8],
) -> Result<Option<Uuid>> {
    let turn = Uuid::new_v4();
    let inserted = db.execute(
        "INSERT INTO host_bridge_send(device,request,turn,fingerprint,process,state)
        VALUES(?1,?2,?3,?4,?5,'pending') ON CONFLICT(device,request) DO NOTHING",
        params![device, request.to_string(), turn.to_string(), fingerprint, *PROCESS],
    )?;
    if inserted == 1 {
        return Ok(Some(turn));
    }
    let saved: Vec<u8> = db.query_row(
        "SELECT fingerprint FROM host_bridge_send WHERE device=?1 AND request=?2",
        params![device, request.to_string()],
        |r| r.get(0),
    )?;
    ensure!(saved == fingerprint, "这个请求编号已经用于另一条消息");
    Ok(None)
}

struct Receipt {
    turn: Uuid,
    state: String,
    conversation: Option<i64>,
    message: Option<i64>,
    error: Option<String>,
}
fn receipt(app: &AppHandle, device: &str, request: Uuid) -> Result<Receipt> {
    let (turn,process,state,mut conversation,mut message,error): (String,String,String,Option<i64>,Option<i64>,Option<String>) =
        store::open(app)?.query_row("SELECT turn,process,state,conversation,message,error FROM host_bridge_send WHERE device=?1 AND request=?2",
            params![device,request.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))
            .optional()?.context("电脑尚未收到此请求")?;
    let mut state = state;
    let mut error = error;
    if state == "pending" && process != *PROCESS {
        let db = ConversationDatabase::new(app)?.get_connection()?;
        let result = db.query_row("SELECT conversation_id,id FROM message WHERE message_type='user'
            AND CASE WHEN json_valid(metadata_json) THEN json_extract(metadata_json,'$.client_request_id')=?1 ELSE 0 END LIMIT 1",
            [&turn],|r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((c, m)) = result {
            conversation = Some(c);
            message = Some(m);
        }
        state = "failed".into();
        error = Some("电脑在确认执行结果前重启，请检查原对话。此请求不会再次执行。".into());
    }
    Ok(Receipt { turn: turn.parse()?, state, conversation, message, error })
}

fn execution_window(app: &AppHandle, kind: &str) -> Result<tauri::Window> {
    if kind != "normal" {
        return butler_api::resolve_or_create_butler_execution_window(app)
            .map_err(anyhow::Error::msg);
    }
    if app.get_webview_window("chat_ui").is_none() {
        crate::window::create_chat_ui_window_hidden(app);
    }
    Ok(app.get_webview_window("chat_ui").context("无法创建桌面聊天执行窗口")?.as_ref().window())
}

async fn deliver(
    app: &AppHandle,
    device: String,
    destination: Destination,
    kind: &str,
    assistant: i64,
    turn: Uuid,
    text: String,
) -> Result<(i64, Option<i64>, Option<String>)> {
    let _starting = Starting::claim(&destination)?;
    let actual = authorize(app, &current_grant(app,&device).await?, &destination)?;
    ensure!(actual.0 == kind && actual.1 == assistant, "目标会话已改变，请重新打开");
    let id = match &destination {
        Destination::New { .. } => String::new(),
        Destination::Existing { conversation } => conversation.to_string(),
    };
    let model = if kind == "butler_main" {
        Some(
            butler_api::get_butler_model_selection(app)
                .await
                .map_err(anyhow::Error::msg)?
                .raw_value,
        )
    } else {
        None
    };
    let window = execution_window(app, kind)?;
    let ai = AiRequest {
        conversation_id: id,
        assistant_id: assistant,
        prompt: text,
        model: None,
        override_model_id: model,
        stream: Some(true),
        temperature: None,
        top_p: None,
        max_tokens: None,
        attachment_list: None,
        quote: None,
        butler_intent: None,
        client_request_id: Some(turn),
        butler_goal_revision: None,
    };
    let result = REMOTE
        .scope(
            Scope { device, destination, turn },
            ai_api::ask_ai(
                app.clone(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                app.state(),
                window,
                ai,
                None,
                None,
                None,
                None,
                None,
            ),
        )
        .await
        .map_err(|e| {
            use crate::errors::AppError;
            let reason = match &e {
                AppError::NoModelFound => "助手没有可用的模型配置",
                AppError::DatabaseError(_) => "读取或写入桌面会话数据库失败",
                AppError::ProviderError(_) => "模型或原生通道启动失败",
                AppError::UnknownError(text)
                    if text == "电脑正在回复，请等待或停止本手机发起的回复后再发送"
                        || text == "手机配对已撤销"
                        || text == "电脑已关闭设备互联"
                        || text.starts_with("插件改变了已确认") =>
                {
                    text
                }
                _ => "桌面准备或启动对话失败",
            };
            // The normal desktop execution path owns the detailed diagnostics. Don't
            // serialize arbitrary plugin output, model HTTP bodies or agent stderr.
            anyhow::anyhow!("{reason}（助手 #{assistant}，请求 {turn}）")
        })?;
    let error = result.error_message.map(|_| {
        format!("桌面启动模型失败，请查看电脑会话 #{} 的错误记录", result.conversation_id)
    });
    ensure!(
        result.user_message_id.is_some(),
        "桌面未返回用户消息编号，请核对电脑会话 #{}（请求 {turn}）",
        result.conversation_id
    );
    Ok((result.conversation_id, result.user_message_id, error))
}

pub async fn handle(
    app: &AppHandle,
    device: String,
    action: ChatAction,
) -> Result<Value> {
    let grant = current_grant(app,&device).await?;
    match action {
        ChatAction::OpenButler { assistant } => {
            ensure!(grant.butler, "管家未授权");
            let id = if let Some(id) = assistant {
                ensure!(grant.assistants.contains(&id), "此联系人未授权");
                butler_api::open_butler_contact(app.clone(), id)
                    .await
                    .map_err(anyhow::Error::msg)?
            } else {
                butler_api::load_or_create_butler_main_internal(app)
                    .await
                    .map_err(anyhow::Error::msg)?
                    .id
            };
            Ok(json!({"conversation":id}))
        }
        ChatAction::Send { request, destination, text } => {
            ensure!(
                !text.trim().is_empty() && text.len() <= 32 * 1024,
                "消息须在 1 至 32768 字节之间"
            );
            let (kind, assistant) = authorize(app, &grant, &destination)?;
            let fingerprint = Sha256::digest(serde_json::to_vec(&(&destination, &text))?);
            if let Some(turn) = reserve(&store::open(app)?, &device, request, &fingerprint)? {
                let app = app.clone();
                // Once reserved, the request survives a dropped HTTP connection.
                tauri::async_runtime::spawn(async move {
                    let result = std::panic::AssertUnwindSafe(deliver(
                        &app,
                        device,
                        destination,
                        &kind,
                        assistant,
                        turn,
                        text,
                    ))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| {
                        Err(anyhow::anyhow!(
                            "桌面对话初始化异常退出（请求 {turn}），请检查原会话及桌面日志"
                        ))
                    });
                    let (id, message, error) = match result {
                        Ok((id, msg, error)) => (Some(id), msg, error),
                        Err(e) => (None, None, Some(format!("{e:#}"))),
                    };
                    let state = if error.is_some() { "failed" } else { "accepted" };
                    if let Err(e) = store::open(&app).and_then(|db| Ok(db.execute(
                        "UPDATE host_bridge_send SET state=?1,conversation=coalesce(?2,conversation),message=coalesce(?3,message),error=?4 WHERE turn=?5",
                        params![state,id,message,error,turn.to_string()])?)) {
                        tracing::error!(%turn,error=%e,"持久化手机发送回执失败");
                    }
                });
            }
            Ok(json!({"request":request}))
        }
        ChatAction::Receipt { request } => {
            let r = receipt(app, &device, request)?;
            if let Some(id) = r.conversation {
                authorize(app, &grant, &Destination::Existing { conversation: id })?;
            }
            Ok(
                json!({"state":r.state,"conversation":r.conversation,"message":r.message,"error":r.error}),
            )
        }
        ChatAction::Stop { request } => {
            let r = receipt(app, &device, request)?;
            ensure!(r.state == "accepted", "电脑尚未确认本轮已启动，或本轮启动失败");
            let id = r.conversation.context("电脑尚未确认本轮会话")?;
            let (kind, _) = authorize(app, &grant, &Destination::Existing { conversation: id })?;
            let slot = OWNERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&id)
                .cloned()
                .context("这轮回复已经结束")?;
            let mut owner = slot.lock().await;
            ensure!(*owner == Some(r.turn), "这轮回复已结束或被其他消息接替");
            let latest: Option<i64> = ConversationDatabase::new(app)?.get_connection()?.query_row(
                "SELECT max(id) FROM message WHERE conversation_id=?1 AND message_type='user'",
                [id],
                |r| r.get(0),
            )?;
            ensure!(latest.is_some() && latest == r.message, "电脑已切换到另一轮回复");
            ensure!(
                app.state::<ConversationActivityManager>().get_runtime_state(id).await.is_running,
                "电脑上的回复已经结束"
            );
            let window = execution_window(app, &kind)?;
            authorize(app,&current_grant(app,&device).await?,&Destination::Existing {conversation:id})?;
            ai_api::cancel_ai(app.clone(), app.state(), app.state(), app.state(), window, id)
                .await
                .map_err(anyhow::Error::msg)?;
            *owner = None;
            release_owner(id, &slot);
            Ok(json!({"stopped":true}))
        }
    }
}

pub async fn bind_message(
    app: &AppHandle,
    conversation: i64,
    message: i64,
    new_chat: bool,
) -> Result<()> {
    let Ok(turn) = REMOTE.try_with(|s| s.turn) else { return Ok(()) };
    if new_chat {
        begin_turn(conversation).await;
        let _ = app.emit("conversation_created", conversation);
    }
    store::open(app)?.execute(
        "UPDATE host_bridge_send SET conversation=?1,message=?2 WHERE turn=?3",
        params![conversation, message, turn.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;

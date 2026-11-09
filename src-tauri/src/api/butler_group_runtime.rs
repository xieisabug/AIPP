//! 群聊调度：判断谁来回复，按顺序启动成员回复，并在成员会话里确认最终回复后发回群里。
//!
//! 每位成员一次只处理一项回复；成员回复走与私聊相同的 `ask_ai` 通道（原生通道不降级、不换会话）。
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::butler_group::{changed, database, roster, sync_roster, RosterEntry};
use crate::db::butler_group::{self as group, Evidence, Outcome};

static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub fn wake() {
    WAKE.notify_one();
}

/// 执行已经空闲、却还找不到最终回复时，再等这么久才判定失败（工具轮次之间会有短暂空闲）。
const IDLE_GRACE: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Runtime {
    routing: HashSet<(i64, i64)>,
    dispatching: HashSet<i64>,
    idle: HashMap<i64, Instant>,
}

type Shared = Arc<Mutex<Runtime>>;

fn lock(state: &Shared) -> MutexGuard<'_, Runtime> {
    state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 调度任务结束（包括出错或 panic）时释放占位，避免同一条消息或同一次回复被永久卡住。
struct Release {
    state: Shared,
    routing: Option<(i64, i64)>,
    turn: Option<i64>,
}

impl Drop for Release {
    fn drop(&mut self) {
        let mut runtime = lock(&self.state);
        if let Some(key) = self.routing {
            runtime.routing.remove(&key);
        }
        if let Some(turn) = self.turn {
            runtime.dispatching.remove(&turn);
        }
        drop(runtime);
        wake();
    }
}

/// 成员是否仍在工作：模型或工具在执行、在等用户回答或审批，或原生通道仍有进行中的轮次。
async fn busy(app: &AppHandle, conversation: i64) -> anyhow::Result<bool> {
    if super::butler_peer::busy(app, conversation).await? {
        return Ok(true);
    }
    if super::ai::agent_driver::lifecycle::session_binding(app, conversation)?.is_some() {
        if let Some(session) = super::ai::agent_driver::lifecycle::lookup_session(app, conversation).await? {
            return Ok(session.snapshot["has_active_prompt"].as_bool() == Some(true));
        }
    }
    Ok(false)
}

async fn settle(app: &AppHandle, state: &Shared, turn: group::Turn) -> anyhow::Result<()> {
    if lock(state).dispatching.contains(&turn.id) {
        return Ok(());
    }
    let snapshot = turn.clone();
    let (evidence, conversation) = database(app, "确认回复", move |conn| {
        Ok((group::evidence(conn, &snapshot)?, group::member(conn, snapshot.member_id)?.conversation_id))
    }).await.map_err(anyhow::Error::msg)?;
    let working = busy(app, conversation).await?;
    let outcome = match evidence {
        Evidence::Failed(reason) => Some(Outcome::Failed(reason)),
        Evidence::Answer(body) if !working => Some(Outcome::Reply(body)),
        Evidence::Silent if !working => Some(Outcome::Silent),
        _ if working => {
            lock(state).idle.remove(&turn.id);
            None
        }
        _ => {
            let since = *lock(state).idle.entry(turn.id).or_insert_with(Instant::now);
            (since.elapsed() >= IDLE_GRACE).then(|| Outcome::Failed(format!(
                "执行已经结束，但没有找到可以发到群里的最终回复（会话 #{conversation}，回复 #{}）；没有自动重试", turn.id)))
        }
    };
    let Some(outcome) = outcome else { return Ok(()) };
    lock(state).idle.remove(&turn.id);
    let id = turn.id;
    let finished = database(app, "公开回复", move |conn| group::finish_turn(conn, id, outcome)).await.map_err(anyhow::Error::msg)?;
    if finished.is_some() {
        changed(app, turn.group_id);
    }
    Ok(())
}

async fn decide(app: &AppHandle, message: &group::Message) -> Result<Vec<i64>, String> {
    let roster = roster(app).await?;
    sync_roster(app, message.group_id, &roster).await?;
    let (group_id, seq, reply_to) = (message.group_id, message.seq, message.reply_to);
    let (room, members, recent, quoted) = database(app, "读取调度资料", move |conn| {
        Ok((group::get(conn, group_id)?, group::active_members(conn, group_id)?,
            group::messages(conn, group_id, Some(seq + 1), 30)?,
            reply_to.map(|s| group::message(conn, group_id, s)).transpose()?))
    }).await?;
    if members.is_empty() {
        return Err("群里已经没有成员，请先邀请助手".into());
    }
    let mentioned = group::parse_mentions(&message.body, &members);
    if !mentioned.is_empty() {
        return Ok(mentioned);
    }
    if let Some(author) = quoted.and_then(|q| q.sender_member).filter(|id| members.iter().any(|m| m.id == *id)) {
        return Ok(vec![author]);
    }
    if members.len() == 1 {
        return Ok(vec![members[0].id]);
    }
    super::butler_group_router::choose(app, group_id, &room.name, &members, &roster, &recent, message).await
}

fn route(app: &AppHandle, state: &Shared, message: group::Message) {
    let key = (message.group_id, message.seq);
    if !lock(state).routing.insert(key) {
        return;
    }
    let app = app.clone();
    let release = Release { state: state.clone(), routing: Some(key), turn: None };
    tauri::async_runtime::spawn(async move {
        let _release = release;
        let (group_id, seq) = key;
        let result = match decide(&app, &message).await {
            Ok(responders) => database(&app, "安排回复", move |conn| group::route(conn, group_id, seq, &responders).map(|_| ())).await,
            Err(reason) => Err(reason),
        };
        if let Err(reason) = result {
            if let Err(error) = database(&app, "记录调度失败", move |conn| group::fail_route(conn, group_id, seq, &reason).map(|_| ())).await {
                tracing::error!(group_id, seq, %error, "failed to record group routing failure");
            }
        }
        changed(&app, group_id);
    });
}

/// 每轮附带的群信息：当前成员名单只随本轮输入发给模型，不写进成员会话的历史。
fn context_prefix(room: &group::Group, me: &group::Member, members: &[group::Member], roster: &HashMap<i64, RosterEntry>) -> String {
    let names = members.iter().map(|m| {
        if m.id == me.id {
            format!("{}（你）", m.name)
        } else {
            let description = roster.get(&m.assistant_id).map(|e| e.description.trim().chars().take(40).collect::<String>())
                .filter(|d| !d.is_empty());
            match description {
                Some(description) => format!("{}（{description}）", m.name),
                None => m.name.clone(),
            }
        }
    }).collect::<Vec<_>>().join("、");
    format!("<group_chat name=\"{}\" you=\"{}\">\n当前成员：{names}\n</group_chat>", room.name, me.name)
}

async fn dispatch(app: &AppHandle, turn: &group::Turn, member: &group::Member, room: &group::Group, input: String) -> Result<(), String> {
    let roster = roster(app).await?;
    let group_id = room.id;
    let members = database(app, "读取成员", move |conn| group::active_members(conn, group_id)).await?;
    let prefix = context_prefix(room, member, &members, &roster);
    let window = super::butler_api::resolve_or_create_butler_execution_window(app)?;
    let result = super::ai_api::ask_ai(app.clone(), app.state(), app.state(), app.state(), app.state(), app.state(), window,
        super::ai::types::AiRequest {
            conversation_id: member.conversation_id.to_string(), assistant_id: member.assistant_id, prompt: input,
            model: None, override_model_id: None, temperature: None, top_p: None, max_tokens: None, stream: Some(true),
            attachment_list: None, quote: None, butler_intent: None, butler_goal_revision: None,
        },
        None, None, None, Some(prefix), Some(format!("butler_group/{}", turn.id))).await
        .map_err(|e| format!("{e}"))?;
    match result.error_message {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn start(app: &AppHandle, state: &Shared, turn: group::Turn) -> anyhow::Result<()> {
    if lock(state).dispatching.contains(&turn.id) {
        return Ok(());
    }
    let member_id = turn.member_id;
    let conversation = database(app, "读取成员", move |conn| Ok(group::member(conn, member_id)?.conversation_id))
        .await.map_err(anyhow::Error::msg)?;
    // 上一轮被停止后执行可能还在收尾，等它真正空闲再开始，避免与旧执行交错。
    if busy(app, conversation).await? {
        return Ok(());
    }
    let id = turn.id;
    let started = database(app, "开始回复", move |conn| group::start_turn(conn, id)).await.map_err(anyhow::Error::msg)?;
    let group::Start::Run { turn, member, group: room, input } = started else {
        changed(app, turn.group_id);
        return Ok(());
    };
    lock(state).dispatching.insert(turn.id);
    changed(app, room.id);
    let app = app.clone();
    let release = Release { state: state.clone(), routing: None, turn: Some(turn.id) };
    tauri::async_runtime::spawn(async move {
        let _release = release;
        if let Err(error) = dispatch(&app, &turn, &member, &room, input).await {
            let id = turn.id;
            let reason = format!("{error}（会话 #{}，回复 #{id}）", member.conversation_id);
            if let Err(save) = database(&app, "记录回复失败", move |conn| group::finish_turn(conn, id, Outcome::Failed(reason)).map(|_| ())).await {
                tracing::error!(turn = id, error = %save, "failed to record group dispatch failure");
            }
            changed(&app, room.id);
        }
    });
    Ok(())
}

async fn tick(app: &AppHandle, state: &Shared) -> anyhow::Result<()> {
    // 关闭管家只阻止新的回复；已经开始的回复照常结算。
    for turn in database(app, "读取进行中的回复", |conn| group::running_turns(conn)).await.map_err(anyhow::Error::msg)? {
        let id = turn.id;
        if let Err(error) = settle(app, state, turn).await {
            tracing::warn!(turn = id, error = %format!("{error:#}"), "group turn settle failed; will retry");
        }
    }
    if super::butler_api::ensure_butler_enabled(app).await.is_err() {
        return Ok(());
    }
    for message in database(app, "读取待调度消息", |conn| group::pending_routes(conn)).await.map_err(anyhow::Error::msg)? {
        route(app, state, message);
    }
    for turn in database(app, "读取待开始的回复", |conn| group::runnable_turns(conn)).await.map_err(anyhow::Error::msg)? {
        let id = turn.id;
        if let Err(error) = start(app, state, turn).await {
            tracing::warn!(turn = id, error = %format!("{error:#}"), "group turn start failed; will retry");
        }
    }
    Ok(())
}

pub fn start_runtime(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match database(&app, "恢复", group::recover).await {
            Ok(groups) => groups.into_iter().for_each(|id| changed(&app, id)),
            Err(error) => tracing::error!(%error, "group recovery failed; unfinished replies will be settled by the dispatcher"),
        }
        let state = Shared::default();
        loop {
            if let Err(error) = tick(&app, &state).await {
                tracing::error!(error = %format!("{error:#}"), "group dispatch tick failed");
            }
            tokio::select! {
                _ = WAKE.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    });
}

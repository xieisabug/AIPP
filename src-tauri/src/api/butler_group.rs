//! 群聊命令（本地用户界面调用）。成员的执行由 `butler_group_runtime` 调度。
use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::db::{assistant_db::AssistantDatabase, butler_group as group, conversation_db::ConversationDatabase};

fn desktop() -> Result<(), String> {
    if !cfg!(desktop) {
        return Err("群聊仅在桌面管家中提供".into());
    }
    Ok(())
}

async fn enabled(app: &AppHandle) -> Result<(), String> {
    desktop()?;
    super::butler_api::ensure_butler_enabled(app).await
}

pub(super) async fn database<T, F>(app: &AppHandle, operation: &'static str, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut rusqlite::Connection) -> anyhow::Result<T> + Send + 'static,
{
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let db = ConversationDatabase::new(&app)?;
        let mut conn = db.get_connection()?;
        work(&mut conn)
    })
    .await
    .map_err(|e| format!("群聊{operation}调度失败：{e}"))?
    .map_err(|e| format!("群聊{operation}失败：{e:#}"))
}

/// 通知界面刷新，并唤醒调度。
pub(super) fn changed(app: &AppHandle, group_id: i64) {
    let _ = app.emit("butler_group_changed", serde_json::json!({ "group_id": group_id }));
    super::butler_group_runtime::wake();
}

/// 助手名册：总管家使用配置的显示名。
#[derive(Clone)]
pub(super) struct RosterEntry {
    pub name: String,
    pub description: String,
    pub chief: bool,
}

pub(super) async fn roster(app: &AppHandle) -> Result<HashMap<i64, RosterEntry>, String> {
    let label = super::butler_api::get_experimental_config_value(app, "butler_display_name")
        .await
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "总管家".into());
    let app = app.clone();
    tokio::task::spawn_blocking(move || -> Result<HashMap<i64, RosterEntry>, String> {
        let assistants = AssistantDatabase::new(&app).map_err(|e| format!("读取助手列表失败：{e}"))?
            .get_assistants().map_err(|e| format!("读取助手列表失败：{e}"))?;
        Ok(assistants.into_iter().map(|a| {
            let chief = super::butler_api::is_butler_system_assistant(&a);
            let entry = RosterEntry {
                name: if chief { label.clone() } else { a.name.clone() },
                description: if chief { "总管家，熟悉整个助手团队".into() } else { a.description.clone().unwrap_or_default() },
                chief,
            };
            (a.id, entry)
        }).collect())
    })
    .await
    .map_err(|e| format!("读取助手列表任务失败：{e}"))?
}

/// 让成员名单跟随助手变化：改名同步显示名；助手已删除的成员移出群聊。
pub(super) async fn sync_roster(app: &AppHandle, group_id: i64, roster: &HashMap<i64, RosterEntry>) -> Result<(), String> {
    let names = roster.iter().map(|(id, entry)| (*id, entry.name.clone())).collect::<HashMap<_, _>>();
    let present = names.keys().copied().collect::<Vec<_>>();
    let (renamed, missing) = database(app, "同步成员", move |conn| {
        let renamed = group::sync_member_names(conn, group_id, &names)?;
        let missing = group::active_members(conn, group_id)?.into_iter()
            .filter(|m| !present.contains(&m.assistant_id)).collect::<Vec<_>>();
        Ok((renamed, missing))
    }).await?;
    for member in &missing {
        // 助手已删除时仍要尽量打断它在群里的执行；打断失败写进群里，不影响打开群聊。
        if let Err(error) = remove_member_internal(app, group_id, member.id, format!("{} 已被删除，已移出群聊", member.name)).await {
            let body = format!("没能停止已删除助手 {} 的回复：{error}", member.name);
            database(app, "记录错误", move |conn| group::post_notice(conn, group_id, "error", &body)).await?;
            changed(app, group_id);
        }
    }
    if renamed && missing.is_empty() {
        changed(app, group_id);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct MemberView {
    pub id: i64,
    pub assistant_id: i64,
    pub name: String,
    pub conversation_id: i64,
    pub is_chief: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnView {
    pub member_id: i64,
    pub state: String,
    /// 多轮讨论的进度；普通回复为 1/1。
    pub round: i64,
    pub rounds: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupSummary {
    pub id: i64,
    pub name: String,
    pub members: Vec<MemberView>,
    pub last_message: Option<group::Message>,
    pub updated_at: String,
    /// 正在回复的成员。
    pub typing: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupDetail {
    pub id: i64,
    pub name: String,
    pub members: Vec<MemberView>,
    pub messages: Vec<group::Message>,
    pub has_more: bool,
    pub turns: Vec<TurnView>,
    /// 有消息正在等待判断由谁回复。
    pub routing: bool,
}

fn member_views(members: Vec<group::Member>, roster: &HashMap<i64, RosterEntry>) -> Vec<MemberView> {
    members.into_iter().filter(|m| m.active).map(|m| MemberView {
        is_chief: roster.get(&m.assistant_id).is_some_and(|entry| entry.chief),
        id: m.id, assistant_id: m.assistant_id, name: m.name, conversation_id: m.conversation_id,
    }).collect()
}

async fn summary(app: &AppHandle, group_id: i64, roster: &HashMap<i64, RosterEntry>) -> Result<GroupSummary, String> {
    let (room, members, last, turns) = database(app, "读取", move |conn| {
        Ok((group::get(conn, group_id)?, group::members(conn, group_id)?, group::last_message(conn, group_id)?, group::open_turns(conn, group_id)?))
    }).await?;
    Ok(GroupSummary {
        id: room.id, name: room.name, members: member_views(members, roster), last_message: last, updated_at: room.updated_at,
        typing: turns.into_iter().filter(|t| t.state == "running").map(|t| t.member_id).collect(),
    })
}

#[tauri::command]
pub async fn list_butler_groups(app_handle: AppHandle) -> Result<Vec<GroupSummary>, String> {
    desktop()?;
    let roster = roster(&app_handle).await?;
    let ids = database(&app_handle, "列表", |conn| Ok(group::list(conn)?.into_iter().map(|g| g.id).collect::<Vec<_>>())).await?;
    let mut result = Vec::new();
    for id in ids {
        sync_roster(&app_handle, id, &roster).await?;
        result.push(summary(&app_handle, id, &roster).await?);
    }
    Ok(result)
}

const PAGE: usize = 80;

#[tauri::command]
pub async fn get_butler_group(app_handle: AppHandle, group_id: i64) -> Result<GroupDetail, String> {
    desktop()?;
    let roster = roster(&app_handle).await?;
    sync_roster(&app_handle, group_id, &roster).await?;
    let (room, members, mut messages, turns, routing) = database(&app_handle, "读取", move |conn| {
        Ok((group::get(conn, group_id)?, group::members(conn, group_id)?,
            group::messages(conn, group_id, None, PAGE + 1)?, group::open_turns(conn, group_id)?,
            group::has_pending_route(conn, group_id)?))
    }).await?;
    let has_more = messages.len() > PAGE;
    if has_more {
        messages.remove(0);
    }
    Ok(GroupDetail {
        id: room.id, name: room.name, members: member_views(members, &roster), messages, has_more,
        turns: turns.into_iter().map(|t| TurnView { member_id: t.member_id, state: t.state, round: t.round, rounds: t.rounds }).collect(),
        routing,
    })
}

#[tauri::command]
pub async fn list_butler_group_messages(app_handle: AppHandle, group_id: i64, before: i64, limit: Option<usize>) -> Result<Vec<group::Message>, String> {
    desktop()?;
    let limit = limit.unwrap_or(PAGE);
    database(&app_handle, "读取消息", move |conn| group::messages(conn, group_id, Some(before), limit)).await
}

async fn invited(app: &AppHandle, assistant_ids: &[i64], include_chief: bool) -> Result<Vec<(i64, String)>, String> {
    let roster = roster(app).await?;
    let mut result = Vec::new();
    if include_chief {
        let chief = super::butler_api::load_or_create_butler_main_internal(app).await?
            .assistant_id.ok_or("总管家主会话未绑定助手")?;
        let name = roster.get(&chief).map(|entry| entry.name.clone()).ok_or("总管家助手配置不存在")?;
        result.push((chief, name));
    }
    for id in assistant_ids {
        let entry = roster.get(id).ok_or_else(|| format!("助手 #{id} 不存在或已删除"))?;
        if entry.chief || result.iter().any(|(existing, _)| existing == id) {
            continue;
        }
        result.push((*id, entry.name.clone()));
    }
    Ok(result)
}

fn default_name(members: &[(i64, String)]) -> String {
    let names = members.iter().take(3).map(|(_, n)| n.as_str()).collect::<Vec<_>>().join("、");
    let name = if members.len() > 3 { format!("{names}等") } else { names };
    name.chars().take(group::MAX_NAME_CHARS).collect()
}

#[tauri::command]
pub async fn create_butler_group(app_handle: AppHandle, name: Option<String>, assistant_ids: Vec<i64>, include_chief: bool) -> Result<GroupSummary, String> {
    enabled(&app_handle).await?;
    let members = invited(&app_handle, &assistant_ids, include_chief).await?;
    if members.len() < 2 {
        return Err("群聊至少需要两位成员".into());
    }
    let name = match name {
        Some(name) => group::validate_name(&name).map_err(|e|e.to_string())?,
        None => default_name(&members),
    };
    let created = database(&app_handle, "创建", move |conn| group::create(conn, &name, &members)).await?;
    changed(&app_handle, created.id);
    let roster = roster(&app_handle).await?;
    summary(&app_handle, created.id, &roster).await
}

#[tauri::command]
pub async fn post_butler_group_message(app_handle: AppHandle, group_id: i64, request_key: String, body: String, reply_to: Option<i64>) -> Result<group::Message, String> {
    enabled(&app_handle).await?;
    let message = database(&app_handle, "发送消息", move |conn| group::post_user(conn, group_id, &request_key, &body, reply_to)).await?;
    changed(&app_handle, group_id);
    Ok(message)
}

#[derive(Debug, Serialize)]
pub struct StopSummary {
    pub stopped: usize,
    pub errors: Vec<String>,
}

/// 打断仍在进行的回复。停止失败需要如实说明原因，不能当成已经停下。
async fn halt(app: &AppHandle, turns: Vec<group::Turn>) -> Vec<String> {
    let mut errors = Vec::new();
    for t in turns {
        let id = t.member_id;
        let member = match database(app, "读取成员", move |conn| group::member(conn, id)).await {
            Ok(member) => member,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        if let Err(error) = super::butler_peer::halt_turn(app, member.conversation_id, member.name.clone()).await {
            errors.push(format!("停止 {} 的回复失败（群聊 #{}，回复 #{}，会话 #{}）：{error:#}", member.name, t.group_id, t.id, member.conversation_id));
        }
    }
    errors
}

#[tauri::command]
pub async fn stop_butler_group_replies(app_handle: AppHandle, group_id: i64) -> Result<StopSummary, String> {
    desktop()?;
    let running = database(&app_handle, "停止回复", move |conn| group::stop(conn, group_id)).await?;
    let stopped = running.len();
    let errors = halt(&app_handle, running).await;
    changed(&app_handle, group_id);
    Ok(StopSummary { stopped, errors })
}

#[tauri::command]
pub async fn add_butler_group_members(app_handle: AppHandle, group_id: i64, assistant_ids: Vec<i64>, include_chief: bool) -> Result<(), String> {
    enabled(&app_handle).await?;
    let members = invited(&app_handle, &assistant_ids, include_chief).await?;
    database(&app_handle, "邀请成员", move |conn| group::add_members(conn, group_id, &members)).await?;
    changed(&app_handle, group_id);
    Ok(())
}

async fn remove_member_internal(app: &AppHandle, group_id: i64, member_id: i64, notice: String) -> Result<(), String> {
    let (_, running) = database(app, "移出成员", move |conn| group::remove_member(conn, group_id, member_id, &notice)).await?;
    let errors = halt(app, running).await;
    changed(app, group_id);
    if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
}

#[tauri::command]
pub async fn remove_butler_group_member(app_handle: AppHandle, group_id: i64, member_id: i64) -> Result<(), String> {
    desktop()?;
    let current = database(&app_handle, "读取成员", move |conn| {
        let member = group::member(conn, member_id)?;
        let remaining = group::active_members(conn, group_id)?.len();
        anyhow::ensure!(member.group_id == group_id, "成员不属于该群聊");
        anyhow::ensure!(remaining > 1 || !member.active, "群里至少要保留一位成员");
        Ok(member)
    }).await?;
    remove_member_internal(&app_handle, group_id, member_id, format!("你将 {} 移出了群聊", current.name)).await
}

#[tauri::command]
pub async fn rename_butler_group(app_handle: AppHandle, group_id: i64, name: String) -> Result<(), String> {
    desktop()?;
    database(&app_handle, "改名", move |conn| group::rename(conn, group_id, &name)).await?;
    changed(&app_handle, group_id);
    Ok(())
}

/// 删除群聊：先打断进行中的回复，再删除成员的群执行会话和群本身。成员的私聊不受影响。
#[tauri::command]
pub async fn delete_butler_group(app_handle: AppHandle, group_id: i64) -> Result<(), String> {
    desktop()?;
    let (running, conversations) = database(&app_handle, "删除", move |conn| {
        let running = group::stop(conn, group_id)?;
        Ok((running, group::member_conversations(conn, group_id)?))
    }).await?;
    let errors = halt(&app_handle, running).await;
    if !errors.is_empty() {
        return Err(format!("无法删除群聊：部分回复没能停止，已保留群聊以便核对。\n{}", errors.join("\n")));
    }
    for conversation in conversations {
        super::conversation_api::delete_conversation(app_handle.clone(), conversation)
            .map_err(|e| format!("删除群成员会话 #{conversation} 失败：{e}"))?;
    }
    super::conversation_api::delete_conversation(app_handle.clone(), group_id)
        .map_err(|e| format!("删除群聊 #{group_id} 失败：{e}"))?;
    changed(&app_handle, group_id);
    Ok(())
}

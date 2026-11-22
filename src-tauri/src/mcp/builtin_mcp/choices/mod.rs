pub mod archive;
pub mod build;
pub mod document;
mod snapshot;
use crate::db::{butler_goal, conversation_db::ConversationDatabase, mcp_db::MCPDatabase};
use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use snapshot::{Candidate, Project, Request};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::{LazyLock, Mutex},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

static BOOT: LazyLock<String> = LazyLock::new(|| uuid::Uuid::new_v4().to_string());
static WAITING: LazyLock<Mutex<HashMap<String, oneshot::Sender<Value>>>> =
    LazyLock::new(Default::default);

fn db(app: &AppHandle) -> Result<Connection> {
    let conn = ConversationDatabase::new(app)?.get_connection()?;
    schema(&conn)?;
    Ok(conn)
}
fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS present_choices (
        request_id TEXT PRIMARY KEY, conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
        call_id INTEGER, message_id INTEGER, boot TEXT NOT NULL, revision INTEGER, expires INTEGER NOT NULL,
        request TEXT NOT NULL, decision TEXT, error TEXT);
        CREATE INDEX IF NOT EXISTS idx_choices_call ON present_choices(conversation_id,call_id);
        CREATE TABLE IF NOT EXISTS choice_snapshot_budget(request_id TEXT PRIMARY KEY REFERENCES present_choices(request_id) ON DELETE CASCADE, remaining INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS choice_project (
        request_id TEXT NOT NULL REFERENCES present_choices(request_id) ON DELETE CASCADE,
        candidate_id TEXT NOT NULL, files TEXT NOT NULL, PRIMARY KEY(request_id,candidate_id));
        CREATE TABLE IF NOT EXISTS choice_build(request_id TEXT NOT NULL REFERENCES present_choices(request_id) ON DELETE CASCADE,
          candidate_id TEXT NOT NULL, files TEXT NOT NULL, PRIMARY KEY(request_id,candidate_id));")
}
fn changed(app: &AppHandle, cid: i64) {
    let _ = app.emit("present-choices-changed", cid);
    let _ = app.emit("butler_interactions_changed", cid);
}
struct WaitGuard {
    app: AppHandle,
    cid: i64,
    id: String,
}
impl Drop for WaitGuard {
    fn drop(&mut self) {
        let removed = WAITING.lock().unwrap().remove(&self.id).is_some();
        if removed
            && crate::EXIT_STATE.load(std::sync::atomic::Ordering::SeqCst) == crate::EXIT_STATE_IDLE
        {
            if let Ok(conn) = db(&self.app) {
                let _ = conn.execute("UPDATE present_choices SET error=coalesce(error,'选择请求所属执行已中断') WHERE request_id=?1 AND decision IS NULL",[&self.id]);
            }
            changed(&self.app, self.cid);
        }
    }
}
fn store(
    app: &AppHandle,
    cid: i64,
    id: &str,
    call: Option<i64>,
    anchor: Option<i64>,
    mut request: Request,
) -> Result<Request> {
    let mut conn = db(app)?;
    let kind: String = conn.query_row(
        "SELECT coalesce(conversation_kind,'') FROM conversation WHERE id=?1",
        [cid],
        |r| r.get(0),
    )?;
    ensure!(kind != "scheduled_run", "定时任务无法等待用户选择");
    let (projects, remaining) = snapshot::prepare(app, cid, &mut request)?;
    let tx = conn.transaction()?;
    butler_goal::assert_message_current(&tx, cid, anchor)?;
    let revision = if butler_goal::is_butler(&tx, cid)? {
        let anchor = anchor.context("比选缺少原始管家回合身份")?;
        butler_goal::schema(&tx)?;
        Some(butler_goal::message_revision(&tx, cid, anchor)?.unwrap_or(0))
    } else {
        None
    };
    tx.execute("INSERT INTO present_choices(request_id,conversation_id,call_id,message_id,boot,revision,expires,request)
        VALUES(?1,?2,?3,?4,?5,?6,unixepoch()+86400,?7)",params![id,cid,call,anchor,&*BOOT,revision,serde_json::to_string(&request)?])?;
    tx.execute("INSERT INTO choice_snapshot_budget VALUES(?1,?2)", params![id, remaining])?;
    for (candidate, files) in projects {
        tx.execute(
            "INSERT INTO choice_project VALUES(?1,?2,?3)",
            params![id, candidate, serde_json::to_string(&files)?],
        )?;
    }
    if let (None, Some(anchor)) = (call, anchor) {
        crate::db::butler_interactions::save_event(&tx, cid, anchor, "choices", id, "{}")?;
    }
    let waiters = WAITING.lock().unwrap();
    ensure!(
        waiters.get(id).is_some_and(|sender| !sender.is_closed()),
        "原请求在快照准备期间已结束"
    );
    tx.commit()?;
    drop(waiters);
    Ok(request)
}
pub async fn present(app: &AppHandle, cid: Option<i64>, args: Value) -> Result<Value, String> {
    let cid = cid.ok_or("present_choices 需要执行会话")?;
    let mut request: Request =
        serde_json::from_value(args).map_err(|e| format!("候选参数：{e}"))?;
    request.validate().map_err(|e| e.to_string())?;
    let unattended: bool = db(app).and_then(|conn| Ok(conn.query_row("SELECT coalesce(conversation_kind,'')='scheduled_run' FROM conversation WHERE id=?1",[cid],|r|r.get(0))?)).map_err(|e|e.to_string())?;
    if unattended {
        return Err("定时任务无法等待用户选择".into());
    }
    let permissions = super::operation::permission::PermissionManager::new(app.clone());
    for candidate in &mut request.candidates {
        let Some(raw) = candidate.url.as_deref() else { continue };
        if raw.starts_with("http://") || raw.starts_with("https://") {
            continue;
        }
        let path = Path::new(raw);
        if !path.is_absolute() {
            return Err(format!("候选 {} 需要绝对文件路径", candidate.id));
        }
        let path = path.canonicalize().map_err(|e| format!("候选 {}：{e}", candidate.id))?;
        let resolved = path.to_str().ok_or("文件路径无法表示为 UTF-8")?;
        if !permissions
            .check_and_request_permission(
                &app.state::<super::OperationState>(),
                "read_file",
                resolved,
                Some(cid),
            )
            .await?
        {
            return Err(format!("用户拒绝读取候选 {}", candidate.id));
        }
        candidate.url = Some(resolved.to_owned());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let call = crate::mcp::execution_api::INTERACTION_CALL_ID.try_with(|id| *id).ok();
    let anchor = crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).ok().flatten();
    let cancel = app
        .state::<crate::state::message_token::MessageTokenManager>()
        .get_cancel_token(cid)
        .await
        .unwrap_or_default();
    let (tx, mut rx) = oneshot::channel();
    WAITING.lock().unwrap().insert(id.clone(), tx);
    let guard = WaitGuard { app: app.clone(), id: id.clone(), cid };
    let app_copy = app.clone();
    let request_id = id.clone();
    let request = tauri::async_runtime::spawn_blocking(move || {
        store(&app_copy, cid, &request_id, call, anchor, request)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("会话 {cid} 准备比选 {id}：{e:#}"))?;
    changed(app, cid);
    let delivery = crate::feishu::deliver_choice_notice(
        app,
        cid,
        &request.title,
        &request
            .candidates
            .iter()
            .enumerate()
            .map(|(idx, c)| {
                let summary = if matches!(c.kind.as_str(), "markdown" | "text") {
                    c.content.as_deref()
                } else {
                    c.summary.as_deref()
                };
                format!(
                    "{}. {}\n{}",
                    idx + 1,
                    c.title,
                    summary.unwrap_or("").chars().take(900).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
    .await;
    if let Err(error) = delivery {
        if let Ok(decision) = rx.try_recv() {
            return Ok(decision);
        }
        if let Ok(conn) = db(app) {
            let _ = conn.execute(
                "UPDATE present_choices SET error=?2 WHERE request_id=?1 AND decision IS NULL",
                params![id, format!("飞书比选提示投递失败：{error}")],
            );
        }
        return Err(format!("会话 {cid} 的飞书比选提示投递失败：{error}"));
    }
    let _ = crate::api::butler_api::emit_butler_task_ask_user_attention(app, cid).await;
    let result = tokio::select! { biased;
        result=rx => result.map_err(|_|format!("选择通道已关闭，request_id={id}")),
        _=cancel.cancelled()=>Err(format!("原会话已停止，request_id={id}")),
    };
    drop(guard);
    result
}
#[derive(Serialize)]
pub struct Card {
    request_id: String,
    request: Request,
    decision: Option<Value>,
    error: Option<String>,
    recovered: bool,
}
fn card(conn: &Connection, cid: i64, id: Option<&str>, call: Option<i64>) -> Result<Option<Card>> {
    let row=conn.query_row("SELECT request_id,request,decision,error,boot,revision,expires FROM present_choices
        WHERE conversation_id=?1 AND ((?2 IS NOT NULL AND request_id=?2) OR (?2 IS NULL AND call_id=?3)) ORDER BY rowid DESC LIMIT 1",
        params![cid,id,call],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,
        r.get::<_,String>(4)?,r.get::<_,Option<i64>>(5)?,r.get::<_,i64>(6)?))).optional()?;
    let Some((request_id, raw, decision, mut error, boot, revision, expires)) = row else {
        return Ok(None);
    };
    let recovered = boot != *BOOT;
    if decision.is_none() && error.is_none() {
        if let Some(rev) = revision.filter(|v| *v > 0) {
            error = butler_goal::assert_current(conn, cid, rev).err().map(|e| e.to_string());
        }
        if recovered && (revision.is_none() || expires <= chrono::Utc::now().timestamp()) {
            error = Some("原执行已中断，选择请求已失效".into());
        }
    }
    Ok(Some(Card {
        request_id,
        request: serde_json::from_str(&raw)?,
        decision: decision.map(|s| serde_json::from_str(&s)).transpose()?,
        error,
        recovered,
    }))
}
#[tauri::command]
pub async fn get_present_choices(
    app_handle: AppHandle,
    conversation_id: i64,
    request_id: Option<String>,
    call_id: Option<i64>,
) -> Result<Option<Card>, String> {
    db(&app_handle)
        .and_then(|conn| card(&conn, conversation_id, request_id.as_deref(), call_id))
        .map_err(|e| e.to_string())
}
#[derive(Deserialize)]
pub struct Decision {
    status: String,
    selected_ids: Vec<String>,
    feedback: Option<String>,
}
fn validate(request: &Request, id: &str, d: Decision) -> Result<Value> {
    let ids = &d.selected_ids;
    ensure!(ids.iter().collect::<HashSet<_>>().len() == ids.len(), "候选 ID 重复");
    let titles = ids
        .iter()
        .map(|id| {
            request.candidates.iter().find(|c| c.id == *id).map(|c| &c.title).context("未知候选 ID")
        })
        .collect::<Result<Vec<_>>>()?;
    match d.status.as_str() {
        "selected" => ensure!(
            !ids.is_empty() && (request.selection_mode == "multiple" || ids.len() == 1),
            "请选择有效数量的候选"
        ),
        "cancelled" | "rejected" => ensure!(ids.is_empty(), "取消或重做时不能提交选项"),
        _ => anyhow::bail!("未知的选择结果"),
    }
    ensure!(
        request.allow_feedback || d.feedback.as_deref().unwrap_or("").is_empty(),
        "此请求未启用意见"
    );
    ensure!(
        d.status == "cancelled" || request.candidates.iter().all(|c| c.content.is_some()),
        "候选内容尚未准备完成"
    );
    Ok(
        json!({"request_id":id,"status":d.status,"selected_ids":ids,"selected_titles":titles,"feedback":d.feedback}),
    )
}
fn complete(conn: &mut Connection, cid: i64, id: &str, d: Decision) -> Result<(bool, bool)> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let card = card(&tx, cid, Some(id), None)?.context("找不到此会话的比选请求")?;
    ensure!(card.decision.is_none(), "选择已提交");
    if let Some(error) = card.error {
        anyhow::bail!("{error}")
    }
    let decision = validate(&card.request, id, d)?;
    let mut waiters = WAITING.lock().unwrap();
    if !card.recovered {
        ensure!(waiters.get(id).is_some_and(|sender| !sender.is_closed()), "原请求已结束");
    }
    let queue = card.recovered && decision["status"] != "cancelled";
    if queue {
        let assistant: i64 =
            tx.query_row("SELECT assistant_id FROM conversation WHERE id=?1", [cid], |r| r.get(0))?;
        let prompt = format!(
            "[用户已回答恢复的比选请求] {}\n{}\n按此选择继续。此前结果不明的外部操作请先核对。",
            card.request.title, decision
        );
        let request = json!({"conversation_id":cid.to_string(),"assistant_id":assistant,"prompt":prompt,"recovered_question_id":id,"butler_intent":"aside","butler_goal_revision":butler_goal::current_revision(&tx,cid)?});
        tx.execute("INSERT INTO queued_conversation_message(conversation_id,queue_kind,status,request_json,prompt,assistant_id) VALUES(?1,'interrupt','queued',?2,?3,?4)",params![cid,request.to_string(),prompt,assistant])?;
    }
    tx.execute(
        "UPDATE present_choices SET decision=?2 WHERE request_id=?1",
        params![id, decision.to_string()],
    )?;
    tx.commit()?;
    if let Some(sender) = waiters.remove(id) {
        sender.send(decision).map_err(|_| anyhow!("选择已保存，原执行已关闭"))?;
    }
    Ok((card.recovered, queue))
}
#[tauri::command]
pub async fn submit_present_choices_response(
    app_handle: AppHandle,
    conversation_id: i64,
    request_id: String,
    decision: Decision,
) -> Result<(), String> {
    let current =
        get_present_choices(app_handle.clone(), conversation_id, Some(request_id.clone()), None)
            .await?
            .ok_or("找不到选择请求")?;
    if current.recovered {
        if app_handle
            .state::<crate::state::activity_state::ConversationActivityManager>()
            .get_runtime_state(conversation_id)
            .await
            .is_running
        {
            return Err("会话仍在运行，请稍后作答".into());
        }
        if decision.status != "cancelled" {
            let calls = MCPDatabase::new(&app_handle)
                .and_then(|d| d.get_mcp_tool_calls_by_conversation(conversation_id))
                .map_err(|e| e.to_string())?;
            if calls.iter().any(|c| matches!(c.status.as_str(), "pending" | "executing")) {
                return Err("原会话还有结果不明的工具操作，请先核对".into());
            }
        }
    }
    let window = if current.recovered && decision.status != "cancelled" {
        Some(crate::api::butler_api::resolve_or_create_butler_execution_window(&app_handle)?)
    } else {
        None
    };
    let (_, queue) = db(&app_handle)
        .and_then(|mut conn| complete(&mut conn, conversation_id, &request_id, decision))
        .map_err(|e| e.to_string())?;
    changed(&app_handle, conversation_id);
    if let (true, Some(window)) = (queue, window) {
        crate::api::ai_api::try_dispatch_queued_message_after_completion(
            &app_handle,
            &window,
            conversation_id,
        )
        .await;
    }
    Ok(())
}
#[tauri::command]
pub async fn list_native_choices(
    app_handle: AppHandle,
    conversation_id: i64,
) -> Result<Vec<(String, Option<i64>)>, String> {
    let conn = db(&app_handle).map_err(|e| e.to_string())?;
    let mut stmt=conn.prepare("SELECT request_id,message_id FROM present_choices WHERE conversation_id=?1 AND call_id IS NULL ORDER BY rowid").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([conversation_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
/// Capture the previous process's unfinished calls during setup, before chat dispatch starts.
pub fn unfinished_calls(app: &AppHandle) -> Result<Vec<i64>> {
    let conn = db(app)?;
    let mut recorded = conn.prepare("SELECT conversation_id,call_id FROM present_choices WHERE call_id IS NOT NULL AND boot!=?1")?;
    let recorded = recorded
        .query_map([&*BOOT], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    let mcp = MCPDatabase::new(app)?;
    let mut pending = mcp.conn.prepare("SELECT t.id,t.conversation_id,(s.is_builtin AND s.command='aipp:ui_interaction' AND t.tool_name='present_choices') FROM mcp_tool_call t JOIN mcp_server s ON t.server_id=s.id WHERE t.status IN ('pending','executing')")?;
    let rows = pending
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<bool>>(2)?.unwrap_or(false),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter(|(call, cid, direct)| *direct || recorded.contains(&(*cid, *call)))
        .map(|(call, _, _)| call)
        .collect())
}
pub async fn reconcile(app: AppHandle, calls: Vec<i64>) {
    for call in calls {
        if let Err(error) = crate::mcp::execution_api::fail_unfinished_tool_call(
            &app,
            call,
            "应用已重启，原比选工具调用已中断",
        )
        .await
        {
            tracing::warn!(%error, call, "恢复比选工具调用失败");
        }
    }
}
fn project(app: &AppHandle, cid: i64, id: &str, candidate: &str) -> Result<(Candidate, Project)> {
    let conn = db(app)?;
    let card = card(&conn, cid, Some(id), None)?.context("未知比选请求")?;
    let c = card
        .request
        .candidates
        .into_iter()
        .find(|c| c.id == candidate && c.kind == "artifact")
        .context("未知工程候选")?;
    let files: String = conn.query_row(
        "SELECT files FROM choice_project WHERE request_id=?1 AND candidate_id=?2",
        params![id, candidate],
        |r| r.get(0),
    )?;
    Ok((c, serde_json::from_str(&files)?))
}
#[tauri::command]
pub async fn get_choice_project(
    app_handle: AppHandle,
    conversation_id: i64,
    request_id: String,
    candidate_id: String,
) -> Result<Project, String> {
    project(&app_handle, conversation_id, &request_id, &candidate_id)
        .map(|p| p.1)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn publish_choice_project(
    app_handle: AppHandle,
    conversation_id: i64,
    request_id: String,
    candidate_id: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        publish(&app_handle, conversation_id, &request_id, &candidate_id)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("发布工程失败：{e:#}"))
}
fn publish(app: &AppHandle, cid: i64, id: &str, candidate: &str) -> Result<String> {
    use crate::artifacts::workspace::{get_artifact_workspace, show_artifact, ShowArtifactRequest};
    let (c, files) = project(app, cid, id, candidate)?;
    let ws = get_artifact_workspace(app, cid).map_err(anyhow::Error::msg)?;
    fs::create_dir_all(&ws.artifacts_dir)?;
    let base = snapshot::workspace_root(app, cid, Path::new(&ws.artifacts_dir))?;
    let request = card(&db(app)?, cid, Some(id), None)?.context("请求已删除")?;
    let index = request
        .request
        .candidates
        .iter()
        .position(|item| item.id == candidate)
        .context("候选已删除")?;
    let key = format!("choice-{id}-{index}");
    let destination = base.join(&key);
    let dir = tempfile::Builder::new().prefix(".publishing-").tempdir_in(&base)?;
    for (name, encoded) in files {
        let path = dir.path().join("project").join(snapshot::relative(&name)?);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, STANDARD.decode(encoded)?)?;
    }
    let reference = json!({"conversationId":cid,"requestId":id,"candidateId":candidate});
    let marker = format!("AIPP_CHOICE_PREVIEW_V1\n{reference}");
    fs::write(dir.path().join("preview.html"), &marker)?;
    if let Err(err) = fs::rename(dir.path(), &destination) {
        ensure!(
            fs::read_to_string(destination.join("preview.html")).ok().as_ref() == Some(&marker),
            "发布目录已被占用：{err}"
        );
    }
    show_artifact(
        app,
        ShowArtifactRequest {
            conversation_id: cid,
            artifact_key: key.clone(),
            entry_file: "preview.html".into(),
            title: Some(c.title),
            language: Some("html".into()),
            preview_type: None,
            db_id: None,
            assistant_id: None,
        },
    )
    .map_err(anyhow::Error::msg)?;
    Ok(key)
}
#[tauri::command]
pub async fn authorize_choice_source(
    app_handle: AppHandle,
    conversation_id: i64,
    request_id: String,
    candidate_id: String,
) -> Result<(), String> {
    use super::preview_resources::{fetch_choice_source, PreviewResourceType};
    let record =
        get_present_choices(app_handle.clone(), conversation_id, Some(request_id.clone()), None)
            .await?
            .ok_or("未知比选请求")?;
    if record.error.is_some() || record.decision.is_some() {
        return Err("比选已结束".into());
    }
    let candidate =
        record.request.candidates.iter().find(|c| c.id == candidate_id).ok_or("未知候选")?;
    let url = candidate.url.as_deref().ok_or("候选没有远端来源")?;
    let kind = match candidate.kind.as_str() {
        "image" => PreviewResourceType::Image,
        "html" => PreviewResourceType::Html,
        "markdown" => PreviewResourceType::Markdown,
        _ => PreviewResourceType::Text,
    };
    let (bytes, mime) = fetch_choice_source(&app_handle, url, kind).await?;
    let content =
        snapshot::to_content(&candidate.kind, &bytes, mime.split(';').next().unwrap_or(""))
            .map_err(|e| e.to_string())?;
    let mut conn = db(&app_handle).map_err(|e| e.to_string())?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let mut current = card(&tx, conversation_id, Some(&request_id), None)
        .map_err(|e| e.to_string())?
        .ok_or("请求已删除")?;
    if current.decision.is_some() || current.error.is_some() {
        return Err("请求已结束".into());
    }
    let c =
        current.request.candidates.iter_mut().find(|c| c.id == candidate_id).ok_or("候选已删除")?;
    if c.content.is_none() {
        let charged = tx.execute("UPDATE choice_snapshot_budget SET remaining=remaining-?2 WHERE request_id=?1 AND remaining>=?2",params![request_id,bytes.len()]).map_err(|e|e.to_string())?;
        if charged == 0 {
            return Err("候选内容总计超过 20 MiB".into());
        }
        c.content = Some(content);
        c.url = None;
    }
    tx.execute(
        "UPDATE present_choices SET request=?2 WHERE request_id=?1",
        params![request_id, serde_json::to_string(&current.request).map_err(|e| e.to_string())?],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    changed(&app_handle, conversation_id);
    Ok(())
}
#[cfg(test)]
#[path = "choices_tests.rs"]
mod tests;

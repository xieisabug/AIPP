//! Read progress from durable work records; notifications never replace the conversation.
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Progress {
    pub waiting_for: Vec<String>,
    pub blocker: Option<String>,
    pub next_step: String,
    pub evidence: Vec<Value>,
    pub artifacts: Vec<Value>,
}

pub fn result_references(conn: &Connection, task: i64, final_message: Option<i64>) -> rusqlite::Result<(Vec<Value>, Vec<Value>)> {
    let mut evidence = Vec::new();
    let mut artifacts = Vec::new();
    if let Some(id) = final_message {
        let owned: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM message WHERE id=?1 AND conversation_id=?2)", params![id,task], |r| r.get(0))?;
        if owned { evidence.push(json!({"kind":"message","conversation_id":task,"message_id":id,"label":"最终答复"})); }
    }
    let mut stmt = conn.prepare("SELECT a.id,a.message_id,a.attachment_url FROM message_attachment a JOIN message m ON m.id=a.message_id WHERE m.conversation_id=?1 ORDER BY a.id")?;
    for row in stmt.query_map([task], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?)))? {
        let (id,message_id,url)=row?;
        artifacts.push(json!({"kind":"attachment","attachment_id":id,"message_id":message_id,"conversation_id":task,"location":url,"label":"会话附件（不代表已核验成果）"}));
    }
    Ok((evidence, artifacts))
}

pub fn read(conn: &Connection, task: i64, status: &str, pending_permission: bool, question_waiting: bool) -> rusqlite::Result<Progress> {
    super::butler_peer::schema(conn)?;
    let mut progress = Progress::default();
    if pending_permission { progress.waiting_for.push("你的操作批准".into()); }
    if question_waiting { progress.waiting_for.push("你的问题答复".into()); }
    let mut stmt=conn.prepare("SELECT DISTINCT recipient FROM butler_peer_exchange WHERE owner=?1 AND state IN ('queued','sending','waiting','returning','reply_sending') AND stop_requested=0 ORDER BY recipient")?;
    progress.waiting_for.extend(stmt.query_map([task], |r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
    if matches!(status,"failed"|"interrupted"|"cancelled") { progress.blocker=Some(format!("工作{}；请查看结果中的具体原因",match status {"failed"=>"失败","cancelled"=>"已停止",_=>"已中断"})); }
    progress.next_step = if !progress.waiting_for.is_empty() {"等待上述答复后继续"}
        else if matches!(status,"succeeded"|"completed") {"查看结果与引用资料"}
        else if progress.blocker.is_some() {"核对已完成部分，再决定是否继续"}
        else {"执行当前工作；尚无已记录的阻塞"}.into();
    let raw=conn.query_row("SELECT evidence_json,artifact_refs_json,summary FROM butler_task_result WHERE task_conversation_id=?1",[task],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?)));
    match raw { Ok((e,a,summary))=> {if progress.blocker.is_some() { if let Some(reason)=summary.filter(|s|!s.trim().is_empty()) {progress.blocker=Some(reason);} } progress.evidence=e.and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_default();progress.artifacts=a.and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_default();},Err(rusqlite::Error::QueryReturnedNoRows)=>{},Err(e)=>return Err(e) }
    let control=super::butler_control::get(conn,task)?;
    if control.cancelled { progress.next_step="已停止；查看已有结果，不会自动重新执行".into(); }
    else if control.paused { progress.blocker=Some(control.note);progress.next_step="由你恢复后再继续；不会自动重放旧操作".into(); }
    Ok(progress)
}

/// Atomic durable duplicate filter, scoped to a specific task and event category.
/// Failures/questions are never suppressed by the quiet policy. UI result delivery is separate.
pub fn claim_notification(conn: &Connection, task:i64, kind:&str, title:&str, body:&str, important:bool, policy:Option<&str>) -> rusqlite::Result<bool> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_notification_state(task_id INTEGER NOT NULL,kind TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(task_id,kind))")?;
    if !important && matches!(policy,Some("quiet"|"important_only")) && kind!="task_terminal" { return Ok(false); }
    let payload=serde_json::to_string(&(title,body)).unwrap_or_default();
    Ok(conn.execute("INSERT INTO butler_notification_state(task_id,kind,payload) VALUES(?1,?2,?3) ON CONFLICT(task_id,kind) DO UPDATE SET payload=excluded.payload WHERE payload<>excluded.payload",params![task,kind,payload])?>0)
}

#[cfg(test)]
#[path="butler_progress_tests.rs"] mod tests;

//! User-controlled contracts for Butler background checks. No contract means no execution.
use chrono::{DateTime,Utc};
use rusqlite::{Connection,params,OptionalExtension};
use serde::{Serialize,Deserialize};
use serde_json::Value;

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct CheckPolicy {
    pub mode:String,
    pub sources:Vec<String>,
    pub expires_at:String,
    pub paused:bool,
    pub stopped:bool,
    pub stop_current_on_pause:bool,
}
pub fn schema(conn:&Connection)->rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_check_policy(task_id INTEGER PRIMARY KEY,policy_json TEXT NOT NULL)")
}
pub fn read(conn:&Connection,id:i64)->rusqlite::Result<Option<CheckPolicy>> {
    schema(conn)?;
    let text:Option<String>=conn.query_row("SELECT policy_json FROM butler_check_policy WHERE task_id=?1",[id],|r|r.get(0)).optional()?;
    text.map(|s|serde_json::from_str(&s).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))).transpose()
}
pub fn save(conn:&Connection,id:i64,p:&CheckPolicy)->rusqlite::Result<()> {
    schema(conn)?;
    conn.execute("INSERT INTO butler_check_policy(task_id,policy_json) VALUES(?1,?2) ON CONFLICT(task_id) DO UPDATE SET policy_json=excluded.policy_json",params![id,serde_json::to_string(p).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?])?;Ok(())
}
/// Policy edits and run admission use the same SQLite write lock. A policy's
/// sources/mode/expiry cannot change between the running check and the write.
pub fn configure(conn: &mut Connection, id: i64, policy: &CheckPolicy, now: DateTime<Utc>) -> Result<(), String> {
    validate(policy, now)?;
    change(conn, id, now, |tx| {
        let running: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM scheduled_task_run WHERE task_id=?1 AND status='running')", [id], |r| r.get(0)).map_err(|e| e.to_string())?;
        if running { return Err("当前检查还在执行；请先停止或等待结束再修改来源和期限".into()); }
        Ok(policy.clone())
    })
}
pub fn control(conn: &mut Connection, id: i64, action: &str, stop_current: bool, now: DateTime<Utc>) -> Result<(), String> {
    change(conn, id, now, |tx| {
        let mut p = read(tx, id).map_err(|e| e.to_string())?.ok_or("请先配置检查范围")?;
        match action {
            "pause" => p.paused = true,
            "stop" => { p.paused = true; p.stopped = true; },
            "resume" => {
                if p.stopped { return Err("已关闭的检查需由用户重新配置，不能自动恢复".into()); }
                p.paused = false;
                ensure_active(Some(&p), now)?;
            },
            _ => return Err("未知的检查操作".into()),
        }
        p.stop_current_on_pause = stop_current;
        Ok(p)
    })
}
fn change(conn: &mut Connection, id: i64, now: DateTime<Utc>, update: impl FnOnce(&Connection) -> Result<CheckPolicy, String>) -> Result<(), String> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
    let owner: Option<i64> = tx.query_row("SELECT butler_conversation_id FROM scheduled_task WHERE id=?1", [id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if owner.is_none() { return Err("只适用于管家后台检查".into()); }
    let p = update(&tx)?;
    save(&tx, id, &p).map_err(|e| e.to_string())?;
    tx.execute("UPDATE scheduled_task SET is_enabled=?1,updated_time=?2 WHERE id=?3", params![!p.paused && !p.stopped, now, id]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn validate(p:&CheckPolicy,now:DateTime<Utc>)->Result<(),String> {
    if !matches!(p.mode.as_str(),"read_only"|"draft") {return Err("检查模式只能是只读或准备草稿".into());}
    if p.sources.is_empty() || p.sources.iter().any(|s|s.trim().is_empty()) {return Err("请明确填写检查来源".into());}
    let expires=DateTime::parse_from_rfc3339(&p.expires_at).map_err(|_|"检查期限格式无效")?;
    if expires<=now {return Err("后台检查已到期，请由用户重新确认期限".into());}
    Ok(())
}
pub fn ensure_active(p:Option<&CheckPolicy>,now:DateTime<Utc>)->Result<(),String> {
    let p=p.ok_or("后台检查尚未经用户配置来源、模式和期限，不能执行")?;
    validate(p,now)?;
    if p.stopped {return Err("后台检查已关闭，不能自动重建或恢复".into());}
    if p.paused {return Err("后台检查已暂停".into());} Ok(())
}
/// Only verified built-in readers are allowed. Unknown tools fail closed, even with a read-like name.
pub fn ensure_tool(p:&CheckPolicy,builtin:bool,server:&str,tool:&str,args:&str,now:DateTime<Utc>)->Result<(),String> {
    ensure_active(Some(p),now)?;
    let a:Value=serde_json::from_str(args).map_err(|_|"检查工具参数无效")?;
    let source=match (builtin,server,tool) {
        (true,"aipp:search","fetch_url") => a.get("url").and_then(Value::as_str),
        (true,"aipp:operation","read_file") => a.get("file_path").and_then(Value::as_str),
        _=>return Err(format!("后台检查只允许已适配的只读来源工具；不允许 {server}/{tool}，草稿只保存在结果中，不发送或写入外部服务")),
    }.ok_or("检查工具缺少来源")?;
    if !p.sources.iter().any(|s|s==source) {return Err("该来源不在用户批准的检查范围内".into());}
    Ok(())
}
#[cfg(test)]
#[path="butler_check_tests.rs"] mod tests;

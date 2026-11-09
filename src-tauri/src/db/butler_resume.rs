//! Explicit user-requested recovery from a normal Butler conversation, never a paused execution.
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ResumeResult {
    pub task_conversation_id: i64,
    pub resumed: bool,
    pub delegation_stopped: bool,
    pub goal_revision: i64,
}

fn direct_user(conn: &Connection, current: i64, source: i64) -> anyhow::Result<i64> {
    let mut next = Some(source);
    for _ in 0..100 {
        let id = next.context("恢复操作缺少直接用户消息")?;
        let (parent, kind, metadata): (Option<i64>, String, Option<String>) = conn.query_row(
            "SELECT parent_id,message_type,metadata_json FROM message WHERE id=?1 AND conversation_id=?2",
            params![id,current], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if kind == "user" {
            let meta: serde_json::Value = metadata.as_deref().map(serde_json::from_str).transpose()?.unwrap_or(serde_json::Value::Null);
            ensure!(meta["origin"].is_null() && meta["relay_origin"].is_null() && meta["relayOrigin"].is_null(), "内部消息不能恢复暂停工作");
            let latest: Option<i64> = conn.query_row("SELECT max(id) FROM message WHERE conversation_id=?1 AND message_type='user'", [current], |r| r.get(0))?;
            ensure!(latest == Some(id), "恢复请求已不是当前用户回合");
            super::butler_goal::assert_message_current(conn,current,Some(id))?;
            return Ok(id);
        }
        ensure!(matches!(kind.as_str(), "response" | "assistant" | "tool" | "tool_result"), "恢复来源不是用户对话回合");
        next = parent;
    }
    anyhow::bail!("恢复来源链过长或存在循环")
}

fn reject_pending(conn: &Connection, table: &str, sql: &str, target: i64) -> anyhow::Result<()> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |r| r.get(0))?;
    if exists {
        let pending: bool = conn.query_row(sql,[target],|r|r.get(0))?;
        ensure!(!pending,"工作还有未处理的旧请求（{table}），为避免重放操作，暂不恢复");
    }
    Ok(())
}

/// Caller holds admit_turn(target), verifies no live runtime or pending MCP executions,
/// and passes runtime-captured TOOL_MESSAGE (never a model-supplied message id).
/// Does not dispatch work. A separate new instruction is required after success.
pub fn resume_work(conn: &mut Connection, current: i64, target: i64, source: i64) -> anyhow::Result<ResumeResult> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let kind: String = tx.query_row("SELECT conversation_kind FROM conversation WHERE id=?1",[current],|r|r.get(0))?;
    ensure!(matches!(kind.as_str(),"butler_main"|"butler_contact"),"只能在总管家或助手联系人会话恢复工作");
    super::butler_control::ensure_turn(&tx,current)?;
    direct_user(&tx,current,source)?;
    let view = super::butler_work::resolve(&tx,current,"task",target)?;
    ensure!(!view.finished,"已结束的工作不能恢复，请另行续接");
    let old = super::butler_control::get(&tx,target)?;
    ensure!(!old.cancelled,"工作已停止，不能恢复");
    let goal = super::butler_goal::current(&tx,target)?;
    ensure!(!goal.as_ref().is_some_and(|g|g.stopped),"工作目标已停止，不能恢复");
    let mut revision = goal.as_ref().map_or(0,|g|g.revision);
    if old.paused {
        reject_pending(&tx,"queued_conversation_message","SELECT EXISTS(SELECT 1 FROM queued_conversation_message WHERE conversation_id=?1 AND status IN ('queued','dispatching'))",target)?;
        reject_pending(&tx,"butler_work_followup","SELECT EXISTS(SELECT 1 FROM butler_work_followup WHERE (task_conversation_id=?1 OR result_task=?1) AND state IN ('queued','sending','spawned','uncertain'))",target)?;
        reject_pending(&tx,"butler_peer_exchange","SELECT EXISTS(SELECT 1 FROM butler_peer_exchange WHERE (owner=?1 OR target=?1) AND (state IN ('queued','sending','waiting','returning','reply_sending') OR receipt_state IN ('pending','sending')))",target)?;
        // Advance only the execution fence. Never delete history, replay requests or restore approvals.
        revision += 1;
        tx.execute("INSERT INTO butler_goal(conversation_id,revision,goal,stopped) VALUES(?1,?2,?3,0) ON CONFLICT(conversation_id) DO UPDATE SET revision=excluded.revision",params![target,revision,goal.map_or(view.title,|g|g.text)])?;
        tx.execute("UPDATE butler_goal_question SET state='superseded' WHERE conversation_id=?1 AND state='waiting'",[target])?;
        super::butler_control::set(&tx,target,"resume")?;
    }
    let result = ResumeResult { task_conversation_id:target,resumed:old.paused,delegation_stopped:old.delegation_stopped,goal_revision:revision };
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
#[path = "butler_resume_tests.rs"]
mod tests;

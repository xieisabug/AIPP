//! Persistent admission controls. Pausing never claims to undo an operation already submitted.
use rusqlite::{Connection, params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Control {
    pub paused: bool,
    pub delegation_stopped: bool,
    pub cancelled: bool,
    pub note: String,
}
pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_task_control(
        conversation_id INTEGER PRIMARY KEY, paused INTEGER NOT NULL DEFAULT 0,
        delegation_stopped INTEGER NOT NULL DEFAULT 0, cancelled INTEGER NOT NULL DEFAULT 0,
        note TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")
}
pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Control> {
    schema(conn)?;
    Ok(conn.query_row("SELECT paused,delegation_stopped,cancelled,note FROM butler_task_control WHERE conversation_id=?1",
        [id], |r| Ok(Control { paused:r.get(0)?,delegation_stopped:r.get(1)?,cancelled:r.get(2)?,note:r.get(3)? })).optional()?.unwrap_or_default())
}
pub fn set(conn: &Connection, id: i64, action: &str) -> anyhow::Result<Control> {
    let old=get(conn,id)?;
    anyhow::ensure!(!old.cancelled || action=="cancel", "工作已停止，不能通过恢复重新执行");
    let (paused,delegation,cancelled,note)=match action {
        "pause" => (true,old.delegation_stopped,false,"已暂停后续回合、应用工具和新协作；运行中的外部 Agent 或已提交操作可能仍会完成"),
        "resume" => (false,old.delegation_stopped,false,"已恢复接收新请求；不会自动重放之前的操作"),
        "stop_delegation" => (old.paused,true,false,"已停止新增协作；已派出的工作继续，整项停止请使用停止整项"),
        "cancel" => (true,true,true,"已要求停止整项工作；已经发生的外部操作不保证撤销"),
        _ => anyhow::bail!("未知控制动作"),
    };
    conn.execute("INSERT INTO butler_task_control(conversation_id,paused,delegation_stopped,cancelled,note) VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(conversation_id) DO UPDATE SET paused=excluded.paused,delegation_stopped=excluded.delegation_stopped,cancelled=excluded.cancelled,note=excluded.note,updated_at=CURRENT_TIMESTAMP",
        params![id,paused,delegation,cancelled,note])?;
    Ok(get(conn,id)?)
}
pub fn ensure_turn(conn: &Connection,id:i64)->anyhow::Result<()> {
    let c=get(conn,id)?;
    anyhow::ensure!(!c.cancelled,"工作已停止，不再启动执行");
    anyhow::ensure!(!c.paused,"工作已暂停。请在总管家或执行助手的联系人对话里说明要恢复哪项工作；本任务窗口不会绕过暂停直接运行"); Ok(())
}
/// A tool result continues the original turn; it cannot resume a paused/stopped
/// task or borrow a newer goal. Recheck at the provider boundary after async prep.
pub fn ensure_execution(conn: &Connection, id: i64, source: Option<i64>) -> anyhow::Result<()> {
    if !super::butler_goal::is_butler(conn, id)? { return Ok(()); }
    ensure_turn(conn, id)?;
    super::butler_goal::assert_message_current(conn, id, source)
}
pub fn ensure_delegation(conn: &Connection,id:i64)->anyhow::Result<()> {
    ensure_turn(conn,id)?;
    anyhow::ensure!(!get(conn,id)?.delegation_stopped,"已停止后续协作，不再派发"); Ok(())
}
#[cfg(test)] #[path="butler_control_tests.rs"] mod tests;

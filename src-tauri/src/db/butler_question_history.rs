//! Durable, read-only history of task questions answered from a contact window.
use anyhow::{ensure, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub key: String,
    pub questions: Value,
    pub metadata: Value,
    pub answers: HashMap<String, String>,
    pub cancelled: bool,
    /// pending means a reply was prepared but delivery was not durably confirmed. Never replay it.
    #[serde(default = "delivered_status")]
    pub delivery_status: String,
}

fn delivered_status() -> String { "delivered".into() }

fn schema(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_task_question_answer (
        task_conversation_id INTEGER NOT NULL, question_key TEXT NOT NULL, record_json TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        PRIMARY KEY(task_conversation_id,question_key)
    )")?;
    Ok(())
}

/// Recheck the task's current executor; an ID supplied by a window is not authorization.
pub fn validate_owner(conn: &Connection, contact: i64, task: i64) -> Result<()> {
    let owned: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversation c JOIN butler_task_definition d
         ON d.executor_assistant_id=c.assistant_id JOIN conversation t ON t.id=d.task_conversation_id
         WHERE c.id=?1 AND c.conversation_kind='butler_contact' AND t.id=?2)",
        params![contact, task], |r| r.get(0))?;
    ensure!(owned, "任务 #{task} 不属于当前联系人，无权回答");
    Ok(())
}

/// Atomically claim an interrupted MCP question before creating a continuation or cancelling it.
/// The connection here is mcp.db, not the conversation history database.
pub fn claim_stale(conn: &Connection, task: i64, call: i64, cancelled: bool) -> Result<()> {
    let reason = if cancelled { "用户在执行助手窗口取消回答" }
        else { "问题正在转为续接，若中断请核对续接状态" };
    let claimed = conn.execute(
        "UPDATE mcp_tool_call SET status='failed',error=?3,finished_time=CURRENT_TIMESTAMP
         WHERE id=?1 AND conversation_id=?2 AND tool_name='ask_user_question' AND status IN ('pending','executing')",
        params![call, task, reason],
    )?;
    ensure!(claimed == 1, "问题已结束、已取消或不属于此任务，未重新启动工作");
    Ok(())
}

pub fn save(conn: &Connection, task: i64, mut record: AnswerRecord) -> Result<()> {
    schema(conn)?;
    // Persist answers for display, never persist password/secret inputs in this extra history.
    if let Some(questions) = record.questions.as_array() {
        for q in questions {
            if q["isSecret"].as_bool().unwrap_or(false) || q["is_secret"].as_bool().unwrap_or(false) {
                for key in ["question", "header", "answerKey"] {
                    if let Some(key) = q[key].as_str() {
                        if record.answers.contains_key(key) {
                            record.answers.insert(key.to_string(), "（敏感内容不保存）".into());
                        }
                    }
                }
            }
        }
    }
    conn.execute("INSERT INTO butler_task_question_answer(task_conversation_id,question_key,record_json)
                  VALUES(?1,?2,?3) ON CONFLICT(task_conversation_id,question_key) DO UPDATE SET record_json=excluded.record_json
                  WHERE json_extract(butler_task_question_answer.record_json,'$.delivery_status')='pending' ",
        params![task, record.key, serde_json::to_string(&record)?])?;
    Ok(())
}

pub fn finish_delivery(conn: &Connection, task: i64, key: &str) -> Result<()> {
    schema(conn)?;
    let raw: String = conn.query_row("SELECT record_json FROM butler_task_question_answer WHERE task_conversation_id=?1 AND question_key=?2",
        params![task,key], |r| r.get(0))?;
    let mut record: AnswerRecord = serde_json::from_str(&raw)?;
    record.delivery_status = "delivered".into();
    conn.execute("UPDATE butler_task_question_answer SET record_json=?3 WHERE task_conversation_id=?1 AND question_key=?2",
        params![task,key,serde_json::to_string(&record)?])?;
    Ok(())
}

pub fn list(conn: &Connection, task: i64) -> Result<Vec<AnswerRecord>> {
    schema(conn)?;
    let values = conn.prepare("SELECT record_json FROM butler_task_question_answer WHERE task_conversation_id=?1 ORDER BY created_at,question_key")?
        .query_map([task], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    values.into_iter().map(|value| Ok(serde_json::from_str(&value)?)).collect()
}

#[cfg(test)]
#[path = "butler_question_history_tests.rs"]
mod tests;

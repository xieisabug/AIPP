//! 管家“工作”的引用与续接：把用户引用的结果、或助手提到的工作，用真实记录身份关联回原工作，
//! 并按原工作状态有序续接（运行中排队送达，已结束则建立关联的后续任务）。
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;

#[cfg(test)]
#[path = "butler_work_tests.rs"]
mod tests;

pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS butler_work_followup(
            id INTEGER PRIMARY KEY, work_ref TEXT NOT NULL, request_key TEXT NOT NULL,
            conversation_id INTEGER NOT NULL, task_conversation_id INTEGER NOT NULL,
            instruction TEXT NOT NULL, chain INTEGER, state TEXT NOT NULL,
            result_task INTEGER, error TEXT, created_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(work_ref, request_key));
         CREATE INDEX IF NOT EXISTS idx_butler_work_followup_state ON butler_work_followup(state,task_conversation_id,id);",
    )
}

/// 一项可被引用的工作。`work_ref` 是稳定身份：`task:<任务会话>`、`exchange:<协作>`、`turn:<用户消息>`。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkView {
    pub work_ref: String,
    pub kind: String,
    pub id: i64,
    pub label: String,
    pub title: String,
    pub status: String,
    pub requester: Option<String>,
    pub result: Option<String>,
    /// 用户引用的那段内容（引用消息或结果卡片时）。
    pub excerpt: Option<String>,
    pub task_conversation_id: Option<i64>,
    pub finished: bool,
}

pub(crate) fn clip(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_string();
    }
    format!("{}…（已截断）", text.chars().take(limit).collect::<String>())
}

fn conversation(conn: &Connection, id: i64) -> anyhow::Result<(String, Option<i64>)> {
    conn.query_row("SELECT conversation_kind,assistant_id FROM conversation WHERE id=?1", [id], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()?
    .ok_or_else(|| anyhow::anyhow!("当前会话 #{id} 不存在"))
}

fn task_status(status: Option<&str>, finalized: bool) -> (&'static str, bool) {
    match status {
        Some("succeeded") => ("已完成", true),
        Some("failed") => ("失败", true),
        Some("cancelled") => ("已停止", true),
        Some("interrupted") => ("已中断", true),
        _ if finalized => ("已结束", true),
        _ => ("进行中", false),
    }
}

fn exchange_status(state: &str) -> (&'static str, bool) {
    match state {
        "delivered" | "returning" | "reply_sending" => ("已完成", true),
        "failed" => ("失败", true),
        "stopped" => ("已停止", true),
        _ => ("进行中", false),
    }
}

fn resolve_task(conn: &Connection, conversation: i64, task: i64) -> anyhow::Result<WorkView> {
    let (kind, assistant) = self::conversation(conn, conversation)?;
    let row = conn
        .query_row(
            "SELECT d.title,d.goal,d.executor_assistant_id,d.butler_conversation_id,t.butler_task_status,
                    t.butler_task_finalized_at IS NOT NULL,
                    coalesce(nullif(final.content,''),r.summary,t.butler_task_summary)
             FROM butler_task_definition d JOIN conversation t ON t.id=d.task_conversation_id
             LEFT JOIN butler_task_result r ON r.task_conversation_id=t.id
             LEFT JOIN message final ON final.id=r.final_message_id
             WHERE d.task_conversation_id=?1",
            [task],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| anyhow::anyhow!("引用的任务 #{task} 已删除或不存在"))?;
    let (title, goal, executor, owner, status, finalized, result) = row;
    // 访问范围：执行助手自己的联系人会话，或派出该任务的总管家会话。
    let allowed = (kind == "butler_contact" && assistant == Some(executor)) || owner == conversation;
    anyhow::ensure!(allowed, "引用的任务 #{task} 不属于当前会话，无权访问");
    let (status, finished) = task_status(status.as_deref(), finalized);
    Ok(WorkView {
        work_ref: format!("task:{task}"),
        kind: "task".into(),
        id: task,
        label: format!("任务《{title}》"),
        title: goal,
        status: status.into(),
        requester: Some("总管家".into()),
        result,
        excerpt: None,
        task_conversation_id: Some(task),
        finished,
    })
}

fn resolve_exchange(conn: &Connection, conversation: i64, id: i64) -> anyhow::Result<WorkView> {
    let (owner, target, sender, recipient, body, state, reply): (i64, i64, String, String, String, String, Option<String>) = conn
        .query_row(
            "SELECT owner,target,sender,recipient,body,state,reply FROM butler_peer_exchange WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow::anyhow!("引用的协作 #{id} 已删除或不存在"))?;
    anyhow::ensure!(owner == conversation || target == conversation, "引用的协作 #{id} 不属于当前会话，无权访问");
    let (status, finished) = exchange_status(&state);
    Ok(WorkView {
        work_ref: format!("exchange:{id}"),
        kind: "exchange".into(),
        id,
        label: format!("{sender} 交给 {recipient} 的工作"),
        title: body,
        status: status.into(),
        requester: Some(sender),
        result: reply,
        excerpt: None,
        task_conversation_id: None,
        finished,
    })
}

fn resolve_turn(conn: &Connection, conversation: i64, id: i64) -> anyhow::Result<WorkView> {
    let content: String = conn
        .query_row(
            "SELECT content FROM message WHERE id=?1 AND conversation_id=?2 AND message_type='user'",
            params![id, conversation],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow::anyhow!("引用的对话 #{id} 已删除或不在当前会话"))?;
    Ok(WorkView {
        work_ref: format!("turn:{id}"),
        kind: "turn".into(),
        id,
        label: format!("本会话中「{}」", clip(&content, 24)),
        title: content,
        status: "本会话对话".into(),
        requester: None,
        result: None,
        excerpt: None,
        task_conversation_id: None,
        finished: true,
    })
}

/// 引用某条回复：按它所在的那一轮找到真实所属工作（受托协作或本会话自己的对话），不靠文字或时间猜测。
fn resolve_message(conn: &Connection, conversation: i64, id: i64) -> anyhow::Result<WorkView> {
    let (owner, kind, content): (i64, String, String) = conn
        .query_row("SELECT conversation_id,message_type,content FROM message WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or_else(|| anyhow::anyhow!("引用的消息 #{id} 已删除"))?;
    anyhow::ensure!(owner == conversation, "引用的消息 #{id} 不在当前会话，无权访问");
    anyhow::ensure!(matches!(kind.as_str(), "response" | "assistant"), "只能引用助手的回复或结果");
    let (user, meta): (i64, Option<String>) = conn
        .query_row(
            "SELECT id,metadata_json FROM message WHERE conversation_id=?1 AND message_type='user' AND id<?2
             ORDER BY id DESC LIMIT 1",
            params![conversation, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow::anyhow!("找不到消息 #{id} 所属的那一轮"))?;
    let meta = meta.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).unwrap_or_default();
    let mut view = if meta["origin"] == "butler_peer" {
        let via = meta["exchange_id"].as_i64().ok_or_else(|| anyhow::anyhow!("协作消息缺少 exchange_id"))?;
        let work = if meta["leg"] == "request" {
            Some(via)
        } else {
            conn.query_row("SELECT parent FROM butler_peer_exchange WHERE id=?1", [via], |r| r.get::<_, Option<i64>>(0))
                .optional()?
                .flatten()
        };
        match work {
            Some(exchange) => resolve_exchange(conn, conversation, exchange)?,
            None => resolve_turn(conn, conversation, meta["root"].as_i64().unwrap_or(user))
                .or_else(|_| resolve_turn(conn, conversation, user))?,
        }
    } else {
        resolve_turn(conn, conversation, user)?
    };
    view.excerpt = Some(content);
    Ok(view)
}

/// 解析用户引用的对象并核对当前会话、助手与可访问范围。
pub fn resolve(conn: &Connection, conversation: i64, kind: &str, id: i64) -> anyhow::Result<WorkView> {
    match kind {
        "message" => resolve_message(conn, conversation, id),
        "task" => resolve_task(conn, conversation, id),
        "exchange" => resolve_exchange(conn, conversation, id),
        "turn" => resolve_turn(conn, conversation, id),
        other => anyhow::bail!("未知引用类型 {other}"),
    }
}

pub fn parse_ref(work_ref: &str) -> anyhow::Result<(&str, i64)> {
    let (kind, id) = work_ref.split_once(':').ok_or_else(|| anyhow::anyhow!("work_ref 格式应为 类型:ID，例如 task:12"))?;
    let id = id.trim().parse::<i64>().map_err(|_| anyhow::anyhow!("work_ref 中的 ID 无效：{work_ref}"))?;
    anyhow::ensure!(matches!(kind, "task" | "exchange" | "turn"), "未知工作类型 {kind}");
    Ok((kind, id))
}

/// 当前会话里近期可能被提到的工作（未引用时帮助判断用户指的是哪一项）。
pub fn recent(conn: &Connection, conversation: i64, limit: usize) -> anyhow::Result<Vec<WorkView>> {
    let (kind, assistant) = self::conversation(conn, conversation)?;
    let mut refs: Vec<(String, i64, i64)> = Vec::new();
    let (filter, key) = if kind == "butler_contact" {
        ("d.executor_assistant_id=?1", assistant.unwrap_or(-1))
    } else {
        ("d.butler_conversation_id=?1", conversation)
    };
    let tasks: Vec<i64> = conn
        .prepare(&format!("SELECT d.task_conversation_id FROM butler_task_definition d WHERE {filter} ORDER BY d.id DESC LIMIT ?2"))?
        .query_map(params![key, limit as i64], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    // 按发生先后排序：任务看其首条消息 ID，协作看投递消息 ID（两者同属全局递增的消息 ID）。
    for task in tasks {
        let order: i64 = conn
            .query_row("SELECT coalesce(min(id),0) FROM message WHERE conversation_id=?1", [task], |r| r.get(0))?;
        refs.push(("task".into(), task, order));
    }
    let exchanges: Vec<(i64, i64)> = conn
        .prepare("SELECT id,coalesce(input_id,id) FROM butler_peer_exchange WHERE target=?1 AND parent IS NULL
                  ORDER BY id DESC LIMIT ?2")?
        .query_map(params![conversation, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, order) in exchanges {
        refs.push(("exchange".into(), id, order));
    }
    refs.sort_by(|a, b| b.2.cmp(&a.2));
    let mut views = Vec::new();
    for (kind, id, _) in refs.into_iter().take(limit) {
        if let Ok(view) = resolve(conn, conversation, &kind, id) {
            views.push(view);
        }
    }
    Ok(views)
}

/// 一次“继续执行”请求。同一工作的相同 request_key 只执行一次；同一任务的请求按顺序送达。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Followup {
    pub id: i64,
    pub work_ref: String,
    pub conversation_id: i64,
    pub task_conversation_id: i64,
    pub instruction: String,
    pub chain: Option<i64>,
    pub state: String,
    pub result_task: Option<i64>,
    pub error: Option<String>,
}

const FOLLOWUP: &str = "SELECT id,work_ref,conversation_id,task_conversation_id,instruction,chain,state,result_task,error FROM butler_work_followup";

fn followup_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Followup> {
    Ok(Followup {
        id: r.get(0)?,
        work_ref: r.get(1)?,
        conversation_id: r.get(2)?,
        task_conversation_id: r.get(3)?,
        instruction: r.get(4)?,
        chain: r.get(5)?,
        state: r.get(6)?,
        result_task: r.get(7)?,
        error: r.get(8)?,
    })
}

pub fn enqueue_followup(
    conn: &mut Connection,
    conversation: i64,
    task: i64,
    request_key: &str,
    instruction: &str,
    chain: Option<i64>,
) -> anyhow::Result<(Followup, bool)> {
    let work_ref = format!("task:{task}");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(old) = tx
        .query_row(&format!("{FOLLOWUP} WHERE work_ref=?1 AND request_key=?2"), params![work_ref, request_key], followup_row)
        .optional()?
    {
        anyhow::ensure!(
            old.conversation_id == conversation && old.instruction == instruction,
            "同一 request_id 已用于不同的续接请求"
        );
        return Ok((old, false));
    }
    tx.execute(
        "INSERT INTO butler_work_followup(work_ref,request_key,conversation_id,task_conversation_id,instruction,chain,state)
         VALUES (?1,?2,?3,?4,?5,?6,'queued')",
        params![work_ref, request_key, conversation, task, instruction, chain],
    )?;
    let created = tx.query_row(&format!("{FOLLOWUP} WHERE id=?1"), [tx.last_insert_rowid()], followup_row)?;
    tx.commit()?;
    Ok((created, true))
}

/// 每个任务最早的一条待送续接（同一任务严格按顺序，前一条处理完才轮到下一条）。
pub fn next_followups(conn: &Connection) -> rusqlite::Result<Vec<Followup>> {
    conn.prepare(&format!(
        "{FOLLOWUP} WHERE state='queued' AND id IN (SELECT min(id) FROM butler_work_followup
            WHERE state IN ('queued','sending') GROUP BY task_conversation_id) ORDER BY id"
    ))?
    .query_map([], followup_row)?
    .collect()
}

pub fn claim_followup(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE butler_work_followup SET state='sending' WHERE id=?1 AND state='queued' AND NOT EXISTS(
            SELECT 1 FROM butler_work_followup o WHERE o.task_conversation_id=
              (SELECT task_conversation_id FROM butler_work_followup WHERE id=?1) AND o.state='sending')",
        [id],
    )? == 1)
}

pub fn finish_followup(conn: &Connection, id: i64, state: &str, result_task: Option<i64>, error: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE butler_work_followup SET state=?2,result_task=coalesce(?3,result_task),error=?4 WHERE id=?1 AND state='sending'",
        params![id, state, result_task, error],
    )?;
    Ok(())
}

/// 启动收尾：投递中退出的续接结果未知，不重复执行。
pub fn recover_followups(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE butler_work_followup SET state='failed',
            error='送达过程中应用退出，是否已开始执行未知，未重复执行；如需继续请重新发起'
         WHERE state='sending'",
        [],
    )
}

/// 会话发起、仍在排队的续接数量。
pub fn pending_followups(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM butler_work_followup WHERE state IN ('queued','sending')", [], |r| r.get(0))
}

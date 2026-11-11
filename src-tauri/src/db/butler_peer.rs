use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[cfg(test)]
#[path = "butler_peer_tests.rs"]
mod tests;

pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_peer_exchange(
        id INTEGER PRIMARY KEY, root INTEGER NOT NULL, owner INTEGER NOT NULL, target INTEGER NOT NULL,
        sender TEXT NOT NULL, recipient TEXT NOT NULL, request_key TEXT NOT NULL,
        body TEXT NOT NULL, state TEXT NOT NULL, input_id INTEGER, reply TEXT, error TEXT,
        round_no INTEGER NOT NULL, max_rounds INTEGER NOT NULL, stop_requested INTEGER NOT NULL DEFAULT 0,
        UNIQUE(root, request_key),
        FOREIGN KEY(owner) REFERENCES conversation(id) ON DELETE CASCADE,
        FOREIGN KEY(target) REFERENCES conversation(id) ON DELETE CASCADE);
        CREATE INDEX IF NOT EXISTS idx_butler_peer_active ON butler_peer_exchange(state,id);
        CREATE INDEX IF NOT EXISTS idx_butler_peer_input ON butler_peer_exchange(input_id);")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_peer_stop(
        root INTEGER PRIMARY KEY, conversation_id INTEGER NOT NULL, reason TEXT NOT NULL,
        created_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")?;
    // 用户要求执行助手直接汇报时，答复以该助手名义展示给用户，发起方不再转述。
    let has_report_column: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_peer_exchange') WHERE name='report_to_user')",
        [], |r| r.get(0))?;
    if !has_report_column {
        conn.execute_batch("ALTER TABLE butler_peer_exchange ADD COLUMN report_to_user INTEGER NOT NULL DEFAULT 0")?;
    }
    // 失败/停止回执：NULL 不需要；pending 待送；sending 投递中；delivered 已送达；failed 多次投递失败
    let has_receipt_column: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_peer_exchange') WHERE name='receipt_state')",
        [], |r| r.get(0))?;
    if !has_receipt_column {
        conn.execute_batch("ALTER TABLE butler_peer_exchange ADD COLUMN receipt_state TEXT")?;
    }
    let has_receipt_attempts: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_peer_exchange') WHERE name='receipt_attempts')",
        [], |r| r.get(0))?;
    if !has_receipt_attempts {
        conn.execute_batch("ALTER TABLE butler_peer_exchange ADD COLUMN receipt_attempts INTEGER NOT NULL DEFAULT 0")?;
    }
    // 有界多层协作：parent 指向受托方正在处理的上级委托，depth 为层数，expires_at 为整条协作链的有效期。
    for (column, ddl) in [
        ("parent", "ALTER TABLE butler_peer_exchange ADD COLUMN parent INTEGER"),
        ("depth", "ALTER TABLE butler_peer_exchange ADD COLUMN depth INTEGER NOT NULL DEFAULT 1"),
        ("expires_at", "ALTER TABLE butler_peer_exchange ADD COLUMN expires_at TEXT"),
        // 整条协作链（同一项工作）的标识：轮数预算、有效期、停止都按链计算；独立任务发起的协作挂到派出该任务的工作上。
        ("chain", "ALTER TABLE butler_peer_exchange ADD COLUMN chain INTEGER"),
        // 停止时实际打断了什么、哪些无法确认已停止，供回执与界面如实说明。
        ("stop_note", "ALTER TABLE butler_peer_exchange ADD COLUMN stop_note TEXT"),
    ] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('butler_peer_exchange') WHERE name=?1)",
            [column], |r| r.get(0))?;
        if !exists {
            conn.execute_batch(ddl)?;
        }
    }
    conn.execute_batch("UPDATE butler_peer_exchange SET chain=root WHERE chain IS NULL;
        CREATE INDEX IF NOT EXISTS idx_butler_peer_parent ON butler_peer_exchange(parent);
        CREATE INDEX IF NOT EXISTS idx_butler_peer_chain ON butler_peer_exchange(chain);
        -- 独立任务属于哪项工作（派出它的那一轮所在的协作链），以及它是否是某个任务的关联后续执行。
        CREATE TABLE IF NOT EXISTS butler_task_link(
            task_conversation_id INTEGER PRIMARY KEY, chain INTEGER NOT NULL, followup_of INTEGER,
            created_time TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
        CREATE INDEX IF NOT EXISTS idx_butler_task_link_chain ON butler_task_link(chain);")?;
    super::butler_work::schema(conn)?;
    super::butler_timeline::schema(conn)
}

#[derive(Clone, Debug, Serialize)]
pub struct Exchange {
    pub id: i64,
    pub root: i64,
    pub owner: i64,
    pub target: i64,
    pub sender: String,
    pub recipient: String,
    pub body: String,
    pub state: String,
    pub input_id: Option<i64>,
    pub reply: Option<String>,
    pub error: Option<String>,
    pub round_no: i64,
    pub max_rounds: i64,
    pub stop_requested: bool,
    /// 答复面向用户：受托助手在自己的会话里以自身身份交付或提问，发起方不再转述。
    /// false 表示内部协作，答复只作为发起方继续工作的资料。
    pub report_to_user: bool,
    pub receipt_state: Option<String>,
    /// 受托方发起本协作时正在处理的上级委托（多层协作）。
    pub parent: Option<i64>,
    pub depth: i64,
    pub expires_at: Option<DateTime<Utc>>,
    /// 所属工作（协作链）标识。
    pub chain: i64,
    pub stop_note: Option<String>,
}
const SELECT: &str = "SELECT id,root,owner,target,sender,recipient,body,state,input_id,reply,error,round_no,max_rounds,stop_requested,report_to_user,receipt_state,parent,depth,expires_at,coalesce(chain,root),stop_note FROM butler_peer_exchange";
/// 仍在进行、尚未交付的协作状态。
const ACTIVE: &str = "('queued','sending','waiting','returning','reply_sending')";
/// 协作链最多层数：总管家→甲→乙→丙。
pub const MAX_DEPTH: i64 = 3;
/// 整条协作链的有效期；排队或等待超时后给发起方失败回执，避免互相等待卡死。
pub const TTL_MINUTES: i64 = 120;
fn row(r: &Row<'_>) -> rusqlite::Result<Exchange> {
    Ok(Exchange {
        id: r.get(0)?,
        root: r.get(1)?,
        owner: r.get(2)?,
        target: r.get(3)?,
        sender: r.get(4)?,
        recipient: r.get(5)?,
        body: r.get(6)?,
        state: r.get(7)?,
        input_id: r.get(8)?,
        reply: r.get(9)?,
        error: r.get(10)?,
        round_no: r.get(11)?,
        max_rounds: r.get(12)?,
        stop_requested: r.get(13)?,
        report_to_user: r.get(14)?,
        receipt_state: r.get(15)?,
        parent: r.get(16)?,
        depth: r.get(17)?,
        expires_at: r.get(18)?,
        chain: r.get(19)?,
        stop_note: r.get(20)?,
    })
}

/// 会话当前（或某条消息之前）这一轮属于哪项工作：协作投递触发的回合沿用其 root，否则就是该用户消息本身。
pub fn current_root(conn: &Connection, conversation: i64, before: Option<i64>) -> rusqlite::Result<Option<i64>> {
    let latest: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT id,metadata_json FROM message WHERE conversation_id=?1 AND message_type='user'
               AND (?2 IS NULL OR id<=?2) ORDER BY id DESC LIMIT 1",
            params![conversation, before],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(latest.map(|(id, meta)| {
        let meta = meta
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .unwrap_or_default();
        if meta["origin"] == "butler_peer" { meta["root"].as_i64().unwrap_or(id) } else { id }
    }))
}

/// 会话当前这一轮在做哪项工作：root 为所属一轮；exchange 为受托处理的那项协作
/// （请求方向即该协作；答复/回执方向是接收方继续处理的上级协作；None 表示会话自己发起的工作）。
#[derive(Debug, Clone, PartialEq)]
pub struct TurnWork {
    pub root: i64,
    pub exchange: Option<i64>,
}

pub fn turn_work(conn: &Connection, conversation: i64) -> rusqlite::Result<Option<TurnWork>> {
    let latest: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT id,metadata_json FROM message WHERE conversation_id=?1 AND message_type='user' ORDER BY id DESC LIMIT 1",
            [conversation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, meta)) = latest else { return Ok(None) };
    let meta = meta.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).unwrap_or_default();
    if meta["origin"] != "butler_peer" {
        return Ok(Some(TurnWork { root: id, exchange: None }));
    }
    let root = meta["root"].as_i64().unwrap_or(id);
    let Some(via) = meta["exchange_id"].as_i64() else { return Ok(Some(TurnWork { root, exchange: None })) };
    let exchange = if meta["leg"] == "request" {
        Some(via)
    } else {
        conn.query_row("SELECT parent FROM butler_peer_exchange WHERE id=?1", [via], |r| r.get::<_, Option<i64>>(0))
            .optional()?
            .flatten()
    };
    Ok(Some(TurnWork { root, exchange }))
}

/// 某项协作及其全部下级协作。
pub fn subtree(conn: &Connection, id: i64) -> rusqlite::Result<Vec<Exchange>> {
    conn.prepare(&format!(
        "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION SELECT e.id FROM butler_peer_exchange e JOIN sub ON e.parent=sub.id)
         {SELECT} WHERE id IN (SELECT id FROM sub) ORDER BY id"
    ))?
    .query_map([id], row)?
    .collect()
}

/// 停止一项协作及其下级（不影响同一轮里并行的其他协作）。返回涉及的协作 ID。
pub fn stop_subtree(conn: &Connection, id: i64) -> rusqlite::Result<Vec<i64>> {
    let ids: Vec<i64> = subtree(conn, id)?.into_iter().map(|e| e.id).collect();
    super::butler_work::stop_exchange_followups(conn, &ids)?;
    for id in &ids {
        stop_rows(conn, "id=?1", *id)?;
    }
    Ok(ids)
}

/// 某一轮（root）所属的工作链：已有协作沿用其链；独立任务里的新一轮挂到派出该任务的工作上。
pub fn chain_of(conn: &Connection, root: i64, owner: i64) -> rusqlite::Result<i64> {
    if let Some(chain) = conn
        .query_row("SELECT coalesce(chain,root) FROM butler_peer_exchange WHERE root=?1 LIMIT 1", [root], |r| r.get(0))
        .optional()?
    {
        return Ok(chain);
    }
    Ok(conn
        .query_row("SELECT chain FROM butler_task_link WHERE task_conversation_id=?1", [owner], |r| r.get(0))
        .optional()?
        .unwrap_or(root))
}

/// 记录独立任务所属的工作（派出它时所在的协作链）及其关联的原任务（续接执行时）。
pub fn link_task(conn: &Connection, task: i64, chain: i64, followup_of: Option<i64>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO butler_task_link(task_conversation_id,chain,followup_of) VALUES (?1,?2,?3)
         ON CONFLICT(task_conversation_id) DO NOTHING",
        params![task, chain, followup_of],
    )?;
    Ok(())
}

pub fn task_link(conn: &Connection, task: i64) -> rusqlite::Result<Option<(i64, Option<i64>)>> {
    conn.query_row(
        "SELECT chain,followup_of FROM butler_task_link WHERE task_conversation_id=?1",
        [task],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// 属于该工作链的独立任务。
pub fn linked_tasks(conn: &Connection, chain: i64) -> rusqlite::Result<Vec<i64>> {
    conn.prepare("SELECT task_conversation_id FROM butler_task_link WHERE chain=?1 ORDER BY task_conversation_id")?
        .query_map([chain], |r| r.get(0))?
        .collect()
}

/// 该工作链涉及的各轮 root（含链本身），用于判断某个会话当前这一轮是否属于这项工作。
pub fn chain_roots(conn: &Connection, chain: i64) -> rusqlite::Result<Vec<i64>> {
    let mut roots: Vec<i64> = conn
        .prepare("SELECT DISTINCT root FROM butler_peer_exchange WHERE coalesce(chain,root)=?1")?
        .query_map([chain], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if !roots.contains(&chain) {
        roots.push(chain);
    }
    Ok(roots)
}

/// 工作链涉及的会话（发起方与接收方）。
pub fn chain_conversations(conn: &Connection, chain: i64) -> rusqlite::Result<Vec<i64>> {
    conn.prepare("SELECT owner FROM butler_peer_exchange WHERE coalesce(chain,root)=?1
                  UNION SELECT target FROM butler_peer_exchange WHERE coalesce(chain,root)=?1")?
        .query_map([chain], |r| r.get(0))?
        .collect()
}

fn assistant_of(conn: &Connection, conversation: i64) -> rusqlite::Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT assistant_id FROM conversation WHERE id=?1", [conversation], |r| r.get::<_, Option<i64>>(0))
        .optional()?
        .flatten())
}

fn read(conn: &Connection, id: i64) -> rusqlite::Result<Exchange> {
    conn.query_row(&format!("{SELECT} WHERE id=?1"), [id], row)
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<Exchange>> {
    read(conn, id).optional()
}
pub fn pending_exchanges(conn: &Connection) -> rusqlite::Result<Vec<Exchange>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE state IN ('queued','waiting','returning') ORDER BY id"))?;
    let rows = stmt.query_map([], row)?;
    rows.collect()
}

pub struct NewRequest<'a> {
    pub owner: i64,
    pub target: i64,
    pub sender: &'a str,
    pub recipient: &'a str,
    pub key: &'a str,
    pub body: &'a str,
    pub limit: i64,
    pub tool_message_id: Option<i64>,
    /// None 按默认规则：用户或助手自主发起的委托由受托助手向用户交付；
    /// 受托期间为完成上级委托而请求的协助、独立任务请来的协助默认是内部协作。
    pub report_to_user: Option<bool>,
}

pub fn enqueue(conn: &mut Connection, req: NewRequest<'_>) -> anyhow::Result<Exchange> {
    anyhow::ensure!(req.owner != req.target, "不能向同一个联系人派发协作请求");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    super::butler_control::ensure_delegation(&tx, req.owner)?;
    let (turn, meta): (i64, Option<String>) = tx.query_row(
        "SELECT id,metadata_json FROM message WHERE conversation_id=?1 AND message_type='user'
           AND (?2 IS NULL OR id<=?2) ORDER BY id DESC LIMIT 1",
        params![req.owner, req.tool_message_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let meta =
        meta.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).unwrap_or_default();
    // 当前回合由协作消息触发时，新请求属于同一条协作链：请求方向的上级就是这条请求；
    // 答复/回执方向沿用受托方当时正在处理的上级委托。
    let (root, parent) = if meta["origin"] == "butler_peer" {
        let root = meta["root"].as_i64().ok_or_else(|| anyhow::anyhow!("协作消息缺少 root"))?;
        let via = meta["exchange_id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("协作消息缺少 exchange_id"))?;
        let parent = match meta["leg"].as_str() {
            Some("request") => Some(via),
            Some("reply" | "receipt") => tx
                .query_row("SELECT parent FROM butler_peer_exchange WHERE id=?1", [via], |r| {
                    r.get::<_, Option<i64>>(0)
                })
                .optional()?
                .flatten(),
            other => anyhow::bail!("未知协作消息方向 {other:?}"),
        };
        (root, parent.map(|id| read(&tx, id)).transpose()?)
    } else {
        (turn, None)
    };
    // 独立任务请来的协助是任务的资料：最终交付由任务本身负责，默认不让协助方直接向用户交付。
    let owner_is_task = tx
        .query_row("SELECT conversation_kind FROM conversation WHERE id=?1", [req.owner], |r| r.get::<_, String>(0))
        .optional()?
        .is_some_and(|kind| kind == "butler_task");
    let report_to_user = req.report_to_user.unwrap_or(parent.is_none() && !owner_is_task);
    let chain = match &parent {
        Some(p) => p.chain,
        None => chain_of(&tx, root, req.owner)?,
    };
    if let Some(old) = tx
        .query_row(
            &format!("{SELECT} WHERE root=?1 AND request_key=?2"),
            params![root, req.key],
            row,
        )
        .optional()?
    {
        anyhow::ensure!(
            old.owner == req.owner && old.target == req.target && old.body == req.body
                && old.report_to_user == report_to_user,
            "同一 request_id 已用于不同请求"
        );
        return Ok(old);
    }
    if let Some(anchor) = req.tool_message_id {
        let superseded: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM message WHERE conversation_id=?1 AND message_type='user' AND id>?2)",
            params![req.owner,anchor], |row| row.get(0))?;
        anyhow::ensure!(!superseded, "发起工具所在回合已结束，不能代表较新的用户消息派工");
    }
    if let Some(p) = &parent {
        anyhow::ensure!(
            p.target == req.owner && p.state == "waiting" && !p.stop_requested,
            "所属协作 #{} 已结束或已停止，不能再为它联系其他助手；请直接说明现有结果",
            p.id
        );
        anyhow::ensure!(
            p.depth < MAX_DEPTH,
            "协作已达到 {MAX_DEPTH} 层上限，不能继续转派；请自行完成，或在答复中写明还需要谁协助"
        );
        // 循环保护按助手身份判断：协作链上已出现的助手（无论在联系人、任务还是主会话里）不能再被拉回来。
        let target_assistant = assistant_of(&tx, req.target)?;
        let mut cursor = Some(p.clone());
        while let Some(ancestor) = cursor {
            let mut seen = vec![ancestor.owner, ancestor.target];
            let same_identity = match target_assistant {
                Some(assistant) => [ancestor.owner, ancestor.target]
                    .into_iter()
                    .map(|conversation| assistant_of(&tx, conversation))
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .contains(&Some(assistant)),
                None => false,
            };
            seen.retain(|conversation| *conversation == req.target);
            anyhow::ensure!(
                seen.is_empty() && !same_identity,
                "{} 已在本条协作链中，不能形成循环协作；需要对方补充信息时请在答复中直接提出",
                req.recipient
            );
            cursor = ancestor.parent.map(|id| read(&tx, id)).transpose()?;
        }
    }
    // 轮数预算与停止状态按整条工作链计算，不能靠切换会话或新一轮重新计数。
    let (count, limit, stopped, chain_expiry): (i64, Option<i64>, i64, Option<DateTime<Utc>>) = tx.query_row(
        "SELECT count(*),min(max_rounds),count(CASE WHEN stop_requested=1 THEN 1 END),min(expires_at)
         FROM butler_peer_exchange WHERE coalesce(chain,root)=?1",
        [chain],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let limit = limit.unwrap_or(req.limit);
    anyhow::ensure!(stopped == 0, "该协作已停止，请向用户汇报现有结果");
    anyhow::ensure!(count < limit, "协作已达到 {limit} 轮，请汇报已有结果和分歧");
    let busy: bool = tx.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM butler_peer_exchange WHERE root=?1 AND target=?2 AND state IN {ACTIVE})"),
        params![root, req.target],
        |r| r.get(0),
    )?;
    anyhow::ensure!(!busy, "与该助手的上一轮仍在进行，收到回复后可继续发送");
    let depth = parent.as_ref().map_or(1, |p| p.depth + 1);
    // 有效期由整条链首次发起时确定，后续轮次与下级继承，不重新计时。
    let expires_at = parent
        .as_ref()
        .and_then(|p| p.expires_at)
        .or(chain_expiry)
        .unwrap_or_else(|| Utc::now() + chrono::Duration::minutes(TTL_MINUTES));
    tx.execute("INSERT INTO butler_peer_exchange(root,owner,target,sender,recipient,request_key,body,state,round_no,max_rounds,report_to_user,parent,depth,expires_at,chain)
        VALUES (?1,?2,?3,?4,?5,?6,?7,'queued',?8,?9,?10,?11,?12,?13,?14)",
        params![root,req.owner,req.target,req.sender,req.recipient,req.key,req.body,count+1,limit,report_to_user,
            parent.as_ref().map(|p| p.id),depth,expires_at,chain])?;
    let ex = read(&tx, tx.last_insert_rowid())?;
    let anchor = match req.tool_message_id {
        Some(id) => id,
        None => tx.query_row("SELECT max(id) FROM message WHERE conversation_id=?1", [req.owner], |r| r.get(0))?,
    };
    super::butler_timeline::record(&tx,ex.id,req.owner,"request",true,anchor)?;
    tx.commit()?;
    Ok(ex)
}

pub fn claim(conn: &Connection, ex: &Exchange) -> rusqlite::Result<bool> {
    if ex.stop_requested || !matches!(ex.state.as_str(), "queued" | "returning") {
        return Ok(false);
    }
    let (destination, next) =
        if ex.state == "queued" { (ex.target, "sending") } else { (ex.owner, "reply_sending") };
    super::butler_control::schema(conn)?;

    // 受托方等待下级答复时，它正在处理的上级委托仍是 waiting；下级答复必须能送回去，不能被上级挡住。
    Ok(conn.execute(
        "UPDATE butler_peer_exchange SET state=?1 WHERE id=?2 AND state=?3 AND stop_requested=0
        AND NOT EXISTS(SELECT 1 FROM butler_task_control c WHERE c.conversation_id=?4 AND (c.paused=1 OR c.cancelled=1))
        AND (?3!='queued' OR NOT EXISTS(SELECT 1 FROM butler_task_control c WHERE c.conversation_id=butler_peer_exchange.owner AND (c.paused=1 OR c.delegation_stopped=1 OR c.cancelled=1)))
        AND NOT EXISTS (
        SELECT 1 FROM butler_peer_exchange WHERE id!=?2 AND id IS NOT ?5 AND
        ((target=?4 AND state IN ('sending','waiting')) OR (owner=?4 AND state='reply_sending')))",
        params![next, ex.id, ex.state, destination, ex.parent],
    )? == 1)
}

// Link the trigger and its provenance in one transaction before the model can start work.
pub fn write_input(
    conn: &mut Connection,
    id: i64,
    leg: &str,
    conversation: i64,
    text: &str,
    model_id: i64,
    model: &str,
) -> anyhow::Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ex = tx.query_row(&format!("{SELECT} WHERE id=?1"), [id], row)?;
    anyhow::ensure!(matches!(leg, "request" | "reply" | "receipt"), "未知投递方向 {leg}");
    let audience = audience(&tx, &ex, leg)?;
    if leg == "receipt" {
        anyhow::ensure!(
            ex.owner == conversation && ex.receipt_state.as_deref() == Some("sending"),
            "exchange={id} 回执投递已处理或归属不符"
        );
        let meta = serde_json::json!({"origin":"butler_peer","exchange_id":id,"root":ex.root,"leg":"receipt","sender_bot":ex.recipient,
            "audience":audience,"parent":ex.parent});
        let now = chrono::Utc::now();
        tx.execute("INSERT INTO message(conversation_id,message_type,content,created_time,llm_model_id,llm_model_name,
            token_count,input_token_count,output_token_count,metadata_json) VALUES (?1,'user',?2,?3,?4,?5,0,0,0,?6)",
            params![conversation,text,now,model_id,model,meta.to_string()])?;
        let message = tx.last_insert_rowid();
        tx.execute("UPDATE conversation SET updated_time=?1 WHERE id=?2", params![now, conversation])?;
        tx.execute("DELETE FROM conversation_summary WHERE conversation_id=?1", [conversation])?;
        tx.execute("UPDATE butler_peer_exchange SET receipt_state='delivered' WHERE id=?1", [id])?;
        tx.commit()?;
        return Ok(message);
    }
    let request = leg == "request";
    // Claiming and delivery are separate: controls may change while a request waits
    // for runtime admission. Never turn a queued delegation into work after stopping it.
    if request { super::butler_control::ensure_delegation(&tx, ex.owner)?; }
    super::butler_control::ensure_turn(&tx, conversation)?;

    let (expected, dest, speaker) = if request {
        ("sending", ex.target, ex.sender)
    } else {
        ("reply_sending", ex.owner, ex.recipient)
    };
    anyhow::ensure!(
        dest == conversation && ex.state == expected,
        "exchange={id} {leg} 投递已停止或归属不符"
    );
    let meta = serde_json::json!({"execution_identity":super::butler_execution::Identity::peer(ex.chain,id),"origin":"butler_peer","exchange_id":id,"root":ex.root,"leg":leg,"sender_bot":speaker,
        "audience":audience,"parent":ex.parent});
    let now = chrono::Utc::now();
    tx.execute("INSERT INTO message(conversation_id,message_type,content,created_time,llm_model_id,llm_model_name,
        token_count,input_token_count,output_token_count,metadata_json) VALUES (?1,'user',?2,?3,?4,?5,0,0,0,?6)",
        params![conversation,text,now,model_id,model,meta.to_string()])?;
    let message = tx.last_insert_rowid();
    tx.execute("UPDATE conversation SET updated_time=?1 WHERE id=?2", params![now, conversation])?;
    tx.execute("DELETE FROM conversation_summary WHERE conversation_id=?1", [conversation])?;
    tx.execute("UPDATE butler_peer_exchange SET state=?1,input_id=CASE WHEN ?2 THEN ?3 ELSE input_id END WHERE id=?4",
        params![if request{"waiting"}else{"delivered"},request,message,id])?;
    super::butler_timeline::record(&tx,id,conversation,leg,false,message)?;
    tx.commit()?;
    Ok(message)
}

/// 投递消息触发的回合面向谁：user 表示接收方本轮最终回复给用户看；peer 表示只回给其他助手。
/// 请求方向取决于本协作；答复/回执方向取决于接收方当时正在处理的上级委托（没有上级就面向用户）。
pub fn audience(conn: &Connection, ex: &Exchange, leg: &str) -> rusqlite::Result<&'static str> {
    let to_user = if leg == "request" {
        ex.report_to_user
    } else if let Some(parent) = ex.parent {
        conn.query_row("SELECT report_to_user FROM butler_peer_exchange WHERE id=?1", [parent], |r| r.get(0))
            .optional()?
            .unwrap_or(true)
    } else {
        true
    };
    Ok(if to_user { "user" } else { "peer" })
}

/// 受托方为本协作向其他助手发出、尚未结束的下级协作（含待送回执）。
pub fn has_open_children(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM butler_peer_exchange WHERE parent=?1
            AND (state IN {ACTIVE} OR receipt_state IN ('pending','sending')))"),
        [id],
        |r| r.get(0),
    )
}

/// 会话（如独立任务）自己发起、仍未闭环的协作。任务据此暂缓收尾，避免把“已请人协助”当成最终交付。
pub fn has_open_owned(conn: &Connection, conversation: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM butler_peer_exchange WHERE owner=?1
            AND ((state IN {ACTIVE} AND stop_requested=0) OR receipt_state IN ('pending','sending')))"),
        [conversation],
        |r| r.get(0),
    )
}

/// 超过有效期仍在排队或等待的协作：失败并回执发起方。等待用户回答/审批中的回合由调用方跳过。
pub fn expire(conn: &Connection, ex: &Exchange, now: DateTime<Utc>) -> rusqlite::Result<bool> {
    if ex.expires_at.is_none_or(|at| at > now) {
        return Ok(false);
    }
    Ok(conn.execute(
        "UPDATE butler_peer_exchange SET state='failed',
            error='协作超过有效期仍未完成，已停止等待；对方可能已部分执行，结果以对方会话为准',
            receipt_state=coalesce(receipt_state,'pending')
         WHERE id=?1 AND state IN ('queued','waiting') AND stop_requested=0",
        [ex.id],
    )? == 1)
}

/// 标记失败并排队一次回执给发起方，避免发起方无限等待。
pub fn fail(conn: &Connection, id: i64, detail: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE butler_peer_exchange SET state='failed',error=?1,receipt_state=coalesce(receipt_state,'pending')
         WHERE id=?2 AND state!='stopped'",
        params![detail, id],
    )?;
    Ok(())
}

/// 进程启动时收尾：投递中的协作结果未知（不重复投递）。
/// 回执写入消息与标记 delivered 在同一事务，仍为 sending 说明回执消息尚未写入，退回待送可安全重发。
pub fn recover(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE butler_peer_exchange SET state='failed',error='投递过程中应用退出，对方是否已收到并执行未知，未重复投递',
            receipt_state=coalesce(receipt_state,'pending')
         WHERE state IN ('sending','reply_sending')",
        [],
    )?;
    conn.execute(
        "UPDATE butler_peer_exchange SET receipt_state='pending' WHERE receipt_state='sending'",
        [],
    )?;
    // 旧版本遗留：unknown（投递中退出）与首次失败即放弃的 failed（未记录重试次数）都未送达，恢复为待送
    conn.execute(
        "UPDATE butler_peer_exchange SET receipt_state='pending'
         WHERE receipt_state='unknown' OR (receipt_state='failed' AND receipt_attempts=0)",
        [],
    )?;
    Ok(())
}

/// 待送达的失败/停止回执。
pub fn pending_receipts(conn: &Connection) -> rusqlite::Result<Vec<Exchange>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE receipt_state='pending' ORDER BY id"))?;
    let rows = stmt.query_map([], row)?;
    rows.collect()
}

/// 领取一次回执投递；同一发起会话同时只投递一条。
pub fn claim_receipt(conn: &Connection, ex: &Exchange) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE butler_peer_exchange SET receipt_state='sending' WHERE id=?1 AND receipt_state='pending' AND NOT EXISTS (
            SELECT 1 FROM butler_peer_exchange WHERE id!=?1 AND owner=?2
              AND (state='reply_sending' OR receipt_state='sending'))",
        params![ex.id, ex.owner],
    )? == 1)
}

pub const MAX_RECEIPT_ATTEMPTS: i64 = 5;

/// 回执投递失败：仍为 sending 说明消息未写入（写入与 delivered 同一事务），退回待送重试；
/// 已 delivered 的不受影响，因此不会重复送达。超过上限后标记 failed。
pub fn fail_receipt(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE butler_peer_exchange SET receipt_attempts=receipt_attempts+1,
            receipt_state=CASE WHEN receipt_attempts+1>=?2 THEN 'failed' ELSE 'pending' END
         WHERE id=?1 AND receipt_state='sending'",
        params![id, MAX_RECEIPT_ATTEMPTS],
    )?;
    Ok(())
}

struct ReplyCandidate {
    id: i64,
    role: String,
    body: String,
    finished: bool,
    metadata: Option<String>,
    tool_calls: Option<String>,
}

pub fn settle(conn: &Connection, ex: &Exchange, recovering: bool) -> anyhow::Result<bool> {
    let trigger = ex.input_id.ok_or_else(|| anyhow::anyhow!("exchange={} 缺少接收消息", ex.id))?;
    // 受托方还在等它请来的协助：本协作尚未完成，不能把“已请人协助”当成答复。
    if has_open_children(conn, ex.id)? {
        return Ok(false);
    }
    // 下级协作的答复与回执属于本项工作的继续，不是新的用户消息。
    let boundary: Option<i64> = conn.query_row(
        "SELECT min(id) FROM message WHERE conversation_id=?1 AND id>?2 AND message_type='user'
           AND NOT coalesce(CASE WHEN json_valid(metadata_json) THEN
             json_extract(metadata_json,'$.origin')='butler_peer'
             AND json_extract(metadata_json,'$.leg') IN ('reply','receipt')
             AND json_extract(metadata_json,'$.exchange_id') IN (SELECT id FROM butler_peer_exchange WHERE parent=?3)
           END,0)",
        params![ex.target, trigger, ex.id],
        |r| r.get(0),
    )?;
    let last = conn.query_row(
        "SELECT message_type,content,finish_time IS NOT NULL,metadata_json,tool_calls_json,id
           FROM message WHERE conversation_id=?1 AND id>?2 AND (?3 IS NULL OR id<?3)
            AND message_type IN ('assistant','response','error','tool_result') ORDER BY id DESC LIMIT 1",
        params![ex.target,trigger,boundary],
        |r| Ok(ReplyCandidate { id:r.get(5)?, role:r.get(0)?, body:r.get(1)?, finished:r.get(2)?, metadata:r.get(3)?, tool_calls:r.get(4)? })
    ).optional()?;
    if let Some(ReplyCandidate { id: reply_id, role: kind, body, finished, metadata: meta, tool_calls: calls }) =
        last
    {
        if super::butler_goal::assert_message_current(conn,ex.target,Some(reply_id)).is_err() {
            fail(conn,ex.id,"接收方目标已变更，旧回复仅保留为记录，不作为本次协作结果")?;
            return Ok(true);
        }
        let meta = meta
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .unwrap_or_default();
        if kind == "error"
            || matches!(meta["end_reason"].as_str(), Some("interrupted" | "cancelled"))
        {
            fail(conn, ex.id, &format!("接收方回合终止：{kind} {body}"))?;
            return Ok(true);
        }
        if super::butler_execution::is_completed_content(&kind,&body,finished,calls.as_deref())
        {
            let tx=conn.unchecked_transaction()?;
            let changed=tx.execute("UPDATE butler_peer_exchange SET state='returning',reply=?1 WHERE id=?2 AND state='waiting'",
                params![body,ex.id])?;
            if changed>0 { super::butler_timeline::record(&tx,ex.id,ex.target,"reply",true,reply_id)?; }
            tx.commit()?;
            return Ok(changed>0);
        }
    }
    if boundary.is_some() || recovering {
        fail(conn, ex.id, "接收方回合已中断，没有可确认的最终答复")?;
        return Ok(true);
    }
    Ok(false)
}

/// 停止某会话（如被取消的独立任务）发起的全部进行中协作，连同其下级分工。返回涉及的协作 ID。
pub fn stop_owned(conn: &Connection, conversation: i64) -> rusqlite::Result<Vec<i64>> {
    super::butler_work::stop_task_followups(conn, conversation)?;
    let owned: Vec<i64> = conn
        .prepare(&format!("SELECT id FROM butler_peer_exchange WHERE owner=?1 AND state IN {ACTIVE} ORDER BY id"))?
        .query_map([conversation], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut stopped = Vec::new();
    for id in owned {
        for id in stop_subtree(conn, id)? {
            if !stopped.contains(&id) {
                stopped.push(id);
            }
        }
    }
    // 被整项停止的一方就是发起方本身，不再给它回执；下级成员仍各收到一次停止回执。
    conn.execute(
        "UPDATE butler_peer_exchange SET receipt_state=NULL
         WHERE owner=?1 AND state='stopped' AND receipt_state='pending'",
        [conversation],
    )?;
    Ok(stopped)
}

fn stop_rows(conn: &Connection, filter: &str, key: i64) -> rusqlite::Result<usize> {
    // 被停止的进行中协作也给发起方一次回执；已送达或已失败的保持原状。
    conn.execute(
        &format!("UPDATE butler_peer_exchange SET stop_requested=1,
            receipt_state=CASE WHEN state IN ('delivered','failed','stopped') THEN receipt_state ELSE coalesce(receipt_state,'pending') END,
            state=CASE WHEN state IN ('delivered','failed') THEN state ELSE 'stopped' END
         WHERE {filter}"),
        [key],
    )
}

/// 某会话发起、仍在进行的协作。
pub fn active_in_owner(conn: &Connection, owner: i64) -> rusqlite::Result<Vec<Exchange>> {
    conn.prepare(&format!("{SELECT} WHERE owner=?1 AND state IN {ACTIVE} ORDER BY id"))?
        .query_map([owner], row)?
        .collect()
}

/// 某会话正在受理（作为接收方）、仍在进行的协作。
pub fn active_in_target(conn: &Connection, target: i64) -> rusqlite::Result<Vec<Exchange>> {
    conn.prepare(&format!("{SELECT} WHERE target=?1 AND state IN {ACTIVE} ORDER BY id"))?
        .query_map([target], row)?
        .collect()
}

/// 进行中的协作（停止前快照），用于停止后判断哪些执行需要实际打断。
pub fn active_in_chain(conn: &Connection, chain: i64) -> rusqlite::Result<Vec<Exchange>> {
    conn.prepare(&format!("{SELECT} WHERE coalesce(chain,root)=?1 AND state IN {ACTIVE} ORDER BY id"))?
        .query_map([chain], row)?
        .collect()
}

/// 停止整项工作（整条协作链）。
pub fn stop_chain(conn: &Connection, chain: i64) -> rusqlite::Result<usize> {
    super::butler_work::stop_chain_followups(conn, chain)?;
    stop_rows(conn, "coalesce(chain,root)=?1", chain)
}

/// 记录停止时实际发生了什么（已打断、无法确认、未打断的原因），只写给本次停止的协作。
pub fn set_stop_note(conn: &Connection, ids: &[i64], note: &str) -> rusqlite::Result<()> {
    for id in ids {
        conn.execute(
            "UPDATE butler_peer_exchange SET stop_note=?2 WHERE id=?1 AND state='stopped' AND stop_note IS NULL",
            params![id, note],
        )?;
    }
    Ok(())
}

/// 工作链上全部协作 ID。
pub fn chain_ids(conn: &Connection, chain: i64) -> rusqlite::Result<Vec<i64>> {
    conn.prepare("SELECT id FROM butler_peer_exchange WHERE coalesce(chain,root)=?1 ORDER BY id")?
        .query_map([chain], |r| r.get(0))?
        .collect()
}

/// 参与方停止某一轮所在的整项工作：整条协作链（含下级、续接的后续轮次）一起停止。
pub fn stop(conn: &Connection, root: i64, conversation: i64) -> rusqlite::Result<usize> {
    let Some(chain) = conn
        .query_row(
            "SELECT coalesce(chain,root) FROM butler_peer_exchange WHERE root=?1
               AND EXISTS(SELECT 1 FROM butler_peer_exchange WHERE root=?1 AND (owner=?2 OR target=?2)) LIMIT 1",
            params![root, conversation],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(0);
    };
    stop_chain(conn, chain)
}

use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
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
    pub report_to_user: bool,
}
const SELECT: &str = "SELECT id,root,owner,target,sender,recipient,body,state,input_id,reply,error,round_no,max_rounds,stop_requested,report_to_user FROM butler_peer_exchange";
const SUMMARY: &str = "SELECT id,root,owner,target,sender,recipient,'',state,input_id,NULL,error,round_no,max_rounds,stop_requested,report_to_user FROM butler_peer_exchange";
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
    })
}
pub fn list(
    conn: &Connection,
    conversation: Option<i64>,
    details: bool,
) -> rusqlite::Result<Vec<Exchange>> {
    let filter = if conversation.is_some() {
        "WHERE owner=?1 OR target=?1 ORDER BY id DESC LIMIT 50"
    } else {
        "WHERE ?1 IS NULL AND state IN ('queued','waiting','returning') ORDER BY id"
    };
    let projection = if details { SELECT } else { SUMMARY };
    let mut stmt = conn.prepare(&format!("{projection} {filter}"))?;
    let rows = stmt.query_map([conversation], row)?;
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
    pub report_to_user: bool,
}

pub fn enqueue(conn: &mut Connection, req: NewRequest<'_>) -> anyhow::Result<Exchange> {
    anyhow::ensure!(req.owner != req.target, "不能向同一个联系人派发协作请求");
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (turn, meta): (i64, Option<String>) = tx.query_row(
        "SELECT id,metadata_json FROM message WHERE conversation_id=?1 AND message_type='user'
           AND (?2 IS NULL OR id<=?2) ORDER BY id DESC LIMIT 1",
        params![req.owner, req.tool_message_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let meta =
        meta.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).unwrap_or_default();
    let root = if meta["origin"] == "butler_peer" {
        anyhow::ensure!(
            meta["leg"] == "reply",
            "请直接结束本轮并给出意见或问题，回复会自动回到发起助手"
        );
        meta["root"].as_i64().ok_or_else(|| anyhow::anyhow!("协作消息缺少 root"))?
    } else {
        turn
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
                && old.report_to_user == req.report_to_user,
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
    let (count, limit, stopped): (i64, Option<i64>, i64) = tx.query_row(
        "SELECT count(*),min(max_rounds),count(CASE WHEN stop_requested=1 THEN 1 END)
         FROM butler_peer_exchange WHERE root=?1",
        [root],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let limit = limit.unwrap_or(req.limit);
    anyhow::ensure!(stopped == 0, "该协作已停止，请向用户汇报现有结果");
    anyhow::ensure!(count < limit, "协作已达到 {limit} 轮，请汇报已有结果和分歧");
    let busy: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM butler_peer_exchange WHERE root=?1 AND target=?2 AND state IN
        ('queued','sending','waiting','returning','reply_sending'))",
        params![root, req.target],
        |r| r.get(0),
    )?;
    anyhow::ensure!(!busy, "与该助手的上一轮仍在进行，收到回复后可继续发送");
    tx.execute("INSERT INTO butler_peer_exchange(root,owner,target,sender,recipient,request_key,body,state,round_no,max_rounds,report_to_user)
        VALUES (?1,?2,?3,?4,?5,?6,?7,'queued',?8,?9,?10)",params![root,req.owner,req.target,req.sender,req.recipient,req.key,req.body,count+1,limit,req.report_to_user])?;
    let ex = tx.query_row(&format!("{SELECT} WHERE id=?1"), [tx.last_insert_rowid()], row)?;
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
    Ok(conn.execute(
        "UPDATE butler_peer_exchange SET state=?1 WHERE id=?2 AND state=?3 AND stop_requested=0 AND NOT EXISTS (
        SELECT 1 FROM butler_peer_exchange WHERE id!=?2 AND
        ((target=?4 AND state IN ('sending','waiting')) OR (owner=?4 AND state='reply_sending')))",
        params![next, ex.id, ex.state, destination],
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
    anyhow::ensure!(matches!(leg, "request" | "reply"), "未知投递方向 {leg}");
    let request = leg == "request";
    let (expected, dest, speaker) = if request {
        ("sending", ex.target, ex.sender)
    } else {
        ("reply_sending", ex.owner, ex.recipient)
    };
    anyhow::ensure!(
        dest == conversation && ex.state == expected,
        "exchange={id} {leg} 投递已停止或归属不符"
    );
    let meta = serde_json::json!({"origin":"butler_peer","exchange_id":id,"root":ex.root,"leg":leg,"sender_bot":speaker});
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

pub fn fail(conn: &Connection, id: i64, detail: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE butler_peer_exchange SET state='failed',error=?1 WHERE id=?2 AND state!='stopped'",
        params![detail, id],
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
    let boundary: Option<i64> = conn.query_row(
        "SELECT min(id) FROM message WHERE conversation_id=?1 AND id>?2 AND message_type='user'",
        params![ex.target, trigger],
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
        let meta = meta
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .unwrap_or_default();
        if kind == "error"
            || matches!(meta["end_reason"].as_str(), Some("interrupted" | "cancelled"))
        {
            fail(conn, ex.id, &format!("接收方回合终止：{kind} {body}"))?;
            return Ok(true);
        }
        if finished
            && kind != "tool_result"
            && !body.trim().is_empty()
            && !body.contains("<!-- MCP_TOOL_CALL")
            && !body.contains("<mcp_tool_call>")
            && calls.as_deref().is_none_or(|v| matches!(v.trim(), "" | "[]" | "null"))
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

pub fn stop(conn: &Connection, root: i64, conversation: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE butler_peer_exchange SET stop_requested=1, state=CASE WHEN state IN ('delivered','failed') THEN state ELSE 'stopped' END WHERE root=?1 AND EXISTS
        (SELECT 1 FROM butler_peer_exchange WHERE root=?1 AND (owner=?2 OR target=?2))",
        params![root, conversation],
    )
}

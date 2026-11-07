use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;

// Presentation events are separate from model input. Bodies stay in their original exchange.
pub fn schema(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='butler_peer_post')",
        [], |r| r.get(0),
    )?;
    if exists { return Ok(()); }
    tx.execute_batch("CREATE TABLE butler_peer_post (
        id INTEGER PRIMARY KEY, exchange_id INTEGER NOT NULL REFERENCES butler_peer_exchange(id) ON DELETE CASCADE,
        conversation_id INTEGER NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
        leg TEXT NOT NULL, outgoing INTEGER NOT NULL, anchor INTEGER NOT NULL, occurred_at TEXT NOT NULL,
        UNIQUE(exchange_id,leg,outgoing));
        CREATE INDEX idx_butler_peer_post_conversation ON butler_peer_post(conversation_id,id);")?;
    let has_exchanges: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_peer_exchange)", [], |row| row.get(0))?;
    if !has_exchanges { return tx.commit(); }
    // Existing deliveries already carry their exact exchange, leg and message ID.
    tx.execute_batch("INSERT OR IGNORE INTO butler_peer_post(exchange_id,conversation_id,leg,outgoing,anchor,occurred_at)
        SELECT e.id,m.conversation_id,json_extract(m.metadata_json,'$.leg'),0,m.id,m.created_time
        FROM message m JOIN butler_peer_exchange e ON e.id=CASE WHEN json_valid(m.metadata_json)
            THEN json_extract(m.metadata_json,'$.exchange_id') END
        WHERE json_valid(m.metadata_json) AND json_extract(m.metadata_json,'$.origin')='butler_peer';
        INSERT OR IGNORE INTO butler_peer_post(exchange_id,conversation_id,leg,outgoing,anchor,occurred_at)
        SELECT e.id,e.owner,'request',1,coalesce((SELECT max(m.id) FROM message m WHERE m.conversation_id=e.owner
            AND m.id<e.input_id),e.root),coalesce(p.occurred_at,r.created_time)
        FROM butler_peer_exchange e JOIN message r ON r.id=e.root
        LEFT JOIN butler_peer_post p ON p.exchange_id=e.id AND p.leg='request' AND p.outgoing=0;
        INSERT OR IGNORE INTO butler_peer_post(exchange_id,conversation_id,leg,outgoing,anchor,occurred_at)
        SELECT e.id,e.target,'reply',1,m.id,m.created_time FROM butler_peer_exchange e JOIN message m ON m.id=(
            SELECT min(a.id) FROM message a WHERE a.conversation_id=e.target AND a.id>e.input_id
            AND a.message_type IN ('response','assistant') AND a.content=e.reply) WHERE e.reply IS NOT NULL;")?;
    tx.commit()
}

pub fn record(conn: &Connection, exchange:i64, conversation:i64, leg:&str, outgoing:bool, anchor:i64) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO butler_peer_post(exchange_id,conversation_id,leg,outgoing,anchor,occurred_at)
        VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(exchange_id,leg,outgoing) DO NOTHING",
        params![exchange,conversation,leg,outgoing,anchor,Utc::now()])?;
    Ok(())
}

#[derive(Serialize)]
pub struct Post {
    pub(super) id:i64, pub(super) anchor:i64, pub(super) occurred_at:DateTime<Utc>, pub(super) outgoing:bool, pub(super) peer:String,
    pub(super) peer_conversation:i64, pub(super) peer_assistant:Option<i64>, pub(super) text:String, pub(super) status:String, pub(super) error:Option<String>,
    pub(super) task_id:Option<i64>, pub(super) task_reply:bool, pub(super) root:Option<i64>,
    /// 执行助手直接交付给用户：以执行助手的名义完整展示答复（发起方会话与执行助手自己的会话都可见）。
    pub(super) direct:bool,
    /// 这项协作面向用户（受托助手直接答复用户）；false 为助手之间的内部协作。
    pub(super) to_user:bool,
    /// 多层协作中的上级协作。
    pub(super) parent:Option<i64>,
    /// 直接交付行的发言者（与 peer 不同时给出，例如执行助手自己会话中的任务交付）。
    pub(super) speaker:Option<String>,
    pub(super) speaker_assistant:Option<i64>,
    /// 协作记录身份（引用回复用）；任务事件为空。
    pub(super) exchange_id:Option<i64>,
}
pub fn list(conn:&Connection,conversation:i64)->rusqlite::Result<Vec<Post>> {
    let mut stmt=conn.prepare("SELECT p.id,p.anchor,p.occurred_at,p.outgoing,
        CASE WHEN p.conversation_id=e.owner THEN e.recipient ELSE e.sender END,c.id,c.assistant_id,
        CASE p.leg WHEN 'request' THEN e.body ELSE e.reply END,e.state,e.error,e.root,
        p.leg='reply' AND p.conversation_id=e.owner AND e.report_to_user=1,e.report_to_user,e.parent,e.id
        FROM butler_peer_post p JOIN butler_peer_exchange e ON e.id=p.exchange_id JOIN conversation c ON c.id=
        CASE WHEN p.conversation_id=e.owner THEN e.target ELSE e.owner END
        WHERE p.conversation_id=?1 ORDER BY p.id")?;
    let rows=stmt.query_map([conversation],|r| Ok(Post{id:r.get(0)?,anchor:r.get(1)?,occurred_at:r.get(2)?,
        outgoing:r.get(3)?,peer:r.get(4)?,peer_conversation:r.get(5)?,peer_assistant:r.get(6)?,
        text:r.get(7)?,status:r.get(8)?,error:r.get(9)?,task_id:None,task_reply:false,root:r.get(10)?,direct:r.get(11)?,
        to_user:r.get(12)?,parent:r.get(13)?,speaker:None,speaker_assistant:None,exchange_id:r.get(14)?}))?;
    rows.collect()
}

// Task conversations have their own runtime. Project their persisted handoff and final result
// into both contacts without adding messages to either model's working context.
pub fn task_posts(
    conn: &Connection,
    conversation: i64,
    names: &HashMap<i64, String>,
) -> rusqlite::Result<Vec<Post>> {
    let mut stmt = conn.prepare(
        "SELECT d.task_conversation_id,d.butler_conversation_id,d.executor_assistant_id,
                parent.assistant_id,d.goal,d.created_time,t.butler_task_status,
                coalesce(r.created_time,t.butler_task_finalized_at),
                coalesce(nullif(final.content,''),CASE WHEN json_valid(r.structured_output_json)
                    THEN json_extract(r.structured_output_json,'$.content') END,r.summary,t.butler_task_summary),
                coalesce(r.handoff_message_id,0),
                CASE WHEN current.id=d.butler_conversation_id THEN 1 ELSE 0 END,
                coalesce((SELECT max(id) FROM message WHERE conversation_id=current.id
                    AND julianday(created_time)<=julianday(d.created_time)),0),
                coalesce(d.result_handling_mode,'')='direct_report',
                CASE WHEN current.id=d.butler_conversation_id THEN current.created_time END
         FROM butler_task_definition d
         JOIN conversation t ON t.id=d.task_conversation_id
         JOIN conversation parent ON parent.id=d.butler_conversation_id
         JOIN conversation current ON current.id=?1
         LEFT JOIN butler_task_result r ON r.task_conversation_id=t.id
         LEFT JOIN message final ON final.id=r.final_message_id
         WHERE current.id=d.butler_conversation_id OR
             (current.conversation_kind='butler_contact' AND current.assistant_id=d.executor_assistant_id)
         ORDER BY d.created_time,d.id"
    )?;
    let rows = stmt.query_map([conversation], |row| {
        let task: i64 = row.get(0)?;
        let owner: i64 = row.get(1)?;
        let executor: i64 = row.get(2)?;
        let chief: Option<i64> = row.get(3)?;
        let owner_view: bool = row.get(10)?;
        let direct_report: bool = row.get(12)?;
        // 重开主会话会把旧任务改挂到新会话以便继续回送结果；新会话只展示它创建之后发生的委派与回复。
        let since: Option<DateTime<Utc>> = row.get(13)?;
        let after_since = |at: &DateTime<Utc>| since.map_or(true, |since| *at >= since);
        let peer_assistant = if owner_view { Some(executor) } else { chief };
        let peer = peer_assistant.and_then(|id| names.get(&id).cloned())
            .unwrap_or_else(|| format!("助手 #{}", peer_assistant.unwrap_or(0)));
        let state: Option<String> = row.get(6)?;
        let status = match state.as_deref() {
            Some("succeeded") => "delivered",
            Some("failed") => "failed",
            Some("cancelled" | "interrupted") => "stopped",
            Some("accepted") => "queued",
            _ => "waiting",
        };
        let finished: Option<DateTime<Utc>> = row.get(7)?;
        let body: Option<String> = row.get(8)?;
        let error = if status == "failed" {
            Some(body.clone().unwrap_or_else(|| format!("任务 #{task} 失败，任务结果未记录底层原因，请查看执行记录")))
        } else { None };
        let request = Post {
            id: (1_i64 << 40) + task * 2,
            anchor: row.get(11)?,
            occurred_at: row.get(5)?,
            outgoing: owner_view,
            peer: peer.clone(),
            peer_conversation: if owner_view { task } else { owner },
            peer_assistant,
            text: row.get(4)?,
            status: status.into(),
            error: if finished.is_none() { error.clone() } else { None },
            task_id: Some(task),
            root: None,
            task_reply: false,
            direct: false,
            to_user: direct_report,
            parent: None,
            speaker: None,
            speaker_assistant: None,
            exchange_id: None,
        };
        let mut posts = Vec::new();
        if after_since(&request.occurred_at) { posts.push(request); }
        if let Some(at) = finished.filter(after_since) {
            posts.push(Post {
                id: (1_i64 << 40) + task * 2 + 1,
                anchor: if owner_view { row.get(9)? } else { 0 },
                occurred_at: at,
                outgoing: !owner_view,
                peer,
                peer_conversation: if owner_view { task } else { owner },
                peer_assistant,
                text: if error.is_some() { String::new() } else {
                    body.unwrap_or_else(|| format!("任务 #{task} 已结束，未返回正文"))
                },
                status: status.into(),
                error,
                task_id: Some(task),
                root: None,
                task_reply: true,
                // 谁接手谁交付：直接交付的结果在执行助手自己的会话里同样以其身份完整展示。
                direct: direct_report && status == "delivered",
                to_user: direct_report,
                parent: None,
                speaker: (!owner_view).then(|| names.get(&executor).cloned()
                    .unwrap_or_else(|| format!("助手 #{executor}"))),
                speaker_assistant: (!owner_view).then_some(executor),
                exchange_id: None,
            });
        }
        Ok(posts)
    })?;
    let mut result = Vec::new();
    for posts in rows { result.extend(posts?); }
    Ok(result)
}

#[cfg(test)]
#[path = "butler_timeline_tests.rs"]
mod tests;

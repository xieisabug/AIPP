//! 跟进事项与用户原话的持久化关联、本地简报重建、以及到期/准备期事项的唤醒查询。
use super::{
    delta::{self, Source},
    Config,
};
use anyhow::Result;
use chrono::Local;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};

pub(super) fn link(db: &Connection, item: i64, row: &Source, quote: &str) -> Result<()> {
    let group = row.kind == "butler_group";
    db.execute(
        "INSERT INTO butler_proactive_source(uuid,item_id,conversation_id,message_id,group_post_id,quote)
        VALUES(?1,?2,?3,CASE WHEN NOT ?4 THEN ?5 END,
        CASE WHEN ?4 THEN (SELECT id FROM butler_chat_group_message WHERE group_id=?3 AND seq=?5) END,?6)
        ON CONFLICT DO NOTHING",
        params![uuid::Uuid::new_v4().to_string(), item, row.conversation, group, row.position, quote],
    )?;
    Ok(())
}

pub(super) fn sources(db: &Connection, cfg: &Config, item: i64) -> Result<Vec<Source>> {
    // 兼容早年只写 evidence 没写 source 的历史数据
    db.execute(
        "INSERT OR IGNORE INTO butler_proactive_source(uuid,item_id,conversation_id,message_id,group_post_id,quote)
        SELECT lower(hex(randomblob(16))),i.id,i.source_conversation_id,i.source_message_id,i.source_group_post_id,min(e.quote)
        FROM butler_proactive_item i JOIN butler_proactive_evidence e ON e.conversation_id=i.source_conversation_id
        AND e.source_position=i.source_position AND e.kind=i.kind
        WHERE i.id=?1 AND NOT EXISTS(SELECT 1 FROM butler_proactive_source WHERE item_id=i.id)
        AND (EXISTS(SELECT 1 FROM message WHERE id=i.source_message_id AND conversation_id=i.source_conversation_id)
             OR EXISTS(SELECT 1 FROM butler_chat_group_message WHERE id=i.source_group_post_id AND group_id=i.source_conversation_id))
        AND NOT EXISTS(SELECT 1 FROM butler_proactive_item other WHERE other.id!=i.id AND other.kind=i.kind
            AND other.source_conversation_id=i.source_conversation_id AND other.source_position=i.source_position)
        GROUP BY i.id HAVING count(*)=1",
        [item],
    )?;
    let mut stmt = db.prepare(
        "SELECT s.conversation_id,coalesce(s.message_id,g.seq),s.quote
        FROM butler_proactive_source s LEFT JOIN butler_chat_group_message g ON g.id=s.group_post_id
        WHERE s.item_id=?1 ORDER BY s.id DESC LIMIT 32",
    )?;
    let mut records = stmt.query([item])?;
    let mut out = Vec::new();
    while let Some(r) = records.next()? {
        if let Some(s) = delta::quote(db, cfg, r.get(0)?, r.get(1)?, &r.get::<_, String>(2)?)? {
            out.push(s);
        }
    }
    Ok(out)
}

/// 跨设备同步过来的事项带有最终简报正文，卡片需要本机不可变消息来展开预览。
pub(super) fn local_brief(db: &Connection, item: i64) -> Result<Option<i64>> {
    let (text, existing): (String, Option<i64>) = db.query_row(
        "SELECT i.detail,m.id FROM butler_proactive_item i
        LEFT JOIN message m ON m.id=i.prep_message_id AND m.content=i.detail AND json_extract(m.metadata_json,'$.origin')='proactive_brief'
        WHERE i.id=?1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if existing.is_some() || text.is_empty() {
        return Ok(existing);
    }
    db.execute(
        "UPDATE butler_proactive_delivery_queue AS q SET brief_message_id=(
        SELECT json_extract(card.value,'$.brief') FROM message m,json_each(m.metadata_json,'$.cards') card
        WHERE m.id=q.message_id AND m.conversation_id=q.conversation_id AND json_extract(card.value,'$.delivery')=q.id)
        WHERE item_id=?1 AND brief_message_id IS NULL AND message_id IS NOT NULL",
        [item],
    )?;
    db.execute(
        "INSERT INTO conversation(name,conversation_kind,is_hidden_from_normal_chat_list)
        SELECT '管家预研简报','butler_proactive_brief',1 WHERE NOT EXISTS(SELECT 1 FROM conversation WHERE conversation_kind='butler_proactive_brief')",
        [],
    )?;
    db.execute(
        "INSERT INTO message(conversation_id,message_type,content,metadata_json,token_count)
        SELECT min(id),'response',?1,json_object('origin','proactive_brief','item_id',?2),0 FROM conversation
        WHERE conversation_kind='butler_proactive_brief'",
        params![text, item],
    )?;
    let id = db.last_insert_rowid();
    db.execute(
        "UPDATE butler_proactive_item SET prep_message_id=?2 WHERE id=?1",
        params![item, id],
    )?;
    Ok(Some(id))
}

const SCOPE: &str = "((?1 AND (c.conversation_kind='butler_contact' OR (c.conversation_kind='butler_main'
    AND c.id IN (SELECT butler_conversation_id FROM butler_main_state)))) OR (?2 AND c.conversation_kind='normal'
    AND c.is_hidden_from_normal_chat_list=0) OR (?3 AND c.conversation_kind='butler_group'))";

#[derive(Serialize)]
pub(super) struct Topic {
    pub id: i64,
    pub revision: i64,
    pub title: String,
    pub date: Option<String>,
    pub kind: String,
    pub conversation: i64,
    pub tracking_summary: String,
    pub calendar: String,
    pub yearly: bool,
    pub lead_days: u32,
}

pub(super) fn active(db: &Connection, cfg: &Config) -> Result<Vec<Topic>> {
    let mut stmt = db.prepare(&format!(
        "SELECT i.id,i.revision,i.title,i.event_date,i.kind,c.id,
        CASE WHEN NOT EXISTS(SELECT 1 FROM butler_proactive_source s WHERE s.item_id=i.id
            AND s.conversation_id NOT IN (SELECT c.id FROM conversation c WHERE {SCOPE}))
            THEN i.tracking_summary ELSE '' END,i.calendar_type,i.yearly,i.lead_days FROM butler_proactive_item i
        JOIN conversation c ON c.id=i.source_conversation_id WHERE i.status='active' AND i.kind!='proposal' AND {SCOPE}
        ORDER BY i.updated_at DESC,i.id LIMIT 60"
    ))?;
    let items = stmt
        .query_map(params![cfg.private_chats, cfg.normal_chats, cfg.groups], |r| {
            Ok(Topic {
                id: r.get(0)?,
                revision: r.get(1)?,
                title: r.get(2)?,
                date: r.get(3)?,
                kind: r.get(4)?,
                conversation: r.get(5)?,
                tracking_summary: r.get(6)?,
                calendar: r.get(7)?,
                yearly: r.get(8)?,
                lead_days: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

pub(super) fn reset_schedule(db: &Connection, item: i64) -> Result<()> {
    db.execute(
        "DELETE FROM butler_proactive_delivery_queue WHERE item_id=?1
        AND delivered_at IS NULL AND status IN ('queued','quiet','cancelled')",
        [item],
    )?;
    db.execute(
        "UPDATE butler_proactive_delivery_queue SET status='cancelled' WHERE item_id=?1
        AND status='queued' AND delivered_at IS NOT NULL",
        [item],
    )?;
    Ok(())
}

fn wake_query() -> String {
    format!(
        "SELECT q.occurrence_key,i.id,i.revision,i.title,q.occurrence,q.stage,i.detail,i.tracking_summary
        FROM butler_proactive_delivery_queue q JOIN butler_proactive_item i ON i.id=q.item_id
        JOIN conversation c ON c.id=i.source_conversation_id
        WHERE q.status='queued' AND q.delivered_at IS NULL AND q.researched=0 AND i.status='active'
        AND (q.stage='prepare' OR (i.kind='prep_topic' AND (i.event_date IS NOT NULL OR i.tracking_summary!='')))
        AND q.occurrence>=?4 AND (q.stage!='prepare' OR q.occurrence>?4) AND {SCOPE}
        AND (?5 IS NULL OR i.id=?5)
        AND NOT EXISTS(SELECT 1 FROM butler_proactive_wake_retry b WHERE b.queue_id=q.id AND b.retry_at>unixepoch())
        ORDER BY q.occurrence,q.id LIMIT 6"
    )
}

pub(super) fn pending(db: &Connection, cfg: &Config) -> Result<bool> {
    Ok(db.query_row(
        &format!("SELECT EXISTS({})", wake_query()),
        params![
            cfg.private_chats,
            cfg.normal_chats,
            cfg.groups,
            Local::now().date_naive().to_string(), Option::<i64>::None
        ],
        |r| r.get(0),
    )?)
}

pub(super) fn review_before_delivery(db: &Connection, cfg: &Config) -> Result<bool> {
    let dated: bool = db.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM butler_proactive_delivery_queue q
        JOIN butler_proactive_item i ON i.id=q.item_id JOIN conversation c ON c.id=i.source_conversation_id
        WHERE q.status='queued' AND q.delivered_at IS NULL AND i.status='active' AND i.event_date IS NOT NULL
        AND q.deliver_after<=unixepoch() AND {SCOPE})"
        ),
        params![cfg.private_chats, cfg.normal_chats, cfg.groups],
        |r| r.get(0),
    )?;
    Ok(dated && delta::unread_digest(db, cfg)?)
}

/// 收集本次需要唤醒复核/预研的事项及其已关联原话，交给 Proactive Agent 自主处理。
pub(super) fn waking_jobs(db: &Connection, cfg: &Config, item: Option<i64>) -> Result<(Vec<(String, i64)>, Vec<Value>)> {
    let today = Local::now().date_naive().to_string();
    let mut stmt = db.prepare(&wake_query())?;
    let rows = stmt
        .query_map(params![cfg.private_chats, cfg.normal_chats, cfg.groups, today, item], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stages = Vec::with_capacity(rows.len());
    let mut items = Vec::with_capacity(rows.len());
    for (key, item_id, revision, title, occurrence, stage, detail, tracking) in rows {
        let linked = sources(db, cfg, item_id)?;
        stages.push((key, revision));
        items.push(json!({
            "item_id": item_id,
            "revision": revision,
            "title": title,
            "occurrence": occurrence,
            "stage": stage,
            "tracking_summary": tracking,
            "previous_brief": detail,
            "linked_sources": linked,
        }));
    }
    Ok((stages, items))
}

/// 本轮 Agent 跑完后，把已交付给 Agent 审查但 Agent 未触发 `proactive_notify` 的唤醒行标为已审查并静默，避免重复唤醒。
pub(super) fn finalize_woken(db: &Connection, stages: &[(String, i64)]) -> Result<()> {
    for (key, revision) in stages {
        db.execute(
            "UPDATE butler_proactive_delivery_queue SET researched=1,status='quiet'
             WHERE occurrence_key=?1 AND researched=0 AND status='queued' AND delivered_at IS NULL
             AND EXISTS(SELECT 1 FROM butler_proactive_item i WHERE i.id=item_id AND i.revision=?2 AND i.status='active')",
            params![key, revision],
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "topics_tests.rs"]
mod tests;

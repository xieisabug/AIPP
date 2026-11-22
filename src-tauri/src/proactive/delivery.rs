use super::{runner::event_date, Config};
use crate::db::{
    assistant_db::AssistantDatabase, conversation_db::ConversationDatabase, llm_db::LLMDatabase,
};
use anyhow::{ensure, Context, Result};
use chrono::{Duration, Local, NaiveDate, NaiveTime};
use rusqlite::{params, Connection, TransactionBehavior};
use tauri::{AppHandle, Emitter};
#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;

pub fn contact(app: &AppHandle, owner: i64) -> Result<(i64, String)> {
    let db = AssistantDatabase::new(app)?;
    let assistant = db.get_assistant(owner).with_context(|| format!("联系人 #{owner} 已不可用"))?;
    let models = db.get_assistant_model(owner)?;
    let m = models.first().context("联系人未配置模型")?;
    LLMDatabase::new(app)?
        .get_llm_model_detail(&m.provider_id, &m.model_code)
        .context("联系人模型已不可用")?;
    let id = ConversationDatabase::new(app)?
        .conversation_repo()?
        .open_butler_contact(owner, &assistant.name)?;
    Ok((id, assistant.name))
}

pub(super) fn schedule(conn: &Connection, now: chrono::DateTime<Local>, cfg: &Config) -> Result<()> {
    let today = now.date_naive();
    let mut stmt = conn.prepare(
        "SELECT id,event_date,yearly,calendar_type,lead_days,kind FROM butler_proactive_item
        WHERE status='active' AND next_check_at<=?1 ORDER BY next_check_at,id LIMIT 128",
    )?;
    let items = stmt
        .query_map([today.to_string()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (item, raw, yearly, calendar, lead, kind) in items {
        let date = raw
            .as_deref()
            .map(|s| event_date(s, yearly, calendar == "lunar", today))
            .transpose();
        let date = match date {
            Ok(Some(date)) => date,
            Ok(None) if !yearly => today,
            invalid => {
                let cause = invalid.err().map(|e| e.to_string()).unwrap_or_else(|| "年度事项缺少日期".into());
                super::runner::record_failure(conn, "schedule", &anyhow::anyhow!("事项 #{item}: {cause}"))?;
                conn.execute("UPDATE butler_proactive_item SET next_check_at=NULL WHERE id=?1",[item])?;
                continue;
            }
        };
        let lead_date = date - Duration::days(lead);
        let stage = if today < lead_date {
            None
        } else if date < today && kind == "milestone" {
            None
        } else if today < date {
            Some("prepare")
        } else {
            Some("due")
        };
        if let Some(stage) = stage {
            let mut after = now.timestamp();
            let time = now.format("%H:%M").to_string();
            if cfg.quiet(&time) {
                let tomorrow = cfg.quiet_start > cfg.quiet_end && time >= cfg.quiet_start;
                let release_day = if tomorrow { today + Duration::days(1) } else { today };
                after = release_day
                    .and_time(NaiveTime::parse_from_str(&cfg.release, "%H:%M")?)
                    .and_local_timezone(Local)
                    .earliest()
                    .context("免打扰释放时间无效")?
                    .timestamp();
            }
            let morning = cfg.quiet(&time)
                || (cfg.quiet_start != cfg.quiet_end
                    && time >= cfg.quiet_end
                    && time < cfg.release);
            conn.execute("INSERT OR IGNORE INTO butler_proactive_delivery_queue(item_id,occurrence,stage,deliver_after,occurrence_key,morning)
                SELECT id,?2,?3,?4,uuid || '/' || ?2 || '/' || ?3,?5 FROM butler_proactive_item WHERE id=?1",params![item,date.to_string(),stage,after,morning])?;
        }
        let next = if today < lead_date {
            Some(lead_date)
        } else if today < date {
            Some(date)
        } else if yearly
            && calendar == "lunar"
            && super::lunar::from_solar(date).is_some_and(|(year, _, _, _)| year == 2100)
        {
            // Deliver the last supported lunar year even though it has no successor in the table.
            None
        } else if yearly {
            let next = event_date(
                raw.as_deref().context("年度事项缺少日期")?,
                true,
                calendar == "lunar",
                today + Duration::days(1),
            )?;
            Some(next - Duration::days(lead))
        } else {
            None
        };
        conn.execute(
            "UPDATE butler_proactive_item SET next_check_at=?2 WHERE id=?1",
            params![item, next.map(|d| d.to_string())],
        )?;
    }
    Ok(())
}

#[derive(Clone)]
struct Reminder {
    queue: i64,
    item: i64,
    owner: Option<i64>,
    title: String,
    kind: String,
    revision: i64,
    brief: Option<i64>,
    source: i64,
    position: i64,
    stage: String,
    date: String,
    uuid: String,
    lunar_date: Option<String>,
    high_priority: bool,
    morning: bool,
    dated: bool,
    summary: String,
}

impl Reminder {
    fn blessing(&self, today: NaiveDate) -> bool {
        self.kind == "milestone" && self.stage == "due" && self.date == today.to_string()
    }
    fn summary(&self) -> String {
        if !self.summary.is_empty() {return self.summary.clone();}
        if self.kind == "milestone" {
            let mut note = String::new();
            if let Some(raw) = &self.lunar_date {
                if let Some((_, _, leap, day)) =
                    self.date.parse().ok().and_then(super::lunar::from_solar)
                {
                    if raw.contains('L') && !leap {
                        note.push_str("，今年按平月");
                    }
                    if raw.ends_with("-30") && day == 29 {
                        note.push_str("，小月按廿九");
                    }
                }
            }
            format!("{}（{}{}）", self.title, self.date, note)
        } else {
            self.title.clone()
        }
    }
    fn card(&self, fallback: bool) -> serde_json::Value {
        serde_json::json!({"origin":"butler_proactive","role":"result","item":self.item,"uuid":self.uuid,"delivery":self.queue,
            "kind":self.kind,"revision":self.revision,"brief":self.brief,"source":self.source,"position":self.position,
            "stage":self.stage,"occurrence":self.date,"fallback":fallback})
    }
}

const ACTIVE_RESULT:&str="m.message_type='response' AND json_valid(m.metadata_json) AND json_extract(m.metadata_json,'$.origin')='butler_proactive'";
fn within_budget(conn: &Connection, today: NaiveDate, limit: u32, blessing: bool) -> Result<bool> {
    let (total, used): (i64, bool) = conn.query_row(
        &format!(
            "SELECT count(*),coalesce(max(json_extract(m.metadata_json,'$.blessing')),0)
        FROM message m WHERE {ACTIVE_RESULT} AND date(m.created_time,'localtime')=?1"
        ),
        [today.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((blessing && !used) || total - i64::from(used) < i64::from(limit))
}

pub(super) fn backoff(conn: &Connection) -> Result<bool> {
    let sql=format!("WITH recent AS (SELECT m.id AS message_id,unixepoch(m.created_time) AS delivered_at FROM message m
        WHERE {ACTIVE_RESULT} ORDER BY m.id DESC LIMIT 2)
        SELECT count(*)=2 AND NOT EXISTS(SELECT 1 FROM recent JOIN butler_read_reply r USING(message_id))
        AND NOT EXISTS(SELECT 1 FROM message m JOIN conversation c ON c.id=m.conversation_id
        WHERE c.conversation_kind IN ('butler_contact','butler_main') AND m.message_type='user' AND m.id>(SELECT min(message_id) FROM recent)
        AND (m.metadata_json IS NULL OR (json_valid(m.metadata_json) AND json_extract(m.metadata_json,'$.origin') IS NULL)))
        AND NOT EXISTS(SELECT 1 FROM butler_chat_group_message WHERE sender_member IS NULL AND kind='message'
        AND unixepoch(created_at)>(SELECT min(delivered_at) FROM recent)) FROM recent");
    Ok(conn.query_row(&sql, [], |r| r.get(0))?)
}

fn due(conn: &Connection, research: bool, review_pending: bool) -> Result<Vec<Reminder>> {
    let mut stmt=conn.prepare("SELECT q.id,i.id,i.owner_assistant_id,i.title,i.kind,i.revision,i.prep_message_id,i.source_conversation_id,
        coalesce(i.source_message_id,(SELECT seq FROM butler_chat_group_message WHERE id=i.source_group_post_id),i.source_position),
        q.stage,q.occurrence,i.uuid,CASE WHEN i.calendar_type='lunar' THEN i.event_date END,i.high_priority,q.morning,q.summary,(i.event_date IS NOT NULL AND q.delivered_at IS NULL) FROM butler_proactive_delivery_queue q JOIN butler_proactive_item i ON i.id=q.item_id
        WHERE q.status='queued' AND q.deliver_after<=unixepoch() AND i.status='active'
        AND (NOT ?2 OR i.event_date IS NULL OR q.delivered_at IS NOT NULL)
        AND (NOT ?1 OR q.researched=1 OR q.delivered_at IS NOT NULL OR
             (q.stage!='prepare' AND (i.kind!='prep_topic' OR (i.event_date IS NULL AND i.tracking_summary=''))))
        ORDER BY q.deliver_after,q.id LIMIT 64")?;
    let rows = stmt.query_map([research,review_pending], |r| {
        Ok(Reminder {
            queue: r.get(0)?,
            item: r.get(1)?,
            owner: r.get(2)?,
            title: r.get(3)?,
            kind: r.get(4)?,
            revision: r.get(5)?,
            brief: r.get(6)?,
            source: r.get(7)?,
            position: r.get(8)?,
            stage: r.get(9)?,
            date: r.get(10)?,
            uuid: r.get(11)?,
            lunar_date: r.get(12)?,
            high_priority: r.get(13)?,
            morning: r.get(14)?,
            summary:r.get(15)?,
            dated:r.get(16)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn publish(
    conn: &mut Connection,
    target: i64,
    rows: &[Reminder],
    text: &str,
    reason: Option<&str>,
    cfg: &Config,
    today: NaiveDate,
) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM conversation WHERE id=?1 AND (conversation_kind='butler_contact'
        OR (conversation_kind='butler_main' AND id IN (SELECT butler_conversation_id FROM butler_main_state))))",[target],|r|r.get(0))?;
    ensure!(current, "目标管家会话 #{target} 已归档或删除");
    let stamp = chrono::Utc::now();
    ensure!(ready_to_publish(&tx,rows,cfg,today)?, "投递时间或跟进上下文已变化，保留排队提醒");
    let mut cards = Vec::new();
    for row in rows {
        let mut card = row.card(reason.is_some());
        card["brief"] = serde_json::Value::Null;
        if row.kind == "prep_topic" || row.brief.is_some() || row.stage == "prepare" {
            if let Some(brief) = super::topics::local_brief(&tx, row.item)? {
                card["brief"] = brief.into();
                tx.execute("UPDATE butler_proactive_delivery_queue SET brief_message_id=?2 WHERE id=?1",params![row.queue,brief])?;
            }
        }
        cards.push(card);
    }
    let mut meta = cards[0].clone();
    meta["cards"] = cards.into();
    meta["blessing"] = serde_json::json!(rows.iter().any(|r| r.blessing(today)));
    tx.execute("INSERT INTO message(conversation_id,message_type,content,metadata_json,created_time,finish_time,token_count)
        VALUES(?1,'user','[管家主动提醒]',json_object('origin','butler_proactive','role','trigger'),?2,?2,0)",params![target,stamp])?;
    let anchor = tx.last_insert_rowid();
    tx.execute("INSERT INTO message(conversation_id,parent_id,message_type,content,metadata_json,created_time,start_time,finish_time,token_count)
        VALUES(?1,?2,'response',?3,?4,?5,?5,?5,0)",params![target,anchor,text,meta.to_string(),stamp])?;
    let message = tx.last_insert_rowid();
    let external_key = uuid::Uuid::new_v4().to_string();
    for row in rows {
        let changed=tx.execute("UPDATE butler_proactive_delivery_queue SET status='delivered',delivered_at=unixepoch(),conversation_id=?2,message_id=?3,
            fallback_reason=?4 WHERE id=?1 AND status='queued' AND EXISTS(SELECT 1 FROM butler_proactive_item i WHERE i.id=item_id AND i.status='active' AND revision=?5)",
            params![row.queue,target,message,reason,row.revision])?;
        ensure!(changed == 1, "提醒 #{} 在投递前已变更", row.queue);
        if !cfg.feishu_open_id.is_empty()
            && (row.blessing(today)
                || (row.kind == "prep_topic" && (row.high_priority || row.morning))
                || (row.kind == "open_loop" && row.high_priority && row.stage == "due"))
        {
            tx.execute("UPDATE butler_proactive_delivery_queue SET external_key=?2,external_recipient=?3,external_status='queued',external_error=NULL WHERE id=?1",
                params![row.queue,external_key,cfg.feishu_open_id])?;
        }
    }
    tx.execute("UPDATE conversation SET updated_time=?2 WHERE id=?1", params![target, stamp])?;
    tx.execute("DELETE FROM conversation_summary WHERE conversation_id=?1", [target])?;
    tx.commit()?;
    Ok(message)
}

fn fallback_summary(conn: &Connection, rows: &[Reminder]) -> Result<String> {
    use rusqlite::OptionalExtension;
    let mut summaries = Vec::new();
    for row in rows {
        let source:Option<String>=conn.query_row("SELECT m.content FROM butler_proactive_item i JOIN message m
            ON m.id=i.source_message_id AND m.conversation_id=i.source_conversation_id WHERE i.id=?1",
            [row.item],|r|r.get(0)).optional()?;
        let label = match row.kind.as_str() {
            "milestone" => "纪念日",
            "prep_topic" => "预研简报",
            "proposal" => "待确认的建议",
            _ => "待跟进事项",
        };
        let title = row.title.trim();
        let chars = title.chars().collect::<Vec<_>>();
        let concise =
            source.as_deref().map(crate::utils::turn_context::visible).is_some_and(|original| {
                !original.is_empty()
                    && !title.contains(original)
                    && !original.contains(title)
                    && !chars
                        .windows(8)
                        .any(|part| original.contains(&part.iter().collect::<String>()))
            });
        let topic = if concise && chars.len() <= 40 && !title.contains(['\n', '\r']) {
            title
        } else {
            label
        };
        summaries.push(format!("{} · {topic}", row.date));
    }
    Ok(format!("原联系人暂时无法送达，我来转达。\n{}", summaries.join("\n")))
}

async fn contact_wording(
    app: &AppHandle,
    session: i64,
    owner: i64,
    name: &str,
    rows: &[Reminder],
) -> Result<String> {
    let db = AssistantDatabase::new(app)?;
    let member = db.get_assistant(owner)?;
    ensure!(member.assistant_type.unwrap_or(0) == 0, "该联系人当前通道不支持隔离润色");
    let models = db.get_assistant_model(owner)?;
    let model = models.first().context("联系人未配置模型")?;
    let selection = format!("{}%%{}", model.model_code, model.provider_id);
    let input =
        serde_json::to_string(&serde_json::json!({"name":name,"description":member.description,
        "reminders":rows.iter().map(Reminder::summary).collect::<Vec<_>>()}))?;
    let wording=tokio::time::timeout(std::time::Duration::from_secs(25),super::model::chat(app,session,&selection,
        "用联系人的口吻自然提醒用户，限一两句，纪念日当天可送祝福。只使用摘要里的事实，不声称已执行任何建议。直接输出正文。",&input)).await??;
    ensure!(
        !wording.is_empty() && wording.chars().count() <= 400 && !super::delta::sensitive(&wording),
        "联系人返回的提醒内容无效"
    );
    Ok(wording)
}

fn can_publish(cfg: &Config, day: NaiveDate, now: chrono::DateTime<Local>) -> bool {
    let time = now.format("%H:%M").to_string();
    now.date_naive() == day && !cfg.quiet(&time)
        && !(cfg.quiet_start != cfg.quiet_end && time >= cfg.quiet_end && time < cfg.release)
}

fn ready_to_publish(db: &Connection, rows: &[Reminder], cfg: &Config, today: NaiveDate) -> Result<bool> {
    if !can_publish(cfg,today,Local::now()) { return Ok(false); }
    Ok(!cfg.research || !rows.iter().any(|r|r.dated) || !super::delta::unread_digest(db,cfg)?)
}

pub async fn flush(app: &AppHandle, conn: &mut Connection, cfg: &Config) -> Result<()> {
    let now = Local::now();
    let today = now.date_naive();
    {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        schedule(&tx, now, cfg)?;
        tx.execute("UPDATE butler_proactive_delivery_queue SET status='cancelled' WHERE status='queued' AND (item_id IN
            (SELECT id FROM butler_proactive_item WHERE status IN ('done','dismissed')) OR (((stage='prepare' AND occurrence<=?1)
            OR (occurrence<?1 AND item_id IN (SELECT id FROM butler_proactive_item WHERE kind IN ('milestone','prep_topic')))) AND delivered_at IS NULL))",[today.to_string()])?;
        tx.commit()?;
    }
    let time = now.format("%H:%M").to_string();
    if cfg.quiet(&time)
        || (cfg.quiet_start != cfg.quiet_end && time >= cfg.quiet_end && time < cfg.release)
        || backoff(conn)?
    {
        return Ok(());
    }
    let mut groups: Vec<(Option<i64>, bool, Vec<Reminder>)> = Vec::new();
    for row in due(conn, cfg.research, super::topics::review_before_delivery(conn,cfg)?)? {
        let blessing = row.blessing(today);
        if let Some(group) = groups.iter_mut().find(|g| g.0 == row.owner && g.1 == blessing) {
            group.2.push(row)
        } else {
            groups.push((row.owner, blessing, vec![row]))
        }
    }
    for (owner, blessing, rows) in groups {
        if !within_budget(conn, today, cfg.daily_limit, blessing)? {
            continue;
        }
        if backoff(conn)? || cfg.quiet(&Local::now().format("%H:%M").to_string()) {
            break;
        }
        let mut reason = None;
        let mut delivered = None;
        if let Some(owner) = owner {
            let result=async{
                let (target,name)=contact(app,owner)?;
                let session:i64=conn.query_row("SELECT conversation_id FROM butler_proactive_run WHERE conversation_id IS NOT NULL ORDER BY started_at DESC LIMIT 1",[],|r|r.get(0))?;
                let wording=contact_wording(app,session,owner,&name,&rows).await?;
                let message=publish(conn,target,&rows,&wording,None,cfg,today)?;
                Ok::<_,anyhow::Error>((target,message,name,wording))
            }.await;
            match result {
                Ok(value) => delivered = Some(value),
                Err(e) => reason = Some(format!("{e:#}")),
            }
        }
        let (target, message, name, wording) = if let Some(value) = delivered {
            value
        } else {
            if !ready_to_publish(conn,&rows,cfg,today)? {
                break;
            }
            let target = crate::api::butler_api::active_butler_main_conversation_id(app)
                .map_err(anyhow::Error::msg)?
                .context("没有可用的总管家主会话")?;
            let wording = if reason.is_some() {
                fallback_summary(conn, &rows)?
            } else {
                rows.iter().map(Reminder::summary).collect::<Vec<_>>().join("\n")
            };
            let message = publish(conn, target, &rows, &wording, reason.as_deref(), cfg, today)?;
            (target, message, "总管家".into(), wording)
        };
        crate::utils::window_utils::send_conversation_event_to_chat_windows(
            app,
            target,
            crate::api::ai::events::ConversationEvent {
                r#type: "message_add".into(),
                data: serde_json::json!({"message_id":message,"message_type":"response"}),
            },
        );
        let _ = app.emit("butler_proactive_changed", target);
        use tauri::Manager;
        let config_map =
            app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
        crate::api::ai::agent_completion::notify_butler_reply(app, &name, &wording, &config_map);
        crate::sync::schedule_sync_after_local_change(app);
    }
    Ok(())
}

fn external_summary(conn: &Connection, recipient: &str) -> Result<Option<(String, String)>> {
    use rusqlite::OptionalExtension;
    let row=conn.query_row("SELECT q.external_key,count(*),group_concat(DISTINCT CASE i.kind WHEN 'milestone' THEN '纪念日'
        WHEN 'prep_topic' THEN '预研简报' ELSE '到期待办' END)
        FROM butler_proactive_delivery_queue q JOIN butler_proactive_item i ON i.id=q.item_id
        WHERE q.external_status='queued' AND q.external_recipient=?1 AND q.status='delivered' AND i.status='active'
        AND date(q.delivered_at,'unixepoch','localtime')=date('now','localtime') GROUP BY q.external_key ORDER BY min(q.id) LIMIT 1",[recipient],
        |r|Ok((r.get::<_,String>(0)?,r.get::<_,u32>(1)?,r.get::<_,String>(2)?))).optional()?;
    Ok(row.map(|(key, count, kinds)| {
        (key, format!("管家有 {count} 项提醒（{}），可在 AIPP 查看。", kinds.replace(',', "、")))
    }))
}

pub async fn external(app: &AppHandle, cfg: &Config) -> Result<()> {
    let time = Local::now().format("%H:%M").to_string();
    if cfg.feishu_open_id.is_empty()
        || cfg.quiet(&time)
        || (cfg.quiet_start != cfg.quiet_end && time >= cfg.quiet_end && time < cfg.release)
    {
        return Ok(());
    }
    let conn = super::connection(app)?;
    if let Some((key, summary)) = external_summary(&conn, &cfg.feishu_open_id)? {
        let result =
            crate::feishu::send_proactive_summary(app, &cfg.feishu_open_id, &summary, &key).await;
        let (status, error) = match result {
            Ok(_) => ("sent", None),
            Err(error) => ("failed", Some(error)),
        };
        conn.execute("UPDATE butler_proactive_delivery_queue SET external_status=?2,external_error=?3 WHERE external_key=?1",params![key,status,error])?;
        if let Some(error) = error {
            anyhow::bail!("提醒 {key} 已在 AIPP 投递，飞书发送失败：{error}")
        }
    }
    Ok(())
}

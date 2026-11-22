use super::{config, connection, delivery, delta, lunar, memory, model, proposal, topics, Config};
use anyhow::{ensure, Context, Result};
use chrono::{Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

pub(super) static RUN: Mutex<()> = Mutex::const_new(());

pub fn event_date(raw: &str, yearly: bool, lunar: bool, today: NaiveDate) -> Result<NaiveDate> {
    if !yearly {
        if lunar {
            let (year, md) = raw.split_once('-').context("一次性农历日期格式为 YYYY-MM-DD")?;
            let (month, day) = md.trim_start_matches('L').split_once('-').context("缺少农历日")?;
            return lunar::anniversary(
                year.parse()?,
                month.parse()?,
                md.starts_with('L'),
                day.parse()?,
            )
            .context("无效农历日期");
        }
        return Ok(NaiveDate::parse_from_str(raw, "%Y-%m-%d")?);
    }
    let (month, day) =
        raw.trim_start_matches('L').split_once('-').context("年度日期格式为 MM-DD")?;
    lunar::next_annual(today, lunar, month.parse()?, raw.starts_with('L'), day.parse()?)
        .context("纪念日超出有效日期范围")
}

pub(super) fn persist(
    conn: &Connection,
    source: &delta::Source,
    f: &model::Finding,
    chief: i64,
    session: i64,
    cfg: &Config,
) -> Result<()> {
    delta::verify(conn, source)?;
    ensure!(
        !f.quote.trim().is_empty() && source.text.contains(&f.quote),
        "整理结果缺少可核对的原话"
    );
    ensure!(!f.title.trim().is_empty() && f.title.chars().count() <= 300, "事项标题为空或过长");
    ensure!(
        ![&f.title, &f.detail, &f.instruction].iter().any(|s| delta::sensitive(s)),
        "整理结果含疑似敏感信息"
    );
    if f.kind == "memory" && !cfg.remember {
        return Ok(());
    }
    ensure!(!f.yearly || f.date.is_some(), "年度事项缺少日期");
    if matches!(f.kind.as_str(), "open_loop" | "proposal") {
        let covered: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM scheduled_task WHERE is_enabled=1 AND assistant_id=?1
            AND (trim(name)=trim(?2) OR (trim(?3)!='' AND trim(task_prompt)=trim(?3))))",
            params![source.assistant.unwrap_or(chief), f.title, f.instruction],
            |r| r.get(0),
        )?;
        if covered {
            return Ok(());
        }
    }
    // Before fact-specific receipts a memory could have been explicitly undone.
    // Replaying an old digest must not recreate it after that undo.
    if f.kind=="memory" && conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_proactive_evidence
        WHERE conversation_id=?1 AND source_position=?2 AND kind='memory' AND quote=?3)",
        params![source.conversation,source.position,f.quote],|r|r.get::<_,bool>(0))? {
        return Ok(());
    }
    // The same user statement can support two independently actionable findings.
    let evidence_kind = if f.kind=="memory" {
        serde_json::to_string(&(&f.kind,f.title.trim()))?
    } else {
        serde_json::to_string(&(&f.kind,f.title.trim(),&f.date,f.yearly,f.lunar,f.instruction.trim()))?
    };
    if conn.execute(
        "INSERT OR IGNORE INTO butler_proactive_evidence VALUES(?1,?2,?3,?4)",
        params![source.conversation, source.position, evidence_kind, f.quote],
    )? == 0
    {
        return Ok(());
    }
    if f.kind == "memory" {
        if cfg.remember {
            memory::save(conn, source, chief, &f.title, f.replaces, f.global)?;
        }
        return Ok(());
    }
    ensure!(
        matches!(f.kind.as_str(), "open_loop" | "milestone" | "prep_topic" | "proposal"),
        "无法识别的整理事项"
    );
    ensure!(f.kind != "proposal" || !f.instruction.trim().is_empty(), "建议缺少待执行指令");
    ensure!(
        f.detail.chars().count() <= 16_000
            && f.instruction.chars().count() <= 4000
            && f.lead_days <= 366,
        "事项内容或提前天数超限"
    );
    let today = Local::now().date_naive();
    let occurrence = f
        .date
        .as_deref()
        .map(|s| event_date(s, f.yearly, f.lunar, today))
        .transpose()?
        .unwrap_or(today);
    let next = occurrence - Duration::days(f.lead_days.into());
    let owner = if source.kind == "butler_contact" { source.assistant } else { None };
    // Earlier schemas also identify an item by its display title within one message.
    // Distinguish two occurrences with the same label while preserving exact replays.
    let mut title = f.title.clone();
    let mut ordinal = 1;
    loop {
        let same = conn.query_row("SELECT event_date IS ?5 AND yearly=?6 AND (calendar_type='lunar')=?7 AND action_prompt=?8
            FROM butler_proactive_item WHERE source_conversation_id=?1 AND
            coalesce(source_message_id,(SELECT seq FROM butler_chat_group_message WHERE id=source_group_post_id),source_position)=?2
            AND kind=?3 AND title=?4",params![source.conversation,source.position,f.kind,title,f.date,f.yearly,f.lunar,f.instruction],
            |r|r.get::<_,bool>(0)).optional()?;
        match same {
            None => break,
            Some(true) => return Ok(()),
            Some(false) => {
                ordinal+=1;
                title=match (&f.date,ordinal) {
                    (Some(date),2)=>format!("{}（{}）",f.title,date),
                    _=>format!("{}（{}）",f.title,ordinal),
                };
            }
        }
    }
    let inserted=conn.execute("INSERT INTO butler_proactive_item(uuid,kind,title,detail,source_conversation_id,source_position,owner_assistant_id,
        event_date,yearly,calendar_type,lead_days,next_check_at,action_prompt,updated_at,source_message_id,source_group_post_id,high_priority)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,unixepoch(),
            CASE WHEN ?14!='butler_group' THEN ?6 END,
            CASE WHEN ?14='butler_group' THEN (SELECT id FROM butler_chat_group_message WHERE group_id=?5 AND seq=?6) END,?15) ON CONFLICT DO NOTHING",
        params![uuid::Uuid::new_v4().to_string(),f.kind,title,f.detail,source.conversation,source.position,owner,
        f.date,f.yearly,if f.lunar {"lunar"} else {"solar"},f.lead_days,next.to_string(),f.instruction,source.kind,f.high_priority])?;
    if inserted == 0 {return Ok(());}
    let item = conn.last_insert_rowid();
    topics::link(conn,item,source,&f.quote)?;
    if !f.detail.is_empty() && f.kind == "prep_topic" {
        conn.execute("INSERT INTO message(conversation_id,message_type,content,metadata_json,token_count) VALUES(?1,'response',?2,
            json_object('origin','proactive_brief','item_id',?3),0)",params![session,f.detail,item])?;
        conn.execute(
            "UPDATE butler_proactive_item SET prep_message_id=?2 WHERE id=?1",
            params![item, conn.last_insert_rowid()],
        )?;
    }
    Ok(())
}

fn trigger(conn: &Connection, cfg: &Config, manual: bool) -> Result<Option<&'static str>> {
    if manual {
        return Ok(Some("manual"));
    }
    let now = Local::now();
    let time = now.format("%H:%M").to_string();
    let day =
        if time >= cfg.digest { now.date_naive() } else { now.date_naive() - Duration::days(1) };
    let last=conn.query_row("SELECT max(started_at) FROM butler_proactive_run WHERE trigger_kind='digest' AND status IN ('quiet','acted','failed')",[],|r|r.get::<_,Option<i64>>(0))?;
    let due = day
        .and_time(chrono::NaiveTime::parse_from_str(&cfg.digest, "%H:%M")?)
        .and_local_timezone(Local)
        .earliest()
        .context("整理时间无效")?
        .timestamp();
    if last.is_none_or(|v| v < due) {
        return Ok(Some("digest"));
    }
    let last=conn.query_row("SELECT max(started_at) FROM butler_proactive_run WHERE trigger_kind IN ('pulse','idle_after_chat','digest','manual')",
        [],|r|r.get::<_,Option<i64>>(0))?.unwrap_or(0);
    if !cfg.quiet(&time) && now.timestamp() - last >= i64::from(cfg.pulse_hours) * 3600 {
        return Ok(Some("pulse"));
    }
    Ok(Some("idle_after_chat"))
}

pub async fn run(app: &AppHandle, manual: bool) -> Result<String> {
    let Ok(_guard) = RUN.try_lock() else {
        if manual {
            anyhow::bail!("已有整理正在进行")
        }
        return Ok(String::new());
    };
    let stored = config(app)?;
    if !stored.enabled {
        // A proposal explicitly accepted by the user is an ordinary queued chat turn.
        // Closing the settings window or disabling future scans must not discard it.
        proposal::recover(app).await?;
        if manual {
            anyhow::bail!("请先开启主动整理")
        }
        return Ok(String::new());
    }
    crate::sync::proactive_owner(app, true).await.map_err(anyhow::Error::msg)?;
    let mut conn = connection(app)?;
    let cfg = super::effective_config(&conn, stored.clone())?;
    conn.execute("UPDATE butler_proactive_run SET status='failed',finished_at=unixepoch(),summary='上次整理在提交结果前中断，保留原扫描位置'
        WHERE status='running'",[])?;
    if let Err(e) = proposal::recover(app).await {
        record_failure(&conn, "proposal", &e)?;
    }
    if cfg.research && !cfg.reminders {
        let tx=conn.transaction()?;
        delivery::schedule(&tx,Local::now(),&cfg)?;
        tx.commit()?;
    }
    if cfg.reminders {
        if let Err(e) = delivery::flush(app, &mut conn, &cfg).await {
            record_failure(&conn, "delivery", &e)?;
        }
    }
    if cfg.reminders {
        if let Err(e) = delivery::external(app, &cfg).await {
            record_failure(&conn, "feishu", &e)?;
        }
    }
    let Some(kind) = trigger(&conn, &cfg, manual)? else { return Ok(String::new()) };
    let waking=topics::pending(&conn,&cfg)? || topics::review_before_delivery(&conn,&cfg)?;
    let kind=if waking && !matches!(kind,"digest"|"manual") {"milestone"} else {kind};
    let deep = matches!(kind, "digest" | "manual" | "milestone");
    let rows = delta::read(&conn, &cfg, deep)?;
    if !manual {
        if !app
            .state::<crate::ConversationActivityManager>()
            .list_running_conversation_ids()
            .await
            .is_empty()
        {
            return Ok(String::new());
        }
        let (pending, latest) = delta::pending(&conn, &cfg, deep)?;
        if Local::now().timestamp() - latest < if kind == "idle_after_chat" { 1200 } else { 600 } {
            return Ok(String::new());
        }
        if kind == "idle_after_chat" && pending < 4 {
            return Ok(String::new());
        }
    }
    if rows.is_empty() && !waking {
        if kind == "digest" {
            conn.execute("INSERT INTO butler_proactive_run(id,trigger_kind,status,started_at,finished_at,summary) VALUES(?1,'digest','quiet',unixepoch(),unixepoch(),'没有新增对话')",
                [uuid::Uuid::new_v4().to_string()])?;
        }
        return Ok("没有新增对话，已跳过整理".into());
    }
    let main = crate::api::butler_api::active_butler_main_conversation_id(app)
        .map_err(anyhow::Error::msg)?
        .context("请先配置总管家主会话")?;
    let chief: i64 =
        conn.query_row("SELECT assistant_id FROM conversation WHERE id=?1", [main], |r| r.get(0))?;
    let run = uuid::Uuid::new_v4().to_string();
    let session = {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO conversation(name,assistant_id,conversation_kind,is_hidden_from_normal_chat_list) VALUES('管家主动整理',?1,'butler_proactive_run',1)",[chief])?;
        let session = tx.last_insert_rowid();
        tx.execute("INSERT INTO butler_proactive_run(id,trigger_kind,status,conversation_id,started_at,scanned)
        VALUES(?1,?2,'running',?3,unixepoch(),?4)",params![run,kind,session,rows.len()])?;
        tx.commit()?;
        session
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        async {
            let findings=analyze(app, session, &cfg, &rows, deep, chief).await?;
            let tracked=topics::active(&connection(app)?,&cfg)?;
            let changes=if deep {topics::reconcile(app,session,&cfg,&tracked,&rows,&findings).await?}else{vec![]};
            Ok::<_,anyhow::Error>((findings,tracked,changes))
        },
    )
    .await
    .context("整理超过 300 秒")
    .and_then(|r| r);
    let persisted = (|| -> Result<(String, String)> {
        let (findings,tracked,changes) = result?;
        ensure!(config(app)? == stored, "整理期间设置已改变，本批次保留待重新扫描");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let merged=topics::apply(&tx,&tracked,&rows,&findings,&changes)?;
        for (draft,(index, f)) in findings.iter().enumerate() {
            if merged.contains(&draft) {continue;}
            if f.yearly && f.date.is_none() {
                record_failure(&tx,"finding",&anyhow::anyhow!("来源 #{} / {} 的年度事项缺少日期",rows[*index].conversation,rows[*index].position))?;
                continue;
            }
            persist(&tx, &rows[*index], f, chief, session, &cfg)?;
        }
        delta::advance(&tx, &rows, deep)?;
        let summary = format!("扫描 {} 条用户消息，归纳 {} 个关联事项，整理出 {} 项。", rows.len(), changes.len(), findings.len()-merged.len());
        let status = if kind == "digest" && delta::pending(&tx, &cfg, true)?.0 > 0 {
            "partial"
        } else if findings.is_empty() && changes.is_empty() && !waking {
            "quiet"
        } else {
            "acted"
        };
        tx.execute("UPDATE butler_proactive_run SET status=?2,summary=?3,finished_at=unixepoch() WHERE id=?1",params![run,status,summary])?;
        tx.commit()?;
        Ok((status.into(), summary))
    })();
    let (_, summary) = match persisted {
        Ok(done) => done,
        Err(e) => {
            conn.execute("UPDATE butler_proactive_run SET status='failed',summary=?2,finished_at=unixepoch() WHERE id=?1",params![run,format!("{e:#}")])?;
            cleanup(&mut conn)?;
            return Err(e.context(format!("主动整理 {run}，会话 #{session}")));
        }
    };
    let mut summary=summary;
    if cfg.research && deep && delta::pending(&conn,&cfg,true)?.0==0 {
        let count=topics::wake(app,&mut conn,&cfg,chief,session).await?;
        if count>0 {
            summary.push_str(&format!(" 已重新预研 {count} 项。"));
            conn.execute("UPDATE butler_proactive_run SET summary=?2,status='acted' WHERE id=?1",params![run,summary])?;
        }
    }
    crate::sync::schedule_sync_after_local_change(app);
    let _ = app.emit(crate::api::butler_memory::CHANGED_EVENT, ());
    if cfg.reminders {
        delivery::flush(app, &mut conn, &cfg).await?;
    }
    cleanup(&mut conn)?;
    Ok(summary)
}

pub(super) fn record_failure(conn: &Connection, kind: &str, error: &anyhow::Error) -> Result<()> {
    let detail = format!("{error:#}");
    tracing::warn!(kind, "主动管家：{detail}");
    conn.execute("INSERT INTO butler_proactive_run(id,trigger_kind,status,summary,started_at,finished_at)
        SELECT ?1,?2,'failed',?3,unixepoch(),unixepoch() WHERE NOT EXISTS(SELECT 1 FROM butler_proactive_run
        WHERE trigger_kind=?2 AND summary=?3 AND started_at>unixepoch()-3600)",params![uuid::Uuid::new_v4().to_string(),kind,detail])?;
    Ok(())
}

pub(super) fn cleanup(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch("CREATE TEMP TABLE IF NOT EXISTS expired_proactive(id INTEGER PRIMARY KEY);
        DELETE FROM expired_proactive;
        INSERT INTO expired_proactive SELECT conversation_id FROM butler_proactive_run WHERE conversation_id IS NOT NULL
        ORDER BY started_at DESC,rowid DESC LIMIT -1 OFFSET 30;
        INSERT INTO conversation(name,conversation_kind,is_hidden_from_normal_chat_list)
        SELECT '管家预研简报','butler_proactive_brief',1 WHERE EXISTS(SELECT 1 FROM message m WHERE m.conversation_id IN (SELECT id FROM expired_proactive) AND m.id IN
        (SELECT prep_message_id FROM butler_proactive_item UNION SELECT brief_message_id FROM butler_proactive_delivery_queue))
        AND NOT EXISTS(SELECT 1 FROM conversation WHERE conversation_kind='butler_proactive_brief');
        UPDATE message SET conversation_id=(SELECT min(id) FROM conversation WHERE conversation_kind='butler_proactive_brief')
        WHERE conversation_id IN (SELECT id FROM expired_proactive) AND id IN (SELECT prep_message_id FROM butler_proactive_item UNION SELECT brief_message_id FROM butler_proactive_delivery_queue);
        DELETE FROM message WHERE conversation_id IN (SELECT id FROM expired_proactive);
        DELETE FROM conversation WHERE id IN (SELECT id FROM expired_proactive) AND conversation_kind='butler_proactive_run';
        UPDATE butler_proactive_run SET conversation_id=NULL WHERE conversation_id IN (SELECT id FROM expired_proactive);")?;
    tx.commit()?;
    Ok(())
}

async fn analyze(
    app: &AppHandle,
    session: i64,
    cfg: &Config,
    rows: &[delta::Source],
    deep: bool,
    chief: i64,
) -> Result<Vec<(usize, model::Finding)>> {
    let mut output = Vec::new();
    let mut remaining_tools = 12;
    // Isolate each conversation, including research evidence and memory candidates.
    let mut start = 0;
    while start < rows.len() {
        let sources = &rows[start..];
        let end = sources.iter().take_while(|s| s.conversation == sources[0].conversation).count();
        let batch = &sources[..end];
        if batch.iter().any(|s| !s.text.is_empty()) {
            let viewer = if batch[0].kind == "butler_group" {
                crate::db::butler_memory::Viewer::Group(batch[0].conversation)
            } else {
                crate::db::butler_memory::Viewer::Assistant(if batch[0].kind == "butler_contact" {
                    batch[0].assistant.context("缺少联系人")?
                } else {
                    chief
                })
            };
            let memories = crate::db::butler_memory::list(&connection(app)?, viewer)?
                .into_iter()
                .take(30)
                .filter(|m| !delta::sensitive(&m.content))
                .map(|m| serde_json::json!({"id":m.id,"revision":m.revision,"text":m.content}))
                .collect();
            let (tracked, existing_schedules) = {
                let db = connection(app)?;
                let mut stmt=db.prepare("SELECT kind,title,status FROM butler_proactive_item WHERE source_conversation_id=?1 ORDER BY updated_at DESC LIMIT 30")?;
                let tracked = stmt
                    .query_map([batch[0].conversation], |r| {
                        Ok(serde_json::json!({"kind":r.get::<_,String>(0)?,
                    "title":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?}))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut tasks=db.prepare("SELECT name FROM scheduled_task WHERE is_enabled=1 AND assistant_id=?1 ORDER BY id DESC LIMIT 30")?;
                let existing = tasks
                    .query_map([batch[0].assistant.unwrap_or(chief)], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .into_iter()
                    .filter(|s| !delta::sensitive(s))
                    .collect();
                (tracked, existing)
            };
            let input = serde_json::to_string(&model::Input {
                today: Local::now().date_naive().to_string(),
                sources: batch,
                memories,
                tracked,
                existing_schedules,
            })?;
            let act = if deep {
                true
            } else {
                let mut selection = cfg.small_model.clone();
                if selection.is_empty() {
                    selection = app
                        .state::<crate::FeatureConfigState>()
                        .config_feature_map
                        .lock()
                        .await
                        .get("experimental")
                        .and_then(|m| m.get(crate::api::butler_progress::HELPER_MODEL_KEY))
                        .map(|c| c.value.clone())
                        .unwrap_or_default();
                }
                match model::chat(
                    app,
                    session,
                    &selection,
                    "判断近期是否值得主动提醒。参考 tracked 里的采纳、完成和忽略记录。已安排或已忽略的同类事项，除非用户明确重新提出或出现新的截止日期，否则返回 QUIET。确有新的准备或提醒需要时返回 ACT。只输出 ACT 或 QUIET。",
                    &input,
                )
                .await?
                .as_str()
                {
                    "ACT" => true,
                    "QUIET" => false,
                    _ => anyhow::bail!("轻巡检未返回 ACT 或 QUIET"),
                }
            };
            if act {
                model::audit(
                    app,
                    session,
                    &format!("整理会话 #{} 的 {} 条用户发言。", batch[0].conversation, end),
                )?;
                let research = super::research::Research {
                    app,
                    session,
                    chief,
                    config: cfg,
                    source: &batch[0],
                    sources: &[],
                    item: None,
                };
                for f in model::findings(&research, &input, &mut remaining_tools).await? {
                    ensure!(f.source < end, "整理来源编号超出本批次");
                    output.push((start + f.source, f));
                }
            } else {
                model::audit(
                    app,
                    session,
                    &format!("会话 #{} 初筛结果为安静。", batch[0].conversation),
                )?;
            }
        }
        start += end;
    }
    Ok(output)
}

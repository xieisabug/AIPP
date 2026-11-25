//! 主动整理调度与主执行器：
//! 1. 空闲与触发判断（`idle_after_chat` / `pulse` / `digest` / `milestone` / `manual`）；
//! 2. 轻巡检（小模型 `ACT` / `QUIET`）跳过无需介入的日常闲聊；
//! 3. 深度运行统一交给 `agent::execute`（支持普通模型无头循环与原生 Agent），
//!    由 AI 自主组合全局工具与主动整理专用工具完成整理；
//! 4. 推进双水位线、收尾唤醒事项、按节律投递桌面卡片与管家飞书提醒。
use super::{agent, config, connection, delivery, delta, lunar, prompt, proposal, topics, Config};
use anyhow::{ensure, Context, Result};
use chrono::{Duration, Local, NaiveDate};
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::{json, Value};
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

fn trigger(conn: &Connection, cfg: &Config, manual: bool) -> Result<Option<&'static str>> {
    if manual {
        return Ok(Some("manual"));
    }
    let now = Local::now();
    let time = now.format("%H:%M").to_string();
    let day =
        if time >= cfg.digest { now.date_naive() } else { now.date_naive() - Duration::days(1) };
    let last = conn.query_row(
        "SELECT max(started_at) FROM butler_proactive_run WHERE trigger_kind='digest' AND status IN ('quiet','acted','failed')",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )?;
    let due = day
        .and_time(chrono::NaiveTime::parse_from_str(&cfg.digest, "%H:%M")?)
        .and_local_timezone(Local)
        .earliest()
        .context("整理时间无效")?
        .timestamp();
    if last.is_none_or(|v| v < due) {
        return Ok(Some("digest"));
    }
    let last = conn
        .query_row(
            "SELECT max(started_at) FROM butler_proactive_run WHERE trigger_kind IN ('pulse','idle_after_chat','digest','manual')",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )?
        .unwrap_or(0);
    if !cfg.quiet(&time) && now.timestamp() - last >= i64::from(cfg.pulse_hours) * 3600 {
        return Ok(Some("pulse"));
    }
    Ok(Some("idle_after_chat"))
}

fn collect_context_snapshot(
    conn: &Connection,
    cfg: &Config,
    chief: i64,
    rows: &[delta::Source],
) -> Result<(Vec<Value>, Vec<topics::Topic>, Vec<String>, Vec<Value>)> {
    let mut memories = Vec::new();
    let mut seen_memory_ids = std::collections::HashSet::new();
    let push_viewer = |memories: &mut Vec<Value>,
                       seen_memory_ids: &mut std::collections::HashSet<i64>,
                       viewer: crate::db::butler_memory::Viewer|
     -> Result<()> {
        for m in crate::db::butler_memory::list(conn, viewer)?.into_iter().take(25) {
            if delta::sensitive(&m.content) || !seen_memory_ids.insert(m.id) {
                continue;
            }
            memories.push(json!({
                "id": m.id,
                "revision": m.revision,
                "scope": m.scope_type,
                "text": m.content,
            }));
        }
        Ok(())
    };
    push_viewer(&mut memories, &mut seen_memory_ids, crate::db::butler_memory::Viewer::Assistant(chief))?;
    let mut visited_viewers = std::collections::HashSet::new();
    for row in rows {
        if row.kind == "butler_group" && visited_viewers.insert((1u8, row.conversation)) {
            push_viewer(&mut memories, &mut seen_memory_ids, crate::db::butler_memory::Viewer::Group(row.conversation))?;
        } else if row.kind == "butler_contact" {
            if let Some(a) = row.assistant {
                if visited_viewers.insert((2u8, a)) {
                    push_viewer(&mut memories, &mut seen_memory_ids, crate::db::butler_memory::Viewer::Assistant(a))?;
                }
            }
        }
        if memories.len() >= 60 {
            break;
        }
    }

    let tracked = topics::active(conn, cfg)?;

    let mut tasks_stmt = conn.prepare("SELECT name FROM scheduled_task WHERE is_enabled=1 ORDER BY id DESC LIMIT 40")?;
    let existing_schedules: Vec<String> = tasks_stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|s| !delta::sensitive(s))
        .collect();

    let mut notes_stmt = conn.prepare(
        "SELECT trigger_kind, status, summary, started_at FROM butler_proactive_run
         WHERE summary!='' AND status IN ('acted','quiet','partial') ORDER BY started_at DESC LIMIT 6",
    )?;
    let recent_notes: Vec<Value> = notes_stmt
        .query_map([], |r| {
            Ok(json!({
                "trigger": r.get::<_, String>(0)?,
                "status": r.get::<_, String>(1)?,
                "summary": r.get::<_, String>(2)?,
                "time": r.get::<_, i64>(3)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok((memories, tracked, existing_schedules, recent_notes))
}

async fn light_triage(
    app: &AppHandle,
    session: i64,
    cfg: &Config,
    rows: &[delta::Source],
    tracked: &[topics::Topic],
    existing_schedules: &[String],
) -> Result<bool> {
    if rows.iter().all(|s| s.text.is_empty()) {
        return Ok(false);
    }
    let mut selection = cfg.small_model.clone();
    if selection.is_empty() {
        let map = app
            .state::<crate::FeatureConfigState>()
            .config_feature_map
            .lock()
            .await
            .clone();
        selection = map
            .get("experimental")
            .and_then(|m| m.get(crate::api::butler_progress::HELPER_MODEL_KEY))
            .map(|c| c.value.clone())
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                map.get("conversation_summary")
                    .and_then(|m| m.get(crate::api::ai::config::DEFAULT_LIGHT_MODEL_KEY))
                    .map(|c| c.value.clone())
                    .filter(|v| !v.trim().is_empty())
            })
            .unwrap_or_default();
    }
    let input = serde_json::to_string(&json!({
        "today": Local::now().date_naive().to_string(),
        "new_messages": rows,
        "tracked_items": tracked,
        "existing_schedules": existing_schedules,
    }))?;
    let verdict = agent::chat_simple(app, session, &selection, &prompt::light_triage_prompt(cfg), &input).await?;
    let trimmed = verdict.trim().to_uppercase();
    if trimmed.starts_with("ACT") {
        Ok(true)
    } else if trimmed.starts_with("QUIET") {
        Ok(false)
    } else {
        anyhow::bail!("轻巡检未返回 ACT 或 QUIET（实际返回：{}）", verdict.chars().take(60).collect::<String>())
    }
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
        proposal::recover(app).await?;
        if manual {
            anyhow::bail!("请先开启主动整理")
        }
        return Ok(String::new());
    }
    {
        let _owner = super::OWNER_CHANGE.lock().await;
        crate::sync::proactive_owner(app, true).await.map_err(anyhow::Error::msg)?;
    }
    let mut conn = connection(app)?;
    let cfg = super::effective_config(&conn, stored.clone())?;
    conn.execute(
        "UPDATE butler_proactive_run SET status='failed',finished_at=unixepoch(),summary='上次整理在提交结果前中断，保留原扫描位置'
         WHERE status='running'",
        [],
    )?;
    if let Err(e) = proposal::recover(app).await {
        record_failure(&conn, "proposal", &e)?;
    }
    {
        let tx = conn.transaction()?;
        delivery::schedule(&tx, Local::now(), &cfg)?;
        tx.commit()?;
    }
    if cfg.allow_notifications {
        if let Err(e) = delivery::flush(app, &mut conn, &cfg).await {
            record_failure(&conn, "delivery", &e)?;
        }
        if let Err(e) = delivery::external(app, &cfg).await {
            record_failure(&conn, "feishu", &e)?;
        }
    }
    let Some(kind) = trigger(&conn, &cfg, manual)? else {
        return Ok(String::new());
    };
    let waking = topics::pending(&conn, &cfg)? || topics::review_before_delivery(&conn, &cfg)?;
    let kind = if waking && !matches!(kind, "digest" | "manual") { "milestone" } else { kind };
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
        if kind == "idle_after_chat" && pending < 4 && !waking {
            return Ok(String::new());
        }
    }
    if rows.is_empty() && !waking {
        if kind == "digest" {
            conn.execute(
                "INSERT INTO butler_proactive_run(id,trigger_kind,status,started_at,finished_at,summary)
                 VALUES(?1,'digest','quiet',unixepoch(),unixepoch(),'没有新增对话')",
                [uuid::Uuid::new_v4().to_string()],
            )?;
        }
        return Ok("没有新增对话，已跳过整理".into());
    }
    let main = crate::api::butler_api::active_butler_main_conversation_id(app)
        .map_err(anyhow::Error::msg)?
        .context("请先配置总管家主会话")?;
    let chief: i64 = conn.query_row("SELECT assistant_id FROM conversation WHERE id=?1", [main], |r| r.get(0))?;
    let run_id = uuid::Uuid::new_v4().to_string();
    let session = {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO conversation(name,assistant_id,conversation_kind,is_hidden_from_normal_chat_list)
             VALUES('管家主动整理',?1,'butler_proactive_run',1)",
            [chief],
        )?;
        let session = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO butler_proactive_run(id,trigger_kind,status,conversation_id,started_at,scanned)
             VALUES(?1,?2,'running',?3,unixepoch(),?4)",
            params![run_id, kind, session, rows.len()],
        )?;
        tx.commit()?;
        session
    };

    let (memories, tracked, existing_schedules, recent_notes) =
        collect_context_snapshot(&conn, &cfg, chief, &rows)?;
    let (waking_stages, waking_items) = if deep {
        topics::waking_jobs(&conn, &cfg, None)?
    } else {
        (Vec::new(), Vec::new())
    };

    // 轻巡检：聊后/定期巡检先用小模型决定是否值得唤醒深层 Agent
    if !deep {
        match light_triage(app, session, &cfg, &rows, &tracked, &existing_schedules).await {
            Ok(false) => {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                delta::advance(&tx, &rows, false)?;
                let summary = format!("扫描 {} 条新增发言，小模型初筛无需介入。", rows.len());
                tx.execute(
                    "UPDATE butler_proactive_run SET status='quiet',summary=?2,finished_at=unixepoch() WHERE id=?1",
                    params![run_id, summary],
                )?;
                tx.commit()?;
                cleanup(&mut conn)?;
                return Ok(summary);
            }
            Ok(true) => {}
            Err(e) => {
                conn.execute(
                    "UPDATE butler_proactive_run SET status='failed',summary=?2,finished_at=unixepoch() WHERE id=?1",
                    params![run_id, format!("{e:#}")],
                )?;
                cleanup(&mut conn)?;
                return Err(e.context(format!("主动整理初筛 {run_id}，会话 #{session}")));
            }
        }
    }

    let today = Local::now().date_naive();
    let system_prompt = prompt::system_prompt(&cfg, kind);
    let user_prompt = prompt::user_input(
        today,
        kind,
        &rows,
        &memories,
        &tracked,
        &existing_schedules,
        &recent_notes,
        &waking_items,
    );

    let exec_result = agent::execute(app, session, &cfg, chief, &system_prompt, &user_prompt).await;
    let output = match exec_result {
        Ok(out) => out,
        Err(e) => {
            conn.execute(
                "UPDATE butler_proactive_run SET status='failed',summary=?2,finished_at=unixepoch() WHERE id=?1",
                params![run_id, format!("{e:#}")],
            )?;
            cleanup(&mut conn)?;
            return Err(e.context(format!("主动整理 {run_id}，会话 #{session}")));
        }
    };

    let persisted = (|| -> Result<String> {
        ensure!(config(app)? == stored, "整理期间设置已改变，本批次保留待重新扫描");
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        delta::advance(&tx, &rows, deep)?;
        topics::finalize_woken(&tx, &waking_stages)?;
        let recorded_summary: String = tx
            .query_row("SELECT summary FROM butler_proactive_run WHERE id=?1", [&run_id], |r| r.get(0))
            .unwrap_or_default();
        let summary = if !recorded_summary.trim().is_empty() {
            recorded_summary
        } else {
            let excerpt: String = output
                .final_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(240)
                .collect();
            if excerpt.is_empty() || delta::sensitive(&excerpt) {
                format!("扫描 {} 条用户发言，执行 {} 次工具调用。", rows.len(), output.tool_calls_total)
            } else {
                excerpt
            }
        };
        let status = if kind == "digest" && delta::pending(&tx, &cfg, true)?.0 > 0 {
            "partial"
        } else if output.tool_calls_total == 0 && waking_stages.is_empty() {
            "quiet"
        } else {
            "acted"
        };
        tx.execute(
            "UPDATE butler_proactive_run SET status=?2,summary=?3,finished_at=unixepoch() WHERE id=?1",
            params![run_id, status, summary],
        )?;
        tx.commit()?;
        Ok(summary)
    })();

    let summary = match persisted {
        Ok(s) => s,
        Err(e) => {
            conn.execute(
                "UPDATE butler_proactive_run SET status='failed',summary=?2,finished_at=unixepoch() WHERE id=?1",
                params![run_id, format!("{e:#}")],
            )?;
            cleanup(&mut conn)?;
            return Err(e.context(format!("主动整理提交 {run_id}，会话 #{session}")));
        }
    };

    crate::sync::schedule_sync_after_local_change(app);
    let _ = app.emit(crate::api::butler_memory::CHANGED_EVENT, ());
    let _ = app.emit("butler_proactive_changed", ());
    if cfg.allow_notifications {
        if let Err(e) = delivery::flush(app, &mut conn, &cfg).await {
            record_failure(&conn, "delivery", &e)?;
        }
        if let Err(e) = delivery::external(app, &cfg).await {
            record_failure(&conn, "feishu", &e)?;
        }
    }
    cleanup(&mut conn)?;
    Ok(summary)
}

pub(super) fn record_failure(conn: &Connection, kind: &str, error: &anyhow::Error) -> Result<()> {
    let detail = format!("{error:#}");
    tracing::warn!(kind, "主动管家：{detail}");
    conn.execute(
        "INSERT INTO butler_proactive_run(id,trigger_kind,status,summary,started_at,finished_at)
        SELECT ?1,?2,'failed',?3,unixepoch(),unixepoch() WHERE NOT EXISTS(SELECT 1 FROM butler_proactive_run
        WHERE trigger_kind=?2 AND summary=?3 AND started_at>unixepoch()-3600)",
        params![uuid::Uuid::new_v4().to_string(), kind, detail],
    )?;
    Ok(())
}

pub(super) fn cleanup(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS expired_proactive(id INTEGER PRIMARY KEY);
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
        UPDATE butler_proactive_run SET conversation_id=NULL WHERE conversation_id IN (SELECT id FROM expired_proactive);",
    )?;
    tx.commit()?;
    Ok(())
}

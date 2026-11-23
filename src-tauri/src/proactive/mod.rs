mod agent;
mod delivery;
mod delta;
pub(crate) mod gate;
mod lunar;
mod memory;
mod prompt;
pub(crate) mod proposal;
mod runner;
pub(crate) mod tools;
mod topics;
#[cfg(test)]
#[path = "proactive_tests.rs"]
mod tests;
use crate::db::{conversation_db::ConversationDatabase, system_db::SystemDatabase};
use anyhow::{ensure, Context, Result};
use chrono::{Local, NaiveTime};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub private_chats: bool,
    pub groups: bool,
    pub normal_chats: bool,
    /// 是否允许主动发送桌面提醒与管家飞书推送（关闭为静默模式，后台照常整理）。
    #[serde(alias = "reminders")]
    pub allow_notifications: bool,
    /// 可选的用户自定义整理偏好与关注重点，注入系统 Prompt 引导管家自主发挥。
    pub custom_instructions: String,
    pub pulse_hours: u32,
    pub daily_limit: u32,
    pub quiet_start: String,
    pub quiet_end: String,
    pub release: String,
    pub digest: String,
    pub small_model: String,
    /// 整理执行者：""=沿用总管家模型；"code%%provider_id"=普通模型；"assistant:<id>"=原生 Agent 助手
    pub deep_model: String,
    pub adaptive_window: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            private_chats: true,
            groups: true,
            normal_chats: false,
            allow_notifications: true,
            custom_instructions: String::new(),
            pulse_hours: 3,
            daily_limit: 3,
            quiet_start: "22:30".into(),
            quiet_end: "08:00".into(),
            release: "08:30".into(),
            digest: "23:00".into(),
            small_model: String::new(),
            deep_model: String::new(),
            adaptive_window: false,
        }
    }
}
impl Config {
    fn validate(&self) -> Result<()> {
        ensure!(
            (1..=24).contains(&self.pulse_hours) && self.daily_limit <= 100,
            "巡检间隔或每日条数无效"
        );
        ensure!(
            self.custom_instructions.chars().count() <= 4000,
            "整理偏好与关注重点不能超过 4000 字"
        );
        for value in [&self.quiet_start, &self.quiet_end, &self.release, &self.digest] {
            let time = NaiveTime::parse_from_str(value, "%H:%M").context("时间格式为 HH:MM")?;
            ensure!(time.format("%H:%M").to_string() == *value, "时间须使用两位小时和分钟");
        }
        ensure!(
            self.release >= self.quiet_end && !self.quiet(&self.release),
            "释放时间必须在免打扰结束后的活跃时段"
        );
        Ok(())
    }
    fn quiet(&self, time: &str) -> bool {
        let (start, end) = (self.quiet_start.as_str(), self.quiet_end.as_str());
        if start > end {
            time >= start || time < end
        } else {
            time >= start && time < end
        }
    }
}
fn config(app: &AppHandle) -> Result<Config> {
    let config = SystemDatabase::new(app)?
        .get_feature_config("butler_proactive", "settings")?
        .map(|c| serde_json::from_str(&c.value))
        .transpose()?
        .unwrap_or_default();
    Config::validate(&config)?;
    Ok(config)
}
pub(crate) fn allow_notifications(app: &AppHandle) -> Result<bool> {
    Ok(config(app)?.allow_notifications)
}
fn connection(app: &AppHandle) -> Result<Connection> {
    Ok(ConversationDatabase::new(app)?.get_connection()?)
}
fn effective_config(conn: &Connection, mut cfg: Config) -> Result<Config> {
    if cfg.adaptive_window {
        if let Some(window) = activity_window(conn, &cfg)? {
            if let (Some(release), Some(digest)) =
                (window["release"].as_str(), window["digest"].as_str())
            {
                cfg.release = release.to_owned();
                cfg.digest = digest.to_owned();
            }
        }
    }
    Ok(cfg)
}
fn error(e: anyhow::Error) -> String {
    format!("{e:#}")
}
#[tauri::command]
pub async fn get_butler_proactive_card(
    app: AppHandle,
    uuid: String,
    message: i64,
    conversation: i64,
) -> Result<Value, String> {
    (||->Result<Value>{use rusqlite::OptionalExtension;
        let conn=connection(&app)?;
        let row=conn.query_row("SELECT i.id,i.revision,i.status,q.id,q.status,i.title FROM butler_proactive_item i
            JOIN butler_proactive_delivery_queue q ON q.item_id=i.id AND q.message_id=?2 AND q.conversation_id=?3
            WHERE i.uuid=?1",params![uuid,message,conversation],|r|Ok(json!({"item":r.get::<_,i64>(0)?,"revision":r.get::<_,i64>(1)?,
                "state":r.get::<_,String>(2)?,"delivery":r.get::<_,Option<i64>>(3)?,"status":r.get::<_,Option<String>>(4)?,"title":r.get::<_,String>(5)?}))).optional()?;
        Ok(row.unwrap_or(Value::Null))
    })().map_err(error)
}
pub fn tick(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = runner::run(&app, false).await {
            tracing::warn!("主动整理调度失败：{e:#}");
        }
    });
}
#[tauri::command]
pub async fn get_butler_proactive_config(app: AppHandle) -> Result<Config, String> {
    config(&app).map_err(error)
}
#[tauri::command]
pub async fn save_butler_proactive_config(app: AppHandle, settings: Config) -> Result<(), String> {
    let _lock = runner::RUN.lock().await;
    settings.validate().map_err(error)?;
    if settings.enabled {
        crate::sync::proactive_owner(&app, true).await?;
    }
    (||->Result<()>{
        SystemDatabase::new(&app)?.conn.execute("INSERT INTO feature_config(feature_code,key,value,data_type)
            VALUES('butler_proactive','settings',?1,'string') ON CONFLICT(feature_code,key) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(&settings)?])?; Ok(()) })().map_err(error)?;
    if !settings.enabled {
        crate::sync::proactive_owner(&app, false).await?;
    }
    Ok(())
}
#[tauri::command]
pub async fn run_butler_proactive_now(app: AppHandle) -> Result<String, String> {
    runner::run(&app, true).await.map_err(error)
}

#[tauri::command]
pub async fn get_butler_proactive_center(app: AppHandle) -> Result<Value, String> {
    (||->Result<Value>{
        let conn=connection(&app)?;
        let mut items=conn.prepare("SELECT id,kind,title,detail,status,event_date,next_check_at,uuid,revision,owner_assistant_id,action_prompt,
            source_conversation_id,coalesce(source_message_id,(SELECT seq FROM butler_chat_group_message WHERE id=source_group_post_id),source_position),
            yearly,calendar_type,lead_days,tracking_summary
            FROM butler_proactive_item ORDER BY updated_at DESC,id DESC")?;
        let items=items.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"kind":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,
            "detail":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,"date":r.get::<_,Option<String>>(5)?,"next":r.get::<_,Option<String>>(6)?,
            "uuid":r.get::<_,String>(7)?,"revision":r.get::<_,i64>(8)?,"owner":r.get::<_,Option<i64>>(9)?,"instruction":r.get::<_,String>(10)?,
            "source":r.get::<_,i64>(11)?,"position":r.get::<_,i64>(12)?,"yearly":r.get::<_,bool>(13)?,
            "calendar":r.get::<_,String>(14)?,"lead":r.get::<_,u32>(15)?,"tracking":r.get::<_,String>(16)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut runs=conn.prepare("SELECT id,trigger_kind,status,summary,scanned,started_at,conversation_id FROM butler_proactive_run ORDER BY started_at DESC LIMIT 30")?;
        let runs=runs.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"trigger":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,
            "summary":r.get::<_,String>(3)?,"scanned":r.get::<_,i64>(4)?,"time":r.get::<_,i64>(5)?,"conversation":r.get::<_,Option<i64>>(6)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let window=activity_window(&conn,&config(&app)?)?;
        let mut log=conn.prepare("SELECT i.title,q.status,q.occurrence,q.stage,q.fallback_reason,q.external_status,q.external_error
            FROM butler_proactive_delivery_queue q JOIN butler_proactive_item i ON i.id=q.item_id
            ORDER BY coalesce(q.delivered_at,q.deliver_after) DESC,q.id DESC LIMIT 30")?;
        let deliveries=log.query_map([],|r|Ok(json!({"title":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,
            "date":r.get::<_,String>(2)?,"stage":r.get::<_,String>(3)?,"fallback":r.get::<_,Option<String>>(4)?,
            "external":r.get::<_,Option<String>>(5)?,"error":r.get::<_,Option<String>>(6)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"items":items,"runs":runs,"window":window,"deliveries":deliveries}))
    })().map_err(error)
}

fn activity_window(conn: &Connection, cfg: &Config) -> Result<Option<Value>> {
    // Aggregate the first/last local activity on each day; one busy day has one vote.
    let mut stmt=conn.prepare("WITH activity(t) AS (
        SELECT m.created_time FROM message m JOIN conversation c ON c.id=m.conversation_id WHERE m.message_type='user'
        AND (m.metadata_json IS NULL OR (json_valid(m.metadata_json) AND json_extract(m.metadata_json,'$.origin') IS NULL))
        AND m.created_time>=datetime('now','-14 days') AND ((?1 AND c.conversation_kind IN ('butler_contact','butler_main')) OR (?2 AND c.conversation_kind='normal'
        AND c.is_hidden_from_normal_chat_list=0)) UNION ALL SELECT created_at FROM butler_chat_group_message
        WHERE ?3 AND sender_member IS NULL AND kind='message' AND created_at>=datetime('now','-14 days'))
        SELECT min(strftime('%H:%M',t,'localtime')),max(strftime('%H:%M',t,'localtime')) FROM activity GROUP BY date(t,'localtime')")?;
    let days = stmt
        .query_map(params![cfg.private_chats, cfg.normal_chats, cfg.groups], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if days.len() < 3 {
        return Ok(None);
    }
    let (mut start, mut end): (Vec<_>, Vec<_>) = days.into_iter().unzip();
    start.sort();
    end.sort();
    let middle = start.len() / 2;
    let release = if cfg.quiet(&start[middle]) || start[middle] < cfg.quiet_end {
        cfg.quiet_end.clone()
    } else {
        start[middle].clone()
    };
    let digest = NaiveTime::parse_from_str(&end[middle], "%H:%M")? + chrono::Duration::minutes(30);
    Ok(Some(
        json!({"release":release,"digest":digest.format("%H:%M").to_string(),"days":start.len()}),
    ))
}

#[derive(Deserialize)]
pub struct ItemTiming {
    date: String,
    yearly: bool,
    calendar: String,
    lead: u32,
}

#[tauri::command]
pub async fn update_butler_proactive_item(
    app: AppHandle,
    id: i64,
    revision: i64,
    action: String,
    title: Option<String>,
    delivery_id: Option<i64>,
    timing: Option<ItemTiming>,
) -> Result<(), String> {
    (||->Result<()>{let mut conn=connection(&app)?; let tx=conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (current,yearly):(i64,bool)=tx.query_row("SELECT revision,yearly FROM butler_proactive_item WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(current==revision,"事项已更新，请重新打开");
        match action.as_str() {
            "edit_proposal"=>{
                let prompt=title.context("缺少执行指令")?;
                ensure!(!prompt.trim().is_empty() && prompt.chars().count()<=4000 && !delta::sensitive(&prompt),"执行指令无效或含敏感信息");
                let changed=tx.execute("UPDATE butler_proactive_item SET action_prompt=?2,revision=revision+1 WHERE id=?1
                    AND kind='proposal' AND status='active' AND NOT EXISTS(SELECT 1 FROM butler_proactive_acceptance a
                    WHERE a.uuid=butler_proactive_item.uuid AND a.status IN ('pending','sent'))",params![id,prompt.trim()])?;
                ensure!(changed==1,"已派发或等待派发的建议无法编辑");
            },
            "dismiss"|"done" if action=="dismiss" || !yearly => {
                let state=if action=="dismiss" {"dismissed"} else {"done"};
                tx.execute("DELETE FROM butler_proactive_acceptance WHERE uuid=(SELECT uuid FROM butler_proactive_item WHERE id=?1) AND message_id IS NULL",[id])?;
                tx.execute("UPDATE butler_proactive_item SET status=?2,revision=revision+1 WHERE id=?1",params![id,state])?;
                tx.execute("UPDATE butler_proactive_delivery_queue SET status='cancelled' WHERE item_id=?1 AND status='queued'",[id])?;
            },
            "rename"=>{let title=title.context("缺少事项标题")?;
                ensure!(!title.trim().is_empty() && title.chars().count()<=300 && !delta::sensitive(&title),"事项标题无效");
                tx.execute("UPDATE butler_proactive_item SET title=?2,revision=revision+1 WHERE id=?1",params![id,title.trim()])?;
                if let Some(t)=timing {
                    ensure!(t.lead<=366 && matches!(t.calendar.as_str(),"solar"|"lunar"),"日历或提前天数无效");
                    ensure!(!t.date.is_empty() || !t.yearly,"年度纪念日需要日期");
                    let today=Local::now().date_naive();
                    let date=if t.date.is_empty(){None}else{Some(t.date.as_str())};
                    let next=date.map(|raw|runner::event_date(raw,t.yearly,t.calendar=="lunar",today)).transpose()?.unwrap_or(today)-chrono::Duration::days(t.lead.into());
                    let changed=tx.execute("UPDATE butler_proactive_item SET event_date=?2,yearly=?3,calendar_type=?4,lead_days=?5,next_check_at=?6
                        WHERE id=?1 AND (event_date IS NOT ?2 OR yearly!=?3 OR calendar_type!=?4 OR lead_days!=?5)",
                        params![id,date,t.yearly,t.calendar,t.lead,next.to_string()])?;
                    if changed>0 {topics::reset_schedule(&tx,id)?;}
                }
            },
            "done"|"snooze"=>{
                let delivery=delivery_id.context("请在本次提醒上操作")?;
                let count=if action=="snooze" {tx.execute("UPDATE butler_proactive_delivery_queue SET status='queued',deliver_after=?3
                    WHERE id=?1 AND item_id=?2 AND status='delivered'",params![delivery,id,(Local::now()+chrono::Duration::days(1)).timestamp()])?}
                else {tx.execute("UPDATE butler_proactive_delivery_queue SET status='acknowledged' WHERE id=?1 AND item_id=?2 AND status='delivered'",params![delivery,id])?};
                ensure!(count==1,"这次提醒已处理");
            },
            _=>anyhow::bail!("未知的事项操作")
        }
        tx.commit()?;
        crate::sync::schedule_sync_after_local_change(&app);
        app.emit("butler_proactive_changed",())?;
        Ok(()) })().map_err(error)
}
#[tauri::command]
pub async fn undo_butler_proactive_memory(
    app: AppHandle,
    id: i64,
    revision: i64,
) -> Result<(), String> {
    (|| -> Result<()> {
        let mut conn = connection(&app)?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        memory::undo(&tx, id, revision)?;
        tx.commit()?;
        crate::sync::schedule_sync_after_local_change(&app);
        app.emit("butler_memory_changed", ())?;
        Ok(())
    })()
    .map_err(error)
}

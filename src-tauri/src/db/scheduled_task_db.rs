use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Result, Row};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};

use crate::scheduler::expr::{self, Schedule};
use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};

use super::{get_db_path, get_db_write_lock};

/// 投递到执行助手与用户的管家私聊（总管家即主会话），每次投递时解析，私聊重开后自动跟随。
pub const DELIVER_OWNER_CHAT: &str = "owner_chat";
/// 投递到一个固定的普通会话（`deliver_conversation_id`）。
pub const DELIVER_CONVERSATION: &str = "conversation";
/// 运行会话本身出现在聊天列表里（不经管家的普通任务）。
pub const DELIVER_RUN_CONVERSATION: &str = "run_conversation";

pub const RUN_RUNNING: &str = "running";
pub const RUN_DELIVERED: &str = "delivered";
pub const RUN_QUIET: &str = "quiet";
pub const RUN_FAILED: &str = "failed";
pub const RUN_SKIPPED: &str = "skipped";
pub const RUN_MISSED: &str = "missed";
pub const RUN_CANCELLED: &str = "cancelled";
/// 旧版本的成功记录（是否通知看 `notify`）。
pub const RUN_LEGACY_SUCCESS: &str = "success";

const RUN_STATUSES: [&str; 8] = [
    RUN_RUNNING, RUN_LEGACY_SUCCESS, RUN_FAILED, RUN_MISSED, RUN_DELIVERED, RUN_QUIET, RUN_SKIPPED, RUN_CANCELLED,
];

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScheduledTask {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
    /// 规范化的时间表达，见 [`crate::scheduler::expr`]。
    pub schedule_expr: String,
    /// 间隔类计划的起点。
    pub schedule_anchor: DateTime<Utc>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    /// 执行助手。
    pub assistant_id: i64,
    pub task_prompt: String,
    /// always | if_needed | never
    pub deliver_policy: String,
    /// owner_chat | conversation | run_conversation
    pub deliver_to: String,
    pub deliver_conversation_id: Option<i64>,
    pub created_in_conversation_id: Option<i64>,
    /// 在管家模式下创建：运行时使用管家模式的工具基线。
    pub butler_mode: bool,
    pub ends_at: Option<DateTime<Utc>>,
    pub max_runs: Option<i64>,
    pub run_count: i64,
    /// run_once | skip
    pub missed_policy: String,
    pub consecutive_failures: i64,
    pub paused_reason: Option<String>,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
}

impl ScheduledTask {
    pub fn schedule(&self) -> std::result::Result<Schedule, String> {
        expr::parse(&self.schedule_expr, self.schedule_anchor)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScheduledTaskLog {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub message_type: String,
    pub content: String,
    pub created_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ScheduledTaskRun {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub status: String,
    /// 是否已投递给用户。
    pub notify: bool,
    /// 产出的简短预览。
    pub summary: Option<String>,
    /// 失败、跳过、错过的具体原因。
    pub error_message: Option<String>,
    pub started_time: DateTime<Utc>,
    pub finished_time: Option<DateTime<Utc>>,
    /// 这次运行对应的计划时间。
    pub scheduled_for: Option<DateTime<Utc>>,
    /// 运行会话（隐藏，可只读查看过程）。
    pub conversation_id: Option<i64>,
    /// 运行会话里的最终答复。
    pub final_message_id: Option<i64>,
    pub delivered_message_id: Option<i64>,
    pub delivery_conversation_id: Option<i64>,
    pub delivery_error: Option<String>,
}

impl ScheduledTaskRun {
    #[cfg(test)]
    pub fn new(task_id: i64, run_id: impl Into<String>, status: &str, started_time: DateTime<Utc>) -> Self {
        Self {
            id: 0,
            task_id,
            run_id: run_id.into(),
            status: status.to_string(),
            notify: false,
            summary: None,
            error_message: None,
            started_time,
            finished_time: None,
            scheduled_for: None,
            conversation_id: None,
            final_message_id: None,
            delivered_message_id: None,
            delivery_conversation_id: None,
            delivery_error: None,
        }
    }
}

const TASK_COLUMNS: &str = "id, name, is_enabled, COALESCE(schedule_expr, ''), COALESCE(schedule_anchor, created_time), next_run_at, last_run_at,
    assistant_id, task_prompt, COALESCE(deliver_policy, 'always'), COALESCE(deliver_to, 'owner_chat'), deliver_conversation_id,
    created_in_conversation_id, COALESCE(butler_mode, 0), ends_at, max_runs, COALESCE(run_count, 0), COALESCE(missed_policy, 'run_once'),
    COALESCE(consecutive_failures, 0), paused_reason, created_time, updated_time";

fn scheduled_task_from_row(row: &Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        is_enabled: row.get(2)?,
        schedule_expr: row.get(3)?,
        schedule_anchor: get_required_datetime_from_row(row, 4, "schedule_anchor")?,
        next_run_at: get_datetime_from_row(row, 5)?,
        last_run_at: get_datetime_from_row(row, 6)?,
        assistant_id: row.get(7)?,
        task_prompt: row.get(8)?,
        deliver_policy: row.get(9)?,
        deliver_to: row.get(10)?,
        deliver_conversation_id: row.get(11)?,
        created_in_conversation_id: row.get(12)?,
        butler_mode: row.get(13)?,
        ends_at: get_datetime_from_row(row, 14)?,
        max_runs: row.get(15)?,
        run_count: row.get(16)?,
        missed_policy: row.get(17)?,
        consecutive_failures: row.get(18)?,
        paused_reason: row.get(19)?,
        created_time: get_required_datetime_from_row(row, 20, "created_time")?,
        updated_time: get_required_datetime_from_row(row, 21, "updated_time")?,
    })
}

const RUN_COLUMNS: &str = "id, task_id, run_id, status, notify, summary, error_message, started_time, finished_time,
    scheduled_for, conversation_id, final_message_id, delivered_message_id, delivery_conversation_id, delivery_error";

fn scheduled_task_run_from_row(row: &Row) -> rusqlite::Result<ScheduledTaskRun> {
    Ok(ScheduledTaskRun {
        id: row.get(0)?,
        task_id: row.get(1)?,
        run_id: row.get(2)?,
        status: row.get(3)?,
        notify: row.get(4)?,
        summary: row.get(5)?,
        error_message: row.get(6)?,
        started_time: get_required_datetime_from_row(row, 7, "started_time")?,
        finished_time: get_datetime_from_row(row, 8)?,
        scheduled_for: get_datetime_from_row(row, 9)?,
        conversation_id: row.get(10)?,
        final_message_id: row.get(11)?,
        delivered_message_id: row.get(12)?,
        delivery_conversation_id: row.get(13)?,
        delivery_error: row.get(14)?,
    })
}

/// 到点后登记的一次运行。
#[derive(Debug, Clone)]
pub struct OccurrenceClaim {
    pub run_id: String,
    pub scheduled_for: DateTime<Utc>,
    /// 这次之后的下一次时间；`None` 表示任务结束。
    pub next_run_at: Option<DateTime<Utc>>,
    /// 更早错过、只记录不补跑的时段。
    pub missed: Vec<DateTime<Utc>>,
    /// 这次本身也按错过记录、不执行（任务设置为错过不补跑）。
    pub main_missed: bool,
    /// 不执行、直接记为跳过的原因（例如定时任务已关闭）。
    pub skip_reason: Option<String>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    /// 已登记为 running，应开始执行。
    Execute,
    /// 已登记为跳过（附原因），不执行。
    Skipped(String),
    /// 只记录为错过，不执行。
    Missed,
    /// 任务已变化（被编辑、暂停、删除或已被领取），什么都没做。
    Stale,
}

/// 一次运行的结束结果，与投递信息一起入库。
#[derive(Debug, Clone, Default)]
pub struct RunFinish {
    pub status: String,
    pub notify: bool,
    pub summary: Option<String>,
    pub error_message: Option<String>,
    pub finished_time: Option<DateTime<Utc>>,
    pub conversation_id: Option<i64>,
    pub final_message_id: Option<i64>,
    pub delivered_message_id: Option<i64>,
    pub delivery_conversation_id: Option<i64>,
    pub delivery_error: Option<String>,
}

/// 是否计入连续失败：执行失败与配置类跳过需要修复，重叠/开关关闭/错过不算。
pub fn counts_as_failure(status: &str, reason: Option<&str>) -> bool {
    match status {
        RUN_FAILED => true,
        RUN_SKIPPED => !reason.is_some_and(|r| r.starts_with(SKIP_OVERLAP) || r.starts_with(SKIP_SWITCH_OFF)),
        _ => false,
    }
}

pub const SKIP_OVERLAP: &str = "上一次运行尚未结束";
pub const SKIP_SWITCH_OFF: &str = "定时任务已关闭";
pub const MISSED_REASON: &str = "应用未运行，错过了这次计划时间";

pub struct ScheduledTaskDatabase {
    pub conn: Connection,
    write_lock: Arc<Mutex<()>>,
}

impl ScheduledTaskDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "conversation.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "conversation.db").unwrap();
        let conn = Connection::open(&db_path)?;
        let write_lock = get_db_write_lock(&db_path);
        Ok(ScheduledTaskDatabase { conn, write_lock })
    }

    #[cfg(test)]
    pub fn from_connection(conn: Connection) -> Self {
        Self { conn, write_lock: Arc::new(Mutex::new(())) }
    }

    fn with_write_lock<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        let conn = &self.conn;
        // 旧的结构化时间列只为迁移保留；新代码只读写 schedule_expr。
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scheduled_task (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                is_enabled BOOLEAN NOT NULL DEFAULT 1,
                schedule_type TEXT NOT NULL CHECK(schedule_type IN ('once', 'interval')),
                interval_value INTEGER,
                interval_unit TEXT,
                start_time TEXT,
                week_days TEXT,
                month_days TEXT,
                run_at DATETIME,
                next_run_at DATETIME,
                last_run_at DATETIME,
                assistant_id INTEGER NOT NULL,
                task_prompt TEXT NOT NULL,
                notify_prompt TEXT NOT NULL,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_time DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        let columns = table_columns(conn, "scheduled_task")?;
        for (column, definition) in [
            ("start_time", "TEXT"),
            ("week_days", "TEXT"),
            ("month_days", "TEXT"),
            ("butler_conversation_id", "INTEGER"),
            ("schedule_expr", "TEXT"),
            ("schedule_anchor", "DATETIME"),
            ("deliver_policy", "TEXT"),
            ("deliver_to", "TEXT"),
            ("deliver_conversation_id", "INTEGER"),
            ("created_in_conversation_id", "INTEGER"),
            ("butler_mode", "BOOLEAN NOT NULL DEFAULT 0"),
            ("ends_at", "DATETIME"),
            ("max_runs", "INTEGER"),
            ("run_count", "INTEGER NOT NULL DEFAULT 0"),
            ("missed_policy", "TEXT"),
            ("consecutive_failures", "INTEGER NOT NULL DEFAULT 0"),
            ("paused_reason", "TEXT"),
        ] {
            if !columns.iter().any(|c| c == column) {
                conn.execute(&format!("ALTER TABLE scheduled_task ADD COLUMN {column} {definition}"), [])?;
            }
        }

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_scheduled_task_enabled_next_run ON scheduled_task(is_enabled, next_run_at)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_scheduled_task_assistant ON scheduled_task(assistant_id)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS scheduled_task_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                run_id TEXT NOT NULL,
                message_type TEXT NOT NULL,
                content TEXT NOT NULL,
                created_time DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_scheduled_task_log_task_time ON scheduled_task_log(task_id, created_time)",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scheduled_task_run (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id INTEGER NOT NULL,
                run_id TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed', 'missed', 'delivered', 'quiet', 'skipped', 'cancelled')),
                notify BOOLEAN NOT NULL DEFAULT 0,
                summary TEXT,
                error_message TEXT,
                started_time DATETIME DEFAULT CURRENT_TIMESTAMP,
                finished_time DATETIME
            )",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_task_run_run_id ON scheduled_task_run(run_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_scheduled_task_run_task_time ON scheduled_task_run(task_id, started_time)",
            [],
        )?;
        let run_columns = table_columns(conn, "scheduled_task_run")?;
        for (column, definition) in [
            // 旧版本管家回报状态，保留列以兼容历史数据。
            ("butler_followup_status", "TEXT"),
            ("conversation_id", "INTEGER"),
            ("final_message_id", "INTEGER"),
            ("scheduled_for", "DATETIME"),
            ("delivered_message_id", "INTEGER"),
            ("delivery_conversation_id", "INTEGER"),
            ("delivery_error", "TEXT"),
        ] {
            if !run_columns.iter().any(|c| c == column) {
                conn.execute(&format!("ALTER TABLE scheduled_task_run ADD COLUMN {column} {definition}"), [])?;
            }
        }

        self.with_write_lock(|| migrate_scheduled_run_statuses(conn))?;

        // Keep the full outcome separate from the public run/list response. Recovery
        // needs details that do not fit the summary.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scheduled_task_run_outcome (
                run_id TEXT PRIMARY KEY NOT NULL,
                outcome_json TEXT NOT NULL
            )",
            [],
        )?;
        // Explicit cleanup also works for connections with foreign_keys disabled,
        // including the existing scheduled-task and remote deletion paths.
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS scheduled_task_run_delete_outcome
             AFTER DELETE ON scheduled_task_run
             BEGIN
                 DELETE FROM scheduled_task_run_outcome WHERE run_id = OLD.run_id;
             END;",
        )?;
        // 管家后台检查合同已由运行时审查取代。
        conn.execute("DROP TABLE IF EXISTS butler_check_policy", [])?;

        self.with_write_lock(|| migrate_legacy_schedules(conn, Utc::now()))?;

        debug!("Scheduled task tables ensured");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn list_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM scheduled_task ORDER BY created_time DESC"
        ))?;
        let rows = stmt.query_map([], scheduled_task_from_row)?;
        rows.collect()
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_task(&self, id: i64) -> Result<Option<ScheduledTask>> {
        self.conn
            .query_row(&format!("SELECT {TASK_COLUMNS} FROM scheduled_task WHERE id = ?"), [id], scheduled_task_from_row)
            .optional()
    }

    #[instrument(level = "debug", skip(self, task), fields(name = %task.name))]
    pub fn create_task(&self, task: &ScheduledTask) -> Result<ScheduledTask> {
        self.with_write_lock(|| {
            self.conn.execute(
                "INSERT INTO scheduled_task (name, is_enabled, schedule_type, schedule_expr, schedule_anchor, next_run_at, last_run_at,
                    assistant_id, task_prompt, notify_prompt, deliver_policy, deliver_to, deliver_conversation_id,
                    created_in_conversation_id, butler_mode, ends_at, max_runs, run_count, missed_policy, consecutive_failures,
                    paused_reason, created_time, updated_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '', ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    task.name,
                    task.is_enabled,
                    legacy_schedule_type(&task.schedule_expr),
                    task.schedule_expr,
                    task.schedule_anchor,
                    task.next_run_at,
                    task.last_run_at,
                    task.assistant_id,
                    task.task_prompt,
                    task.deliver_policy,
                    task.deliver_to,
                    task.deliver_conversation_id,
                    task.created_in_conversation_id,
                    task.butler_mode,
                    task.ends_at,
                    task.max_runs,
                    task.run_count,
                    task.missed_policy,
                    task.consecutive_failures,
                    task.paused_reason,
                    task.created_time,
                    task.updated_time
                ],
            )?;
            let id = self.conn.last_insert_rowid();
            Ok(ScheduledTask { id, ..task.clone() })
        })
    }

    /// 保存用户可编辑的设置与调度状态；`run_count` 与连续失败计数由运行流程维护。
    #[instrument(level = "debug", skip(self, task), fields(id = task.id))]
    pub fn update_task(&self, task: &ScheduledTask) -> Result<()> {
        self.with_write_lock(|| {
            let changed = self.conn.execute(
                "UPDATE scheduled_task SET name = ?1, is_enabled = ?2, schedule_type = ?3, schedule_expr = ?4, schedule_anchor = ?5,
                    next_run_at = ?6, assistant_id = ?7, task_prompt = ?8, deliver_policy = ?9, deliver_to = ?10,
                    deliver_conversation_id = ?11, ends_at = ?12, max_runs = ?13, missed_policy = ?14, paused_reason = ?15,
                    consecutive_failures = ?16, updated_time = ?17
                 WHERE id = ?18",
                params![
                    task.name,
                    task.is_enabled,
                    legacy_schedule_type(&task.schedule_expr),
                    task.schedule_expr,
                    task.schedule_anchor,
                    task.next_run_at,
                    task.assistant_id,
                    task.task_prompt,
                    task.deliver_policy,
                    task.deliver_to,
                    task.deliver_conversation_id,
                    task.ends_at,
                    task.max_runs,
                    task.missed_policy,
                    task.paused_reason,
                    task.consecutive_failures,
                    task.updated_time,
                    task.id
                ],
            )?;
            if changed == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })
    }

    /// 暂停任务并记录原因（自动暂停、投递会话被删除、执行助手被删除等）。
    pub fn pause_task(&self, id: i64, reason: &str, now: DateTime<Utc>) -> Result<bool> {
        self.with_write_lock(|| {
            Ok(self.conn.execute(
                "UPDATE scheduled_task SET is_enabled = 0, paused_reason = ?1, updated_time = ?2 WHERE id = ?3",
                params![reason, now, id],
            )? > 0)
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_approved_task(&self, id: i64, expected: &serde_json::Value) -> Result<()> {
        self.with_write_lock(|| {
            let tx = self.conn.unchecked_transaction()?;
            // Acquire the write reservation before reading the approval snapshot.
            tx.execute("UPDATE scheduled_task SET id=id WHERE id=?1", [id])?;
            let task = self.read_task(id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            let current = serde_json::to_value(task).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            if &current != expected {
                return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other("定时任务已变化，请重新批准"))));
            }
            super::cascade_delete::delete(&tx, "scheduled_task", id)?;
            tx.commit()
        })
    }

    pub fn delete_task(&self, id: i64) -> Result<()> {
        self.with_write_lock(|| super::cascade_delete::delete(&self.conn, "scheduled_task", id))
    }

    #[instrument(level = "debug", skip(self, now), fields(now = %now))]
    pub fn list_due_tasks(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM scheduled_task
             WHERE is_enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?
             ORDER BY next_run_at ASC"
        ))?;
        let rows = stmt.query_map([now], scheduled_task_from_row)?;
        rows.collect()
    }

    /// 到点登记：在同一个事务里确认任务仍在这个时点到期、写入运行记录（含更早错过的时段）
    /// 并推进下一次时间。之后的任何检查失败都只会结束这条记录，不会让同一时段被反复处理。
    pub fn claim_occurrence(&mut self, task_id: i64, expected_next: DateTime<Utc>, claim: &OccurrenceClaim) -> Result<ClaimResult> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: Option<(bool, Option<DateTime<Utc>>)> = tx
            .query_row("SELECT is_enabled, next_run_at FROM scheduled_task WHERE id = ?1", [task_id], |row| {
                Ok((row.get(0)?, get_datetime_from_row(row, 1)?))
            })
            .optional()?;
        if current != Some((true, Some(expected_next))) {
            return Ok(ClaimResult::Stale);
        }
        let overlapping: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM scheduled_task_run WHERE task_id = ?1 AND status = 'running')",
            [task_id],
            |row| row.get(0),
        )?;
        let skip_reason = claim.skip_reason.clone().or_else(|| overlapping.then(|| SKIP_OVERLAP.to_string()));
        for slot in &claim.missed {
            tx.execute(
                "INSERT OR IGNORE INTO scheduled_task_run (task_id, run_id, status, notify, error_message, started_time, finished_time, scheduled_for)
                 VALUES (?1, ?2, 'missed', 0, ?3, ?4, ?4, ?5)",
                params![task_id, format!("missed:{task_id}:{}", slot.to_rfc3339()), MISSED_REASON, claim.now, slot],
            )?;
        }
        let (status, reason, finished) = if claim.main_missed {
            (RUN_MISSED, Some(MISSED_REASON.to_string()), Some(claim.now))
        } else if let Some(reason) = &skip_reason {
            (RUN_SKIPPED, Some(reason.clone()), Some(claim.now))
        } else {
            (RUN_RUNNING, None, None)
        };
        tx.execute(
            "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, error_message, started_time, finished_time, scheduled_for)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
            params![task_id, claim.run_id, status, reason, claim.now, finished, claim.scheduled_for],
        )?;
        let executes = status == RUN_RUNNING;
        tx.execute(
            "UPDATE scheduled_task SET next_run_at = ?1, is_enabled = ?2,
                run_count = COALESCE(run_count, 0) + ?3,
                last_run_at = CASE WHEN ?3 = 1 THEN ?4 ELSE last_run_at END,
                paused_reason = CASE WHEN ?2 = 0 THEN COALESCE(paused_reason, '已完成') ELSE paused_reason END
             WHERE id = ?5",
            params![claim.next_run_at, claim.next_run_at.is_some(), i64::from(executes), claim.now, task_id],
        )?;
        tx.commit()?;
        Ok(if claim.main_missed {
            ClaimResult::Missed
        } else {
            match skip_reason {
                Some(reason) => ClaimResult::Skipped(reason),
                None => ClaimResult::Execute,
            }
        })
    }

    /// 手动「立即运行」：登记运行但不改变计划时间。
    pub fn claim_manual_run(&mut self, task_id: i64, run_id: &str, now: DateTime<Utc>) -> Result<ClaimResult> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM scheduled_task WHERE id = ?1)", [task_id], |r| r.get(0))?;
        if !exists {
            return Ok(ClaimResult::Stale);
        }
        let overlapping: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM scheduled_task_run WHERE task_id = ?1 AND status = 'running')",
            [task_id],
            |row| row.get(0),
        )?;
        if overlapping {
            return Ok(ClaimResult::Skipped(SKIP_OVERLAP.to_string()));
        }
        tx.execute(
            "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, started_time, scheduled_for) VALUES (?1, ?2, 'running', 0, ?3, ?3)",
            params![task_id, run_id, now],
        )?;
        tx.execute(
            "UPDATE scheduled_task SET run_count = COALESCE(run_count, 0) + 1, last_run_at = ?1 WHERE id = ?2",
            params![now, task_id],
        )?;
        tx.commit()?;
        Ok(ClaimResult::Execute)
    }

    #[instrument(level = "debug", skip(self, log), fields(task_id = log.task_id))]
    pub fn add_log(&self, log: &ScheduledTaskLog) -> Result<ScheduledTaskLog> {
        self.with_write_lock(|| {
            let inserted = self.conn.execute(
                "INSERT INTO scheduled_task_log (task_id, run_id, message_type, content, created_time)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS(SELECT 1 FROM scheduled_task WHERE id=?1)",
                params![log.task_id, log.run_id, log.message_type, log.content, log.created_time],
            )?;
            if inserted == 0 {
                return Err(rusqlite::Error::InvalidParameterName(format!("定时任务 {} 已删除，拒绝写入日志 run={}", log.task_id, log.run_id)));
            }
            let id = self.conn.last_insert_rowid();
            Ok(ScheduledTaskLog { id, ..log.clone() })
        })
    }

    #[instrument(level = "debug", skip(self), fields(task_id, limit))]
    pub fn list_logs_by_task(&self, task_id: i64, limit: u32) -> Result<Vec<ScheduledTaskLog>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, run_id, message_type, content, created_time
             FROM scheduled_task_log
             WHERE task_id = ?
             ORDER BY created_time DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![task_id, limit], log_from_row)?;
        let mut logs: Vec<ScheduledTaskLog> = rows.collect::<Result<Vec<_>>>()?;
        logs.reverse();
        Ok(logs)
    }

    #[instrument(level = "debug", skip(self), fields(task_id))]
    pub fn list_logs_by_run(&self, task_id: i64, run_id: &str, limit: u32) -> Result<Vec<ScheduledTaskLog>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, run_id, message_type, content, created_time
             FROM scheduled_task_log
             WHERE task_id = ? AND run_id = ?
             ORDER BY created_time DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![task_id, run_id, limit], log_from_row)?;
        let mut logs: Vec<ScheduledTaskLog> = rows.collect::<Result<Vec<_>>>()?;
        logs.reverse();
        Ok(logs)
    }

    #[instrument(level = "debug", skip(self), fields(task_id, limit))]
    pub fn list_runs_by_task(&self, task_id: i64, limit: u32) -> Result<Vec<ScheduledTaskRun>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM scheduled_task_run WHERE task_id = ? ORDER BY started_time DESC, id DESC LIMIT ?"
        ))?;
        let rows = stmt.query_map(params![task_id, limit], scheduled_task_run_from_row)?;
        rows.collect()
    }

    /// 每个任务最近一次运行（列表展示上次结果）。
    pub fn latest_runs(&self) -> Result<std::collections::HashMap<i64, ScheduledTaskRun>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM scheduled_task_run r
             WHERE r.id = (SELECT r2.id FROM scheduled_task_run r2 WHERE r2.task_id = r.task_id ORDER BY r2.started_time DESC, r2.id DESC LIMIT 1)"
        ))?;
        let rows = stmt.query_map([], scheduled_task_run_from_row)?;
        rows.map(|run| run.map(|run| (run.task_id, run))).collect()
    }

    #[instrument(level = "debug", skip(self), fields(run_id))]
    pub fn read_run(&self, run_id: &str) -> Result<Option<ScheduledTaskRun>> {
        self.conn
            .query_row(&format!("SELECT {RUN_COLUMNS} FROM scheduled_task_run WHERE run_id = ?"), [run_id], scheduled_task_run_from_row)
            .optional()
    }

    #[cfg(test)]
    pub fn create_run(&self, run: &ScheduledTaskRun) -> Result<ScheduledTaskRun> {
        self.with_write_lock(|| {
            let inserted = self.conn.execute(
                "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, summary, error_message, started_time, finished_time, scheduled_for)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9 WHERE EXISTS(SELECT 1 FROM scheduled_task WHERE id=?1)",
                params![
                    run.task_id,
                    run.run_id,
                    run.status,
                    run.notify,
                    run.summary,
                    run.error_message,
                    run.started_time,
                    run.finished_time,
                    run.scheduled_for
                ],
            )?;
            if inserted == 0 {
                return Err(rusqlite::Error::InvalidParameterName(format!("定时任务 {} 已删除，拒绝创建运行记录 run={}", run.task_id, run.run_id)));
            }
            let id = self.conn.last_insert_rowid();
            Ok(ScheduledTaskRun { id, ..run.clone() })
        })
    }

    /// 写入运行的结束状态、完整结果与投递信息，并在同一事务里维护任务的连续失败计数。
    /// 返回更新后的连续失败次数。
    #[instrument(level = "debug", skip(self, finish, outcome_json), fields(run_id, status = %finish.status))]
    pub fn save_run_outcome(&self, run_id: &str, finish: &RunFinish, outcome_json: &str) -> Result<i64> {
        self.with_write_lock(|| {
            let tx = self.conn.unchecked_transaction()?;
            let task_id: Option<i64> = tx
                .query_row(
                    "SELECT r.task_id FROM scheduled_task_run r JOIN scheduled_task t ON t.id = r.task_id WHERE r.run_id = ?1",
                    [run_id],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(task_id) = task_id else {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "运行记录或所属定时任务已删除，拒绝保存完整结果 run={run_id}"
                )));
            };
            tx.execute(
                "UPDATE scheduled_task_run
                 SET status = ?1, notify = ?2, summary = ?3, error_message = ?4, finished_time = ?5,
                     conversation_id = ?6, final_message_id = ?7, delivered_message_id = ?8,
                     delivery_conversation_id = ?9, delivery_error = ?10
                 WHERE run_id = ?11",
                params![
                    finish.status,
                    finish.notify,
                    finish.summary,
                    finish.error_message,
                    finish.finished_time,
                    finish.conversation_id,
                    finish.final_message_id,
                    finish.delivered_message_id,
                    finish.delivery_conversation_id,
                    finish.delivery_error,
                    run_id
                ],
            )?;
            tx.execute(
                "INSERT INTO scheduled_task_run_outcome (run_id, outcome_json) VALUES (?1, ?2)
                 ON CONFLICT(run_id) DO UPDATE SET outcome_json = excluded.outcome_json",
                params![run_id, outcome_json],
            )?;
            if counts_as_failure(&finish.status, finish.error_message.as_deref()) {
                tx.execute(
                    "UPDATE scheduled_task SET consecutive_failures = COALESCE(consecutive_failures, 0) + 1 WHERE id = ?1",
                    [task_id],
                )?;
            } else if matches!(finish.status.as_str(), RUN_DELIVERED | RUN_QUIET) {
                tx.execute("UPDATE scheduled_task SET consecutive_failures = 0 WHERE id = ?1", [task_id])?;
            }
            let failures: i64 = tx.query_row(
                "SELECT COALESCE(consecutive_failures, 0) FROM scheduled_task WHERE id = ?1",
                [task_id],
                |row| row.get(0),
            )?;
            tx.commit()?;
            Ok(failures)
        })
    }

    /// 运行开始时登记运行会话，界面可以在运行中打开查看过程。
    pub fn set_run_conversation(&self, run_id: &str, conversation_id: i64) -> Result<()> {
        self.with_write_lock(|| {
            self.conn.execute(
                "UPDATE scheduled_task_run SET conversation_id = ?1 WHERE run_id = ?2",
                params![conversation_id, run_id],
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(run_id))]
    pub fn read_run_outcome(&self, run_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT outcome_json FROM scheduled_task_run_outcome WHERE run_id = ?",
                [run_id],
                |row| row.get(0),
            )
            .optional()
    }

    /// 启动时收尾上一个进程遗留的 running 记录：标为失败并返回被收尾的记录（状态已更新）。
    /// `skip_run_ids` 为本进程仍在执行的 run，不会被改写。
    pub fn mark_interrupted_runs(
        &self,
        skip_run_ids: &[String],
        error_message: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<ScheduledTaskRun>> {
        self.with_write_lock(|| mark_interrupted_runs_in_connection(&self.conn, skip_run_ids, error_message, now))
    }
}

fn log_from_row(row: &Row) -> rusqlite::Result<ScheduledTaskLog> {
    Ok(ScheduledTaskLog {
        id: row.get(0)?,
        task_id: row.get(1)?,
        run_id: row.get(2)?,
        message_type: row.get(3)?,
        content: row.get(4)?,
        created_time: get_required_datetime_from_row(row, 5, "created_time")?,
    })
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    conn.prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect()
}

/// 旧列 `schedule_type` 仍有 NOT NULL/CHECK 约束，按新表达填一个兼容值。
fn legacy_schedule_type(schedule_expr: &str) -> &'static str {
    match expr::parse(schedule_expr, Utc::now()) {
        Ok(Schedule::Once(_)) => "once",
        _ => "interval",
    }
}

pub(crate) fn mark_interrupted_runs_in_connection(
    conn: &Connection,
    skip_run_ids: &[String],
    error_message: &str,
    now: DateTime<Utc>,
) -> Result<Vec<ScheduledTaskRun>> {
    let tx = conn.unchecked_transaction()?;
    let candidates: Vec<ScheduledTaskRun> = {
        let mut stmt = tx.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM scheduled_task_run WHERE status = 'running' AND finished_time IS NULL"
        ))?;
        let rows = stmt.query_map([], scheduled_task_run_from_row)?;
        rows.collect::<Result<Vec<_>>>()?
    };
    let mut interrupted = Vec::new();
    for run in candidates {
        if skip_run_ids.iter().any(|id| id == &run.run_id) {
            continue;
        }
        let message = format!(
            "{error_message}（run_id={}，开始于 {}）",
            run.run_id,
            run.started_time.to_rfc3339()
        );
        let changed = tx.execute(
            "UPDATE scheduled_task_run SET status = 'failed', error_message = ?1, finished_time = ?2
             WHERE run_id = ?3 AND status = 'running'",
            params![message, now, run.run_id],
        )?;
        if changed == 1 {
            interrupted.push(ScheduledTaskRun {
                status: RUN_FAILED.to_string(),
                error_message: Some(message),
                finished_time: Some(now),
                ..run
            });
        }
    }
    tx.commit()?;
    Ok(interrupted)
}

// ── 旧数据迁移 ──────────────────────────────────────────────────────────

/// 旧结构化时间配置（schedule_type + interval_* + start_time + week_days + month_days + run_at）。
#[derive(Debug, Clone, Default)]
pub(crate) struct LegacySchedule {
    pub schedule_type: String,
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>,
    pub start_time: Option<String>,
    pub week_days: Option<String>,
    pub month_days: Option<String>,
    pub run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
}

/// 旧配置转换结果：新表达、间隔类的锚点，以及无法精确表达时需要用户确认的说明。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConvertedSchedule {
    pub expr: String,
    pub anchor: DateTime<Utc>,
    pub needs_review: Option<String>,
}

fn json_days(text: &Option<String>) -> Vec<u32> {
    let mut days: Vec<u32> = text
        .as_deref()
        .and_then(|s| serde_json::from_str::<Vec<i64>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|d| u32::try_from(d).ok())
        .collect();
    days.sort_unstable();
    days.dedup();
    days
}

fn join_days(days: &[u32]) -> String {
    days.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

pub(crate) fn convert_legacy_schedule<Tz: chrono::TimeZone>(legacy: &LegacySchedule, now: DateTime<Utc>, tz: &Tz) -> std::result::Result<ConvertedSchedule, String>
where
    Tz::Offset: std::fmt::Display,
{
    use chrono::{Datelike, Timelike};
    let reference = legacy.next_run_at.unwrap_or(now);
    let local_ref = reference.with_timezone(tz);
    if legacy.schedule_type == "once" {
        let at = legacy.run_at.or(legacy.next_run_at).ok_or("一次性任务缺少执行时间")?;
        return Ok(ConvertedSchedule { expr: Schedule::Once(at).to_expr_in(tz), anchor: at, needs_review: None });
    }
    let value = legacy.interval_value.filter(|v| *v > 0).ok_or("间隔任务缺少间隔数值")?;
    let unit = legacy.interval_unit.as_deref().ok_or("间隔任务缺少间隔单位")?;
    let (hour, minute) = legacy
        .start_time
        .as_deref()
        .and_then(|s| s.split_once(':'))
        .and_then(|(h, m)| Some((h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?)))
        .filter(|(h, m)| *h < 24 && *m < 60)
        .unwrap_or((local_ref.hour(), local_ref.minute()));
    let n = value as u32;
    let every = |suffix: &str, step: chrono::Duration| ConvertedSchedule {
        expr: format!("every {n}{suffix}"),
        anchor: reference - step,
        needs_review: None,
    };
    let converted = match unit {
        "minute" => every("m", chrono::Duration::minutes(value)),
        "hour" => every("h", chrono::Duration::hours(value)),
        "day" if n == 1 => ConvertedSchedule { expr: format!("{minute} {hour} * * *"), anchor: reference, needs_review: None },
        "day" => ConvertedSchedule { expr: format!("every {n}d at {hour:02}:{minute:02}"), anchor: reference, needs_review: None },
        "week" => {
            let mut days = json_days(&legacy.week_days);
            days.retain(|d| *d <= 6);
            if days.is_empty() {
                days.push(local_ref.weekday().num_days_from_sunday());
            }
            if n == 1 {
                ConvertedSchedule { expr: format!("{minute} {hour} * * {}", join_days(&days)), anchor: reference, needs_review: None }
            } else if days.len() == 1 {
                ConvertedSchedule { expr: format!("every {n}w at {hour:02}:{minute:02}"), anchor: reference, needs_review: None }
            } else {
                ConvertedSchedule {
                    expr: format!("{minute} {hour} * * {}", join_days(&days)),
                    anchor: reference,
                    needs_review: Some(format!("原来是每 {n} 周的多个星期几，新版无法精确表达，已改为每周；请确认时间后再启用")),
                }
            }
        }
        "month" => {
            let mut days = json_days(&legacy.month_days);
            days.retain(|d| (1..=31).contains(d));
            if days.is_empty() {
                days.push(local_ref.day());
            }
            ConvertedSchedule {
                expr: format!("{minute} {hour} {} * *", join_days(&days)),
                anchor: reference,
                needs_review: (n != 1).then(|| format!("原来是每 {n} 个月，新版无法精确表达，已改为每月；请确认时间后再启用")),
            }
        }
        other => return Err(format!("未知的间隔单位 {other}")),
    };
    expr::parse_in(&converted.expr, now, tz).map_err(|e| format!("转换结果无效：{e}"))?;
    Ok(converted)
}

/// 把旧任务转换为新表达：管家任务投递到执行助手的私聊，普通任务保持「结果出现在聊天列表」；
/// 旧的通知规则并入指令，按需投递。迁移前错过的时段不补跑：按现在重新计算下一次时间。
fn migrate_legacy_schedules(conn: &Connection, now: DateTime<Utc>) -> Result<()> {
    let legacy: Vec<(i64, bool, LegacySchedule, String, String, Option<i64>)> = conn
        .prepare(
            "SELECT id, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at,
                    next_run_at, task_prompt, notify_prompt, butler_conversation_id
             FROM scheduled_task WHERE schedule_expr IS NULL OR schedule_expr = ''",
        )?
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                LegacySchedule {
                    schedule_type: row.get(2)?,
                    interval_value: row.get(3)?,
                    interval_unit: row.get(4)?,
                    start_time: row.get(5)?,
                    week_days: row.get(6)?,
                    month_days: row.get(7)?,
                    run_at: get_datetime_from_row(row, 8)?,
                    next_run_at: get_datetime_from_row(row, 9)?,
                },
                row.get(10)?,
                row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                row.get(12)?,
            ))
        })?
        .collect::<Result<_>>()?;
    if legacy.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for (id, enabled, schedule, prompt, notify_prompt, butler_conversation_id) in legacy {
        let (expr_text, anchor, review) = match convert_legacy_schedule(&schedule, now, &chrono::Local) {
            Ok(converted) => (converted.expr, converted.anchor, converted.needs_review),
            Err(error) => {
                warn!(task_id = id, error = %error, "legacy scheduled task cannot be converted; paused");
                ("0 9 * * *".to_string(), now, Some(format!("旧的时间设置无法转换（{error}），已暂停；请重新设置时间")))
            }
        };
        let next = expr::parse(&expr_text, anchor).ok().and_then(|s| s.next_after(now, anchor));
        let task_prompt = if notify_prompt.trim().is_empty() {
            prompt
        } else {
            format!("{prompt}\n\n【什么时候需要告诉用户】{}", notify_prompt.trim())
        };
        let (deliver_to, butler_mode) = if butler_conversation_id.is_some() {
            (DELIVER_OWNER_CHAT, true)
        } else {
            (DELIVER_RUN_CONVERSATION, false)
        };
        let still_enabled = enabled && review.is_none() && next.is_some();
        let paused_reason = review.or_else(|| (enabled && next.is_none()).then(|| "已完成".to_string()));
        tx.execute(
            "UPDATE scheduled_task SET schedule_expr = ?1, schedule_anchor = ?2, next_run_at = ?3, task_prompt = ?4,
                deliver_policy = 'if_needed', deliver_to = ?5, butler_mode = ?6, missed_policy = 'run_once',
                is_enabled = ?7, paused_reason = ?8, run_count = COALESCE(run_count, 0), consecutive_failures = 0
             WHERE id = ?9",
            params![expr_text, anchor, next, task_prompt, deliver_to, butler_mode, still_enabled, paused_reason, id],
        )?;
    }
    tx.commit()
}

/// Rebuild only the known legacy status constraint, retaining the table's other schema,
/// columns, explicit indexes, triggers and AUTOINCREMENT high-water mark.
fn migrate_scheduled_run_statuses(conn: &Connection) -> Result<()> {
    let sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='scheduled_task_run'", [], |row| row.get(0),
    )?;
    let constraint = regex::Regex::new(r#"(?i)CHECK\s*\(\s*["`\[]?status["`\]]?\s+IN\s*\(([^)]*)\)\s*\)"#).expect("static regex");
    let Some(captures) = constraint.captures(&sql) else {
        // Unknown status constraints must not be silently removed or treated as migrated.
        if regex::Regex::new(r"(?i)CHECK\s*\([^)]*\bstatus\b").expect("static regex").is_match(&sql) {
            return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: unknown status CHECK; migration refused".into()));
        }
        return Ok(());
    };
    let statuses: Vec<String> = captures[1].split(',')
        .map(|s| s.trim().trim_matches('\'').to_ascii_lowercase()).collect();
    if RUN_STATUSES.iter().all(|required| statuses.iter().any(|s| s == required)) {
        return Ok(());
    }
    if !["running", "success", "failed"].iter().all(|required| statuses.iter().any(|s| s == *required))
        || statuses.iter().any(|s| !RUN_STATUSES.contains(&s.as_str()))
    {
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: unexpected status values; migration refused".into()));
    }
    if !regex::Regex::new(r"(?i)\bAUTOINCREMENT\b").expect("static regex").is_match(&sql) {
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: legacy schema without AUTOINCREMENT; migration refused".into()));
    }
    let table_prefix = regex::Regex::new(r#"(?i)^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:"scheduled_task_run"|`scheduled_task_run`|\[scheduled_task_run\]|scheduled_task_run)\s*\("#).expect("static regex");
    let target = RUN_STATUSES.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ");
    let expanded = constraint.replace(&sql, format!("CHECK(status IN ({target}))").as_str());
    if !table_prefix.is_match(&expanded) {
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: unsupported CREATE TABLE; migration refused".into()));
    }
    let new_sql = table_prefix.replace(&expanded, "CREATE TABLE scheduled_task_run_status_migration (").into_owned();

    conn.execute_batch("SAVEPOINT scheduled_run_status_migration")?;
    let result = (|| -> Result<()> {
        // Never drop a table referenced by foreign keys: DROP can cascade before rollback.
        let tables: Vec<String> = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
            .query_map([], |row| row.get(0))?.collect::<Result<_>>()?;
        for table in tables {
            let quoted = format!("\"{}\"", table.replace('"', "\"\""));
            let sql = format!("PRAGMA foreign_key_list({quoted})");
            let refs: Vec<String> = conn.prepare(&sql)?.query_map([], |row| row.get(2))?.collect::<Result<_>>()?;
            if refs.iter().any(|name| name.eq_ignore_ascii_case("scheduled_task_run")) {
                return Err(rusqlite::Error::InvalidParameterName(format!("scheduled_task_run is referenced by foreign keys in {table}; migration refused")));
            }
        }
        let objects: Vec<String> = conn.prepare(
            "SELECT sql FROM sqlite_master WHERE tbl_name='scheduled_task_run' AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name",
        )?.query_map([], |row| row.get(0))?.collect::<Result<_>>()?;
        let columns: Vec<String> = conn.prepare("PRAGMA table_xinfo(scheduled_task_run)")?
            .query_map([], |row| Ok((row.get::<_, String>(1)?, row.get::<_, i64>(6)?)))?
            .filter_map(|row| match row { Ok((name, 0)) => Some(Ok(format!("\"{}\"", name.replace('"', "\"\"")))), Ok(_) => None, Err(e) => Some(Err(e)) })
            .collect::<Result<_>>()?;
        let sequence: Option<i64> = conn.query_row("SELECT seq FROM sqlite_sequence WHERE name='scheduled_task_run'", [], |row| row.get(0)).optional()?;
        conn.execute_batch(&new_sql)?;
        let fields = columns.join(",");
        conn.execute_batch(&format!("INSERT INTO scheduled_task_run_status_migration ({fields}) SELECT {fields} FROM scheduled_task_run; DROP TABLE scheduled_task_run; ALTER TABLE scheduled_task_run_status_migration RENAME TO scheduled_task_run;"))?;
        for object in objects { conn.execute_batch(&object)?; }
        if let Some(sequence) = sequence {
            conn.execute("INSERT INTO sqlite_sequence(name, seq) SELECT 'scheduled_task_run', ?1 WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name='scheduled_task_run')", [sequence])?;
            conn.execute("UPDATE sqlite_sequence SET seq=MAX(seq, ?1) WHERE name='scheduled_task_run'", [sequence])?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("RELEASE scheduled_run_status_migration"),
        Err(error) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK TO scheduled_run_status_migration; RELEASE scheduled_run_status_migration") {
                return Err(rusqlite::Error::InvalidParameterName(format!("scheduled_task_run migration failed: {error}; rollback failed: {rollback}")));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
#[path = "scheduled_task_db_tests.rs"]
mod migration_tests;

#[cfg(test)]
#[path = "tests/scheduled_task_db_tests.rs"]
mod tests;

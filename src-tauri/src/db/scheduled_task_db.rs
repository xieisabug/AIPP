use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Result, Row};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};

use super::{get_db_path, get_db_write_lock};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScheduledTask {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String, // 'once' | 'interval'
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>, // 'minute' | 'hour' | 'day' | 'week' | 'month'
    pub start_time: Option<String>,    // HH:mm format for day/week/month schedules
    pub week_days: Option<String>,     // JSON array e.g. "[1,3,5]" for Mon/Wed/Fri
    pub month_days: Option<String>,    // JSON array e.g. "[1,15]" for 1st and 15th
    pub run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
    pub butler_conversation_id: Option<i64>,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScheduledTaskRun {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub status: String,
    pub notify: bool,
    pub summary: Option<String>,
    pub error_message: Option<String>,
    pub started_time: DateTime<Utc>,
    pub finished_time: Option<DateTime<Utc>>,
    pub butler_followup_status: Option<String>,
}

/// Column order: id, name, is_enabled, schedule_type, interval_value, interval_unit,
/// start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id,
/// task_prompt, notify_prompt, butler_conversation_id, created_time, updated_time
fn scheduled_task_from_row(row: &Row) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        is_enabled: row.get(2)?,
        schedule_type: row.get(3)?,
        interval_value: row.get(4)?,
        interval_unit: row.get(5)?,
        start_time: row.get(6)?,
        week_days: row.get(7)?,
        month_days: row.get(8)?,
        run_at: get_datetime_from_row(row, 9)?,
        next_run_at: get_datetime_from_row(row, 10)?,
        last_run_at: get_datetime_from_row(row, 11)?,
        assistant_id: row.get(12)?,
        task_prompt: row.get(13)?,
        notify_prompt: row.get(14)?,
        butler_conversation_id: row.get(15)?,
        created_time: get_required_datetime_from_row(row, 16, "created_time")?,
        updated_time: get_required_datetime_from_row(row, 17, "updated_time")?,
    })
}

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
        butler_followup_status: row.get(9)?,
    })
}

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

    fn with_write_lock<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let _guard = self.write_lock.lock().expect("scheduled task db write lock poisoned");
        f()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn create_tables(&self) -> rusqlite::Result<()> {
        let conn = &self.conn;
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

        // Migration: add new columns if they don't exist
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(scheduled_task)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>>>()?;
        if !columns.contains(&"start_time".to_string()) {
            conn.execute("ALTER TABLE scheduled_task ADD COLUMN start_time TEXT", [])?;
        }
        if !columns.contains(&"week_days".to_string()) {
            conn.execute("ALTER TABLE scheduled_task ADD COLUMN week_days TEXT", [])?;
        }
        if !columns.contains(&"month_days".to_string()) {
            conn.execute("ALTER TABLE scheduled_task ADD COLUMN month_days TEXT", [])?;
        }
        if !columns.contains(&"butler_conversation_id".to_string()) {
            conn.execute(
                "ALTER TABLE scheduled_task ADD COLUMN butler_conversation_id INTEGER",
                [],
            )?;
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
                status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed', 'missed')),
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

        // Migration: add butler_followup_status to scheduled_task_run
        let run_columns: Vec<String> = conn
            .prepare("PRAGMA table_info(scheduled_task_run)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>>>()?;
        if !run_columns.contains(&"butler_followup_status".to_string()) {
            conn.execute(
                "ALTER TABLE scheduled_task_run ADD COLUMN butler_followup_status TEXT",
                [],
            )?;
        }

        self.with_write_lock(|| migrate_scheduled_run_missed(conn))?;

        debug!("Scheduled task tables ensured");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn list_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, butler_conversation_id, created_time, updated_time
             FROM scheduled_task
             ORDER BY created_time DESC",
        )?;
        let rows = stmt.query_map([], |row| scheduled_task_from_row(row))?;
        let tasks: Vec<ScheduledTask> = rows.collect::<Result<Vec<_>>>()?;
        Ok(tasks)
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_task(&self, id: i64) -> Result<Option<ScheduledTask>> {
        let task = self
            .conn
            .query_row(
                "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, butler_conversation_id, created_time, updated_time
                 FROM scheduled_task WHERE id = ?",
                [id],
                |row| scheduled_task_from_row(row),
            )
            .optional()?;
        Ok(task)
    }

    #[instrument(level = "debug", skip(self, task), fields(name = %task.name))]
    pub fn create_task(&self, task: &ScheduledTask) -> Result<ScheduledTask> {
        self.with_write_lock(|| {
            self.conn.execute(
                "INSERT INTO scheduled_task (name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, butler_conversation_id, created_time, updated_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    task.name,
                    task.is_enabled,
                    task.schedule_type,
                    task.interval_value,
                    task.interval_unit,
                    task.start_time,
                    task.week_days,
                    task.month_days,
                    task.run_at,
                    task.next_run_at,
                    task.last_run_at,
                    task.assistant_id,
                    task.task_prompt,
                    task.notify_prompt,
                    task.butler_conversation_id,
                    task.created_time,
                    task.updated_time
                ],
            )?;
            let id = self.conn.last_insert_rowid();
            Ok(ScheduledTask { id, ..task.clone() })
        })
    }

    #[instrument(level = "debug", skip(self, task), fields(id = task.id))]
    pub fn update_task(&self, task: &ScheduledTask) -> Result<()> {
        self.with_write_lock(|| {
            self.conn.execute(
                "UPDATE scheduled_task SET name = ?1, is_enabled = ?2, schedule_type = ?3, interval_value = ?4, interval_unit = ?5, start_time = ?6, week_days = ?7, month_days = ?8, run_at = ?9, next_run_at = ?10, last_run_at = ?11, assistant_id = ?12, task_prompt = ?13, notify_prompt = ?14, butler_conversation_id = ?15, updated_time = ?16 WHERE id = ?17",
                params![
                    task.name,
                    task.is_enabled,
                    task.schedule_type,
                    task.interval_value,
                    task.interval_unit,
                    task.start_time,
                    task.week_days,
                    task.month_days,
                    task.run_at,
                    task.next_run_at,
                    task.last_run_at,
                    task.assistant_id,
                    task.task_prompt,
                    task.notify_prompt,
                    task.butler_conversation_id,
                    task.updated_time,
                    task.id
                ],
            )?;
            Ok(())
        })
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_task(&self, id: i64) -> Result<()> {
        self.with_write_lock(|| super::cascade_delete::delete(&self.conn, "scheduled_task", id))
    }

    #[instrument(level = "debug", skip(self, now), fields(now = %now))]
    pub fn list_due_tasks(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, butler_conversation_id, created_time, updated_time
             FROM scheduled_task
             WHERE is_enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?
             ORDER BY next_run_at ASC",
        )?;
        let rows = stmt.query_map([now], |row| scheduled_task_from_row(row))?;
        let tasks: Vec<ScheduledTask> = rows.collect::<Result<Vec<_>>>()?;
        Ok(tasks)
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
        let rows = stmt.query_map(params![task_id, limit], |row| {
            Ok(ScheduledTaskLog {
                id: row.get(0)?,
                task_id: row.get(1)?,
                run_id: row.get(2)?,
                message_type: row.get(3)?,
                content: row.get(4)?,
                created_time: get_required_datetime_from_row(row, 5, "created_time")?,
            })
        })?;
        let mut logs: Vec<ScheduledTaskLog> = rows.collect::<Result<Vec<_>>>()?;
        logs.reverse();
        Ok(logs)
    }

    #[instrument(level = "debug", skip(self), fields(task_id, limit))]
    pub fn list_runs_by_task(&self, task_id: i64, limit: u32) -> Result<Vec<ScheduledTaskRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, run_id, status, notify, summary, error_message, started_time, finished_time, butler_followup_status
             FROM scheduled_task_run
             WHERE task_id = ?
             ORDER BY started_time DESC
             LIMIT ?",
        )?;
        let rows =
            stmt.query_map(params![task_id, limit], |row| scheduled_task_run_from_row(row))?;
        let runs: Vec<ScheduledTaskRun> = rows.collect::<Result<Vec<_>>>()?;
        Ok(runs)
    }

    #[instrument(level = "debug", skip(self), fields(task_id))]
    pub fn list_logs_by_run(
        &self,
        task_id: i64,
        run_id: &str,
        limit: u32,
    ) -> Result<Vec<ScheduledTaskLog>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, run_id, message_type, content, created_time
             FROM scheduled_task_log
             WHERE task_id = ? AND run_id = ?
             ORDER BY created_time DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![task_id, run_id, limit], |row| {
            Ok(ScheduledTaskLog {
                id: row.get(0)?,
                task_id: row.get(1)?,
                run_id: row.get(2)?,
                message_type: row.get(3)?,
                content: row.get(4)?,
                created_time: get_required_datetime_from_row(row, 5, "created_time")?,
            })
        })?;
        let mut logs: Vec<ScheduledTaskLog> = rows.collect::<Result<Vec<_>>>()?;
        logs.reverse();
        Ok(logs)
    }

    #[instrument(level = "debug", skip(self, run), fields(task_id = run.task_id, status = %run.status))]
    pub fn create_run(&self, run: &ScheduledTaskRun) -> Result<ScheduledTaskRun> {
        self.with_write_lock(|| {
            let inserted = self.conn.execute(
                "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, summary, error_message, started_time, finished_time, butler_followup_status)
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
                    run.butler_followup_status
                ],
            )?;
            if inserted == 0 {
                return Err(rusqlite::Error::InvalidParameterName(format!("定时任务 {} 已删除，拒绝创建运行记录 run={}", run.task_id, run.run_id)));
            }
            let id = self.conn.last_insert_rowid();
            Ok(ScheduledTaskRun { id, ..run.clone() })
        })
    }

    #[instrument(
        level = "debug",
        skip(self, summary, error_message, finished_time),
        fields(run_id, status)
    )]
    pub fn update_run_result(
        &self,
        run_id: &str,
        status: &str,
        notify: bool,
        summary: Option<&str>,
        error_message: Option<&str>,
        finished_time: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.with_write_lock(|| {
            self.conn.execute(
                "UPDATE scheduled_task_run
                 SET status = ?1, notify = ?2, summary = ?3, error_message = ?4, finished_time = ?5
                 WHERE run_id = ?6",
                params![status, notify, summary, error_message, finished_time, run_id],
            )?;
            Ok(())
        })
    }

    /// List scheduled tasks created from Butler context.
    #[instrument(level = "debug", skip(self))]
    pub fn list_butler_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, butler_conversation_id, created_time, updated_time
             FROM scheduled_task
             WHERE butler_conversation_id IS NOT NULL
             ORDER BY created_time DESC",
        )?;
        let rows = stmt.query_map([], |row| scheduled_task_from_row(row))?;
        rows.collect()
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

    /// 已结束但管家回报尚未确认完成的运行（pending / continuing）。
    pub fn list_runs_awaiting_butler_followup(&self) -> Result<Vec<ScheduledTaskRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, run_id, status, notify, summary, error_message, started_time, finished_time, butler_followup_status
             FROM scheduled_task_run
             WHERE status != 'running' AND butler_followup_status IN ('pending', 'continuing')
             ORDER BY started_time ASC",
        )?;
        let rows = stmt.query_map([], |row| scheduled_task_run_from_row(row))?;
        rows.collect()
    }

    /// Update the butler followup status for a run.
    #[instrument(level = "debug", skip(self), fields(run_id, status))]
    pub fn update_run_butler_followup_status(&self, run_id: &str, status: &str) -> Result<()> {
        self.with_write_lock(|| {
            self.conn.execute(
                "UPDATE scheduled_task_run SET butler_followup_status = ? WHERE run_id = ?",
                params![status, run_id],
            )?;
            Ok(())
        })
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
        let mut stmt = tx.prepare(
            "SELECT id, task_id, run_id, status, notify, summary, error_message, started_time, finished_time, butler_followup_status
             FROM scheduled_task_run WHERE status = 'running' AND finished_time IS NULL",
        )?;
        let rows = stmt.query_map([], |row| scheduled_task_run_from_row(row))?;
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
                status: "failed".to_string(),
                error_message: Some(message),
                finished_time: Some(now),
                ..run
            });
        }
    }
    tx.commit()?;
    Ok(interrupted)
}


/// Rebuild only the known legacy status constraint, retaining the table's other schema,
/// columns, explicit indexes, triggers and AUTOINCREMENT high-water mark.
fn migrate_scheduled_run_missed(conn: &Connection) -> Result<()> {
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
    if statuses.iter().any(|status| status == "missed") {
        if ["running", "success", "failed"].iter().all(|required| statuses.iter().any(|s| s == *required)) {
            return Ok(());
        }
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: incomplete status values; migration refused".into()));
    }
    if statuses != ["running", "success", "failed"] {
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: unexpected status values; migration refused".into()));
    }
    if !regex::Regex::new(r"(?i)\bAUTOINCREMENT\b").expect("static regex").is_match(&sql) {
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: legacy schema without AUTOINCREMENT; migration refused".into()));
    }
    let table_prefix = regex::Regex::new(r#"(?i)^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:"scheduled_task_run"|`scheduled_task_run`|\[scheduled_task_run\]|scheduled_task_run)\s*\("#).expect("static regex");
    let expanded = constraint.replace(&sql, "CHECK(status IN ('running', 'success', 'failed', 'missed'))");
    if !table_prefix.is_match(&expanded) {
        return Err(rusqlite::Error::InvalidParameterName("scheduled_task_run: unsupported CREATE TABLE; migration refused".into()));
    }
    let new_sql = table_prefix.replace(&expanded, "CREATE TABLE scheduled_task_run_missed_migration (").into_owned();

    conn.execute_batch("SAVEPOINT scheduled_run_missed_migration")?;
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
        conn.execute_batch(&format!("INSERT INTO scheduled_task_run_missed_migration ({fields}) SELECT {fields} FROM scheduled_task_run; DROP TABLE scheduled_task_run; ALTER TABLE scheduled_task_run_missed_migration RENAME TO scheduled_task_run;"))?;
        for object in objects { conn.execute_batch(&object)?; }
        if let Some(sequence) = sequence {
            conn.execute("INSERT INTO sqlite_sequence(name, seq) SELECT 'scheduled_task_run', ?1 WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name='scheduled_task_run')", [sequence])?;
            conn.execute("UPDATE sqlite_sequence SET seq=MAX(seq, ?1) WHERE name='scheduled_task_run'", [sequence])?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("RELEASE scheduled_run_missed_migration"),
        Err(error) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK TO scheduled_run_missed_migration; RELEASE scheduled_run_missed_migration") {
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
mod tests {
    use super::{ScheduledTask, ScheduledTaskDatabase};
    use chrono::Utc;
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};

    fn build_test_db() -> ScheduledTaskDatabase {
        let conn = Connection::open_in_memory().unwrap();
        let db = ScheduledTaskDatabase { conn, write_lock: Arc::new(Mutex::new(())) };
        db.create_tables().unwrap();
        db
    }

    fn build_task(name: &str, butler_conversation_id: Option<i64>) -> ScheduledTask {
        let now = Utc::now();
        ScheduledTask {
            id: 0,
            name: name.to_string(),
            is_enabled: true,
            schedule_type: "interval".to_string(),
            interval_value: Some(1),
            interval_unit: Some("day".to_string()),
            start_time: Some("09:00".to_string()),
            week_days: None,
            month_days: None,
            run_at: None,
            next_run_at: Some(now),
            last_run_at: None,
            assistant_id: 1,
            task_prompt: "do work".to_string(),
            notify_prompt: "notify".to_string(),
            butler_conversation_id,
            created_time: now,
            updated_time: now,
        }
    }

    #[test]
    fn list_butler_tasks_returns_all_butler_owned_tasks() {
        let db = build_test_db();
        let task_a = db.create_task(&build_task("task-a", Some(100))).unwrap();
        let task_b = db.create_task(&build_task("task-b", Some(200))).unwrap();
        db.create_task(&build_task("task-c", None)).unwrap();

        let tasks = db.list_butler_tasks().unwrap();
        let ids: Vec<i64> = tasks.into_iter().map(|task| task.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&task_a.id));
        assert!(ids.contains(&task_b.id));
    }
}

#[cfg(test)]
#[path = "tests/scheduled_task_db_tests.rs"]
mod cascade_tests;

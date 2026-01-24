use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};

use super::get_db_path;

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
}

pub struct ScheduledTaskDatabase {
    pub conn: Connection,
    pub db_path: PathBuf,
}

impl ScheduledTaskDatabase {
    #[instrument(level = "debug", skip(app_handle), fields(db = "conversation.db"))]
    pub fn new(app_handle: &tauri::AppHandle) -> rusqlite::Result<Self> {
        let db_path = get_db_path(app_handle, "conversation.db").unwrap();
        let conn = Connection::open(&db_path)?;
        debug!("Opened scheduled task database");
        Ok(ScheduledTaskDatabase { conn, db_path })
    }

    pub fn get_connection(&self) -> rusqlite::Result<Connection> {
        Connection::open(&self.db_path)
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
                status TEXT NOT NULL CHECK(status IN ('running', 'success', 'failed')),
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

        debug!("Scheduled task tables ensured");
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn list_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, created_time, updated_time
             FROM scheduled_task
             ORDER BY created_time DESC",
        )?;
        let rows = stmt.query_map([], |row| {
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
                created_time: get_required_datetime_from_row(row, 15, "created_time")?,
                updated_time: get_required_datetime_from_row(row, 16, "updated_time")?,
            })
        })?;
        let tasks: Vec<ScheduledTask> = rows.collect::<Result<Vec<_>>>()?;
        Ok(tasks)
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn read_task(&self, id: i64) -> Result<Option<ScheduledTask>> {
        let task = self
            .conn
            .query_row(
                "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, created_time, updated_time
                 FROM scheduled_task WHERE id = ?",
                [id],
                |row| {
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
                        created_time: get_required_datetime_from_row(row, 15, "created_time")?,
                        updated_time: get_required_datetime_from_row(row, 16, "updated_time")?,
                    })
                },
            )
            .optional()?;
        Ok(task)
    }

    #[instrument(level = "debug", skip(self, task), fields(name = %task.name))]
    pub fn create_task(&self, task: &ScheduledTask) -> Result<ScheduledTask> {
        self.conn.execute(
            "INSERT INTO scheduled_task (name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, created_time, updated_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                task.created_time,
                task.updated_time
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(ScheduledTask { id, ..task.clone() })
    }

    #[instrument(level = "debug", skip(self, task), fields(id = task.id))]
    pub fn update_task(&self, task: &ScheduledTask) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_task SET name = ?1, is_enabled = ?2, schedule_type = ?3, interval_value = ?4, interval_unit = ?5, start_time = ?6, week_days = ?7, month_days = ?8, run_at = ?9, next_run_at = ?10, last_run_at = ?11, assistant_id = ?12, task_prompt = ?13, notify_prompt = ?14, updated_time = ?15 WHERE id = ?16",
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
                task.updated_time,
                task.id
            ],
        )?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(id))]
    pub fn delete_task(&self, id: i64) -> Result<()> {
        let _ = self
            .conn
            .execute("DELETE FROM scheduled_task_log WHERE task_id = ?", [id]);
        let _ = self
            .conn
            .execute("DELETE FROM scheduled_task_run WHERE task_id = ?", [id]);
        self.conn.execute("DELETE FROM scheduled_task WHERE id = ?", [id])?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self, now), fields(now = %now))]
    pub fn list_due_tasks(&self, now: DateTime<Utc>) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, is_enabled, schedule_type, interval_value, interval_unit, start_time, week_days, month_days, run_at, next_run_at, last_run_at, assistant_id, task_prompt, notify_prompt, created_time, updated_time
             FROM scheduled_task
             WHERE is_enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?
             ORDER BY next_run_at ASC",
        )?;
        let rows = stmt.query_map([now], |row| {
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
                created_time: get_required_datetime_from_row(row, 15, "created_time")?,
                updated_time: get_required_datetime_from_row(row, 16, "updated_time")?,
            })
        })?;
        let tasks: Vec<ScheduledTask> = rows.collect::<Result<Vec<_>>>()?;
        Ok(tasks)
    }

    #[instrument(level = "debug", skip(self, log), fields(task_id = log.task_id))]
    pub fn add_log(&self, log: &ScheduledTaskLog) -> Result<ScheduledTaskLog> {
        self.conn.execute(
            "INSERT INTO scheduled_task_log (task_id, run_id, message_type, content, created_time)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                log.task_id,
                log.run_id,
                log.message_type,
                log.content,
                log.created_time
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(ScheduledTaskLog { id, ..log.clone() })
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
            "SELECT id, task_id, run_id, status, notify, summary, error_message, started_time, finished_time
             FROM scheduled_task_run
             WHERE task_id = ?
             ORDER BY started_time DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![task_id, limit], |row| {
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
            })
        })?;
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
        self.conn.execute(
            "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, summary, error_message, started_time, finished_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.task_id,
                run.run_id,
                run.status,
                run.notify,
                run.summary,
                run.error_message,
                run.started_time,
                run.finished_time
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(ScheduledTaskRun { id, ..run.clone() })
    }

    #[instrument(level = "debug", skip(self, summary, error_message, finished_time), fields(run_id, status))]
    pub fn update_run_result(
        &self,
        run_id: &str,
        status: &str,
        notify: bool,
        summary: Option<&str>,
        error_message: Option<&str>,
        finished_time: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_task_run
             SET status = ?1, notify = ?2, summary = ?3, error_message = ?4, finished_time = ?5
             WHERE run_id = ?6",
            params![status, notify, summary, error_message, finished_time, run_id],
        )?;
        Ok(())
    }
}

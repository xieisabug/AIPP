use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use std::sync::OnceLock;
use tracing::{error, info, warn};

use crate::api::scheduled_task_api::{
    compute_next_run_at_with_config, execute_scheduled_task, ScheduleConfig,
};
use crate::db::scheduled_task_db::{ScheduledTask, ScheduledTaskDatabase};
use crate::FeatureConfigState;
use tauri::Manager;

use super::SchedulerState;

// The first scan happens when the process starts; do not replay earlier occurrences on mobile.
static FIRST_SCHEDULER_SCAN: OnceLock<DateTime<Utc>> = OnceLock::new();

fn is_missed_on_startup(is_mobile: bool, due: DateTime<Utc>, first_scan: DateTime<Utc>) -> bool {
    is_mobile && due < first_scan
}

fn parse_json_array(s: &Option<String>) -> Option<Vec<i32>> {
    s.as_ref().and_then(|v| serde_json::from_str(v).ok())
}

pub async fn run_scheduled_tasks(
    app_handle: tauri::AppHandle,
    scheduler_state: &SchedulerState,
) -> Result<(), String> {
    let feature_state = app_handle
        .try_state::<FeatureConfigState>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "无法获取功能配置状态".to_string())?;
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let now = Utc::now();
    let first_scan = *FIRST_SCHEDULER_SCAN.get_or_init(|| now);
    let due_tasks = db.list_due_tasks(now).map_err(|e| e.to_string())?;
    if due_tasks.is_empty() {
        return Ok(());
    }

    for task in due_tasks {
        let Some(claim) = scheduler_state.claim_scheduled_task(task.id) else { continue; };

        let app_handle = app_handle.clone();
        let feature_state = feature_state.clone();
        let task = task.clone();
        tauri::async_runtime::spawn(async move {
            let _claim = claim;
            let task_id = task.id;
            // The task may have been manually run or edited since list_due_tasks.
            let result = async {
                let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
                let current = db.read_task(task_id).map_err(|e| e.to_string())?;
                let Some(current) = current else { return Ok(()); };
                if !current.is_enabled || current.next_run_at != task.next_run_at {
                    return Ok(());
                }
                let Some(due) = current.next_run_at else { return Ok(()); };
                if is_missed_on_startup(cfg!(mobile), due, first_scan) {
                    return record_missed_task(&app_handle, &current, Utc::now());
                }
                process_scheduled_task(&app_handle, &feature_state, &current).await
            }.await;
            if let Err(err) = result {
                warn!(task_id, error = %err, "定时任务执行失败");
            }
        });
    }

    Ok(())
}

/// Atomically record the missed occurrence and advance the schedule without changing last_run_at.
fn record_missed_task(
    app_handle: &tauri::AppHandle,
    task: &ScheduledTask,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let due = task.next_run_at.ok_or_else(|| format!("task_id={}; missing due time", task.id))?;
    let next = if task.schedule_type == "once" {
        None
    } else {
        compute_next_run_at_with_config(ScheduleConfig {
            schedule_type: &task.schedule_type,
            interval_value: task.interval_value,
            interval_unit: task.interval_unit.as_deref(),
            start_time: task.start_time.as_deref(),
            week_days: parse_json_array(&task.week_days),
            month_days: parse_json_array(&task.month_days),
            run_at: task.run_at,
        }, now)?
    };
    let mut db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    record_missed_in_connection(&mut db.conn, task, due, next, now)
}

fn record_missed_in_connection(
    conn: &mut rusqlite::Connection,
    task: &ScheduledTask,
    due: DateTime<Utc>,
    next: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| format!("task_id={}; missed transaction: {e}", task.id))?;
    let current = tx.query_row(
        "SELECT next_run_at FROM scheduled_task WHERE id=?1 AND is_enabled=1", [task.id],
        |row| crate::utils::db_utils::get_datetime_from_row(row, 0),
    ).optional().map_err(|e| format!("task_id={}; missed schedule read: {e}", task.id))?;
    if current != Some(Some(due)) { return Ok(()); }
    let changed = tx.execute(
        "UPDATE scheduled_task SET is_enabled=?1, next_run_at=?2, updated_time=?3 WHERE id=?4 AND is_enabled=1",
        rusqlite::params![task.schedule_type != "once", next, now, task.id],
    ).map_err(|e| format!("task_id={}; missed schedule update: {e}", task.id))?;
    if changed == 0 { return Ok(()); }
    let run_id = format!("missed:{}:{}", task.id, due.to_rfc3339());
    tx.execute(
        "INSERT INTO scheduled_task_run (task_id, run_id, status, notify, summary, error_message, started_time, finished_time, butler_followup_status) VALUES (?1, ?2, 'missed', 0, NULL, ?3, ?4, ?5, NULL)",
        rusqlite::params![task.id, run_id, "应用进程启动前错过计划时间，未自动执行", due, now],
    ).map_err(|e| format!("task_id={}; missed record: {e}", task.id))?;
    tx.commit().map_err(|e| format!("task_id={}; missed commit: {e}", task.id))?;
    Ok(())
}

#[cfg(test)]
#[path = "scheduled_task_tests.rs"]
mod tests;

async fn process_scheduled_task(
    app_handle: &tauri::AppHandle,
    feature_state: &FeatureConfigState,
    task: &ScheduledTask,
) -> Result<(), String> {
    let now = Utc::now();
    let next_run_at = if task.schedule_type == "once" {
        None
    } else {
        compute_next_run_at_with_config(
            ScheduleConfig {
                schedule_type: &task.schedule_type,
                interval_value: task.interval_value,
                interval_unit: task.interval_unit.as_deref(),
                start_time: task.start_time.as_deref(),
                week_days: parse_json_array(&task.week_days),
                month_days: parse_json_array(&task.month_days),
                run_at: task.run_at,
            },
            now,
        )?
    };

    let updated = ScheduledTask {
        is_enabled: if task.schedule_type == "once" { false } else { task.is_enabled },
        last_run_at: Some(now),
        next_run_at,
        updated_time: now,
        ..task.clone()
    };

    match execute_scheduled_task(app_handle, feature_state, task, &updated).await {
        Ok(result) => {
            if result.notify {
                info!(task_id = updated.id, "定时任务完成并通知");
            } else {
                info!(task_id = updated.id, "定时任务完成");
            }
        }
        Err(err) => {
            error!(task_id = updated.id, error = %err, "定时任务执行失败");
            return Err(err);
        }
    }

    Ok(())
}

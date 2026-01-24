use chrono::Utc;
use tracing::{error, info, warn};

use crate::api::scheduled_task_api::{compute_next_run_at, execute_scheduled_task};
use crate::db::scheduled_task_db::{ScheduledTask, ScheduledTaskDatabase};
use crate::FeatureConfigState;
use tauri::Manager;

use super::SchedulerState;

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
    let due_tasks = db.list_due_tasks(now).map_err(|e| e.to_string())?;
    if due_tasks.is_empty() {
        return Ok(());
    }

    for task in due_tasks {
        let mut running = scheduler_state.running_scheduled_tasks.lock().await;
        if running.contains(&task.id) {
            continue;
        }
        running.insert(task.id);
        drop(running);

        let app_handle = app_handle.clone();
        let scheduler_state = scheduler_state.clone();
        let feature_state = feature_state.clone();
        let task = task.clone();
        tauri::async_runtime::spawn(async move {
            let task_id = task.id;
            let result = process_scheduled_task(&app_handle, &feature_state, &task).await;
            if let Err(err) = result {
                warn!(task_id, error = %err, "定时任务执行失败");
            }
            let mut running = scheduler_state.running_scheduled_tasks.lock().await;
            running.remove(&task_id);
        });
    }

    Ok(())
}

async fn process_scheduled_task(
    app_handle: &tauri::AppHandle,
    feature_state: &FeatureConfigState,
    task: &ScheduledTask,
) -> Result<(), String> {
    let now = Utc::now();
    let next_run_at = if task.schedule_type == "once" {
        None
    } else {
        compute_next_run_at(
            &task.schedule_type,
            task.interval_value,
            task.interval_unit.as_deref(),
            task.run_at,
            now,
        )?
    };

    let updated = ScheduledTask {
        is_enabled: if task.schedule_type == "once" {
            false
        } else {
            task.is_enabled
        },
        last_run_at: Some(now),
        next_run_at,
        updated_time: now,
        ..task.clone()
    };

    let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.update_task(&updated).map_err(|e| e.to_string())?;

    match execute_scheduled_task(app_handle, feature_state, &updated).await {
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

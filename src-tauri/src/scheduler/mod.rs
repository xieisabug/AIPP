//! 定时任务调度器模块
//!
//! 每分钟扫描一次到期任务（见 [`scheduled_task`]），并定时执行对话总结。

pub mod delivery;
pub mod escalation;
pub mod expr;
pub mod gate;
mod scheduled_task;
pub mod service;
mod summary_task;
pub mod tool;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tracing::{error, info};

use crate::errors::AppError;

pub use scheduled_task::run_now;

/// 一次运行的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledRunKind {
    Scheduled,
    Manual,
}

/// 会话是否为管家模式创建的定时任务的运行会话（与管家会话享有同样的工具权限）。
pub fn is_butler_mode_run(app: &tauri::AppHandle, conversation_id: i64) -> bool {
    let Ok(db) = crate::db::scheduled_task_db::ScheduledTaskDatabase::new(app) else { return false };
    db.conn
        .query_row(
            "SELECT COALESCE(t.butler_mode, 0) FROM scheduled_task_run r JOIN scheduled_task t ON t.id = r.task_id
             WHERE r.conversation_id = ?1 LIMIT 1",
            [conversation_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
}

/// 调度器状态，用于管理正在进行的任务
#[derive(Clone)]
pub struct SchedulerState {
    /// 正在进行总结的对话 ID 集合
    pub summarizing_conversations: Arc<TokioMutex<std::collections::HashSet<i64>>>,
}

impl SchedulerState {
    pub fn new() -> Self {
        Self { summarizing_conversations: Arc::new(TokioMutex::new(std::collections::HashSet::new())) }
    }
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run_conversation_summary_now(
    app_handle: &tauri::AppHandle,
    scheduler_state: &SchedulerState,
) -> Result<(), AppError> {
    summary_task::run_summary_task(app_handle, scheduler_state).await
}

pub async fn get_conversation_summary_running_count(scheduler_state: &SchedulerState) -> usize {
    scheduler_state.summarizing_conversations.lock().await.len()
}

/// 启动定时任务调度器
///
/// 在应用启动时调用此函数，会启动一个后台任务，每分钟执行一次注册的定时任务。
pub fn start_scheduler(app_handle: tauri::AppHandle, scheduler_state: SchedulerState) {
    info!("启动定时任务调度器...");

    tauri::async_runtime::spawn(async move {
        // 先完成上次退出遗留运行的收尾，再开始调度新任务，避免两者交错。
        loop {
            match crate::api::scheduled_task_api::recover_interrupted_scheduled_runs(&app_handle).await {
                Ok(count) => {
                    if count > 0 {
                        info!(count, "已收尾上次退出时未结束的定时任务运行");
                    }
                    break;
                }
                Err(e) => {
                    error!(error = %e, "收尾中断的定时任务运行失败，保留待恢复状态并重试");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }

        if let Err(e) = scheduled_task::run_scheduled_tasks(&app_handle).await {
            error!(error = %e, "定时任务调度失败");
        }

        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // 跳过第一次立即执行，等待第一个完整周期
        interval.tick().await;

        loop {
            interval.tick().await;
            // 定时任务先于对话总结：总结可能要等模型，不能推迟到点的任务。
            if let Err(e) = scheduled_task::run_scheduled_tasks(&app_handle).await {
                error!(error = %e, "定时任务调度失败");
            }
            // 上一轮总结还没结束时不再叠加新的一轮。
            static SUMMARY_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if SUMMARY_RUNNING.swap(true, std::sync::atomic::Ordering::AcqRel) {
                continue;
            }
            let app = app_handle.clone();
            let state = scheduler_state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = summary_task::run_summary_task(&app, &state).await {
                    error!(error = %e, "对话总结定时任务执行失败");
                }
                SUMMARY_RUNNING.store(false, std::sync::atomic::Ordering::Release);
            });
        }
    });

    info!("定时任务调度器已启动，每分钟执行一次");
}

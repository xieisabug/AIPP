//! 定时任务调度器模块
//!
//! 提供基于 tokio::time::interval 的定时任务框架，支持注册多个周期性任务。

mod summary_task;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, error, info};

/// 调度器状态，用于管理正在进行的任务
#[derive(Clone)]
pub struct SchedulerState {
    /// 正在进行总结的对话 ID 集合
    pub summarizing_conversations: Arc<TokioMutex<std::collections::HashSet<i64>>>,
}

impl SchedulerState {
    pub fn new() -> Self {
        Self {
            summarizing_conversations: Arc::new(TokioMutex::new(std::collections::HashSet::new())),
        }
    }
}

impl Default for SchedulerState {
    fn default() -> Self {
        Self::new()
    }
}

/// 启动定时任务调度器
///
/// 在应用启动时调用此函数，会启动一个后台任务，每分钟执行一次注册的定时任务。
pub fn start_scheduler(app_handle: tauri::AppHandle, scheduler_state: SchedulerState) {
    info!("启动定时任务调度器...");

    // 使用 tauri::async_runtime::spawn 确保在 Tauri 的异步运行时中执行
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        // 跳过第一次立即执行，等待第一个完整周期
        interval.tick().await;

        loop {
            interval.tick().await;
            debug!("定时任务调度器：开始执行周期任务");

            // 执行对话总结任务
            if let Err(e) = summary_task::run_summary_task(&app_handle, &scheduler_state).await {
                error!(error = %e, "对话总结定时任务执行失败");
            }

            // 未来可以在这里添加更多定时任务
            // 例如：
            // - 清理过期缓存
            // - 同步数据
            // - 健康检查
        }
    });

    info!("定时任务调度器已启动，每分钟执行一次");
}

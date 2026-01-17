//! 对话总结定时任务
//!
//! 每分钟扫描需要总结的对话，并触发总结生成。

use crate::db::conversation_db::ConversationDatabase;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::FeatureConfigState;
use std::collections::HashMap;
use tauri::Manager;
use tracing::{debug, error, info, warn};

use super::SchedulerState;

/// 对话空闲时间阈值（秒）
/// 对话最后一条消息超过此时间后，才会触发总结
const IDLE_THRESHOLD_SECONDS: i64 = 600; // 10 分钟

/// 同时进行的最大总结任务数
const MAX_CONCURRENT_SUMMARIES: usize = 3;

/// 执行对话总结定时任务
///
/// 查询需要总结的对话（满足以下条件）：
/// 1. 最后一条消息时间超过 10 分钟
/// 2. 尚未生成过总结
/// 3. 当前没有正在进行的总结任务
pub async fn run_summary_task(
    app_handle: &tauri::AppHandle,
    scheduler_state: &SchedulerState,
) -> Result<(), AppError> {
    // 检查对话总结功能是否启用
    let config_map = get_feature_config_map(app_handle).await?;
    if !is_summary_enabled(&config_map) {
        debug!("对话总结功能已禁用，跳过定时任务");
        return Ok(());
    }

    // 获取需要总结的对话列表
    let conversations_to_summarize =
        get_conversations_needing_summary(app_handle, scheduler_state).await?;

    if conversations_to_summarize.is_empty() {
        debug!("没有需要总结的对话");
        return Ok(());
    }

    info!(count = conversations_to_summarize.len(), "找到需要总结的对话");

    // 限制并发数量
    let to_process: Vec<i64> =
        conversations_to_summarize.into_iter().take(MAX_CONCURRENT_SUMMARIES).collect();

    // 标记为正在总结中
    {
        let mut summarizing = scheduler_state.summarizing_conversations.lock().await;
        for &conv_id in &to_process {
            summarizing.insert(conv_id);
        }
    }

    // 为每个对话启动总结任务
    for conversation_id in to_process {
        let app_handle_clone = app_handle.clone();
        let scheduler_state_clone = scheduler_state.clone();
        let config_map_clone = config_map.clone();

        tokio::spawn(async move {
            let result = crate::api::ai::summary::generate_conversation_summary(
                &app_handle_clone,
                conversation_id,
                config_map_clone,
            )
            .await;

            // 无论成功失败，都从正在总结集合中移除
            {
                let mut summarizing = scheduler_state_clone.summarizing_conversations.lock().await;
                summarizing.remove(&conversation_id);
            }

            match result {
                Ok(()) => {
                    info!(conversation_id, "对话总结任务完成");
                }
                Err(e) => {
                    warn!(conversation_id, error = %e, "对话总结任务失败");
                }
            }
        });
    }

    Ok(())
}

/// 获取功能配置映射
async fn get_feature_config_map(
    app_handle: &tauri::AppHandle,
) -> Result<HashMap<String, HashMap<String, FeatureConfig>>, AppError> {
    let feature_state = app_handle
        .try_state::<FeatureConfigState>()
        .ok_or_else(|| AppError::UnknownError("无法获取功能配置状态".to_string()))?;

    let config_map = feature_state.config_feature_map.lock().await.clone();
    Ok(config_map)
}

/// 检查对话总结功能是否启用
fn is_summary_enabled(config_map: &HashMap<String, HashMap<String, FeatureConfig>>) -> bool {
    config_map
        .get("conversation_summary")
        .and_then(|fc| fc.get("conversation_summary_enabled"))
        .map(|c| c.value == "true" || c.value == "1")
        .unwrap_or(true) // 默认启用
}

/// 获取需要总结的对话列表
///
/// 条件：
/// 1. 最后一条消息时间超过 IDLE_THRESHOLD_SECONDS
/// 2. 尚未生成过总结（conversation_summary 表中不存在记录）
/// 3. 当前不在正在总结的集合中
async fn get_conversations_needing_summary(
    app_handle: &tauri::AppHandle,
    scheduler_state: &SchedulerState,
) -> Result<Vec<i64>, AppError> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    let conn = conversation_db.get_connection().map_err(AppError::from)?;

    // 获取当前正在总结的对话 ID
    let summarizing_ids: Vec<i64> = {
        let summarizing = scheduler_state.summarizing_conversations.lock().await;
        summarizing.iter().copied().collect()
    };

    // 构建排除条件
    let exclude_clause = if summarizing_ids.is_empty() {
        String::new()
    } else {
        let ids_str: Vec<String> = summarizing_ids.iter().map(|id| id.to_string()).collect();
        format!(" AND c.id NOT IN ({})", ids_str.join(","))
    };

    // 查询需要总结的对话
    // 条件：
    // 1. 对话中有消息
    // 2. 最后一条消息时间超过阈值
    // 3. 没有总结记录
    // 4. 不在正在总结的集合中
    let query = format!(
        r#"
        SELECT DISTINCT c.id
        FROM conversation c
        INNER JOIN message m ON m.conversation_id = c.id
        LEFT JOIN conversation_summary cs ON cs.conversation_id = c.id
        WHERE cs.id IS NULL
          AND (
              SELECT MAX(m2.created_time)
              FROM message m2
              WHERE m2.conversation_id = c.id
          ) < datetime('now', '-{} seconds')
          {}
        ORDER BY c.id DESC
        LIMIT 10
        "#,
        IDLE_THRESHOLD_SECONDS, exclude_clause
    );

    let mut stmt = conn.prepare(&query).map_err(|e| {
        error!(error = %e, "准备查询语句失败");
        AppError::DatabaseError(format!("准备查询语句失败: {}", e))
    })?;

    let conversation_ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| {
            error!(error = %e, "执行查询失败");
            AppError::DatabaseError(format!("执行查询失败: {}", e))
        })?
        .filter_map(|r| r.ok())
        .collect();

    debug!(count = conversation_ids.len(), "查询到需要总结的对话数量");

    Ok(conversation_ids)
}

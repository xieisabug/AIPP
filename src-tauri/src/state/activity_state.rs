use std::collections::HashMap;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;
use tracing::debug;

use crate::api::ai::events::{ActivityFocus, ActivityFocusChangeEvent, ConversationEvent};

/// 对话活动状态，包含当前焦点和 MCP 执行前的备份状态
#[derive(Clone, Debug)]
struct ConversationActivity {
    current: ActivityFocus,
    /// MCP 执行前的状态备份，用于 MCP 完成后恢复
    pre_mcp_backup: Option<ActivityFocus>,
}

impl Default for ConversationActivity {
    fn default() -> Self {
        Self { current: ActivityFocus::None, pre_mcp_backup: None }
    }
}

/// 对话活动状态管理器
///
/// 统一管理每个对话的活动焦点状态，负责发送 activity_focus_change 事件。
/// 这样前端只需要监听一个事件来控制闪亮边框的显示。
#[derive(Clone)]
pub struct ConversationActivityManager {
    /// 每个对话的当前活动状态
    activities: Arc<RwLock<HashMap<i64, ConversationActivity>>>,
}

impl ConversationActivityManager {
    pub fn new() -> Self {
        Self { activities: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// 获取当前活动焦点
    pub async fn get_focus(&self, conversation_id: i64) -> ActivityFocus {
        let activities = self.activities.read().await;
        activities.get(&conversation_id).map(|a| a.current.clone()).unwrap_or_default()
    }

    /// 更新活动焦点并发送事件
    ///
    /// 只有当状态真正变化时才发送事件，避免不必要的前端更新。
    pub async fn set_focus(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        focus: ActivityFocus,
    ) {
        let should_emit = {
            let mut activities = self.activities.write().await;
            let activity = activities.entry(conversation_id).or_default();

            // 只有状态变化时才更新和发送事件
            if activity.current != focus {
                activity.current = focus.clone();
                // 非 MCP 状态时清除备份
                if !matches!(focus, ActivityFocus::McpExecuting { .. }) {
                    activity.pre_mcp_backup = None;
                }
                true
            } else {
                false
            }
        };

        if should_emit {
            debug!(conversation_id = conversation_id, ?focus, "Activity focus changed");

            let event = ActivityFocusChangeEvent { conversation_id, focus };

            let _ = app_handle.emit(
                &format!("conversation_event_{}", conversation_id),
                ConversationEvent {
                    r#type: "activity_focus_change".to_string(),
                    data: serde_json::to_value(event).unwrap(),
                },
            );
        }
    }

    /// 清除活动焦点（设置为 None）
    pub async fn clear_focus(&self, app_handle: &tauri::AppHandle, conversation_id: i64) {
        self.set_focus(app_handle, conversation_id, ActivityFocus::None).await;
    }

    /// 设置用户消息等待响应状态
    pub async fn set_user_pending(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
    ) {
        self.set_focus(app_handle, conversation_id, ActivityFocus::UserPending { message_id })
            .await;
    }

    /// 设置 Assistant 消息流式输出状态
    pub async fn set_assistant_streaming(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
    ) {
        self.set_focus(
            app_handle,
            conversation_id,
            ActivityFocus::AssistantStreaming { message_id },
        )
        .await;
    }

    /// 设置 MCP 工具调用执行状态
    ///
    /// 进入 MCP 状态前，会自动备份当前状态，以便 MCP 完成后恢复。
    pub async fn set_mcp_executing(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        call_id: i64,
    ) {
        // 先备份当前状态（如果不是 MCP 状态）
        {
            let mut activities = self.activities.write().await;
            let activity = activities.entry(conversation_id).or_default();
            if !matches!(activity.current, ActivityFocus::McpExecuting { .. }) {
                activity.pre_mcp_backup = Some(activity.current.clone());
            }
        }

        self.set_focus(app_handle, conversation_id, ActivityFocus::McpExecuting { call_id }).await;
    }

    /// MCP 完成后恢复到执行前的状态
    pub async fn restore_after_mcp(&self, app_handle: &tauri::AppHandle, conversation_id: i64) {
        let backup = {
            let mut activities = self.activities.write().await;
            if let Some(activity) = activities.get_mut(&conversation_id) {
                activity.pre_mcp_backup.take()
            } else {
                None
            }
        };

        if let Some(focus) = backup {
            self.set_focus(app_handle, conversation_id, focus).await;
        } else {
            self.clear_focus(app_handle, conversation_id).await;
        }
    }

    /// 移除对话的活动状态（对话删除时调用）
    pub async fn remove_conversation(&self, conversation_id: i64) {
        let mut activities = self.activities.write().await;
        activities.remove(&conversation_id);
    }
}

impl Default for ConversationActivityManager {
    fn default() -> Self {
        Self::new()
    }
}

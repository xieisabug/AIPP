use std::collections::HashMap;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::api::ai::events::{
    ActivityFocus, ActivityFocusChangeEvent, ConversationEvent, ConversationRuntimePhase,
    ConversationRuntimeState, ConversationShineState, RuntimeStateSnapshotEvent,
    ShineStateSnapshotEvent, ShineTarget,
};
use crate::db::mcp_db::MCPDatabase;

/// 对话活动状态（后端单一真相源）
#[derive(Clone, Debug)]
struct ConversationActivity {
    current: ActivityFocus,
    pending_user_message_id: Option<i64>,
    streaming_message_id: Option<i64>,
    active_mcp_call_ids: Vec<i64>,
    epoch: u64,
    revision: u64,
}

impl Default for ConversationActivity {
    fn default() -> Self {
        Self {
            current: ActivityFocus::None,
            pending_user_message_id: None,
            streaming_message_id: None,
            active_mcp_call_ids: Vec::new(),
            epoch: 0,
            revision: 0,
        }
    }
}

/// 对话活动状态管理器
///
/// 统一管理每个对话的活动焦点状态，负责发送：
/// - 兼容事件 `activity_focus_change`
/// - 语义运行态事件 `runtime_state_snapshot`
/// - 新事件 `shine_state_snapshot`
#[derive(Clone)]
pub struct ConversationActivityManager {
    /// 每个对话的当前活动状态
    activities: Arc<RwLock<HashMap<i64, ConversationActivity>>>,
}

impl ConversationActivityManager {
    pub fn new() -> Self {
        Self { activities: Arc::new(RwLock::new(HashMap::new())) }
    }

    fn recompute_focus(activity: &ConversationActivity) -> ActivityFocus {
        if let Some(call_id) = activity.active_mcp_call_ids.last() {
            return ActivityFocus::McpExecuting { call_id: *call_id };
        }

        if let Some(message_id) = activity.streaming_message_id {
            return ActivityFocus::AssistantStreaming { message_id };
        }

        if let Some(message_id) = activity.pending_user_message_id {
            return ActivityFocus::UserPending { message_id };
        }

        ActivityFocus::None
    }

    fn focus_to_target(focus: &ActivityFocus) -> ShineTarget {
        match focus {
            ActivityFocus::None => ShineTarget::None,
            ActivityFocus::UserPending { message_id } => {
                ShineTarget::Message { message_id: *message_id, reason: "user_pending".to_string() }
            }
            ActivityFocus::AssistantStreaming { message_id } => ShineTarget::Message {
                message_id: *message_id,
                reason: "assistant_streaming".to_string(),
            },
            ActivityFocus::McpExecuting { call_id } => {
                ShineTarget::McpCall { call_id: *call_id, reason: "mcp_executing".to_string() }
            }
        }
    }

    fn focus_to_runtime_phase(focus: &ActivityFocus) -> ConversationRuntimePhase {
        match focus {
            ActivityFocus::None => ConversationRuntimePhase::Idle,
            ActivityFocus::UserPending { .. } => ConversationRuntimePhase::UserPending,
            ActivityFocus::AssistantStreaming { .. } => {
                ConversationRuntimePhase::AssistantStreaming
            }
            ActivityFocus::McpExecuting { .. } => ConversationRuntimePhase::McpExecuting,
        }
    }

    fn build_runtime_state(
        conversation_id: i64,
        activity: &ConversationActivity,
    ) -> ConversationRuntimeState {
        let phase = Self::focus_to_runtime_phase(&activity.current);
        ConversationRuntimeState {
            conversation_id,
            is_running: phase != ConversationRuntimePhase::Idle,
            phase,
            epoch: activity.epoch,
            revision: activity.revision,
        }
    }

    fn build_shine_state(
        conversation_id: i64,
        activity: &ConversationActivity,
    ) -> ConversationShineState {
        ConversationShineState {
            conversation_id,
            epoch: activity.epoch,
            revision: activity.revision,
            primary_target: Self::focus_to_target(&activity.current),
        }
    }

    fn emit_runtime_state_snapshot(
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        state: ConversationRuntimeState,
    ) {
        let event = RuntimeStateSnapshotEvent { state };
        let _ = app_handle.emit(
            &format!("conversation_event_{}", conversation_id),
            ConversationEvent {
                r#type: "runtime_state_snapshot".to_string(),
                data: serde_json::to_value(event).unwrap(),
            },
        );
    }

    fn emit_shine_state_snapshot(
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        state: ConversationShineState,
    ) {
        let event = ShineStateSnapshotEvent { state };
        let _ = app_handle.emit(
            &format!("conversation_event_{}", conversation_id),
            ConversationEvent {
                r#type: "shine_state_snapshot".to_string(),
                data: serde_json::to_value(event).unwrap(),
            },
        );
    }

    fn fetch_active_mcp_call_ids(app_handle: &tauri::AppHandle, conversation_id: i64) -> Vec<i64> {
        let db = match MCPDatabase::new(app_handle) {
            Ok(db) => db,
            Err(e) => {
                warn!(conversation_id, error = %e, "failed to open mcp database while syncing active calls");
                return Vec::new();
            }
        };

        let calls = match db.get_mcp_tool_calls_by_conversation(conversation_id) {
            Ok(calls) => calls,
            Err(e) => {
                warn!(conversation_id, error = %e, "failed to read mcp calls while syncing active calls");
                return Vec::new();
            }
        };

        let mut active: Vec<i64> = calls
            .into_iter()
            .filter(|call| call.status == "pending" || call.status == "executing")
            .map(|call| call.id)
            .collect();
        active.sort_unstable();
        active
    }

    async fn update_state<F>(&self, app_handle: &tauri::AppHandle, conversation_id: i64, updater: F)
    where
        F: FnOnce(&mut ConversationActivity),
    {
        let (state_changed, focus_changed, focus, runtime_state, shine_snapshot) = {
            let mut activities = self.activities.write().await;
            let activity = activities.entry(conversation_id).or_default();
            let before = activity.clone();

            updater(activity);
            activity.current = Self::recompute_focus(activity);

            let state_changed = activity.current != before.current
                || activity.pending_user_message_id != before.pending_user_message_id
                || activity.streaming_message_id != before.streaming_message_id
                || activity.active_mcp_call_ids != before.active_mcp_call_ids
                || activity.epoch != before.epoch;

            if state_changed {
                activity.revision = activity.revision.saturating_add(1);
            }

            let focus_changed = activity.current != before.current;
            (
                state_changed,
                focus_changed,
                activity.current.clone(),
                Self::build_runtime_state(conversation_id, activity),
                Self::build_shine_state(conversation_id, activity),
            )
        };

        if !state_changed {
            return;
        }

        if focus_changed {
            debug!(conversation_id, ?focus, "Activity focus changed");
            let event = ActivityFocusChangeEvent { conversation_id, focus };
            let _ = app_handle.emit(
                &format!("conversation_event_{}", conversation_id),
                ConversationEvent {
                    r#type: "activity_focus_change".to_string(),
                    data: serde_json::to_value(event).unwrap(),
                },
            );
        }

        Self::emit_runtime_state_snapshot(app_handle, conversation_id, runtime_state);
        Self::emit_shine_state_snapshot(app_handle, conversation_id, shine_snapshot);
    }

    /// 获取当前活动焦点
    pub async fn get_focus(&self, conversation_id: i64) -> ActivityFocus {
        let activities = self.activities.read().await;
        activities.get(&conversation_id).map(|a| a.current.clone()).unwrap_or_default()
    }

    /// 获取当前闪亮状态快照
    pub async fn get_shine_state(&self, conversation_id: i64) -> ConversationShineState {
        let activities = self.activities.read().await;
        if let Some(activity) = activities.get(&conversation_id) {
            return Self::build_shine_state(conversation_id, activity);
        }
        ConversationShineState {
            conversation_id,
            epoch: 0,
            revision: 0,
            primary_target: ShineTarget::None,
        }
    }

    /// 获取当前语义化运行状态（用于发送按钮等运行态 UI）
    pub async fn get_runtime_state(&self, conversation_id: i64) -> ConversationRuntimeState {
        let activities = self.activities.read().await;
        if let Some(activity) = activities.get(&conversation_id) {
            return Self::build_runtime_state(conversation_id, activity);
        }
        ConversationRuntimeState {
            conversation_id,
            is_running: false,
            phase: ConversationRuntimePhase::Idle,
            epoch: 0,
            revision: 0,
        }
    }

    /// 清除所有活动焦点（设置为 None）
    pub async fn clear_focus(&self, app_handle: &tauri::AppHandle, conversation_id: i64) {
        self.update_state(app_handle, conversation_id, |activity| {
            activity.pending_user_message_id = None;
            activity.streaming_message_id = None;
            activity.active_mcp_call_ids.clear();
        })
        .await;
    }

    /// 清除消息焦点但保留/同步 MCP 执行焦点（流式结束时使用）
    pub async fn clear_message_focus_keep_mcp(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
    ) {
        let active_calls = Self::fetch_active_mcp_call_ids(app_handle, conversation_id);
        self.update_state(app_handle, conversation_id, move |activity| {
            activity.pending_user_message_id = None;
            activity.streaming_message_id = None;
            activity.active_mcp_call_ids = active_calls;
        })
        .await;
    }

    /// 设置用户消息等待响应状态
    pub async fn set_user_pending(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
    ) {
        self.update_state(app_handle, conversation_id, |activity| {
            // 新一轮请求开始，切换 epoch 并清理旧残留
            activity.epoch = activity.epoch.saturating_add(1);
            activity.pending_user_message_id = Some(message_id);
            activity.streaming_message_id = None;
            activity.active_mcp_call_ids.clear();
        })
        .await;
    }

    /// 设置 Assistant 消息流式输出状态
    pub async fn set_assistant_streaming(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
    ) {
        self.update_state(app_handle, conversation_id, |activity| {
            activity.pending_user_message_id = None;
            activity.streaming_message_id = Some(message_id);
        })
        .await;
    }

    /// 设置 MCP 工具调用执行状态
    pub async fn set_mcp_executing(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        call_id: i64,
    ) {
        self.update_state(app_handle, conversation_id, |activity| {
            if let Some(idx) = activity.active_mcp_call_ids.iter().position(|id| *id == call_id) {
                activity.active_mcp_call_ids.remove(idx);
            }
            activity.active_mcp_call_ids.push(call_id);
        })
        .await;
    }

    /// MCP 完成后从数据库重同步活跃调用，再重新计算焦点
    pub async fn restore_after_mcp(&self, app_handle: &tauri::AppHandle, conversation_id: i64) {
        let active_calls = Self::fetch_active_mcp_call_ids(app_handle, conversation_id);
        self.update_state(app_handle, conversation_id, move |activity| {
            activity.active_mcp_call_ids = active_calls;
        })
        .await;
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

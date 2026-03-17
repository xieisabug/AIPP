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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveMcpStatus {
    Pending,
    Executing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveMcpCall {
    call_id: i64,
    status: ActiveMcpStatus,
}

/// 对话活动状态（后端单一真相源）
#[derive(Clone, Debug)]
struct ConversationActivity {
    current: ActivityFocus,
    pending_user_message_id: Option<i64>,
    streaming_message_id: Option<i64>,
    active_mcp_calls: Vec<ActiveMcpCall>,
    epoch: u64,
    revision: u64,
}

impl Default for ConversationActivity {
    fn default() -> Self {
        Self {
            current: ActivityFocus::None,
            pending_user_message_id: None,
            streaming_message_id: None,
            active_mcp_calls: Vec::new(),
            epoch: 0,
            revision: 0,
        }
    }
}

impl ConversationActivity {
    fn upsert_mcp_call(&mut self, call_id: i64, status: ActiveMcpStatus) {
        self.finish_mcp_call(call_id);

        let active_call = ActiveMcpCall { call_id, status };
        match status {
            ActiveMcpStatus::Pending => {
                let insert_at = self
                    .active_mcp_calls
                    .iter()
                    .position(|call| call.status == ActiveMcpStatus::Executing)
                    .unwrap_or(self.active_mcp_calls.len());
                self.active_mcp_calls.insert(insert_at, active_call);
            }
            ActiveMcpStatus::Executing => {
                self.active_mcp_calls.push(active_call);
            }
        }
    }

    fn finish_mcp_call(&mut self, call_id: i64) {
        self.active_mcp_calls.retain(|call| call.call_id != call_id);
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
        if let Some(call) = activity
            .active_mcp_calls
            .iter()
            .rev()
            .find(|call| call.status == ActiveMcpStatus::Executing)
        {
            return ActivityFocus::McpExecuting { call_id: call.call_id };
        }

        if let Some(message_id) = activity.streaming_message_id {
            return ActivityFocus::AssistantStreaming { message_id };
        }

        if let Some(message_id) = activity.pending_user_message_id {
            return ActivityFocus::UserPending { message_id };
        }

        ActivityFocus::None
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
        let primary_target = match &activity.current {
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
        };

        ConversationShineState {
            conversation_id,
            epoch: activity.epoch,
            revision: activity.revision,
            primary_target,
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

    fn fetch_active_mcp_calls(
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
    ) -> Vec<ActiveMcpCall> {
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

        let mut pending: Vec<(String, i64)> = calls
            .iter()
            .filter(|call| call.status == "pending")
            .map(|call| (call.created_time.clone(), call.id))
            .collect();
        pending.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        let mut executing: Vec<(String, i64)> = calls
            .iter()
            .filter(|call| call.status == "executing")
            .map(|call| {
                (call.started_time.clone().unwrap_or_else(|| call.created_time.clone()), call.id)
            })
            .collect();
        executing.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));

        pending
            .into_iter()
            .map(|(_, call_id)| ActiveMcpCall { call_id, status: ActiveMcpStatus::Pending })
            .chain(
                executing.into_iter().map(|(_, call_id)| ActiveMcpCall {
                    call_id,
                    status: ActiveMcpStatus::Executing,
                }),
            )
            .collect()
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
                || activity.active_mcp_calls != before.active_mcp_calls
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
            activity.active_mcp_calls.clear();
        })
        .await;
    }

    /// 清除消息焦点但保留/同步 MCP 执行焦点（流式结束时使用）
    pub async fn clear_message_focus_keep_mcp(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
    ) {
        let active_calls = Self::fetch_active_mcp_calls(app_handle, conversation_id);
        self.update_state(app_handle, conversation_id, move |activity| {
            activity.pending_user_message_id = None;
            activity.streaming_message_id = None;
            activity.active_mcp_calls = active_calls;
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
            activity.active_mcp_calls.clear();
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

    /// 设置 MCP 工具调用待执行状态
    pub async fn set_mcp_pending(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        call_id: i64,
    ) {
        self.update_state(app_handle, conversation_id, |activity| {
            activity.upsert_mcp_call(call_id, ActiveMcpStatus::Pending);
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
            activity.upsert_mcp_call(call_id, ActiveMcpStatus::Executing);
        })
        .await;
    }

    /// 移除已经完成的 MCP 调用，让消息流式状态及时接管
    pub async fn finish_mcp_call(
        &self,
        app_handle: &tauri::AppHandle,
        conversation_id: i64,
        call_id: i64,
    ) {
        self.update_state(app_handle, conversation_id, |activity| {
            activity.finish_mcp_call(call_id);
        })
        .await;
    }

    /// MCP 完成后从数据库重同步活跃调用，再重新计算焦点
    pub async fn restore_after_mcp(&self, app_handle: &tauri::AppHandle, conversation_id: i64) {
        let active_calls = Self::fetch_active_mcp_calls(app_handle, conversation_id);
        self.update_state(app_handle, conversation_id, move |activity| {
            activity.active_mcp_calls = active_calls;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recompute_focus_prefers_newest_executing_call() {
        let mut activity = ConversationActivity::default();
        activity.upsert_mcp_call(11, ActiveMcpStatus::Pending);
        activity.upsert_mcp_call(12, ActiveMcpStatus::Pending);
        activity.upsert_mcp_call(11, ActiveMcpStatus::Executing);
        activity.upsert_mcp_call(13, ActiveMcpStatus::Executing);

        assert_eq!(
            ConversationActivityManager::recompute_focus(&activity),
            ActivityFocus::McpExecuting { call_id: 13 }
        );
    }

    #[test]
    fn recompute_focus_ignores_pending_calls_when_waiting_for_user() {
        let mut activity = ConversationActivity::default();
        activity.upsert_mcp_call(21, ActiveMcpStatus::Pending);
        activity.upsert_mcp_call(22, ActiveMcpStatus::Pending);

        assert_eq!(ConversationActivityManager::recompute_focus(&activity), ActivityFocus::None);
    }

    #[test]
    fn pending_mcp_does_not_override_streaming_message_focus() {
        let mut activity = ConversationActivity::default();
        activity.streaming_message_id = Some(99);
        activity.upsert_mcp_call(31, ActiveMcpStatus::Pending);
        assert_eq!(
            ConversationActivityManager::recompute_focus(&activity),
            ActivityFocus::AssistantStreaming { message_id: 99 }
        );
    }

    #[test]
    fn finish_mcp_call_hands_off_to_streaming_message() {
        let mut activity = ConversationActivity::default();
        activity.streaming_message_id = Some(99);
        activity.upsert_mcp_call(31, ActiveMcpStatus::Executing);
        activity.finish_mcp_call(31);
        assert_eq!(
            ConversationActivityManager::recompute_focus(&activity),
            ActivityFocus::AssistantStreaming { message_id: 99 }
        );
    }

    #[test]
    fn runtime_and_shine_follow_user_to_assistant_to_idle_lifecycle() {
        let conversation_id = 41;
        let mut activity = ConversationActivity::default();
        activity.epoch = 3;

        activity.pending_user_message_id = Some(1001);
        activity.current = ConversationActivityManager::recompute_focus(&activity);

        let pending_runtime =
            ConversationActivityManager::build_runtime_state(conversation_id, &activity);
        let pending_shine =
            ConversationActivityManager::build_shine_state(conversation_id, &activity);
        assert_eq!(pending_runtime.phase, ConversationRuntimePhase::UserPending);
        assert!(pending_runtime.is_running);
        assert_eq!(
            pending_shine.primary_target,
            ShineTarget::Message { message_id: 1001, reason: "user_pending".to_string() }
        );

        activity.pending_user_message_id = None;
        activity.streaming_message_id = Some(1002);
        activity.current = ConversationActivityManager::recompute_focus(&activity);

        let streaming_runtime =
            ConversationActivityManager::build_runtime_state(conversation_id, &activity);
        let streaming_shine =
            ConversationActivityManager::build_shine_state(conversation_id, &activity);
        assert_eq!(streaming_runtime.phase, ConversationRuntimePhase::AssistantStreaming);
        assert!(streaming_runtime.is_running);
        assert_eq!(
            streaming_shine.primary_target,
            ShineTarget::Message { message_id: 1002, reason: "assistant_streaming".to_string() }
        );

        activity.streaming_message_id = None;
        activity.current = ConversationActivityManager::recompute_focus(&activity);

        let idle_runtime =
            ConversationActivityManager::build_runtime_state(conversation_id, &activity);
        let idle_shine = ConversationActivityManager::build_shine_state(conversation_id, &activity);
        assert_eq!(idle_runtime.phase, ConversationRuntimePhase::Idle);
        assert!(!idle_runtime.is_running);
        assert_eq!(idle_shine.primary_target, ShineTarget::None);
    }

    #[test]
    fn pending_mcp_only_keeps_runtime_idle_and_shine_none() {
        let conversation_id = 42;
        let mut activity = ConversationActivity::default();
        activity.epoch = 4;
        activity.upsert_mcp_call(88, ActiveMcpStatus::Pending);
        activity.current = ConversationActivityManager::recompute_focus(&activity);

        let runtime = ConversationActivityManager::build_runtime_state(conversation_id, &activity);
        let shine = ConversationActivityManager::build_shine_state(conversation_id, &activity);

        assert_eq!(activity.current, ActivityFocus::None);
        assert_eq!(runtime.phase, ConversationRuntimePhase::Idle);
        assert!(!runtime.is_running);
        assert_eq!(shine.primary_target, ShineTarget::None);
    }
}

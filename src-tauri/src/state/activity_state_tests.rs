use super::*;
use crate::api::ai::agent_driver::lifecycle::{AgentSessionBinding, AgentSessionBindings};

fn binding(driver: &str, run: &str) -> AgentSessionBinding {
    AgentSessionBinding {
        conversation_id: 42, provider_id: 7, driver_id: driver.into(),
        config_fingerprint: "config".into(), run_id: run.into(),
    }
}

fn busy_activity() -> ConversationActivity {
    let mut activity = ConversationActivity {
        pending_user_message_id: Some(101), streaming_message_id: Some(102),
        epoch: 5, revision: 9, ..Default::default()
    };
    activity.upsert_mcp_call(103, ActiveMcpStatus::Executing);
    activity.current = ConversationActivityManager::recompute_focus(&activity);
    activity
}

/// Exercise the production binding guard and actual field mutation together.
/// Only desktop delivery and MCP database reads are omitted in this test.
#[test]
fn test_agent_activity_matching_run_clears_pending_streaming_and_mcp_fields() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("codex_app_server", "current")).unwrap();
    let identity = AgentEventIdentity::new("codex_app_server", 42).with_run("current");
    let mut activity = busy_activity();
    assert!(bindings.publish_if_current(42, &identity.driver_id, identity.run_id.as_deref(),
        || activity.clear_message_fields(Vec::new())).unwrap());
    assert_eq!(activity.pending_user_message_id, None);
    assert_eq!(activity.streaming_message_id, None);
    assert!(activity.active_mcp_calls.is_empty());
    assert_eq!(ConversationActivityManager::recompute_focus(&activity), ActivityFocus::None);
    assert_eq!((activity.epoch, activity.revision), (5, 9));
}

#[test]
fn test_agent_activity_old_driver_run_or_missing_run_preserves_replacement_activity() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("codex_app_server", "current")).unwrap();
    for identity in [
        AgentEventIdentity::new("acp", 42).with_run("current"),
        AgentEventIdentity::new("codex_app_server", 42).with_run("old"),
        AgentEventIdentity::new("codex_app_server", 42),
    ] {
        let mut activity = busy_activity();
        let before = activity.clone();
        assert!(!bindings.publish_if_current(42, &identity.driver_id, identity.run_id.as_deref(),
            || activity.clear_message_fields(Vec::new())).unwrap());
        assert_eq!(activity.pending_user_message_id, before.pending_user_message_id);
        assert_eq!(activity.streaming_message_id, before.streaming_message_id);
        assert_eq!(activity.active_mcp_calls, before.active_mcp_calls);
        assert_eq!(activity.current, before.current);
        assert_eq!((activity.epoch, activity.revision), (before.epoch, before.revision));
        assert!(ConversationActivityManager::build_runtime_state(42, &activity).is_running);
    }
}

#[test]
fn test_agent_activity_matching_run_preserves_refreshed_mcp_execution_on_message_clear() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("claude_sdk", "current")).unwrap();
    let mut activity = busy_activity();
    let refreshed = vec![ActiveMcpCall { call_id: 200, status: ActiveMcpStatus::Executing }];
    assert!(bindings.publish_if_current(42, "claude_sdk", Some("current"),
        || activity.clear_message_fields(refreshed.clone())).unwrap());
    assert_eq!(activity.pending_user_message_id, None);
    assert_eq!(activity.streaming_message_id, None);
    assert_eq!(activity.active_mcp_calls, refreshed);
    activity.current = ConversationActivityManager::recompute_focus(&activity);
    assert_eq!(activity.current, ActivityFocus::McpExecuting { call_id: 200 });
    assert!(ConversationActivityManager::build_runtime_state(42, &activity).is_running);
}

/// The generation check must happen after obtaining the activity lock. An old
/// completion waiting for that lock cannot act on its earlier binding snapshot.
#[tokio::test]
async fn test_agent_activity_replacement_while_waiting_for_activity_lock_blocks_old_cleanup() {
    let manager = ConversationActivityManager::new();
    let bindings = Arc::new(AgentSessionBindings::default());
    bindings.bind(binding("acp", "old")).unwrap();
    let mut locked = manager.activities.write().await;
    locked.insert(42, busy_activity());
    let waiting = Arc::new(tokio::sync::Notify::new());
    let task = tokio::spawn({
        let manager = manager.clone(); let bindings = bindings.clone(); let waiting = waiting.clone();
        async move {
            waiting.notify_one();
            let mut activities = manager.activities.write().await;
            let activity = activities.get_mut(&42).unwrap();
            bindings.publish_if_current(42, "acp", Some("old"), || activity.clear_message_fields(Vec::new())).unwrap()
        }
    });
    waiting.notified().await;
    bindings.bind(binding("codex_app_server", "new")).unwrap();
    drop(locked);
    assert!(!task.await.unwrap());
    assert!(manager.get_runtime_state(42).await.is_running);
    assert_eq!(manager.get_focus(42).await, ActivityFocus::McpExecuting { call_id: 103 });
    let activities = manager.activities.read().await;
    assert_eq!(activities[&42].pending_user_message_id, Some(101));
    assert_eq!(activities[&42].streaming_message_id, Some(102));
}

#[test]
fn test_agent_activity_old_stream_delta_cannot_replace_current_pending_focus() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("codex_app_server", "new")).unwrap();
    for (driver, run) in [("acp", Some("old")), ("codex_app_server", Some("old")), ("codex_app_server", None)] {
        let mut activity = ConversationActivity { pending_user_message_id: Some(200), epoch: 3, revision: 5, ..Default::default() };
        activity.current = ConversationActivityManager::recompute_focus(&activity);
        assert!(!bindings.publish_if_current(42, driver, run, || activity.set_streaming_message(100)).unwrap());
        assert_eq!(activity.pending_user_message_id, Some(200));
        assert_eq!(activity.streaming_message_id, None);
        assert_eq!(activity.current, ActivityFocus::UserPending { message_id: 200 });
        assert_eq!((activity.epoch, activity.revision), (3, 5));
    }
}

#[test]
fn test_agent_activity_current_stream_delta_sets_message_and_preserves_mcp_calls() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("codex_app_server", "new")).unwrap();
    let mut activity = busy_activity();
    let mcp_calls = activity.active_mcp_calls.clone();
    assert!(bindings.publish_if_current(42, "codex_app_server", Some("new"), || activity.set_streaming_message(200)).unwrap());
    assert_eq!(activity.pending_user_message_id, None);
    assert_eq!(activity.streaming_message_id, Some(200));
    assert_eq!(activity.active_mcp_calls, mcp_calls);
}

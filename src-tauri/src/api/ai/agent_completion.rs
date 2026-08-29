use crate::api::ai::title::maybe_generate_title_from_conversation_if_needed;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::system_db::FeatureConfig;
use crate::FeatureConfigState;
use std::collections::HashMap;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Codex,
    ClaudeCode,
    Acp,
}

impl AgentKind {
    fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::Acp => "ACP",
        }
    }
}

fn notification_enabled(
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> bool {
    config_feature_map
        .get("display")
        .and_then(|config| config.get("notification_on_completion"))
        .is_some_and(|config| matches!(config.value.as_str(), "true" | "1"))
}

fn truncate_notification_body(content: &str) -> String {
    if content.chars().count() > 60 {
        format!("{}...", content.chars().take(57).collect::<String>())
    } else {
        content.to_string()
    }
}

fn assistant_name(app_handle: &tauri::AppHandle, conversation_id: i64) -> Option<String> {
    let conversation_db = ConversationDatabase::new(app_handle).ok()?;
    let conversation = conversation_db.conversation_repo().ok()?.read(conversation_id).ok()??;
    let assistant_id = conversation.assistant_id?;
    AssistantDatabase::new(app_handle).ok()?.get_assistant(assistant_id).ok().map(|item| item.name)
}

pub async fn handle_agent_success(
    app_handle: &tauri::AppHandle,
    window: &tauri::Window,
    conversation_id: i64,
    content: &str,
    agent_kind: AgentKind,
) {
    let config_feature_map = app_handle
        .state::<FeatureConfigState>()
        .config_feature_map
        .lock()
        .await
        .clone();

    if let Err(error) = maybe_generate_title_from_conversation_if_needed(
        app_handle,
        conversation_id,
        config_feature_map.clone(),
        window.clone(),
        "agent_success",
    )
    .await
    {
        warn!(conversation_id, agent = agent_kind.display_name(), error = %error, "failed to schedule Agent title generation");
    }

    if !notification_enabled(&config_feature_map) {
        return;
    }
    if crate::utils::window_utils::is_chat_or_ask_window_focused(app_handle) {
        debug!(conversation_id, agent = agent_kind.display_name(), "Agent completion notification skipped because chat or ask window is focused");
        return;
    }

    let title = assistant_name(app_handle, conversation_id)
        .map(|name| format!("{} 已完成 - {}", agent_kind.display_name(), name))
        .unwrap_or_else(|| format!("{} 已完成", agent_kind.display_name()));
    let body = truncate_notification_body(content);
    if let Err(error) = app_handle.notification().builder().title(&title).body(&body).show() {
        warn!(conversation_id, agent = agent_kind.display_name(), error = %error, "failed to send Agent completion notification");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature_config(key: &str, value: &str) -> FeatureConfig {
        FeatureConfig {
            id: None,
            feature_code: "display".to_string(),
            key: key.to_string(),
            value: value.to_string(),
            data_type: "boolean".to_string(),
            description: None,
        }
    }

    #[test]
    fn test_agent_notification_is_disabled_by_default() {
        assert!(!notification_enabled(&HashMap::new()));
    }

    #[test]
    fn test_agent_notification_uses_unified_display_switch() {
        let config = HashMap::from([(
            "display".to_string(),
            HashMap::from([(
                "notification_on_completion".to_string(),
                feature_config("notification_on_completion", "true"),
            )]),
        )]);

        assert!(notification_enabled(&config));

        let disabled = HashMap::from([(
            "display".to_string(),
            HashMap::from([(
                "notification_on_completion".to_string(),
                feature_config("notification_on_completion", "false"),
            )]),
        )]);
        assert!(!notification_enabled(&disabled));
    }

    #[test]
    fn test_agent_notification_body_truncates_unicode_safely() {
        let body = truncate_notification_body(&"好".repeat(61));
        assert_eq!(body.chars().count(), 60);
        assert!(body.ends_with("..."));
    }
}

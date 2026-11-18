use crate::api::ai::title::maybe_generate_title_from_conversation_if_needed;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::system_db::FeatureConfig;
use crate::FeatureConfigState;
use std::collections::HashMap;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Codex,
    ClaudeCode,
    Acp,
    Pi,
    Cursor,
    OpenCode,
    Kiro,
    Antigravity,
}

impl AgentKind {
    fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::Acp => "ACP",
            Self::Pi => "Pi",
            Self::Cursor => "Cursor",
            Self::OpenCode => "OpenCode",
            Self::Kiro => "Kiro",
            Self::Antigravity => "Antigravity",
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

type ConfigMap = HashMap<String, HashMap<String, FeatureConfig>>;

fn butler_reply_notifications_enabled(config: &ConfigMap) -> bool {
    config.get("display").and_then(|c| c.get("notification_on_butler_reply"))
        .is_none_or(|c| !matches!(c.value.as_str(), "false" | "0"))
}

pub(crate) fn notify_butler_reply(app: &tauri::AppHandle, name: &str, content: &str, config: &ConfigMap) {
    if !butler_reply_notifications_enabled(config) || matches!(content.trim(), "" | "[[NO_REPLY]]") {
        return;
    }
    let app_focused = app.webview_windows().values().any(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    });
    if app_focused {
        return;
    }
    let body = truncate_notification_body(crate::utils::reply_quote::strip(content));
    if let Err(error) = app.notification().builder().title(format!("{name} 回复了消息")).body(body).show() {
        warn!(%error, "failed to send Butler reply notification");
    }
}

pub(super) fn notify_completion(app: &tauri::AppHandle, conversation_id: i64, message_id: Option<i64>, content: &str, config: &ConfigMap, agent: Option<AgentKind>) {
    let lookup = ConversationDatabase::new(app).map_err(crate::errors::AppError::from)
        .and_then(|db| db.conversation_repo());
    let repo = match lookup {
        Ok(repo) => repo,
        Err(error) => {
            warn!(conversation_id, %error, "failed to open conversation repository for reply notification");
            return;
        }
    };
    let Ok(Some(conversation)) = repo.read(conversation_id) else { return };
    let kind = conversation.conversation_kind.as_str();
    if kind == crate::db::butler_group::MEMBER_KIND {
        return; // 群成员只有在回复正式公开后才通知。
    }
    if matches!(kind, "butler_main" | "butler_contact") {
        if !message_id.is_some_and(|id| repo.is_public_butler_reply(conversation_id, id).unwrap_or(false)) {
            return;
        }
        let _ = app.emit("butler_reply_received", conversation_id);
    }
    if kind == "butler_main" {
        let name = config.get("experimental").and_then(|c| c.get("butler_display_name"))
            .map(|c| c.value.trim()).filter(|s| !s.is_empty()).unwrap_or("总管家");
        notify_butler_reply(app, name, content, config);
        return;
    }
    let name = || conversation.assistant_id
        .and_then(|id| AssistantDatabase::new(app).ok()?.get_assistant(id).ok()).map(|a| a.name);
    if kind == "butler_contact" {
        notify_butler_reply(app, &name().unwrap_or_else(|| "助手".into()), content, config);
        return;
    }
    if !notification_enabled(config) || crate::utils::window_utils::is_chat_or_ask_window_focused(app) {
        return;
    }
    let label = agent.map(|a| format!("{} 已完成", a.display_name())).unwrap_or_else(|| "AI 消息完成".into());
    let title = name().map(|n| format!("{label} - {n}")).unwrap_or(label);
    if let Err(error) = app.notification().builder().title(title).body(truncate_notification_body(content)).show() {
        warn!(conversation_id, %error, "failed to send completion notification");
    }
}

pub async fn handle_agent_success(
    app_handle: &tauri::AppHandle,
    window: &tauri::Window,
    conversation_id: i64,
    content: &str,
    message_id: i64,
    agent_kind: AgentKind,
) {
    if !super::events::output_goal_current(app_handle,conversation_id,Some(message_id)) {return;}
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

    notify_completion(app_handle, conversation_id, Some(message_id), content, &config_feature_map, Some(agent_kind));
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
    fn test_butler_reply_notification_defaults_on_and_respects_its_own_switch() {
        let mut config = ConfigMap::new();
        assert!(butler_reply_notifications_enabled(&config));
        let display = config.entry("display".into()).or_default();
        display.insert("notification_on_completion".into(), feature_config("notification_on_completion", "true"));
        display.insert("notification_on_butler_reply".into(), feature_config("notification_on_butler_reply", "false"));
        assert!(!butler_reply_notifications_enabled(&config));
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

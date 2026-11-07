use super::*;
use super::super::registry::AgentDriverMetadata;
use super::super::session::{AgentPromptContext, AgentSessionCapabilities, AgentSessionControl, AgentSessionHandle};
use crate::db::conversation_db::MessageAttachment;
use std::sync::{Arc, Mutex};

struct Control(&'static str);
#[async_trait::async_trait]
impl AgentSessionControl for Control {
    fn metadata(&self) -> AgentDriverMetadata {
        AgentDriverMetadata { id: self.0, display_name: self.0, message_model_name: self.0, operations: AgentSessionCapabilities { structured_attachments: false, cancel_prompt: true, set_config_option: true } }
    }
    fn send_prompt(&self, _: i64, _: String, _: Vec<MessageAttachment>, _: &dyn AgentPromptContext) -> Result<(), AppError> { Ok(()) }
    async fn cancel_current_prompt(&self) -> Result<(), AppError> { Ok(()) }
    async fn set_config_option(&self, _: String, _: String) -> Result<(), AppError> { Ok(()) }
}
fn binding(driver_id: &str, run_id: &str) -> AgentSessionBinding {
    AgentSessionBinding { conversation_id: 42, provider_id: 7, driver_id: driver_id.into(), config_fingerprint: "fingerprint".into(), run_id: run_id.into() }
}
fn session(driver_id: &'static str, run_id: &str) -> AgentSessionLookup {
    AgentSessionLookup { handle: AgentSessionHandle::new(Control(driver_id), run_id.into()), snapshot: serde_json::json!({"conversation_id":42}), run_id: run_id.into(), binding: None }
}

#[test]
fn test_lifecycle_rejects_cross_instance_driver_and_stale_generation() {
    let binding = binding("codex_app_server", "new");
    assert!(validate_instance(&binding, 7, "codex_app_server").is_ok());
    assert!(validate_instance(&binding, 8, "codex_app_server").is_err());
    assert!(validate_instance(&binding, 7, "acp").is_err());
    assert!(validate_lookup(Some(&binding), Some(session("acp", "new"))).is_err());
    assert!(validate_lookup(Some(&binding), Some(session("codex_app_server", "old"))).is_err());
    let resolved = validate_lookup(Some(&binding), Some(session("codex_app_server", "new"))).unwrap().unwrap();
    assert_eq!(resolved.binding.as_ref(), Some(&binding));
}

#[test]
fn test_lifecycle_unbound_legacy_sessions_require_unambiguous_driver() {
    assert!(resolve_unbound(vec![], 42).unwrap().is_none());
    assert_eq!(resolve_unbound(vec![session("acp", "old")], 42).unwrap().unwrap().handle.metadata().id, "acp");
    let error = resolve_unbound(vec![session("acp", "old"), session("codex_app_server", "new")], 42).err().unwrap().to_string();
    assert!(error.contains("multiple unbound")); assert!(error.contains("no fallback"));
}

/// The departed selected actor must remain authoritative on *later* reads too.
#[tokio::test]
async fn test_lifecycle_departed_selected_actor_never_adopts_stale_other_driver() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("codex_app_server", "selected")).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    for _ in 0..3 {
        let calls = calls.clone();
        let result = lookup_registered_session(&bindings, 42, vec!["acp".into(), "codex_app_server".into()], move |driver| {
            calls.lock().unwrap().push(driver.clone());
            async move { Ok(if driver == "acp" { Some(session("acp", "stale")) } else { None }) }
        }).await.unwrap();
        assert!(result.is_none());
        assert_eq!(bindings.get(42).unwrap().unwrap().driver_id, "codex_app_server");
    }
    assert_eq!(*calls.lock().unwrap(), ["codex_app_server", "codex_app_server", "codex_app_server"]);
}

#[tokio::test]
async fn test_lifecycle_actual_binding_wins_over_old_native_map_entries() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("custom_driver", "selected")).unwrap();
    let result = lookup_registered_session(&bindings, 42, vec!["acp".into(), "custom_driver".into()], |driver| async move {
        assert_eq!(driver, "custom_driver");
        Ok(Some(session("custom_driver", "selected")))
    }).await.unwrap().unwrap();
    assert_eq!(result.handle.run_id(), "selected");
    assert_eq!(result.binding.unwrap().provider_id, 7);
}

#[test]
fn test_lifecycle_binding_rejects_invalid_instance_and_allows_explicit_replacement() {
    let bindings = AgentSessionBindings::default();
    let mut invalid = binding("acp", "run"); invalid.provider_id = 0;
    assert!(bindings.bind(invalid).is_err());
    bindings.bind(binding("acp", "old")).unwrap();
    bindings.bind(binding("codex_app_server", "new")).unwrap();
    assert_eq!(bindings.get(42).unwrap().unwrap().run_id, "new");
}

#[test]
fn test_lifecycle_publication_guard_checks_current_driver_and_exact_generation() {
    let bindings = AgentSessionBindings::default();
    let calls = std::cell::Cell::new(0);
    assert!(bindings.publish_if_current(42, "acp", None, || calls.set(calls.get() + 1)).unwrap());
    bindings.bind(binding("codex_app_server", "current")).unwrap();
    for (driver, run) in [("acp", Some("current")), ("codex_app_server", Some("old")), ("codex_app_server", None)] {
        assert!(!bindings.publish_if_current(42, driver, run, || calls.set(calls.get() + 1)).unwrap());
    }
    assert_eq!(calls.get(), 1);
    assert!(bindings.publish_if_current(42, "codex_app_server", Some("current"), || calls.set(calls.get() + 1)).unwrap());
    assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn test_lifecycle_replacement_during_native_lookup_rejects_obsolete_handle() {
    let bindings = AgentSessionBindings::default();
    bindings.bind(binding("acp", "old")).unwrap();
    let result = lookup_registered_session(&bindings, 42, vec!["acp".into()], |_| async {
        bindings.bind(binding("codex_app_server", "new")).unwrap();
        Ok(Some(session("acp", "old")))
    }).await;
    assert!(result.err().unwrap().to_string().contains("binding changed"));
}

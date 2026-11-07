use super::*;
use crate::api::ai::acp::{AcpConversationSessionState, AcpPromptCapabilitiesPayload};
use crate::api::ai::claude_sdk::ClaudeConversationSessionState;
use crate::api::ai::codex_app_server::CodexConversationSessionState;
use serde_json::json;

fn option(id: &str, current: &str, values: &[&str]) -> Value {
    json!({
        "id": id, "name": id, "category": "vendor-defined", "current_value": current,
        "options": values.iter().map(|value| json!({"value": value, "name": value})).collect::<Vec<_>>(),
    })
}

fn acp() -> AgentCapabilities {
    AgentCapabilities::baseline("acp", AgentCapabilityDeclaration::acp())
}

#[test]
fn test_capabilities_baseline_distinguishes_adapter_from_negotiation() {
    let capabilities = acp();
    assert_eq!(capabilities.discovery, CapabilityDiscovery::Baseline);
    assert_eq!(capabilities.operations.cancel_prompt, Capability::known(true, CapabilitySource::Adapter));
    assert_eq!(capabilities.attachments.image.support, CapabilitySupport::Unknown);
    assert_eq!(capabilities.attachments.embedded_context.support, CapabilitySupport::Unknown);
    assert_eq!(capabilities.sessions.resume.support, CapabilitySupport::Unknown);
    assert_eq!(capabilities.configuration.source, CapabilitySource::Unknown);
    assert!(!capabilities.configuration.complete);
    assert_eq!(capabilities.attachments.audio, Capability::known(false, CapabilitySource::Adapter));
}

#[test]
fn test_capabilities_negotiated_input_requires_local_encoder_and_remote_support() {
    let capabilities = acp().with_session(&json!({
        "session_id": "opaque/session", "prompt_capabilities": {"image": true, "audio": true, "embedded_context": false},
        "load_session_supported": false, "session_resume_supported": true,
    })).unwrap();
    assert_eq!(capabilities.discovery, CapabilityDiscovery::Session);
    assert_eq!(capabilities.attachments.image, Capability::known(true, CapabilitySource::Session));
    assert_eq!(capabilities.attachments.embedded_context, Capability::known(false, CapabilitySource::Session));
    assert_eq!(capabilities.attachments.audio, Capability::known(false, CapabilitySource::Adapter));
    assert_eq!(capabilities.sessions.load.support, CapabilitySupport::Unsupported);
    assert_eq!(capabilities.sessions.resume.support, CapabilitySupport::Supported);
}

#[test]
fn test_capabilities_discovery_refresh_does_not_retain_stale_facts() {
    let live = acp().with_session(&json!({
        "session_id": "old", "prompt_capabilities": {"image": true},
        "config_options": [option("old-model", "old", &["old"])], "config_options_complete": true,
    })).unwrap();
    let newer = live.with_session(&json!({"session_id": "new"})).unwrap();
    assert_eq!(newer.attachments.image.support, CapabilitySupport::Unknown);
    assert!(newer.configuration.options.is_empty());
    assert!(!newer.configuration.complete);
    let initializing = live.with_session(&json!({
        "session_id": null, "load_session_supported": false, "config_options": [],
    })).unwrap();
    assert_eq!(initializing.discovery, CapabilityDiscovery::Baseline);
    assert_eq!(initializing.sessions.load.support, CapabilitySupport::Unknown);
    assert!(initializing.configuration.options.is_empty());
}

#[test]
fn test_capabilities_complete_empty_catalog_is_not_an_unknown_catalog() {
    let missing = acp().with_session(&json!({"session_id": "a", "config_options": []})).unwrap();
    assert!(!missing.configuration.complete);
    assert!(missing.validate_config_option("future-extension", "opaque").is_ok());
    let empty = acp().with_session(&json!({
        "session_id": "b", "config_options": [], "config_options_complete": true,
    })).unwrap();
    assert_eq!(empty.operations.set_config_option.support, CapabilitySupport::Unsupported);
    let error = empty.validate_config_option("future-extension", "opaque").unwrap_err().to_string();
    assert!(error.contains("acp set_config_option"));
    assert!(error.contains("unsupported"));
}

#[test]
fn test_capabilities_preserve_valid_opaque_config_identifiers_and_choices() {
    let id = "vendor/extensions/model:v2";
    let values = ["", " model with whitespace ", "vendor/model@2026:preview"];
    let capabilities = acp().with_session(&json!({
        "session_id": "a", "config_options": [option(id, "", &values)], "config_options_complete": true,
    })).unwrap();
    for value in values {
        assert!(capabilities.validate_config_option(id, value).is_ok());
    }
    assert!(capabilities.validate_config_option(id, "model with whitespace").is_err());
    assert!(capabilities.validate_config_option("model", "").is_err());
    assert_eq!(capabilities.configuration.options[0].options[1].value, values[1]);
}

#[test]
fn test_capabilities_dynamic_catalog_choices_replace_previous_model_choices() {
    let first = acp().with_session(&json!({
        "session_id": "same", "config_options": [option("reasoning", "high", &["high", "max"])],
        "config_options_complete": true,
    })).unwrap();
    let second = first.with_session(&json!({
        "session_id": "same", "config_options": [option("reasoning", "low", &["low", "medium"])],
        "config_options_complete": true,
    })).unwrap();
    assert!(first.validate_config_option("reasoning", "max").is_ok());
    assert!(second.validate_config_option("reasoning", "max").is_err());
    assert!(second.validate_config_option("reasoning", "medium").is_ok());
}

#[test]
fn test_capabilities_same_driver_instances_keep_independent_live_catalogs() {
    let declaration = acp();
    let a = declaration.with_session(&json!({
        "conversation_id": 1, "session_id": "provider-11/session", "config_options_complete": true,
        "config_options": [option("model", "alpha", &["alpha"])], "prompt_capabilities": {"image": true},
    })).unwrap();
    let b = declaration.with_session(&json!({
        "conversation_id": 2, "session_id": "provider-22/session", "config_options_complete": true,
        "config_options": [option("model", "beta", &["beta"])], "prompt_capabilities": {"image": false},
    })).unwrap();
    assert!(a.validate_config_option("model", "alpha").is_ok());
    assert!(a.validate_config_option("model", "beta").is_err());
    assert!(b.validate_config_option("model", "alpha").is_err());
    assert_eq!(a.attachments.image.support, CapabilitySupport::Supported);
    assert_eq!(b.attachments.image.support, CapabilitySupport::Unsupported);
    assert!(declaration.configuration.options.is_empty());
}

#[test]
fn test_capabilities_native_text_adapters_do_not_claim_structured_inputs() {
    for (driver, declaration) in [("codex_app_server", AgentCapabilityDeclaration::codex()), ("claude_sdk", AgentCapabilityDeclaration::claude())] {
        let capabilities = AgentCapabilities::baseline(driver, declaration).with_session(&json!({
            "session_id": "s", "prompt_capabilities": {"image": true, "audio": true, "embedded_context": true},
            "load_session_supported": false, "session_resume_supported": true,
        })).unwrap();
        assert_eq!(capabilities.attachments.input, AttachmentInput::PromptText);
        assert_eq!(capabilities.attachments.structured.support, CapabilitySupport::Unsupported);
        assert_eq!(capabilities.attachments.image.support, CapabilitySupport::Unsupported);
        assert_eq!(capabilities.attachments.audio.support, CapabilitySupport::Unsupported);
        assert_eq!(capabilities.sessions.resume.support, CapabilitySupport::Supported);
        assert_eq!(capabilities.operations.respond_permission.support, CapabilitySupport::Supported);
    }
}

#[test]
fn test_capabilities_custom_driver_needs_no_builtin_kind_branch() {
    let custom = AgentCapabilities::baseline("future-driver", AgentCapabilityDeclaration {
        respond_permission: Some(false), ..AgentCapabilityDeclaration::unknown(false, false, false)
    });
    for operation in [AgentOperation::CancelPrompt, AgentOperation::SetConfigOption, AgentOperation::RespondPermission] {
        let error = custom.require_operation(operation).unwrap_err().to_string();
        assert!(error.contains("future-driver"));
        assert!(error.contains(operation.name()));
        assert!(error.contains("unsupported"));
    }
    assert!(custom.require_operation(AgentOperation::SendPrompt).is_ok());
    assert_eq!(custom.sessions.resume.support, CapabilitySupport::Unknown);
}

#[test]
fn test_capabilities_unknown_permission_is_not_claimed_as_supported() {
    let custom = AgentCapabilities::baseline("custom", AgentCapabilityDeclaration::unknown(true, true, true));
    assert_eq!(custom.operations.respond_permission.support, CapabilitySupport::Unknown);
    let live = custom.with_session(&json!({"session_id": "s", "prompt_capabilities": {"image": true}})).unwrap();
    assert_eq!(live.attachments.image.support, CapabilitySupport::Unknown);
}

#[test]
fn test_capabilities_malformed_live_data_returns_specific_error() {
    for (snapshot, field) in [
        (json!([]), "snapshot"),
        (json!({"session_id": 42}), "session_id"),
        (json!({"session_id": "s", "prompt_capabilities": []}), "prompt_capabilities"),
        (json!({"session_id": "s", "prompt_capabilities": {"image": "yes"}}), "image"),
        (json!({"session_id": "s", "config_options": [{}]}), "config_options"),
    ] {
        let error = acp().with_session(&snapshot).unwrap_err().to_string();
        assert!(error.contains("acp capability discovery failed"));
        assert!(error.contains(field));
    }
}

#[test]
fn test_capabilities_additive_projection_preserves_all_native_legacy_fields() {
    let snapshots = [
        ("acp", AgentCapabilityDeclaration::acp(), serde_json::to_value(AcpConversationSessionState {
            conversation_id: 42, session_id: Some("acp-session".into()),
            prompt_capabilities: AcpPromptCapabilitiesPayload { image: true, audio: false, embedded_context: true },
            ..Default::default()
        }).unwrap()),
        ("codex_app_server", AgentCapabilityDeclaration::codex(), serde_json::to_value(CodexConversationSessionState {
            conversation_id: 43, session_id: Some("codex-thread".into()), model: Some("actual-model".into()),
            session_resume_supported: true, ..Default::default()
        }).unwrap()),
        ("claude_sdk", AgentCapabilityDeclaration::claude(), serde_json::to_value(ClaudeConversationSessionState {
            conversation_id: 44, session_id: Some("claude-session".into()), permission_mode: Some("plan".into()),
            session_resume_supported: true, ..Default::default()
        }).unwrap()),
    ];
    for (driver, declaration, snapshot) in snapshots {
        let enriched = AgentCapabilities::baseline(driver, declaration).enrich_snapshot(snapshot.clone()).unwrap();
        for (key, value) in snapshot.as_object().unwrap() {
            assert_eq!(&enriched[key], value, "legacy field {key} changed for {driver}");
        }
        assert_eq!(enriched["agent_capabilities"]["schema_version"], 1);
        assert_eq!(enriched["agent_capabilities"]["driver_id"], driver);
    }
}


#[test]
fn test_capabilities_snapshot_events_preserve_clear_and_live_legacy_payloads() {
    let cleared = snapshot_event_payload::<AcpConversationSessionState>(
        "acp", AgentCapabilityDeclaration::acp(), None,
    ).unwrap();
    assert_eq!(cleared, json!({"state": null}));
    let state = AcpConversationSessionState {
        conversation_id: 9, session_id: Some("active".into()), ..Default::default()
    };
    let payload = snapshot_event_payload("acp", AgentCapabilityDeclaration::acp(), Some(&state)).unwrap();
    assert_eq!(payload["state"]["conversation_id"], 9);
    assert_eq!(payload["state"]["session_id"], "active");
    assert_eq!(payload["state"]["agent_capabilities"]["discovery"], "session");
    assert_eq!(payload["state"]["agent_capabilities"]["configuration"]["complete"], false);
}

#[test]
fn test_capabilities_custom_discovery_attachment_does_not_reproject_overrides() {
    let mut discovered = AgentCapabilities::baseline(
        "custom", AgentCapabilityDeclaration::unknown(true, true, false),
    );
    discovered.operations.cancel_prompt = Capability::known(false, CapabilitySource::Session);
    let wire = discovered.attach_to_snapshot(json!({"session_id": "custom-session"})).unwrap();
    assert_eq!(wire["agent_capabilities"]["operations"]["cancel_prompt"]["support"], "unsupported");
    assert_eq!(wire["agent_capabilities"]["operations"]["cancel_prompt"]["source"], "session");
}

#[test]
fn test_capabilities_legacy_acp_snapshot_defaults_catalog_provenance_to_unknown() {
    let mut legacy = serde_json::to_value(AcpConversationSessionState {
        session_id: Some("legacy".into()), ..Default::default()
    }).unwrap();
    legacy.as_object_mut().unwrap().remove("config_options_complete");
    let state: AcpConversationSessionState = serde_json::from_value(legacy).unwrap();
    assert!(!state.config_options_complete);
    let capabilities = acp().with_session(&serde_json::to_value(state).unwrap()).unwrap();
    assert!(!capabilities.configuration.complete);
    assert!(capabilities.validate_config_option("server-extension", " exact ").is_ok());
}


#[test]
fn test_capabilities_partial_catalog_does_not_block_unrepresented_extensions() {
    let capabilities = acp().with_session(&json!({
        "session_id": "s", "config_options_complete": false,
        "config_options": [option("represented", "current", &["current", " exact "])],
    })).unwrap();
    assert!(capabilities.validate_config_option("vendor/new-config-kind", "opaque/choice").is_ok());
    assert!(capabilities.validate_config_option("represented", " exact ").is_ok());
    assert!(!capabilities.configuration.complete);
}

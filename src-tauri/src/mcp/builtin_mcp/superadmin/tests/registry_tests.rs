use crate::mcp::builtin_mcp::superadmin::registry::build_registry;
use crate::mcp::builtin_mcp::superadmin::types::*;

#[test]
fn registry_builds_without_panic() {
    let registry = build_registry();
    assert!(registry.action_count() > 0, "Registry should have at least one action");
}

#[test]
fn all_four_domains_registered() {
    let registry = build_registry();
    let domains = registry.domains();
    assert!(domains.contains(&"assistant".to_string()), "Missing assistant domain");
    assert!(domains.contains(&"conversation".to_string()), "Missing conversation domain");
    assert!(domains.contains(&"task".to_string()), "Missing task domain");
    assert!(domains.contains(&"schedule".to_string()), "Missing schedule domain");
}

#[test]
fn get_existing_action() {
    let registry = build_registry();
    let meta = registry.get_meta("assistant.list");
    assert!(meta.is_some(), "assistant.list should exist");
    let meta = meta.unwrap();
    assert_eq!(meta.domain, "assistant");
    assert_eq!(meta.risk_level, RiskLevel::SAFE);
}

#[test]
fn test_assistant_reset_conversation_discoverable_by_purpose() {
    let registry = build_registry();
    let results = registry.search(Some("重开"), Some("assistant"), None, None);
    let reset = results.iter().find(|m| m.action_id == "assistant.reset_conversation").unwrap();
    assert_eq!(reset.args_schema["required"], serde_json::json!(["assistant_id"]));
    assert_eq!(reset.allowed_scopes.len(), 1);
    assert!(matches!(reset.allowed_scopes[0], ActionScope::Assistant));
    assert!(reset.supports_dry_run);
}

#[test]
fn get_nonexistent_action() {
    let registry = build_registry();
    assert!(registry.get_meta("does_not_exist").is_none());
}

#[test]
fn search_by_domain() {
    let registry = build_registry();
    let results = registry.search(None, Some("schedule"), None, None);
    assert!(results.len() >= 4, "schedule domain should have at least 4 actions");
    for meta in &results {
        assert_eq!(meta.domain, "schedule");
    }
}

#[test]
fn search_by_tag() {
    let registry = build_registry();
    let results = registry.search(None, None, Some("read"), None);
    assert!(!results.is_empty(), "Should find actions tagged 'read'");
    for meta in &results {
        assert!(meta.tags.contains(&"read".to_string()));
    }
}

#[test]
fn search_by_risk_level() {
    let registry = build_registry();
    let results = registry.search(None, None, None, Some(0));
    assert!(!results.is_empty(), "Should find safe (risk=0) actions");
    for meta in &results {
        assert_eq!(meta.risk_level, RiskLevel::SAFE);
    }
}

#[test]
fn search_by_query_keyword() {
    let registry = build_registry();
    let results = registry.search(Some("助手"), None, None, None);
    assert!(!results.is_empty(), "Should find actions matching '助手'");
}

#[test]
fn search_combined_filters() {
    let registry = build_registry();
    let results = registry.search(None, Some("assistant"), Some("read"), None);
    for meta in &results {
        assert_eq!(meta.domain, "assistant");
        assert!(meta.tags.contains(&"read".to_string()));
    }
}

#[test]
fn all_metas_sorted_by_action_id() {
    let registry = build_registry();
    let metas = registry.all_metas();
    for window in metas.windows(2) {
        assert!(
            window[0].action_id <= window[1].action_id,
            "all_metas should be sorted: {} > {}",
            window[0].action_id,
            window[1].action_id
        );
    }
}

#[test]
fn action_meta_completeness() {
    let registry = build_registry();
    for meta in registry.all_metas() {
        assert!(!meta.action_id.is_empty(), "action_id must not be empty");
        assert!(!meta.domain.is_empty(), "domain must not be empty for {}", meta.action_id);
        assert!(!meta.summary.is_empty(), "summary must not be empty for {}", meta.action_id);
        assert!(
            meta.action_id.starts_with(&meta.domain),
            "action_id '{}' should start with domain '{}'",
            meta.action_id,
            meta.domain
        );
        assert!(meta.risk_level.0 <= 3, "risk_level out of range for {}", meta.action_id);
    }
}

#[test]
fn conversation_delete_is_explicit_and_needs_user_approval() {
    let registry = build_registry();
    assert!(registry.get("conversation.archive").is_none(), "the misleading archive name must not be offered");
    let meta = registry.get_meta("conversation.delete").expect("conversation.delete registered");
    assert!(meta.requires_approval);
    assert_eq!(meta.approval_policy, ApprovalPolicy::UserApprovalRequired);
    assert!(meta.rollback_hint.is_none(), "deletion is not undoable");
    assert!(crate::mcp::builtin_mcp::superadmin::action_approval_gate("conversation.delete", false).is_some());
    assert!(crate::mcp::builtin_mcp::superadmin::action_approval_gate("conversation.delete", true).is_none());
}

#[test]
fn conversation_delete_refuses_kinds_with_their_own_lifecycle() {
    use crate::mcp::builtin_mcp::superadmin::actions::conversation::deletion_refusal;
    for kind in ["normal", "butler_task", "butler_contact_archive"] {
        assert!(deletion_refusal(kind).is_none(), "{kind} should be deletable");
    }
    assert!(deletion_refusal("butler_contact").unwrap().contains("assistant.reset_conversation"));
    for kind in ["butler_main", "butler_group", "butler_group_member", "scheduled_run", "butler_proactive_run", "onboarding_probe", ""] {
        assert!(deletion_refusal(kind).is_some(), "{kind} must be refused");
    }
}

#[test]
fn catalog_only_lists_executable_actions_and_hints_name_real_ones() {
    let registry = build_registry();
    assert!(registry.get("task.spawn").is_none(), "non-executable actions must not be listed");
    let action_ref = regex::Regex::new(r"\b([a-z]+\.[a-z_]+)\b").unwrap();
    for meta in registry.all_metas() {
        let Some(hint) = &meta.rollback_hint else { continue };
        for name in action_ref.captures_iter(hint).map(|c| c[1].to_string()) {
            assert!(registry.get(&name).is_some(), "{} rollback_hint names missing action {name}", meta.action_id);
        }
    }
}

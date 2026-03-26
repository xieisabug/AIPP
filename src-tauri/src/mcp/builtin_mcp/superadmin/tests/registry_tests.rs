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

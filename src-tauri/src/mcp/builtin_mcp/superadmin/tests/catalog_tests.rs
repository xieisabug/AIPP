use crate::mcp::builtin_mcp::superadmin::catalog::handle_catalog;
use crate::mcp::builtin_mcp::superadmin::inspect::handle_inspect;
use crate::mcp::builtin_mcp::superadmin::registry::build_registry;
use crate::mcp::builtin_mcp::superadmin::types::*;

#[test]
fn catalog_returns_all_when_no_filters() {
    let registry = build_registry();
    let request = CatalogRequest {
        query: None,
        domain: None,
        tag: None,
        risk_level: None,
        detail_level: None,
        limit: None,
        cursor: None,
    };
    let response = handle_catalog(&registry, &request);
    assert!(response.total > 0, "catalog should return actions");
    assert_eq!(response.items.len(), response.total.min(20)); // default limit=20
}

#[test]
fn catalog_pagination() {
    let registry = build_registry();
    let request = CatalogRequest {
        query: None,
        domain: None,
        tag: None,
        risk_level: None,
        detail_level: None,
        limit: Some(3),
        cursor: None,
    };
    let first_page = handle_catalog(&registry, &request);
    assert_eq!(first_page.items.len(), 3);

    if first_page.total > 3 {
        assert!(first_page.next_cursor.is_some());
        let second_request = CatalogRequest {
            cursor: first_page.next_cursor,
            limit: Some(3),
            ..request.clone()
        };
        let second_page = handle_catalog(&registry, &second_request);
        assert!(!second_page.items.is_empty());
        // No overlap between pages
        assert_ne!(first_page.items[0].action_id, second_page.items[0].action_id);
    }
}

#[test]
fn catalog_domain_filter() {
    let registry = build_registry();
    let request = CatalogRequest {
        query: None,
        domain: Some("assistant".into()),
        tag: None,
        risk_level: None,
        detail_level: None,
        limit: Some(100),
        cursor: None,
    };
    let response = handle_catalog(&registry, &request);
    assert!(response.total > 0);
    for item in &response.items {
        assert_eq!(item.domain, "assistant");
    }
}

#[test]
fn catalog_risk_filter() {
    let registry = build_registry();
    let request = CatalogRequest {
        query: None,
        domain: None,
        tag: None,
        risk_level: Some(2),
        detail_level: None,
        limit: Some(100),
        cursor: None,
    };
    let response = handle_catalog(&registry, &request);
    for item in &response.items {
        assert_eq!(item.risk_level, RiskLevel::MEDIUM);
    }
}

#[test]
fn inspect_existing_action() {
    let registry = build_registry();
    let result = handle_inspect(&registry, "assistant.list");
    assert!(result.is_ok());
    let info = result.unwrap();
    assert_eq!(info.action_id, "assistant.list");
    assert_eq!(info.domain, "assistant");
    assert!(!info.summary.is_empty());
    // Verify schema is an object
    assert!(info.args_schema.is_object());
}

#[test]
fn inspect_nonexistent_action() {
    let registry = build_registry();
    let result = handle_inspect(&registry, "no.such.action");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

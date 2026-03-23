use rusqlite::Connection;

use crate::mcp::builtin_mcp::superadmin::audit::*;
use crate::mcp::builtin_mcp::superadmin::types::RiskLevel;

fn setup_in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    create_audit_table(&conn).expect("create audit table");
    conn
}

#[test]
fn create_table_idempotent() {
    let conn = setup_in_memory_db();
    // Second call should not error
    create_audit_table(&conn).expect("create audit table again");
}

#[test]
fn insert_and_query_audit_log() {
    let conn = setup_in_memory_db();
    let audit_id = generate_audit_id();

    let row_id = insert_audit_log(
        &conn,
        &audit_id,
        "assistant.list",
        "assistant",
        RiskLevel::SAFE,
        Some("{}"),
        Some("test reason"),
        false,
        false,
        true,
        Some("{\"count\":5}"),
        None,
        Some(42),
        "butler",
    )
    .expect("insert should succeed");

    assert!(row_id > 0);

    let entries = query_audit_log(&conn, Some("assistant.list"), None, 10)
        .expect("query should succeed");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.audit_id, audit_id);
    assert_eq!(entry.action_id, "assistant.list");
    assert_eq!(entry.domain, "assistant");
    assert_eq!(entry.risk_level, 0);
    assert!(entry.success);
    assert!(!entry.dry_run);
    assert!(!entry.approval_used);
    assert_eq!(entry.butler_conversation_id, Some(42));
    assert_eq!(entry.source, "butler");
}

#[test]
fn query_by_domain() {
    let conn = setup_in_memory_db();

    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "assistant.list",
        "assistant",
        RiskLevel::SAFE,
        None, None, false, false, true, None, None, None, "butler",
    )
    .unwrap();

    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "schedule.list",
        "schedule",
        RiskLevel::SAFE,
        None, None, false, false, true, None, None, None, "butler",
    )
    .unwrap();

    let assistant_entries = query_audit_log(&conn, None, Some("assistant"), 10).unwrap();
    assert_eq!(assistant_entries.len(), 1);
    assert_eq!(assistant_entries[0].domain, "assistant");

    let all_entries = query_audit_log(&conn, None, None, 10).unwrap();
    assert_eq!(all_entries.len(), 2);
}

#[test]
fn query_respects_limit() {
    let conn = setup_in_memory_db();

    for i in 0..5 {
        insert_audit_log(
            &conn,
            &generate_audit_id(),
            &format!("test.action_{}", i),
            "test",
            RiskLevel::LOW,
            None, None, false, false, true, None, None, None, "butler",
        )
        .unwrap();
    }

    let entries = query_audit_log(&conn, None, None, 3).unwrap();
    assert_eq!(entries.len(), 3);
}

#[test]
fn audit_id_uniqueness() {
    let id1 = generate_audit_id();
    let id2 = generate_audit_id();
    assert_ne!(id1, id2);
}

#[test]
fn insert_failed_action() {
    let conn = setup_in_memory_db();

    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "schedule.delete",
        "schedule",
        RiskLevel::HIGH,
        Some("{\"task_id\":1}"),
        None,
        false,
        false,
        false,
        None,
        Some("Approval required"),
        Some(10),
        "butler",
    )
    .unwrap();

    let entries = query_audit_log(&conn, Some("schedule.delete"), None, 10).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].success);
    assert_eq!(entries[0].error.as_deref(), Some("Approval required"));
}

#[test]
fn insert_dry_run() {
    let conn = setup_in_memory_db();

    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "assistant.create",
        "assistant",
        RiskLevel::LOW,
        Some("{\"name\":\"test\"}"),
        None,
        true,
        false,
        true,
        Some("{\"dry_run\":true}"),
        None,
        None,
        "butler",
    )
    .unwrap();

    let entries = query_audit_log(&conn, Some("assistant.create"), None, 10).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].dry_run);
}

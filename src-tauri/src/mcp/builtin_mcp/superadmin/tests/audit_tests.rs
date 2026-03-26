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
        None,
    )
    .expect("insert should succeed");

    assert!(row_id > 0);

    let entries = query_audit_log(&conn, Some("assistant.list"), None, None, false, 10, 0)
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
    assert!(entry.before_snapshot_json.is_none());
    assert!(!entry.is_undone);
    assert!(entry.undo_audit_id.is_none());
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
        None,
        None,
        false,
        false,
        true,
        None,
        None,
        None,
        "butler",
        None,
    )
    .unwrap();

    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "schedule.list",
        "schedule",
        RiskLevel::SAFE,
        None,
        None,
        false,
        false,
        true,
        None,
        None,
        None,
        "butler",
        None,
    )
    .unwrap();

    let assistant_entries =
        query_audit_log(&conn, None, Some("assistant"), None, false, 10, 0).unwrap();
    assert_eq!(assistant_entries.len(), 1);
    assert_eq!(assistant_entries[0].domain, "assistant");

    let all_entries = query_audit_log(&conn, None, None, None, false, 10, 0).unwrap();
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
            None,
            None,
            false,
            false,
            true,
            None,
            None,
            None,
            "butler",
            None,
        )
        .unwrap();
    }

    let entries = query_audit_log(&conn, None, None, None, false, 3, 0).unwrap();
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
        None,
    )
    .unwrap();

    let entries =
        query_audit_log(&conn, Some("schedule.delete"), None, None, false, 10, 0).unwrap();
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
        None,
    )
    .unwrap();

    let entries =
        query_audit_log(&conn, Some("assistant.create"), None, None, false, 10, 0).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].dry_run);
}

// ---- New tests for snapshot/undo features ----

#[test]
fn insert_with_snapshot() {
    let conn = setup_in_memory_db();
    let audit_id = generate_audit_id();

    insert_audit_log(
        &conn,
        &audit_id,
        "assistant.update_prompt",
        "assistant",
        RiskLevel::MEDIUM,
        Some("{\"assistant_id\":1,\"prompt\":\"new\"}"),
        Some("updating prompt"),
        false,
        false,
        true,
        Some("{\"updated\":true}"),
        None,
        Some(10),
        "butler",
        Some("{\"_type\":\"assistant.update_prompt\",\"assistant_id\":1,\"old_prompt\":\"old\"}"),
    )
    .unwrap();

    let entry = get_audit_entry(&conn, &audit_id).unwrap().expect("should find entry");
    assert_eq!(
        entry.before_snapshot_json.as_deref(),
        Some("{\"_type\":\"assistant.update_prompt\",\"assistant_id\":1,\"old_prompt\":\"old\"}")
    );
    assert!(!entry.is_undone);
    assert!(entry.undo_audit_id.is_none());
}

#[test]
fn mark_undone_and_verify() {
    let conn = setup_in_memory_db();
    let audit_id = generate_audit_id();
    let undo_id = generate_audit_id();

    insert_audit_log(
        &conn,
        &audit_id,
        "assistant.update_prompt",
        "assistant",
        RiskLevel::MEDIUM,
        None,
        None,
        false,
        false,
        true,
        None,
        None,
        None,
        "butler",
        Some("{\"old_prompt\":\"hello\"}"),
    )
    .unwrap();

    mark_audit_undone(&conn, &audit_id, &undo_id).unwrap();

    let entry = get_audit_entry(&conn, &audit_id).unwrap().expect("should exist");
    assert!(entry.is_undone);
    assert_eq!(entry.undo_audit_id.as_deref(), Some(undo_id.as_str()));
}

#[test]
fn query_undoable_only() {
    let conn = setup_in_memory_db();

    // Undoable: success, not dry_run, has snapshot, not undone
    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "assistant.update_prompt",
        "assistant",
        RiskLevel::MEDIUM,
        None,
        None,
        false,
        false,
        true,
        None,
        None,
        None,
        "butler",
        Some("{\"snapshot\":true}"),
    )
    .unwrap();

    // Not undoable: no snapshot
    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "assistant.list",
        "assistant",
        RiskLevel::SAFE,
        None,
        None,
        false,
        false,
        true,
        None,
        None,
        None,
        "butler",
        None,
    )
    .unwrap();

    // Not undoable: dry_run
    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "assistant.update_model",
        "assistant",
        RiskLevel::MEDIUM,
        None,
        None,
        true,
        false,
        true,
        None,
        None,
        None,
        "butler",
        Some("{\"snapshot\":true}"),
    )
    .unwrap();

    // Not undoable: failed
    insert_audit_log(
        &conn,
        &generate_audit_id(),
        "assistant.create",
        "assistant",
        RiskLevel::LOW,
        None,
        None,
        false,
        false,
        false,
        None,
        Some("err"),
        None,
        "butler",
        Some("{\"snapshot\":true}"),
    )
    .unwrap();

    let undoable = query_audit_log(&conn, None, None, None, true, 50, 0).unwrap();
    assert_eq!(undoable.len(), 1);
    assert_eq!(undoable[0].action_id, "assistant.update_prompt");

    let all = query_audit_log(&conn, None, None, None, false, 50, 0).unwrap();
    assert_eq!(all.len(), 4);
}

#[test]
fn migrate_audit_table_is_idempotent() {
    let conn = setup_in_memory_db();
    // Table already has the columns from create_audit_table
    migrate_audit_table(&conn).expect("migrate should succeed");
    migrate_audit_table(&conn).expect("second migrate should also succeed");
}

#[test]
fn get_nonexistent_audit_entry() {
    let conn = setup_in_memory_db();
    let result = get_audit_entry(&conn, "nonexistent-id").unwrap();
    assert!(result.is_none());
}

#[test]
fn query_with_offset() {
    let conn = setup_in_memory_db();

    for i in 0..5 {
        insert_audit_log(
            &conn,
            &generate_audit_id(),
            &format!("test.action_{}", i),
            "test",
            RiskLevel::LOW,
            None,
            None,
            false,
            false,
            true,
            None,
            None,
            None,
            "butler",
            None,
        )
        .unwrap();
    }

    let page1 = query_audit_log(&conn, None, None, None, false, 2, 0).unwrap();
    assert_eq!(page1.len(), 2);

    let page2 = query_audit_log(&conn, None, None, None, false, 2, 2).unwrap();
    assert_eq!(page2.len(), 2);

    let page3 = query_audit_log(&conn, None, None, None, false, 2, 4).unwrap();
    assert_eq!(page3.len(), 1);
}

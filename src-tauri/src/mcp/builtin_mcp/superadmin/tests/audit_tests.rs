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

/// Credentials are removed before the database write, not just while returning this tool call.
/// Query again outside the invocation to model a later superadmin_audit_query without vault access.
#[tokio::test]
async fn test_audit_redacts_credentials_before_persistence_and_later_query() {
    use base64::Engine;
    use crate::vault::placeholder;
    use serde_json::json;

    let conn = setup_in_memory_db();
    let secret = "audit-secret-\"quoted\\path /?=+中文";
    let encoded = base64::engine::general_purpose::STANDARD.encode(secret);
    let url_encoded = urlencoding::encode(secret).into_owned();
    let args = json!({"payload": secret, "nested": [{(secret): encoded}]}).to_string();
    let result = json!({"response": url_encoded, "debug": json!({"value": secret}).to_string()}).to_string();
    let reason = format!("Updating with {secret}");
    let snapshot = json!({"old_value": secret}).to_string();
    placeholder::test_prepared(&[secret]).run(async {
        insert_audit_log(&conn, "vault-success", "config.set", "config", RiskLevel::MEDIUM,
            Some(&args), Some(&reason), false, true, true, Some(&result), None,
            Some(42), "butler", Some(&snapshot)).unwrap();
    }).await;
    assert!(placeholder::current_redactor().is_none());

    // Check raw stored columns as well as the public query path. No cleanup is active here.
    let stored: (String, String, String, Option<String>) = conn.query_row(
        "SELECT args_json, reason, result_json, before_snapshot_json FROM superadmin_audit_log WHERE audit_id = 'vault-success'",
        (), |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    let stored_args: serde_json::Value = serde_json::from_str(&stored.0).unwrap();
    assert_eq!(stored_args["payload"], "[REDACTED]");
    assert_eq!(stored_args["nested"][0]["[REDACTED]"], "[REDACTED]");
    let stored_result: serde_json::Value = serde_json::from_str(&stored.2).unwrap();
    assert_eq!(stored_result["response"], "[REDACTED]");
    let embedded: serde_json::Value = serde_json::from_str(stored_result["debug"].as_str().unwrap()).unwrap();
    assert_eq!(embedded["value"], "[REDACTED]");
    assert!(stored.3.is_none(), "redacted undo inputs must not be replayable");
    assert_eq!(stored.1, "Updating with [REDACTED]");

    let entries = query_audit_log(&conn, None, None, None, false, 20, 0).unwrap();
    let retained = serde_json::to_string(&entries).unwrap();
    for sensitive in [secret.to_owned(), encoded, url_encoded] {
        assert!(!retained.contains(&sensitive));
    }
    assert!(query_audit_log(&conn, None, None, None, true, 20, 0).unwrap().is_empty());
}

#[tokio::test]
async fn test_audit_redacts_failure_reason_and_error_before_storage() {
    use crate::vault::placeholder;

    let conn = setup_in_memory_db();
    let secret = "error-token-56789";
    let error = format!("HTTP 401 for request req-42: invalid credential {secret}");
    placeholder::test_prepared(&[secret]).run(async {
        // Includes the shared failed-undo write path and malformed JSON fallback.
        insert_audit_log(&conn, "vault-failure", "undo:config.set", "config", RiskLevel::MEDIUM,
            Some(secret), Some(secret), false, false, false, None, Some(&error),
            Some(42), "butler", None).unwrap();
    }).await;
    let entry = get_audit_entry(&conn, "vault-failure").unwrap().unwrap();
    assert_eq!(entry.args_json.as_deref(), Some("[REDACTED]"));
    assert_eq!(entry.reason.as_deref(), Some("[REDACTED]"));
    assert_eq!(entry.error.as_deref(), Some("HTTP 401 for request req-42: invalid credential [REDACTED]"));
}

/// Sanitizing only one of the two undo inputs is enough to disable replay; result-only cleanup is
/// not, because safe original arguments and snapshots can still restore the original state.
#[tokio::test]
async fn test_audit_disables_only_undo_with_redacted_inputs() {
    use crate::vault::placeholder;

    let conn = setup_in_memory_db();
    let secret = "undo-token-98765";
    let sensitive = serde_json::json!({"value": secret}).to_string();
    let safe = "{ \"value\": \"unchanged\" }";
    placeholder::test_prepared(&[secret]).run(async {
        for (id, args, snapshot, result) in [
            ("args-only", sensitive.as_str(), safe, safe),
            ("snapshot-only", safe, sensitive.as_str(), safe),
            ("result-only", safe, safe, sensitive.as_str()),
            ("untouched", safe, safe, safe),
        ] {
            insert_audit_log(&conn, id, "config.set", "config", RiskLevel::MEDIUM,
                Some(args), None, false, false, true, Some(result), None,
                Some(42), "butler", Some(snapshot)).unwrap();
        }
    }).await;
    for id in ["args-only", "snapshot-only"] {
        assert!(get_audit_entry(&conn, id).unwrap().unwrap().before_snapshot_json.is_none());
    }
    for id in ["result-only", "untouched"] {
        let entry = get_audit_entry(&conn, id).unwrap().unwrap();
        assert_eq!(entry.args_json.as_deref(), Some(safe));
        assert_eq!(entry.before_snapshot_json.as_deref(), Some(safe));
    }
    assert_eq!(query_audit_log(&conn, None, None, None, true, 20, 0).unwrap().len(), 2);
}

/// Short values and JSON scalars must not enter the audit store, but their ambiguous spelling must
/// not trigger a new global replacement policy for ordinary tool messages.
#[tokio::test]
async fn test_audit_omits_payload_for_short_or_json_credentials_without_global_redaction() {
    use crate::vault::placeholder;

    let conn = setup_in_memory_db();
    for secret in ["ab", "123", "true", "123456", r#"{"key":"value"}"#] {
        let audit_id = generate_audit_id();
        let original = serde_json::json!({"value": secret, "ordinary_flag": true}).to_string();
        placeholder::test_prepared(&[secret]).run(async {
            assert!(placeholder::audit_requires_opaque_values());
            placeholder::Prepared::default().run(async {
                assert!(!placeholder::audit_requires_opaque_values());
            }).await;
            assert!(placeholder::audit_requires_opaque_values());
            if secret == "ab" {
                // Retain the existing short-value policy outside persistent audit records.
                assert_eq!(placeholder::current_redactor().unwrap().clean("table ab"), "table ab");
            }
            insert_audit_log(&conn, &audit_id, "config.set", "config", RiskLevel::MEDIUM,
                Some(&original), Some(secret), false, true, false, Some(&original), Some(secret),
                Some(42), "butler", Some(&original)).unwrap();
        }).await;
        assert!(!placeholder::audit_requires_opaque_values());
        let entry = get_audit_entry(&conn, &audit_id).unwrap().unwrap();
        assert!(entry.args_json.is_none());
        assert!(entry.reason.is_none());
        assert!(entry.result_json.is_none());
        assert!(entry.error.is_none());
        assert!(entry.before_snapshot_json.is_none());
        assert_eq!(entry.action_id, "config.set");
        assert!(!entry.success);
    }
}

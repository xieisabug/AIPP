use super::*;

fn sync_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SYNC_SCHEMA_SQL).unwrap();
    conn
}

fn change(kind: &str, id: &str) -> PullChange {
    serde_json::from_value(json!({
        "seq": 1, "event_id": format!("event-{id}"), "device_id": "remote",
        "object_type": kind, "object_id": id, "operation": "upsert", "version": 1,
        "payload": {"fields": {"value": "remote"}, "refs": {}},
        "deleted_at": null, "created_at": "now"
    })).unwrap()
}

#[test]
fn test_sync_ui_refresh_all_domains_and_unknown_types() {
    for (kind, expected) in [
        ("assistant", SyncUiDomains { assistant: true, ..Default::default() }),
        ("assistant.model", SyncUiDomains { assistant: true, ..Default::default() }),
        ("conversation", SyncUiDomains { conversation: true, ..Default::default() }),
        ("conversation.message_attachment", SyncUiDomains { conversation: true, ..Default::default() }),
        ("conversation.summary", SyncUiDomains { conversation: true, ..Default::default() }),
        ("conversation.todo", SyncUiDomains { conversation: true, ..Default::default() }),
        ("llm.model", SyncUiDomains { llm: true, ..Default::default() }),
        ("mcp.server_tool", SyncUiDomains { mcp: true, ..Default::default() }),
        ("system.feature_config", SyncUiDomains { feature: true, ..Default::default() }),
        ("artifacts.collection", SyncUiDomains { artifacts: true, ..Default::default() }),
        ("unknown", SyncUiDomains::default()),
    ] {
        assert_eq!(SyncUiDomains::from_types(&HashSet::from([kind.into()])), expected, "{kind}");
    }
}

#[test]
fn test_sync_ui_refresh_ignores_failed_and_duplicate_applications() {
    let conn = sync_db();
    let event = change("conversation.message", "message-1");
    let mut applied = HashSet::new();
    assert!(apply_and_record(&conn, &event, &mut applied, || Err("缺少同步依赖".into())).is_err());
    assert!(applied.is_empty());
    apply_and_record(&conn, &event, &mut applied, || upsert_shadow_from_remote(&conn, &event)).unwrap();
    assert_eq!(applied, HashSet::from(["conversation.message".into()]));
    applied.clear();
    apply_and_record(&conn, &event, &mut applied, || Ok(())).unwrap();
    assert!(applied.is_empty());
}

#[test]
fn test_sync_ui_refresh_keeps_committed_domains_after_page_failure() {
    let conn = sync_db();
    conn.execute_batch("CREATE TRIGGER reject_dead_letter BEFORE INSERT ON sync_dead_letter
        BEGIN SELECT RAISE(ABORT, 'cannot persist failed row'); END;").unwrap();
    let events = [change("assistant", "a"), change("conversation.message", "m")];
    let mut applied = HashSet::new();
    let result = apply_pull_page(&conn, &events, |event| {
        apply_and_record(&conn, event, &mut applied, || {
            if event.object_type == "assistant" { upsert_shadow_from_remote(&conn, event) }
            else { Err("invalid message".into()) }
        })
    });
    assert!(result.is_err());
    assert_eq!(applied, HashSet::from(["assistant".into()]));
}

#[test]
fn test_sync_ui_refresh_records_successful_dead_letter_replays_only() {
    let conn = sync_db();
    let events = [change("artifacts.collection", "a"), change("conversation.todo", "t")];
    for event in &events { record_dead_letter(&conn, event, "pending").unwrap(); }
    let mut applied = HashSet::new();
    let counts = replay_dead_letter_rows(&conn, |event| {
        apply_and_record(&conn, event, &mut applied, || {
            if event.object_type == "artifacts.collection" { upsert_shadow_from_remote(&conn, event) }
            else { Err("缺少同步依赖 todo owner".into()) }
        })
    }).unwrap();
    assert_eq!(counts, (1, 1));
    assert_eq!(applied, HashSet::from(["artifacts.collection".into()]));
}

#[test]
fn test_sync_ui_refresh_invalidates_todo_reader_cache_without_writing() {
    use crate::mcp::builtin_mcp::agent::todo::types::{TodoItem, TodoStatus};
    let state = crate::mcp::builtin_mcp::TodoState::new();
    state.set_todos(7, vec![TodoItem {
        content: "old".into(), status: TodoStatus::Pending, active_form: "working".into(),
    }]);
    assert_eq!(state.get_todos(7).len(), 1);
    state.invalidate_cache();
    assert!(state.get_todos(7).is_empty());
}

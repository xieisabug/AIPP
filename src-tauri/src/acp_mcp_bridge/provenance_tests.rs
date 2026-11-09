use super::*;

#[test]
fn test_native_mcp_missing_idle_and_wrong_runtime_fail_closed() {
    let scope = NativeMcpScope::new(1).unwrap();
    assert!(capture("unknown", 1).is_err());
    assert!(capture(scope.token(), 1).is_err());
    let _prompt = scope.begin_prompt(10).unwrap();
    assert!(capture(scope.token(), 2).is_err());
    assert_eq!(capture(scope.token(), 1).unwrap().source_message_id(), 10);
    let other = NativeMcpScope::new(1).unwrap();
    assert!(capture(other.token(), 1).is_err());
    assert!(scope.begin_prompt(11).is_err(), "overlapping native turns must not relabel tools");
}

#[test]
fn test_native_mcp_captured_request_keeps_original_source_after_next_turn() {
    let scope = NativeMcpScope::new(1).unwrap();
    let first = scope.begin_prompt(10).unwrap();
    let request = capture(scope.token(), 1).unwrap();
    drop(first);
    let _second = scope.begin_prompt(11).unwrap();
    assert_eq!(request.source_message_id(), 10);
    assert!(request.ensure_active().is_err());
    assert!(request.cancellation().is_cancelled());
    assert_eq!(capture(scope.token(), 1).unwrap().source_message_id(), 11);
}

#[test]
fn test_native_mcp_cancel_and_runtime_drop_revoke_inflight_and_token() {
    let scope = NativeMcpScope::new(1).unwrap();
    let token = scope.token().to_string();
    let first = scope.begin_prompt(10).unwrap();
    let request = capture(&token, 1).unwrap();
    scope.cancel_prompt();
    assert!(capture(&token, 1).is_err());
    assert!(request.ensure_active().is_err());
    drop(first);
    let _next = scope.begin_prompt(11).unwrap();
    let next_request = capture(&token, 1).unwrap();
    drop(scope);
    assert!(capture(&token, 1).is_err());
    assert!(next_request.ensure_active().is_err());
}

#[tokio::test]
async fn test_native_mcp_cancel_signal_revokes_while_waiting_for_approval() {
    let scope = NativeMcpScope::new(1).unwrap();
    let (signal, receiver) = watch::channel(4);
    let _prompt = scope.begin_prompt(10).unwrap().watch_cancellation(receiver, 4);
    let request = capture(scope.token(), 1).unwrap();
    signal.send(5).unwrap();
    // No yield: the request must fail even before the watcher gets polled.
    assert!(capture(scope.token(), 1).is_err());
    tokio::time::timeout(std::time::Duration::from_secs(1), request.cancellation().cancelled()).await.unwrap();
    assert!(request.ensure_active().is_err());
}

#[tokio::test]
async fn test_native_mcp_already_cancelled_generation_never_admitted() {
    let scope = NativeMcpScope::new(1).unwrap();
    let (_signal, receiver) = watch::channel(5);
    let _prompt = scope.begin_prompt(10).unwrap().watch_cancellation(receiver, 4);
    assert!(capture(scope.token(), 1).is_err());
}

fn goal_fixture() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY,conversation_kind TEXT);
        CREATE TABLE message(id INTEGER PRIMARY KEY,conversation_id INTEGER,parent_id INTEGER,message_type TEXT,content TEXT,finish_time TEXT);
        INSERT INTO conversation VALUES(1,'butler_contact'),(2,'butler_contact');
        INSERT INTO message VALUES(10,1,NULL,'user','work',NULL),(11,1,10,'response','',NULL),(20,2,NULL,'user','other',NULL);").unwrap();
    crate::db::butler_goal::schema(&conn).unwrap();
    conn.execute_batch("INSERT INTO butler_goal VALUES(1,1,'work',0); INSERT INTO butler_goal_turn VALUES(10,1,1);").unwrap();
    conn
}

#[test]
fn test_native_mcp_valid_response_source_preserved_goal_change_rejected() {
    let conn = goal_fixture();
    let scope = NativeMcpScope::new(1).unwrap();
    let _prompt = scope.begin_prompt(11).unwrap();
    let request = capture(scope.token(), 1).unwrap();
    request.validate_connection(&conn).unwrap();
    conn.execute("UPDATE butler_goal SET revision=2 WHERE conversation_id=1", []).unwrap();
    assert!(request.validate_connection(&conn).unwrap_err().contains("目标已变更"));
}

#[test]
fn test_native_mcp_pause_and_cross_conversation_source_rejected() {
    let conn = goal_fixture();
    let scope = NativeMcpScope::new(1).unwrap();
    let prompt = scope.begin_prompt(11).unwrap();
    let request = capture(scope.token(), 1).unwrap();
    crate::db::butler_control::set(&conn, 1, "pause").unwrap();
    assert!(request.validate_connection(&conn).unwrap_err().contains("暂停"));
    drop(prompt);
    let _other_source = scope.begin_prompt(20).unwrap();
    assert!(capture(scope.token(), 1).unwrap().validate_connection(&conn).unwrap_err().contains("does not belong"));
}

#[test]
fn test_native_mcp_debug_and_errors_redact_token() {
    let scope = NativeMcpScope::new(1).unwrap();
    assert!(!format!("{scope:?}").contains(scope.token()));
    let config = crate::acp_mcp_bridge::AcpMcpProxyConfig { addr: "127.0.0.1:1234".into(), token: scope.token().to_string() };
    assert!(!format!("{config:?}").contains(scope.token()));
    let error = capture(scope.token(), 2).err().unwrap();
    assert!(!error.contains(scope.token()));
}

#[tokio::test]
async fn test_native_mcp_run_preserves_source_across_await_and_revokes_result() {
    let scope = NativeMcpScope::new(1).unwrap();
    let _guard = scope.begin_prompt(11).unwrap();
    let request = capture(scope.token(), 1).unwrap();
    let source = request.run(async {
        tokio::task::yield_now().await;
        Ok(crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).unwrap())
    }).await.unwrap();
    assert_eq!(source, Some(11));
    let result = request.run(async {
        tokio::task::yield_now().await;
        scope.cancel_prompt();
        Ok("stale result")
    }).await;
    assert!(result.is_err());
    assert!(crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).is_err());
}

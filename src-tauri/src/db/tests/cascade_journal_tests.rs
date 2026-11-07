//! Exercise preparation failure with in-memory schemas standing in for the
//! persistent schema inventory. This does not simulate filesystem crash recovery.
use super::*;

#[test]
fn test_cascade_delete_journal_preparation_rejects_unsupported_mode_before_mutation() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("ATTACH ':memory:' AS aipp_delete_peer;
        CREATE TABLE assistant(id INTEGER PRIMARY KEY); INSERT INTO assistant VALUES(1);
        PRAGMA synchronous=OFF;").unwrap();
    let error = match prepare_schemas(&conn, vec!["main".into(), "aipp_delete_peer".into()]) {
        Ok(_) => panic!("memory journal cannot satisfy durable DELETE"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("journal_mode=memory") && error.contains("尚未删除数据"), "{error}");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM assistant", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    assert_eq!(conn.query_row("PRAGMA synchronous", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
    assert!(conn.is_autocommit());
}

#[test]
fn test_cascade_delete_journal_verification_rolls_back_outer_changes() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("ATTACH ':memory:' AS aipp_delete_peer;
        CREATE TABLE assistant(id INTEGER PRIMARY KEY); INSERT INTO assistant VALUES(1);").unwrap();
    let result = atomic(&conn, || {
        conn.execute("DELETE FROM assistant", [])?;
        verify_schemas(&conn, &["main".into(), "aipp_delete_peer".into()])
    });
    assert!(result.unwrap_err().to_string().contains("事务未安全准备"));
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM assistant", [], |r| r.get::<_,i64>(0)).unwrap(), 1);
    assert!(conn.is_autocommit());
}

#[test]
fn test_cascade_delete_journal_rejects_persistent_peer_without_main() {
    let conn = Connection::open_in_memory().unwrap();
    assert!(prepare_schemas(&conn, vec!["aipp_delete_peer".into()]).is_err());
    assert!(verify_schemas(&conn, &["aipp_delete_peer".into()]).is_err());
}

#[test]
fn test_cascade_delete_continues_in_wal_when_peer_connection_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let peer_path = dir.path().join("conversation.db");
    let main = Connection::open(dir.path().join("assistant.db")).unwrap();
    let peer = Connection::open(&peer_path).unwrap();
    for conn in [&main, &peer] {
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0)).unwrap();
    }
    main.execute_batch("CREATE TABLE assistant(id INTEGER PRIMARY KEY); INSERT INTO assistant VALUES(1);").unwrap();
    peer.execute_batch("CREATE TABLE conversation(id INTEGER PRIMARY KEY, assistant_id INTEGER);
        INSERT INTO conversation VALUES(1, 1);").unwrap();
    // The running app keeps this connection open; leaving WAL is impossible.
    main.execute("ATTACH DATABASE ?1 AS aipp_delete_conversation", [peer_path.to_string_lossy().as_ref()]).unwrap();

    let guard = prepare_schemas(&main, vec!["main".into(), "aipp_delete_conversation".into()]).unwrap();
    atomic(&main, || {
        main.execute("UPDATE aipp_delete_conversation.conversation SET assistant_id=NULL", [])?;
        main.execute("DELETE FROM assistant WHERE id=1", [])
    }).unwrap();
    drop(guard);

    assert_eq!(main.query_row("SELECT COUNT(*) FROM assistant", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let owner: Option<i64> = peer.query_row("SELECT assistant_id FROM conversation", [], |r| r.get(0)).unwrap();
    assert_eq!(owner, None);
    let mode: String = peer.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
}

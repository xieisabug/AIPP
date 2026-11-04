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

use super::*;

fn previous_connection() -> (MCPDatabase, i64) {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    super::super::init(&db.conn).unwrap();
    let id = db.upsert_mcp_server_with_builtin("test", None, "http", None, None, Some("{}"),
        Some("https://example.com/mcp"), None, false, true, false, true, false).unwrap();
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status,verified_session)
        VALUES(?1,'original','test','{}','test','{}','available','previous')", [id]).unwrap();
    (db, id)
}

#[test]
fn test_startup_recheck_selects_only_enabled_previously_available_connections() {
    let (db, id) = previous_connection();
    assert_eq!(candidates(&db.conn).unwrap(), vec![id]);
    let check = PreviousCheck::read(&db, id).unwrap().unwrap();
    assert!(check.record_failure(&db, "connection timed out").unwrap());
    let saved: (String, String) = db.conn.query_row("SELECT status,error FROM app_connection", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(saved, ("error".into(), "connection timed out".into()));
    assert!(candidates(&db.conn).unwrap().is_empty());
    for status in ["unchecked", "disconnected", "connecting", "auth_required", "error"] {
        db.conn.execute("UPDATE app_connection SET status=?1,verified_session='previous'", [status]).unwrap();
        assert!(candidates(&db.conn).unwrap().is_empty());
    }
    db.conn.execute("UPDATE app_connection SET status='available',verified_session=?1", [SESSION.as_str()]).unwrap();
    assert!(candidates(&db.conn).unwrap().is_empty());
    db.conn.execute("UPDATE app_connection SET verified_session=NULL", []).unwrap();
    assert_eq!(candidates(&db.conn).unwrap(), vec![id]);
    db.conn.execute("UPDATE mcp_server SET is_enabled=0 WHERE id=?1", [id]).unwrap();
    assert!(candidates(&db.conn).unwrap().is_empty());
}

#[test]
fn test_startup_recheck_discards_failure_after_new_verification_or_configuration() {
    let (db, id) = previous_connection();
    let check = PreviousCheck::read(&db, id).unwrap().unwrap();
    db.conn.execute("UPDATE app_connection SET verified_session=?1", [SESSION.as_str()]).unwrap();
    assert!(!check.record_failure(&db, "late").unwrap());
    db.conn.execute("UPDATE app_connection SET verified_session='previous',request_id='updated'", []).unwrap();
    assert!(!check.record_failure(&db, "late").unwrap());
    let current = PreviousCheck::read(&db, id).unwrap().unwrap();
    db.conn.execute("UPDATE mcp_server SET proxy_enabled=1 WHERE id=?1", [id]).unwrap();
    assert!(!current.record_failure(&db, "late").unwrap());
}

#[test]
fn test_connection_edit_and_reenable_require_a_fresh_check() {
    let (db,id) = previous_connection();
    db.conn.execute("UPDATE mcp_server SET timeout=45000 WHERE id=?1",[id]).unwrap();
    let status = || db.conn.query_row("SELECT status FROM app_connection WHERE server_id=?1",[id],|r|r.get::<_,String>(0)).unwrap();
    assert_eq!(status(),"unchecked");
    db.conn.execute("UPDATE app_connection SET status='available'",[]).unwrap();
    db.toggle_mcp_server(id,false).unwrap();
    db.toggle_mcp_server(id,true).unwrap();
    assert_eq!(status(),"unchecked");
    db.conn.execute("UPDATE app_connection SET status='auth_required'",[]).unwrap();
    db.conn.execute("UPDATE mcp_server SET timeout=55000 WHERE id=?1",[id]).unwrap();
    assert_eq!(status(),"auth_required");
}

use super::*;

fn fixture() -> (MCPDatabase, Connection, MCPServer, i64) {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    super::super::init(&db.conn).unwrap();
    let id = db.upsert_mcp_server_with_builtin("account-a", None, "http", None, None, Some("{}"), Some("https://example.com/mcp"), None, false, true, false, true, false).unwrap();
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status) VALUES(?1,'generation-1','test','{}','Account A','{}','available')", [id]).unwrap();
    let tool = db.upsert_mcp_server_tool(id, "read_notes", Some("Read"), Some("{}")).unwrap();
    let assistants = Connection::open_in_memory().unwrap();
    assistants.execute_batch("CREATE TABLE assistant_mcp_config(assistant_id INTEGER,mcp_server_id INTEGER,is_enabled INTEGER); CREATE TABLE assistant_mcp_tool_config(assistant_id INTEGER,mcp_tool_id INTEGER,is_enabled INTEGER);").unwrap();
    assistants.execute("INSERT INTO assistant_mcp_config VALUES(7,?1,1)", [id]).unwrap();
    let server = db.get_mcp_server(id).unwrap();
    (db, assistants, server, tool)
}

#[test]
fn test_apps_execution_rechecks_assistant_and_tool_permissions() {
    let (db, assistants, server, tool) = fixture();
    validate(&db, &assistants, 7, &server, "read_notes").unwrap();
    assert!(validate(&db, &assistants, 8, &server, "read_notes").is_err());
    assistants.execute("INSERT INTO assistant_mcp_tool_config VALUES(7,?1,0)", [tool]).unwrap();
    assert!(validate(&db, &assistants, 7, &server, "read_notes").is_err());
    assistants.execute("DELETE FROM assistant_mcp_tool_config", []).unwrap();
    db.update_mcp_server_tool(tool, false, false).unwrap();
    assert!(validate(&db, &assistants, 7, &server, "read_notes").is_err());
}

#[test]
fn test_apps_execution_rejects_disabled_reconfigured_and_other_connection() {
    let (db, assistants, server, _) = fixture();
    db.toggle_mcp_server(server.id, false).unwrap();
    assert!(validate(&db, &assistants, 7, &server, "read_notes").is_err());
    db.toggle_mcp_server(server.id, true).unwrap();
    db.conn.execute("UPDATE mcp_server SET headers='new-reference' WHERE id=?1", [server.id]).unwrap();
    assert!(validate(&db, &assistants, 7, &server, "read_notes").unwrap_err().contains("旧请求"));
    let other = db.upsert_mcp_server_with_builtin("account-b", None, "http", None, None, Some("{}"), Some("https://example.com/mcp"), None, false, true, false, true, false).unwrap();
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status) VALUES(?1,'generation-b','test','{}','Account B','{}','available')", [other]).unwrap();
    db.upsert_mcp_server_tool(other, "read_notes", Some("Read"), Some("{}")).unwrap();
    assert!(validate(&db, &assistants, 7, &db.get_mcp_server(other).unwrap(), "read_notes").is_err());
}

#[test]
fn test_apps_queued_call_cannot_use_reauthorized_connection() {
    let (db, _, server, _) = fixture();
    let call = db.create_mcp_tool_call(1, None, server.id, &server.name, "read_notes", "{}").unwrap();
    validate_call(&db.conn, server.id, call.id).unwrap();
    db.conn.execute("UPDATE app_connection SET request_id='generation-2' WHERE server_id=?1", [server.id]).unwrap();
    assert!(validate_call(&db.conn, server.id, call.id).unwrap_err().contains("批准已失效"));
    let next = db.create_mcp_tool_call(1, None, server.id, &server.name, "read_notes", "{}").unwrap();
    validate_call(&db.conn, server.id, next.id).unwrap();
}

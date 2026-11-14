use super::*;
use rusqlite::Connection;

#[test]
fn test_assistant_access_denials_filter_dynamic_catalog_and_loaded_tools() {
    let db = AssistantDatabase {conn:Connection::open_in_memory().unwrap(),mcp_conn:Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    let id = db.add_assistant("test","",Some(0),false).unwrap();
    let access = Restrictions::load(&db,id).unwrap();
    assert!(access.tool(42,100));
    db.upsert_assistant_mcp_config(id,42,false).unwrap();
    db.upsert_assistant_mcp_tool_config(id,101,false,false).unwrap();
    let access = Restrictions::load(&db,id).unwrap();
    assert!(!access.server(42));
    assert!(!access.tool(42,100));
    assert!(!access.tool(43,101));
    assert!(access.tool(43,100));
    db.upsert_assistant_mcp_config(id,42,true).unwrap();
    let refreshed = Restrictions::load(&db,id).unwrap();
    assert!(refreshed.tool(42,100));
    assert!(!refreshed.tool(42,101));
}

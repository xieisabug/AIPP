use super::MCPDatabase;
use rusqlite::Connection;

#[test]
fn test_summary_catalog_uses_only_discovered_tools_and_skips_custom_servers() {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    crate::mcp::apps::init(&db.conn).unwrap();
    db.conn.execute_batch(r#"
        INSERT INTO mcp_server(id,name,transport_type,is_builtin) VALUES
            (11,'My Runway','http',0),(12,'Custom','http',0),(13,'Builtin','builtin',1);
        INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status)
            VALUES(11,'a','runway','{"connection":{"url":"https://example.test/mcp"}}','My Runway','{}','available');
        INSERT INTO app_catalog_cache VALUES(1,'{"apps":[{"id":"runway","connection":{"url":"https://example.test/mcp"},"tool_summaries":{"generate":"生成视频","hidden":"未授权"}}]}','today');
        INSERT INTO mcp_server_tool(server_id,tool_name,tool_description) VALUES
            (11,'generate','Generate video'),(11,'new','Original description'),(12,'generate','User tool');
    "#).unwrap();
    db.rebuild_dynamic_mcp_catalog_force().unwrap();
    let managed = db.list_tool_catalog(Some(11)).unwrap();
    assert_eq!(managed.len(), 2);
    assert!(managed.iter().any(|t| t.tool_name == "generate" && t.summary == "生成视频"));
    assert!(managed.iter().any(|t| t.tool_name == "new" && t.summary == "Original description"));
    assert!(managed.iter().all(|t| t.summary_generated_at.is_some()));
    assert!(db.list_tool_catalog(Some(12)).unwrap()[0].summary_generated_at.is_none());
    assert_eq!(db.enabled_custom_summary_servers().unwrap().iter().map(|s| s.id).collect::<Vec<_>>(), vec![12]);
}

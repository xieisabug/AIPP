use super::*;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn test_tool_refresh_prepares_attached_database_before_transaction_and_rolls_back_together() {
    let db = MCPDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    db.conn.execute_batch("
        INSERT INTO mcp_server(id,name,transport_type) VALUES(18,'test','http');
        INSERT INTO mcp_server_tool(id,server_id,tool_name) VALUES(42,18,'old');
        ATTACH ':memory:' AS aipp_delete_assistant;
        CREATE TABLE aipp_delete_assistant.assistant_mcp_tool_config(mcp_tool_id INTEGER);
        INSERT INTO aipp_delete_assistant.assistant_mcp_tool_config VALUES(42);
        CREATE TRIGGER refuse_prompt BEFORE INSERT ON mcp_server_prompt
            BEGIN SELECT RAISE(ABORT,'prompt save failed'); END;
    ").unwrap();
    let server = db.get_mcp_server(18).unwrap();
    // Memory databases have no disk journals. Observe the preparation step and
    // reject an outer BEGIN that comes before the first database_list inspection.
    let prepared = AtomicBool::new(false);
    db.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        match ctx.action {
            AuthAction::Pragma { pragma_name: "database_list", .. } => prepared.store(true, Ordering::Relaxed),
            AuthAction::Transaction { .. } if !prepared.load(Ordering::Relaxed) => return Authorization::Deny,
            _ => {}
        }
        Authorization::Allow
    }));
    let refresh = |prompts| persist_capability_sets(&db, &server, "test",
        Some(vec![SimpleTool { name: "new".into(), description: None, params_json: "{}".into() }]),
        None, prompts);
    let bindings = || db.conn.query_row("SELECT COUNT(*) FROM aipp_delete_assistant.assistant_mcp_tool_config",
        [], |row| row.get::<_, i64>(0)).unwrap();
    let err = refresh(Some(vec![SimplePrompt { name: "test".into(), description: None, args_json: "[]".into() }])).unwrap_err();
    assert!(err.contains("prompt save failed"), "{err}");
    let tools = db.get_mcp_server_tools(18).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "old");
    assert_eq!(bindings(), 1);
    refresh(None).unwrap();
    let tools = db.get_mcp_server_tools(18).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "new");
    assert_eq!(bindings(), 0);
}

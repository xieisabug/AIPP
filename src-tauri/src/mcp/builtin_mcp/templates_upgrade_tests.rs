use super::*;

#[test]
fn test_builtin_upgrade_continues_after_an_unrelated_template_error() {
    let db = MCPDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    db.conn.execute_batch(
        "CREATE TRIGGER simulate_failure BEFORE INSERT ON mcp_server_tool
         WHEN (SELECT command FROM mcp_server WHERE id=NEW.server_id)='aipp:vault'
         BEGIN SELECT RAISE(ABORT,'simulated registration error'); END;",
    ).unwrap();
    let error = sync_builtin_catalog(&db).unwrap_err().to_string();
    assert!(error.contains("aipp:vault") && error.contains("simulated registration error"));
    assert!(db.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mcp_tool_catalog WHERE tool_name='present_choices' AND summary_generated_at IS NOT NULL)",
        [], |r| r.get::<_, bool>(0),
    ).unwrap());
}

#[test]
fn test_builtin_upgrade_updates_all_instances_and_preserves_preferences() {
    let db = MCPDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let tpl = builtin_templates().into_iter().find(|t| t.command == "aipp:ui_interaction").unwrap();
    sync_builtin_template(&db, &tpl).unwrap();
    let default_id = db.get_mcp_servers().unwrap()[0].id;
    let custom_id = db.upsert_mcp_server_with_builtin(
        "My UI", Some("custom"), "stdio", Some(&tpl.command), None, None, None,
        Some(12345), false, false, true, true, false,
    ).unwrap();
    db.conn.execute("DELETE FROM mcp_server_tool WHERE tool_name='present_choices'", []).unwrap();
    let question_id = db.upsert_mcp_server_tool(custom_id, "ask_user_question", Some("old"), None).unwrap();
    db.update_mcp_server_tool(question_id, false, true).unwrap();

    sync_builtin_template(&db, &tpl).unwrap();
    sync_builtin_template(&db, &tpl).unwrap();
    for id in [default_id, custom_id] {
        let tools = db.get_mcp_server_tools(id).unwrap();
        let choices = tools.iter().find(|t| t.tool_name == "present_choices").unwrap();
        assert!(choices.is_enabled && !choices.is_auto_run);
        assert_eq!(tools.len(), get_builtin_tools_for_command(&tpl.command).len());
    }
    let custom = db.get_mcp_server(custom_id).unwrap();
    assert!(!custom.is_enabled);
    assert_eq!(custom.timeout, Some(12345));
    let question = db.get_mcp_server_tools(custom_id).unwrap().into_iter().find(|t| t.id == question_id).unwrap();
    assert!(!question.is_enabled && question.is_auto_run);
    assert!(question.parameters.is_some());
}

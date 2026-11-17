use super::*;

fn database() -> MCPDatabase {
    let db = MCPDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    db.conn
        .execute_batch(
            "ATTACH ':memory:' AS aipp_delete_assistant;
        CREATE TABLE aipp_delete_assistant.assistant_mcp_tool_config(mcp_tool_id INTEGER);",
        )
        .unwrap();
    db
}

fn request(name: &str) -> MCPServerRequest {
    serde_json::from_value(serde_json::json!({"name":name,"transport_type":"stdio","command":"untrusted-placeholder --option",
        "is_enabled":true,"is_long_running":true,"proxy_enabled":false})).unwrap()
}

#[test]
fn test_connection_import_stays_disabled_and_name_conflict_rolls_back_whole_batch() {
    let db = database();
    import_configs(&db, &[request("original")]).unwrap();
    let before = db.get_mcp_servers().unwrap();
    assert_eq!(before.len(), 1);
    assert!(!before[0].is_enabled);
    assert!(db.get_mcp_server_tools(before[0].id).unwrap().is_empty());
    assert!(import_configs(&db, &[request("other"), request("original")]).is_err());
    assert_eq!(db.get_mcp_servers().unwrap(), before);
}

#[test]
fn test_connection_refresh_previews_permission_change_and_invalidates_old_confirmation() {
    let db = database();
    import_configs(&db, &[request("source")]).unwrap();
    let server = db.get_mcp_servers().unwrap().remove(0);
    let tools = |sig: &str| {
        vec![SimpleTool {
            name: "notes".into(),
            description: None,
            params_json: "{}".into(),
            signature: sig.into(),
        }]
    };
    let signatures = HashMap::new();
    let save = |sig: &str, expected: Option<&[MCPServerTool]>| {
        persist_reviewed(
            &db,
            &server,
            "test",
            Some(tools(sig)),
            None,
            None,
            expected.map(|e| (e, &signatures)),
        )
    };
    save("read", None).unwrap();
    let initial = db.get_mcp_server_tools(server.id).unwrap();
    assert!(!initial[0].is_enabled);
    db.update_mcp_server_tool(initial[0].id, true, true).unwrap();
    let before = db.get_mcp_server_tools(server.id).unwrap();
    let signatures = tool_signatures(&db, server.id).unwrap();
    assert!(changes(&signatures, &before, Some(&tools("read"))).changed.is_empty());
    assert_eq!(changes(&signatures, &before, Some(&tools("write"))).changed, vec!["notes"]);
    assert!(save("write", Some(&initial)).is_err());
    assert_eq!(db.get_mcp_server_tools(server.id).unwrap(), before);
    persist_reviewed(
        &db,
        &server,
        "test",
        Some(tools("write")),
        None,
        None,
        Some((&before, &signatures)),
    )
    .unwrap();
    let after = db.get_mcp_server_tools(server.id).unwrap();
    assert!(!after[0].is_enabled && !after[0].is_auto_run);
}

#[test]
fn test_connection_import_rejects_local_credential_and_plugin_bindings() {
    let db = database();
    let mut foreign = request("foreign");
    foreign.headers = Some(r#"{"X-AIPP-Credential-Ref":"test-reference"}"#.into());
    assert!(import_configs(&db, &[foreign]).unwrap_err().contains("凭据引用"));
    let mut plugin = request("plugin");
    plugin.command = Some(format!("{}example", apps::bundles::runtime::PREFIX));
    assert!(import_configs(&db, &[plugin]).is_err());
    assert!(db.get_mcp_servers().unwrap().is_empty());
}

#[test]
fn test_connection_restart_quarantines_removed_tools_without_applying_new_ones() {
    let db = database();
    apps::init(&db.conn).unwrap();
    import_configs(&db, &[request("a")]).unwrap();
    let id = db.get_mcp_servers().unwrap()[0].id;
    db.toggle_mcp_server(id, true).unwrap();
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status,verified_session)
        VALUES(?1,'generation','service','{}','a','{}','available','previous')",[id]).unwrap();
    let server = db.get_mcp_server(id).unwrap();
    db.upsert_mcp_server_tool(id, "removed", None, Some("{}")).unwrap();
    let old = db.get_mcp_server_tools(id).unwrap();
    let remote = [SimpleTool {
        name: "new".into(),
        description: None,
        params_json: "{}".into(),
        signature: "null".into(),
    }];
    let reason = restrict_changed_tools(
        &db,
        &server,
        &Some("generation".into()),
        &old,
        &HashMap::new(),
        Some(&remote),
    )
    .unwrap()
    .unwrap();
    assert!(reason.contains("移除 1"));
    let pending = db.get_mcp_server_tools(id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tool_name, "removed");
    assert!(!pending[0].is_enabled);
}

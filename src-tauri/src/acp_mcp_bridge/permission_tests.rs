use super::*;
use serde_json::json;

#[test]
fn test_native_review_exemption_uses_registered_source_arguments_and_target() {
    let db = MCPDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let access = Restrictions::default();
    let mut snapshot = Vec::new();
    let mut names = Vec::new();
    for (name, builtin) in [("Agent", true), ("External", false)] {
        let server = db.upsert_mcp_server_with_builtin(name, None, "stdio", Some("aipp:agent"),
            None, None, None, None, false, true, builtin, true, false).unwrap();
        let id = db.upsert_mcp_server_tool(server, "schedule_task", None, Some("{}")).unwrap();
        db.update_mcp_server_tool(id, true, false).unwrap();
        snapshot.push(json!({"server_id":server,"tools":[{"tool_id":id,"tool_name":"schedule_task"}]}));
        names.push(acp_loaded_tool_name(id, "schedule_task"));
    }
    let catalog = catalog(&json!(snapshot).to_string()).unwrap();
    let registered = &catalog[&names[0]];
    let args = json!({"action":"list"});
    let resolve = |reg, args: &Value| exempt_target(&db, reg, args, 7, "", &access);
    let target = resolve(registered, &args).unwrap();
    assert!(catalog.get("schedule_task").is_none());
    assert!(resolve(&catalog[&names[1]], &args).is_none());
    for non_exempt in [json!({"action":"delete"}), json!({}), json!(null), json!("{\"action\":\"list\"}")] {
        assert!(resolve(registered, &non_exempt).is_none());
    }
    let mut exemptions = Exemptions::default();
    exemptions.record(registered, &args, &target);
    assert!(exemptions.check(registered.server, &registered.name, &args, &target).is_ok());
    let changed = dynamic::Target { server: db.get_mcp_server(catalog[&names[1]].server).unwrap(),
        tool_name: target.tool_name, parameters: target.parameters };
    assert!(exemptions.check(registered.server, &registered.name, &args, &changed).is_err());
    db.update_mcp_server_tool(registered.id, false, false).unwrap();
    assert!(resolve(registered, &args).is_none());
}

#[test]
fn test_native_review_exemption_requires_protocol_identity_and_actual_arguments() {
    let registered = "aipp_t42_report_progress";
    let args = json!({"text":"working"});
    let method = "mcpServer/elicitation/request";
    let mut codex = json!({"serverName":"aipp","_meta":{"codex_approval_kind":"mcp_tool_call","tool_name":registered,"tool_params":args}});
    assert_eq!(codex_call(method, &codex), Some((registered, &args)));
    assert!(codex_call("item/commandExecution/requestApproval", &codex).is_none());
    codex["serverName"] = json!("other");
    assert!(codex_call(method, &codex).is_none());
    codex["serverName"] = json!("aipp");
    codex["_meta"].as_object_mut().unwrap().remove("tool_params");
    codex["_meta"]["tool_params_display"] = json!([{"value":args}]);
    assert!(codex_call(method, &codex).is_none());

    let mut claude = json!({"subtype":"can_use_tool","tool_name": format!("mcp__aipp__{registered}"),"input":args});
    assert_eq!(claude_call(&claude), Some((registered, &args)));
    claude["tool_name"] = json!("Bash");
    claude["input"] = json!({"tool_name":registered,"serverName":"aipp"});
    assert!(claude_call(&claude).is_none());
    let metadata = json!({"mcp":{"serverName":"AIPP MCP Tools","toolName":registered}});
    let meta = metadata.as_object().unwrap();
    assert_eq!(acp_call(meta, &args, "AIPP MCP Tools"), Some((registered, &args)));
    assert!(acp_call(meta, &args, "aipp").is_none());
    assert!(acp_call(&Default::default(), &metadata, "AIPP MCP Tools").is_none());
}

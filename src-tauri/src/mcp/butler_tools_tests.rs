use super::*;

#[cfg(desktop)]
#[test]
fn test_builtin_upgrade_repairs_missing_tool_during_butler_send() {
    let db = MCPDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    for command in ["aipp:agent", "aipp:superadmin", "aipp:ui_interaction"] {
        let id = db.upsert_mcp_server_with_builtin(command, None, "stdio", Some(command),
            None, None, None, None, false, true, true, false, false).unwrap();
        let mut tools = get_builtin_tools_for_command(command);
        tools.retain(|t| t.name != "present_choices");
        register_builtin_tools(&db, id, &tools).unwrap();
    }
    for kind in ["butler_main", "butler_contact"] {
        let mut selected = Vec::new();
        merge_tools(&mut selected, direct_servers(&db, kind).unwrap(), kind);
        let ui = selected.iter().find(|s| s.command.as_deref() == Some("aipp:ui_interaction")).unwrap();
        let choices = ui.tools.iter().find(|t| t.name == "present_choices").unwrap();
        assert!(choices.is_enabled && !choices.is_auto_run);
        assert!(db.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM mcp_tool_catalog WHERE tool_id=?1 AND summary_generated_at IS NOT NULL)",
            [choices.id], |r| r.get::<_, bool>(0),
        ).unwrap());
    }
}

fn server(id: i64, command: &str) -> MCPServerWithTools {
    MCPServerWithTools { id, name:command.into(),summary:String::new(),command:Some(command.into()),is_enabled:false,
        tools:get_builtin_tools_for_command(command).into_iter().enumerate().map(|(n,t)|MCPToolInfo {
            id:n as i64+id*100,name:t.name,description:t.description,parameters:t.input_schema.to_string(),
            is_enabled:false,is_auto_run:false,
        }).collect() }
}

#[test]
fn test_butler_tools_inject_unconfigured_assistants_in_all_butler_conversations() {
    for kind in ["butler_main","butler_contact","butler_task"] {
        for dynamic in [false,true] {
            let mut configured=Vec::new();
            merge_tools(&mut configured,vec![server(1,"aipp:agent"),server(2,"aipp:superadmin"),server(3,"aipp:ui_interaction")],kind);
            let names:Vec<_>=configured.iter().flat_map(|s|s.tools.iter().filter(move |t|
                !dynamic || super::super::builtin_mcp::templates::is_dynamic_loading_fixed_tool(s.command.as_deref(),&t.name,true)))
                .map(|t|t.name.as_str()).collect();
            // 受托助手可在自己的会话里向用户提问；其余界面交互工具仍按助手自身配置。
            assert!(!names.contains(&"preview_code"),"{kind} should not force preview tools");
            for expected in ["list_assistants","get_assistant_details","get_assistant_exchange","stop_assistant_exchange","message_bot","read_work_context","continue_work","resume_work","butler_memory","superadmin_execute","superadmin_undo","ask_user_question","schedule_task"] {
                assert!(names.contains(&expected),"{kind} dynamic={dynamic} missing {expected}");
            }
            assert!(configured.iter().all(|s|s.is_enabled&&s.tools.iter().all(|t|t.is_enabled)));
            // Runtime overrides keep the original auto-run/review policy.
            assert!(configured.iter().flat_map(|s|&s.tools).all(|t|!t.is_auto_run));
        }
    }
}

#[test]
fn test_butler_tools_group_member_has_same_permissions_as_other_butler_assistants() {
    // 管家模式下所有助手与总管家权限一致，群成员也能协作、管理定时任务；只有独立任务工具留在主会话。
    let mut configured=vec![server(1,"aipp:agent")];
    for tool in &mut configured[0].tools { tool.is_enabled=true; }
    merge_tools(&mut configured,vec![server(1,"aipp:agent"),server(2,"aipp:superadmin"),server(3,"aipp:ui_interaction")],"butler_group_member");
    let names:Vec<_>=configured.iter().flat_map(|s|s.tools.iter()).map(|t|t.name.as_str()).collect();
    for expected in ["butler_memory","superadmin_execute","superadmin_undo","ask_user_question","message_bot","list_assistants","search_bot_history","schedule_task"] {
        assert!(names.contains(&expected),"group member missing {expected}");
    }
    for hidden in ["spawn_task_conversation","task_conversation_operation"] {
        assert!(!names.contains(&hidden),"group member should not see {hidden}");
    }
    assert!(is_direct_tool("butler_group_member",Some("aipp:agent"),"message_bot"));
    assert!(is_direct_tool("butler_group_member",Some("aipp:agent"),"schedule_task"));
    assert!(is_direct_tool("butler_group_member",Some("aipp:superadmin"),"superadmin_execute"));
    // 群成员读写的是本群记忆，所以记忆工具在群里保留。
    assert!(is_direct_tool("butler_group_member",Some("aipp:agent"),"butler_memory"));
}

#[test]
fn test_butler_tools_global_baseline_merges_without_duplicates() {
    let mut configured=vec![server(5,"aipp:operation")];
    configured[0].tools.truncate(1);
    let first=configured[0].tools[0].name.clone();
    let mut baseline=server(5,"aipp:operation");
    for tool in &mut baseline.tools { tool.is_enabled=true; }
    let total=baseline.tools.len();
    let mut search=server(6,"aipp:search");
    search.is_enabled=true;
    add_baseline(&mut configured,vec![baseline,search]);
    assert_eq!(configured.len(),2);
    assert_eq!(configured[0].tools.len(),total,"已有服务补齐缺少的工具");
    assert!(configured[0].tools.iter().find(|t|t.name==first).unwrap().is_enabled,"已有工具改为启用而不是重复");
    assert!(configured.iter().all(|s|s.is_enabled));
}

#[test]
fn test_butler_group_history_tool_only_in_group_member_sessions() {
    for kind in ["butler_group_member", "butler_main", "butler_contact", "butler_task", "scheduled_run", "normal"] {
        let mut configured = vec![server(1, "aipp:agent")];
        merge_tools(&mut configured, vec![server(1, "aipp:agent")], kind);
        let visible = configured[0].tools.iter().any(|t| t.name == "read_group_history");
        assert_eq!(visible, kind == "butler_group_member", "{kind}");
        assert_eq!(is_direct_tool(kind, Some("aipp:agent"), "read_group_history"), visible);
    }
}

#[test]
fn test_butler_tools_overrides_disabled_config_without_duplicates_or_schema_drift() {
    let mut configured=vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")];
    merge_tools(&mut configured,vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")],"butler_contact");
    let first=serde_json::to_value(&configured).unwrap();
    merge_tools(&mut configured,vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")],"butler_contact");
    assert_eq!(first,serde_json::to_value(&configured).unwrap());
    for s in configured { let mut names=std::collections::HashSet::new(); for t in s.tools {assert!(names.insert(t.name));} }
}

#[test]
fn test_butler_tools_do_not_grant_administration_to_normal_chats() {
    for kind in ["normal","chat",""] {
        let mut configured=Vec::new();
        merge_tools(&mut configured,vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")],kind);
        assert!(configured.is_empty());
        assert!(!is_direct_tool(kind,Some("aipp:superadmin"),"superadmin_execute"));
        assert!(!is_direct_tool(kind,Some("aipp:agent"),"list_assistants"));
        assert!(!is_direct_tool(kind,Some("aipp:agent"),"butler_memory"));
        let mut configured=vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")];
        merge_tools(&mut configured,Vec::new(),kind);
        assert_eq!(configured.len(),1);
        assert!(!configured[0].tools.iter().any(|t|crate::api::butler_peer::is_peer_tool(&t.name)||t.name=="butler_memory"));
    }
    assert!(!is_direct_tool("butler_contact",Some("aipp:superadmin"),"not_a_tool"));
    assert!(!is_direct_tool("butler_contact",Some("aipp:operation"),"execute_command"));
}

#[tokio::test]
async fn test_butler_tools_have_direct_schemas_in_native_bridge_and_dynamic_prompt() {
    let mut servers=Vec::new();
    merge_tools(&mut servers,vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")],"butler_contact");
    let payload=crate::api::ai::acp::selected_tools_payload(servers.clone()).unwrap();
    let native:serde_json::Value=serde_json::from_str(&payload).unwrap();
    let names:Vec<_>=native.as_array().unwrap().iter().flat_map(|s|s["tools"].as_array().unwrap())
        .map(|t|t["tool_name"].as_str().unwrap()).collect();
    assert!(names.contains(&"butler_memory"));assert!(names.contains(&"resume_work"));assert!(names.contains(&"superadmin_execute"));assert!(names.contains(&"get_assistant_details"));
    let info=MCPInfoForAssistant {enabled_servers:servers,use_native_toolcall:false,dynamic_loading_enabled:true};
    let prompt=super::super::prompt::format_mcp_prompt(String::new(),&info).await;
    assert!(prompt.contains("butler_memory"));assert!(prompt.contains("superadmin_execute"));assert!(prompt.contains("get_assistant_details"));
    assert!(prompt.contains("server_name: aipp:superadmin"));
    let (tools,mapping)=crate::api::ai_api::build_tools_with_mapping(&info.enabled_servers);
    assert_eq!(tools.len(),names.len());
    assert!(mapping.values().any(|(_,tool)|tool=="superadmin_execute"));
}

use super::*;

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
            for expected in ["list_assistants","get_assistant_details","get_assistant_exchange","stop_assistant_exchange","message_bot","superadmin_execute","superadmin_undo","ask_user_question"] {
                assert!(names.contains(&expected),"{kind} dynamic={dynamic} missing {expected}");
            }
            assert!(configured.iter().all(|s|s.is_enabled&&s.tools.iter().all(|t|t.is_enabled)));
            // Runtime overrides keep the original auto-run/review policy.
            assert!(configured.iter().flat_map(|s|&s.tools).all(|t|!t.is_auto_run));
        }
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
        let mut configured=vec![server(1,"aipp:agent"),server(2,"aipp:superadmin")];
        merge_tools(&mut configured,Vec::new(),kind);
        assert_eq!(configured.len(),1);
        assert!(!configured[0].tools.iter().any(|t|crate::api::butler_peer::is_peer_tool(&t.name)));
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
    assert!(names.contains(&"superadmin_execute"));assert!(names.contains(&"get_assistant_details"));
    let info=MCPInfoForAssistant {enabled_servers:servers,use_native_toolcall:false,dynamic_loading_enabled:true};
    let prompt=super::super::prompt::format_mcp_prompt(String::new(),&info).await;
    assert!(prompt.contains("superadmin_execute"));assert!(prompt.contains("get_assistant_details"));
    assert!(prompt.contains("server_name: aipp:superadmin"));
    let (tools,mapping)=crate::api::ai_api::build_tools_with_mapping(&info.enabled_servers);
    assert_eq!(tools.len(),names.len());
    assert!(mapping.values().any(|(_,tool)|tool=="superadmin_execute"));
}

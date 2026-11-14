use super::*;
use rusqlite::Connection;
use serde_json::json;

fn fixture() -> (MCPDatabase,MCPServer,i64,i64) {
    let db = MCPDatabase {conn:Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    let agent = db.upsert_mcp_server_with_builtin("Agent",None,"stdio",Some("aipp:agent"),None,None,None,None,false,true,true,false,false).unwrap();
    let wrapper = db.upsert_mcp_server_tool(agent,"call_mcp_tool",None,Some("{}")).unwrap();
    db.update_mcp_server_tool(wrapper,true,false).unwrap();
    for name in ["load_mcp_server","load_mcp_tool"] {db.upsert_mcp_server_tool(agent,name,None,Some("{}")).unwrap();}
    let target = db.upsert_mcp_server_with_builtin("Notes",None,"http",None,None,None,Some("http://127.0.0.1/mcp"),None,false,true,false,true,false).unwrap();
    let tool = db.upsert_mcp_server_tool(target,"read_notes",None,Some("{\"type\":\"object\"}")).unwrap();
    db.update_mcp_server_tool(tool,true,false).unwrap();
    db.rebuild_dynamic_mcp_catalog().unwrap();
    let agent = db.get_mcp_server(agent).unwrap();
    (db,agent,target,tool)
}

fn arguments() -> String { json!({"server_name":"Notes","tool_name":"read_notes","parameters":{"query":"hello"}}).to_string() }
fn error(result: Result<Target,String>) -> String {match result {Ok(_)=>panic!("unexpected execution target"),Err(e)=>e}}

#[test]
fn test_native_dynamic_call_dispatches_loaded_target_instead_of_agent_wrapper() {
    let (db,agent,id,tool) = fixture();
    db.upsert_conversation_loaded_tool(7,tool,Some("load_mcp_tool")).unwrap();
    let target = resolve_target(&db,agent,"call_mcp_tool",&arguments(),7,&Restrictions::default()).unwrap();
    assert_eq!(target.server.id,id);
    assert_eq!(target.server.transport_type,"http");
    assert_eq!(target.server.command,None);
    assert_eq!(target.tool_name,"read_notes");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&target.parameters).unwrap(),json!({"query":"hello"}));
}

#[test]
fn test_native_dynamic_call_requires_current_conversation_load_and_fresh_schema() {
    let (db,agent,_,tool) = fixture();
    assert!(error(resolve_target(&db,agent.clone(),"call_mcp_tool",&arguments(),7,&Restrictions::default())).contains("尚未加载"));
    db.upsert_conversation_loaded_tool(8,tool,None).unwrap();
    assert!(resolve_target(&db,agent.clone(),"call_mcp_tool",&arguments(),7,&Restrictions::default()).is_err());
    db.upsert_conversation_loaded_tool(7,tool,None).unwrap();
    db.conn.execute("UPDATE mcp_server_tool SET parameters='changed' WHERE id=?1",[tool]).unwrap();
    assert!(error(resolve_target(&db,agent,"call_mcp_tool",&arguments(),7,&Restrictions::default())).contains("已失效"));
}

#[test]
fn test_native_dynamic_call_rechecks_service_tool_and_assistant_denials() {
    let (db,agent,id,tool) = fixture();
    db.upsert_conversation_loaded_tool(7,tool,None).unwrap();
    db.update_mcp_server_tool(tool,false,false).unwrap();
    assert!(error(resolve_target(&db,agent.clone(),"call_mcp_tool",&arguments(),7,&Restrictions::default())).contains("已禁用"));
    db.update_mcp_server_tool(tool,true,false).unwrap();
    db.conn.execute("UPDATE mcp_server SET is_enabled=0 WHERE id=?1",[id]).unwrap();
    assert!(error(resolve_target(&db,agent.clone(),"call_mcp_tool",&arguments(),7,&Restrictions::default())).contains("已禁用"));
    db.conn.execute("UPDATE mcp_server SET is_enabled=1 WHERE id=?1",[id]).unwrap();
    let assistants = crate::db::assistant_db::AssistantDatabase {conn:Connection::open_in_memory().unwrap(),mcp_conn:Connection::open_in_memory().unwrap()};
    assistants.create_tables().unwrap();
    let assistant = assistants.add_assistant("test","",Some(4),false).unwrap();
    assistants.upsert_assistant_mcp_config(assistant,id,false).unwrap();
    let access = Restrictions::load(&assistants,assistant).unwrap();
    assert!(error(resolve_target(&db,agent.clone(),"call_mcp_tool",&arguments(),7,&access)).contains("已禁用"));
    assistants.upsert_assistant_mcp_config(assistant,id,true).unwrap();
    assistants.upsert_assistant_mcp_tool_config(assistant,tool,false,false).unwrap();
    let access = Restrictions::load(&assistants,assistant).unwrap();
    assert!(error(resolve_target(&db,agent,"call_mcp_tool",&arguments(),7,&access)).contains("已禁用"));
}

#[test]
fn test_native_dynamic_call_rejects_missing_target_and_recursive_wrapper() {
    let (db,agent,_,_) = fixture();
    for args in [json!({}),json!({"server_name":"missing","tool_name":"read_notes"}),
        json!({"server_name":"Notes","tool_name":"missing"}),
        json!({"server_name":"Agent","tool_name":"call_mcp_tool","parameters":{}})] {
        assert!(resolve_target(&db,agent.clone(),"call_mcp_tool",&args.to_string(),7,&Restrictions::default()).is_err());
    }
    let target = resolve_target(&db,agent.clone(),"load_mcp_tool","{\"names\":[\"read\"]}",7,&Restrictions::default()).unwrap();
    assert_eq!(target.server.id,agent.id);
    assert_eq!(target.tool_name,"load_mcp_tool");
}

#[test]
fn test_native_dynamic_call_does_not_project_third_party_tools_with_same_name() {
    let (db,_,id,_) = fixture();
    let source = db.get_mcp_server(id).unwrap();
    let target = resolve_target(&db,source,"call_mcp_tool",&arguments(),7,&Restrictions::default()).unwrap();
    assert_eq!(target.server.id,id);
    assert_eq!(target.tool_name,"call_mcp_tool");
    assert_eq!(target.parameters,arguments());
}

#[tokio::test]
async fn test_native_dynamic_call_reaches_transport_and_returns_its_result() {
    let (db,agent,id,tool) = fixture();
    db.upsert_conversation_loaded_tool(7,tool,None).unwrap();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let result = dispatch(&db,agent.clone(),"call_mcp_tool",&arguments(),7,&Restrictions::default(),|target| {
        calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        assert_eq!(target.server.id,id);
        assert_eq!(target.tool_name,"read_notes");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&target.parameters).unwrap(),json!({"query":"hello"}));
        async { Ok("actual tool result".into()) }
    }).await.unwrap();
    assert_eq!(result,"actual tool result");
    let error = dispatch(&db,agent.clone(),"call_mcp_tool",&arguments(),8,&Restrictions::default(),|_| {
        calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        async { Ok("must not execute".into()) }
    }).await.unwrap_err();
    assert!(error.contains("尚未加载"));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst),1);
    let error = dispatch(&db,agent,"call_mcp_tool",&arguments(),7,&Restrictions::default(),|_| async {
        Err("original transport error".into())
    }).await.unwrap_err();
    assert_eq!(error,"original transport error");
}

#[tokio::test]
async fn test_native_dynamic_entries_recheck_global_flags_after_session_start() {
    for name in ["load_mcp_server","load_mcp_tool","call_mcp_tool"] {
        let (db,agent,_,tool) = fixture();
        db.upsert_conversation_loaded_tool(7,tool,None).unwrap();
        let entry = db.get_mcp_server_tools(agent.id).unwrap().into_iter().find(|t|t.tool_name==name).unwrap();
        assert!(resolve_target(&db,agent.clone(),name,&arguments(),7,&Restrictions::default()).is_ok());
        db.update_mcp_server_tool(entry.id,false,false).unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let failure = dispatch(&db,agent.clone(),name,&arguments(),7,&Restrictions::default(),|_| {
            calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            async {Ok("must not execute".into())}
        }).await.unwrap_err();
        assert!(failure.contains("已停用"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst),0);
        db.update_mcp_server_tool(entry.id,true,false).unwrap();
        db.conn.execute("UPDATE mcp_server SET is_enabled=0 WHERE id=?1",[agent.id]).unwrap();
        assert!(error(resolve_target(&db,agent,name,&arguments(),7,&Restrictions::default())).contains("已停用"));
    }
}

use super::*;
use rusqlite::Connection;

fn fixture() -> (AssistantDatabase, LLMDatabase, i64, i64) {
    let db = AssistantDatabase {conn:Connection::open_in_memory().unwrap(),mcp_conn:Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    db.conn.execute_batch("CREATE TABLE assistant_skill_config(id INTEGER PRIMARY KEY,assistant_id INTEGER,skill_identifier TEXT,is_enabled BOOLEAN,priority INTEGER,created_time TEXT DEFAULT CURRENT_TIMESTAMP,UNIQUE(assistant_id,skill_identifier));").unwrap();
    let llm = LLMDatabase {conn:Connection::open_in_memory().unwrap()};
    llm.create_tables().unwrap();
    llm.add_llm_provider("local mock","openai","",false,true).unwrap();
    let normal = llm.conn.last_insert_rowid();
    llm.add_llm_model("mock",normal,"mock-model","",false,false,false).unwrap();
    llm.add_llm_provider("Codex mock","codex_app_server","",false,true).unwrap();
    let codex = llm.conn.last_insert_rowid();
    (db,llm,normal,codex)
}

fn create(db: &AssistantDatabase, llm: &LLMDatabase, provider: i64, kind: i64) -> i64 {
    profile(db,llm,"create",&json!({"name":"助手","description":"简介","assistant_type":kind,"prompt":"system","provider_id":provider,"model_code":if kind==4{""}else{"mock-model"}}),&["agents:one".into(),"agents:two".into()],false).unwrap()["assistant_id"].as_i64().unwrap()
}

#[test]
fn test_assistant_management_create_defaults_and_partial_update() {
    let (db,llm,normal,_) = fixture();
    let id = create(&db,&llm,normal,0);
    assert_eq!(config_value(&db,id,"dynamic_mcp_loading_enabled").unwrap().as_deref(),Some("true"));
    assert_eq!(db.conn.query_row("SELECT count(*) FROM assistant_skill_config WHERE assistant_id=?1 AND is_enabled=1",[id],|r|r.get::<_,i64>(0)).unwrap(),2);
    profile(&db,&llm,"update",&json!({"assistant_id":id,"name":"新名称"}),&["agents:new".into()],false).unwrap();
    let assistant = db.get_assistant(id).unwrap();
    assert_eq!(assistant.name,"新名称");
    assert_eq!(assistant.description.as_deref(),Some("简介"));
    assert_eq!(db.get_assistant_prompt(id).unwrap()[0].prompt,"system");
    assert_eq!(db.get_assistant_model(id).unwrap()[0].provider_id,normal);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM assistant_skill_config WHERE assistant_id=?1",[id],|r|r.get::<_,i64>(0)).unwrap(),2);
}

#[test]
fn test_assistant_management_copy_preserves_configs_and_remaps_relations() {
    let (db,llm,normal,_) = fixture();
    let id = create(&db,&llm,normal,0);
    let prompt = db.get_assistant_prompt(id).unwrap()[0].id;
    db.add_assistant_prompt_param(id,prompt,"name","string","value").unwrap();
    db.upsert_assistant_mcp_config(id,77,false).unwrap();
    db.upsert_assistant_mcp_tool_config(id,88,false,true).unwrap();
    db.add_assistant_workspace(id,"/project").unwrap();
    db.conn.execute("UPDATE assistant_skill_config SET is_enabled=0,priority=9 WHERE assistant_id=?1",[id]).unwrap();
    let copied = profile(&db,&llm,"copy",&json!({"assistant_id":id,"name":"副本","prompt":"override"}),&["agents:new".into()],false).unwrap()["assistant_id"].as_i64().unwrap();
    assert_ne!(id,copied);
    assert_eq!(db.get_assistant_prompt(id).unwrap()[0].prompt,"system");
    assert_eq!(db.get_assistant_prompt(copied).unwrap()[0].prompt,"override");
    assert_eq!(db.get_assistant_mcp_configs(copied).unwrap()[0].is_enabled,false);
    assert_eq!(db.get_assistant_mcp_tool_configs(copied).unwrap()[0].is_auto_run,true);
    assert_eq!(db.get_assistant_workspaces(copied).unwrap()[0].path,"/project");
    assert_eq!(db.get_assistant_prompt_params(copied).unwrap()[0].assistant_prompt_id,db.get_assistant_prompt(copied).unwrap()[0].id);
    assert_eq!(db.get_assistant_model_configs(copied).unwrap().len(),db.get_assistant_model_configs(id).unwrap().len());
    assert!(db.get_assistant_model_configs(copied).unwrap().iter().all(|c|c.assistant_model_id==db.get_assistant_model(copied).unwrap()[0].id));
    let disabled:i64 = db.conn.query_row("SELECT count(*) FROM assistant_skill_config WHERE assistant_id=?1 AND is_enabled=0 AND priority=9",[copied],|r|r.get(0)).unwrap();
    assert_eq!(disabled,2);
    profile(&db,&llm,"update",&json!({"assistant_id":copied,"description":"独立"}),&[],false).unwrap();
    assert_eq!(db.get_assistant(id).unwrap().description.as_deref(),Some("简介"));
    // Only configuration tables are touched; there is no conversation/memory database in this fixture.
}

#[test]
fn test_assistant_management_rejects_invalid_profiles_without_partial_writes() {
    let (db,llm,normal,codex) = fixture();
    let id = create(&db,&llm,normal,0);
    for args in [json!({"assistant_id":id,"name":" "}),json!({"assistant_id":id,"name":null}),
        json!({"assistant_id":id,"assistant_type":99}),json!({"assistant_id":id,"assistant_type":4}),
        json!({"assistant_id":id,"provider_id":codex,"model_code":"x"}),
        json!({"assistant_id":id,"provider_id":normal,"model_code":"missing"}),
        json!({"assistant_id":id,"provider_id":-1,"model_code":"x"})] {
        assert!(profile(&db,&llm,"update",&args,&[],false).is_err(),"{args}");
        assert_eq!(db.get_assistant(id).unwrap().name,"助手");
        assert_eq!(db.get_assistant_model(id).unwrap()[0].provider_id,normal);
    }
    assert!(profile(&db,&llm,"update",&json!({"assistant_id":1,"assistant_type":1}),&[],false).is_err());
}

#[test]
fn test_assistant_management_dry_run_and_failure_roll_back_all_configuration() {
    let (db,llm,normal,_) = fixture();
    let before = db.get_assistants().unwrap().len();
    let args = json!({"name":"preview","provider_id":normal,"model_code":"mock-model"});
    profile(&db,&llm,"create",&args,&["agents:one".into()],true).unwrap();
    assert_eq!(db.get_assistants().unwrap().len(),before);
    db.conn.execute_batch("CREATE TRIGGER reject_skill BEFORE INSERT ON assistant_skill_config BEGIN SELECT RAISE(ABORT,'test skill failure'); END;").unwrap();
    assert!(profile(&db,&llm,"create",&args,&["agents:one".into()],false).unwrap_err().contains("test skill failure"));
    assert_eq!(db.get_assistants().unwrap().len(),before);
}

#[test]
fn test_assistant_management_channel_overrides_inherit_and_match_codex_runtime() {
    let (db,llm,_,codex) = fixture();
    let id = create(&db,&llm,codex,4);
    llm.add_llm_provider_config(codex,"codex_sandbox","read-only","",false).unwrap();
    let before = channel::read(&db,&llm,id).unwrap();
    let field = |v:&Value,key:&str|v["parameters"].as_array().unwrap().iter().find(|p|p["name"]==key).unwrap().clone();
    assert_eq!(field(&before,"codex_sandbox")["source"],"provider");
    let set = json!({"codex_sandbox":"workspace-write","codex_approval_policy":"never","codex_approvals_reviewer":"auto_review"});
    channel::patch(&db,&llm,id,&set,&json!([]),false).unwrap();
    let config = crate::api::ai::codex_app_server::extract_codex_app_server_config(&db.get_assistant_model_configs(id).unwrap(),&llm.get_llm_provider_config(codex).unwrap(),None).unwrap();
    assert_eq!(config.sandbox.as_deref(),Some("workspace-write"));
    assert_eq!(config.approval_policy.as_deref(),Some("never"));
    assert_eq!(config.approvals_reviewer.as_deref(),Some("auto_review"));
    let restored = channel::patch(&db,&llm,id,&json!({}),&json!(["codex_sandbox","codex_approval_policy","codex_approvals_reviewer"]),false).unwrap();
    assert_eq!(field(&restored,"codex_sandbox")["effective_value"],"read-only");
    assert_eq!(field(&restored,"codex_sandbox")["source"],"provider");
    assert_eq!(field(&restored,"codex_approvals_reviewer")["effective_value"],"user");
    assert!(!restored["runtime_verified"].as_bool().unwrap());
}

#[test]
fn test_assistant_management_channel_validation_is_atomic_and_scoped() {
    let (db,llm,normal,codex) = fixture();
    let normal_id = create(&db,&llm,normal,0);
    let id = create(&db,&llm,codex,4);
    for set in [json!({"codex_sandbox":"invalid"}),json!({"stream":false}),json!({"tool_review_mode":"auto_model","codex_approvals_reviewer":null})] {
        assert!(channel::patch(&db,&llm,id,&set,&json!([]),false).is_err());
        assert!(config_value(&db,id,"tool_review_mode").unwrap().is_none());
    }
    assert!(channel::patch(&db,&llm,normal_id,&json!({"codex_sandbox":"read-only"}),&json!([]),false).is_err());
    channel::patch(&db,&llm,normal_id,&json!({"stream":false,"tool_review_mode":"auto_model"}),&json!([]),true).unwrap();
    assert_eq!(config_value(&db,normal_id,"stream").unwrap().as_deref(),Some("true"));
    channel::patch(&db,&llm,normal_id,&json!({"stream":false}),&json!([]),false).unwrap();
    assert_eq!(config_value(&db,normal_id,"stream").unwrap().as_deref(),Some("false"));
}

#[test]
fn test_assistant_management_workspace_deduplicates_and_rejects_current_directory_removal() {
    let (db,llm,_,codex) = fixture();
    let id = create(&db,&llm,codex,4);
    let cwd = directory(std::env::current_dir().unwrap().to_str().unwrap()).unwrap();
    let parent = directory(std::path::Path::new(&cwd).parent().unwrap().to_str().unwrap()).unwrap();
    for path in [&cwd,&format!("{cwd}/.")] { channel::workspace(&db,&llm,id,&json!({"action":"add","path":path}),false).unwrap(); }
    assert_eq!(db.get_assistant_workspaces(id).unwrap().len(),1);
    let wid = db.get_assistant_workspaces(id).unwrap()[0].id;
    channel::workspace(&db,&llm,id,&json!({"action":"set_directory","path":cwd}),false).unwrap();
    assert!(channel::workspace(&db,&llm,id,&json!({"action":"remove","workspace_id":wid}),false).unwrap_err().contains("当前生效"));
    assert_eq!(db.get_assistant_workspaces(id).unwrap().len(),1);
    channel::workspace(&db,&llm,id,&json!({"action":"set_directory","path":parent}),false).unwrap();
    channel::workspace(&db,&llm,id,&json!({"action":"remove","workspace_id":wid}),false).unwrap();
    assert!(db.get_assistant_workspaces(id).unwrap().is_empty());
    assert!(channel::workspace(&db,&llm,id,&json!({"action":"add","path":"relative/path"}),false).is_err());
    let runtime = crate::api::ai::codex_app_server::extract_codex_app_server_config(&db.get_assistant_model_configs(id).unwrap(),&[],None).unwrap();
    assert_eq!(runtime.working_directory,std::path::PathBuf::from(parent));
    assert_eq!(runtime.sandbox.as_deref(),Some("workspace-write"));
}

#[test]
fn test_assistant_management_actions_are_discoverable_with_distinct_schemas() {
    let registry = crate::mcp::builtin_mcp::superadmin::registry::build_registry();
    for id in ["assistant.update","assistant.copy","assistant.get_channel_config","assistant.patch_channel_config","assistant.get_mcp_config","assistant.get_skill_config","assistant.set_mcp_mode","assistant.workspace","conversation.group_list","conversation.group_create","conversation.group_get","conversation.group_rename"] {
        assert!(registry.get_meta(id).is_some(),"{id}");
    }
    assert!(registry.get_meta("assistant.create").unwrap().args_schema["properties"].get("provider_id").is_some());
}

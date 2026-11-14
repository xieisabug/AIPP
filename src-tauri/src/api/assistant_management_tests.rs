use super::*;
use rusqlite::Connection;

#[test]
fn test_assistant_management_plugin_database_uri_keeps_local_path_and_rw_mode() {
    let path = std::env::temp_dir().join("用户 数据#%&").join("plugin.db");
    let mut previous = tauri::Url::from_file_path(&path).unwrap();
    previous.query_pairs_mut().append_pair("mode","rw");
    let value = plugin_database_uri(&path).unwrap();
    assert_eq!(value,previous.as_str());
    let parsed = tauri::Url::parse(&value).unwrap();
    assert_eq!(parsed.to_file_path().unwrap(),path);
    assert_eq!(parsed.query_pairs().collect::<Vec<_>>(),vec![("mode".into(),"rw".into())]);
    assert_eq!(parsed.fragment(),None);
}

#[cfg(windows)]
#[test]
fn test_assistant_management_plugin_database_uri_moves_unc_host_into_path() {
    for path in [r"\\server\share\plugin.db",r"\\?\UNC\server\share\plugin.db"] {
        let value = plugin_database_uri(std::path::Path::new(path)).unwrap();
        assert_eq!(value,"file:////server/share/plugin.db?mode=rw");
    }
}

#[cfg(windows)]
#[test]
fn test_assistant_management_plugin_database_uri_preserves_encoded_unc_filename() {
    let value = plugin_database_uri(std::path::Path::new(r"\\server\共享 目录\中文 #?%&\plugin.db")).unwrap();
    assert!(value.starts_with("file:////server/"));
    assert!(value.contains("%20") && value.contains("%23") && value.contains("%3F") && value.contains("%25"));
    // Browser-style URL parsing on Windows normalizes the leading path slashes;
    // assert the actual SQLite wire format, then decode only the filename bytes.
    assert_eq!(value,"file:////server/%E5%85%B1%E4%BA%AB%20%E7%9B%AE%E5%BD%95/%E4%B8%AD%E6%96%87%20%23%3F%25&/plugin.db?mode=rw");
    let filename = value.strip_prefix("file://").unwrap().strip_suffix("?mode=rw").unwrap();
    assert_eq!(urlencoding::decode(filename).unwrap(),"//server/共享 目录/中文 #?%&/plugin.db");
}

fn fixture() -> (AssistantDatabase, LLMDatabase, i64, i64) {
    let db = AssistantDatabase {conn:Connection::open_in_memory().unwrap(),mcp_conn:Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    db.conn.execute_batch("CREATE TABLE assistant_skill_config(id INTEGER PRIMARY KEY,assistant_id INTEGER,skill_identifier TEXT,is_enabled BOOLEAN,priority INTEGER,created_time TEXT DEFAULT CURRENT_TIMESTAMP,UNIQUE(assistant_id,skill_identifier));").unwrap();
    db.conn.execute_batch("ATTACH DATABASE ':memory:' AS assistant_copy_plugins;
        CREATE TABLE assistant_copy_plugins.Plugins(plugin_id INTEGER PRIMARY KEY);
        INSERT INTO assistant_copy_plugins.Plugins VALUES(101),(102);
        CREATE TABLE assistant_copy_plugins.PluginAssistantConfigurations(
            config_id INTEGER PRIMARY KEY AUTOINCREMENT,plugin_id INTEGER NOT NULL,
            assistant_id INTEGER NOT NULL,config_key TEXT NOT NULL,config_value TEXT,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(plugin_id) REFERENCES Plugins(plugin_id),UNIQUE(plugin_id,assistant_id,config_key));").unwrap();
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

fn copy_configuration_counts(db: &AssistantDatabase) -> Vec<i64> {
    ["assistant","assistant_prompt","assistant_prompt_param","assistant_model","assistant_model_config",
        "assistant_mcp_config","assistant_mcp_tool_config","assistant_skill_config","assistant_workspace",
        "assistant_copy_plugins.PluginAssistantConfigurations"].iter()
        .map(|table|db.conn.query_row(&format!("SELECT count(*) FROM {table}"),[],|row|row.get(0)).unwrap()).collect()
}

#[test]
fn test_assistant_management_copy_preserves_plugin_configuration_and_isolates_edits() {
    let (db,llm,normal,_) = fixture();
    let source = create(&db,&llm,normal,0);
    let other = create(&db,&llm,normal,0);
    let model = config_model_id(&db,source).unwrap();
    db.add_assistant_model_config(source,model,"legacyPluginContext","legacy context","string").unwrap();
    for (plugin,key,value) in [
        (101,"content",Some("隐藏首轮上下文\n{\"instructions\":\"保持原样\"}")),
        (101,"enabled",Some("false")),(101,"empty",Some("")),(101,"nullable",None),
        (102,"content",Some("另一个插件的同名配置")),
    ] {
        db.conn.execute("INSERT INTO assistant_copy_plugins.PluginAssistantConfigurations(plugin_id,assistant_id,config_key,config_value) VALUES(?1,?2,?3,?4)",params![plugin,source,key,value]).unwrap();
    }
    db.conn.execute("INSERT INTO assistant_copy_plugins.PluginAssistantConfigurations(plugin_id,assistant_id,config_key,config_value) VALUES(101,?1,'other-only','独立助手')",[other]).unwrap();
    db.conn.execute_batch("CREATE TABLE assistant_copy_plugins.PluginConfigurations(config_key TEXT,config_value TEXT);
        INSERT INTO assistant_copy_plugins.PluginConfigurations VALUES('global','unchanged');
        CREATE TABLE assistant_copy_plugins.PluginStatus(plugin_id INTEGER,is_active BOOLEAN);
        INSERT INTO assistant_copy_plugins.PluginStatus VALUES(101,1),(102,0);
        CREATE TABLE assistant_copy_plugins.PluginData(data_value TEXT);
        INSERT INTO assistant_copy_plugins.PluginData VALUES('runtime-state');").unwrap();
    let copied = profile(&db,&llm,"copy",&json!({"assistant_id":source,"name":"独立副本"}),&[],false).unwrap()["assistant_id"].as_i64().unwrap();
    assert_eq!(config_value(&db,copied,"legacyPluginContext").unwrap().as_deref(),Some("legacy context"));
    assert_eq!(db.conn.query_row("SELECT count(*) FROM assistant_copy_plugins.PluginConfigurations WHERE config_key='global' AND config_value='unchanged'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM assistant_copy_plugins.PluginStatus WHERE (plugin_id=101 AND is_active=1) OR (plugin_id=102 AND is_active=0)",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    assert_eq!(db.conn.query_row("SELECT count(*) FROM assistant_copy_plugins.PluginData WHERE data_value='runtime-state'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    // Use the actual assistantConfig repository API, whose unqualified table
    // resolves to this attached in-memory plugin database.
    let plugins = crate::db::plugin_db::PluginDatabase {conn:db.conn};
    for plugin in [101,102] {
        let original = plugins.get_plugin_assistant_configurations(plugin,source).unwrap();
        let clone = plugins.get_plugin_assistant_configurations(plugin,copied).unwrap();
        assert_eq!(clone.len(),original.len());
        for (a,b) in original.iter().zip(&clone) {
            assert_ne!(a.config_id,b.config_id);
            assert_eq!((a.plugin_id,a.config_key.as_str(),&a.config_value,a.updated_at),(b.plugin_id,b.config_key.as_str(),&b.config_value,b.updated_at));
            assert_eq!(b.assistant_id,copied);
        }
    }
    assert_eq!(plugins.get_plugin_assistant_configurations(101,other).unwrap().len(),1);
    plugins.set_plugin_assistant_configuration(101,copied,"content",Some("副本新内容".into())).unwrap();
    assert_eq!(plugins.get_plugin_assistant_configurations(101,source).unwrap().iter().find(|c|c.config_key=="content").unwrap().config_value.as_deref(),Some("隐藏首轮上下文\n{\"instructions\":\"保持原样\"}"));
    plugins.set_plugin_assistant_configuration(101,source,"enabled",Some("true".into())).unwrap();
    assert_eq!(plugins.get_plugin_assistant_configurations(101,copied).unwrap().iter().find(|c|c.config_key=="enabled").unwrap().config_value.as_deref(),Some("false"));
    plugins.delete_plugin_assistant_configurations(102,copied).unwrap();
    assert_eq!(plugins.get_plugin_assistant_configurations(102,source).unwrap().len(),1);
}

#[test]
fn test_assistant_management_copy_plugin_preview_rolls_back_both_stores() {
    let (db,llm,normal,_) = fixture();
    let source = create(&db,&llm,normal,0);
    db.conn.execute("INSERT INTO assistant_copy_plugins.PluginAssistantConfigurations(plugin_id,assistant_id,config_key,config_value) VALUES(101,?1,'content','preview-context')",[source]).unwrap();
    let before = copy_configuration_counts(&db);
    let preview = profile(&db,&llm,"copy",&json!({"assistant_id":source}),&[],true).unwrap();
    assert_eq!(preview["saved"],false);
    assert_eq!(preview["dry_run"],true);
    assert_eq!(copy_configuration_counts(&db),before);
}

#[test]
fn test_assistant_management_copy_plugin_failure_rolls_back_entire_assistant() {
    let (db,llm,normal,_) = fixture();
    let source = create(&db,&llm,normal,0);
    db.conn.execute("INSERT INTO assistant_copy_plugins.PluginAssistantConfigurations(plugin_id,assistant_id,config_key,config_value) VALUES(101,?1,'content','source-context')",[source]).unwrap();
    db.conn.execute_batch(&format!("CREATE TRIGGER assistant_copy_plugins.reject_plugin_copy BEFORE INSERT ON PluginAssistantConfigurations WHEN NEW.assistant_id<>{source} BEGIN SELECT RAISE(ABORT,'plugin copy rejected'); END;")).unwrap();
    let before = copy_configuration_counts(&db);
    let error = profile(&db,&llm,"copy",&json!({"assistant_id":source}),&[],false).unwrap_err();
    assert!(error.contains("复制插件助手配置失败"),"{error}");
    assert!(error.contains("plugin copy rejected"),"{error}");
    assert_eq!(copy_configuration_counts(&db),before);
}

#[test]
fn test_assistant_management_copy_missing_plugin_store_fails_without_partial_copy() {
    let (db,llm,normal,_) = fixture();
    let source = create(&db,&llm,normal,0);
    db.conn.execute_batch("DETACH DATABASE assistant_copy_plugins").unwrap();
    let before = db.get_assistants().unwrap().len();
    let error = profile(&db,&llm,"copy",&json!({"assistant_id":source}),&[],false).unwrap_err();
    assert!(error.contains("复制插件助手配置失败"),"{error}");
    assert_eq!(db.get_assistants().unwrap().len(),before);
}

#[test]
fn test_assistant_management_claude_directory_readback_matches_launch_config() {
    let (db,llm,_,_) = fixture();
    llm.add_llm_provider("Claude mock","claude_sdk","",false,true).unwrap();
    let provider = llm.conn.last_insert_rowid();
    let id = create(&db,&llm,provider,4);
    llm.add_llm_provider_config(provider,"acp_working_directory","~/provider-project","",false).unwrap();
    let model_id = config_model_id(&db,id).unwrap();
    let field = |value: &Value| value["parameters"].as_array().unwrap().iter()
        .find(|p|p["name"]=="acp_working_directory").unwrap().clone();
    // Simulate existing persisted values, bypassing the new absolute-path setter.
    let absolute = std::env::temp_dir().join("explicit project");
    for raw in ["~/project", "~", r"~\project", "~other/project", absolute.to_str().unwrap(), " "] {
        db.conn.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type) VALUES(?1,?2,'acp_working_directory',?3,'string') ON CONFLICT(assistant_id,name) DO UPDATE SET value=excluded.value",params![id,model_id,raw]).unwrap();
        let value = field(&channel::read(&db,&llm,id).unwrap());
        let config = crate::api::ai::claude_sdk::extract_claude_sdk_config(
            &db.get_assistant_model_configs(id).unwrap(),&llm.get_llm_provider_config(provider).unwrap(),None,vec![]).unwrap();
        assert_eq!(value["effective_value"].as_str().unwrap(), config.working_directory.to_str().unwrap(),"{raw}");
        let expected = match raw {
            "~/project" => dirs::home_dir().unwrap().join("project"),
            "~" => dirs::home_dir().unwrap(),
            " " => dirs::home_dir().unwrap().join("provider-project"),
            #[cfg(windows)]
            r"~\project" => dirs::home_dir().unwrap().join("project"),
            _ => std::path::PathBuf::from(raw),
        };
        assert_eq!(config.working_directory,expected);
        assert_eq!(value["source"],if raw.trim().is_empty(){"provider"}else{"assistant"});
    }
    db.conn.execute("DELETE FROM assistant_model_config WHERE assistant_id=?1 AND name='acp_working_directory'",[id]).unwrap();
    let inherited = field(&channel::read(&db,&llm,id).unwrap());
    assert_eq!(inherited["source"],"provider");
    assert_eq!(std::path::Path::new(inherited["effective_value"].as_str().unwrap()),dirs::home_dir().unwrap().join("provider-project"));
    llm.conn.execute("DELETE FROM llm_provider_config WHERE llm_provider_id=?1 AND name='acp_working_directory'",[provider]).unwrap();
    let default = field(&channel::read(&db,&llm,id).unwrap());
    let config = crate::api::ai::claude_sdk::extract_claude_sdk_config(&db.get_assistant_model_configs(id).unwrap(),&[],None,vec![]).unwrap();
    assert_eq!(default["source"],"channel_default");
    assert_eq!(default["effective_value"].as_str().unwrap(),config.working_directory.to_str().unwrap());
}

#[test]
fn test_assistant_management_other_agent_directory_readback_matches_launch_config() {
    let (db,llm,_,_) = fixture();
    for api in ["acp","pi","codex_app_server"] {
        llm.add_llm_provider("Agent mock",api,"",false,true).unwrap();
        let provider = llm.conn.last_insert_rowid();
        if api=="acp" { llm.add_llm_provider_config(provider,"acp_cli_command","gemini","",false).unwrap(); }
        llm.add_llm_provider_config(provider,"acp_working_directory","~","",false).unwrap();
        let id = create(&db,&llm,provider,4);
        let configs = db.get_assistant_model_configs(id).unwrap();
        let providers = llm.get_llm_provider_config(provider).unwrap();
        let runtime = match api {
            "acp" => crate::api::ai::acp::extract_acp_config(&configs,&providers).unwrap().working_directory,
            "pi" => crate::api::ai::agent_driver::native_agents::configuration(&configs,&providers,None).unwrap().cwd,
            _ => crate::api::ai::codex_app_server::extract_codex_app_server_config(&configs,&providers,None).unwrap().working_directory,
        };
        let read = channel::read(&db,&llm,id).unwrap();
        let field = read["parameters"].as_array().unwrap().iter().find(|p|p["name"]=="acp_working_directory").unwrap();
        assert_eq!(field["effective_value"].as_str().unwrap(),runtime.to_str().unwrap(),"{api}");
        assert_eq!(runtime,if api=="codex_app_server" {std::path::PathBuf::from("~")} else {dirs::home_dir().unwrap()});
    }
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

fn channel_field(value: &Value, name: &str) -> Value {
    value["parameters"].as_array().unwrap().iter().find(|p|p["name"]==name).unwrap().clone()
}

#[test]
fn test_assistant_management_stream_readback_matches_requests_after_clear() {
    let (db,llm,normal,_) = fixture();
    let id = create(&db,&llm,normal,0);
    assert_eq!(channel_field(&channel::read(&db,&llm,id).unwrap(),"stream")["effective_value"],"true");
    let cleared = channel::patch(&db,&llm,id,&json!({}),&json!(["stream"]),false).unwrap();
    assert_eq!(channel_field(&cleared,"stream")["source"],"channel_default");
    assert_eq!(channel_field(&cleared,"stream")["effective_value"],"false");
    assert!(!crate::api::ai::config::stream_enabled(config_value(&db,id,"stream").unwrap().as_deref()));
    for value in [false,true] {
        let saved = channel::patch(&db,&llm,id,&json!({"stream":value}),&json!([]),false).unwrap();
        let runtime = crate::api::ai::config::stream_enabled(config_value(&db,id,"stream").unwrap().as_deref());
        assert_eq!(runtime,value);
        assert_eq!(channel_field(&saved,"stream")["effective_value"],value.to_string());
    }
}

#[test]
fn test_assistant_management_legacy_provider_supports_channel_patch_and_workspace() {
    for has_model_row in [true,false] {
        let (db,llm,_,codex) = fixture();
        let id = create(&db,&llm,codex,4);
        let model = db.get_assistant_model(id).unwrap()[0].id;
        db.add_assistant_model_config(id,if has_model_row{model}else{0},"acp_provider",&codex.to_string(),"string").unwrap();
        if has_model_row {
            db.conn.execute("UPDATE assistant_model SET provider_id=0 WHERE assistant_id=?1",[id]).unwrap();
        } else { db.conn.execute("DELETE FROM assistant_model WHERE assistant_id=?1",[id]).unwrap(); }
        let before = channel::read(&db,&llm,id).unwrap();
        assert_eq!(before["channel"],"codex_app_server");
        let saved = channel::patch(&db,&llm,id,&json!({"codex_sandbox":"read-only"}),&json!([]),false).unwrap();
        assert_eq!(channel_field(&saved,"codex_sandbox")["effective_value"],"read-only");
        let cwd = directory(std::env::current_dir().unwrap().to_str().unwrap()).unwrap();
        channel::workspace(&db,&llm,id,&json!({"action":"add","path":cwd}),false).unwrap();
        channel::workspace(&db,&llm,id,&json!({"action":"set_directory","path":cwd}),false).unwrap();
        let workspaces = channel::workspace(&db,&llm,id,&json!({"action":"list"}),false).unwrap();
        assert_eq!(workspaces["workspaces"].as_array().unwrap().len(),1);
        let models = db.get_assistant_model(id).unwrap();
        let configs = db.get_assistant_model_configs(id).unwrap();
        assert_eq!(crate::api::assistant_api::resolve_acp_provider_id(&models,&configs),Some(codex));
        let runtime = crate::api::ai::codex_app_server::extract_codex_app_server_config(&configs,&[],None).unwrap();
        assert_eq!(runtime.working_directory,std::path::PathBuf::from(cwd));
        assert_eq!(runtime.sandbox.as_deref(),Some("read-only"));
        assert_eq!(models.first().map(|m|m.provider_id),if has_model_row{Some(0)}else{None});
    }
}

#[test]
fn test_assistant_management_provider_priority_does_not_replace_missing_modern_binding() {
    let (db,llm,normal,codex) = fixture();
    let id = create(&db,&llm,codex,4);
    let model = db.get_assistant_model(id).unwrap()[0].id;
    db.add_assistant_model_config(id,model,"acp_provider",&normal.to_string(),"string").unwrap();
    assert_eq!(channel::read(&db,&llm,id).unwrap()["channel"],"codex_app_server");
    db.conn.execute("UPDATE assistant_model SET provider_id=999999 WHERE assistant_id=?1",[id]).unwrap();
    assert!(channel::read(&db,&llm,id).unwrap_err().contains("999999"));
    assert!(channel::workspace(&db,&llm,id,&json!({"action":"list"}),false).unwrap_err().contains("999999"));
}

#[test]
fn test_assistant_management_mcp_mode_uses_legacy_config_target_in_preview_and_save() {
    for has_model in [true,false] {
        let (db,llm,_,codex) = fixture();
        let id = create(&db,&llm,codex,4);
        db.add_assistant_model_config(id,0,"acp_provider",&codex.to_string(),"string").unwrap();
        if !has_model {db.conn.execute("DELETE FROM assistant_model WHERE assistant_id=?1",[id]).unwrap();}
        else {db.conn.execute("UPDATE assistant_model SET provider_id=0 WHERE assistant_id=?1",[id]).unwrap();}
        set_mcp_mode(&db,id,"static",true).unwrap();
        assert_eq!(config_value(&db,id,"dynamic_mcp_loading_enabled").unwrap().as_deref(),Some("true"));
        for mode in ["static","dynamic"] {
            set_mcp_mode(&db,id,mode,false).unwrap();
            assert_eq!(config_value(&db,id,"dynamic_mcp_loading_enabled").unwrap().as_deref(),Some(if mode=="dynamic" {"true"}else{"false"}));
            assert_eq!(config_value(&db,id,"mcp_loading_mode_explicit").unwrap().as_deref(),Some("true"));
        }
        assert!(set_mcp_mode(&db,id,"bad-mode",false).is_err());
        assert_eq!(crate::api::assistant_api::resolve_acp_provider_id(&db.get_assistant_model(id).unwrap(),&db.get_assistant_model_configs(id).unwrap()),Some(codex));
    }
}

#[test]
fn test_assistant_permissions_channel_expansion_requires_approval_and_rechecks_snapshot() {
    use permissions::Policy;
    let (db,llm,_,codex) = fixture();
    let id = create(&db,&llm,codex,4);
    let set = json!({"codex_sandbox":"danger-full-access","codex_approval_policy":"never","codex_approvals_reviewer":"auto_review"});
    for single in [json!({"codex_sandbox":"danger-full-access"}),json!({"codex_approval_policy":"never"}),json!({"codex_approvals_reviewer":"auto_review"}),json!({"tool_review_mode":"auto_model"})] {
        assert!(channel::patch_managed(&db,&llm,id,&single,&json!([]),false,Some(Policy::Ordinary)).is_err());
    }
    let preview = channel::patch_managed(&db,&llm,id,&set,&json!([]),true,Some(Policy::Ordinary)).unwrap();
    let snapshot = &preview["permission_change"];
    assert_eq!(snapshot["requires_user_approval"],true);
    assert!(channel::patch_managed(&db,&llm,id,&set,&json!([]),false,Some(Policy::Ordinary)).unwrap_err().contains("assistant.patch_agent_permissions"));
    assert!(config_value(&db,id,"codex_sandbox").unwrap().is_none());
    channel::patch_managed(&db,&llm,id,&set,&json!([]),false,Some(Policy::Approved(snapshot))).unwrap();
    assert_eq!(config_value(&db,id,"codex_sandbox").unwrap().as_deref(),Some("danger-full-access"));
    assert!(channel::patch_managed(&db,&llm,id,&set,&json!([]),false,Some(Policy::Approved(snapshot))).unwrap_err().contains("等待批准期间"));
    // Tightening remains an ordinary configuration change.
    channel::patch_managed(&db,&llm,id,&json!({"codex_sandbox":"read-only","codex_approval_policy":"untrusted","codex_approvals_reviewer":"user"}),&json!([]),false,Some(Policy::Ordinary)).unwrap();
}

#[test]
fn test_assistant_permissions_inherited_expansion_and_provider_drift_are_rejected() {
    use permissions::Policy;
    let (db,llm,_,codex) = fixture();
    let id = create(&db,&llm,codex,4);
    llm.add_llm_provider_config(codex,"codex_sandbox","danger-full-access","",false).unwrap();
    channel::patch(&db,&llm,id,&json!({"codex_sandbox":"read-only"}),&json!([]),false).unwrap();
    assert!(channel::patch_managed(&db,&llm,id,&json!({}),&json!(["codex_sandbox"]),false,Some(Policy::Ordinary)).is_err());
    assert_eq!(config_value(&db,id,"codex_sandbox").unwrap().as_deref(),Some("read-only"));
    let preview = channel::patch_managed(&db,&llm,id,&json!({}),&json!(["codex_sandbox"]),true,Some(Policy::Ordinary)).unwrap();
    llm.conn.execute("UPDATE llm_provider_config SET value='workspace-write' WHERE llm_provider_id=?1 AND name='codex_sandbox'",[codex]).unwrap();
    assert!(channel::patch_managed(&db,&llm,id,&json!({}),&json!(["codex_sandbox"]),false,Some(Policy::Approved(&preview["permission_change"]))).is_err());
    assert_eq!(config_value(&db,id,"codex_sandbox").unwrap().as_deref(),Some("read-only"));
}

#[test]
fn test_assistant_permissions_workspace_grant_is_one_time_and_narrowing_is_ordinary() {
    use permissions::Policy;
    let (db,llm,normal,_) = fixture();
    let id = create(&db,&llm,normal,0);
    let path = directory(std::env::current_dir().unwrap().to_str().unwrap()).unwrap();
    let args = json!({"action":"add","path":path});
    let preview = channel::workspace_managed(&db,&llm,id,&args,true,Some(Policy::Ordinary)).unwrap();
    assert!(db.get_assistant_workspaces(id).unwrap().is_empty());
    assert!(channel::workspace_managed(&db,&llm,id,&args,false,Some(Policy::Ordinary)).unwrap_err().contains("assistant.grant_workspace"));
    assert!(db.get_assistant_workspaces(id).unwrap().is_empty());
    let other = std::path::Path::new(&path).parent().unwrap().to_string_lossy().to_string();
    assert!(channel::workspace_managed(&db,&llm,id,&json!({"action":"add","path":other}),false,Some(Policy::Approved(&preview["permission_change"]))).is_err());
    assert!(db.get_assistant_workspaces(id).unwrap().is_empty());
    channel::workspace_managed(&db,&llm,id,&args,false,Some(Policy::Approved(&preview["permission_change"]))).unwrap();
    channel::workspace_managed(&db,&llm,id,&args,false,Some(Policy::Ordinary)).unwrap();
    let wid = db.get_assistant_workspaces(id).unwrap()[0].id;
    channel::workspace_managed(&db,&llm,id,&json!({"action":"remove","workspace_id":wid}),false,Some(Policy::Ordinary)).unwrap();
    assert!(db.get_assistant_workspaces(id).unwrap().is_empty());
}

#[test]
fn test_assistant_permissions_grant_actions_have_static_user_gates() {
    use crate::mcp::builtin_mcp::superadmin::{registry::build_registry,types::ApprovalPolicy};
    let registry = build_registry();
    for id in ["assistant.patch_agent_permissions","assistant.grant_workspace"] {
        let meta = registry.get_meta(id).unwrap();
        assert!(meta.requires_approval);
        assert_eq!(meta.approval_policy,ApprovalPolicy::UserApprovalRequired);
        assert!(crate::mcp::builtin_mcp::superadmin::action_approval_gate(id,false).is_some());
        assert!(crate::mcp::builtin_mcp::superadmin::action_approval_gate(id,true).is_none());
    }
    assert!(!registry.get_meta("assistant.patch_channel_config").unwrap().requires_approval);
    assert!(!registry.get_meta("assistant.workspace").unwrap().requires_approval);
}

#[test]
fn test_assistant_permissions_cwd_expansion_tracks_workspace_write_scope() {
    let cwd = std::env::current_dir().unwrap();
    let parent = cwd.parent().unwrap();
    let before = json!({"workspace_paths":[],"permissions":{"codex_sandbox":"workspace-write","acp_working_directory":cwd.to_string_lossy()}});
    let mut after = before.clone();
    after["permissions"]["acp_working_directory"] = json!(parent.to_string_lossy());
    assert!(permissions::expands(&before,&after));
    after["permissions"]["codex_sandbox"]=json!("read-only");
    assert!(!permissions::expands(&before,&after));
    // Changing an execution start directory alone does not grant AIPP path trust.
    let before = json!({"workspace_paths":[],"permissions":{"acp_working_directory":cwd.to_string_lossy()}});
    let after = json!({"workspace_paths":[],"permissions":{"acp_working_directory":parent.to_string_lossy()}});
    assert!(!permissions::expands(&before,&after));
}

//! Production deletion paths against isolated in-memory profiles, including
//! attached in-memory peers. No file-backed database is opened by these tests.
use super::test_helpers::create_test_db;
use crate::db::{assistant_db::AssistantDatabase, cascade_delete, llm_db::{LLMDatabase, LLMModel},
    mcp_db::MCPDatabase, plugin_db::PluginDatabase};
use rusqlite::Connection;

fn assistant_db() -> AssistantDatabase {
    let db = AssistantDatabase { conn: Connection::open_in_memory().unwrap(), mcp_conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    db.conn.execute_batch("CREATE TABLE assistant_skill_config(id INTEGER PRIMARY KEY, assistant_id INTEGER REFERENCES assistant(id), skill_identifier TEXT);").unwrap();
    db
}

fn mcp_db() -> MCPDatabase {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    db
}

fn llm_db() -> LLMDatabase {
    let db = LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    db
}

fn peer(conn: &Connection, alias: &str, source: &Connection) {
    conn.execute_batch(&format!("ATTACH DATABASE ':memory:' AS {alias}")).unwrap();
    let tables: Vec<(String, String)> = source.prepare("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY rowid").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    for (name, sql) in tables {
        conn.execute_batch(&sql.replacen(&name, &format!("{alias}.\"{name}\""), 1)).unwrap();
    }
}

fn count(conn: &Connection, table: &str, predicate: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"), [], |r| r.get(0)).unwrap()
}

fn seed_messages(conn: &Connection) {
    conn.execute_batch("INSERT INTO conversation(id,name,created_time) VALUES(10,'delete','now'),(20,'keep','now');
        INSERT INTO message(id,conversation_id,message_type,content,created_time) VALUES(11,10,'user','delete','now'),(21,20,'user','keep','now');
        INSERT INTO message_attachment(message_id,attachment_type) VALUES(11,1),(21,1);").unwrap();
}

fn external_tables(conn: &Connection) {
    conn.execute_batch("CREATE TABLE acp_session(conversation_id INTEGER, agent_kind TEXT, session_id TEXT);
        CREATE TABLE external_channel_message_link(id INTEGER PRIMARY KEY, conversation_id INTEGER, local_message_id INTEGER);
        CREATE TABLE external_channel_relay_scope(id INTEGER PRIMARY KEY, conversation_id INTEGER, start_after_local_message_id INTEGER, last_delivered_local_message_id INTEGER);
        CREATE TABLE external_channel_message_delivery(id INTEGER PRIMARY KEY, scope_id INTEGER, conversation_id INTEGER, local_message_id INTEGER);
        INSERT INTO acp_session VALUES(10,'codex','native-session'),(20,'acp','keep-session');
        INSERT INTO external_channel_message_link VALUES(1,10,11),(2,20,21);
        INSERT INTO external_channel_relay_scope VALUES(1,10,11,11),(2,20,21,21);
        INSERT INTO external_channel_message_delivery VALUES(1,1,10,11),(2,2,20,21);").unwrap();
}

fn mcp_history(conn: &Connection, schema: &str) {
    conn.execute_batch(&format!("INSERT INTO {schema}.mcp_server(id,name,transport_type) VALUES(1,'server','stdio');
        INSERT INTO {schema}.mcp_tool_call(id,conversation_id,message_id,assistant_message_id,server_id,server_name,tool_name,parameters)
            VALUES(1,10,11,NULL,1,'server','tool','{{}}'),(2,20,21,11,1,'server','tool','{{}}'),(3,20,21,NULL,1,'server','tool','{{}}');
        INSERT INTO {schema}.tool_review_log(conversation_id,mcp_tool_call_id,server_name,tool_name,parameters,verdict)
            VALUES(10,1,'server','tool','{{}}','allow'),(20,2,'server','tool','{{}}','allow'),(20,3,'server','tool','{{}}','allow');
        INSERT INTO {schema}.conversation_mcp_loaded_tool(conversation_id,tool_id,loaded_schema_hash,loaded_epoch,status)
            VALUES(10,1,'hash',1,'valid'),(20,1,'hash',1,'valid');")).unwrap();
}

#[test]
fn test_cascade_delete_conversation_cleans_cross_database_children_and_preserves_siblings() {
    for foreign_keys in [false, true] {
        let conn = create_test_db();
        conn.pragma_update(None, "foreign_keys", foreign_keys).unwrap();
        seed_messages(&conn);
        external_tables(&conn);
        peer(&conn, "aipp_delete_mcp", &mcp_db().conn);
        mcp_history(&conn, "aipp_delete_mcp");
        conn.execute_batch("ATTACH DATABASE ':memory:' AS aipp_delete_plugin;
            CREATE TABLE aipp_delete_plugin.PluginHookAuditLog(id INTEGER PRIMARY KEY, conversation_id INTEGER, message_id INTEGER);
            INSERT INTO aipp_delete_plugin.PluginHookAuditLog VALUES(1,10,11),(2,20,21);").unwrap();
        cascade_delete::delete(&conn, "conversation", 10).unwrap();
        cascade_delete::delete(&conn, "conversation", 10).unwrap();
        for table in ["message", "external_channel_message_link", "external_channel_relay_scope", "external_channel_message_delivery", "acp_session",
            "aipp_delete_mcp.mcp_tool_call", "aipp_delete_mcp.tool_review_log", "aipp_delete_mcp.conversation_mcp_loaded_tool", "aipp_delete_plugin.PluginHookAuditLog"] {
            assert_eq!(count(&conn, table, "conversation_id=10"), 0, "{table}");
            assert!(count(&conn, table, "conversation_id=20") > 0, "{table}");
        }
        assert_eq!(count(&conn, "aipp_delete_mcp.mcp_tool_call", "id=2"), 0); // assistant_message_id was deleted
        assert_eq!(count(&conn, "message_attachment", "message_id=11"), 0);
        assert_eq!(count(&conn, "message_attachment", "message_id=21"), 1);
    }
}

#[test]
fn test_cascade_delete_message_preserves_descendants_and_relay_watermarks() {
    let conn = create_test_db(); seed_messages(&conn); external_tables(&conn);
    conn.execute_batch("UPDATE message SET parent_id=11 WHERE id=21;").unwrap();
    cascade_delete::delete(&conn, "message", 11).unwrap();
    assert_eq!(count(&conn, "message", "id=21 AND parent_id IS NULL"), 1);
    assert_eq!(count(&conn, "external_channel_message_link", "local_message_id=11"), 0);
    assert_eq!(count(&conn, "external_channel_message_delivery", "local_message_id=11"), 0);
    assert_eq!(count(&conn, "external_channel_relay_scope", "start_after_local_message_id=11 AND last_delivered_local_message_id=11"), 1);
    assert_eq!(count(&conn, "conversation", "id=10"), 1);
}

#[test]
fn test_cascade_delete_cross_database_failure_rolls_back_all_children() {
    let conn = create_test_db(); seed_messages(&conn);
    peer(&conn, "aipp_delete_mcp", &mcp_db().conn); mcp_history(&conn, "aipp_delete_mcp");
    conn.execute_batch("CREATE TRIGGER aipp_delete_mcp.fail_review_delete BEFORE DELETE ON tool_review_log
        BEGIN SELECT RAISE(ABORT,'injected review cleanup failure'); END;").unwrap();
    let error = cascade_delete::delete(&conn, "conversation", 10).unwrap_err().to_string();
    assert!(error.contains("conversation id=10") && error.contains("tool_review_log") && error.contains("injected review cleanup failure"), "{error}");
    assert_eq!(count(&conn, "message_attachment", "message_id=11"), 1);
    assert_eq!(count(&conn, "message", "id=11"), 1);
    assert_eq!(count(&conn, "conversation", "id=10"), 1);
    assert_eq!(count(&conn, "aipp_delete_mcp.mcp_tool_call", "id=1"), 1);
    assert!(conn.is_autocommit());
}

#[test]
fn test_cascade_delete_savepoint_respects_outer_transaction() {
    let conn = create_test_db(); seed_messages(&conn);
    conn.execute_batch("BEGIN").unwrap();
    cascade_delete::delete(&conn, "conversation", 10).unwrap();
    assert!(!conn.is_autocommit());
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(count(&conn, "conversation", "id=10"), 1);
    assert_eq!(count(&conn, "message_attachment", "message_id=11"), 1);
}

#[test]
fn test_cascade_delete_assistant_removes_owned_config_and_keeps_history() {
    for foreign_keys in [false, true] {
        let db = assistant_db();
        db.conn.pragma_update(None, "foreign_keys", foreign_keys).unwrap();
        let id = db.add_assistant("delete", "", Some(0), false).unwrap();
        let keep = db.add_assistant("keep", "", Some(0), false).unwrap();
        for assistant in [id, keep] {
            let prompt = db.add_assistant_prompt(assistant, "prompt").unwrap();
            let model = db.add_assistant_model(assistant, 77, "model", "alias").unwrap();
            db.add_assistant_model_config(assistant, model, "temperature", "0.7", "number").unwrap();
            db.conn.execute("INSERT INTO assistant_prompt_param(assistant_id,assistant_prompt_id,param_name) VALUES(?1,?2,'param')", [assistant,prompt]).unwrap();
            for (table, column, value) in [("assistant_mcp_config","mcp_server_id","1"),("assistant_mcp_tool_config","mcp_tool_id","1"),
                ("assistant_skill_config","skill_identifier","'skill'"),("assistant_workspace","path","'/workspace'"),
                ("assistant_summary","summary","'summary'")] {
                db.conn.execute(&format!("INSERT INTO {table}(assistant_id,{column}) VALUES(?1,{value})"), [assistant]).unwrap();
            }
        }
        peer(&db.conn,"aipp_delete_conversation",&create_test_db());
        db.conn.execute("INSERT INTO aipp_delete_conversation.conversation(id,name,assistant_id,created_time) VALUES(10,'history',?1,'now')",[id]).unwrap();
        db.conn.execute_batch("ATTACH DATABASE ':memory:' AS aipp_delete_plugin;
            CREATE TABLE aipp_delete_plugin.PluginAssistantConfigurations(assistant_id INTEGER);
            ATTACH DATABASE ':memory:' AS aipp_delete_artifacts;
            CREATE TABLE aipp_delete_artifacts.artifacts_collection(id INTEGER PRIMARY KEY, assistant_id INTEGER, code TEXT);").unwrap();
        db.conn.execute("INSERT INTO aipp_delete_plugin.PluginAssistantConfigurations VALUES(?1)",[id]).unwrap();
        db.conn.execute("INSERT INTO aipp_delete_artifacts.artifacts_collection VALUES(1,?1,'keep code')",[id]).unwrap();
        db.delete_assistant(id).unwrap();
        for table in ["assistant_prompt_param","assistant_model_config","assistant_model","assistant_prompt","assistant_mcp_config",
            "assistant_mcp_tool_config","assistant_skill_config","assistant_workspace","assistant_summary"] {
            assert_eq!(count(&db.conn, table, &format!("assistant_id={id}")), 0, "{table}");
            assert_eq!(count(&db.conn, table, &format!("assistant_id={keep}")), 1, "{table}");
        }
        assert_eq!(count(&db.conn,"aipp_delete_conversation.conversation","id=10 AND assistant_id=1"),1);
        assert_eq!(count(&db.conn,"aipp_delete_artifacts.artifacts_collection","id=1 AND assistant_id IS NULL AND code='keep code'"),1);
        assert_eq!(count(&db.conn,"aipp_delete_plugin.PluginAssistantConfigurations","1=1"),0);
    }
}

#[test]
fn test_cascade_delete_assistant_with_scheduled_task_is_rejected_without_changes() {
    let db=assistant_db(); let id=db.add_assistant("used","",Some(0),false).unwrap();
    db.add_assistant_prompt(id,"keep prompt").unwrap();
    db.conn.execute_batch("ATTACH DATABASE ':memory:' AS aipp_delete_conversation;
        CREATE TABLE aipp_delete_conversation.scheduled_task(id INTEGER, name TEXT, assistant_id INTEGER);").unwrap();
    db.conn.execute("INSERT INTO aipp_delete_conversation.scheduled_task VALUES(42,'每日报告',?1)",[id]).unwrap();
    let error=db.delete_assistant(id).unwrap_err().to_string();
    assert!(error.contains("每日报告") && error.contains("42") && error.contains(&format!("助手 {id}")),"{error}");
    assert_eq!(count(&db.conn,"assistant",&format!("id={id}")),1);
    assert_eq!(count(&db.conn,"assistant_prompt",&format!("assistant_id={id}")),1);
}

#[test]
fn test_cascade_delete_plugin_database_method_cleans_all_children() {
    let db=PluginDatabase{conn:Connection::open_in_memory().unwrap()}; db.create_tables().unwrap();
    db.conn.pragma_update(None,"foreign_keys",true).unwrap();
    let id=db.add_plugin("delete","1","delete",None,None).unwrap();
    let keep=db.add_plugin("keep","1","keep",None,None).unwrap();
    for plugin in [id,keep] {
        db.conn.execute("INSERT INTO PluginStatus(plugin_id) VALUES(?1)",[plugin]).unwrap();
        db.conn.execute("INSERT INTO PluginConfigurations(plugin_id,config_key) VALUES(?1,'key')",[plugin]).unwrap();
        db.conn.execute("INSERT INTO PluginAssistantConfigurations(plugin_id,assistant_id,config_key) VALUES(?1,2,'key')",[plugin]).unwrap();
        db.conn.execute("INSERT INTO PluginData(plugin_id,session_id,data_key) VALUES(?1,'session','key')",[plugin]).unwrap();
        db.conn.execute("INSERT INTO PluginRuntime(plugin_id,runtime_type,entry) VALUES(?1,'js','index.js')",[plugin]).unwrap();
        db.conn.execute("INSERT INTO PluginHookRegistration(plugin_id,hook_name,hook_kind) VALUES(?1,'hook','observe')",[plugin]).unwrap();
        db.conn.execute("INSERT INTO PluginHookAuditLog(plugin_id,hook_name,status) VALUES(?1,'hook','success')",[plugin]).unwrap();
    }
    db.delete_plugin(id).unwrap();
    for table in ["PluginStatus","PluginConfigurations","PluginAssistantConfigurations","PluginData","PluginRuntime","PluginHookRegistration","PluginHookAuditLog"] {
        assert_eq!(count(&db.conn,table,&format!("plugin_id={id}")),0,"{table}");
        assert_eq!(count(&db.conn,table,&format!("plugin_id={keep}")),1,"{table}");
    }
}

#[test]
fn test_cascade_delete_mcp_server_cleans_review_catalog_and_assistant_bindings() {
    for foreign_keys in [false,true] {
        let db=mcp_db(); db.conn.pragma_update(None,"foreign_keys",foreign_keys).unwrap();
        mcp_history(&db.conn,"main");
        db.conn.execute_batch("INSERT INTO mcp_server(id,name,transport_type) VALUES(2,'keep','stdio');
            INSERT INTO mcp_server_tool(id,server_id,tool_name) VALUES(7,1,'delete'),(8,2,'keep');
            UPDATE conversation_mcp_loaded_tool SET tool_id=7 WHERE conversation_id=10;
            UPDATE conversation_mcp_loaded_tool SET tool_id=8 WHERE conversation_id=20;
            INSERT INTO mcp_tool_catalog(tool_id,server_id,tool_name,schema_hash) VALUES(7,1,'delete','hash'),(8,2,'keep','hash');
            INSERT INTO mcp_server_capability_epoch_catalog(server_id) VALUES(1),(2);
            INSERT INTO mcp_server_resource(server_id,resource_uri,resource_name,resource_type) VALUES(1,'uri','resource','text');
            INSERT INTO mcp_server_prompt(server_id,prompt_name) VALUES(1,'prompt');
            ATTACH DATABASE ':memory:' AS aipp_delete_assistant;
            CREATE TABLE aipp_delete_assistant.assistant_mcp_config(mcp_server_id INTEGER);
            CREATE TABLE aipp_delete_assistant.assistant_mcp_tool_config(mcp_tool_id INTEGER);
            INSERT INTO aipp_delete_assistant.assistant_mcp_config VALUES(1),(2);
            INSERT INTO aipp_delete_assistant.assistant_mcp_tool_config VALUES(7),(8);").unwrap();
        db.delete_mcp_server(1).unwrap();
        for table in ["mcp_server_tool","mcp_server_resource","mcp_server_prompt","mcp_tool_call","mcp_tool_catalog","mcp_server_capability_epoch_catalog"] {
            assert_eq!(count(&db.conn,table,"server_id=1"),0,"{table}");
        }
        assert_eq!(count(&db.conn,"tool_review_log","1=1"),0);
        assert_eq!(count(&db.conn,"conversation_mcp_loaded_tool","tool_id=7"),0);
        assert_eq!(count(&db.conn,"conversation_mcp_loaded_tool","tool_id=8"),1);
        assert_eq!(count(&db.conn,"aipp_delete_assistant.assistant_mcp_tool_config","mcp_tool_id=7"),0);
        assert_eq!(count(&db.conn,"aipp_delete_assistant.assistant_mcp_config","mcp_server_id=2"),1);
    }
}

#[test]
fn test_cascade_delete_tool_refresh_preserves_invalid_capability_explanation() {
    let db=mcp_db(); mcp_history(&db.conn,"main");
    db.conn.execute_batch("INSERT INTO mcp_server_tool(id,server_id,tool_name) VALUES(1,1,'gone'),(2,1,'keep');
        INSERT INTO mcp_tool_catalog(tool_id,server_id,tool_name,schema_hash) VALUES(1,1,'gone','hash');").unwrap();
    assert_eq!(db.delete_mcp_server_tools_not_in(1,&["keep".into()]).unwrap(),1);
    assert_eq!(count(&db.conn,"mcp_server_tool","id=2"),1);
    assert_eq!(count(&db.conn,"mcp_tool_catalog","tool_id=1"),0);
    assert_eq!(count(&db.conn,"conversation_mcp_loaded_tool","tool_id=1 AND status='invalid_deleted' AND invalid_reason IS NOT NULL"),2);
    assert_eq!(count(&db.conn,"mcp_tool_call","1=1"),3); // retained history does not belong to tool definitions
    db.conn.execute("UPDATE conversation_mcp_loaded_tool SET loaded_server_name='server' WHERE tool_id=1", []).unwrap();
    db.delete_mcp_server(1).unwrap();
    assert_eq!(count(&db.conn,"conversation_mcp_loaded_tool","tool_id=1"),0);
}

fn model(code: &str) -> LLMModel {
    LLMModel { id:0,name:code.into(),llm_provider_id:77,code:code.into(),description:String::new(),vision_support:false,audio_support:false,video_support:false,request_mode:"responses".into() }
}
fn seed_provider(db:&LLMDatabase) {
    db.conn.execute("INSERT INTO llm_provider(id,name,api_type) VALUES(77,'test','openai')",[]).unwrap();
    db.add_llm_model("keep",77,"keep","",false,false,false).unwrap();
    db.add_llm_model("gone",77,"gone","",false,false,false).unwrap();
    db.upsert_model_request_mode(77,"keep","responses").unwrap();
    db.upsert_model_request_mode(77,"gone","responses").unwrap();
}

#[test]
fn test_cascade_delete_model_refresh_keeps_ids_and_surviving_preferences() {
    let db=llm_db();seed_provider(&db);
    let old=db.conn.query_row("SELECT id FROM llm_model WHERE llm_provider_id=77 AND code='keep'",[],|r|r.get::<_,i64>(0)).unwrap();
    db.replace_llm_models(77,&[model("keep"),model("new")],false).unwrap();
    assert_eq!(count(&db.conn,"llm_model",&format!("id={old} AND code='keep'")),1);
    assert_eq!(db.get_model_request_mode(77,"keep").unwrap().as_deref(),Some("responses"));
    assert_eq!(db.get_model_request_mode(77,"gone").unwrap(),None);
    db.delete_llm_model(77,"keep".into()).unwrap();
    assert_eq!(db.get_model_request_mode(77,"keep").unwrap(),None);
}

#[test]
fn test_cascade_delete_model_refresh_failure_restores_selection_and_preferences() {
    let db=llm_db();seed_provider(&db);
    db.conn.execute_batch("CREATE TRIGGER fail_model_insert BEFORE INSERT ON llm_model WHEN NEW.code='bad'
        BEGIN SELECT RAISE(ABORT,'injected model failure'); END;").unwrap();
    assert!(db.replace_llm_models(77,&[model("keep"),model("bad")],true).is_err());
    assert_eq!(count(&db.conn,"llm_model","llm_provider_id=77"),2);
    assert_eq!(db.get_model_request_mode(77,"gone").unwrap().as_deref(),Some("responses"));
}

#[test]
fn test_cascade_delete_provider_unbinds_slots_and_preserves_message_history() {
    let db=llm_db();seed_provider(&db);
    peer(&db.conn,"aipp_delete_assistant",&assistant_db().conn);
    peer(&db.conn,"aipp_delete_conversation",&create_test_db());
    db.conn.execute_batch("INSERT INTO aipp_delete_assistant.assistant(id,name) VALUES(2,'keep');
        INSERT INTO aipp_delete_assistant.assistant_model(id,assistant_id,provider_id,model_code,alias) VALUES(3,2,77,'keep','alias');
        INSERT INTO aipp_delete_assistant.assistant_model_config(assistant_id,assistant_model_id,name,value) VALUES(2,3,'temperature','0.7');
        INSERT INTO aipp_delete_conversation.conversation(id,name,created_time) VALUES(10,'history','now');
        INSERT INTO aipp_delete_conversation.message(id,conversation_id,message_type,content,created_time,llm_model_id,llm_model_name)
            SELECT 11,10,'response','keep history','now',id,'keep model name' FROM llm_model WHERE llm_provider_id=77 AND code='keep';").unwrap();
    db.conn.pragma_update(None,"foreign_keys",true).unwrap();
    db.delete_llm_provider(77).unwrap();
    assert_eq!(count(&db.conn,"llm_model","llm_provider_id=77"),0);
    assert_eq!(count(&db.conn,"llm_model_request_mode_preference","llm_provider_id=77"),0);
    assert_eq!(count(&db.conn,"aipp_delete_assistant.assistant_model","id=3 AND provider_id=0 AND model_code='' AND alias=''"),1);
    assert_eq!(count(&db.conn,"aipp_delete_assistant.assistant_model_config","assistant_model_id=3"),1);
    assert_eq!(count(&db.conn,"aipp_delete_conversation.message","id=11 AND llm_model_id IS NULL AND llm_model_name='keep model name'"),1);
}

#[test]
fn test_cascade_delete_scheduled_task_failure_keeps_task_logs_and_runs() {
    let conn=Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE scheduled_task(id INTEGER PRIMARY KEY);
        CREATE TABLE scheduled_task_log(task_id INTEGER); CREATE TABLE scheduled_task_run(task_id INTEGER);
        INSERT INTO scheduled_task VALUES(1),(2); INSERT INTO scheduled_task_log VALUES(1),(2); INSERT INTO scheduled_task_run VALUES(1),(2);
        CREATE TRIGGER fail_run_delete BEFORE DELETE ON scheduled_task_run BEGIN SELECT RAISE(ABORT,'injected run failure'); END;").unwrap();
    assert!(cascade_delete::delete(&conn,"scheduled_task",1).is_err());
    assert_eq!(count(&conn,"scheduled_task_log","task_id=1"),1);
    assert_eq!(count(&conn,"scheduled_task","id=1"),1);
    conn.execute_batch("DROP TRIGGER fail_run_delete").unwrap();
    cascade_delete::delete(&conn,"scheduled_task",1).unwrap();
    for table in ["scheduled_task_log","scheduled_task_run"] { assert_eq!(count(&conn,table,"task_id=1"),0); assert_eq!(count(&conn,table,"task_id=2"),1); }
}

#[test]
fn test_cascade_delete_prompt_and_model_replacement_cleans_parameters() {
    let db=assistant_db();
    let id=db.add_assistant("replace","",Some(0),false).unwrap();
    let prompt=db.add_assistant_prompt(id,"prompt").unwrap();
    let model=db.add_assistant_model(id,77,"model","alias").unwrap();
    db.add_assistant_model_config(id,model,"temperature","0.7","number").unwrap();
    db.conn.execute("INSERT INTO assistant_prompt_param(assistant_id,assistant_prompt_id,param_name) VALUES(?1,?2,'param')",[id,prompt]).unwrap();
    db.conn.pragma_update(None,"foreign_keys",true).unwrap();
    db.delete_assistant_prompt_by_assistant_id(id).unwrap();
    db.delete_assistant_model_by_assistant_id(id).unwrap();
    assert_eq!(count(&db.conn,"assistant_prompt_param",&format!("assistant_id={id}")),0);
    assert_eq!(count(&db.conn,"assistant_model_config",&format!("assistant_id={id}")),0);
    assert_eq!(count(&db.conn,"assistant",&format!("id={id}")),1);
}


#[test]
fn test_cascade_delete_task_conversation_removes_butler_ownership_and_queued_messages() {
    let conn=create_test_db(); seed_messages(&conn);
    conn.execute_batch("UPDATE conversation SET conversation_kind='butler_task', parent_butler_conversation_id=20 WHERE id=10;
        INSERT INTO butler_task_definition(butler_conversation_id,task_conversation_id,title,goal,executor_assistant_id,executor_assistant_source,created_time)
            VALUES(20,10,'task','goal',1,'assistant_id','now');
        INSERT INTO butler_task_result(task_conversation_id,summary,final_message_id,created_time,updated_time) VALUES(10,'result',11,'now','now');
        INSERT INTO queued_conversation_message(conversation_id,queue_kind,request_json,prompt,assistant_id) VALUES(10,'user','{}','queued',1),(20,'user','{}','keep',1);").unwrap();
    crate::db::conversation_db::cascade_delete_conversation(&conn,10).unwrap();
    assert_eq!(count(&conn,"butler_task_definition","task_conversation_id=10"),0);
    assert_eq!(count(&conn,"butler_task_result","task_conversation_id=10"),0);
    assert_eq!(count(&conn,"queued_conversation_message","conversation_id=10"),0);
    assert_eq!(count(&conn,"queued_conversation_message","conversation_id=20"),1);
    assert_eq!(count(&conn,"conversation","id=20"),1);
}

#[test]
fn test_cascade_delete_remote_main_tombstone_detaches_surviving_child_history() {
    let conn=create_test_db(); seed_messages(&conn);
    conn.execute_batch("UPDATE conversation SET conversation_kind='butler_main' WHERE id=10;
        UPDATE conversation SET conversation_kind='butler_task', parent_butler_conversation_id=10 WHERE id=20;
        INSERT INTO butler_main_state(butler_conversation_id,slot,last_active_at,created_time,updated_time) VALUES(10,'default','now','now','now');
        INSERT INTO butler_task_definition(butler_conversation_id,task_conversation_id,title,goal,executor_assistant_id,executor_assistant_source,created_time)
            VALUES(10,20,'task','goal',1,'assistant_id','now');").unwrap();
    crate::db::conversation_db::cascade_delete_conversation(&conn,10).unwrap();
    assert_eq!(count(&conn,"conversation","id=20 AND parent_butler_conversation_id IS NULL"),1);
    assert_eq!(count(&conn,"message","conversation_id=20"),1);
    assert_eq!(count(&conn,"butler_task_definition","butler_conversation_id=10"),0);
    assert_eq!(count(&conn,"butler_main_state","butler_conversation_id=10"),0);
}

#[test]
fn test_cascade_delete_deferred_commit_failure_rolls_back_owned_records() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE conversation(id INTEGER PRIMARY KEY);
        CREATE TABLE message(id INTEGER PRIMARY KEY, conversation_id INTEGER REFERENCES conversation(id));
        CREATE TABLE message_attachment(id INTEGER PRIMARY KEY, message_id INTEGER REFERENCES message(id));
        CREATE TABLE retained_reference(conversation_id INTEGER REFERENCES conversation(id) DEFERRABLE INITIALLY DEFERRED);
        INSERT INTO conversation VALUES(1); INSERT INTO message VALUES(11,1); INSERT INTO message_attachment VALUES(111,11);
        INSERT INTO retained_reference VALUES(1);").unwrap();
    let error=cascade_delete::delete(&conn,"conversation",1).unwrap_err().to_string();
    assert!(error.contains("FOREIGN KEY constraint failed"),"{error}");
    assert!(conn.is_autocommit());
    assert_eq!(count(&conn,"conversation","id=1"),1);
    assert_eq!(count(&conn,"message_attachment","message_id=11"),1);
}

#[test]
fn test_cascade_delete_assistant_keeps_pending_request_and_rejects_silent_reassignment() {
    let db=assistant_db(); let id=db.add_assistant("pending","",Some(0),false).unwrap();
    peer(&db.conn,"aipp_delete_conversation",&create_test_db());
    db.conn.execute("INSERT INTO aipp_delete_conversation.conversation(id,name,assistant_id,created_time) VALUES(10,'pending',?1,'now')",[id]).unwrap();
    let payload=format!("{{\"assistant_id\":{id},\"prompt\":\"keep draft\"}}");
    db.conn.execute("INSERT INTO aipp_delete_conversation.queued_conversation_message(id,conversation_id,queue_kind,request_json,prompt,assistant_id)
        VALUES(42,10,'normal',?1,'keep draft',?2)",rusqlite::params![payload,id]).unwrap();
    let error=db.delete_assistant(id).unwrap_err().to_string();
    assert!(error.contains("42") && error.contains("10") && error.contains("排队"),"{error}");
    assert_eq!(count(&db.conn,"aipp_delete_conversation.conversation",&format!("id=10 AND assistant_id={id}")),1);
    assert_eq!(db.conn.query_row("SELECT request_json FROM aipp_delete_conversation.queued_conversation_message WHERE id=42",[],|r|r.get::<_,String>(0)).unwrap(),payload);
}

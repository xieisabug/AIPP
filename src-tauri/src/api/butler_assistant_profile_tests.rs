use super::*;
use rusqlite::Connection;

fn database() -> AssistantDatabase {
    let db = AssistantDatabase {conn: Connection::open_in_memory().unwrap(), mcp_conn: Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    db.add_assistant("快速助手", "", Some(0), false).unwrap();
    db
}
fn model(code: &str) -> ButlerModelSelection {
    ButlerModelSelection {local_agent:false,raw_value:format!("{code}%%2"),model_code:code.into(),provider_id:2,display_name:code.into()}
}

#[test]
fn test_butler_profile_unchanged_load_needs_no_write_and_keeps_ids() {
    let db = database();
    let assistant = sync(&db,&model("configured"),"prompt",None, None).unwrap();
    let prompt_id = db.get_assistant_prompt(assistant.id).unwrap()[0].id;
    let model_id = db.get_assistant_model(assistant.id).unwrap()[0].id;
    let before: i64 = db.conn.query_row("SELECT total_changes()",[],|r|r.get(0)).unwrap();
    // Any attempted DELETE/UPDATE/INSERT fails, including a zero-row write.
    db.conn.execute_batch("PRAGMA query_only=ON").unwrap();
    for _ in 0..10 { assert_eq!(sync(&db,&model("configured"),"prompt",None, None).unwrap().id,assistant.id); }
    let after: i64 = db.conn.query_row("SELECT total_changes()",[],|r|r.get(0)).unwrap();
    assert_eq!(before,after);
    db.conn.execute_batch("PRAGMA query_only=OFF").unwrap();
    sync(&db,&model("changed"),"new prompt",None, None).unwrap();
    assert_eq!(db.get_assistant_prompt(assistant.id).unwrap()[0].id,prompt_id);
    assert_eq!(db.get_assistant_model(assistant.id).unwrap()[0].id,model_id);
    assert_eq!(db.get_assistant_model(assistant.id).unwrap()[0].model_code,"changed");
}

#[test]
fn test_butler_profile_failure_rolls_back_prompt_and_model_together() {
    let db = database();
    let assistant = sync(&db,&model("configured"),"original prompt",None, None).unwrap();
    db.conn.execute_batch("CREATE TRIGGER reject_model BEFORE UPDATE ON assistant_model BEGIN SELECT RAISE(ABORT,'simulated write failure'); END").unwrap();
    let error = sync(&db,&model("changed"),"replacement prompt",None, None).unwrap_err();
    assert!(error.to_string().contains("simulated write failure"));
    assert_eq!(db.get_assistant_prompt(assistant.id).unwrap()[0].prompt,"original prompt");
    assert_eq!(db.get_assistant_model(assistant.id).unwrap()[0].model_code,"configured");
    assert_eq!(db.get_assistant_model_configs(assistant.id).unwrap().len(),config(None,None).len());
    assert!(db.conn.is_autocommit());
}

#[test]
fn test_butler_profile_repairs_partial_old_initialization_without_duplicates() {
    let db = database();
    let id = db.add_assistant(BUTLER_SYSTEM_ASSISTANT_NAME,"old",Some(0),true).unwrap();
    let first = db.add_assistant_prompt(id,"old").unwrap();
    db.add_assistant_prompt(id,"duplicate").unwrap();
    db.add_assistant_prompt_param(id,first,"old","string","x").unwrap();
    assert_eq!(sync(&db,&model("configured"),"repaired",None, None).unwrap().id,id);
    assert_eq!(db.get_assistant_prompt(id).unwrap().len(),1);
    assert_eq!(db.get_assistant_prompt(id).unwrap()[0].id,first);
    assert!(db.get_assistant_prompt_params(id).unwrap().is_empty());
    assert_eq!(db.get_assistant_model(id).unwrap().len(),1);
    assert_eq!(db.get_assistant_model_configs(id).unwrap().len(),config(None,None).len());
}

#[test]
fn test_butler_profile_omits_sampling_defaults_on_creation_and_upgrade() {
    let db = database();
    let selection = model("configured");
    let assistant = sync(&db, &selection, "prompt", None, None).unwrap();
    let model_id = db.get_assistant_model(assistant.id).unwrap()[0].id;
    let sampling_is_absent = || db.get_assistant_model_configs(assistant.id).unwrap()
        .iter().all(|c| c.name != "temperature" && c.name != "top_p");
    assert!(sampling_is_absent());

    db.add_assistant_model_config(assistant.id, model_id, "temperature", "0.8", "float").unwrap();
    db.add_assistant_model_config(assistant.id, model_id, "top_p", "1.0", "float").unwrap();
    assert_eq!(sync(&db, &selection, "prompt", None, None).unwrap().id, assistant.id);
    assert!(sampling_is_absent());
}

#[test]
fn test_butler_profile_uses_model_auto_review_for_tools() {
    let db = database();
    let assistant = sync(&db,&model("configured"),"prompt",None, None).unwrap();
    let configs = db.get_assistant_model_configs(assistant.id).unwrap();
    let mode = configs.iter().find(|c| c.name == "tool_review_mode").expect("tool_review_mode");
    assert_eq!(mode.value.as_deref(), Some("auto_model"));
}

#[test]
fn test_onboarding_aux_butler_profile_switches_driver_and_binds_default_model_with_workspace() {
    let db = database();
    let http = model("api");
    let assistant = sync(&db, &http, "prompt", None, None).unwrap();
    let mut agent = model("");
    agent.local_agent = true;
    let updated = sync(&db, &agent, "prompt", Some("workspace"), None).unwrap();
    assert_eq!(updated.id, assistant.id);
    assert_eq!(updated.assistant_type, Some(4));
    assert!(db.get_assistant_model(updated.id).unwrap()[0].model_code.is_empty());
    let configs = db.get_assistant_model_configs(updated.id).unwrap();
    assert!(configs.iter().any(|c| c.name == "acp_working_directory" && c.value.as_deref() == Some("workspace")));
    assert_eq!(sync(&db, &http, "prompt", None, None).unwrap().assistant_type, Some(0));
    assert!(db.get_assistant_model_configs(updated.id).unwrap().iter().all(|c| c.name != "acp_working_directory"));
}

#[test]
fn test_onboarding_aux_channel_switch_depends_on_native_instance() {
    let db = super::super::LLMDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let api = db.add_llm_provider("API", "anthropic", "", false, true).unwrap();
    let other_api = db.add_llm_provider("B", "openai_api", "", false, true).unwrap();
    let a = db.add_llm_provider("Claude", "claude_sdk", "", false, true).unwrap();
    let b = db.add_llm_provider("Claude B", "claude_sdk", "", false, true).unwrap();
    for (old, new, expected) in [(api, other_api, false), (api, a, true), (a, api, true), (a, b, true), (a, a, false)] {
        assert_eq!(super::super::butler_channel_changed(&db, &format!("first%%{old}"), &format!("second%%{new}")).unwrap(), expected);
    }
    assert!(!super::super::butler_channel_changed(&db, "", &format!("acp%%{a}")).unwrap());
}

#[test]
fn test_onboarding_chief_profile_uses_model_output_limit_and_dynamic_tools_after_reload() {
    let db = database();
    let selection = model("small");
    for limit in ["32000", "4096", "4096"] {
        let assistant = sync(&db, &selection, "prompt", None, Some(limit)).unwrap();
        let rows = db.get_assistant_model_configs(assistant.id).unwrap();
        for (name, value) in [("max_tokens", limit), ("stream", "true"),
            ("mcp_loading_mode_explicit", "true"), ("dynamic_mcp_loading_enabled", "true")] {
            assert!(rows.iter().any(|row| row.name == name && row.value.as_deref() == Some(value)));
        }
    }
}

use super::*;
use rusqlite::Connection;

fn database() -> AssistantDatabase {
    let db = AssistantDatabase {conn: Connection::open_in_memory().unwrap(), mcp_conn: Connection::open_in_memory().unwrap()};
    db.create_tables().unwrap();
    db.add_assistant("快速助手", "", Some(0), false).unwrap();
    db
}
fn model(code: &str) -> ButlerModelSelection {
    ButlerModelSelection {raw_value:format!("{code}%%2"),model_code:code.into(),provider_id:2,display_name:code.into()}
}

#[test]
fn test_butler_profile_unchanged_load_needs_no_write_and_keeps_ids() {
    let db = database();
    let assistant = sync(&db,&model("configured"),"prompt").unwrap();
    let prompt_id = db.get_assistant_prompt(assistant.id).unwrap()[0].id;
    let model_id = db.get_assistant_model(assistant.id).unwrap()[0].id;
    let before: i64 = db.conn.query_row("SELECT total_changes()",[],|r|r.get(0)).unwrap();
    // Any attempted DELETE/UPDATE/INSERT fails, including a zero-row write.
    db.conn.execute_batch("PRAGMA query_only=ON").unwrap();
    for _ in 0..10 { assert_eq!(sync(&db,&model("configured"),"prompt").unwrap().id,assistant.id); }
    let after: i64 = db.conn.query_row("SELECT total_changes()",[],|r|r.get(0)).unwrap();
    assert_eq!(before,after);
    db.conn.execute_batch("PRAGMA query_only=OFF").unwrap();
    sync(&db,&model("changed"),"new prompt").unwrap();
    assert_eq!(db.get_assistant_prompt(assistant.id).unwrap()[0].id,prompt_id);
    assert_eq!(db.get_assistant_model(assistant.id).unwrap()[0].id,model_id);
    assert_eq!(db.get_assistant_model(assistant.id).unwrap()[0].model_code,"changed");
}

#[test]
fn test_butler_profile_failure_rolls_back_prompt_and_model_together() {
    let db = database();
    let assistant = sync(&db,&model("configured"),"original prompt").unwrap();
    db.conn.execute_batch("CREATE TRIGGER reject_model BEFORE UPDATE ON assistant_model BEGIN SELECT RAISE(ABORT,'simulated write failure'); END").unwrap();
    let error = sync(&db,&model("changed"),"replacement prompt").unwrap_err();
    assert!(error.to_string().contains("simulated write failure"));
    assert_eq!(db.get_assistant_prompt(assistant.id).unwrap()[0].prompt,"original prompt");
    assert_eq!(db.get_assistant_model(assistant.id).unwrap()[0].model_code,"configured");
    assert_eq!(db.get_assistant_model_configs(assistant.id).unwrap().len(),CONFIG.len());
    assert!(db.conn.is_autocommit());
}

#[test]
fn test_butler_profile_repairs_partial_old_initialization_without_duplicates() {
    let db = database();
    let id = db.add_assistant(BUTLER_SYSTEM_ASSISTANT_NAME,"old",Some(0),true).unwrap();
    let first = db.add_assistant_prompt(id,"old").unwrap();
    db.add_assistant_prompt(id,"duplicate").unwrap();
    db.add_assistant_prompt_param(id,first,"old","string","x").unwrap();
    assert_eq!(sync(&db,&model("configured"),"repaired").unwrap().id,id);
    assert_eq!(db.get_assistant_prompt(id).unwrap().len(),1);
    assert_eq!(db.get_assistant_prompt(id).unwrap()[0].id,first);
    assert!(db.get_assistant_prompt_params(id).unwrap().is_empty());
    assert_eq!(db.get_assistant_model(id).unwrap().len(),1);
    assert_eq!(db.get_assistant_model_configs(id).unwrap().len(),CONFIG.len());
}

#[test]
fn test_butler_profile_uses_model_auto_review_for_tools() {
    let db = database();
    let assistant = sync(&db,&model("configured"),"prompt").unwrap();
    let configs = db.get_assistant_model_configs(assistant.id).unwrap();
    let mode = configs.iter().find(|c| c.name == "tool_review_mode").expect("tool_review_mode");
    assert_eq!(mode.value.as_deref(), Some("auto_model"));
}

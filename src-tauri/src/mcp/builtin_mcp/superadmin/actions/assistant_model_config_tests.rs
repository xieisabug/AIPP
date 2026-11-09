use super::*;

#[test]
fn test_model_config_repair_is_discoverable_with_preview_and_scope() {
    let registry = crate::mcp::builtin_mcp::superadmin::registry::build_registry();
    let action = registry.get_meta("assistant.patch_model_config").unwrap();
    assert!(action.supports_dry_run);
    assert_eq!(action.domain,"assistant");
    assert!(action.args_schema["required"].as_array().unwrap().contains(&json!("expected_configs")));
}

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE assistant_model(id INTEGER PRIMARY KEY,assistant_id INTEGER);
        CREATE TABLE assistant_model_config(assistant_id INTEGER,assistant_model_id INTEGER,name TEXT,value TEXT,value_type TEXT);
        INSERT INTO assistant_model VALUES(7,2);
        INSERT INTO assistant_model_config VALUES(2,7,'temperature','0.7','float'),(2,7,'stream','true','boolean');").unwrap();
    conn
}
fn args(conn: &Connection) -> Value {
    json!({"assistant_id":2,"assistant_model_id":7,"expected_configs":snapshot(conn,2).unwrap(),
        "set":{"max_tokens":1024},"remove":["temperature"]})
}
#[test]
fn test_model_config_repair_preview_and_commit_preserve_other_parameters() {
    let mut conn = fixture();
    let args = args(&conn);
    let before = snapshot(&conn,2).unwrap();
    let preview = patch(&mut conn,&args,true).unwrap();
    assert_eq!(snapshot(&conn,2).unwrap(),before);
    let result = patch(&mut conn,&args,false).unwrap();
    assert_eq!(preview["model_configs"],result["model_configs"]);
    let after = snapshot(&conn,2).unwrap();
    assert!(after.as_array().unwrap().iter().any(|r| r["name"]=="stream" && r["value"]=="true"));
    assert!(!after.as_array().unwrap().iter().any(|r| r["name"]=="temperature"));
    assert!(patch(&mut conn,&args,false).unwrap_err().contains("已变化"));
}
#[test]
fn test_model_config_repair_rejects_invalid_values_and_wrong_binding_atomically() {
    let mut conn = fixture();
    let original = snapshot(&conn,2).unwrap();
    for invalid in [json!({"temperature":null}),json!({"top_p":2}),json!({"max_tokens":-1}),json!({"provider_id":3}),json!({"reasoning_effort":"nonsense"})] {
        let mut args = args(&conn);
        args["set"] = invalid;
        assert!(patch(&mut conn,&args,false).is_err());
        assert_eq!(snapshot(&conn,2).unwrap(),original);
    }
    let mut args = args(&conn);
    args["assistant_model_id"] = json!(8);
    assert!(patch(&mut conn,&args,false).unwrap_err().contains("不属于"));
    assert_eq!(snapshot(&conn,2).unwrap(),original);
}

#[test]
fn test_model_config_repair_undo_preserves_unrelated_edits_and_rejects_conflicts() {
    let mut conn = fixture();
    let args = args(&conn);
    let before = json!({"configs":snapshot(&conn,2).unwrap()});
    patch(&mut conn,&args,false).unwrap();
    conn.execute("UPDATE assistant_model_config SET value='2048' WHERE name='max_tokens'",[]).unwrap();
    assert!(undo_patch(&mut conn,&before,&args).is_err());
    assert!(!snapshot(&conn,2).unwrap().as_array().unwrap().iter().any(|r|r["name"]=="temperature"));
    conn.execute("UPDATE assistant_model_config SET value='1024' WHERE name='max_tokens'",[]).unwrap();
    conn.execute("UPDATE assistant_model_config SET value='false' WHERE name='stream'",[]).unwrap();
    undo_patch(&mut conn,&before,&args).unwrap();
    let after = snapshot(&conn,2).unwrap();
    assert!(after.as_array().unwrap().iter().any(|r|r["name"]=="temperature" && r["value"]=="0.7"));
    assert!(after.as_array().unwrap().iter().any(|r|r["name"]=="stream" && r["value"]=="false"));
    assert!(!after.as_array().unwrap().iter().any(|r|r["name"]=="max_tokens"));
}

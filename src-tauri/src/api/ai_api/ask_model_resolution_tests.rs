use super::{
    describe_model_lookup_failure, is_unconfigured_assistant_model, parse_override_model_id,
    resolve_ask_model_detail, AssistantDetail, LLMDatabase,
};
use crate::db::assistant_db::{Assistant, AssistantModel};

#[test]
fn test_provider_switch_controls_bound_and_override_models() {
    let db = LLMDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let bound = db.add_llm_provider("主提供商", "openai_api", "", false, true).unwrap();
    let other = db.add_llm_provider("备用提供商", "openai_api", "", false, true).unwrap();
    for id in [bound, other] {
        db.add_llm_model("Model", id, "shared", "", false, false, false).unwrap();
    }
    let assistant = AssistantDetail {
        assistant: Assistant {
            id: 1, name: "助手".into(), description: None, assistant_type: Some(0),
            is_addition: false, created_time: String::new(),
        },
        model: vec![AssistantModel {
            id: 1, assistant_id: 1, provider_id: bound, model_code: "shared".into(), alias: String::new(),
        }],
        prompts: vec![], model_configs: vec![], prompt_params: vec![], mcp_configs: vec![], mcp_tool_configs: vec![],
    };
    let resolve = |selection: Option<&str>| resolve_ask_model_detail(&db, selection, &assistant);
    let original = resolve(None).unwrap().model.id;
    db.update_llm_provider(bound, "主提供商", "openai_api", "", false).unwrap();
    assert!(resolve(None).unwrap_err().contains("主提供商"));
    assert!(resolve(Some(&format!("shared%%{bound}"))).unwrap_err().contains("已停用"));
    assert_eq!(resolve(Some(&format!("shared%%{other}"))).unwrap().provider.id, other);
    assert_eq!(db.get_llm_model_detail(&bound, "shared").unwrap().model.id, original);
    db.update_llm_provider(bound, "主提供商", "openai_api", "", true).unwrap();
    assert_eq!(resolve(None).unwrap().model.id, original);
}

#[test]
fn placeholder_assistant_model_counts_as_unconfigured() {
    assert!(is_unconfigured_assistant_model(0, ""));
    assert!(is_unconfigured_assistant_model(3, "  "));
    assert!(is_unconfigured_assistant_model(0, "gpt-4o"));
    assert!(!is_unconfigured_assistant_model(3, "gpt-4o"));
}

#[test]
fn override_model_id_is_parsed_without_fallback() {
    assert_eq!(parse_override_model_id("gpt-4o%%7"), Ok(("gpt-4o".to_string(), 7)));
    assert!(parse_override_model_id("gpt-4o").is_err());
    assert!(parse_override_model_id("gpt-4o%%x").is_err());
    assert!(parse_override_model_id("%%7").is_err());
}

#[test]
fn missing_model_message_names_the_model() {
    let text = describe_model_lookup_failure(
        7,
        "gpt-4o",
        false,
        &rusqlite::Error::QueryReturnedNoRows.into(),
    );
    assert!(text.contains("gpt-4o"));
    assert!(text.contains("不存在或已被删除"));
}

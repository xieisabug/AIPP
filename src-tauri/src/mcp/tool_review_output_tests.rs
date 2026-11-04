use super::*;
use crate::db::llm_db::{LLMModel, LLMProvider, LLMProviderConfig};
use rusqlite::Connection;

fn failure(status: u16, message: &str, param: &str) -> genai::Error {
    genai::Error::WebModelCall {
        model_iden: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "review"),
        webc_error: genai::webc::Error::ResponseFailedStatus {
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            body: serde_json::json!({"error":{"message":message,"param":param}}).to_string(),
            headers: Default::default(),
        },
    }
}

#[test]
fn test_review_output_only_explicit_capability_errors_qualify() {
    assert!(unsupported_format_error(&failure(400, "This response_format type is unavailable now", "")));
    assert!(unsupported_format_error(&failure(422, "Unsupported parameter", "response_format")));
    assert!(unsupported_format_error(&failure(400, "json_schema is not supported", "")));
    for status in [401, 403, 429, 500, 503] {
        assert!(!unsupported_format_error(&failure(status, "response_format unsupported request_id=400", "")));
    }
    for message in ["Invalid JSON schema: required missing", "unsupported model", "response_format schema is invalid", "invalid api key", "response_format json_schema has unsupported keyword"] {
        assert!(!unsupported_format_error(&failure(400, message, "")), "{message}");
    }
}

#[test]
fn test_review_output_memory_survives_reload_and_reset_blocks_stale_request() {
    let db = SystemDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    assert!(!load_capability(&db, "review").unwrap().prompt);
    remember_prompt(&db, "review", 0, "response_format unavailable").unwrap();
    assert!(load_capability(&db, "review").unwrap().prompt);
    write_capability(&db, "review", &OutputCapability { generation: 1, ..Default::default() }).unwrap();
    remember_prompt(&db, "review", 0, "stale").unwrap();
    let value = load_capability(&db, "review").unwrap();
    assert_eq!(value.generation, 1);
    assert!(!value.prompt);
    remember_prompt(&db, "review", 1, "fresh").unwrap();
    assert_eq!(load_capability(&db, "review").unwrap().reason, "fresh");
    assert!(!load_capability(&db, "other").unwrap().prompt);
}

#[test]
fn test_review_output_identity_isolates_provider_model_protocol_and_endpoint() {
    let mut detail = ModelDetail {
        provider: LLMProvider { id: 41, name: "provider".into(), api_type: "openai".into(), description: "".into(), is_official: false, is_enabled: true },
        model: LLMModel { id: 1, name: "model".into(), code: "review".into(), llm_provider_id: 41, description: "".into(), vision_support: false, audio_support: false, video_support: false, request_mode: "chat_completions".into() },
        configs: vec![LLMProviderConfig { id: 1, name: "endpoint".into(), llm_provider_id: 41, value: "https://example.com/v1".into(), append_location: "".into(), is_addition: false }],
    };
    let original = capability_key(&detail);
    detail.provider.id += 1;
    assert_ne!(original, capability_key(&detail));
    detail.provider.id -= 1;
    detail.model.code.push('2');
    assert_ne!(original, capability_key(&detail));
    detail.model.code.pop();
    detail.model.request_mode = "responses".into();
    assert_ne!(original, capability_key(&detail));
    detail.model.request_mode = "chat_completions".into();
    detail.configs[0].value.push('/');
    assert_eq!(original, capability_key(&detail));
    detail.configs[0].value.push('2');
    assert_ne!(original, capability_key(&detail));
    assert!(!original.contains("example.com"));
}

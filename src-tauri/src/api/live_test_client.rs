use crate::api::genai_client::create_client_with_config;
use crate::db::llm_db::LLMProviderConfig;
use crate::db::system_db::FeatureConfig;
use std::collections::HashMap;

pub(crate) fn configured_client() -> (genai::Client, String) {
    let config = |name: &str, value| LLMProviderConfig {
        id: 0,
        name: name.into(),
        llm_provider_id: 1,
        value,
        append_location: String::new(),
        is_addition: false,
    };
    let model = std::env::var("AIPP_CACHE_PROBE_MODEL").unwrap();
    let endpoint = std::env::var("AIPP_CACHE_PROBE_ENDPOINT").unwrap();
    let key = std::env::var("AIPP_CACHE_PROBE_KEY").unwrap();
    let headers = FeatureConfig {
        id: None,
        feature_code: "network_config".into(),
        key: "custom_headers".into(),
        value: r#"{"User-Agent":"AIPP-Butler-Cache-Verification/1.0"}"#.into(),
        data_type: "string".into(),
        description: None,
    };
    let features = HashMap::from([(
        headers.feature_code.clone(),
        HashMap::from([(headers.key.clone(), headers)]),
    )]);
    let client = create_client_with_config(
        &[config("endpoint", endpoint), config("api_key", key)],
        &model,
        "openai_api",
        None,
        None,
        false,
        Some(150),
        false,
        &features,
    )
    .unwrap();
    (client, model)
}

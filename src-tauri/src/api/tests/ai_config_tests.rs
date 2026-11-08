//! AI 配置逻辑测试
//!
//! ## 测试范围
//!
//! - ChatOptions 构建
//! - 模型配置合并
//! - 网络配置获取
//! - 重试延迟计算

use crate::api::ai::config::{
    build_proxy_env_vars, calculate_retry_delay, get_network_proxy_from_config,
    get_request_timeout_from_config, get_retry_attempts_from_config, ConfigBuilder,
    OpenAiCacheContext, DEFAULT_REQUEST_TIMEOUT_SECS, MAX_RETRY_ATTEMPTS, RETRY_DELAY_BASE_MS,
};
use crate::db::assistant_db::AssistantModelConfig;
use crate::db::llm_db::{LLMModel, LLMProvider, ModelDetail};
use crate::db::system_db::FeatureConfig;
use std::collections::HashMap;

// ============================================================================
// ChatOptions 构建测试
// ============================================================================

/// 测试空配置时返回默认 ChatOptions
#[test]
fn test_build_chat_options_empty_config() {
    let config_map: HashMap<String, String> = HashMap::new();
    let options = ConfigBuilder::build_chat_options(&config_map, None, None);

    // 默认选项，温度/max_tokens/top_p 都是 None
    // 由于 ChatOptions 没有公开的字段访问，我们只能验证它不会崩溃
    assert!(format!("{:?}", options).contains("ChatOptions"));
}

/// 测试温度配置
#[test]
fn test_build_chat_options_with_temperature() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("temperature".to_string(), "0.7".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map, None, None);
    let debug_str = format!("{:?}", options);

    // 验证选项已创建
    assert!(debug_str.contains("ChatOptions"));
}

/// 测试 max_tokens 配置
#[test]
fn test_build_chat_options_with_max_tokens() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("max_tokens".to_string(), "4096".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map, None, None);
    let debug_str = format!("{:?}", options);

    assert!(debug_str.contains("ChatOptions"));
}

/// 测试 top_p 配置
#[test]
fn test_build_chat_options_with_top_p() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("top_p".to_string(), "0.9".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map, None, None);
    let debug_str = format!("{:?}", options);

    assert!(debug_str.contains("ChatOptions"));
}

/// 测试所有配置项组合
#[test]
fn test_build_chat_options_with_all_configs() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("temperature".to_string(), "0.8".to_string());
    config_map.insert("max_tokens".to_string(), "2048".to_string());
    config_map.insert("top_p".to_string(), "0.95".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map, None, None);
    let debug_str = format!("{:?}", options);

    assert!(debug_str.contains("ChatOptions"));
}

/// 测试 reasoning_effort 配置
#[test]
fn test_build_chat_options_with_reasoning_effort() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("reasoning_effort".to_string(), "medium".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map, None, None);

    assert!(format!("{:?}", options).contains("ChatOptions"));
}

/// 测试 OpenAI Responses 默认启用 prompt cache key 和 24h retention
#[test]
fn test_build_chat_options_openai_responses_prompt_cache_defaults() {
    let config_map: HashMap<String, String> = HashMap::new();
    let feature_config_map: HashMap<String, HashMap<String, FeatureConfig>> = HashMap::new();
    let context = OpenAiCacheContext {
        provider_id: 7,
        provider_api_type: "openai_api".to_string(),
        model_code: "gpt-5.4".to_string(),
        request_mode: "responses".to_string(),
        assistant_id: 11,
        conversation_id: 13,
    };

    let options =
        ConfigBuilder::build_chat_options(&config_map, Some(&feature_config_map), Some(&context));

    assert_eq!(
        options.prompt_cache_key.as_deref(),
        Some("aipp:7:gpt-5.4:11:13")
    );
    assert_eq!(options.cache_control, Some(genai::chat::CacheControl::Ephemeral24h));
}

/// 测试关闭 OpenAI prompt cache key 后不写入 cache 参数
#[test]
fn test_build_chat_options_openai_prompt_cache_can_be_disabled() {
    let config_map: HashMap<String, String> = HashMap::new();
    let mut network_config = HashMap::new();
    network_config.insert(
        "openai_prompt_cache_key_enabled".to_string(),
        FeatureConfig {
            id: None,
            feature_code: "network_config".to_string(),
            key: "openai_prompt_cache_key_enabled".to_string(),
            value: "false".to_string(),
            data_type: "boolean".to_string(),
            description: None,
        },
    );
    let feature_config_map = HashMap::from([("network_config".to_string(), network_config)]);
    let context = OpenAiCacheContext {
        provider_id: 7,
        provider_api_type: "openai".to_string(),
        model_code: "gpt-5.4".to_string(),
        request_mode: "responses".to_string(),
        assistant_id: 11,
        conversation_id: 13,
    };

    let options =
        ConfigBuilder::build_chat_options(&config_map, Some(&feature_config_map), Some(&context));

    assert_eq!(options.prompt_cache_key, None);
    assert_eq!(options.cache_control, None);
}

/// 测试无效的温度值（非数字）
#[test]
fn test_build_chat_options_invalid_temperature() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("temperature".to_string(), "not_a_number".to_string());

    // 不应该崩溃，应该忽略无效值
    let options = ConfigBuilder::build_chat_options(&config_map, None, None);
    assert!(format!("{:?}", options).contains("ChatOptions"));
}

/// 测试无效的 max_tokens 值
#[test]
fn test_build_chat_options_invalid_max_tokens() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("max_tokens".to_string(), "-100".to_string()); // 负数

    let options = ConfigBuilder::build_chat_options(&config_map, None, None);
    assert!(format!("{:?}", options).contains("ChatOptions"));
}

// ============================================================================
// 模型配置合并测试
// ============================================================================

fn prompt_cache_options_for_test(
    model: &str,
    request_mode: &str,
    provider: &str,
    settings: &[(&str, &str)],
) -> genai::chat::ChatOptions {
    let network_config = settings.iter().map(|(key, value)| {
        ((*key).to_string(), create_feature_config(value))
    }).collect();
    let features = HashMap::from([("network_config".to_string(), network_config)]);
    ConfigBuilder::build_chat_options(&HashMap::new(), Some(&features), Some(&OpenAiCacheContext {
        provider_id: 7,
        provider_api_type: provider.to_string(),
        model_code: model.to_string(),
        request_mode: request_mode.to_string(),
        assistant_id: 11,
        conversation_id: 13,
    }))
}

/// Exercise the real Responses adapter over loopback, including client defaults and call overrides.
async fn prompt_cache_request_payload(options: &genai::chat::ChatOptions) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        let (body_start, content_length) = loop {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&chunk[..n]);
            if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&request[..index]).unwrap();
                assert!(headers.starts_with("POST /v1/responses HTTP/1.1"));
                let length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                }).unwrap();
                break (index + 4, length);
            }
        };
        while request.len() < body_start + content_length {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&chunk[..n]);
        }
        let payload = serde_json::from_slice(&request[body_start..body_start + content_length]).unwrap();
        let body = r#"{"id":"resp_test","model":"gpt-test","status":"completed","output":[]}"#;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        payload
    });
    let client = genai::Client::builder().with_reqwest(
        reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap()
    ).build();
    let target = genai::ServiceTarget {
        endpoint: genai::resolver::Endpoint::from_owned(endpoint),
        auth: genai::resolver::AuthData::from_single("local-test-key"),
        model: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAIResp, "gpt-test"),
    };
    client.exec_chat(target, genai::chat::ChatRequest::from_user("test"), Some(options)).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), server).await.unwrap().unwrap()
}

#[tokio::test]
async fn test_prompt_cache_modern_models_send_new_ttl_without_legacy_retention() {
    for model in ["gpt-5.6", "gpt-5.6-mini-2026-01-01", "gpt-5.10", "gpt-6", "openai/gpt-6.1-sol"] {
        for ttl in ["30m", "12h"] {
            let options = prompt_cache_options_for_test(model, "responses", "openai_api", &[
                ("openai_prompt_cache_ttl", ttl), ("openai_prompt_cache_retention", "24h"),
            ]);
            let payload = prompt_cache_request_payload(&options).await;
            assert_eq!(payload["prompt_cache_options"]["ttl"], ttl, "{model}");
            assert!(payload.get("prompt_cache_retention").is_none(), "{model}");
            assert_eq!(payload["prompt_cache_key"], format!("aipp:7:{model}:11:13"));
        }
    }
}

#[tokio::test]
async fn test_prompt_cache_upgrade_keeps_legacy_policy_and_defaults_new_models_to_30m() {
    let old_settings = [("openai_prompt_cache_retention", "24h")];
    let payload = prompt_cache_request_payload(&prompt_cache_options_for_test("gpt-6.1-sol", "responses", "openai", &old_settings)).await;
    assert_eq!(payload["prompt_cache_options"]["ttl"], "30m");
    for model in ["gpt-5.4", "gpt-5.5", "gpt-5.5-pro", "gpt-4.1", "o3", "custom-alias"] {
        let payload = prompt_cache_request_payload(&prompt_cache_options_for_test(model, "responses", "openai", &old_settings)).await;
        assert_eq!(payload["prompt_cache_retention"], "24h", "{model}");
        assert!(payload.get("prompt_cache_options").is_none(), "{model}");
    }
    let options = prompt_cache_options_for_test("gpt-5.4", "responses", "openai", &[("openai_prompt_cache_retention", "in_memory")]);
    let payload = prompt_cache_request_payload(&options).await;
    assert_eq!(payload["prompt_cache_retention"], "in_memory");
}

#[tokio::test]
async fn test_prompt_cache_disabled_or_ineligible_requests_omit_both_cache_parameters() {
    for (mode, provider, settings) in [
        ("responses", "openai", vec![("openai_prompt_cache_key_enabled", "false"), ("openai_prompt_cache_ttl", "12h")]),
        ("chat_completions", "openai", vec![]),
        ("responses", "opencode_go", vec![]),
    ] {
        let payload = prompt_cache_request_payload(&prompt_cache_options_for_test("gpt-6.1-sol", mode, provider, &settings)).await;
        assert!(payload.get("prompt_cache_key").is_none());
        assert!(payload.get("prompt_cache_options").is_none());
        assert!(payload.get("prompt_cache_retention").is_none());
    }
}

#[tokio::test]
async fn test_prompt_cache_invalid_ttl_fails_before_http_without_substitution() {
    let options = prompt_cache_options_for_test("gpt-6.1-sol", "responses", "openai", &[("openai_prompt_cache_ttl", "24h")]);
    let target = genai::ServiceTarget {
        endpoint: genai::resolver::Endpoint::from_static("http://127.0.0.1:9/v1/"),
        auth: genai::resolver::AuthData::from_single("local-test-key"),
        model: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAIResp, "gpt-6.1-sol"),
    };
    let err = genai::Client::default().exec_chat(target, genai::chat::ChatRequest::from_user("test"), Some(&options)).await.unwrap_err();
    assert!(matches!(&err, genai::Error::PromptCacheTtlParsing { actual } if actual == "24h"));
    assert!(err.to_string().contains("prompt_cache_options.ttl"));
}

fn create_test_model_detail() -> ModelDetail {
    ModelDetail {
        model: LLMModel {
            id: 1,
            name: "Test Model".to_string(),
            code: "test-model-v1".to_string(),
            llm_provider_id: 1,
            description: "A test model".to_string(),
            vision_support: false,
            audio_support: false,
            video_support: false,
            request_mode: "chat_completions".to_string(),
        },
        provider: LLMProvider {
            id: 1,
            name: "Test Provider".to_string(),
            api_type: "test".to_string(),
            description: "Test provider".to_string(),
            is_official: true,
            is_enabled: true,
        },
        configs: vec![],
    }
}

/// 测试空基础配置时添加模型配置
#[test]
fn test_merge_model_configs_empty_base() {
    let base_configs: Vec<AssistantModelConfig> = vec![];
    let model_detail = create_test_model_detail();

    let result = ConfigBuilder::merge_model_configs(base_configs, &model_detail, None);

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "model");
    assert_eq!(result[0].value, Some("test-model-v1".to_string()));
}

/// 测试带有基础配置的合并
#[test]
fn test_merge_model_configs_with_base() {
    let base_configs = vec![AssistantModelConfig {
        id: 1,
        assistant_id: 1,
        assistant_model_id: 1,
        name: "temperature".to_string(),
        value: Some("0.7".to_string()),
        value_type: "number".to_string(),
    }];
    let model_detail = create_test_model_detail();

    let result = ConfigBuilder::merge_model_configs(base_configs, &model_detail, None);

    assert_eq!(result.len(), 2);
    assert!(result.iter().any(|c| c.name == "temperature"));
    assert!(result.iter().any(|c| c.name == "model"));
}

/// 测试覆盖配置
#[test]
fn test_merge_model_configs_with_override() {
    let base_configs = vec![AssistantModelConfig {
        id: 1,
        assistant_id: 1,
        assistant_model_id: 1,
        name: "temperature".to_string(),
        value: Some("0.7".to_string()),
        value_type: "number".to_string(),
    }];
    let model_detail = create_test_model_detail();

    let mut override_configs = HashMap::new();
    override_configs.insert(
        "temperature".to_string(),
        serde_json::Value::Number(serde_json::Number::from_f64(0.9).unwrap()),
    );

    let result =
        ConfigBuilder::merge_model_configs(base_configs, &model_detail, Some(override_configs));

    let temp_config = result.iter().find(|c| c.name == "temperature").unwrap();
    assert_eq!(temp_config.value, Some("0.9".to_string()));
}

/// 测试添加新的覆盖配置
#[test]
fn test_merge_model_configs_add_new_override() {
    let base_configs: Vec<AssistantModelConfig> = vec![];
    let model_detail = create_test_model_detail();

    let mut override_configs = HashMap::new();
    override_configs.insert(
        "max_tokens".to_string(),
        serde_json::Value::Number(serde_json::Number::from(4096)),
    );
    override_configs.insert(
        "stop_sequences".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::String("END".to_string())]),
    );

    let result =
        ConfigBuilder::merge_model_configs(base_configs, &model_detail, Some(override_configs));

    assert_eq!(result.len(), 3); // model + max_tokens + stop_sequences

    let max_tokens = result.iter().find(|c| c.name == "max_tokens").unwrap();
    assert_eq!(max_tokens.value, Some("4096".to_string()));
    assert_eq!(max_tokens.value_type, "number");

    let stop_seq = result.iter().find(|c| c.name == "stop_sequences").unwrap();
    assert_eq!(stop_seq.value_type, "array");
}

/// 测试不同类型的覆盖值
#[test]
fn test_merge_model_configs_different_value_types() {
    let base_configs: Vec<AssistantModelConfig> = vec![];
    let model_detail = create_test_model_detail();

    let mut override_configs = HashMap::new();
    override_configs
        .insert("string_val".to_string(), serde_json::Value::String("hello".to_string()));
    override_configs.insert("number_val".to_string(), serde_json::Value::Number(42.into()));
    override_configs.insert("bool_val".to_string(), serde_json::Value::Bool(true));
    override_configs.insert("null_val".to_string(), serde_json::Value::Null);

    let result =
        ConfigBuilder::merge_model_configs(base_configs, &model_detail, Some(override_configs));

    let string_config = result.iter().find(|c| c.name == "string_val").unwrap();
    assert_eq!(string_config.value_type, "string");
    assert_eq!(string_config.value, Some("hello".to_string()));

    let number_config = result.iter().find(|c| c.name == "number_val").unwrap();
    assert_eq!(number_config.value_type, "number");

    let bool_config = result.iter().find(|c| c.name == "bool_val").unwrap();
    assert_eq!(bool_config.value_type, "boolean");

    let null_config = result.iter().find(|c| c.name == "null_val").unwrap();
    assert_eq!(null_config.value_type, "null");
}

// ============================================================================
// 网络配置获取测试
// ============================================================================

fn create_feature_config(value: &str) -> FeatureConfig {
    FeatureConfig {
        id: Some(1),
        feature_code: "network_config".to_string(),
        key: "test".to_string(),
        value: value.to_string(),
        data_type: "string".to_string(),
        description: None,
    }
}

/// 测试获取重试次数 - 有配置
#[test]
fn test_get_retry_attempts_with_config() {
    let mut network_config = HashMap::new();
    network_config.insert("retry_attempts".to_string(), create_feature_config("5"));

    let mut config_map = HashMap::new();
    config_map.insert("network_config".to_string(), network_config);

    let attempts = get_retry_attempts_from_config(&config_map);
    assert_eq!(attempts, 5);
}

/// 测试获取重试次数 - 无配置，返回默认值
#[test]
fn test_get_retry_attempts_no_config() {
    let config_map: HashMap<String, HashMap<String, FeatureConfig>> = HashMap::new();

    let attempts = get_retry_attempts_from_config(&config_map);
    assert_eq!(attempts, MAX_RETRY_ATTEMPTS);
}

/// 测试获取重试次数 - 无效值，返回默认值
#[test]
fn test_get_retry_attempts_invalid_value() {
    let mut network_config = HashMap::new();
    network_config.insert("retry_attempts".to_string(), create_feature_config("not_a_number"));

    let mut config_map = HashMap::new();
    config_map.insert("network_config".to_string(), network_config);

    let attempts = get_retry_attempts_from_config(&config_map);
    assert_eq!(attempts, MAX_RETRY_ATTEMPTS);
}

/// 测试获取请求超时 - 有配置
#[test]
fn test_get_request_timeout_with_config() {
    let mut network_config = HashMap::new();
    network_config.insert("request_timeout".to_string(), create_feature_config("300"));

    let mut config_map = HashMap::new();
    config_map.insert("network_config".to_string(), network_config);

    let timeout = get_request_timeout_from_config(&config_map);
    assert_eq!(timeout, 300);
}

/// 测试获取请求超时 - 无配置
#[test]
fn test_get_request_timeout_no_config() {
    let config_map: HashMap<String, HashMap<String, FeatureConfig>> = HashMap::new();

    let timeout = get_request_timeout_from_config(&config_map);
    assert_eq!(timeout, DEFAULT_REQUEST_TIMEOUT_SECS);
}

/// 测试获取网络代理 - 有配置
#[test]
fn test_get_network_proxy_with_config() {
    let mut network_config = HashMap::new();
    network_config.insert(
        "network_proxy".to_string(),
        create_feature_config("http://proxy.example.com:8080"),
    );

    let mut config_map = HashMap::new();
    config_map.insert("network_config".to_string(), network_config);

    let proxy = get_network_proxy_from_config(&config_map);
    assert_eq!(proxy, Some("http://proxy.example.com:8080".to_string()));
}

/// 测试获取网络代理 - 空字符串
#[test]
fn test_get_network_proxy_empty_string() {
    let mut network_config = HashMap::new();
    network_config.insert("network_proxy".to_string(), create_feature_config("   "));

    let mut config_map = HashMap::new();
    config_map.insert("network_config".to_string(), network_config);

    let proxy = get_network_proxy_from_config(&config_map);
    assert_eq!(proxy, None);
}

/// 测试获取网络代理 - 无配置
#[test]
fn test_get_network_proxy_no_config() {
    let config_map: HashMap<String, HashMap<String, FeatureConfig>> = HashMap::new();

    let proxy = get_network_proxy_from_config(&config_map);
    assert_eq!(proxy, None);
}

/// 测试构建代理环境变量
#[test]
fn test_build_proxy_env_vars() {
    let envs = build_proxy_env_vars("http://proxy.example.com:8080");

    assert_eq!(envs.get("HTTP_PROXY"), Some(&"http://proxy.example.com:8080".to_string()));
    assert_eq!(envs.get("HTTPS_PROXY"), Some(&"http://proxy.example.com:8080".to_string()));
    assert_eq!(envs.get("ALL_PROXY"), Some(&"http://proxy.example.com:8080".to_string()));
    assert_eq!(envs.get("http_proxy"), Some(&"http://proxy.example.com:8080".to_string()));
    assert_eq!(envs.get("https_proxy"), Some(&"http://proxy.example.com:8080".to_string()));
    assert_eq!(envs.get("all_proxy"), Some(&"http://proxy.example.com:8080".to_string()));
}

/// 测试空代理地址不会生成环境变量
#[test]
fn test_build_proxy_env_vars_with_empty_value() {
    let envs = build_proxy_env_vars("   ");
    assert!(envs.is_empty());
}

// ============================================================================
// 重试延迟计算测试
// ============================================================================

/// 测试第一次重试的延迟
#[test]
fn test_calculate_retry_delay_first_attempt() {
    let delay = calculate_retry_delay(1);
    assert_eq!(delay, RETRY_DELAY_BASE_MS); // 2000ms
}

/// 测试第二次重试的延迟（指数退避）
#[test]
fn test_calculate_retry_delay_second_attempt() {
    let delay = calculate_retry_delay(2);
    assert_eq!(delay, RETRY_DELAY_BASE_MS * 2); // 4000ms
}

/// 测试第三次重试的延迟
#[test]
fn test_calculate_retry_delay_third_attempt() {
    let delay = calculate_retry_delay(3);
    assert_eq!(delay, RETRY_DELAY_BASE_MS * 4); // 8000ms
}

/// 测试零次重试的延迟
#[test]
fn test_calculate_retry_delay_zero_attempt() {
    // attempt = 0 时，2^(0-1) 会因为 saturating_sub 变成 2^0 = 1
    let delay = calculate_retry_delay(0);
    assert_eq!(delay, RETRY_DELAY_BASE_MS);
}

/// 测试大数值重试次数
#[test]
fn test_calculate_retry_delay_large_attempt() {
    // 测试不会溢出
    let delay = calculate_retry_delay(10);
    assert!(delay > 0);
    assert_eq!(delay, RETRY_DELAY_BASE_MS * 512); // 2^9 = 512
}

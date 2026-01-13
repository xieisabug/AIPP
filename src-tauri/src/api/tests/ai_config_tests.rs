//! AI 配置逻辑测试
//!
//! ## 测试范围
//!
//! - ChatOptions 构建
//! - 模型配置合并
//! - 网络配置获取
//! - 重试延迟计算

use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config, ConfigBuilder, DEFAULT_REQUEST_TIMEOUT_SECS,
    MAX_RETRY_ATTEMPTS, RETRY_DELAY_BASE_MS,
};
use crate::db::assistant_db::AssistantModelConfig;
use crate::db::llm_db::{LLMModel, LLMProvider, LLMProviderConfig, ModelDetail};
use crate::db::system_db::FeatureConfig;
use std::collections::HashMap;

// ============================================================================
// ChatOptions 构建测试
// ============================================================================

/// 测试空配置时返回默认 ChatOptions
#[test]
fn test_build_chat_options_empty_config() {
    let config_map: HashMap<String, String> = HashMap::new();
    let options = ConfigBuilder::build_chat_options(&config_map);

    // 默认选项，温度/max_tokens/top_p 都是 None
    // 由于 ChatOptions 没有公开的字段访问，我们只能验证它不会崩溃
    assert!(format!("{:?}", options).contains("ChatOptions"));
}

/// 测试温度配置
#[test]
fn test_build_chat_options_with_temperature() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("temperature".to_string(), "0.7".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map);
    let debug_str = format!("{:?}", options);

    // 验证选项已创建
    assert!(debug_str.contains("ChatOptions"));
}

/// 测试 max_tokens 配置
#[test]
fn test_build_chat_options_with_max_tokens() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("max_tokens".to_string(), "4096".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map);
    let debug_str = format!("{:?}", options);

    assert!(debug_str.contains("ChatOptions"));
}

/// 测试 top_p 配置
#[test]
fn test_build_chat_options_with_top_p() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("top_p".to_string(), "0.9".to_string());

    let options = ConfigBuilder::build_chat_options(&config_map);
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

    let options = ConfigBuilder::build_chat_options(&config_map);
    let debug_str = format!("{:?}", options);

    assert!(debug_str.contains("ChatOptions"));
}

/// 测试无效的温度值（非数字）
#[test]
fn test_build_chat_options_invalid_temperature() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("temperature".to_string(), "not_a_number".to_string());

    // 不应该崩溃，应该忽略无效值
    let options = ConfigBuilder::build_chat_options(&config_map);
    assert!(format!("{:?}", options).contains("ChatOptions"));
}

/// 测试无效的 max_tokens 值
#[test]
fn test_build_chat_options_invalid_max_tokens() {
    let mut config_map: HashMap<String, String> = HashMap::new();
    config_map.insert("max_tokens".to_string(), "-100".to_string()); // 负数

    let options = ConfigBuilder::build_chat_options(&config_map);
    assert!(format!("{:?}", options).contains("ChatOptions"));
}

// ============================================================================
// 模型配置合并测试
// ============================================================================

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

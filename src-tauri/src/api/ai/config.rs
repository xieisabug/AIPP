use crate::db::assistant_db::AssistantModelConfig;
use crate::db::system_db::FeatureConfig;
use genai::chat::{CacheControl, ChatOptions};
use genai::Client;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ChatConfig {
    pub model_name: String,
    pub stream: bool,
    pub chat_options: ChatOptions,
    pub client: Client,
}

pub struct ConfigBuilder;

#[derive(Debug, Clone)]
pub struct OpenAiCacheContext {
    pub provider_id: i64,
    pub provider_api_type: String,
    pub model_code: String,
    pub request_mode: String,
    pub assistant_id: i64,
    pub conversation_id: i64,
}

impl ConfigBuilder {
    pub fn build_chat_options(
        config_map: &HashMap<String, String>,
        feature_config_map: Option<&HashMap<String, HashMap<String, FeatureConfig>>>,
        openai_cache_context: Option<&OpenAiCacheContext>,
    ) -> ChatOptions {
        let mut chat_options = ChatOptions::default();
        if let Some(temp_str) = config_map.get("temperature") {
            if let Ok(temp) = temp_str.parse::<f64>() {
                chat_options = chat_options.with_temperature(temp);
            }
        }
        if let Some(max_tokens_str) = config_map.get("max_tokens") {
            if let Ok(max_tokens) = max_tokens_str.parse::<u32>() {
                chat_options = chat_options.with_max_tokens(max_tokens);
            }
        }
        if let Some(top_p_str) = config_map.get("top_p") {
            if let Ok(top_p) = top_p_str.parse::<f64>() {
                chat_options = chat_options.with_top_p(top_p);
            }
        }
        if let Some(reasoning_str) = config_map.get("reasoning_effort") {
            if let Some(effort) = genai::chat::ReasoningEffort::from_keyword(reasoning_str) {
                chat_options = chat_options.with_reasoning_effort(effort);
            }
        }
        if let (Some(feature_config_map), Some(context)) =
            (feature_config_map, openai_cache_context)
        {
            chat_options =
                apply_openai_prompt_cache_options(chat_options, feature_config_map, context);
        }
        chat_options
    }

    pub fn merge_model_configs(
        base_configs: Vec<AssistantModelConfig>,
        model_detail: &crate::db::llm_db::ModelDetail,
        override_configs: Option<HashMap<String, serde_json::Value>>,
    ) -> Vec<AssistantModelConfig> {
        let mut model_config_clone = base_configs;
        model_config_clone.push(AssistantModelConfig {
            id: 0,
            assistant_id: model_detail.model.id,
            assistant_model_id: model_detail.model.id,
            name: "model".to_string(),
            value: Some(model_detail.model.code.clone()),
            value_type: "string".to_string(),
        });

        if let Some(override_configs) = override_configs {
            for (key, value) in override_configs {
                let value_type = match &value {
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::Bool(_) => "boolean",
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                    serde_json::Value::Null => "null",
                };

                let value_str = match value {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };

                if let Some(existing_config) = model_config_clone.iter_mut().find(|c| c.name == key)
                {
                    existing_config.value = Some(value_str);
                    existing_config.value_type = value_type.to_string();
                } else {
                    model_config_clone.push(AssistantModelConfig {
                        id: 0,
                        assistant_id: model_detail.model.id,
                        assistant_model_id: model_detail.model.id,
                        name: key,
                        value: Some(value_str),
                        value_type: value_type.to_string(),
                    });
                }
            }
        }

        model_config_clone
    }
}

fn is_openai_like_provider(api_type: &str) -> bool {
    let api_type = api_type.trim().to_ascii_lowercase();
    api_type == "openai" || api_type == "openai_api"
}

fn is_enabled_feature_value(value: Option<&str>, default_value: bool) -> bool {
    match value.map(str::trim).map(str::to_ascii_lowercase) {
        Some(value) if value == "false" || value == "0" || value == "off" => false,
        Some(value) if value == "true" || value == "1" || value == "on" => true,
        _ => default_value,
    }
}

fn network_config_value<'a>(
    config_feature_map: &'a HashMap<String, HashMap<String, FeatureConfig>>,
    key: &str,
) -> Option<&'a str> {
    config_feature_map
        .get("network_config")
        .and_then(|network_config| network_config.get(key))
        .map(|config| config.value.as_str())
}

pub fn get_openai_prompt_cache_key_enabled(
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> bool {
    is_enabled_feature_value(
        network_config_value(config_feature_map, "openai_prompt_cache_key_enabled"),
        true,
    )
}

pub fn get_openai_responses_stateful_enabled(
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> bool {
    is_enabled_feature_value(
        network_config_value(config_feature_map, "openai_responses_stateful_enabled"),
        false,
    )
}

pub fn should_use_openai_responses_features(provider_api_type: &str, request_mode: &str) -> bool {
    is_openai_like_provider(provider_api_type) && request_mode.eq_ignore_ascii_case("responses")
}

pub fn build_openai_prompt_cache_key(context: &OpenAiCacheContext) -> String {
    format!(
        "aipp:{}:{}:{}:{}",
        context.provider_id, context.model_code, context.assistant_id, context.conversation_id
    )
}

fn apply_openai_prompt_cache_options(
    mut chat_options: ChatOptions,
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
    context: &OpenAiCacheContext,
) -> ChatOptions {
    if !should_use_openai_responses_features(&context.provider_api_type, &context.request_mode)
        || !get_openai_prompt_cache_key_enabled(config_feature_map)
    {
        return chat_options;
    }

    chat_options = chat_options.with_prompt_cache_key(build_openai_prompt_cache_key(context));

    let retention =
        network_config_value(config_feature_map, "openai_prompt_cache_retention").unwrap_or("24h");
    if retention.trim().eq_ignore_ascii_case("24h") {
        chat_options = chat_options.with_cache_control(CacheControl::Ephemeral24h);
    }

    chat_options
}

pub const MAX_RETRY_ATTEMPTS: u32 = 3;
pub const RETRY_DELAY_BASE_MS: u64 = 2000;
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 180; // 3分钟默认超时

/// 从网络配置中获取重试次数，如果没有配置则使用默认值
pub fn get_retry_attempts_from_config(
    config_feature_map: &HashMap<String, HashMap<String, crate::db::system_db::FeatureConfig>>,
) -> u32 {
    if let Some(network_config) = config_feature_map.get("network_config") {
        if let Some(retry_config) = network_config.get("retry_attempts") {
            if let Ok(attempts) = retry_config.value.parse::<u32>() {
                return attempts;
            }
        }
    }
    MAX_RETRY_ATTEMPTS
}

/// 从网络配置中获取请求超时时间（秒），如果没有配置则使用默认值
pub fn get_request_timeout_from_config(
    config_feature_map: &HashMap<String, HashMap<String, crate::db::system_db::FeatureConfig>>,
) -> u64 {
    if let Some(network_config) = config_feature_map.get("network_config") {
        if let Some(timeout_config) = network_config.get("request_timeout") {
            if let Ok(timeout) = timeout_config.value.parse::<u64>() {
                return timeout;
            }
        }
    }
    DEFAULT_REQUEST_TIMEOUT_SECS
}

/// 从网络配置中获取网络代理URL
pub fn get_network_proxy_from_config(
    config_feature_map: &HashMap<String, HashMap<String, crate::db::system_db::FeatureConfig>>,
) -> Option<String> {
    if let Some(network_config) = config_feature_map.get("network_config") {
        if let Some(proxy_config) = network_config.get("network_proxy") {
            let proxy_url = proxy_config.value.trim();
            if !proxy_url.is_empty() {
                return Some(proxy_url.to_string());
            }
        }
    }
    None
}

/// 根据代理地址构建常见的代理环境变量
pub fn build_proxy_env_vars(proxy_url: &str) -> HashMap<String, String> {
    let proxy_url = proxy_url.trim();
    if proxy_url.is_empty() {
        return HashMap::new();
    }

    ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"]
        .into_iter()
        .map(|key| (key.to_string(), proxy_url.to_string()))
        .collect()
}

/// 从工具错误配置中获取是否继续对话（默认开启）
pub fn get_continue_on_tool_error_from_config(
    config_feature_map: &HashMap<String, HashMap<String, crate::db::system_db::FeatureConfig>>,
) -> bool {
    if let Some(tool_config) = config_feature_map.get("tool_error_continue") {
        if let Some(enabled_config) = tool_config.get("enabled") {
            let raw_value = enabled_config.value.trim().to_lowercase();
            if raw_value == "false" || raw_value == "0" {
                return false;
            }
        }
    }
    true
}

/// 从网络配置中获取自定义 HTTP Headers
pub fn get_custom_headers_from_config(
    config_feature_map: &HashMap<String, HashMap<String, crate::db::system_db::FeatureConfig>>,
) -> HashMap<String, String> {
    if let Some(network_config) = config_feature_map.get("network_config") {
        if let Some(headers_config) = network_config.get("custom_headers") {
            let headers_json = headers_config.value.trim();
            if !headers_json.is_empty() {
                if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(headers_json) {
                    return parsed;
                }
            }
        }
    }
    HashMap::new()
}

/// 计算重试延迟，使用指数退避策略
pub fn calculate_retry_delay(attempt: u32) -> u64 {
    RETRY_DELAY_BASE_MS * (2_u64.pow(attempt.saturating_sub(1)))
}

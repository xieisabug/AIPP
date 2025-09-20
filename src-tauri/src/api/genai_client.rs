use crate::errors::AppError;
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{adapter::AdapterKind, ModelIden, ServiceTarget};
use genai::{Client, WebConfig};
use std::time::Duration;
use tracing::{debug, info, warn};

// 默认端点映射
pub const DEFAULT_ENDPOINTS: &[(AdapterKind, &str)] = &[
    (AdapterKind::OpenAI, "https://api.openai.com/v1/"),
    (AdapterKind::Anthropic, "https://api.anthropic.com/"),
    (AdapterKind::Cohere, "https://api.cohere.ai/v1/"),
    (AdapterKind::Gemini, "https://generativelanguage.googleapis.com/v1beta/"),
    (AdapterKind::Groq, "https://api.groq.com/openai/v1/"),
    (AdapterKind::Xai, "https://api.x.ai/v1/"),
    (AdapterKind::DeepSeek, "https://api.deepseek.com/"),
    (AdapterKind::Ollama, "http://localhost:11434/v1/"),
];

/// 推断适配器类型
pub fn infer_adapter_kind(model_name: &str, api_type: &str) -> AdapterKind {
    debug!(model_name = %model_name, api_type = %api_type, "infer_adapter_kind called");
    match api_type.to_lowercase().as_str() {
        "openai" => AdapterKind::OpenAI,
        "openai_api" => AdapterKind::OpenAI,
        "anthropic" => AdapterKind::Anthropic,
        "cohere" => AdapterKind::Cohere,
        "gemini" => AdapterKind::Gemini,
        "groq" => AdapterKind::Groq,
        "xai" => AdapterKind::Xai,
        "deepseek" => AdapterKind::DeepSeek,
        "ollama" => AdapterKind::Ollama,
        _ => {
            // 根据模型名称推断
            let model_lower = model_name.to_lowercase();
            if model_lower.contains("gpt") || model_lower.contains("o1") {
                AdapterKind::OpenAI
            } else if model_lower.contains("claude") {
                AdapterKind::Anthropic
            } else if model_lower.contains("gemini") {
                AdapterKind::Gemini
            } else if model_lower.contains("llama") || model_lower.contains("qwen") {
                AdapterKind::Ollama
            } else {
                AdapterKind::OpenAI // 默认
            }
        }
    }
}

/// 仅根据API类型推断适配器类型（用于llm_api.rs）
pub fn infer_adapter_kind_simple(api_type: &str) -> AdapterKind {
    match api_type.to_lowercase().as_str() {
        "openai" => AdapterKind::OpenAI,
        "openai_api" => AdapterKind::OpenAI,
        "anthropic" => AdapterKind::Anthropic,
        "cohere" => AdapterKind::Cohere,
        "gemini" => AdapterKind::Gemini,
        "groq" => AdapterKind::Groq,
        "xai" => AdapterKind::Xai,
        "deepseek" => AdapterKind::DeepSeek,
        "ollama" => AdapterKind::Ollama,
        _ => AdapterKind::OpenAI, // 默认
    }
}

/// 获取默认端点
pub fn get_default_endpoint(adapter_kind: AdapterKind) -> &'static str {
    DEFAULT_ENDPOINTS
        .iter()
        .find(|(kind, _)| *kind == adapter_kind)
        .map(|(_, endpoint)| *endpoint)
        .unwrap_or("https://api.openai.com/v1/")
}

/// 创建客户端配置
pub fn create_client_with_config(
    configs: &[crate::db::llm_db::LLMProviderConfig],
    model_name: &str,
    api_type: &str,
    network_proxy: Option<&str>,
    proxy_enabled: bool,
    request_timeout: Option<u64>, // 超时时间（秒）
) -> Result<Client, AppError> {
    let adapter_kind = infer_adapter_kind(model_name, api_type);

    let mut api_key = String::new();
    let mut endpoint_opt: Option<String> = None;

    for config in configs {
        match config.name.as_str() {
            "api_key" => {
                api_key = config.value.clone();
            }
            "endpoint" => {
                let trimmed = config.value.trim();
                if !trimmed.is_empty() {
                    endpoint_opt = Some(trimmed.to_string());
                }
            }
            _ => {}
        }
    }

    // 构建 WebConfig 配置代理和超时
    let mut web_config = WebConfig::default();

    // 配置超时
    if let Some(timeout_secs) = request_timeout {
        if timeout_secs > 0 {
            web_config = web_config.with_timeout(Duration::from_secs(timeout_secs));
            info!(timeout_secs, "request timeout configured");
        }
    }

    // 配置代理
    if proxy_enabled {
        if let Some(proxy_url) = network_proxy {
            if !proxy_url.trim().is_empty() {
                match reqwest::Proxy::all(proxy_url) {
                    Ok(proxy) => {
                        web_config = WebConfig::default().with_proxy(proxy);
                        if let Some(timeout_secs) = request_timeout {
                            if timeout_secs > 0 {
                                web_config =
                                    web_config.with_timeout(Duration::from_secs(timeout_secs));
                            }
                        }
                        info!(proxy_url = %proxy_url, "proxy configured");
                    }
                    Err(e) => {
                        warn!(error = %e, proxy_url = %proxy_url, "proxy configuration failed");
                    }
                }
            }
        }
    }

    // 克隆值以便在闭包中使用
    let api_key_clone = api_key.clone();
    let endpoint_clone = endpoint_opt.clone();

    // 使用 ServiceTargetResolver 来配置端点和认证
    let target_resolver = ServiceTargetResolver::from_resolver_fn(
        move |service_target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
            let ServiceTarget { model, .. } = service_target;

            let endpoint = match endpoint_clone.as_deref() {
                Some(ep) if !ep.trim().is_empty() && ep.trim().starts_with("http") => {
                    let mut endpoint_str = ep.trim().to_string();
                    if !endpoint_str.ends_with('/') {
                        endpoint_str.push('/');
                    }
                    Endpoint::from_owned(endpoint_str)
                }
                _ => {
                    let default_endpoint = get_default_endpoint(adapter_kind);
                    Endpoint::from_static(default_endpoint)
                }
            };

            let auth = AuthData::from_single(api_key_clone.clone());
            let model = ModelIden::new(adapter_kind, model.model_name);

            debug!(?endpoint, ?model, "resolved service target");

            Ok(ServiceTarget { endpoint, auth, model })
        },
    );

    let client = Client::builder()
        .with_service_target_resolver(target_resolver)
        .with_web_config(web_config)
        .build();

    Ok(client)
}

use crate::api::ai::config::get_custom_headers_from_config;
use crate::errors::AppError;
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{adapter::AdapterKind, ModelIden, ServiceTarget};
use genai::{Client, WebConfig};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::time::Duration;
use tracing::{debug, info, warn};

// 默认端点映射
pub const DEFAULT_ENDPOINTS: &[(AdapterKind, &str)] = &[
    (AdapterKind::OpenAI, "https://api.openai.com/v1/"),
    (AdapterKind::OpenAIResp, "https://api.openai.com/v1/"),
    (AdapterKind::Anthropic, "https://api.anthropic.com/"),
    (AdapterKind::Cohere, "https://api.cohere.ai/v1/"),
    (AdapterKind::Gemini, "https://generativelanguage.googleapis.com/v1beta/"),
    (AdapterKind::Groq, "https://api.groq.com/openai/v1/"),
    (AdapterKind::Xai, "https://api.x.ai/v1/"),
    (AdapterKind::DeepSeek, "https://api.deepseek.com/"),
    (AdapterKind::Ollama, "http://localhost:11434/v1/"),
    (AdapterKind::Copilot, "https://api.individual.githubcopilot.com"),
    (AdapterKind::CopilotResp, "https://api.individual.githubcopilot.com"),
];

pub fn supports_model_request_mode(api_type: &str) -> bool {
    matches!(api_type.to_lowercase().as_str(), "openai" | "openai_api" | "github_copilot")
}

pub fn normalize_request_mode(request_mode: Option<&str>) -> &'static str {
    match request_mode {
        Some("responses") => "responses",
        _ => "chat_completions",
    }
}

/// 推断适配器类型
pub fn infer_adapter_kind(
    model_name: &str,
    api_type: &str,
    request_mode: Option<&str>,
) -> AdapterKind {
    let request_mode = normalize_request_mode(request_mode);
    debug!(
        model_name = %model_name,
        api_type = %api_type,
        request_mode = %request_mode,
        "infer_adapter_kind called"
    );
    match api_type.to_lowercase().as_str() {
        "openai" | "openai_api" => {
            if request_mode == "responses" {
                AdapterKind::OpenAIResp
            } else {
                AdapterKind::OpenAI
            }
        }
        "anthropic" => AdapterKind::Anthropic,
        "cohere" => AdapterKind::Cohere,
        "gemini" => AdapterKind::Gemini,
        "groq" => AdapterKind::Groq,
        "xai" => AdapterKind::Xai,
        "deepseek" => AdapterKind::DeepSeek,
        "ollama" => AdapterKind::Ollama,
        "github_copilot" => {
            if request_mode == "responses" {
                AdapterKind::CopilotResp
            } else {
                AdapterKind::Copilot
            }
        }
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
        "github_copilot" => AdapterKind::Copilot, // Copilot 使用 OpenAI 兼容 API
        _ => AdapterKind::OpenAI,                 // 默认
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

/// 提供商 Endpoint 解析结果：显式区分「未设置」与「自定义有效地址」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderEndpointResolution {
    /// 未填写 / 仅空白：产品语义上使用适配器默认端点
    Unset,
    /// 已通过严格校验的自定义端点（保证以 `/` 结尾）
    Custom(String),
}

/// 将用户填写的 Endpoint 解析为「未设置」或「自定义有效 URL」。
///
/// - 空 / 仅空白 → [`ProviderEndpointResolution::Unset`]（允许回退默认端点）
/// - 非空但无法解析为带主机的 `http`/`https` URL → 本地失败，返回字段级中文错误
/// - 有效自定义地址 → [`ProviderEndpointResolution::Custom`]，绝不静默回退到默认外网端点
pub fn resolve_provider_endpoint(raw: Option<&str>) -> Result<ProviderEndpointResolution, AppError> {
    let Some(raw) = raw else {
        return Ok(ProviderEndpointResolution::Unset);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(ProviderEndpointResolution::Unset);
    }

    let parsed = reqwest::Url::parse(trimmed).map_err(|_| invalid_endpoint_error(trimmed))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(invalid_endpoint_error(trimmed));
    }
    // 必须有主机名，拒绝 `http://`、`http:///path`、相对片段等
    let host = parsed.host_str().unwrap_or("").trim();
    if host.is_empty() {
        return Err(invalid_endpoint_error(trimmed));
    }

    let mut normalized = trimmed.to_string();
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    Ok(ProviderEndpointResolution::Custom(normalized))
}

fn invalid_endpoint_error(raw: &str) -> AppError {
    // 错误文案不得包含默认外网 URL（验收：错误中不出现默认 OpenAI URL）
    AppError::ProviderError(format!(
        "Endpoint 无效：「{raw}」。请填写以 http:// 或 https:// 开头且包含主机名的完整地址；若要使用提供商默认端点，请将 Endpoint 留空。"
    ))
}

/// 空值提示（与无效值错误文案可区分；产品上允许留空走默认端点）。
pub fn unset_endpoint_hint() -> &'static str {
    "Endpoint 未填写，将使用该提供商的默认端点。"
}

/// 解析成功后的最终请求基址（发送前可呈现）。
pub fn effective_endpoint_url(
    resolution: &ProviderEndpointResolution,
    adapter_kind: AdapterKind,
) -> String {
    match resolution {
        ProviderEndpointResolution::Unset => get_default_endpoint(adapter_kind).to_string(),
        ProviderEndpointResolution::Custom(ep) => ep.clone(),
    }
}

/// 创建客户端配置
pub fn create_client_with_config(
    configs: &[crate::db::llm_db::LLMProviderConfig],
    model_name: &str,
    api_type: &str,
    request_mode: Option<&str>,
    network_proxy: Option<&str>,
    proxy_enabled: bool,
    request_timeout: Option<u64>, // 超时时间（秒）
    is_stream: bool,
    config_feature_map: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, crate::db::system_db::FeatureConfig>,
    >,
) -> Result<Client, AppError> {
    let adapter_kind = infer_adapter_kind(model_name, api_type, request_mode);

    let mut api_key = String::new();
    let mut endpoint_raw: Option<String> = None;

    for config in configs {
        match config.name.as_str() {
            "api_key" => {
                api_key = config.value.clone();
            }
            "endpoint" => {
                endpoint_raw = Some(config.value.clone());
            }
            _ => {}
        }
    }

    // 在发起任何网络请求前 fail-closed：非空但非法的 Endpoint 不得静默回退到默认外网地址
    let resolved_endpoint = resolve_provider_endpoint(endpoint_raw.as_deref())?;
    let final_endpoint_url = effective_endpoint_url(&resolved_endpoint, adapter_kind);
    match &resolved_endpoint {
        ProviderEndpointResolution::Unset => {
            info!(
                endpoint = %final_endpoint_url,
                hint = %unset_endpoint_hint(),
                "provider endpoint unset; using adapter default"
            );
        }
        ProviderEndpointResolution::Custom(_) => {
            info!(
                endpoint = %final_endpoint_url,
                "provider endpoint resolved; requests will use this base URL"
            );
        }
    }

    // 构建 WebConfig 配置代理和超时
    let mut web_config = WebConfig::default();

    // 配置超时
    if let Some(timeout_secs) = request_timeout {
        if timeout_secs > 0 {
            let timeout = Duration::from_secs(timeout_secs);
            if is_stream {
                web_config = web_config.with_read_timeout(timeout);
                info!(timeout_secs, "stream read timeout configured");
            } else {
                web_config = web_config.with_timeout(timeout);
                info!(timeout_secs, "request timeout configured");
            }
        }
    }

    // 配置代理
    if proxy_enabled {
        if let Some(proxy_url) = network_proxy {
            if !proxy_url.trim().is_empty() {
                match web_config.clone().with_all_proxy_url(proxy_url) {
                    Ok(config_with_proxy) => {
                        web_config = config_with_proxy;
                        info!(proxy_url = %proxy_url, "proxy configured");
                    }
                    Err(e) => {
                        warn!(error = %e, proxy_url = %proxy_url, "proxy configuration failed");
                    }
                }
            }
        }
    }

    // 配置自定义 headers
    let custom_headers = get_custom_headers_from_config(config_feature_map);
    if !custom_headers.is_empty() {
        let mut headers = HeaderMap::new();
        for (key, value) in custom_headers {
            if let Ok(header_name) = HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = HeaderValue::from_str(&value) {
                    headers.insert(header_name, header_value);
                }
            }
        }
        web_config = web_config.with_default_headers(headers);
        info!("custom headers configured");
    }

    // 克隆值以便在闭包中使用（自定义端点已在上方严格校验）
    let api_key_clone = api_key.clone();
    let custom_endpoint = match resolved_endpoint {
        ProviderEndpointResolution::Unset => None,
        ProviderEndpointResolution::Custom(ep) => Some(ep),
    };

    // 使用 ServiceTargetResolver 来配置端点和认证
    let target_resolver = ServiceTargetResolver::from_resolver_fn(
        move |service_target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
            let ServiceTarget { model, .. } = service_target;

            let endpoint = match custom_endpoint.as_deref() {
                Some(ep) => Endpoint::from_owned(ep.to_string()),
                None => {
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


#[cfg(test)]
mod tests {
    use super::{
        effective_endpoint_url, get_default_endpoint, infer_adapter_kind_simple,
        resolve_provider_endpoint, unset_endpoint_hint, ProviderEndpointResolution,
    };
    use genai::adapter::AdapterKind;

    #[test]
    fn empty_endpoint_is_unset_for_product_default() {
        assert_eq!(
            resolve_provider_endpoint(None).unwrap(),
            ProviderEndpointResolution::Unset
        );
        assert_eq!(
            resolve_provider_endpoint(Some("")).unwrap(),
            ProviderEndpointResolution::Unset
        );
        assert_eq!(
            resolve_provider_endpoint(Some("   ")).unwrap(),
            ProviderEndpointResolution::Unset
        );

        let openai = AdapterKind::OpenAI;
        let url = effective_endpoint_url(&ProviderEndpointResolution::Unset, openai);
        assert_eq!(url, get_default_endpoint(openai));
        // 空值提示与无效值错误必须可区分
        assert!(unset_endpoint_hint().contains("未填写"));
        assert!(!unset_endpoint_hint().contains("无效"));
    }

    #[test]
    fn invalid_endpoint_rejected_without_default_fallback_or_openai_url_leak() {
        let err = resolve_provider_endpoint(Some("bad")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Endpoint 无效"),
            "expected field-level Chinese error, got: {msg}"
        );
        assert!(msg.contains("bad"), "error should echo the invalid value: {msg}");
        assert!(
            !msg.contains("api.openai.com"),
            "invalid-endpoint error must not leak default OpenAI URL: {msg}"
        );
        assert!(
            !msg.contains(unset_endpoint_hint()),
            "invalid error must be distinguishable from empty hint"
        );
        assert!(msg.contains("无效"));
        assert!(!msg.contains("未填写"));

        // 常见「半截 http」也不应静默放行 / 不触发外网默认
        for bad in ["http", "http://", "ftp://example.com", "not a url", "openai.com/v1"] {
            let e = resolve_provider_endpoint(Some(bad)).unwrap_err().to_string();
            assert!(e.contains("Endpoint 无效"), "expected reject for {bad}: {e}");
            assert!(!e.contains("api.openai.com"), "no OpenAI URL leak for {bad}: {e}");
        }
    }

    #[test]
    fn valid_http_https_loopback_kept_as_final_destination() {
        let r = resolve_provider_endpoint(Some("http://127.0.0.1:8080/v1")).unwrap();
        assert_eq!(
            r,
            ProviderEndpointResolution::Custom("http://127.0.0.1:8080/v1/".into())
        );
        assert_eq!(
            effective_endpoint_url(&r, AdapterKind::OpenAI),
            "http://127.0.0.1:8080/v1/"
        );

        let r = resolve_provider_endpoint(Some("https://localhost:8443/v1/")).unwrap();
        assert_eq!(
            r,
            ProviderEndpointResolution::Custom("https://localhost:8443/v1/".into())
        );
        assert_eq!(
            effective_endpoint_url(&r, infer_adapter_kind_simple("openai")),
            "https://localhost:8443/v1/"
        );

        let r = resolve_provider_endpoint(Some("  https://example.com/v1  ")).unwrap();
        assert_eq!(
            r,
            ProviderEndpointResolution::Custom("https://example.com/v1/".into())
        );
    }
}

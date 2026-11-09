use crate::api::ai::config::get_custom_headers_from_config;
use crate::errors::AppError;
use super::{magpie, opencode_go};
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
    matches!(api_type.to_lowercase().as_str(), "openai" | "openai_api" | "github_copilot" | "opencode_go" | "magpie")
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
    if api_type == opencode_go::PROVIDER {
        if let Some((adapter, _)) = opencode_go::route(request_mode) { return adapter; }
    }
    if api_type == magpie::PROVIDER {
        return magpie::adapter_kind(request_mode);
    }
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

    // URL parsers repair inputs such as `http:///host` or `https:host`.
    // Reject those locally instead of silently choosing a repaired destination.
    let (_, authority_and_path) = trimmed
        .split_once("://")
        .ok_or_else(|| invalid_endpoint_error(trimmed))?;
    if authority_and_path.is_empty()
        || authority_and_path.starts_with('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid_endpoint_error(trimmed));
    }
    let mut parsed = reqwest::Url::parse(trimmed).map_err(|_| invalid_endpoint_error(trimmed))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(invalid_endpoint_error(trimmed));
    }
    // 必须有主机名，拒绝 `http://`、`http:///path`、相对片段等
    let host = parsed.host_str().unwrap_or("").trim();
    if host.is_empty()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid_endpoint_error(trimmed));
    }

    if !parsed.path().ends_with('/') {
        parsed.set_path(&format!("{}/", parsed.path()));
    }
    Ok(ProviderEndpointResolution::Custom(parsed.to_string()))
}

fn invalid_endpoint_error(_raw: &str) -> AppError {
    // 错误文案不得包含默认外网 URL（验收：错误中不出现默认 OpenAI URL）
    AppError::ProviderError(
        "Endpoint 无效：请填写以 http:// 或 https:// 开头且包含主机名的完整基址，不含空白、反斜杠、账号密码、查询参数或片段；若要使用提供商默认端点，请将 Endpoint 留空。".to_string()
    )
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
    conversation: Option<(&tauri::AppHandle, i64)>,
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
    let mut adapter_kind = infer_adapter_kind(model_name, api_type, request_mode);

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
    let mut skip_proxy = false;
    let (resolved_endpoint, session) = if api_type == opencode_go::PROVIDER {
        let (_, base) = opencode_go::route(request_mode).ok_or_else(|| AppError::ProviderError(
            format!("OpenCode Go 模型 {model_name} 的协议未受支持，请刷新模型目录")))?;
        if api_key.trim().is_empty() {
            return Err(AppError::ProviderError(format!("OpenCode Go 模型 {model_name} 缺少 API Key")));
        }
        let (app, id) = conversation.ok_or_else(|| AppError::ProviderError(format!(
            "OpenCode Go 模型 {model_name} 需要持久化会话，此调用没有会话标识")))?;
        let sync_id = crate::sync::conversation_sync_id(app, id).map_err(|e| AppError::ProviderError(
            format!("OpenCode Go 会话 {id} 获取同步 UUID 失败: {e}")))?;
        (ProviderEndpointResolution::Custom(base.to_owned()), Some(opencode_go::session_id(&sync_id)))
    } else if api_type == magpie::PROVIDER {
        let origin = match &resolved_endpoint {
            ProviderEndpointResolution::Custom(url) => magpie::origin_from_endpoint(url).map_err(AppError::ProviderError)?,
            ProviderEndpointResolution::Unset => magpie::default_origin(),
        };
        let (kind, base) = magpie::route(request_mode, &origin).map_err(AppError::ProviderError)?;
        adapter_kind = kind;
        api_key = magpie::api_key_or_default(Some(api_key.as_str()));
        skip_proxy = magpie::origin_is_loopback(&origin);
        (ProviderEndpointResolution::Custom(base), None)
    } else { (resolved_endpoint, None) };
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

    // Go continuations and auxiliary requests inherit the provider's proxy setting.
    let go_proxy = if session.is_some() && configs.iter().any(|c| c.name == "proxy_enabled" && c.value == "true") {
        crate::api::ai::config::get_network_proxy_from_config(config_feature_map)
    } else { None };
    let network_proxy = go_proxy.as_deref().or(network_proxy);
    if !skip_proxy && (proxy_enabled || go_proxy.is_some()) {
        if let Some(proxy_url) = network_proxy {
            if !proxy_url.trim().is_empty() {
                match web_config.clone().with_all_proxy_url(proxy_url) {
                    Ok(config_with_proxy) => {
                        web_config = config_with_proxy;
                        info!(proxy_url = %proxy_url, "proxy configured");
                    }
                    Err(e) if session.is_some() => {
                        return Err(AppError::ProviderError(format!("OpenCode Go 代理配置失败: {e}")));
                    }
                    Err(e) => {
                        warn!(error = %e, proxy_url = %proxy_url, "proxy configuration failed");
                    }
                }
            }
        }
    }

    let go_model = session.map(|_| model_name.to_owned());
    let mut custom_headers = get_custom_headers_from_config(config_feature_map);
    if api_type == magpie::PROVIDER {
        custom_headers.insert("User-Agent".to_string(), magpie::USER_AGENT.to_string());
        custom_headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
        if let Some((app, id)) = conversation {
            match crate::sync::conversation_sync_id(app, id) {
                Ok(sync_id) => { custom_headers.insert("X-Magpie-Session".to_string(), sync_id); }
                Err(e) => warn!(error = %e, conversation_id = id, "Magpie 会话标识不可用，本次请求不带 X-Magpie-Session"),
            }
        }
    }
    build_client(adapter_kind, final_endpoint_url, api_key, go_model, session, web_config, custom_headers)
}

fn build_client(adapter_kind: AdapterKind, endpoint_url: String, api_key: String,
    go_model: Option<String>, session: Option<uuid::Uuid>, mut web_config: WebConfig,
    custom_headers: std::collections::HashMap<String, String>) -> Result<Client, AppError> {
    if !custom_headers.is_empty() || session.is_some() {
        let mut headers = HeaderMap::new();
        for (key, value) in custom_headers {
            if let Ok(header_name) = HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = HeaderValue::from_str(&value) {
                    headers.insert(header_name, header_value);
                }
            }
        }
        if let Some(session) = session {
            headers.insert("user-agent", HeaderValue::from_static(opencode_go::USER_AGENT));
            headers.insert("x-opencode-session", HeaderValue::from_str(&session.to_string()).expect("UUID is ASCII"));
        }
        web_config = web_config.with_default_headers(headers);
        info!("custom headers configured");
    }

    // 使用 ServiceTargetResolver 来配置端点和认证
    let target_resolver = ServiceTargetResolver::from_resolver_fn(
        move |service_target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
            let ServiceTarget { model, .. } = service_target;

            let endpoint = Endpoint::from_owned(endpoint_url.clone());
            let auth = AuthData::from_single(api_key.clone());
            let model = match &go_model {
                Some(code) => ModelIden::new(adapter_kind, code.clone()),
                None => ModelIden::new(adapter_kind, model.model_name),
            };

            debug!(?endpoint, ?model, "resolved service target");

            Ok(ServiceTarget { endpoint, auth, model })
        },
    );

    Client::builder()
        .with_service_target_resolver(target_resolver)
        .with_web_config(web_config)
        .build()
        .map_err(|error| AppError::ProviderError(format!("Failed to build genai client: {error}")))
}


#[cfg(test)]
#[path = "genai_client_tests.rs"]
mod tests;

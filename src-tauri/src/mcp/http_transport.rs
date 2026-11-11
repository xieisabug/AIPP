use crate::db::mcp_db::MCPServer;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tauri::Manager;
use rmcp::transport::{
    streamable_http_client::StreamableHttpClientTransportConfig,
    StreamableHttpClientTransport,
};

pub(crate) async fn configured_http_builder(app: &tauri::AppHandle, server: &MCPServer) -> Result<reqwest::ClientBuilder, String> {
    let mut builder = reqwest::Client::builder();
    if server.proxy_enabled {
        let state = app.try_state::<crate::FeatureConfigState>().ok_or("网络配置状态不可用")?;
        let config = state.config_feature_map.lock().await;
        let proxy = crate::api::ai::config::get_network_proxy_from_config(&config).filter(|value| !value.trim().is_empty()).ok_or("此连接启用了代理，但全局网络代理尚未配置")?;
        builder = builder.proxy(reqwest::Proxy::all(&proxy).map_err(|_| "全局网络代理地址格式无效")?);
    }
    Ok(builder)
}

/// Preserve configured headers, including the complete Authorization scheme.
/// rmcp's auth_header accepts a bare bearer token, not an HTTP header value.
pub(crate) fn create_http_transport(
    server: &MCPServer,
    builder: reqwest::ClientBuilder,
) -> Result<StreamableHttpClientTransport<reqwest::Client>, String> {
    let url = server.url.as_deref().ok_or("No URL specified for HTTP transport")?;
    if let Some(raw) = server.headers.as_deref().filter(|raw| !raw.trim().is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(raw)
            .map_err(|_| "Invalid MCP HTTP headers: expected a JSON object of string values".to_string())?;
        let object = parsed.as_object()
            .ok_or("Invalid MCP HTTP headers: expected a JSON object of string values")?;
        if object.values().any(|value| !value.is_string()) {
            return Err("Invalid MCP HTTP headers: all values must be strings".into());
        }
    }
    let (_, headers) = super::util::parse_server_headers(server);
    let mut header_map = HeaderMap::new();
    for (name, value) in headers.unwrap_or_default() {
        if name.eq_ignore_ascii_case(super::apps::credentials::REFERENCE_HEADER) {
            return Err("目录应用凭据必须通过受控 HTTP 执行入口解析".into());
        }
        let name = HeaderName::try_from(name.as_str())
            .map_err(|error| format!("Invalid MCP HTTP header name: {error}"))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|error| format!("Invalid MCP HTTP header value for {name}: {error}"))?;
        header_map.insert(name, value);
    }
    let client = builder.redirect(reqwest::redirect::Policy::none()).default_headers(header_map).build()
        .map_err(|error| format!("Failed to build MCP HTTP client: {error}"))?;
    Ok(StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(url),
    ))
}

pub(crate) async fn create_authenticated_http_transport(
    app: &tauri::AppHandle,
    server: &MCPServer,
    builder: reqwest::ClientBuilder,
) -> Result<(StreamableHttpClientTransport<reqwest::Client>, ResponseRedactor), String> {
    let mut headers: std::collections::HashMap<String, String> = match server.headers.as_deref().filter(|v| !v.trim().is_empty()) {
        Some(raw) => serde_json::from_str(raw).map_err(|_| "Invalid MCP HTTP headers: expected string values")?,
        None => Default::default(),
    };
    let keys: Vec<_> = headers.keys().filter(|key| key.eq_ignore_ascii_case(super::apps::credentials::REFERENCE_HEADER)).cloned().collect();
    if keys.is_empty() { return Ok((create_http_transport(server, builder)?, ResponseRedactor::default())); }
    if keys.len() != 1 { return Err("连接包含重复凭据引用".into()); }
    let reference = headers.remove(&keys[0]).unwrap();
    if !server.is_enabled { return Err(format!("连接 {} 已停用", server.id)); }
    let secrets = super::apps::credentials::resolve(app, &reference, server.url.as_deref().ok_or("连接缺少服务地址")?, server.id).await?;
    let redactor = ResponseRedactor::new(secrets.values().cloned());
    for (key, value) in secrets {
        headers.retain(|existing, _| !existing.eq_ignore_ascii_case(&key));
        headers.insert(key, value);
    }
    // Resolved credentials are literal values, never environment-variable templates.
    let mut header_map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::try_from(name.as_str()).map_err(|_| "应用请求头名称无效")?;
        let mut value = HeaderValue::from_str(&value).map_err(|_| "应用请求头值无效")?;
        value.set_sensitive(true);
        header_map.insert(name, value);
    }
    let client = builder.redirect(reqwest::redirect::Policy::none()).default_headers(header_map).build()
        .map_err(|_| "无法创建应用 HTTP 客户端")?;
    Ok((StreamableHttpClientTransport::with_client(client,
        StreamableHttpClientTransportConfig::with_uri(server.url.as_deref().ok_or("连接缺少服务地址")?)), redactor))
}

#[derive(Default)]
pub(crate) struct ResponseRedactor(Vec<String>);

impl ResponseRedactor {
    pub(crate) fn new(values: impl Iterator<Item=String>) -> Self {
        let mut secrets = Vec::new();
        for value in values {
            if let Some(token) = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("Basic ")) { secrets.push(token.to_owned()); }
            secrets.push(value);
        }
        secrets.retain(|s| !s.is_empty());
        // Tool content and registry schemas are serialized before cleanup; errors can
        // also contain JSON or Debug-quoted strings. Match the escaped string contents
        // as well as the literal credential, without consuming surrounding JSON quotes.
        let escaped: Vec<_> = secrets.iter().map(|secret| {
            let json = serde_json::to_string(secret).expect("serializing a string cannot fail");
            json[1..json.len() - 1].to_owned()
        }).collect();
        secrets.extend(escaped);
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        Self(secrets)
    }
    pub(crate) fn clean(&self, text: &str) -> String {
        let mut result = text.to_owned();
        for secret in &self.0 { result = result.replace(secret, "[REDACTED]"); }
        result
    }
}

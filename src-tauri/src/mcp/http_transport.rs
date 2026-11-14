use crate::db::mcp_db::MCPServer;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tauri::Manager;
use rmcp::model::ClientJsonRpcMessage;
use rmcp::transport::{
    common::client_side_sse::BoxedSseResponse,
    streamable_http_client::{StreamableHttpClient, StreamableHttpClientTransportConfig, StreamableHttpError, StreamableHttpPostResponse},
    StreamableHttpClientTransport,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Raised when the MCP server rejects the request at the HTTP layer (401). It is a typed signal on the
/// transport, so a tool result or JSON-RPC error that merely *mentions* "401" can never disable a connection.
#[derive(Clone, Default)]
pub(crate) struct AuthSignal(Arc<AtomicBool>);

impl AuthSignal {
    pub(crate) fn rejected(&self) -> bool { self.0.load(Ordering::SeqCst) }
}

/// rmcp's reqwest client turns a 401 into an error that the transport worker only logs; callers just see
/// "connection closed". Wrapping the client is the only place the status is still observable.
#[derive(Clone)]
pub(crate) struct AuthAwareClient { inner: reqwest::Client, signal: AuthSignal }

impl AuthAwareClient {
    pub(crate) fn new(inner: reqwest::Client) -> (Self, AuthSignal) {
        let signal = AuthSignal::default();
        (Self { inner, signal: signal.clone() }, signal)
    }
    fn observe<T>(&self, result: &Result<T, StreamableHttpError<reqwest::Error>>) {
        if result.as_ref().err().is_some_and(is_auth_rejection) { self.signal.0.store(true, Ordering::SeqCst); }
    }
}

pub(crate) fn is_auth_rejection(error: &StreamableHttpError<reqwest::Error>) -> bool {
    match error {
        StreamableHttpError::AuthRequired(_) => true,
        StreamableHttpError::Client(error) => error.status() == Some(reqwest::StatusCode::UNAUTHORIZED),
        _ => false,
    }
}

impl StreamableHttpClient for AuthAwareClient {
    type Error = reqwest::Error;
    async fn post_message(&self, uri: Arc<str>, message: ClientJsonRpcMessage, session_id: Option<Arc<str>>, auth_header: Option<String>, custom_headers: HashMap<HeaderName, HeaderValue>)
        -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let result = self.inner.post_message(uri, message, session_id, auth_header, custom_headers).await;
        self.observe(&result);
        result
    }
    async fn delete_session(&self, uri: Arc<str>, session_id: Arc<str>, auth_header: Option<String>) -> Result<(), StreamableHttpError<Self::Error>> {
        let result = self.inner.delete_session(uri, session_id, auth_header).await;
        self.observe(&result);
        result
    }
    async fn get_stream(&self, uri: Arc<str>, session_id: Arc<str>, last_event_id: Option<String>, auth_header: Option<String>)
        -> Result<BoxedSseResponse, StreamableHttpError<Self::Error>> {
        let result = self.inner.get_stream(uri, session_id, last_event_id, auth_header).await;
        self.observe(&result);
        result
    }
}

pub(crate) type AppHttpTransport = StreamableHttpClientTransport<AuthAwareClient>;

/// Appended to transport failures when the server answered 401, so the cause is not lost behind
/// rmcp's generic "connection closed".
pub(crate) const AUTH_REJECTED_MESSAGE: &str = "服务返回 HTTP 401，拒绝了当前凭据";

/// If the transport saw a 401, mark the managed connection as needing re-login and put the real cause in
/// front of the (already redacted) error. Without a 401 the error is returned unchanged and nothing is recorded.
pub(crate) fn explain_failure(app: &tauri::AppHandle, server: &MCPServer, signal: &AuthSignal, error: String) -> String {
    if !signal.rejected() { return error; }
    super::apps::record_auth_failure(app, server, AUTH_REJECTED_MESSAGE);
    format!("{AUTH_REJECTED_MESSAGE}：{error}")
}

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
/// Plain transport without the 401 signal; production goes through `create_authenticated_http_transport`.
#[cfg(test)]
pub(crate) fn create_http_transport(
    server: &MCPServer,
    builder: reqwest::ClientBuilder,
) -> Result<StreamableHttpClientTransport<reqwest::Client>, String> {
    let (client, url) = build_plain_client(server, builder, &[])?;
    Ok(StreamableHttpClientTransport::with_client(client, StreamableHttpClientTransportConfig::with_uri(url)))
}

/// `secrets` are values already substituted from vault references; headers carrying them are sensitive.
fn build_plain_client(server: &MCPServer, builder: reqwest::ClientBuilder, secrets: &[String]) -> Result<(reqwest::Client, String), String> {
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
        if crate::vault::config_ref::contains(&value) {
            return Err("保险箱引用必须通过受控 HTTP 执行入口解析".into());
        }
        let name = HeaderName::try_from(name.as_str())
            .map_err(|error| format!("Invalid MCP HTTP header name: {error}"))?;
        let sensitive = secrets.iter().any(|secret| value.contains(secret.as_str()));
        let mut value = HeaderValue::from_str(&value)
            .map_err(|_| format!("Invalid MCP HTTP header value for {name}"))?;
        value.set_sensitive(sensitive);
        header_map.insert(name, value);
    }
    let client = builder.redirect(reqwest::redirect::Policy::none()).default_headers(header_map).build()
        .map_err(|error| format!("Failed to build MCP HTTP client: {error}"))?;
    Ok((client, url.to_owned()))
}

/// Header values that are credentials and must be scrubbed from responses. Non-secret header-bound
/// fields (e.g. a read-only switch sending the value true) are sent but never redacted: scrubbing such
/// a common value would corrupt every tool schema and result that contains it. Without a known secret
/// set (definition unreadable) everything is treated as secret.
pub(crate) fn secret_header_values(headers: &HashMap<String, String>, secret_names: Option<&HashSet<String>>) -> Vec<String> {
    headers.iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization")
            || secret_names.is_none_or(|names| names.contains(&name.to_ascii_lowercase())))
        .map(|(_, value)| value.clone())
        .collect()
}

pub(crate) async fn create_authenticated_http_transport(
    app: &tauri::AppHandle,
    server: &MCPServer,
    builder: reqwest::ClientBuilder,
) -> Result<(AppHttpTransport, ResponseRedactor, AuthSignal), String> {
    let mut headers: std::collections::HashMap<String, String> = match server.headers.as_deref().filter(|v| !v.trim().is_empty()) {
        Some(raw) => serde_json::from_str(raw).map_err(|_| "Invalid MCP HTTP headers: expected string values")?,
        None => Default::default(),
    };
    let keys: Vec<_> = headers.keys().filter(|key| key.eq_ignore_ascii_case(super::apps::credentials::REFERENCE_HEADER)).cloned().collect();
    if keys.is_empty() {
        // Vault references resolve only for the origin the user confirmed when saving this server.
        let resolved = crate::vault::config_ref::resolve_http_server(app, server).await?;
        let (server, secrets) = match &resolved { Some((server, secrets)) => (server, secrets.as_slice()), None => (server, &[][..]) };
        let (client, url) = build_plain_client(server, builder, secrets)?;
        let (client, signal) = AuthAwareClient::new(client);
        let redactor = crate::vault::executor::redactor_for(secrets.iter().cloned());
        return Ok((StreamableHttpClientTransport::with_client(client, StreamableHttpClientTransportConfig::with_uri(url)), redactor, signal));
    }
    if keys.len() != 1 { return Err("连接包含重复凭据引用".into()); }
    let reference = headers.remove(&keys[0]).unwrap();
    if !server.is_enabled { return Err(format!("连接 {} 已停用", server.id)); }
    let secrets = match super::apps::credentials::resolve(app, &reference, server.url.as_deref().ok_or("连接缺少服务地址")?, server.id).await {
        Ok(secrets) => secrets,
        Err(error) => {
            // Errors here come from AIPP's own vault/token handling, never from remote tool output.
            super::apps::record_credential_error(app, server, &error);
            return Err(error);
        }
    };
    let secret_names = super::apps::secret_header_names(app, server.id);
    let redactor = ResponseRedactor::new(secret_header_values(&secrets, secret_names.as_ref()).into_iter());
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
    let (client, signal) = AuthAwareClient::new(client);
    Ok((StreamableHttpClientTransport::with_client(client,
        StreamableHttpClientTransportConfig::with_uri(server.url.as_deref().ok_or("连接缺少服务地址")?)), redactor, signal))
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

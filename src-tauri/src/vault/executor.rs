//! HTTP sender for `vault_http_request`.
//!
//! By the time this runs, `placeholder::prepare` has already asked the user and substituted the
//! values into the arguments, and `execute_tool_by_transport` will redact the result. This module
//! only sends: it encodes `query` and `basic_auth` (the assistant cannot encode a value it cannot
//! see), never follows redirects, and drops `Set-Cookie`.
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE as BASE64_URL};
use base64::Engine;
use crate::mcp::http_transport::ResponseRedactor;
use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_BODY_CHARS: usize = 60_000;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];

#[derive(Debug, Deserialize)]
pub(crate) struct BasicAuth { pub username: String, #[serde(default)] pub password: String }

#[derive(Debug, Deserialize)]
pub(crate) struct RequestArgs {
    #[serde(default = "default_method")]
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub query: BTreeMap<String, Value>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub json: Option<Value>,
    #[serde(default)]
    pub basic_auth: Option<BasicAuth>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}
fn default_method() -> String { "GET".into() }

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecError { pub code: &'static str, pub message: String }

impl ExecError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into() } }
}

/// Validate the arguments and build the request. Values may already be substituted in.
pub(crate) fn build_request(client: &reqwest::Client, args: &RequestArgs) -> Result<reqwest::RequestBuilder, ExecError> {
    let method = args.method.trim().to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return Err(ExecError::new("invalid_request", format!("不支持的请求方法 {method}，可用：{}", METHODS.join("/"))));
    }
    let mut url = reqwest::Url::parse(args.url.trim()).map_err(|_| ExecError::new("invalid_request", "URL 无效"))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(ExecError::new("invalid_request", "只支持 http/https 请求"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ExecError::new("invalid_request", "URL 不能包含用户名或密码，请改用 basic_auth"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(ExecError::new("invalid_request", "URL 缺少主机名"));
    }
    if args.body.is_some() && args.json.is_some() {
        return Err(ExecError::new("invalid_request", "body 与 json 只能二选一"));
    }
    url.set_fragment(None);
    if !args.query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in &args.query {
            let text = match value { Value::String(s) => s.clone(), Value::Null => String::new(), other => other.to_string() };
            pairs.append_pair(name, &text);
        }
    }
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in &args.headers {
        let lower = name.trim().to_ascii_lowercase();
        let header = HeaderName::from_bytes(lower.as_bytes()).map_err(|_| ExecError::new("invalid_request", format!("请求头名称无效: {name}")))?;
        if lower == "host" { return Err(ExecError::new("invalid_request", "不能自定义 Host 请求头")); }
        // A substituted value may contain characters a header cannot carry (e.g. a newline).
        let mut value = HeaderValue::from_str(value).map_err(|_| ExecError::new("invalid_request", format!("请求头 {name} 的值含有不能用于请求头的字符")))?;
        value.set_sensitive(true);
        headers.insert(header, value);
    }
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| ExecError::new("invalid_request", "请求方法无效"))?;
    let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS));
    let mut builder = client.request(method, url).headers(headers).timeout(timeout);
    if let Some(auth) = &args.basic_auth {
        if headers_has_authorization(&args.headers) {
            return Err(ExecError::new("invalid_request", "basic_auth 与 Authorization 请求头只能二选一"));
        }
        builder = builder.basic_auth(&auth.username, Some(&auth.password));
    }
    if let Some(json) = &args.json { builder = builder.json(json); }
    if let Some(body) = &args.body { builder = builder.body(body.clone()); }
    Ok(builder)
}

fn headers_has_authorization(headers: &BTreeMap<String, String>) -> bool {
    headers.keys().any(|name| name.trim().eq_ignore_ascii_case("authorization"))
}

/// Basic encodes the combined username/password, not each value separately. Register exactly
/// the credential material this application builds when either field uses a vault placeholder.
pub(crate) fn basic_auth_secret(original: &Value, substituted: &Value) -> Option<String> {
    let original = original.get("basic_auth")?;
    if !original.to_string().contains(super::placeholder::MARKER) { return None; }
    let auth = substituted.get("basic_auth")?;
    let username = auth.get("username")?.as_str()?;
    let password = auth.get("password").and_then(Value::as_str).unwrap_or_default();
    Some(format!("{username}:{password}"))
}

/// Literal values plus the encodings a service is likely to echo back (base64, URL-encoding; JSON
/// escaping is added by `ResponseRedactor`). Shared by placeholders and configuration references.
pub(crate) fn redactor_for(secrets: impl IntoIterator<Item = String>) -> crate::mcp::http_transport::ResponseRedactor {
    let mut values = Vec::new();
    for value in secrets {
        values.push(BASE64.encode(&value));
        values.push(BASE64_URL.encode(&value));
        values.push(BASE64_URL.encode(&value).trim_end_matches('=').to_string());
        values.push(BASE64.encode(&value).trim_end_matches('=').to_string());
        let encoded = urlencoding::encode(&value).into_owned();
        if encoded != value { values.push(encoded); }
        // reqwest/Url query_pairs_mut uses form encoding: spaces become '+', not '%20'.
        let mut query = reqwest::Url::parse("https://redaction.invalid/").expect("static URL");
        query.query_pairs_mut().append_pair("", &value);
        values.push(query.query().unwrap_or_default().trim_start_matches('=').to_owned());
        values.push(value);
    }
    values.retain(|v| v.len() >= 4);
    crate::mcp::http_transport::ResponseRedactor::new(values.into_iter())
}

pub(crate) struct Output { pub status: u16, pub headers: BTreeMap<String, String>, pub body: String, pub truncated: bool }

fn is_text(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.is_empty() || ct.starts_with("text/") || ["json", "xml", "javascript", "x-www-form-urlencoded", "yaml", "graphql"].iter().any(|t| ct.contains(t))
}

/// Clean the bounded text before the character cap. Drop an incomplete final UTF-8 code point
/// at a transport cutoff even if earlier bytes were malformed; replacement would break matching.
fn clean_body(bytes: &[u8], truncated: bool, redactor: &ResponseRedactor) -> String {
    let end = if truncated {
        (bytes.len().saturating_sub(3)..bytes.len()).find(|&start| {
            std::str::from_utf8(&bytes[start..]).err()
                .is_some_and(|error| error.valid_up_to() == 0 && error.error_len().is_none())
        }).unwrap_or(bytes.len())
    } else { bytes.len() };
    let text = String::from_utf8_lossy(&bytes[..end]);
    if truncated { redactor.clean_truncated(&text) } else { redactor.clean(&text) }
}

/// Send and read a bounded body.
pub(crate) async fn send(request: reqwest::RequestBuilder, redactor: &ResponseRedactor) -> Result<Output, ExecError> {
    let mut response = request.send().await.map_err(|e| {
        let kind = if e.is_timeout() { "请求超时" } else if e.is_connect() { "无法连接目标服务" } else { "请求失败" };
        ExecError::new("upstream_error", format!("{kind}: {}", e.without_url()))
    })?;
    let status = response.status().as_u16();
    let mut headers = BTreeMap::new();
    for (name, value) in response.headers() {
        if name == reqwest::header::SET_COOKIE { continue; }
        let text = redactor.clean(&String::from_utf8_lossy(value.as_bytes()));
        headers.entry(name.as_str().to_string()).and_modify(|v: &mut String| { v.push_str(", "); v.push_str(&text); }).or_insert(text);
    }
    let content_type = headers.get("content-type").cloned().unwrap_or_default();
    let mut bytes = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await.map_err(|e| ExecError::new("upstream_error", format!("读取响应失败: {}", e.without_url())))? {
        if bytes.len() + chunk.len() > MAX_BODY_BYTES {
            bytes.extend_from_slice(&chunk[..MAX_BODY_BYTES - bytes.len()]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = if is_text(&content_type) || std::str::from_utf8(&bytes).is_ok() {
        let text = clean_body(&bytes, truncated, redactor);
        if text.chars().count() > MAX_BODY_CHARS {
            truncated = true;
            text.chars().take(MAX_BODY_CHARS).collect()
        } else { text }
    } else {
        format!("[二进制内容 {} 字节，content-type: {content_type}，未返回]", bytes.len())
    };
    Ok(Output { status, headers, body, truncated })
}

fn client(proxy: Option<&str>) -> Result<reqwest::Client, ExecError> {
    let mut builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| ExecError::new("vault_error", "全局网络代理地址格式无效"))?);
    }
    builder.build().map_err(|e| ExecError::new("vault_error", format!("无法创建 HTTP 客户端: {e}")))
}

async fn global_proxy(app: &AppHandle) -> Option<String> {
    let state = app.try_state::<crate::FeatureConfigState>()?;
    let config = state.config_feature_map.lock().await;
    crate::api::ai::config::get_network_proxy_from_config(&config).filter(|p| !p.trim().is_empty())
}

/// Entry point for `vault_http_request`. Clean raw output before truncation and JSON wrappers.
pub(crate) async fn run(app: &AppHandle, args: &Value) -> Result<Value, ExecError> {
    let redactor = super::placeholder::current_redactor().unwrap_or_default();
    let result = async {
        let args: RequestArgs = serde_json::from_value(args.clone()).map_err(|e| ExecError::new("invalid_request", format!("参数无效: {e}")))?;
        let client = client(global_proxy(app).await.as_deref())?;
        let output = send(build_request(&client, &args)?, &redactor).await?;
        Ok(json!({
            "status": output.status,
            "headers": output.headers,
            "body": output.body,
            "truncated": output.truncated,
        }))
    }.await;
    // Error messages can quote a substituted argument. Clean them before error_json adds wrappers.
    result.map_err(|mut error: ExecError| { error.message = redactor.clean(&error.message); error })
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;

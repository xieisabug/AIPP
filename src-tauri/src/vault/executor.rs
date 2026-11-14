//! Trusted executor for `vault_http_request`.
//!
//! The model names a credential and describes a request; this module injects the real value at the
//! position the user declared, sends the request, and redacts the response. It is the single
//! decision point for every entry path (chat pipeline, Butler, native-agent bridge, schedules).
use crate::sync::personal_vault::{self, InjectionRule, Resolved};
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE as BASE64_URL};
use base64::Engine;
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
pub(crate) struct RequestArgs {
    pub credential: String,
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
    pub timeout_secs: Option<u64>,
}
fn default_method() -> String { "GET".into() }

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecError { pub code: &'static str, pub message: String }

impl ExecError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into() } }
    fn outcome(&self) -> &'static str {
        if matches!(self.code, "user_denied" | "site_not_approved" | "approval_timeout") { "denied" } else { "error" }
    }
}

/// What the request targets, before any secret is added.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub method: reqwest::Method,
    /// Agent URL plus agent query parameters. Never contains injected values.
    pub url: reqwest::Url,
    pub origin: String,
}

pub(crate) fn parse_target(args: &RequestArgs) -> Result<Target, ExecError> {
    let method = args.method.trim().to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return Err(ExecError::new("invalid_request", format!("不支持的请求方法 {method}，可用：{}", METHODS.join("/"))));
    }
    let mut url = reqwest::Url::parse(args.url.trim()).map_err(|_| ExecError::new("invalid_request", format!("URL 无效: {}", args.url)))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err(ExecError::new("invalid_request", "只支持 http/https 请求"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ExecError::new("invalid_request", "URL 不能包含用户名或密码；认证由保险箱注入"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(ExecError::new("invalid_request", "URL 缺少主机名"));
    }
    url.set_fragment(None);
    if !args.query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in &args.query {
            let text = match value { Value::String(s) => s.clone(), Value::Null => String::new(), other => other.to_string() };
            pairs.append_pair(name, &text);
        }
    }
    let origin = url.origin().ascii_serialization();
    let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| ExecError::new("invalid_request", "请求方法无效"))?;
    Ok(Target { method, url, origin })
}

/// The assistant may add ordinary headers and parameters, but never ones the credential injects.
pub(crate) fn check_agent_inputs(args: &RequestArgs, resolved: &Resolved, target: &Target) -> Result<(), ExecError> {
    if args.body.is_some() && args.json.is_some() {
        return Err(ExecError::new("invalid_request", "body 与 json 只能二选一"));
    }
    let injected: Vec<String> = resolved.rules.iter().filter_map(|r| r.header_name().map(str::to_ascii_lowercase)).collect();
    for (name, value) in &args.headers {
        let lower = name.trim().to_ascii_lowercase();
        if HeaderName::from_bytes(lower.as_bytes()).is_err() || HeaderValue::from_str(value).is_err() {
            return Err(ExecError::new("invalid_request", format!("请求头无效: {name}")));
        }
        if injected.contains(&lower) || lower == "host" {
            return Err(ExecError::new("header_not_allowed", format!("请求头 {name} 由保险箱注入或不允许自定义，请去掉它")));
        }
    }
    let injected_params: Vec<&str> = resolved.rules.iter().filter_map(|r| match r { InjectionRule::Query { name, .. } => Some(name.as_str()), _ => None }).collect();
    if target.url.query_pairs().any(|(name, _)| injected_params.contains(&name.as_ref())) {
        return Err(ExecError::new("header_not_allowed", "查询参数与保险箱注入的参数同名，请去掉它"));
    }
    Ok(())
}

fn sensitive(value: &str) -> Result<HeaderValue, ExecError> {
    let mut value = HeaderValue::from_str(value).map_err(|_| ExecError::new("vault_error", "凭据值不能作为请求头（含非法字符），请在保险箱中替换"))?;
    value.set_sensitive(true);
    Ok(value)
}

/// Build the outgoing request with the secret injected where the user declared it.
pub(crate) fn build_request(client: &reqwest::Client, target: &Target, args: &RequestArgs, resolved: &Resolved, timeout: Duration) -> Result<reqwest::RequestBuilder, ExecError> {
    let mut url = target.url.clone();
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in &args.headers {
        let name = HeaderName::from_bytes(name.trim().to_ascii_lowercase().as_bytes()).map_err(|_| ExecError::new("invalid_request", "请求头无效"))?;
        headers.insert(name, HeaderValue::from_str(value).map_err(|_| ExecError::new("invalid_request", "请求头值无效"))?);
    }
    let field = |name: &str| resolved.fields.get(name).ok_or_else(|| ExecError::new("vault_error", format!("凭据缺少字段 {name}，请在保险箱中替换")));
    for rule in &resolved.rules {
        match rule {
            InjectionRule::Header { name, prefix, field: key } => {
                let name = HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes()).map_err(|_| ExecError::new("vault_error", "注入的请求头名称无效"))?;
                headers.insert(name, sensitive(&format!("{prefix}{}", field(key)?))?);
            }
            InjectionRule::Basic => {
                let pair = resolved.basic_pair().ok_or_else(|| ExecError::new("vault_error", "Basic 凭据缺少用户名或密码"))?;
                headers.insert(reqwest::header::AUTHORIZATION, sensitive(&format!("Basic {}", BASE64.encode(pair)))?);
            }
            InjectionRule::Query { name, field: key } => { url.query_pairs_mut().append_pair(name, field(key)?); }
        }
    }
    let mut builder = client.request(target.method.clone(), url).headers(headers).timeout(timeout);
    if let Some(json) = &args.json { builder = builder.json(json); }
    if let Some(body) = &args.body { builder = builder.body(body.clone()); }
    Ok(builder)
}

/// Literal values plus the encodings a service is likely to echo back (base64, URL-encoding, JSON escape).
pub(crate) fn redactor(resolved: &Resolved) -> crate::mcp::http_transport::ResponseRedactor {
    redactor_for(resolved.secret_values())
}

/// Same encodings for any set of secret values (also used for configuration references).
pub(crate) fn redactor_for(secrets: impl IntoIterator<Item = String>) -> crate::mcp::http_transport::ResponseRedactor {
    let mut values = Vec::new();
    for value in secrets {
        values.push(BASE64.encode(&value));
        values.push(BASE64_URL.encode(&value));
        values.push(BASE64.encode(&value).trim_end_matches('=').to_string());
        let encoded = urlencoding::encode(&value).into_owned();
        if encoded != value { values.push(encoded); }
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

/// Send, read a bounded body, and redact everything that goes back to the model.
pub(crate) async fn send(request: reqwest::RequestBuilder, resolved: &Resolved) -> Result<Output, ExecError> {
    let redactor = redactor(resolved);
    let mut response = request.send().await.map_err(|e| {
        let kind = if e.is_timeout() { "请求超时" } else if e.is_connect() { "无法连接目标服务" } else { "请求失败" };
        ExecError::new("upstream_error", redactor.clean(&format!("{kind}: {}", e.without_url())))
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
    while let Some(chunk) = response.chunk().await.map_err(|e| ExecError::new("upstream_error", redactor.clean(&format!("读取响应失败: {}", e.without_url()))))? {
        if bytes.len() + chunk.len() > MAX_BODY_BYTES {
            bytes.extend_from_slice(&chunk[..MAX_BODY_BYTES - bytes.len()]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = if is_text(&content_type) || std::str::from_utf8(&bytes).is_ok() {
        let text = redactor.clean(&String::from_utf8_lossy(&bytes));
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

struct Caller { assistant_id: Option<i64>, assistant_name: Option<String>, attended: bool }

fn caller(app: &AppHandle, conversation_id: Option<i64>) -> Caller {
    use crate::db::conversation_db::Repository;
    let conversation = conversation_id.and_then(|id| crate::db::conversation_db::ConversationDatabase::new(app).ok()
        .and_then(|db| db.conversation_repo().ok()).and_then(|repo| repo.read(id).ok().flatten()));
    let assistant_id = conversation.as_ref().and_then(|c| c.assistant_id);
    let assistant_name = assistant_id.and_then(|id| crate::db::assistant_db::AssistantDatabase::new(app).ok()
        .and_then(|db| db.get_assistant(id).ok()).map(|a| a.name));
    // Scheduled runs have nobody watching: they can only use sites the user already approved.
    let attended = conversation.as_ref().is_some_and(|c| c.conversation_kind != crate::db::message_preview::SCHEDULED_RUN_KIND);
    Caller { assistant_id, assistant_name, attended }
}

fn log_path(target: &Target) -> String { format!("{}{}", target.origin, target.url.path()) }

/// Entry point for the `vault_http_request` tool. Returns the JSON shown to the model.
pub(crate) async fn run(app: &AppHandle, conversation_id: Option<i64>, args: &Value) -> Result<Value, ExecError> {
    let args: RequestArgs = serde_json::from_value(args.clone()).map_err(|e| ExecError::new("invalid_request", format!("参数无效: {e}")))?;
    let handle = args.credential.trim().to_string();
    let lookup_app = app.clone();
    let lookup = handle.clone();
    let resolved = tauri::async_runtime::spawn_blocking(move || personal_vault::resolve(&lookup_app, &lookup)).await
        .map_err(|e| ExecError::new("vault_error", format!("读取保险箱失败: {e}")))?
        .map_err(|e| ExecError::new(e.code, e.message))?;
    let target = parse_target(&args)?;
    let caller = caller(app, conversation_id);
    let outcome = execute(app, conversation_id, &caller, &args, &resolved, &target).await;
    let (status, outcome_code, reason) = match &outcome {
        Ok(output) => (Some(output.status as i64), "ok", None),
        Err(error) => (None, error.outcome(), Some(format!("{}: {}", error.code, error.message))),
    };
    personal_vault::log_usage(app, &personal_vault::NewUsage {
        item_id: &resolved.meta.id, conversation_id, assistant_id: caller.assistant_id,
        method: target.method.as_str(), url: &log_path(&target), status, outcome: outcome_code, reason: reason.as_deref(),
    });
    let output = outcome?;
    Ok(json!({
        "credential": resolved.meta.handle,
        "method": target.method.as_str(),
        "url": target.url.as_str(),
        "status": output.status,
        "headers": output.headers,
        "body": output.body,
        "truncated": output.truncated,
    }))
}

async fn execute(app: &AppHandle, conversation_id: Option<i64>, caller: &Caller, args: &RequestArgs, resolved: &Resolved, target: &Target) -> Result<Output, ExecError> {
    check_agent_inputs(args, resolved, target)?;
    if !resolved.meta.allowed_sites.contains(&target.origin) {
        if !caller.attended {
            return Err(ExecError::new("site_not_approved", format!(
                "凭据 {} 还没有被批准用于 {}；无人值守的任务不能询问用户，请用户先在对话中批准或在保险箱中添加该网站", resolved.meta.handle, target.origin)));
        }
        let request = super::approval::VaultUseRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            conversation_id,
            assistant_name: caller.assistant_name.clone(),
            credential_id: resolved.meta.id.clone(),
            credential_name: resolved.meta.name.clone(),
            credential_handle: resolved.meta.handle.clone(),
            method: target.method.to_string(),
            url: target.url.to_string(),
            origin: target.origin.clone(),
            insecure: target.url.scheme() == "http",
            header_names: args.headers.keys().cloned().collect(),
            has_body: args.body.is_some() || args.json.is_some(),
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        match super::approval::ask(app, request).await.map_err(|(code, message)| ExecError::new(code, message))? {
            super::approval::Decision::Deny => return Err(ExecError::new("user_denied", format!("用户拒绝了用凭据 {} 访问 {}", resolved.meta.handle, target.origin))),
            super::approval::Decision::Once => {}
            super::approval::Decision::Always => {
                let (approve_app, id, origin) = (app.clone(), resolved.meta.id.clone(), target.origin.clone());
                tauri::async_runtime::spawn_blocking(move || personal_vault::approve_site(&approve_app, &id, &origin)).await
                    .map_err(|e| ExecError::new("vault_error", e.to_string()))?
                    .map_err(|e| ExecError::new("vault_error", format!("记录已批准的网站失败: {e}")))?;
                crate::api::vault_api::notify_changed(app);
            }
        }
    }
    let timeout = Duration::from_secs(args.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS).clamp(1, MAX_TIMEOUT_SECS));
    let client = client(global_proxy(app).await.as_deref())?;
    send(build_request(&client, target, args, resolved, timeout)?, resolved).await
}

#[cfg(test)]
#[path = "executor_tests.rs"]
mod tests;

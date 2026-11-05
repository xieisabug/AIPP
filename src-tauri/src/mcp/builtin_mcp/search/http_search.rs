//! Explicit HTTP search providers. No browser, provider fallback, or automatic retry.
use std::{collections::HashMap, time::Duration};
use futures::StreamExt;
use reqwest::{header::HeaderValue, Url};
use serde_json::{json, Value};
use super::types::SearchItem;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub struct HttpSearchConfig {
    provider: String,
    endpoint: Url,
    key: String,
    timeout: Duration,
}

impl HttpSearchConfig {
    pub fn from_env(env: &HashMap<String, String>) -> Result<Self, String> {
        let provider = env.get("HTTP_SEARCH_PROVIDER").map(|s| s.trim()).unwrap_or("");
        let default_endpoint = match provider {
            "tavily" => "https://api.tavily.com/search",
            "brave" => "https://api.search.brave.com/res/v1/web/search",
            "searxng" => "",
            "" | "disabled" => return Err("search_http: stage=config; 尚未配置 HTTP 搜索服务，请在搜索工具设置中选择 Tavily、Brave 或 SearXNG".into()),
            _ => return Err("search_http: stage=config; 未支持的 HTTP 搜索服务，未切换其他提供商".into()),
        };
        let raw = env.get("HTTP_SEARCH_ENDPOINT").map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or(default_endpoint);
        let endpoint = Url::parse(raw).map_err(|_| "search_http: stage=config; 搜索地址无效，请填写完整 HTTP/HTTPS API 地址（SearXNG 使用 /search）".to_string())?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none()
            || !endpoint.username().is_empty() || endpoint.password().is_some() || endpoint.fragment().is_some() || endpoint.query().is_some() {
            return Err("search_http: stage=config; 地址仅支持 HTTP/HTTPS，不允许内嵌账号、密码、查询参数或片段".into());
        }
        let key = env.get("HTTP_SEARCH_API_KEY").map(|s| s.trim()).unwrap_or("").to_string();
        if provider != "searxng" && key.is_empty() {
            return Err(format!("search_http: provider={provider}; stage=config; 未配置 API Key"));
        }
        let timeout = env.get("HTTP_SEARCH_TIMEOUT_MS").filter(|s| !s.trim().is_empty())
            .map(|s| s.parse::<u64>()).transpose().map_err(|_| "search_http: stage=config; 超时必须为 1000–60000 毫秒".to_string())?.unwrap_or(30_000);
        if !(1000..=60_000).contains(&timeout) { return Err("search_http: stage=config; 超时必须为 1000–60000 毫秒".into()); }
        Ok(Self { provider: provider.into(), endpoint, key, timeout: Duration::from_millis(timeout) })
    }

    fn error(&self, stage: &str, reason: impl std::fmt::Display) -> String {
        let text = format!("search_http: provider={}; host={}; stage={stage}; {reason}", self.provider, self.endpoint.host_str().unwrap_or(""));
        if self.key.is_empty() { text } else { text.replace(&self.key, "[redacted]") }
    }
}

fn parse_items(provider: &str, value: &Value, limit: usize) -> Result<Vec<SearchItem>, String> {
    // An explicit empty array is a successful search; a missing array is a protocol error.
    let rows = if provider == "brave" { value.pointer("/web/results") } else { value.get("results") }
        .and_then(Value::as_array).ok_or("响应缺少搜索结果数组，不能将错误响应当作空结果")?;
    rows.iter().take(limit).enumerate().map(|(index, row)| {
        let title = row.get("title").and_then(Value::as_str).ok_or("结果缺少 title 字符串")?;
        let url = row.get("url").and_then(Value::as_str).ok_or("结果缺少 url 字符串")?;
        let parsed = Url::parse(url).map_err(|_| "结果 URL 无效")?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none()
            || !parsed.username().is_empty() || parsed.password().is_some() { return Err("结果 URL 不是可访问的 HTTP/HTTPS 地址".into()); }
        let field = if provider == "brave" { "description" } else { "content" };
        let snippet = match row.get(field) { None | Some(Value::Null) => "", Some(Value::String(text)) => text, _ => return Err("结果摘要不是字符串".into()) };
        Ok(SearchItem { title: title.into(), url: url.into(), snippet: snippet.into(), rank: index + 1, display_url: None })
    }).collect()
}

pub async fn search(config: &HttpSearchConfig, query: &str, limit: usize) -> Result<Value, String> {
    if query.trim().is_empty() || query.chars().count() > 600 || !(1..=20).contains(&limit) {
        return Err(config.error("validate", "关键词须为 1–600 字符，结果数量须为 1–20"));
    }
    let client = reqwest::Client::builder().timeout(config.timeout)
        .redirect(reqwest::redirect::Policy::none()).build()
        .map_err(|e| config.error("client", e.without_url()))?;
    let mut endpoint = config.endpoint.clone();
    if config.provider == "brave" {
        endpoint.query_pairs_mut().append_pair("q", query).append_pair("count", &limit.to_string()).append_pair("text_decorations", "false");
    } else if config.provider == "searxng" {
        endpoint.query_pairs_mut().append_pair("q", query).append_pair("format", "json");
    }
    let mut request = match config.provider.as_str() {
        "tavily" => client.post(config.endpoint.clone()).json(&json!({"query": query, "max_results": limit, "search_depth": "basic", "include_answer": false, "include_raw_content": false, "auto_parameters": false})),
        _ => client.get(endpoint),
    }.header("Accept", "application/json");
    if !config.key.is_empty() {
        let raw = if config.provider == "brave" { config.key.clone() } else { format!("Bearer {}", config.key) };
        let mut header = HeaderValue::from_str(&raw).map_err(|_| config.error("config", "API Key 包含非法 HTTP 头字符"))?;
        header.set_sensitive(true);
        request = request.header(if config.provider == "brave" { "X-Subscription-Token" } else { "Authorization" }, header);
    }
    let response = request.send().await.map_err(|e| config.error("request", format!("{:#}", anyhow::Error::new(e.without_url()))))?;
    let status = response.status();
    let content_type = response.headers().get("content-type").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    if status.is_redirection() { return Err(config.error("response", format!("HTTP {status}; 拒绝重定向，请直接配置最终 API 地址"))); }
    if response.content_length().is_some_and(|length| length > MAX_RESPONSE_BYTES as u64) {
        return Err(config.error("response", format!("HTTP {status}; 响应超过 2 MiB，未返回截断结果")));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| config.error("body", e.without_url()))?;
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES { return Err(config.error("body", "响应超过 2 MiB，未返回截断结果")); }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let reason = String::from_utf8_lossy(&bytes).chars().take(1000).collect::<String>();
        return Err(config.error("response", format!("HTTP {status}; {}", if reason.trim().is_empty() { "服务未返回错误正文" } else { &reason })));
    }
    if !content_type.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json") {
        return Err(config.error("decode", "服务未返回 application/json；SearXNG 实例须开启 JSON 格式"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| config.error("decode", e))?;
    if value.get("error").is_some() { return Err(config.error("provider", value["error"].to_string().chars().take(1000).collect::<String>())); }
    let items = parse_items(&config.provider, &value, limit).map_err(|e| config.error("decode", e))?;
    let engine_errors = value.get("unresponsive_engines").and_then(Value::as_array).filter(|errors| !errors.is_empty());
    if config.provider == "searxng" && items.is_empty() && engine_errors.is_some() {
        return Err(config.error("provider", format!("搜索引擎未正常响应: {}", value["unresponsive_engines"].to_string().chars().take(1000).collect::<String>())));
    }
    Ok(json!({"provider": config.provider, "query": query, "items": items, "partial": config.provider == "searxng" && engine_errors.is_some(), "message": if items.is_empty() { "搜索完成，没有匹配结果" } else { "搜索完成" }}))
}

#[cfg(test)]
#[path = "http_search_tests.rs"]
mod tests;

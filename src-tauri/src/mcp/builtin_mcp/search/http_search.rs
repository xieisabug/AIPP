//! Explicit HTTP search providers. No browser, provider fallback, or automatic retry.
use std::{collections::HashMap, time::Duration};
use futures::StreamExt;
use reqwest::{header::HeaderValue, Url};
use serde_json::{json, Value};
use super::types::SearchItem;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FETCH_BYTES: usize = 8 * 1024 * 1024;
pub const DEEPSEEK_SEARCH_ENDPOINT: &str = "https://api.deepseek.com/v1/tools/search";
/// DeepSeek Search accepts at most 128 Unicode characters per query.
const DEEPSEEK_MAX_QUERY_CHARS: usize = 128;

pub struct HttpSearchConfig {
    provider: String,
    endpoint: Url,
    key: String,
    timeout: Duration,
}

/// The search service (`HTTP_SEARCH_PROVIDER`) is the single choice for both search tools: `local`
/// (browser search, desktop only) or an HTTP API. Unset and the legacy `disabled` mean local.
pub fn uses_local_search(env: &HashMap<String, String>) -> bool {
    matches!(env.get("HTTP_SEARCH_PROVIDER").map(|s| s.trim()).unwrap_or(""), "" | "disabled" | "local")
}

/// The configured HTTP search provider and the exact URL its requests go to. Shared with the vault
/// reference check (`vault::config_ref`), so the confirmed destination is the one actually called.
pub fn configured_endpoint(env: &HashMap<String, String>) -> Result<(String, Url), String> {
    let provider = env.get("HTTP_SEARCH_PROVIDER").map(|s| s.trim()).unwrap_or("");
    let default_endpoint = match provider {
        "tavily" => "https://api.tavily.com/search",
        "brave" => "https://api.search.brave.com/res/v1/web/search",
        "deepseek" => DEEPSEEK_SEARCH_ENDPOINT,
        "searxng" => "",
        "" | "disabled" | "local" => return Err("search_http: stage=config; 当前搜索服务是本地搜索（浏览器），没有 HTTP 搜索地址；需要 HTTP 搜索请在搜索工具设置中选择 DeepSeek、Tavily、Brave 或 SearXNG".into()),
        _ => return Err("search_http: stage=config; 未支持的 HTTP 搜索服务，未切换其他提供商".into()),
    };
    let raw = env.get("HTTP_SEARCH_ENDPOINT").map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or(default_endpoint);
    let endpoint = Url::parse(raw).map_err(|_| "search_http: stage=config; 搜索地址无效，请填写完整 HTTP/HTTPS API 地址（SearXNG 使用 /search）".to_string())?;
    if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none()
        || !endpoint.username().is_empty() || endpoint.password().is_some() || endpoint.fragment().is_some() || endpoint.query().is_some() {
        return Err("search_http: stage=config; 地址仅支持 HTTP/HTTPS，不允许内嵌账号、密码、查询参数或片段".into());
    }
    Ok((provider.to_string(), endpoint))
}

/// `fetch_url` goes through DeepSeek Fetch instead of the local browser/HTTP fetcher. Only while the
/// search service is DeepSeek too: the settings page hides the option otherwise, so a leftover
/// `FETCH_PROVIDER=deepseek` must not take effect.
pub fn uses_deepseek_fetch(env: &HashMap<String, String>) -> bool {
    env.get("FETCH_PROVIDER").map(|s| s.trim()) == Some("deepseek")
        && env.get("HTTP_SEARCH_PROVIDER").map(|s| s.trim()) == Some("deepseek")
}

/// HTTP search results as Markdown, for `search_web` when the search service is an HTTP API.
pub fn items_to_markdown(query: &str, provider: &str, items: &[SearchItem]) -> String {
    if items.is_empty() {
        return format!("# 搜索：{query}\n\n（{provider} 没有返回匹配结果，可以换个关键词再试）\n");
    }
    let mut out = format!("# 搜索：{query}\n\n来源：{provider}\n");
    for item in items {
        out.push_str(&format!("\n## {}. [{}]({})\n", item.rank, item.title.replace(['[', ']'], ""), item.url));
        if !item.snippet.trim().is_empty() {
            out.push_str(&format!("{}\n", item.snippet.trim()));
        }
    }
    out
}

impl HttpSearchConfig {
    pub fn from_env(env: &HashMap<String, String>) -> Result<Self, String> {
        let (provider, endpoint) = configured_endpoint(env)?;
        let provider = provider.as_str();
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
        redact_key(&text, &self.key)
    }
}

/// Remove the key and the masked tail some providers echo back (DeepSeek 401: `Your api key: ****ef0f`).
/// Tool results and logs must not carry any fragment of a credential, including such tails.
fn redact_key(text: &str, key: &str) -> String {
    if key.is_empty() { return text.to_string(); }
    let mut out = text.replace(key, "[redacted]");
    let chars: Vec<char> = key.chars().collect();
    for len in [6usize, 4] {
        if chars.len() > len {
            let tail: String = chars[chars.len() - len..].iter().collect();
            out = out.replace(&format!("*{tail}"), "*[redacted]");
        }
    }
    out
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
        // DeepSeek: `summary` is the long, query-focused abstract meant for models; `snippet` is the short one.
        let fields: &[&str] = match provider { "brave" => &["description"], "deepseek" => &["summary", "snippet"], _ => &["content"] };
        let mut snippet = "";
        for field in fields {
            match row.get(*field) {
                None | Some(Value::Null) => {}
                Some(Value::String(text)) if text.trim().is_empty() => {}
                Some(Value::String(text)) => { snippet = text; break; }
                _ => return Err("结果摘要不是字符串".into()),
            }
        }
        Ok(SearchItem { title: title.into(), url: url.into(), snippet: snippet.into(), rank: index + 1, display_url: None })
    }).collect()
}

fn client(config: &HttpSearchConfig) -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(config.timeout)
        .redirect(reqwest::redirect::Policy::none()).build()
        .map_err(|e| config.error("client", e.without_url()))
}

fn auth_header(config: &HttpSearchConfig) -> Result<Option<(&'static str, HeaderValue)>, String> {
    if config.key.is_empty() { return Ok(None); }
    let raw = if config.provider == "brave" { config.key.clone() } else { format!("Bearer {}", config.key) };
    let mut header = HeaderValue::from_str(&raw).map_err(|_| config.error("config", "API Key 包含非法 HTTP 头字符"))?;
    header.set_sensitive(true);
    Ok(Some((if config.provider == "brave" { "X-Subscription-Token" } else { "Authorization" }, header)))
}

/// Bounded JSON body. Redirects are refused (credentials stay on the configured host), error
/// statuses keep the provider's reason, and non-JSON is never treated as an empty result.
async fn read_json(config: &HttpSearchConfig, response: reqwest::Response, limit: usize, non_json: &str) -> Result<Value, String> {
    let status = response.status();
    let content_type = response.headers().get("content-type").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    let mib = limit / (1024 * 1024);
    if status.is_redirection() { return Err(config.error("response", format!("HTTP {status}; 拒绝重定向，请直接配置最终 API 地址"))); }
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(config.error("response", format!("HTTP {status}; 响应超过 {mib} MiB，未返回截断结果")));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| config.error("body", e.without_url()))?;
        if bytes.len() + chunk.len() > limit { return Err(config.error("body", format!("响应超过 {mib} MiB，未返回截断结果"))); }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let reason = String::from_utf8_lossy(&bytes).chars().take(1000).collect::<String>();
        return Err(config.error("response", format!("HTTP {status}; {}", if reason.trim().is_empty() { "服务未返回错误正文" } else { &reason })));
    }
    if !content_type.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json") {
        return Err(config.error("decode", non_json));
    }
    serde_json::from_slice(&bytes).map_err(|e| config.error("decode", e))
}

/// DeepSeek Fetch: the page as Markdown, rendered by DeepSeek (no local browser). Same host and key
/// as the configured DeepSeek search; the endpoint is the sibling `fetch` path of the search URL.
pub async fn fetch_deepseek(config: &HttpSearchConfig, url: &str) -> Result<String, String> {
    if config.provider != "deepseek" {
        return Err(config.error("config", "fetch_url 选择了 DeepSeek 抓取，但 HTTP 搜索服务不是 DeepSeek；请在搜索工具设置中把 HTTP 搜索服务也设为 DeepSeek"));
    }
    let target = Url::parse(url).map_err(|_| "fetch_url: URL 无效（仅支持 HTTP/HTTPS）".to_string())?;
    if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() || !target.username().is_empty() || target.password().is_some() {
        return Err("fetch_url: 仅支持有主机名、不含账号密码的 HTTP/HTTPS URL".into());
    }
    let endpoint = config.endpoint.join("fetch").map_err(|_| config.error("config", "无法根据搜索地址推出抓取地址"))?;
    let mut request = client(config)?.post(endpoint).header("Accept", "application/json").json(&json!({"url": url}));
    if let Some((name, header)) = auth_header(config)? { request = request.header(name, header); }
    let response = request.send().await.map_err(|e| config.error("fetch", format!("{:#}", anyhow::Error::new(e.without_url()))))?;
    let value = read_json(config, response, MAX_FETCH_BYTES, "DeepSeek Fetch 未返回 application/json").await?;
    let content = value.get("content").and_then(Value::as_str).ok_or_else(|| config.error("decode", "响应缺少 content 字符串"))?;
    if content.trim().is_empty() { return Err(config.error("fetch", "网页没有可提取的正文")); }
    let title = value.get("title").and_then(Value::as_str).map(str::trim).unwrap_or("");
    Ok(if title.is_empty() || content.trim_start().starts_with('#') { content.to_string() } else { format!("# {title}\n\n{content}") })
}

pub async fn search(config: &HttpSearchConfig, query: &str, limit: usize) -> Result<Value, String> {
    if query.trim().is_empty() || query.chars().count() > 600 || !(1..=20).contains(&limit) {
        return Err(config.error("validate", "关键词须为 1–600 字符，结果数量须为 1–20"));
    }
    if config.provider == "deepseek" && query.chars().count() > DEEPSEEK_MAX_QUERY_CHARS {
        return Err(config.error("validate", "DeepSeek 搜索的关键词最多 128 个字符，请改用更短的关键词（可拆成多次搜索）"));
    }
    let client = client(config)?;
    let mut endpoint = config.endpoint.clone();
    if config.provider == "brave" {
        endpoint.query_pairs_mut().append_pair("q", query).append_pair("count", &limit.to_string()).append_pair("text_decorations", "false");
    } else if config.provider == "searxng" {
        endpoint.query_pairs_mut().append_pair("q", query).append_pair("format", "json");
    }
    let mut request = match config.provider.as_str() {
        "tavily" => client.post(config.endpoint.clone()).json(&json!({"query": query, "max_results": limit, "search_depth": "basic", "include_answer": false, "include_raw_content": false, "auto_parameters": false})),
        "deepseek" => client.post(config.endpoint.clone()).json(&json!({"query": query, "count": limit})),
        _ => client.get(endpoint),
    }.header("Accept", "application/json");
    if let Some((name, header)) = auth_header(config)? { request = request.header(name, header); }
    let response = request.send().await.map_err(|e| config.error("request", format!("{:#}", anyhow::Error::new(e.without_url()))))?;
    let value = read_json(config, response, MAX_RESPONSE_BYTES, "服务未返回 application/json；SearXNG 实例须开启 JSON 格式").await?;
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

//! Bounded HTTP document retrieval. No browser fallback or provider substitution.
use futures::StreamExt;
use reqwest::Url;

const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn validate_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|_| "fetch_url: URL 无效（仅支持 HTTP/HTTPS）".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("fetch_url: 仅支持有主机名的 HTTP/HTTPS URL".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("fetch_url: 不接受 URL 内嵌账号密码，请使用已配置认证的 HTTP MCP".to_string());
    }
    Ok(url)
}

pub(crate) fn decode_document(bytes: &[u8], content_type: &str) -> Result<String, String> {
    let mime = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if !matches!(mime.as_str(), "text/html" | "application/xhtml+xml" | "text/plain" | "text/markdown" | "application/json") {
        return Err(format!("fetch_url: 不支持响应类型 {mime}，仅抓取 HTML、文本、Markdown 或 JSON"));
    }
    let charset = content_type.split(';').skip(1).find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        key.eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches('"'))
    }).unwrap_or("utf-8");
    let encoding = encoding_rs::Encoding::for_label(charset.as_bytes())
        .ok_or_else(|| format!("fetch_url: 未支持的字符编码 {charset}"))?;
    let (text, _, malformed) = encoding.decode(bytes);
    if malformed {
        return Err(format!("fetch_url: 响应正文不符合声明的字符编码 {charset}"));
    }
    if text.trim().is_empty() {
        return Err("fetch_url: 响应正文为空，移动 HTTP 抓取不执行网页 JavaScript".to_string());
    }
    if matches!(mime.as_str(), "text/html" | "application/xhtml+xml")
        && super::engines::base::SearchEngineBase::html_to_markdown(&text).trim().is_empty() {
        return Err("fetch_url: HTML 没有可提取正文；可能需要 JavaScript 渲染，移动端不会启动浏览器补抓".to_string());
    }
    Ok(text.into_owned())
}

pub(crate) async fn fetch_document(raw: &str) -> Result<String, String> {
    let url = validate_url(raw)?;
    let host = url.host_str().unwrap_or_default().to_string();
    let client = reqwest::Client::builder()
        .user_agent("AIPP-Mobile-HTTP/1.0")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 { return attempt.error("fetch_url: 重定向超过 10 次"); }
            if validate_url(attempt.url().as_str()).is_err() { return attempt.error("fetch_url: 重定向目标不允许"); }
            if attempt.previous().last().is_some_and(|url| url.scheme() == "https") && attempt.url().scheme() != "https" {
                return attempt.error("fetch_url: 拒绝 HTTPS 降级重定向");
            }
            attempt.follow()
        }))
        .timeout(std::time::Duration::from_secs(30))
        .build().map_err(|e| format!("fetch_url: 创建 HTTP 客户端失败: {}", e.without_url()))?;
    let response = client.get(url).header("Accept", "text/html,application/xhtml+xml,text/plain,text/markdown,application/json")
        .send().await.map_err(|e| format!("fetch_url: host={host}; 请求失败: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!("fetch_url: host={host}; HTTP {}", response.status().as_u16()));
    }
    if response.content_length().is_some_and(|length| length > MAX_DOCUMENT_BYTES as u64) {
        return Err("fetch_url: 响应超过 4 MiB 上限，未返回截断内容".to_string());
    }
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok()).unwrap_or("").to_string();
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("fetch_url: host={host}; 读取正文失败: {}", e.without_url()))?;
        if bytes.len().saturating_add(chunk.len()) > MAX_DOCUMENT_BYTES {
            return Err("fetch_url: 流式响应超过 4 MiB 上限，未返回截断内容".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    decode_document(&bytes, &content_type)
}

#[cfg(test)]
#[path = "mobile_http_tests.rs"]
mod tests;

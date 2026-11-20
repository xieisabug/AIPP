use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn config(provider: &str, endpoint: &str) -> HttpSearchConfig {
    HttpSearchConfig::from_env(&HashMap::from([
        ("HTTP_SEARCH_PROVIDER".into(), provider.into()),
        ("HTTP_SEARCH_ENDPOINT".into(), endpoint.into()),
        ("HTTP_SEARCH_API_KEY".into(), "test-secret".into()),
    ])).unwrap()
}

async fn server(status: &str, content_type: &str, body: &str, extra: &str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/search", listener.local_addr().unwrap());
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}", body.len());
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 { break; }
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length = headers.lines().find_map(|line| line.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                if request.len() >= end + 4 + length { break; }
            }
        }
        let _ = socket.write_all(response.as_bytes()).await;
        String::from_utf8(request).unwrap()
    });
    (endpoint, handle)
}

#[test]
fn test_http_search_config_rejects_missing_invalid_and_embedded_credentials() {
    assert!(HttpSearchConfig::from_env(&HashMap::new()).is_err());
    for raw in ["bad", "file:///a", "https://u:p@example.com/search", "https://example.com/search?key=secret"] {
        let env = HashMap::from([("HTTP_SEARCH_PROVIDER".into(), "searxng".into()), ("HTTP_SEARCH_ENDPOINT".into(), raw.into())]);
        assert!(HttpSearchConfig::from_env(&env).is_err());
    }
    assert!(HttpSearchConfig::from_env(&HashMap::from([("HTTP_SEARCH_PROVIDER".into(), "tavily".into())])).is_err());
}

#[tokio::test]
async fn test_http_search_provider_requests_and_result_mapping() {
    for (provider, body) in [
        ("tavily", r#"{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","content":"语言"}]}"#),
        ("brave", r#"{"web":{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","description":"语言"}]}}"#),
        ("searxng", r#"{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","content":"语言"}]}"#),
    ] {
        let (endpoint, handle) = server("200 OK", "application/json", body, "").await;
        let result = search(&config(provider, &endpoint), "Rust 中文", 3).await.unwrap();
        assert_eq!(result["items"][0]["snippet"], "语言");
        assert_eq!(result["items"][0]["rank"], 1);
        assert_eq!(result["provider"], provider);
        let request = handle.await.unwrap();
        if provider == "tavily" {
            assert!(request.starts_with("POST /search "));
            let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
            assert_eq!(body["query"], "Rust 中文");
            assert_eq!(body["max_results"], 3);
            assert!(!body["auto_parameters"].as_bool().unwrap());
        } else {
            assert!(request.starts_with("GET /search?q=Rust+%E4%B8%AD%E6%96%87"));
        }
        assert!(request.to_lowercase().contains(if provider == "brave" { "x-subscription-token: test-secret" } else { "authorization: bearer test-secret" }));
    }
}

#[tokio::test]
async fn test_http_search_error_status_preserves_reason_and_redacts_key() {
    for status in ["401 Unauthorized", "402 Payment Required", "429 Too Many Requests"] {
        let (endpoint, handle) = server(status, "application/json", r#"{"error":"quota or key test-secret rejected"}"#, "").await;
        let error = search(&config("tavily", &endpoint), "test", 5).await.unwrap_err();
        assert!(error.contains(status));
        assert!(error.contains("quota or key [redacted] rejected"));
        assert!(!error.contains("test-secret"));
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_http_search_empty_results_and_protocol_errors_are_distinct() {
    for (body, valid) in [(r#"{"results":[]}"#, true), (r#"{"results":[],"unresponsive_engines":[["google","timeout"]]}"#, false), (r#"{"message":"broken"}"#, false), ("not json", false), (r#"{"results":[{"title":"bad","url":"javascript:alert(1)"}]}"#, false)] {
        let (endpoint, handle) = server("200 OK", "application/json", body, "").await;
        let result = search(&config("searxng", &endpoint), "test", 5).await;
        assert_eq!(result.is_ok(), valid);
        if valid { assert_eq!(result.unwrap()["items"], json!([])); }
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_http_search_redirect_does_not_forward_credentials() {
    let (endpoint, handle) = server("302 Found", "application/json", "", "Location: http://127.0.0.1:1/credential-target\r\n").await;
    let error = search(&config("brave", &endpoint), "test", 5).await.unwrap_err();
    assert!(error.contains("拒绝重定向"));
    handle.await.unwrap();
}

#[tokio::test]
async fn test_http_search_enforces_timeout_and_size_limit() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut settings = config("tavily", &format!("http://{}/search", listener.local_addr().unwrap()));
    settings.timeout = Duration::from_millis(40);
    let handle = tokio::spawn(async move { let (_socket, _) = listener.accept().await.unwrap(); tokio::time::sleep(Duration::from_secs(1)).await; });
    let error = search(&settings, "test", 5).await.unwrap_err();
    assert!(error.contains("stage=request"));
    handle.abort();
    let body = "x".repeat(MAX_RESPONSE_BYTES + 1);
    let (endpoint, handle) = server("200 OK", "application/json", &body, "").await;
    assert!(search(&config("brave", &endpoint), "test", 5).await.unwrap_err().contains("2 MiB"));
    handle.await.unwrap();
}

#[tokio::test]
async fn test_deepseek_search_posts_count_and_prefers_long_summary() {
    let body = r#"{"results":[{"rerank_score":2.4,"title":"DeepSeek","url":"https://api-docs.deepseek.com/","snippet":"短","summary":"给模型读的长摘要","content":"","site_name":null,"site_icon":null,"date":null,"authority":3},{"title":"B","url":"https://b.example/","snippet":"只有短摘要","summary":""}]}"#;
    let (endpoint, handle) = server("200 OK", "application/json; charset=utf-8", body, "").await;
    let result = search(&config("deepseek", &endpoint), "DeepSeek 全家桶", 2).await.unwrap();
    assert_eq!(result["provider"], "deepseek");
    assert_eq!(result["items"][0]["snippet"], "给模型读的长摘要");
    assert_eq!(result["items"][1]["snippet"], "只有短摘要");
    let request = handle.await.unwrap();
    assert!(request.starts_with("POST /search "));
    assert!(request.to_lowercase().contains("authorization: bearer test-secret"));
    let sent: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(sent, json!({"query": "DeepSeek 全家桶", "count": 2}));
}

#[tokio::test]
async fn test_deepseek_search_rejects_queries_over_128_chars_before_sending() {
    let error = search(&config("deepseek", "http://127.0.0.1:9/search"), &"字".repeat(129), 5).await.unwrap_err();
    assert!(error.contains("stage=validate"));
    assert!(error.contains("128"));
}

#[test]
fn test_deepseek_default_endpoint_and_fetch_selection() {
    let env = HashMap::from([("HTTP_SEARCH_PROVIDER".into(), "deepseek".into())]);
    assert_eq!(configured_endpoint(&env).unwrap().1.as_str(), DEEPSEEK_SEARCH_ENDPOINT);
    assert!(!uses_deepseek_fetch(&env));
    let both = HashMap::from([("HTTP_SEARCH_PROVIDER".into(), "deepseek".into()), ("FETCH_PROVIDER".into(), "deepseek".into())]);
    assert!(uses_deepseek_fetch(&both));
    // A leftover fetch setting is hidden in Settings once the service changes, so it must not apply.
    let leftover = HashMap::from([("HTTP_SEARCH_PROVIDER".into(), "tavily".into()), ("FETCH_PROVIDER".into(), "deepseek".into())]);
    assert!(!uses_deepseek_fetch(&leftover));
    assert!(!uses_deepseek_fetch(&HashMap::from([("FETCH_PROVIDER".into(), "deepseek".into())])));
}

#[test]
fn test_search_service_local_covers_unset_and_legacy_disabled() {
    for value in [None, Some(""), Some("disabled"), Some("local"), Some(" local ")] {
        let env: HashMap<String, String> = value.map(|v| ("HTTP_SEARCH_PROVIDER".to_string(), v.to_string())).into_iter().collect();
        assert!(uses_local_search(&env), "{value:?}");
        assert!(configured_endpoint(&env).unwrap_err().contains("本地搜索"));
    }
    for value in ["deepseek", "tavily", "brave", "searxng"] {
        assert!(!uses_local_search(&HashMap::from([("HTTP_SEARCH_PROVIDER".into(), value.into())])), "{value}");
    }
}

#[test]
fn test_http_items_render_as_markdown_for_search_web() {
    let items = vec![
        SearchItem { title: "Rust [lang]".into(), url: "https://www.rust-lang.org/".into(), snippet: "系统编程语言".into(), rank: 1, display_url: None },
        SearchItem { title: "Docs".into(), url: "https://doc.rust-lang.org/".into(), snippet: "  ".into(), rank: 2, display_url: None },
    ];
    let text = items_to_markdown("rust", "deepseek", &items);
    assert!(text.starts_with("# 搜索：rust

来源：deepseek
"));
    assert!(text.contains("## 1. [Rust lang](https://www.rust-lang.org/)
系统编程语言
"));
    assert!(text.contains("## 2. [Docs](https://doc.rust-lang.org/)
"));
    assert!(items_to_markdown("x", "tavily", &[]).contains("没有返回匹配结果"));
}

#[tokio::test]
async fn test_deepseek_fetch_uses_sibling_endpoint_and_returns_markdown() {
    let (endpoint, handle) = server("200 OK", "application/json", r#"{"title":"Example","content":"Body text."}"#, "").await;
    let page = fetch_deepseek(&config("deepseek", &endpoint), "https://example.com/a").await.unwrap();
    assert_eq!(page, "# Example\n\nBody text.");
    let request = handle.await.unwrap();
    assert!(request.starts_with("POST /fetch "));
    assert!(request.to_lowercase().contains("authorization: bearer test-secret"));
    let sent: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(sent, json!({"url": "https://example.com/a"}));
}

#[tokio::test]
async fn test_deepseek_fetch_requires_deepseek_search_and_valid_url() {
    let error = fetch_deepseek(&config("tavily", "http://127.0.0.1:9/search"), "https://example.com/").await.unwrap_err();
    assert!(error.contains("HTTP 搜索服务也设为 DeepSeek"));
    for bad in ["file:///etc/passwd", "https://u:p@example.com/", "not a url"] {
        assert!(fetch_deepseek(&config("deepseek", "http://127.0.0.1:9/search"), bad).await.is_err(), "{bad}");
    }
}

#[tokio::test]
async fn test_provider_errors_drop_masked_key_tails() {
    let body = r#"{"error":{"message":"Authentication Fails, Your api key: ****cret is invalid","type":"authentication_error"}}"#;
    let (endpoint, handle) = server("401 Unauthorized", "application/json", body, "").await;
    let error = search(&config("deepseek", &endpoint), "test", 5).await.unwrap_err();
    assert!(error.contains("401"));
    assert!(error.contains("*[redacted]"));
    assert!(!error.contains("cret"), "{error}");
    handle.await.unwrap();
}

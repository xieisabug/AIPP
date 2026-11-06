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

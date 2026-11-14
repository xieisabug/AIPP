use super::http_transport::{create_http_transport, ResponseRedactor};
use crate::db::mcp_db::MCPServer;
use rmcp::{model::ClientInfo, ServiceExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

fn server(url: String, headers: serde_json::Value) -> MCPServer {
    MCPServer {
        id: 1,
        name: "loopback test".into(),
        description: String::new(),
        transport_type: "http".into(),
        command: None,
        environment_variables: None,
        headers: Some(headers.to_string()),
        url: Some(url),
        timeout: Some(3000),
        is_long_running: false,
        is_enabled: true,
        is_builtin: false,
        is_deletable: true,
        proxy_enabled: false,
        created_time: String::new(),
    }
}

async fn assert_initialize_authorization(expected: &'static str) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (send, receive) = tokio::sync::oneshot::channel();
    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0, "connection closed before headers");
            request.extend_from_slice(&chunk[..n]);
            if let Some(index) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..header_end]).unwrap();
        let authorization: Vec<String> = headers.lines().filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization").then(|| value.trim().to_owned())
        }).collect();
        let length: usize = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse().unwrap())
        }).unwrap();
        while request.len() < header_end + length {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0, "connection closed before request body");
            request.extend_from_slice(&chunk[..n]);
        }
        let message: serde_json::Value = serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
        assert_eq!(message["method"], "initialize");
        let response = serde_json::json!({"jsonrpc":"2.0", "id":message["id"], "result":{
            "protocolVersion":message["params"]["protocolVersion"],
            "capabilities":{}, "serverInfo":{"name":"test", "version":"1"}
        }}).to_string();
        let wire = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response);
        stream.write_all(wire.as_bytes()).await.unwrap();
        send.send(authorization).unwrap();
        // Respond to the initialized notification if the client sends it before serve returns.
        if let Ok(Ok((mut stream, _))) = timeout(Duration::from_secs(2), listener.accept()).await {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            let _ = stream.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        }
    });
    let server = server(format!("http://{address}/mcp"), serde_json::json!({"Authorization": expected}));
    let transport = create_http_transport(&server, reqwest::Client::builder().no_proxy()).unwrap();
    let client = timeout(Duration::from_secs(5), ClientInfo::default().serve(transport)).await.expect("initialize timed out").expect("initialize failed");
    let authorization = timeout(Duration::from_secs(2), receive).await.unwrap().unwrap();
    assert_eq!(authorization, vec![expected]);
    let _ = client.cancel().await;
    server_task.abort();
}

/// A configured Bearer header reaches a real initialize request unchanged and exactly once.
#[tokio::test]
async fn test_http_initialize_bearer_authorization_preserved() {
    assert_initialize_authorization("Bearer test-token").await;
}

/// A configured Basic header reaches a real initialize request unchanged and exactly once.
#[tokio::test]
async fn test_http_initialize_basic_authorization_preserved() {
    assert_initialize_authorization("Basic dXNlcjpwYXNz").await;
}

/// Invalid header names and values fail transport creation instead of being discarded.
#[test]
fn test_http_transport_invalid_headers_return_error() {
    for headers in [serde_json::json!({"Bad Header": "value"}), serde_json::json!({"Authorization": "Bearer bad\nvalue"})] {
        let server = server("http://127.0.0.1:1/mcp".into(), headers);
        assert!(create_http_transport(&server, reqwest::Client::builder()).is_err());
    }
    for raw in ["{", "[]", "null", "{\"Authorization\":123}"] {
        let mut server = server("http://127.0.0.1:1/mcp".into(), serde_json::json!({}));
        server.headers = Some(raw.into());
        assert!(create_http_transport(&server, reqwest::Client::builder()).is_err());
    }
}

/// JSON serialization must not make quoted or backslashed header credentials visible.
#[test]
fn test_response_redactor_serialized_content_and_schema() {
    for secret in [r#"synthetic-quote-"-secret"#, r"synthetic-slash-\-secret", r#"synthetic-\"-secret"#] {
        reqwest::header::HeaderValue::from_str(secret).unwrap();
        let redactor = ResponseRedactor::new(vec![secret.to_owned()].into_iter());
        let content = vec![rmcp::model::Content::text(format!("echo {secret}"))];
        let cleaned = redactor.clean(&serde_json::to_string(&content).unwrap());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&cleaned).unwrap(),
            serde_json::json!([{"type": "text", "text": "echo [REDACTED]"}]));

        let schema = serde_json::json!({"type": "object", "properties": {
            "token": {"type": "string", "description": secret, "default": secret}
        }});
        let cleaned = redactor.clean(&serde_json::to_string(&schema).unwrap());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&cleaned).unwrap(),
            serde_json::json!({"type": "object", "properties": {
                "token": {"type": "string", "description": "[REDACTED]", "default": "[REDACTED]"}
            }}));
    }
}

/// Error strings include literal, JSON-escaped and Debug-quoted credential echoes.
#[test]
fn test_response_redactor_formatted_errors_and_auth_schemes() {
    let token = r#"synthetic-\"-token"#;
    for scheme in ["Bearer ", "Basic ", ""] {
        let header = format!("{scheme}{token}");
        let redactor = ResponseRedactor::new(vec![header.clone()].into_iter());
        let payload = serde_json::json!({"message": format!("rejected {header}; token {token}")});
        let error = format!("HTTP tools/list failed: {payload}; raw {header}; debug {token:?}");
        assert_eq!(redactor.clean(&error),
            "HTTP tools/list failed: {\"message\":\"rejected [REDACTED]; token [REDACTED]\"}; raw [REDACTED]; debug \"[REDACTED]\"");
        // serialize_tool_response formats server-reported tool errors with Debug.
        let content = vec![rmcp::model::Content::text(format!("rejected {header}"))];
        let expected = vec![rmcp::model::Content::text("rejected [REDACTED]")];
        assert_eq!(redactor.clean(&format!("工具执行错误: {content:?}")),
            format!("工具执行错误: {expected:?}"));
    }
}

/// Existing literal matching and empty-secret behavior remain unchanged.
#[test]
fn test_response_redactor_plain_and_empty_values() {
    let redactor = ResponseRedactor::new(vec![String::new(), "Bearer synthetic-token".into()].into_iter());
    assert_eq!(redactor.clean("Bearer synthetic-token / synthetic-token / safe"),
        "[REDACTED] / [REDACTED] / safe");
    assert_eq!(ResponseRedactor::default().clean(r#"{"safe":true}"#), r#"{"safe":true}"#);
}

#[test]
fn test_unresolved_vault_reference_is_never_sent() {
    let reference = "Bearer vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b#token";
    let error = create_http_transport(&server("http://127.0.0.1:1/mcp".into(), serde_json::json!({ "Authorization": reference })), reqwest::Client::builder())
        .err().expect("unresolved reference must fail");
    assert!(error.contains("保险箱引用"));
}


#[test]
fn test_response_redactor_stream_preserves_multiline_and_unicode_across_every_boundary() {
    let secret = "first-secret-line\r\n第二行-secret";
    let redactor = std::sync::Arc::new(ResponseRedactor::new([secret.to_owned()].into_iter()));
    let text = format!("safe {secret} end");
    for split in (0..=text.len()).filter(|&index| text.is_char_boundary(index)) {
        let mut stream = redactor.stream();
        let mut output = stream.push(&text[..split]);
        // No prefix of the secret is released while waiting for its next line.
        assert!(!output.contains("first-secret"));
        output.push_str(&stream.push(&text[split..]));
        output.push_str(&stream.finish());
        assert_eq!(output, "safe [REDACTED] end", "split {split}");
    }
}

#[test]
fn test_response_redactor_stream_does_not_release_overlapping_prefix() {
    let redactor = std::sync::Arc::new(ResponseRedactor::new(["abcd".into(), "abcdef".into()].into_iter()));
    let mut stream = redactor.stream();
    assert_eq!(stream.push("before abcd"), "before ");
    assert_eq!(stream.push("ef after"), "[REDACTED] after");
    assert_eq!(stream.finish(), "");
    let mut stream = redactor.stream();
    assert_eq!(stream.push("safe abc"), "safe ");
    assert_eq!(stream.finish_truncated(), "[REDACTED]");
    let mut stream = redactor.stream();
    assert_eq!(stream.push("safe abc"), "safe ");
    assert_eq!(stream.finish(), "abc");
}

#[test]
fn test_response_redactor_cleans_json_before_serialization() {
    let secret = "first\"quoted\\second\nlast-line";
    let redactor = ResponseRedactor::new([secret.to_owned()].into_iter());
    let mut value = serde_json::json!({"nested": [{"value": secret}]});
    redactor.clean_json(&mut value);
    assert_eq!(value["nested"][0]["value"], "[REDACTED]");
    let serialized = serde_json::to_string(&value).unwrap();
    assert!(!serialized.contains("first"));
}

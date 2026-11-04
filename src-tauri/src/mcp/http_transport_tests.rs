use super::http_transport::create_http_transport;
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

//! Authentication failures are detected from the HTTP status on the transport, not from error text.
use super::http_transport::AuthAwareClient;
use rmcp::model::{CallToolRequestParams, ClientInfo};
use rmcp::transport::{streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport};
use rmcp::ServiceExt;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

type Responder = Arc<dyn Fn(&serde_json::Value) -> String + Send + Sync>;

fn wire(status: &str, extra: &str, body: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

/// Minimal streamable-HTTP MCP endpoint: one JSON response per POST, decided by `respond`.
async fn fake_mcp(respond: Responder) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let respond = respond.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 { return; }
                    request.extend_from_slice(&chunk[..n]);
                    if let Some(index) = request.windows(4).position(|w| w == b"\r\n\r\n") { break index + 4; }
                };
                let head = String::from_utf8_lossy(&request[..header_end]).to_string();
                let length: usize = head.lines().find_map(|line| line.split_once(':').filter(|(n, _)| n.eq_ignore_ascii_case("content-length")).map(|(_, v)| v.trim().parse().unwrap())).unwrap_or(0);
                while request.len() < header_end + length {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 { return; }
                    request.extend_from_slice(&chunk[..n]);
                }
                let message: serde_json::Value = serde_json::from_slice(&request[header_end..header_end + length]).unwrap_or_default();
                let _ = stream.write_all(respond(&message).as_bytes()).await;
            });
        }
    });
    (url, task)
}

fn transport(url: &str) -> (StreamableHttpClientTransport<AuthAwareClient>, super::http_transport::AuthSignal) {
    let (client, signal) = AuthAwareClient::new(reqwest::Client::builder().no_proxy().build().unwrap());
    (StreamableHttpClientTransport::with_client(client, StreamableHttpClientTransportConfig::with_uri(url)), signal)
}

/// rmcp reports a 401 only as a closed transport; the old substring match on "401 Unauthorized" therefore
/// never fired for a revoked token. The wrapped client records the rejection itself.
#[tokio::test]
async fn test_http_auth_rejection_is_signalled_although_rmcp_hides_the_status() {
    let (url, server) = fake_mcp(Arc::new(|_| wire("401 Unauthorized", "WWW-Authenticate: Bearer error=\"invalid_token\"\r\n", "{}"))).await;
    let (transport, signal) = transport(&url);
    let error = timeout(Duration::from_secs(5), ClientInfo::default().serve(transport)).await.expect("initialize timed out").expect_err("401 must fail initialize");
    assert!(signal.rejected());
    assert!(!format!("{error:#}").contains("401 Unauthorized"), "rmcp now exposes the status; text matching would work again: {error:#}");
    server.abort();
}

/// A remote tool error that merely mentions 401/invalid_client (e.g. a fetch tool relaying an upstream
/// response) must not look like an authentication failure of this connection.
#[tokio::test]
async fn test_http_tool_error_mentioning_401_is_not_an_auth_rejection() {
    let (url, server) = fake_mcp(Arc::new(|message| {
        match message["method"].as_str() {
            Some("initialize") => wire("200 OK", "", &serde_json::json!({"jsonrpc":"2.0","id":message["id"],"result":{
                "protocolVersion":message["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"t","version":"1"}}}).to_string()),
            Some("tools/call") => wire("200 OK", "", &serde_json::json!({"jsonrpc":"2.0","id":message["id"],
                "error":{"code":-32000,"message":"upstream answered 401 Unauthorized: invalid_client, invalid_grant"}}).to_string()),
            _ => "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        }
    })).await;
    let (transport, signal) = transport(&url);
    let client = timeout(Duration::from_secs(5), ClientInfo::default().serve(transport)).await.unwrap().unwrap();
    let error = timeout(Duration::from_secs(5), client.call_tool(CallToolRequestParams { meta: None, name: "fetch".into(), arguments: None, task: None })).await.unwrap().unwrap_err();
    assert!(error.to_string().contains("401 Unauthorized"));
    assert!(!signal.rejected());
    let _ = client.cancel().await;
    server.abort();
}

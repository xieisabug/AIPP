use super::*;
use crate::sync::personal_vault::{AuthKind, ItemMeta};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "aipp-fake-token-7f3a91";

fn resolved(kind: AuthKind, rules: Vec<InjectionRule>, fields: &[(&str, &str)], sites: &[&str]) -> Resolved {
    Resolved {
        meta: ItemMeta {
            id: "00000000-0000-4000-8000-000000000001".into(), handle: "test_cred".into(), name: "测试凭据".into(),
            description: String::new(), auth_kind: kind, injection: vec![], fields: vec![], allowed_sites: sites.iter().map(|s| s.to_string()).collect(),
            sync_enabled: true, status: "ready", created_at: 0, updated_at: 0, last_used_at: None,
        },
        rules,
        fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        config_bindings: vec![],
    }
}

fn bearer() -> Resolved {
    resolved(AuthKind::Bearer, vec![InjectionRule::Header { name: "Authorization".into(), prefix: "Bearer ".into(), field: "token".into() }], &[("token", TOKEN)], &[])
}

fn args(url: &str) -> RequestArgs {
    RequestArgs { credential: "test_cred".into(), method: "GET".into(), url: url.into(), headers: Default::default(),
        query: Default::default(), body: None, json: None, timeout_secs: None }
}

/// One-shot HTTP server. `respond` gets the raw request and returns (status line, extra headers, body).
async fn serve(respond: impl Fn(&str) -> (String, String, String) + Send + 'static) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = vec![0u8; 16 * 1024];
        let mut raw = Vec::new();
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            raw.extend_from_slice(&buffer[..n]);
            if n == 0 || raw.windows(4).any(|w| w == b"\r\n\r\n") { break; }
        }
        let request = String::from_utf8_lossy(&raw).to_string();
        let (status, headers, body) = respond(&request);
        let response = format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n{headers}\r\n{body}", body.len());
        socket.write_all(response.as_bytes()).await.unwrap();
        request
    });
    (base, handle)
}

fn echo(request: &str) -> (String, String, String) {
    ("200 OK".into(), "content-type: text/plain\r\n".into(), request.to_string())
}

fn test_client() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).no_proxy().build().unwrap()
}

async fn run_against(resolved: &Resolved, args: &RequestArgs) -> Result<Output, ExecError> {
    let target = parse_target(args)?;
    check_agent_inputs(args, resolved, &target)?;
    send(build_request(&test_client(), &target, args, resolved, Duration::from_secs(5))?, resolved).await
}

#[tokio::test]
async fn test_vault_exec_bearer_injected_and_echo_redacted() {
    let (base, server) = serve(echo).await;
    let output = run_against(&bearer(), &args(&format!("{base}/user"))).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen.to_ascii_lowercase().contains(&format!("authorization: bearer {TOKEN}").to_ascii_lowercase()));
    assert_eq!(output.status, 200);
    assert!(!output.body.contains(TOKEN), "echoed token must be redacted");
    assert!(output.body.contains("[REDACTED]"));
}

#[tokio::test]
async fn test_vault_exec_basic_injected_and_base64_redacted() {
    let creds = resolved(AuthKind::Basic, vec![InjectionRule::Basic], &[("username", "me"), ("password", "pw-secret-1")], &[]);
    let (base, server) = serve(echo).await;
    let output = run_against(&creds, &args(&format!("{base}/"))).await.unwrap();
    let seen = server.await.unwrap();
    let encoded = BASE64.encode("me:pw-secret-1");
    assert!(seen.contains(&format!("Basic {encoded}")));
    assert!(!output.body.contains(&encoded));
    assert!(!output.body.contains("pw-secret-1"));
}

#[tokio::test]
async fn test_vault_exec_custom_header_and_query_injection() {
    let creds = resolved(AuthKind::Query,
        vec![InjectionRule::Query { name: "appid".into(), field: "query_0".into() },
             InjectionRule::Header { name: "X-Api-Key".into(), prefix: String::new(), field: "header_0".into() }],
        &[("query_0", "q-secret-value"), ("header_0", "h-secret-value")], &[]);
    let (base, server) = serve(echo).await;
    let mut request = args(&format!("{base}/weather"));
    request.query.insert("city".into(), Value::String("Beijing".into()));
    let output = run_against(&creds, &request).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen.starts_with("GET /weather?city=Beijing&appid=q-secret-value "));
    assert!(seen.to_ascii_lowercase().contains("x-api-key: h-secret-value"));
    assert!(!output.body.contains("q-secret-value") && !output.body.contains("h-secret-value"));
    // The URL the model sees and the UI shows never carries the injected parameter.
    assert!(!parse_target(&request).unwrap().url.as_str().contains("q-secret-value"));
}

#[tokio::test]
async fn test_vault_exec_redirect_not_followed() {
    let (base, server) = serve(|_| ("302 Found".into(), "location: https://elsewhere.example/steal\r\n".into(), String::new())).await;
    let output = run_against(&bearer(), &args(&format!("{base}/start"))).await.unwrap();
    server.await.unwrap();
    assert_eq!(output.status, 302);
    assert_eq!(output.headers.get("location").map(String::as_str), Some("https://elsewhere.example/steal"));
}

#[tokio::test]
async fn test_vault_exec_response_header_redacted_and_cookie_dropped() {
    let header_line = format!("x-echo: Bearer {TOKEN}\r\nset-cookie: session=abc\r\n");
    let (base, server) = serve(move |_| ("200 OK".into(), header_line.clone(), "{}".into())).await;
    let output = run_against(&bearer(), &args(&format!("{base}/"))).await.unwrap();
    server.await.unwrap();
    assert!(!output.headers.values().any(|v| v.contains(TOKEN)));
    assert!(!output.headers.contains_key("set-cookie"));
}

#[test]
fn test_vault_exec_rejects_injected_header_override() {
    let mut request = args("https://api.example.com/");
    request.headers.insert("authorization".into(), "Bearer mine".into());
    let target = parse_target(&request).unwrap();
    assert_eq!(check_agent_inputs(&request, &bearer(), &target).unwrap_err().code, "header_not_allowed");
    let mut request = args("https://api.example.com/");
    request.headers.insert("Host".into(), "evil.example".into());
    assert_eq!(check_agent_inputs(&request, &bearer(), &target).unwrap_err().code, "header_not_allowed");
}

#[test]
fn test_vault_exec_rejects_injected_query_override() {
    let creds = resolved(AuthKind::Query, vec![InjectionRule::Query { name: "appid".into(), field: "q".into() }], &[("q", "v")], &[]);
    let request = args("https://api.example.com/?appid=guess");
    let target = parse_target(&request).unwrap();
    assert_eq!(check_agent_inputs(&request, &creds, &target).unwrap_err().code, "header_not_allowed");
}

#[test]
fn test_vault_exec_parse_target() {
    let target = parse_target(&args("https://API.Example.com/a/b?x=1#frag")).unwrap();
    assert_eq!(target.origin, "https://api.example.com");
    assert_eq!(target.url.as_str(), "https://api.example.com/a/b?x=1");
    assert_eq!(parse_target(&args("ftp://example.com/")).unwrap_err().code, "invalid_request");
    assert_eq!(parse_target(&args("https://u:p@example.com/")).unwrap_err().code, "invalid_request");
    let mut bad_method = args("https://example.com/");
    bad_method.method = "TRACE".into();
    assert_eq!(parse_target(&bad_method).unwrap_err().code, "invalid_request");
}

#[test]
fn test_vault_exec_body_and_json_exclusive() {
    let mut request = args("https://api.example.com/");
    request.body = Some("x".into());
    request.json = Some(serde_json::json!({}));
    let target = parse_target(&request).unwrap();
    assert_eq!(check_agent_inputs(&request, &bearer(), &target).unwrap_err().code, "invalid_request");
}

#[test]
fn test_vault_exec_redactor_covers_encodings() {
    let secret = "a b/c+d=e\"q";
    let creds = resolved(AuthKind::Bearer, vec![], &[("token", secret)], &[]);
    let redactor = redactor(&creds);
    let json_escaped = serde_json::to_string(secret).unwrap();
    let variants = [secret.to_string(), BASE64.encode(secret), BASE64_URL.encode(secret),
        urlencoding::encode(secret).into_owned(), json_escaped[1..json_escaped.len() - 1].to_string()];
    for leaked in variants {
        let cleaned = redactor.clean(&format!("prefix {leaked} suffix"));
        assert_eq!(cleaned, "prefix [REDACTED] suffix", "{leaked}");
    }
}

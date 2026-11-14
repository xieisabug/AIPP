use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "aipp-fake-token-7f3a91";

fn args(url: &str) -> RequestArgs {
    RequestArgs { method: "GET".into(), url: url.into(), headers: Default::default(), query: Default::default(),
        body: None, json: None, basic_auth: None, timeout_secs: Some(5) }
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

async fn run_against(args: &RequestArgs) -> Result<Output, ExecError> {
    send(build_request(&test_client(), args)?).await
}

#[tokio::test]
async fn test_vault_http_sends_substituted_headers() {
    let (base, server) = serve(echo).await;
    let mut request = args(&format!("{base}/user"));
    request.headers.insert("Authorization".into(), format!("Bearer {TOKEN}"));
    let output = run_against(&request).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen.to_ascii_lowercase().contains(&format!("authorization: bearer {TOKEN}").to_ascii_lowercase()));
    assert_eq!(output.status, 200);
}

#[tokio::test]
async fn test_vault_http_encodes_query_and_basic_auth() {
    let (base, server) = serve(echo).await;
    let mut request = args(&format!("{base}/weather"));
    request.query.insert("city".into(), Value::String("Beijing".into()));
    request.query.insert("key".into(), Value::String("a&b=c #d".into()));
    request.basic_auth = Some(BasicAuth { username: "me".into(), password: "pw-secret-1".into() });
    run_against(&request).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen.starts_with("GET /weather?city=Beijing&key=a%26b%3Dc+%23d "), "{seen}");
    assert!(seen.contains(&format!("Basic {}", BASE64.encode("me:pw-secret-1"))));
}

#[tokio::test]
async fn test_vault_http_redirect_not_followed() {
    let (base, server) = serve(|_| ("302 Found".into(), "location: https://elsewhere.example/steal\r\n".into(), String::new())).await;
    let output = run_against(&args(&format!("{base}/start"))).await.unwrap();
    server.await.unwrap();
    assert_eq!(output.status, 302);
    assert_eq!(output.headers.get("location").map(String::as_str), Some("https://elsewhere.example/steal"));
}

#[tokio::test]
async fn test_vault_http_drops_set_cookie() {
    let (base, server) = serve(|_| ("200 OK".into(), "set-cookie: session=abc\r\nx-other: 1\r\n".into(), "{}".into())).await;
    let output = run_against(&args(&format!("{base}/"))).await.unwrap();
    server.await.unwrap();
    assert!(!output.headers.contains_key("set-cookie"));
    assert_eq!(output.headers.get("x-other").map(String::as_str), Some("1"));
}

#[test]
fn test_vault_http_validates_arguments() {
    let client = test_client();
    assert_eq!(build_request(&client, &args("ftp://example.com/")).unwrap_err().code, "invalid_request");
    assert_eq!(build_request(&client, &args("https://u:p@example.com/")).unwrap_err().code, "invalid_request");
    let mut bad_method = args("https://example.com/");
    bad_method.method = "TRACE".into();
    assert_eq!(build_request(&client, &bad_method).unwrap_err().code, "invalid_request");
    let mut both = args("https://example.com/");
    both.body = Some("x".into());
    both.json = Some(serde_json::json!({}));
    assert_eq!(build_request(&client, &both).unwrap_err().code, "invalid_request");
    let mut host = args("https://example.com/");
    host.headers.insert("Host".into(), "evil.example".into());
    assert_eq!(build_request(&client, &host).unwrap_err().code, "invalid_request");
    // A substituted value with a newline cannot become a header.
    let mut newline = args("https://example.com/");
    newline.headers.insert("X-Key".into(), "a\nb".into());
    assert_eq!(build_request(&client, &newline).unwrap_err().code, "invalid_request");
    let mut twice = args("https://example.com/");
    twice.headers.insert("authorization".into(), "Bearer x".into());
    twice.basic_auth = Some(BasicAuth { username: "u".into(), password: "p".into() });
    assert_eq!(build_request(&client, &twice).unwrap_err().code, "invalid_request");
}

#[test]
fn test_vault_redactor_covers_encodings() {
    let secret = "a b/c+d=e\"q";
    let redactor = redactor_for([secret.to_string()]);
    let json_escaped = serde_json::to_string(secret).unwrap();
    let variants = [secret.to_string(), BASE64.encode(secret), BASE64_URL.encode(secret),
        urlencoding::encode(secret).into_owned(), json_escaped[1..json_escaped.len() - 1].to_string()];
    for leaked in variants {
        let cleaned = redactor.clean(&format!("prefix {leaked} suffix"));
        assert_eq!(cleaned, "prefix [REDACTED] suffix", "{leaked}");
    }
}

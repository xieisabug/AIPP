use super::*;
use crate::api::genai_client::create_client_with_config;
use crate::db::llm_db::LLMProviderConfig;
use base64::Engine;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SECRET: &str = "vault-key/with+quote\"and\\slash";

fn error_redactor(secret: &str) -> ProviderErrorRedactor {
    let configs = vec![LLMProviderConfig { id: 1, llm_provider_id: 1,
        name: "api_key".into(), value: secret.into(), append_location: String::new(), is_addition: false }];
    crate::vault::config_ref::provider_redactor(&configs).unwrap()
}

fn forms() -> Vec<String> {
    let json = serde_json::to_string(SECRET).unwrap();
    vec![SECRET.into(), base64::engine::general_purpose::STANDARD.encode(SECRET),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(SECRET),
        urlencoding::encode(SECRET).into_owned(), json[1..json.len() - 1].into()]
}

fn assert_safe(error: &genai::Error) {
    for rendered in [error.to_string(), format!("{error:?}")] {
        for secret in forms() { assert!(!rendered.contains(&secret), "credential escaped redaction"); }
        assert!(rendered.contains("[REDACTED]"));
        assert!(rendered.contains("fixture rejection"), "diagnostics were discarded");
    }
    assert!(std::error::Error::source(error).is_none());
}

async fn mock_client(stream: bool) -> (Client, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let origin = endpoint.trim_end_matches("/v1");
    let configs = vec![
        LLMProviderConfig { id: 1, llm_provider_id: 1, name: "api_key".into(), value: SECRET.into(), append_location: "header".into(), is_addition: false },
        LLMProviderConfig { id: 2, llm_provider_id: 1, name: "endpoint".into(), value: endpoint.clone(), append_location: "header".into(), is_addition: false },
        LLMProviderConfig { id: 0, llm_provider_id: 1, name: crate::vault::config_ref::PROVIDER_GUARD.into(),
            value: serde_json::json!({"sites": [origin], "context": "test provider", "credentials": ["test key"], "secrets": [SECRET]}).to_string(),
            append_location: String::new(), is_addition: false },
    ];
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let length: usize = headers.lines().find_map(|s| s.strip_prefix("content-length: "))
                    .map(|s| s.parse().unwrap()).unwrap_or(0);
                if bytes.len() >= end + 4 + length { break; }
            }
        }
        let json = serde_json::json!({"error": {"message": format!("fixture rejection {}", forms().join(" | ")), "code": "invalid_api_key"}}).to_string();
        let (status, content_type, body) = if stream {
            ("200 OK", "text/event-stream", format!("data: {json}\n\ndata: [DONE]\n\n"))
        } else { ("429 Too Many Requests", "application/json", json) };
        socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nRetry-After: 7\r\nX-Request-Id: request-fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    });
    let client = create_client_with_config(None, &configs, "gpt-test", "openai_api", None,
        None, false, Some(3), stream, &HashMap::new()).unwrap();
    (client, server)
}

#[tokio::test]
async fn test_provider_errors_redact_non_stream_and_model_list() {
    for models in [false, true] {
        let (client, server) = mock_client(false).await;
        let error = if models {
            client.all_models(genai::adapter::AdapterKind::OpenAI, ()).await.unwrap_err()
        } else {
            client.exec_chat("gpt-test", ChatRequest::from_user("hello"), None).await.unwrap_err()
        };
        assert_safe(&error);
        assert_eq!(error.status().unwrap().as_u16(), 429);
        assert_eq!(error.headers().unwrap().get("retry-after").unwrap(), "7");
        assert_eq!(error.headers().unwrap().get("x-request-id").unwrap(), "request-fixture");
        server.await.unwrap();
    }
}

#[tokio::test]
async fn test_provider_errors_redact_http_rejection_when_stream_is_polled() {
    let (client, server) = mock_client(false).await;
    // rust-genai creates the stream lazily. The HTTP request and its 429 response happen on poll,
    // not while exec_chat_stream builds the response wrapper.
    let mut stream = client.exec_chat_stream("gpt-test", ChatRequest::from_user("hello"), None).await.unwrap().stream;
    let error = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = stream.next().await {
            if let Err(error) = event { return error; }
        }
        panic!("the rejected HTTP stream must yield an error");
    }).await.expect("stream request did not complete");
    assert_safe(&error);
    server.await.unwrap();
}

#[tokio::test]
async fn test_provider_errors_redact_stream_setup_resolver_failure() {
    let resolver = genai::resolver::ServiceTargetResolver::from_resolver_fn(
        |_target: genai::ServiceTarget| -> genai::resolver::Result<genai::ServiceTarget> {
            Err(genai::resolver::Error::Custom(format!("fixture rejection {SECRET}")))
        },
    );
    let inner = genai::Client::builder().with_service_target_resolver(resolver).build().unwrap();
    let configs = vec![LLMProviderConfig { id: 1, llm_provider_id: 1, name: "api_key".into(),
        value: SECRET.into(), append_location: String::new(), is_addition: false }];
    let redactor = crate::vault::config_ref::provider_redactor(&configs).unwrap();
    let client = Client::new(inner, Arc::new(redactor));
    let error = client.exec_chat_stream("gpt-test", ChatRequest::from_user("hello"), None).await.err().unwrap();
    assert_safe(&error);
}

#[tokio::test]
async fn test_provider_errors_redact_mid_stream_event() {
    let (client, server) = mock_client(true).await;
    let mut stream = client.exec_chat_stream("gpt-test", ChatRequest::from_user("hello"), None).await.unwrap().stream;
    let mut saw_error = false;
    while let Some(event) = stream.next().await {
        if let Err(error) = event { assert_safe(&error); saw_error = true; }
    }
    assert!(saw_error);
    server.await.unwrap();
}

#[test]
fn test_provider_errors_redact_embedded_vault_values_and_debug() {
    let configs = vec![LLMProviderConfig { id: 0, llm_provider_id: 1,
        name: crate::vault::config_ref::PROVIDER_GUARD.into(),
        value: serde_json::json!({"sites": [], "context": "test", "credentials": [], "secrets": [SECRET]}).to_string(),
        append_location: String::new(), is_addition: false }];
    let redactor = crate::vault::config_ref::provider_redactor(&configs).unwrap();
    let error = genai::Error::ChatResponse { model_iden: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "test"),
        body: serde_json::json!({"error": format!("fixture rejection {}", forms().join(" | "))}) };
    assert_safe(&clean_error(error, &redactor));
}

#[test]
fn test_provider_errors_redact_headers_without_body_echo() {
    let redactor = error_redactor(SECRET);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-debug-token", reqwest::header::HeaderValue::from_str(SECRET).unwrap());
    headers.insert("retry-after", reqwest::header::HeaderValue::from_static("9"));
    let error = clean_error(genai::Error::HttpError {
        status: reqwest::StatusCode::TOO_MANY_REQUESTS, canonical_reason: "Too Many Requests".into(),
        body: "fixture rejection".into(), headers: Box::new(headers),
    }, &redactor);
    assert_eq!(error.status().unwrap().as_u16(), 429);
    assert_eq!(error.headers().unwrap().get("x-debug-token").unwrap(), "[REDACTED]");
    assert_eq!(error.headers().unwrap().get("retry-after").unwrap(), "9");
    assert!(!format!("{error:?}").contains(SECRET));
}

#[test]
fn test_provider_errors_redact_stream_source_without_retaining_raw_error() {
    let redactor = error_redactor(SECRET);
    let error = clean_error(genai::Error::WebStream {
        model_iden: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "test"),
        cause: format!("fixture rejection {SECRET}"),
        error: Box::new(std::io::Error::other(format!("fixture rejection {SECRET}"))),
    }, &redactor);
    assert_safe(&error);
    let genai::Error::WebStream { error: source, .. } = error else { panic!("stream shape lost") };
    assert!(!source.to_string().contains(SECRET));
    assert!(source.to_string().contains("fixture rejection"));
}

#[test]
fn test_provider_errors_preserve_unrelated_diagnostics() {
    let redactor = error_redactor(SECRET);
    let error = genai::Error::Internal("fixture rejection: protocol mismatch, request_id=req-123".into());
    let before = error.to_string();
    assert_eq!(clean_error(error, &redactor).to_string(), before);
}

#[test]
fn test_provider_errors_omit_normalized_json_credentials() {
    for secret in ["1e3", "123", "true", "null", r#"{ "token": "abc", "amount": 1e3 }"#,
        r#"["array-token", 1e3]"#, r#""quoted-token""#] {
        let redactor = error_redactor(secret);
        assert!(redactor.requires_opaque_errors());
        let value: serde_json::Value = serde_json::from_str(secret).unwrap();
        let normalized = value.to_string();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-echo", reqwest::header::HeaderValue::from_str(&normalized).unwrap());
        headers.insert("retry-after", reqwest::header::HeaderValue::from_static("7"));
        // Parsing/reformatting can already have happened in the library before our boundary.
        let error = clean_error(genai::Error::HttpError {
            status: reqwest::StatusCode::TOO_MANY_REQUESTS,
            canonical_reason: format!("fixture rejection {secret}"),
            body: format!("{{\"echo\": {normalized}, \"alternate_number\": 1000}}"), headers: Box::new(headers),
        }, &redactor);
        assert_eq!(error.status().unwrap().as_u16(), 429);
        assert!(error.headers().unwrap().is_empty(), "opaque errors must not retain response headers");
        let genai::Error::HttpError { body, canonical_reason, .. } = error else { panic!("status lost") };
        assert_eq!(body, redactor.clean(""));
        assert_eq!(canonical_reason, "Too Many Requests");

        let model_iden = genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "test");
        let errors = [
            genai::Error::ChatResponse { model_iden: model_iden.clone(), body: value.clone() },
            genai::Error::ChatResponseGeneration { model_iden: model_iden.clone(),
                request_payload: Box::new(serde_json::json!({"echo": value.clone()})),
                response_body: Box::new(value), cause: format!("fixture rejection {normalized}") },
            genai::Error::WebStream { model_iden, cause: normalized.clone(),
                error: Box::new(std::io::Error::other(normalized)) },
        ];
        for error in errors {
            let cleaned = clean_error(error, &redactor);
            let genai::Error::Internal(ref detail) = cleaned else { panic!("opaque error source retained") };
            assert_eq!(detail, &redactor.clean(""));
            assert!(std::error::Error::source(&cleaned).is_none());
        }
        // This policy is scoped to Provider errors, not successful model/tool messages.
        assert_eq!(crate::vault::executor::redactor_for(["t".into()]).clean("text"), "text");
    }
}

#[test]
fn test_provider_errors_omit_json_constituents_of_embedded_keys() {
    let configs = vec![
        LLMProviderConfig { id: 1, llm_provider_id: 1, name: "api_key".into(),
            value: "prefix-1e3-suffix".into(), append_location: String::new(), is_addition: false },
        LLMProviderConfig { id: 0, llm_provider_id: 1, name: crate::vault::config_ref::PROVIDER_GUARD.into(),
            value: serde_json::json!({"sites": [], "context": "test", "credentials": [], "secrets": ["1e3"]}).to_string(),
            append_location: String::new(), is_addition: false },
    ];
    let redactor = crate::vault::config_ref::provider_redactor(&configs).unwrap();
    assert!(redactor.requires_opaque_errors());
    assert_eq!(redactor.clean("credential normalized to 1000.0"), redactor.clean(""));
    assert!(redactor.clean("").contains("[REDACTED]"));
    assert!(error_redactor("Bearer 1e3").requires_opaque_errors());
}

#[test]
fn test_provider_errors_discard_hidden_stream_source_chain() {
    struct HiddenSource(std::io::Error);
    impl std::fmt::Debug for HiddenSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("HiddenSource") }
    }
    impl std::fmt::Display for HiddenSource {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("outer failure") }
    }
    impl std::error::Error for HiddenSource {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { Some(&self.0) }
    }
    let redactor = error_redactor(SECRET);
    let error = clean_error(genai::Error::WebStream {
        model_iden: genai::ModelIden::new(genai::adapter::AdapterKind::OpenAI, "test"),
        cause: "stream failed".into(),
        error: Box::new(HiddenSource(std::io::Error::other(format!("fixture rejection {SECRET}")))),
    }, &redactor);
    assert_safe(&error);
    let genai::Error::WebStream { error: source, .. } = error else { panic!("stream shape lost") };
    assert!(source.source().is_none());
    assert!(source.to_string().contains("Caused by:"));
}

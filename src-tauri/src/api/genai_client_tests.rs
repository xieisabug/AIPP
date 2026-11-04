use super::{
    effective_endpoint_url, get_default_endpoint, infer_adapter_kind_simple,
    resolve_provider_endpoint, unset_endpoint_hint, ProviderEndpointResolution,
};
use genai::adapter::AdapterKind;
use super::create_client_with_config;
use crate::db::llm_db::LLMProviderConfig;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn endpoint_config(endpoint: &str) -> Vec<LLMProviderConfig> {
    vec![LLMProviderConfig {
        id: 1, llm_provider_id: 1, name: "endpoint".into(), value: endpoint.into(),
        append_location: "header".into(), is_addition: false,
    }]
}

/// Invalid inputs fail during client construction, before any transport exists.
#[test]
fn test_endpoint_invalid_fails_before_client_creation() {
    for endpoint in ["bad", "http:///path", "https:host", "https://", "ftp://localhost",
        "http://localhost/v1?key=secret", "http://localhost/v1#fragment",
        "http://secret:password@localhost/v1", "http://localhost\\other", "http://local\nhost"] {
        let err = create_client_with_config(&endpoint_config(endpoint), "gpt-test", "openai_api",
            None, None, false, Some(2), false, &HashMap::new()).err().expect(endpoint);
        let message = err.to_string();
        assert!(message.contains("Endpoint 无效"), "{endpoint}: {message}");
        assert!(!message.contains("api.openai.com"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("password"));
    }
}

/// Only loopback is contacted; verify the real genai models request path.
#[tokio::test]
async fn test_endpoint_models_request_uses_local_mock() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|part| part == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&chunk[..n]);
        }
        let body = r#"{"object":"list","data":[{"id":"local-model","object":"model","created":0,"owned_by":"mock"}]}"#;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    let client = create_client_with_config(&endpoint_config(&endpoint), "", "openai_api",
        None, None, false, Some(2), false, &HashMap::new()).unwrap();
    let models = client.all_models(AdapterKind::OpenAI).await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id.to_string(), "local-model");
    let request = tokio::time::timeout(std::time::Duration::from_secs(3), server).await.unwrap().unwrap();
    assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"), "{request}");
}

#[test]
fn test_endpoint_empty_is_unset_for_product_default() {
    assert_eq!(
        resolve_provider_endpoint(None).unwrap(),
        ProviderEndpointResolution::Unset
    );
    assert_eq!(
        resolve_provider_endpoint(Some("")).unwrap(),
        ProviderEndpointResolution::Unset
    );
    assert_eq!(
        resolve_provider_endpoint(Some("   ")).unwrap(),
        ProviderEndpointResolution::Unset
    );

    let openai = AdapterKind::OpenAI;
    let url = effective_endpoint_url(&ProviderEndpointResolution::Unset, openai);
    assert_eq!(url, get_default_endpoint(openai));
    // 空值提示与无效值错误必须可区分
    assert!(unset_endpoint_hint().contains("未填写"));
    assert!(!unset_endpoint_hint().contains("无效"));
}

#[test]
fn test_endpoint_invalid_rejected_without_default_fallback() {
    let err = resolve_provider_endpoint(Some("bad")).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Endpoint 无效"),
        "expected field-level Chinese error, got: {msg}"
    );
    assert!(
        !msg.contains("api.openai.com"),
        "invalid-endpoint error must not leak default OpenAI URL: {msg}"
    );
    assert!(
        !msg.contains(unset_endpoint_hint()),
        "invalid error must be distinguishable from empty hint"
    );
    assert!(msg.contains("无效"));
    assert!(!msg.contains("未填写"));

    // 常见「半截 http」也不应静默放行 / 不触发外网默认
    for bad in ["http", "http://", "ftp://example.com", "not a url", "openai.com/v1"] {
        let e = resolve_provider_endpoint(Some(bad)).unwrap_err().to_string();
        assert!(e.contains("Endpoint 无效"), "expected reject for {bad}: {e}");
        assert!(!e.contains("api.openai.com"), "no OpenAI URL leak for {bad}: {e}");
    }
}

#[test]
fn test_endpoint_valid_loopback_kept_as_final_destination() {
    let r = resolve_provider_endpoint(Some("http://127.0.0.1:8080/v1")).unwrap();
    assert_eq!(
        r,
        ProviderEndpointResolution::Custom("http://127.0.0.1:8080/v1/".into())
    );
    assert_eq!(
        effective_endpoint_url(&r, AdapterKind::OpenAI),
        "http://127.0.0.1:8080/v1/"
    );

    let r = resolve_provider_endpoint(Some("https://localhost:8443/v1/")).unwrap();
    assert_eq!(
        r,
        ProviderEndpointResolution::Custom("https://localhost:8443/v1/".into())
    );
    assert_eq!(
        effective_endpoint_url(&r, infer_adapter_kind_simple("openai")),
        "https://localhost:8443/v1/"
    );

    let r = resolve_provider_endpoint(Some("  https://example.com/v1  ")).unwrap();
    assert_eq!(
        r,
        ProviderEndpointResolution::Custom("https://example.com/v1/".into())
    );
}

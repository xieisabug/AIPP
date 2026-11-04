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
        let err = create_client_with_config(None, &endpoint_config(endpoint), "gpt-test", "openai_api",
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
    let client = create_client_with_config(None, &endpoint_config(&endpoint), "", "openai_api",
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

#[tokio::test]
async fn test_opencode_go_protocol_requests() {
    use crate::api::opencode_go::{route, session_id, USER_AGENT};
    use genai::chat::{ChatMessage, ChatRequest};

    async fn capture(mode: &str, sync_id: &str, suffix: &str, auth: &str) {
        let (adapter, base) = route(Some(mode)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = base.replace("https://opencode.ai", &format!("http://{}",listener.local_addr().unwrap()));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            let mut chunk = [0; 2048];
            while !data.windows(4).any(|s| s == b"\r\n\r\n") {
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                data.extend_from_slice(&chunk[..n]);
            }
            socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            String::from_utf8(data).unwrap().to_ascii_lowercase()
        });
        let session = session_id(sync_id);
        let client = super::build_client(adapter, endpoint, "test-key".into(),
            Some("go-model".into()), Some(session), genai::WebConfig::default(), HashMap::new());
        let req = ChatRequest::new(vec![ChatMessage::user("test")]);
        use futures::StreamExt;
        let mut stream = client.exec_chat_stream("go-model", req, None).await.unwrap().stream;
        while let Some(Ok(_)) = stream.next().await {}
        let request = tokio::time::timeout(std::time::Duration::from_secs(5), server).await.unwrap().unwrap();
        assert!(request.starts_with(&format!("post /inference/go/{suffix}")), "{request}");
        assert!(request.contains(auth), "{request}");
        assert!(request.contains(&format!("x-opencode-session: {session}")), "{request}");
        assert!(request.contains(&format!("user-agent: {}", USER_AGENT.to_ascii_lowercase())), "{request}");
    }
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    assert_ne!(session_id(&a), session_id(&b));
    tokio::join!(
        capture("chat_completions", &a, "openai/v1/chat/completions", "authorization: bearer test-key"),
        capture("responses", &a, "openai/v1/responses", "authorization: bearer test-key"),
        capture("anthropic_messages", &b, "anthropic/v1/messages", "x-api-key: test-key"),
        capture("google_gemini", &b, "google/v1beta/models/go-model:streamgeneratecontent", "x-goog-api-key: test-key"),
    );
}

#[tokio::test]
async fn test_opencode_go_deepseek_stream_delivers_content() {
    use futures::StreamExt;
    use genai::chat::{ChatMessage, ChatRequest, ChatStreamEvent};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let model = "deepseek-v4.1-flash";
    let (adapter, base) = crate::api::opencode_go::route(Some("chat_completions")).unwrap();
    let client = super::build_client(adapter, base.replace("https://opencode.ai", &format!("http://{addr}")),
        "test-key".into(), Some(model.into()), Some(uuid::Uuid::new_v4()), genai::WebConfig::default(), HashMap::new());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut chunk = [0; 2048];
        while !data.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&chunk[..n]);
        }
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Think\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n",
            "data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":39,\"completion_tokens\":16,\"total_tokens\":55,\"prompt_tokens_details\":{\"cached_tokens\":0},\"completion_tokens_details\":{\"reasoning_tokens\":13},\"cost\":0,\"opencode\":{\"billing_source\":\"go\",\"allowance\":0.00000927}}}\n\n",
            "data: [DONE]\n\n");
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
    });
    let req = ChatRequest::new(vec![ChatMessage::user("test")]);
    let options = genai::chat::ChatOptions::default().with_capture_usage(true);
    let mut stream = client.exec_chat_stream(model, req, Some(&options)).await.unwrap().stream;
    let mut text = String::new();
    let mut ended = false;
    while let Some(event) = stream.next().await {
        match event.unwrap() {
            ChatStreamEvent::Chunk(chunk) => text.push_str(&chunk.content),
            ChatStreamEvent::End(end) => {
                ended = true;
                let usage = end.captured_usage.expect("Go stop chunk must retain usage");
                assert_eq!((usage.prompt_tokens, usage.completion_tokens, usage.total_tokens), (Some(39), Some(16), Some(55)));
                let billing = crate::api::opencode_go::billing_metadata(&usage).unwrap();
                assert_eq!(billing["allowance"], 0.00000927);
                assert_eq!(billing["cost"], 0);
                assert_eq!(usage.completion_tokens_details.unwrap().reasoning_tokens, Some(13));
            }
            _ => {}
        }
    }
    server.await.unwrap();
    assert_eq!(text, "OK");
    assert!(ended);
}

#[tokio::test]
async fn test_opencode_go_billing_survives_translated_streams() {
    use futures::StreamExt;
    use genai::chat::{ChatMessage, ChatOptions, ChatRequest, ChatStreamEvent};
    use serde_json::json;
    let billing = json!({"billing_source":"go","allowance":0.00000927,"list_cost":0.00001545});
    let responses = json!({"type":"response.completed","response":{"id":"r1","model":"test","status":"completed","output":[],
        "usage":{"input_tokens":39,"output_tokens":16,"total_tokens":55,"cost":0,"opencode":billing}}});
    let delta = json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
        "usage":{"output_tokens":16,"cost":0,"opencode":billing}});
    let gemini = json!({"candidates":[{"finishReason":"STOP"}],
        "usageMetadata":{"promptTokenCount":39,"candidatesTokenCount":16,"totalTokenCount":55,"cost":0,"opencode":billing}});
    let gemini_text = json!({"candidates":[{"content":{"parts":[{"text":"OK"}]}}]});
    let frames = [
        ("responses", "text/event-stream", format!("event: response.completed\ndata: {responses}\n\n")),
        ("anthropic_messages", "text/event-stream", format!(
            "event: message_start\ndata: {{\"message\":{{\"usage\":{{\"input_tokens\":39}}}}}}\n\nevent: message_delta\ndata: {delta}\n\nevent: message_stop\ndata: {{}}\n\n")),
        ("google_gemini", "application/json", format!("[\n{gemini_text}\n,\n{gemini}\n]")),
    ];
    for (mode, content_type, body) in frames {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (adapter, base) = crate::api::opencode_go::route(Some(mode)).unwrap();
        let client = super::build_client(adapter, base.replace("https://opencode.ai", &format!("http://{addr}")),
            "test-key".into(), Some("test".into()), Some(uuid::Uuid::new_v4()), genai::WebConfig::default(), HashMap::new());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            let mut chunk = [0; 2048];
            while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                data.extend_from_slice(&chunk[..n]);
            }
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let req = ChatRequest::new(vec![ChatMessage::user("test")]);
        let options = ChatOptions::default().with_capture_usage(true);
        let mut stream = client.exec_chat_stream("test", req, Some(&options)).await.unwrap().stream;
        let mut ended = false;
        while let Some(event) = stream.next().await {
            if let ChatStreamEvent::End(end) = event.unwrap() {
                let usage = end.captured_usage.expect(mode);
                assert_eq!(usage.total_tokens, Some(55), "{mode}");
                let info = crate::api::opencode_go::billing_metadata(&usage).expect(mode);
                assert_eq!(info["allowance"], 0.00000927, "{mode}");
                assert_eq!(info["cost"], 0, "{mode}");
                ended = true;
            }
        }
        assert!(ended, "{mode}");
        server.await.unwrap();
    }
}

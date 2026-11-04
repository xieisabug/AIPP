use super::*;

#[test]
fn test_mobile_http_rejects_non_http_and_embedded_credentials() {
    for raw in ["file:///etc/passwd", "content://file/1", "javascript:alert(1)", "https://user:secret@example.com", "not a url"] {
        assert!(validate_url(raw).is_err());
    }
    assert!(validate_url("https://example.com/page").is_ok());
}

#[test]
fn test_mobile_http_decodes_declared_charset_and_rejects_binary_or_empty() {
    let (gbk, _, _) = encoding_rs::GBK.encode("中文正文");
    assert_eq!(decode_document(&gbk, "text/plain; charset=gbk").unwrap(), "中文正文");
    assert!(decode_document(b"image", "image/png").is_err());
    assert!(decode_document(b" ", "text/plain").is_err());
    assert!(decode_document(&[0xff], "text/plain; charset=utf-8").is_err());
    assert!(decode_document(b"x", "text/plain; charset=unknown").is_err());
}

#[tokio::test]
async fn test_mobile_http_reports_http_error_and_limits_before_reading_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for (response, expected) in [
        ("HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n", "HTTP 429"),
        ("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5000000\r\n\r\n", "4 MiB"),
        ("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello", "hello"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let result = fetch_document(&format!("http://{address}/"), "markdown").await;
        let text = result.unwrap_or_else(|error| error);
        assert!(text.contains(expected), "{text}");
        server.await.unwrap();
    }
}

#[test]
fn test_mobile_http_preserves_non_html_when_markdown_requested() {
    for content_type in ["text/plain", "text/markdown", "application/json"] {
        let content = r#"{"tag":"<example>","value":"**keep**"}"#;
        assert_eq!(render_document(content.as_bytes(), content_type, "markdown").unwrap(), content);
    }
    assert_eq!(render_document(b"<p>Hello</p>", "text/html", "markdown").unwrap().trim(), "Hello");
}

#[tokio::test]
async fn test_mobile_http_rejects_chunked_overflow_without_content_length() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        stream.read(&mut request).await.unwrap();
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        let chunk = vec![b'x'; 64 * 1024];
        for _ in 0..66 {
            if stream.write_all(b"10000\r\n").await.is_err() { break; }
            if stream.write_all(&chunk).await.is_err() { break; }
            if stream.write_all(b"\r\n").await.is_err() { break; }
        }
        let _ = stream.write_all(b"0\r\n\r\n").await;
    });
    let error = fetch_document(&format!("http://{address}/"), "markdown").await.unwrap_err();
    assert!(error.contains("流式响应超过 4 MiB"), "{error}");
    server.await.unwrap();
}

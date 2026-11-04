use crate::db::mcp_db::MCPServer;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::transport::{
    streamable_http_client::StreamableHttpClientTransportConfig,
    StreamableHttpClientTransport,
};

/// Preserve configured headers, including the complete Authorization scheme.
/// rmcp's auth_header accepts a bare bearer token, not an HTTP header value.
pub(crate) fn create_http_transport(
    server: &MCPServer,
    builder: reqwest::ClientBuilder,
) -> Result<StreamableHttpClientTransport<reqwest::Client>, String> {
    let url = server.url.as_deref().ok_or("No URL specified for HTTP transport")?;
    if let Some(raw) = server.headers.as_deref().filter(|raw| !raw.trim().is_empty()) {
        let parsed: serde_json::Value = serde_json::from_str(raw)
            .map_err(|_| "Invalid MCP HTTP headers: expected a JSON object of string values".to_string())?;
        let object = parsed.as_object()
            .ok_or("Invalid MCP HTTP headers: expected a JSON object of string values")?;
        if object.values().any(|value| !value.is_string()) {
            return Err("Invalid MCP HTTP headers: all values must be strings".into());
        }
    }
    let (_, headers) = super::util::parse_server_headers(server);
    let mut header_map = HeaderMap::new();
    for (name, value) in headers.unwrap_or_default() {
        let name = HeaderName::try_from(name.as_str())
            .map_err(|error| format!("Invalid MCP HTTP header name: {error}"))?;
        let value = HeaderValue::from_str(&value)
            .map_err(|error| format!("Invalid MCP HTTP header value for {name}: {error}"))?;
        header_map.insert(name, value);
    }
    let client = builder.default_headers(header_map).build()
        .map_err(|error| format!("Failed to build MCP HTTP client: {error}"))?;
    Ok(StreamableHttpClientTransport::with_client(
        client,
        StreamableHttpClientTransportConfig::with_uri(url),
    ))
}

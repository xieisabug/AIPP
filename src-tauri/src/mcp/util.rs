use crate::db::mcp_db::MCPServer;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Parse server.headers JSON string into:
/// - Option<String> Authorization header value (after env placeholder replacement)
/// - Option<HashMap<String,String>> of all headers (after env placeholder replacement)
pub fn parse_server_headers(
    server: &MCPServer,
) -> (Option<String>, Option<HashMap<String, String>>) {
    if let Some(raw) = &server.headers {
        if raw.trim().is_empty() {
            return (None, None);
        }
        if let Ok(v) = serde_json::from_str::<JsonValue>(raw) {
            if let Some(obj) = v.as_object() {
                let mut map = HashMap::new();
                let mut auth: Option<String> = None;
                // Build server env map once for placeholder replacement
                let server_env = build_server_env_map(server);
                for (k, val) in obj.iter() {
                    if let Some(s) = val.as_str() {
                        let replaced = replace_env_placeholders_from_maps(s, &server_env);
                        if replaced.contains("${") {
                            // placeholder unresolved
                            warn!(key=%k, "Header value contains unresolved ${{...}} placeholder. Check environment or server env settings.");
                        } else {
                            debug!(key=%k, "Header placeholder resolved or no placeholder found");
                        }
                        if k.eq_ignore_ascii_case("authorization") {
                            auth = Some(replaced.clone());
                        }
                        map.insert(k.clone(), replaced);
                    }
                }
                return (auth, Some(map));
            }
        }
    }
    (None, None)
}

/// Replace ${VAR} placeholders with environment variables if present; keep placeholder if missing
pub fn replace_env_placeholders(input: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
            if let Some(end) = chars[i + 2..].iter().position(|c| *c == '}') {
                let var_name: String = chars[i + 2..i + 2 + end].iter().collect();
                if let Ok(val) = std::env::var(&var_name) {
                    out.push_str(&val);
                } else {
                    out.push_str(&format!("${{{}}}", var_name));
                }
                i += 2 + end + 1; // skip ${VAR}
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Replace ${VAR} placeholders using server env map first, then OS env variables.
fn replace_env_placeholders_from_maps(input: &str, server_env: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
            if let Some(end) = chars[i + 2..].iter().position(|c| *c == '}') {
                let var_name: String = chars[i + 2..i + 2 + end].iter().collect();
                if let Some(val) = server_env.get(&var_name) {
                    out.push_str(val);
                } else if let Ok(val) = std::env::var(&var_name) {
                    out.push_str(&val);
                } else {
                    out.push_str(&format!("${{{}}}", var_name));
                }
                i += 2 + end + 1; // skip ${VAR}
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Build a K=V map from server.environment_variables (lines KEY=value). Later entries override earlier.
fn build_server_env_map(server: &MCPServer) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(env_lines) = &server.environment_variables {
        for line in env_lines.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    map
}

/// Produce a sanitized copy of headers for logging: masks sensitive values.
pub fn sanitize_headers_for_log(headers: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in headers.iter() {
        let masked = mask_header_value(k, v);
        out.insert(k.clone(), masked);
    }
    out
}

/// Mask sensitive header values for logs. Authorization keeps scheme and masks token.
pub fn mask_header_value(key: &str, value: &str) -> String {
    if key.eq_ignore_ascii_case("authorization") {
        // Try to keep the scheme (e.g. "Bearer"), mask the rest but leave a short hint
        let mut parts = value.splitn(2, ' ');
        let scheme = parts.next().unwrap_or("");
        let token = parts.next().unwrap_or("");
        let suffix = if token.len() > 8 { &token[..8] } else { token };
        return format!("{} ****{}", scheme, suffix);
    }
    // Default: show up to 8 chars then mask
    if value.len() > 12 {
        format!("{}... (len={})", &value[..8], value.len())
    } else if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test MCPServer with the correct structure
    fn create_test_server(
        headers: Option<String>,
        environment_variables: Option<String>,
    ) -> MCPServer {
        MCPServer {
            id: 1,
            name: "test".to_string(),
            description: "Test server".to_string(),
            transport_type: "stdio".to_string(),
            command: None,
            environment_variables,
            headers,
            url: None,
            timeout: None,
            is_long_running: false,
            is_enabled: true,
            is_builtin: false,
            is_deletable: true,
            proxy_enabled: false,
            created_time: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    // ============================================
    // replace_env_placeholders Tests
    // ============================================

    #[test]
    fn test_replace_env_placeholders_no_placeholders() {
        let input = "hello world";
        let result = replace_env_placeholders(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_replace_env_placeholders_with_existing_var() {
        // Set a test env var
        std::env::set_var("TEST_MCP_VAR_123", "replaced_value");
        let input = "prefix_${TEST_MCP_VAR_123}_suffix";
        let result = replace_env_placeholders(input);
        assert_eq!(result, "prefix_replaced_value_suffix");
        std::env::remove_var("TEST_MCP_VAR_123");
    }

    #[test]
    fn test_replace_env_placeholders_missing_var() {
        // Ensure the var doesn't exist
        std::env::remove_var("NONEXISTENT_VAR_XYZ");
        let input = "prefix_${NONEXISTENT_VAR_XYZ}_suffix";
        let result = replace_env_placeholders(input);
        assert_eq!(result, "prefix_${NONEXISTENT_VAR_XYZ}_suffix");
    }

    #[test]
    fn test_replace_env_placeholders_multiple_vars() {
        std::env::set_var("TEST_MCP_A", "AAA");
        std::env::set_var("TEST_MCP_B", "BBB");
        let input = "${TEST_MCP_A}-${TEST_MCP_B}";
        let result = replace_env_placeholders(input);
        assert_eq!(result, "AAA-BBB");
        std::env::remove_var("TEST_MCP_A");
        std::env::remove_var("TEST_MCP_B");
    }

    #[test]
    fn test_replace_env_placeholders_partial_syntax() {
        let input = "hello ${ incomplete";
        let result = replace_env_placeholders(input);
        assert_eq!(result, "hello ${ incomplete");
    }

    #[test]
    fn test_replace_env_placeholders_empty_var_name() {
        // Note: Cannot set empty env var name on most systems (Invalid argument)
        let input = "${}";
        let result = replace_env_placeholders(input);
        // Empty var name - behavior depends on impl, likely keeps as-is or returns empty
        assert!(result == "${}" || result.is_empty());
    }

    #[test]
    fn test_replace_env_placeholders_dollar_without_brace() {
        let input = "price is $100";
        let result = replace_env_placeholders(input);
        assert_eq!(result, "price is $100");
    }

    // ============================================
    // mask_header_value Tests
    // ============================================

    #[test]
    fn test_mask_header_value_authorization_bearer() {
        let result = mask_header_value("Authorization", "Bearer abc123456789");
        assert_eq!(result, "Bearer ****abc12345");
    }

    #[test]
    fn test_mask_header_value_authorization_case_insensitive() {
        let result = mask_header_value("AUTHORIZATION", "Bearer xyz");
        assert!(result.starts_with("Bearer "));
        assert!(result.contains("****"));
    }

    #[test]
    fn test_mask_header_value_authorization_short_token() {
        let result = mask_header_value("authorization", "Bearer abc");
        assert_eq!(result, "Bearer ****abc");
    }

    #[test]
    fn test_mask_header_value_authorization_no_scheme() {
        let result = mask_header_value("Authorization", "only_token");
        // No space, so entire value becomes scheme
        assert!(result.contains("****"));
    }

    #[test]
    fn test_mask_header_value_regular_header_short() {
        let result = mask_header_value("Content-Type", "application");
        assert_eq!(result, "application");
    }

    #[test]
    fn test_mask_header_value_regular_header_long() {
        let result = mask_header_value("X-Custom-Header", "this_is_a_very_long_value");
        assert!(result.contains("..."));
        assert!(result.contains("len="));
    }

    #[test]
    fn test_mask_header_value_empty_value() {
        let result = mask_header_value("Empty-Header", "");
        assert_eq!(result, "<empty>");
    }

    // ============================================
    // sanitize_headers_for_log Tests
    // ============================================

    #[test]
    fn test_sanitize_headers_for_log_empty() {
        let headers = HashMap::new();
        let result = sanitize_headers_for_log(&headers);
        assert!(result.is_empty());
    }

    #[test]
    fn test_sanitize_headers_for_log_mixed() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer secret_token_12345".to_string());
        headers.insert("Content-Type".to_string(), "text/plain".to_string()); // short enough (10 chars)

        let result = sanitize_headers_for_log(&headers);

        // Authorization should be masked
        assert!(result.get("Authorization").unwrap().contains("****"));
        // Content-Type should remain as-is (it's 10 chars, under threshold of 12)
        assert_eq!(result.get("Content-Type").unwrap(), "text/plain");
    }

    // ============================================
    // build_server_env_map Tests
    // ============================================

    #[test]
    fn test_build_server_env_map_basic() {
        let server = create_test_server(None, Some("KEY1=value1\nKEY2=value2".to_string()));

        let map = build_server_env_map(&server);
        assert_eq!(map.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(map.get("KEY2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_build_server_env_map_with_comments() {
        let server = create_test_server(
            None,
            Some("# This is a comment\nKEY=value\n# Another comment".to_string()),
        );

        let map = build_server_env_map(&server);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_build_server_env_map_empty_lines() {
        let server = create_test_server(None, Some("\n\nKEY=value\n\n".to_string()));

        let map = build_server_env_map(&server);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_build_server_env_map_value_with_equals() {
        let server =
            create_test_server(None, Some("URL=https://api.example.com?key=value".to_string()));

        let map = build_server_env_map(&server);
        // split_once only splits on first '='
        assert_eq!(map.get("URL"), Some(&"https://api.example.com?key=value".to_string()));
    }

    #[test]
    fn test_build_server_env_map_none() {
        let server = create_test_server(None, None);

        let map = build_server_env_map(&server);
        assert!(map.is_empty());
    }

    // ============================================
    // parse_server_headers Tests
    // ============================================

    #[test]
    fn test_parse_server_headers_none() {
        let server = create_test_server(None, None);

        let (auth, headers) = parse_server_headers(&server);
        assert!(auth.is_none());
        assert!(headers.is_none());
    }

    #[test]
    fn test_parse_server_headers_empty_string() {
        let server = create_test_server(Some("   ".to_string()), None);

        let (auth, headers) = parse_server_headers(&server);
        assert!(auth.is_none());
        assert!(headers.is_none());
    }

    #[test]
    fn test_parse_server_headers_with_authorization() {
        let server =
            create_test_server(Some(r#"{"Authorization": "Bearer test_token"}"#.to_string()), None);

        let (auth, headers) = parse_server_headers(&server);
        assert_eq!(auth, Some("Bearer test_token".to_string()));
        assert!(headers.is_some());
        assert_eq!(headers.unwrap().get("Authorization"), Some(&"Bearer test_token".to_string()));
    }

    #[test]
    fn test_parse_server_headers_with_env_replacement() {
        std::env::set_var("TEST_API_KEY_789", "secret_key");

        let server =
            create_test_server(Some(r#"{"X-API-Key": "${TEST_API_KEY_789}"}"#.to_string()), None);

        let (_, headers) = parse_server_headers(&server);
        assert!(headers.is_some());
        assert_eq!(headers.unwrap().get("X-API-Key"), Some(&"secret_key".to_string()));

        std::env::remove_var("TEST_API_KEY_789");
    }

    #[test]
    fn test_parse_server_headers_with_server_env() {
        let server = create_test_server(
            Some(r#"{"X-Custom": "${MY_SERVER_VAR}"}"#.to_string()),
            Some("MY_SERVER_VAR=server_value".to_string()),
        );

        let (_, headers) = parse_server_headers(&server);
        assert!(headers.is_some());
        assert_eq!(headers.unwrap().get("X-Custom"), Some(&"server_value".to_string()));
    }

    #[test]
    fn test_parse_server_headers_invalid_json() {
        let server = create_test_server(Some("not valid json".to_string()), None);

        let (auth, headers) = parse_server_headers(&server);
        assert!(auth.is_none());
        assert!(headers.is_none());
    }
}

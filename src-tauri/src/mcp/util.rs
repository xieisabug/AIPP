use crate::db::mcp_db::MCPServer;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use tracing::{warn, debug};

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
            if line.is_empty() || line.starts_with('#') { continue; }
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

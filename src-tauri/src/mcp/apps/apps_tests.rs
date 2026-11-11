use super::*;

fn catalog() -> AppCatalog {
    serde_json::from_str(include_str!("../../../../tests/fixtures/apps-catalog.json")).unwrap()
}

#[test]
fn test_apps_catalog_rejects_unsupported_protocol_and_reserved_identity() {
    let mut data = catalog();
    data.validate().unwrap();
    data.schema_version = 2;
    assert!(data.validate().is_err());
    data.schema_version = 1;
    data.apps[0].id = "builtin:operation".into();
    assert!(data.validate().is_err());
    data.apps[0].id = data.apps[1].id.clone();
    assert!(data.validate().is_err());
}

#[test]
fn test_apps_catalog_rejects_unsafe_targets_and_headers() {
    for url in ["http://example.com/mcp", "https://localhost/mcp", "https://127.0.0.1/mcp", "https://user:secret@example.com/mcp", "https://example.com/mcp?token=secret"] {
        assert!(catalog::public_https(url).is_err(), "{url}");
    }
    let mut data = catalog();
    data.apps[1].fields[0].header = Some("Host".into());
    assert!(data.validate().is_err());
    data.apps[1].fields[0].header = Some("X-Aipp-Credential-Ref".into());
    assert!(data.validate().is_err());
    data.apps[1].fields[0].header = Some("X-Api-Key".into());
    data.apps[1].fields[0].secret = false;
    assert!(data.validate().is_err());
}

#[test]
fn test_apps_configuration_separates_secrets_from_persisted_config() {
    let app = &catalog().apps[1];
    assert!(app.configuration(&HashMap::new()).is_err());
    let (config, headers) = app.configuration(&HashMap::from([("token".into(), "test-secret".into())])).unwrap();
    assert!(!serde_json::to_string(&config).unwrap().contains("test-secret"));
    assert_eq!(headers["authorization"], "Bearer test-secret");
    assert!(app.configuration(&HashMap::from([("token".into(), "test\r\nInjected: x".into())])).is_err());
}

#[test]
fn test_apps_oauth_callback_rejects_wrong_state_duplicate_code_and_denial() {
    assert_eq!(oauth::callback("GET /oauth/callback?state=expected&code=abc HTTP/1.1\r\n\r\n", "expected").unwrap(), "abc");
    for path in ["/oauth/callback?state=wrong&code=abc", "/oauth/callback?state=expected&code=a&code=b", "/oauth/callback?state=expected&state=expected&code=a", "/oauth/callback?state=expected&error=access_denied", "/other?state=expected&code=abc"] {
        assert!(oauth::callback(&format!("GET {path} HTTP/1.1\r\n\r\n"), "expected").is_err());
    }
}

#[test]
fn test_apps_credential_binding_rejects_endpoint_change() {
    let credential = credentials::Credential { server_id: 1, endpoint: "https://example.com/mcp".into(), headers: HashMap::new(), oauth: None };
    credentials::validate_binding(&credential, "https://example.com/mcp").unwrap();
    assert!(credentials::validate_binding(&credential, "https://other.example/mcp").is_err());
    assert!(credentials::validate_binding(&credential, "https://example.com/other").is_err());
}

#[test]
fn test_apps_metadata_keeps_existing_servers_and_enforces_single_instance() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE mcp_server(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO mcp_server VALUES(1,'legacy');").unwrap();
    init(&conn).unwrap();
    init(&conn).unwrap();
    assert_eq!(conn.query_row("SELECT name FROM mcp_server WHERE id=1", [], |r| r.get::<_,String>(0)).unwrap(), "legacy");
    conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status) VALUES(1,'request-1','notion','{}','workspace','{}','unchecked')", []).unwrap();
    assert!(conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status) VALUES(1,'request-2','notion','{}','workspace2','{}','unchecked')", []).is_err());
    conn.execute("DELETE FROM mcp_server WHERE id=1", []).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM app_connection", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
}

#[test]
fn test_apps_disconnected_connection_cannot_be_enabled_or_redirected_by_legacy_crud() {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    init(&db.conn).unwrap();
    let id = db.upsert_mcp_server_with_builtin("test", Some("test"), "http", None, None, Some("{}"), Some("https://example.com/mcp"), None, false, false, false, true, false).unwrap();
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status) VALUES(?1,'request-1','test','{}','test','{}','disconnected')", [id]).unwrap();
    assert!(db.toggle_mcp_server(id, true).is_err());
    assert!(db.upsert_mcp_server_with_builtin("test", Some("test"), "http", None, None, Some("{}"), Some("https://example.com/mcp"), None, false, true, false, true, false).is_err());
    assert!(db.update_mcp_server_with_builtin(id, "test", Some("test"), "http", None, None, Some("{}"), Some("https://other.example/mcp"), None, false, false, false, false).is_err());
    db.update_mcp_server_with_builtin(id, "renamed", Some("new description"), "http", None, None, Some("{}"), Some("https://example.com/mcp"), None, false, false, false, false).unwrap();
    assert!(!db.get_mcp_server(id).unwrap().is_enabled);
}

#[test]
fn test_apps_reference_is_not_sent_by_uncontrolled_transport_and_errors_are_redacted() {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let id = db.upsert_mcp_server_with_builtin("test", None, "http", None, None, Some(r#"{"x-aipp-credential-ref":"reference"}"#), Some("https://example.com/mcp"), None, false, true, false, true, false).unwrap();
    let server = db.get_mcp_server(id).unwrap();
    assert!(crate::mcp::http_transport::create_http_transport(&server, reqwest::Client::builder()).is_err());
    let redactor = crate::mcp::http_transport::ResponseRedactor::new(["Bearer test-secret".to_string()].into_iter());
    assert_eq!(redactor.clean("echo Bearer test-secret and test-secret"), "echo [REDACTED] and [REDACTED]");
}

/// Regression: every credential header value was redacted, so a non-secret header such as a read-only
/// switch sending "true" turned each `true` in tool schemas/results into [REDACTED].
#[test]
fn test_apps_redaction_covers_only_secret_headers() {
    use crate::mcp::http_transport::{secret_header_values, ResponseRedactor};
    let mut app = catalog().apps[1].clone();
    app.fields.push(serde_json::from_value(serde_json::json!({"key":"readonly","label":"只读","kind":"boolean","header":"X-MCP-Readonly","default":"true"})).unwrap());
    app.fields.push(serde_json::from_value(serde_json::json!({"key":"api_key","label":"API Key","kind":"password","secret":true,"header":"X-Api-Key"})).unwrap());
    AppCatalog { schema_version: 1, version: "1".into(), apps: vec![app.clone()] }.validate().unwrap();
    let (_, headers) = app.configuration(&HashMap::from([("token".into(), "tok-123456".into()), ("api_key".into(), "key-abcdef".into())])).unwrap();
    assert_eq!(headers["x-mcp-readonly"], "true");
    let names = app.secret_headers();
    assert_eq!(names, std::collections::HashSet::from(["authorization".to_string(), "x-api-key".to_string()]));
    let result = r#"{"readonly":true,"note":"tok-123456 key-abcdef"}"#;
    let scoped = ResponseRedactor::new(secret_header_values(&headers, Some(&names)).into_iter());
    assert_eq!(scoped.clean(result), r#"{"readonly":true,"note":"[REDACTED] [REDACTED]"}"#);
    // Unknown definition falls back to treating every credential header as secret.
    let fallback = ResponseRedactor::new(secret_header_values(&headers, None).into_iter());
    assert!(fallback.clean(result).contains("[REDACTED]"));
    assert!(!fallback.clean(result).contains("tok-123456"));
}

/// Regression: one entry using an auth kind this client does not know rejected the whole catalog, so
/// older clients fell back to a stale cache for every app.
#[test]
fn test_apps_catalog_skips_unsupported_entries_but_rejects_identity_conflicts() {
    let mut value: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/apps-catalog.json")).unwrap();
    let mut future = value["apps"][0].clone();
    future["id"] = "future".into(); future["name"] = "Future App".into(); future["connection"]["auth"] = "device_code".into();
    let malformed = serde_json::json!({"id":"broken","name":"Broken","connection":"not-an-object"});
    value["apps"].as_array_mut().unwrap().extend([future, malformed]);
    let (parsed, skipped) = AppCatalog::parse_lenient(value.clone()).unwrap();
    assert_eq!(parsed.apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["notion", "github"]);
    assert_eq!(skipped.len(), 2);
    assert!(skipped[0].starts_with("Future App") && skipped[1].starts_with("Broken"));
    let mut reserved = value.clone();
    reserved["apps"][2]["id"] = "builtin:operation".into();
    assert!(AppCatalog::parse_lenient(reserved).is_err());
    let mut duplicate = value;
    duplicate["apps"][2]["id"] = "notion".into();
    assert!(AppCatalog::parse_lenient(duplicate).is_err());
}

/// Regression: a create that failed after inserting rows left a stray "需要重新登录" connection behind.
#[test]
fn test_apps_failed_create_discards_only_rows_it_owns() {
    let db = MCPDatabase { conn: Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    init(&db.conn).unwrap();
    let id = db.upsert_mcp_server_with_builtin("new", None, "http", None, None, Some("{}"), Some("https://example.com/mcp"), None, false, false, false, true, false).unwrap();
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,status) VALUES(?1,'request-1','test','{}','test','{}','auth_required')", [id]).unwrap();
    discard_created(&db.conn, id, "request-2").unwrap();
    assert!(db.get_mcp_server(id).is_ok(), "a connection taken over by another request must stay");
    discard_created(&db.conn, id, "request-1").unwrap();
    assert!(db.get_mcp_server(id).is_err());
    assert_eq!(db.conn.query_row("SELECT COUNT(*) FROM app_connection", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_apps_credential_stage_classification_matches_aipp_generated_errors() {
    for error in ["OAuth 令牌交换/刷新失败：HTTP 401 Unauthorized；服务未提供可安全展示的错误代码", "OAuth 令牌交换/刷新失败：HTTP 400 Bad Request；invalid_grant：授权已失效，需要重新登录", "登录已过期，请重新授权"] {
        assert!(is_credential_auth_failure(error), "{error}");
    }
    // A credential missing on this device must not disable the connection: that state syncs and used to
    // ping-pong between a device with the credential and one without (no sync passphrase).
    for error in [credentials::MISSING_ON_DEVICE, "OAuth 令牌请求失败：error sending request", "读取连接凭据任务失败", "此连接启用了代理，但全局网络代理尚未配置"] {
        assert!(!is_credential_auth_failure(error), "{error}");
    }
}

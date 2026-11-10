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

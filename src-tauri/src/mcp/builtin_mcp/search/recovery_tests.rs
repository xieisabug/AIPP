use super::*;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE mcp_server (id INTEGER PRIMARY KEY, name TEXT, command TEXT, is_builtin INTEGER, environment_variables TEXT, timeout INTEGER);
        CREATE TABLE mcp_server_tool (server_id INTEGER, tool_name TEXT);
        INSERT INTO mcp_server VALUES (3,'search','aipp:search',1,'SEARCH_ENGINE=google',20000),
        (5,'kagi搜索工具','aipp:artifact',1,'SEARCH_ENGINE=kagi',60000),
        (13,'Artifact','aipp:artifact',1,NULL,20000),
        (14,'custom search','aipp:search',1,'SEARCH_ENGINE=bing',120000);
        INSERT INTO mcp_server_tool VALUES (5,'search_web'),(5,'fetch_url'),(5,'show_artifact'),(13,'show_artifact');").unwrap();
    conn
}

#[test]
fn test_search_recovery_repairs_kagi_without_touching_artifact_or_custom_timeout() {
    let conn = fixture();
    repair_search_servers(&conn).unwrap();
    repair_search_servers(&conn).unwrap();
    let command: String = conn.query_row("SELECT command FROM mcp_server WHERE id=5", [], |r| r.get(0)).unwrap();
    assert_eq!(command, "aipp:search");
    assert_eq!(conn.query_row("SELECT timeout FROM mcp_server WHERE id=3", [], |r| r.get::<_, i32>(0)).unwrap(), 60000);
    assert_eq!(conn.query_row("SELECT timeout FROM mcp_server WHERE id=14", [], |r| r.get::<_, i32>(0)).unwrap(), 120000);
    assert_eq!(conn.query_row("SELECT command FROM mcp_server WHERE id=13", [], |r| r.get::<_, String>(0)).unwrap(), "aipp:artifact");
    assert_eq!(conn.query_row("SELECT count(*) FROM mcp_server_tool WHERE server_id=5 AND tool_name='show_artifact'", [], |r| r.get::<_, i32>(0)).unwrap(), 0);
    assert_eq!(conn.query_row("SELECT count(*) FROM mcp_server_tool WHERE server_id=13", [], |r| r.get::<_, i32>(0)).unwrap(), 1);
}

#[test]
fn test_search_recovery_loads_selected_server_and_rejects_missing_server() {
    let conn = fixture();
    conn.execute("UPDATE mcp_server SET command='aipp:search' WHERE id=5", []).unwrap();
    assert_eq!(load_server_environment(&conn, Some(5)).unwrap().as_deref(), Some("SEARCH_ENGINE=kagi"));
    assert!(load_server_environment(&conn, Some(99)).is_err());
    assert!(load_server_environment(&conn, Some(13)).is_err());
}

#[test]
fn test_search_recovery_default_timeout_is_sixty_seconds() {
    assert_eq!(SEARCH_TIMEOUT_MS, 60_000);
}

#[test]
fn test_search_recovery_repairs_missing_kagi_engine_and_preserves_other_settings() {
    let conn = fixture();
    conn.execute("UPDATE mcp_server SET environment_variables='HEADLESS=false' WHERE id=5", []).unwrap();
    repair_search_servers(&conn).unwrap();
    let env = load_server_environment(&conn, Some(5)).unwrap().unwrap();
    assert!(env.lines().any(|line| line == "SEARCH_ENGINE=kagi"));
    assert!(env.lines().any(|line| line == "HEADLESS=false"));
}

#[test]
fn test_search_recovery_does_not_guess_unrelated_artifact_server() {
    let conn = fixture();
    conn.execute("UPDATE mcp_server SET name='custom Artifact',environment_variables=NULL WHERE id=5", []).unwrap();
    repair_search_servers(&conn).unwrap();
    let command: String = conn.query_row("SELECT command FROM mcp_server WHERE id=5", [], |r| r.get(0)).unwrap();
    assert_eq!(command, "aipp:artifact");
}

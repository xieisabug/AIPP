use crate::db::connection::Connection;

pub(crate) const SEARCH_TIMEOUT_MS: i32 = 60_000;

pub(crate) fn repair_search_servers(conn: &Connection) -> rusqlite::Result<()> {
    let result = conn.execute_batch(include_str!("repair_search_servers.sql"));
    if result.is_err() {
        let _ = conn.execute_batch("ROLLBACK TO repair_search_servers; RELEASE repair_search_servers;");
    }
    result
}

pub(crate) fn load_server_environment(conn: &Connection, server_id: Option<i64>) -> Result<Option<String>, String> {
    if let Some(id) = server_id {
        return conn.query_row(
            "SELECT environment_variables FROM mcp_server WHERE id=? AND command='aipp:search' AND is_builtin=1",
            [id], |row| row.get(0),
        ).map_err(|e| format!("Search configuration lookup failed (server_id={id}): {e}"));
    }
    // Direct calls without a server identity are only unambiguous with one search server.
    let mut statement = conn.prepare("SELECT environment_variables FROM mcp_server WHERE command='aipp:search' AND is_builtin=1 ORDER BY id")
        .map_err(|e| format!("Search configuration lookup failed: {e}"))?;
    let values = statement.query_map([], |row| row.get::<_, Option<String>>(0))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    match values.as_slice() {
        [value] => Ok(value.clone()),
        [] => Err("Search configuration lookup failed: no search server configured".into()),
        _ => Err("Search configuration lookup failed: multiple search servers; server_id is required".into()),
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod recovery_tests;

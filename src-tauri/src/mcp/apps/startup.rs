use super::{db, credentials, CHANGE_LOCK, SESSION};
use crate::db::connection::{params, Connection, OptionalExtension};
use crate::db::mcp_db::{MCPDatabase, MCPServer};
use tauri::Emitter;

fn candidates(conn: &Connection) -> Result<Vec<i64>, String> {
    let mut stmt = conn.prepare("SELECT c.server_id FROM app_connection c JOIN mcp_server s ON s.id=c.server_id
        WHERE c.status='available' AND s.is_enabled=1 AND c.verified_session IS NOT ?1 ORDER BY c.server_id")
        .map_err(|e| e.to_string())?;
    let ids = stmt.query_map([SESSION.as_str()], |row| row.get(0)).map_err(|e| e.to_string())?;
    ids.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

struct PreviousCheck {
    server: MCPServer,
    request_id: String,
    session: Option<String>,
    credential: Option<String>,
}

impl PreviousCheck {
    fn read(database: &MCPDatabase, id: i64) -> Result<Option<Self>, String> {
        let snapshot = database.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let row = snapshot.query_row("SELECT request_id,verified_session,credential_ref FROM app_connection
            WHERE server_id=?1 AND status='available' AND verified_session IS NOT ?2",
            params![id, SESSION.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional().map_err(|e| e.to_string())?;
        let Some((request_id, session, credential)) = row else { return Ok(None); };
        let server = database.get_mcp_server(id).map_err(|e| e.to_string())?;
        Ok(server.is_enabled.then_some(Self { server, request_id, session, credential }))
    }

    fn record_failure(&self, database: &MCPDatabase, error: &str) -> Result<bool, String> {
        let tx = database.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        if database.get_mcp_server(self.server.id).map_err(|e| e.to_string())? != self.server {
            return Ok(false);
        }
        let changed = tx.execute("UPDATE app_connection SET status='error', error=?1, verified_session=?2
            WHERE server_id=?3 AND request_id=?4 AND verified_session IS ?5 AND credential_ref IS ?6 AND status='available'",
            params![error, SESSION.as_str(), self.server.id, self.request_id, self.session, self.credential])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(changed > 0)
    }
}

async fn recheck(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    let handle = app.clone();
    let check = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let check = PreviousCheck::read(&db(&handle)?, id)?;
        if let Some(reference) = check.as_ref().and_then(|check| check.credential.as_ref()) {
            if !crate::sync::vault::stored_references(&handle)?.contains(reference) { return Ok(None); }
        }
        Ok(check)
    }).await.map_err(|e| e.to_string())??;
    let Some(check) = check else { return Ok(()); };
    if let Err(error) = super::super::registry_api::refresh_mcp_server_capabilities(app.clone(), id).await {
        // Authentication rejection is recorded by the transport. Missing credentials are device-local.
        if error.contains(credentials::MISSING_ON_DEVICE) { return Ok(()); }
        let _change = CHANGE_LOCK.lock().await;
        if check.record_failure(&db(app)?, &error)? {
            let _ = app.emit("mcp_state_changed", ());
        }
    }
    Ok(())
}

pub fn recheck_available_connections(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        match db(&app).and_then(|database| candidates(&database.conn)) {
            Ok(ids) => for id in ids {
                if let Err(error) = recheck(&app, id).await {
                    tracing::warn!(server_id=id, %error, "Could not recheck app connection at startup");
                }
            },
            Err(error) => tracing::warn!(%error, "Could not list previously available app connections"),
        }
    });
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;

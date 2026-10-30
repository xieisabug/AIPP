//! Recoverable commits across business databases and sync.db (including WAL).
//! Business writes and their metadata redo log commit first. A sync receipt in
//! the metadata transaction makes recovery idempotent if either commit/cleanup
//! is interrupted. No cross-file atomicity is assumed.
use super::*;

const JOURNAL_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS aipp_sync_apply_journal (
    id TEXT PRIMARY KEY, redo_sql TEXT NOT NULL)";

fn prepare_capture(sync: &Connection) -> Result<(), String> {
    let installed: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE name='sync_apply_capture_ready')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if installed {
        sync.execute_batch("DELETE FROM sync_apply_redo; INSERT INTO sync_apply_capture_active VALUES (1);").map_err(|e| e.to_string())?;
        return Ok(());
    }
    sync.execute_batch("SAVEPOINT install_sync_capture").map_err(|e| e.to_string())?;
    let result = (|| {
    sync.execute_batch("CREATE TEMP TABLE sync_apply_redo (sql TEXT NOT NULL);
        CREATE TEMP TABLE sync_apply_capture_active (enabled INTEGER);").map_err(|e| e.to_string())?;
    // Only client-owned metadata tables. quote() encodes values locally; no
    // server-provided identifiers or executable SQL enter the redo log.
    for table in ["sync_object_map", "sync_object_alias", "sync_branch_route", "sync_shadow", "sync_natural_key", "sync_outbox",
        "sync_secret_pending", "sync_identity_origin", "sync_merge_redirect"] {
        let mut stmt = sync.prepare(&format!("PRAGMA table_info({table})")).map_err(|e| e.to_string())?;
        let columns = stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))
            .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
        let names = columns.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(",");
        let values = columns.iter().map(|(name, _)| format!("quote(NEW.{name})")).collect::<Vec<_>>().join(" || ',' || ");
        let key = columns.iter().filter(|(_, pk)| *pk > 0)
            .map(|(name, _)| format!("'{name} IS ' || quote(OLD.{name})")).collect::<Vec<_>>().join(" || ' AND ' || ");
        for action in ["INSERT", "UPDATE", "DELETE"] {
            let delete = format!("'DELETE FROM {table} WHERE ' || {key} || ';'");
            let insert = format!("'INSERT OR REPLACE INTO {table} ({names}) VALUES (' || {values} || ');'");
            let sql_expr = match action {
                "DELETE" => delete,
                "UPDATE" => format!("{delete} || {insert}"),
                _ => insert,
            };
            sync.execute_batch(&format!("CREATE TEMP TRIGGER IF NOT EXISTS capture_{table}_{action}
                AFTER {action} ON main.{table} WHEN EXISTS (SELECT 1 FROM sync_apply_capture_active) BEGIN INSERT INTO sync_apply_redo VALUES ({sql_expr}); END;"))
                .map_err(|e| e.to_string())?;
        }
    }
    sync.execute_batch("CREATE TEMP TABLE sync_apply_capture_ready (ready INTEGER); INSERT INTO sync_apply_capture_active VALUES (1)").map_err(|e| e.to_string())?;
    Ok(())
    })();
    finish_savepoint(sync, "install_sync_capture", result)
}

struct CaptureGuard<'a>(&'a Connection);
impl Drop for CaptureGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.execute_batch("DELETE FROM sync_apply_capture_active; DELETE FROM sync_apply_redo;");
    }
}

pub(super) fn recover(sync: &Connection, data: &Connection) -> Result<(), String> {
    if !sync.is_autocommit() || !data.is_autocommit() {
        return Err("同步恢复必须在独立事务中执行".into());
    }
    ensure_durable(data)?;
    data.execute_batch(JOURNAL_SCHEMA).map_err(|e| e.to_string())?;
    let rows = data.prepare("SELECT id, redo_sql FROM aipp_sync_apply_journal ORDER BY rowid")
        .map_err(|e| e.to_string())?.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (id, redo) in rows {
        sync.execute_batch("SAVEPOINT recover_sync_apply").map_err(|e| e.to_string())?;
        let result = (|| {
            let done: bool = sync.query_row("SELECT EXISTS(SELECT 1 FROM sync_apply_receipt WHERE id=?1)", params![id], |r| r.get(0)).map_err(|e| e.to_string())?;
            if !done {
                sync.execute_batch(&redo).map_err(|e| e.to_string())?;
                sync.execute("INSERT INTO sync_apply_receipt VALUES (?1)", params![id]).map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(())
        })();
        finish_savepoint(sync, "recover_sync_apply", result)?;
        cleanup(sync, data, &id)?;
    }
    Ok(())
}

fn cleanup(sync: &Connection, data: &Connection, id: &str) -> Result<(), String> {
    // Delete the business journal durably before forgetting the sync receipt.
    data.execute("DELETE FROM aipp_sync_apply_journal WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    sync.execute("DELETE FROM sync_apply_receipt WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn coordinated<T>(sync: &Connection, data: &Connection, apply: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if !sync.is_autocommit() || !data.is_autocommit() {
        return Err("同步跨库写入必须在独立事务中执行".into());
    }
    // FULL also covers journal cleanup ordering under process/power failure.
    ensure_durable(data)?;
    recover(sync, data)?;
    prepare_capture(sync)?;
    let _capture = CaptureGuard(sync);
    sync.execute_batch("SAVEPOINT coordinated_sync_apply").map_err(|e| e.to_string())?;
    if let Err(error) = data.execute_batch("SAVEPOINT coordinated_data_apply") {
        return finish_savepoint(sync, "coordinated_sync_apply", Err(error.to_string()));
    }
    let id = Uuid::new_v4().to_string();
    let prepared = (|| {
        let value = apply()?;
        let redo = sync.prepare("SELECT sql FROM sync_apply_redo ORDER BY rowid").map_err(|e| e.to_string())?
            .query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?.join("\n");
        data.execute("INSERT INTO aipp_sync_apply_journal VALUES (?1, ?2)", params![id, redo]).map_err(|e| e.to_string())?;
        sync.execute("INSERT INTO sync_apply_receipt VALUES (?1)", params![id]).map_err(|e| e.to_string())?;
        Ok(value)
    })();
    let value = match prepared {
        Ok(value) => value,
        Err(error) => {
            let result = finish_savepoint(data, "coordinated_data_apply", Err(error));
            return finish_savepoint(sync, "coordinated_sync_apply", result);
        }
    };
    if let Err(error) = data.execute_batch("RELEASE coordinated_data_apply") {
        let result = finish_savepoint(data, "coordinated_data_apply", Err(format!("提交同步业务数据失败: {error}")));
        return finish_savepoint(sync, "coordinated_sync_apply", result);
    }
    // If this fails, committed business data carries the redo journal. Stop;
    // recover it before the next scan/apply rather than inserting another row.
    finish_savepoint(sync, "coordinated_sync_apply", Ok(()))?;
    cleanup(sync, data, &id)?;
    Ok(value)
}

fn ensure_durable(data: &Connection) -> Result<(), String> {
    let current: i64 = data.query_row("PRAGMA synchronous", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if current < 2 { data.execute_batch("PRAGMA synchronous=FULL").map_err(|e| e.to_string())?; }
    Ok(())
}

pub(super) fn finish_savepoint<T>(conn: &Connection, name: &str, result: Result<T, String>) -> Result<T, String> {
    match result {
        Ok(value) => match conn.execute_batch(&format!("RELEASE {name}")) {
            Ok(()) => Ok(value),
            Err(error) => {
                finish_savepoint(conn, name, Err(format!("提交同步事务 {name} 失败: {error}")))
            }
        },
        Err(error) => {
            let rollback = conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
            match rollback { Ok(()) => Err(error), Err(other) => Err(format!("{error}; rollback: {other}")) }
        }
    }
}

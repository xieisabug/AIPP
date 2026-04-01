use crate::db::connection::{self, params, params_from_iter, Connection, Value};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use super::types::RiskLevel;

// ---------------------------------------------------------------------------
// Audit log entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub audit_id: String,
    pub action_id: String,
    pub domain: String,
    pub risk_level: u8,
    pub args_json: Option<String>,
    pub reason: Option<String>,
    pub dry_run: bool,
    pub approval_used: bool,
    pub success: bool,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub butler_conversation_id: Option<i64>,
    pub source: String,
    pub created_time: String,
    /// Snapshot of the entity state *before* the mutation (for undo support).
    pub before_snapshot_json: Option<String>,
    /// Whether this action has been undone.
    pub is_undone: bool,
    /// If undone, the audit_id of the undo operation that reversed this.
    pub undo_audit_id: Option<String>,
}

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

pub fn create_audit_table(conn: &Connection) -> connection::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS superadmin_audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            audit_id TEXT NOT NULL UNIQUE,
            action_id TEXT NOT NULL,
            domain TEXT NOT NULL,
            risk_level INTEGER NOT NULL,
            args_json TEXT,
            reason TEXT,
            dry_run INTEGER NOT NULL DEFAULT 0,
            approval_used INTEGER NOT NULL DEFAULT 0,
            success INTEGER NOT NULL,
            result_json TEXT,
            error TEXT,
            butler_conversation_id INTEGER,
            source TEXT DEFAULT 'butler',
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            before_snapshot_json TEXT,
            is_undone INTEGER NOT NULL DEFAULT 0,
            undo_audit_id TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_superadmin_audit_action
            ON superadmin_audit_log(action_id);
        CREATE INDEX IF NOT EXISTS idx_superadmin_audit_domain
            ON superadmin_audit_log(domain);
        CREATE INDEX IF NOT EXISTS idx_superadmin_audit_time
            ON superadmin_audit_log(created_time);",
    )
}

/// Migrate the audit table to add snapshot/undo columns if they don't exist.
pub fn migrate_audit_table(conn: &Connection) -> connection::Result<()> {
    // Check if columns exist by querying table info
    let has_snapshot: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('superadmin_audit_log') WHERE name = 'before_snapshot_json'")?
        .query_row((), |row| row.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_snapshot {
        conn.execute_batch(
            "ALTER TABLE superadmin_audit_log ADD COLUMN before_snapshot_json TEXT;
             ALTER TABLE superadmin_audit_log ADD COLUMN is_undone INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE superadmin_audit_log ADD COLUMN undo_audit_id TEXT;",
        )?;
    }
    Ok(())
}

pub fn generate_audit_id() -> String {
    Uuid::new_v4().to_string()
}

#[instrument(skip(conn, args_json, result_json, before_snapshot_json))]
pub fn insert_audit_log(
    conn: &Connection,
    audit_id: &str,
    action_id: &str,
    domain: &str,
    risk_level: RiskLevel,
    args_json: Option<&str>,
    reason: Option<&str>,
    dry_run: bool,
    approval_used: bool,
    success: bool,
    result_json: Option<&str>,
    error: Option<&str>,
    butler_conversation_id: Option<i64>,
    source: &str,
    before_snapshot_json: Option<&str>,
) -> connection::Result<i64> {
    conn.execute(
        "INSERT INTO superadmin_audit_log
            (audit_id, action_id, domain, risk_level, args_json, reason,
             dry_run, approval_used, success, result_json, error,
             butler_conversation_id, source, before_snapshot_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            audit_id,
            action_id,
            domain,
            risk_level.0,
            args_json,
            reason,
            dry_run as i32,
            approval_used as i32,
            success as i32,
            result_json,
            error,
            butler_conversation_id,
            source,
            before_snapshot_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Mark an audit entry as undone.
pub fn mark_audit_undone(
    conn: &Connection,
    original_audit_id: &str,
    undo_audit_id: &str,
) -> connection::Result<()> {
    conn.execute(
        "UPDATE superadmin_audit_log SET is_undone = 1, undo_audit_id = ?1 WHERE audit_id = ?2",
        params![undo_audit_id, original_audit_id],
    )?;
    Ok(())
}

/// Get a single audit entry by audit_id.
pub fn get_audit_entry(
    conn: &Connection,
    audit_id: &str,
) -> connection::Result<Option<AuditLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, audit_id, action_id, domain, risk_level, args_json, reason,
                dry_run, approval_used, success, result_json, error,
                butler_conversation_id, source, created_time,
                before_snapshot_json, is_undone, undo_audit_id
         FROM superadmin_audit_log WHERE audit_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![audit_id], map_audit_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn query_audit_log(
    conn: &Connection,
    action_id: Option<&str>,
    domain: Option<&str>,
    success_only: Option<bool>,
    undoable_only: bool,
    limit: usize,
    offset: usize,
) -> connection::Result<Vec<AuditLogEntry>> {
    let mut sql = String::from(
        "SELECT id, audit_id, action_id, domain, risk_level, args_json, reason,
                dry_run, approval_used, success, result_json, error,
                butler_conversation_id, source, created_time,
                before_snapshot_json, is_undone, undo_audit_id
         FROM superadmin_audit_log WHERE 1=1",
    );
    let mut param_values: Vec<Value> = Vec::new();

    if let Some(aid) = action_id {
        sql.push_str(" AND action_id = ?");
        param_values.push(Value::from(aid.to_string()));
    }
    if let Some(d) = domain {
        sql.push_str(" AND domain = ?");
        param_values.push(Value::from(d.to_string()));
    }
    if let Some(s) = success_only {
        sql.push_str(" AND success = ?");
        param_values.push(Value::from(s as i32));
    }
    if undoable_only {
        // Only show successful write ops with snapshots that haven't been undone
        sql.push_str(" AND success = 1 AND is_undone = 0 AND before_snapshot_json IS NOT NULL AND dry_run = 0");
    }
    sql.push_str(" ORDER BY created_time DESC LIMIT ? OFFSET ?");
    param_values.push(Value::from(limit as i64));
    param_values.push(Value::from(offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(param_values), map_audit_row)?;
    rows.collect()
}

fn map_audit_row(row: &connection::Row) -> connection::Result<AuditLogEntry> {
    Ok(AuditLogEntry {
        id: row.get(0)?,
        audit_id: row.get(1)?,
        action_id: row.get(2)?,
        domain: row.get(3)?,
        risk_level: row.get::<_, i64>(4)? as u8,
        args_json: row.get(5)?,
        reason: row.get(6)?,
        dry_run: row.get::<_, i64>(7)? != 0,
        approval_used: row.get::<_, i64>(8)? != 0,
        success: row.get::<_, i64>(9)? != 0,
        result_json: row.get(10)?,
        error: row.get(11)?,
        butler_conversation_id: row.get(12)?,
        source: row.get::<_, Option<String>>(13)?.unwrap_or_else(|| "butler".to_string()),
        created_time: row.get(14)?,
        before_snapshot_json: row.get(15)?,
        is_undone: row.get::<_, i64>(16).unwrap_or(0) != 0,
        undo_audit_id: row.get(17)?,
    })
}

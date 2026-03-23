use rusqlite::{params, Connection};
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
}

// ---------------------------------------------------------------------------
// DB helpers
// ---------------------------------------------------------------------------

pub fn create_audit_table(conn: &Connection) -> rusqlite::Result<()> {
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
            created_time DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_superadmin_audit_action
            ON superadmin_audit_log(action_id);
        CREATE INDEX IF NOT EXISTS idx_superadmin_audit_domain
            ON superadmin_audit_log(domain);
        CREATE INDEX IF NOT EXISTS idx_superadmin_audit_time
            ON superadmin_audit_log(created_time);",
    )
}

pub fn generate_audit_id() -> String {
    Uuid::new_v4().to_string()
}

#[instrument(skip(conn, args_json, result_json))]
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
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO superadmin_audit_log
            (audit_id, action_id, domain, risk_level, args_json, reason,
             dry_run, approval_used, success, result_json, error,
             butler_conversation_id, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn query_audit_log(
    conn: &Connection,
    action_id: Option<&str>,
    domain: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<AuditLogEntry>> {
    let mut sql = String::from(
        "SELECT id, audit_id, action_id, domain, risk_level, args_json, reason,
                dry_run, approval_used, success, result_json, error,
                butler_conversation_id, source, created_time
         FROM superadmin_audit_log WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(aid) = action_id {
        sql.push_str(" AND action_id = ?");
        param_values.push(Box::new(aid.to_string()));
    }
    if let Some(d) = domain {
        sql.push_str(" AND domain = ?");
        param_values.push(Box::new(d.to_string()));
    }
    sql.push_str(" ORDER BY created_time DESC LIMIT ?");
    param_values.push(Box::new(limit as i64));

    let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
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
        })
    })?;

    rows.collect()
}

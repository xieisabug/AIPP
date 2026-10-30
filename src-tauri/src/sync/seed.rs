//! Rows every fresh install creates by itself (quick assistant defaults,
//! built-in MCP tool list, default summary settings).
//!
//! They are not user data: a device that only has them may join an existing
//! account by pulling, instead of being treated as a populated device that
//! lost its sync baseline. A seed row edited by the user is no longer pristine
//! and counts as real data again.
use super::*;
use crate::db::assistant_db::{DEFAULT_QUICK_ASSISTANT_MODEL_CONFIGS, DEFAULT_QUICK_ASSISTANT_PROMPT};
use crate::db::system_db::{DEFAULT_SUMMARY_LENGTH, DEFAULT_SUMMARY_PROMPT};

fn lit(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// SQL condition matching the pristine seed rows of `table`, if it has any.
/// Official/built-in parents are already excluded through `official_column` /
/// `seed_official_ids`; these are the seeded rows without such a flag.
pub(super) fn pristine_condition(table: &str) -> Option<String> {
    match table {
        "assistant_prompt" => Some(format!("assistant_id = 1 AND prompt = {}", lit(DEFAULT_QUICK_ASSISTANT_PROMPT))),
        "assistant_model" => Some("assistant_id = 1 AND provider_id = 0 AND model_code = '' AND COALESCE(alias, '') = ''".into()),
        "assistant_model_config" => {
            let pairs = DEFAULT_QUICK_ASSISTANT_MODEL_CONFIGS
                .iter()
                .map(|(name, value, _)| format!("({}, {})", lit(name), lit(value)))
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!("assistant_id = 1 AND (name, value) IN (VALUES {pairs})"))
        }
        // Built-in tool definitions are rewritten on every start; only the user's
        // per-tool switches are data.
        "mcp_server_tool" => Some(
            "is_enabled = 1 AND is_auto_run = 0 AND server_id IN (SELECT id FROM mcp_server WHERE COALESCE(is_builtin, 0) = 1)".into(),
        ),
        "feature_config" => Some(format!(
            "feature_code = 'conversation_summary' AND ((key = 'summary_length' AND value = {}) OR (key = 'prompt' AND value = {}))",
            lit(DEFAULT_SUMMARY_LENGTH),
            lit(DEFAULT_SUMMARY_PROMPT)
        )),
        _ => None,
    }
}

/// Number of rows of `spec` that are user data (not official, not a pristine seed).
pub(super) fn user_row_count(conn: &Connection, spec: &SyncTableSpec) -> Result<i64, String> {
    let mut conditions: Vec<String> = spec.where_clause.map(|w| format!("({w})")).into_iter().collect();
    if let Some(column) = spec.official_column {
        conditions.push(format!("COALESCE({column}, 0) = 0"));
    }
    if !spec.seed_official_ids.is_empty() {
        let ids = spec.seed_official_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(", ");
        conditions.push(format!("id NOT IN ({ids})"));
    }
    if let Some(seed) = pristine_condition(spec.table) {
        conditions.push(format!("NOT ({seed})"));
    }
    let where_sql = if conditions.is_empty() { String::new() } else { format!(" WHERE {}", conditions.join(" AND ")) };
    conn.query_row(&format!("SELECT COUNT(*) FROM {}{}", spec.table, where_sql), [], |row| row.get(0))
        .map_err(|e| e.to_string())
}

/// True when every synced table holds only official rows and pristine seeds.
pub(super) fn local_scope_is_empty(
    mut open: impl FnMut(&str) -> Result<Connection, String>,
) -> Result<bool, String> {
    for spec in specs() {
        let conn = open(spec.db_name)?;
        if user_row_count(&conn, &spec)? > 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// After joining, the quick assistant's pulled prompt replaces the local
/// default one. Only a never-synced pristine seed is removed, and only when a
/// synced prompt for the quick assistant exists; `init_assistant` re-creates
/// the default whenever no prompt is left.
pub(super) fn drop_superseded_seeds(sync: &Connection, assistant: &Connection) -> Result<usize, String> {
    let synced_ids = |conn: &Connection| -> Result<Vec<i64>, String> {
        conn.prepare(
            "SELECT local_id FROM sync_object_map WHERE object_type = 'assistant.prompt'
             UNION SELECT local_id FROM sync_object_alias WHERE object_type = 'assistant.prompt'",
        )
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
    };
    let synced = synced_ids(sync)?;
    let quick_prompts: Vec<(i64, bool)> = assistant
        .prepare(&format!(
            "SELECT id, CASE WHEN {} THEN 1 ELSE 0 END FROM assistant_prompt WHERE assistant_id = 1",
            pristine_condition("assistant_prompt").unwrap()
        ))
        .map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? == 1)))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    if !quick_prompts.iter().any(|(id, _)| synced.contains(id)) {
        return Ok(0);
    }
    let mut dropped = 0;
    for (id, pristine) in quick_prompts {
        if pristine && !synced.contains(&id) {
            dropped += assistant
                .execute("DELETE FROM assistant_prompt WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(dropped)
}

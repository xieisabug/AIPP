use serde_json::json;
use tauri::AppHandle;
use tracing::error;

use super::audit;
use crate::db::conversation_db::ConversationDatabase;

/// Handle the `superadmin_audit_query` tool call.
///
/// Allows querying the audit log with filters:
/// - action_id: filter by specific action
/// - domain: filter by domain
/// - success_only: only show successful actions
/// - undoable_only: only show actions that can be undone
/// - limit: max results (default 20, max 100)
/// - offset: pagination offset
pub async fn handle_audit_query(
    app_handle: &AppHandle,
    args: &serde_json::Value,
) -> serde_json::Value {
    let action_id = args.get("action_id").and_then(|v| v.as_str());
    let domain = args.get("domain").and_then(|v| v.as_str());
    let success_only = args.get("success_only").and_then(|v| v.as_bool());
    let undoable_only = args.get("undoable_only").and_then(|v| v.as_bool()).unwrap_or(false);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20).min(100) as usize;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    // Open DB
    let db = match ConversationDatabase::new(app_handle) {
        Ok(db) => db,
        Err(e) => {
            error!(error = %e, "Failed to open DB for audit query");
            return error_result(&format!("Failed to open database: {}", e));
        }
    };
    let conn = match db.get_connection() {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Failed to get connection for audit query");
            return error_result(&format!("Failed to get connection: {}", e));
        }
    };

    // Query
    match audit::query_audit_log(
        &conn,
        action_id,
        domain,
        success_only,
        undoable_only,
        limit,
        offset,
    ) {
        Ok(entries) => {
            let results: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    let mut entry = json!({
                        "audit_id": e.audit_id,
                        "action_id": e.action_id,
                        "domain": e.domain,
                        "risk_level": e.risk_level,
                        "success": e.success,
                        "dry_run": e.dry_run,
                        "created_time": e.created_time,
                        "is_undone": e.is_undone,
                    });
                    if let Some(ref reason) = e.reason {
                        entry["reason"] = json!(reason);
                    }
                    if let Some(ref err) = e.error {
                        entry["error"] = json!(err);
                    }
                    if e.is_undone {
                        if let Some(ref uid) = e.undo_audit_id {
                            entry["undo_audit_id"] = json!(uid);
                        }
                    }
                    // For undoable entries, indicate presence of snapshot
                    if e.before_snapshot_json.is_some() {
                        entry["has_snapshot"] = json!(true);
                    }
                    // Include a brief summary of args (not full JSON to save tokens)
                    if let Some(ref args_str) = e.args_json {
                        if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(args_str) {
                            // Include compact version for readability
                            let compact = args_val.to_string();
                            if compact.len() <= 200 {
                                entry["args_preview"] = args_val;
                            } else {
                                entry["args_preview"] = json!(format!("{}...", &compact[..197]));
                            }
                        }
                    }
                    // Include result preview for successful actions
                    if e.success {
                        if let Some(ref result_str) = e.result_json {
                            if let Ok(result_val) =
                                serde_json::from_str::<serde_json::Value>(result_str)
                            {
                                let compact = result_val.to_string();
                                if compact.len() <= 200 {
                                    entry["result_preview"] = result_val;
                                } else {
                                    entry["result_preview"] =
                                        json!(format!("{}...", &compact[..197]));
                                }
                            }
                        }
                    }
                    entry
                })
                .collect();

            let has_more = results.len() == limit;

            json!({
                "content": [{"type": "json", "json": {
                    "entries": results,
                    "count": entries.len(),
                    "offset": offset,
                    "limit": limit,
                    "has_more": has_more,
                }}],
                "isError": false
            })
        }
        Err(e) => {
            error!(error = %e, "Failed to query audit log");
            error_result(&format!("Failed to query audit log: {}", e))
        }
    }
}

fn error_result(message: &str) -> serde_json::Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

pub async fn handle_scoped_query(app:&AppHandle,args:&serde_json::Value,conversation:i64) -> serde_json::Value {
    let app = app.clone();
    let args = args.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
        let conn = ConversationDatabase::new(&app)?.get_connection()?;
        crate::db::butler_group::scope(&conn,conversation)?;
        let limit = args["limit"].as_i64().unwrap_or(20).clamp(1,100);
        let offset = args["offset"].as_i64().unwrap_or(0).max(0);
        let mut stmt = conn.prepare("SELECT audit_id FROM superadmin_audit_log WHERE butler_conversation_id=?1
            AND (?2 IS NULL OR action_id=?2) AND (?3 IS NULL OR domain=?3)
            AND (?4=0 OR success=1) AND (?5=0 OR (success=1 AND dry_run=0 AND is_undone=0 AND before_snapshot_json IS NOT NULL))
            ORDER BY id DESC LIMIT ?6 OFFSET ?7")?;
        let ids = stmt.query_map(rusqlite::params![conversation,args["action_id"].as_str(),args["domain"].as_str(),
            args["success_only"].as_bool().unwrap_or(false),args["undoable_only"].as_bool().unwrap_or(false),limit+1,offset],|r|r.get::<_,String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = ids.len() > limit as usize;
        let entries = ids.iter().take(limit as usize).map(|id|audit::get_audit_entry(&conn,id)).collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"entries":entries,"has_more":more,"conversation_id":conversation}))
    }).await;
    match result {
        Ok(Ok(value)) => json!({"content":[{"type":"json","json":value}],"isError":false}),
        Ok(Err(error)) => error_result(&format!("群审计读取失败：{error:#}")),
        Err(error) => error_result(&format!("群审计读取调度失败：{error}")),
    }
}

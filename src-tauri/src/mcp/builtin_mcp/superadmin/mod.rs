pub mod actions;
pub mod audit;
pub mod audit_query;
pub mod batch;
pub mod catalog;
pub mod execute;
pub mod inspect;
pub mod registry;
pub mod types;
pub mod undo;

#[cfg(test)]
mod tests;

use std::sync::LazyLock;
use serde_json::json;
use tauri::AppHandle;
use tracing::{debug, error};

use registry::ActionRegistry;
use types::*;

// ---------------------------------------------------------------------------
// Global singleton registry (built once, read many)
// ---------------------------------------------------------------------------

static REGISTRY: LazyLock<ActionRegistry> = LazyLock::new(|| {
    debug!("Initializing SuperAdmin action registry (lazy)");
    registry::build_registry()
});

/// Ensure the audit table exists. Call from init_builtin_mcp_servers.
pub fn init_superadmin_tables(app_handle: &AppHandle) {
    use crate::db::conversation_db::ConversationDatabase;

    match ConversationDatabase::new(app_handle) {
        Ok(db) => match db.get_connection() {
            Ok(conn) => {
                if let Err(e) = audit::create_audit_table(&conn) {
                    error!(error = %e, "Failed to create superadmin_audit_log table");
                }
                // Migrate existing tables to add snapshot/undo columns
                if let Err(e) = audit::migrate_audit_table(&conn) {
                    error!(error = %e, "Failed to migrate superadmin_audit_log table");
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to get connection for superadmin init");
            }
        },
        Err(e) => {
            error!(error = %e, "Failed to open conversation DB for superadmin init");
        }
    }
}

// ---------------------------------------------------------------------------
// Main dispatch – called from execute_aipp_builtin_tool
// ---------------------------------------------------------------------------

/// Dispatch a superadmin tool call.
/// `tool_name` is one of: superadmin_catalog, superadmin_inspect,
/// superadmin_execute, superadmin_batch, superadmin_undo, superadmin_audit_query.
/// `args` is the parsed JSON parameters.
/// `conversation_id` is the butler's conversation id (for audit).
pub async fn dispatch(
    app_handle: &AppHandle,
    tool_name: &str,
    args: &serde_json::Value,
    conversation_id: Option<i64>,
) -> serde_json::Value {
    let registry = &*REGISTRY;

    match tool_name {
        "superadmin_catalog" => {
            let request: CatalogRequest = match serde_json::from_value(args.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return error_response(&format!("Invalid catalog parameters: {}", e));
                }
            };
            let response = catalog::handle_catalog(registry, &request);
            json_response(&serde_json::to_value(response).unwrap_or(json!({})))
        }

        "superadmin_inspect" => {
            let action_id = match args.get("action_id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => return error_response("Missing required parameter: action_id"),
            };
            match inspect::handle_inspect(registry, action_id) {
                Ok(response) => {
                    json_response(&serde_json::to_value(response).unwrap_or(json!({})))
                }
                Err(e) => error_response(&e),
            }
        }

        "superadmin_execute" => {
            let request: ExecuteRequest = match serde_json::from_value(args.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return error_response(&format!("Invalid execute parameters: {}", e));
                }
            };
            let result =
                execute::handle_execute(app_handle, registry, request, conversation_id).await;
            if result.success {
                json_response(&serde_json::to_value(&result).unwrap_or(json!({})))
            } else {
                // Return structured result even on failure so the AI can read the error
                let val = serde_json::to_value(&result).unwrap_or(json!({}));
                json!({
                    "content": [{"type": "json", "json": val}],
                    "isError": true
                })
            }
        }

        "superadmin_batch" => {
            let request: BatchRequest = match serde_json::from_value(args.clone()) {
                Ok(r) => r,
                Err(e) => {
                    return error_response(&format!("Invalid batch parameters: {}", e));
                }
            };
            let result =
                batch::handle_batch(app_handle, registry, request, conversation_id).await;
            let is_error = !result.all_succeeded;
            let val = serde_json::to_value(&result).unwrap_or(json!({}));
            json!({
                "content": [{"type": "json", "json": val}],
                "isError": is_error
            })
        }

        "superadmin_undo" => {
            let audit_id = match args.get("audit_id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => return error_response("Missing required parameter: audit_id"),
            };
            let reason = args.get("reason").and_then(|v| v.as_str());
            undo::handle_undo(app_handle, registry, audit_id, reason, conversation_id).await
        }

        "superadmin_audit_query" => {
            audit_query::handle_audit_query(app_handle, args).await
        }

        _ => error_response(&format!("Unknown superadmin tool: {}", tool_name)),
    }
}

// ---------------------------------------------------------------------------
// Response helpers (match builtin MCP response format)
// ---------------------------------------------------------------------------

fn json_response(value: &serde_json::Value) -> serde_json::Value {
    json!({
        "content": [{"type": "json", "json": value}],
        "isError": false
    })
}

fn error_response(message: &str) -> serde_json::Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

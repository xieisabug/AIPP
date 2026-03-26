use tauri::AppHandle;
use tracing::{debug, error};

use super::execute::handle_execute;
use super::registry::ActionRegistry;
use super::types::*;

/// Handle the `superadmin_batch` tool call.
/// Executes actions sequentially. If `stop_on_error` is true (default),
/// stops at the first failure. The previous step's result is available
/// to the caller for composing subsequent args.
pub async fn handle_batch(
    app_handle: &AppHandle,
    registry: &ActionRegistry,
    request: BatchRequest,
    butler_conversation_id: Option<i64>,
) -> BatchResponse {
    let mut steps: Vec<BatchStepResult> = Vec::new();
    let mut all_succeeded = true;
    let mut stopped_at: Option<usize> = None;
    let mut _prev_result: serde_json::Value = serde_json::Value::Null;

    for (index, item) in request.actions.iter().enumerate() {
        debug!(
            index = index,
            action_id = %item.action_id,
            "Batch executing step"
        );

        let exec_req = ExecuteRequest {
            action_id: item.action_id.clone(),
            args: item.args.clone(),
            dry_run: request.dry_run,
            reason: item.reason.clone(),
        };

        let result = handle_execute(app_handle, registry, exec_req, butler_conversation_id).await;

        let step = BatchStepResult {
            index,
            action_id: item.action_id.clone(),
            success: result.success,
            result: result.result.clone(),
            audit_id: result.audit_id.clone(),
            error: result.error.clone(),
        };

        if result.success {
            _prev_result = result.result;
        } else {
            all_succeeded = false;
            if request.stop_on_error {
                error!(
                    index = index,
                    action_id = %item.action_id,
                    error = ?result.error,
                    "Batch stopped on error"
                );
                stopped_at = Some(index);
                steps.push(step);
                break;
            }
        }

        steps.push(step);
    }

    BatchResponse { steps, all_succeeded, stopped_at }
}

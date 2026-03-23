use super::registry::ActionRegistry;
use super::types::*;

/// Handle the `superadmin_inspect` tool call.
pub fn handle_inspect(registry: &ActionRegistry, action_id: &str) -> Result<InspectResponse, String> {
    let meta = registry
        .get_meta(action_id)
        .ok_or_else(|| format!("Action not found: {}", action_id))?;

    Ok(InspectResponse::from(meta))
}

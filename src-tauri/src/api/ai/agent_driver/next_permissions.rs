use crate::errors::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Selections = BTreeMap<String, String>;

/// A user selection belongs to the existing native session and provider. Queue
/// entries store the message; the dispatcher reads the latest selection separately.
pub fn read(conn: &Connection, cid: i64, driver: &str, provider: Option<i64>, session: Option<&str>) -> Result<Selections, AppError> {
    let json: Option<String> = conn.query_row(
        "SELECT next_permissions FROM acp_session WHERE conversation_id=?1 AND agent_kind=?2
         AND (?3 IS NULL OR provider_instance_id=?3) AND (?4 IS NULL OR session_id=?4)",
        params![cid, driver, provider, session], |row| row.get(0),
    ).optional()?;
    serde_json::from_str(json.as_deref().unwrap_or("{}"))
        .map_err(|e| AppError::ProviderError(format!("读取会话 {cid} 的 {driver} 权限选择失败：{e}")))
}

pub fn save(conn: &Connection, cid: i64, driver: &str, provider: i64, session: &str, key: &str, value: &str) -> Result<(), AppError> {
    let changed = conn.execute(
        "UPDATE acp_session SET next_permissions=json_patch(next_permissions,json_object(?1,?2))
         WHERE conversation_id=?3 AND agent_kind=?4 AND provider_instance_id=?5 AND session_id=?6",
        params![key, value, cid, driver, provider, session],
    )?;
    if changed != 1 { return Err(AppError::ProviderError(format!("{driver} 会话 {cid} 已切换，未保存权限选择"))); }
    Ok(())
}

pub fn deferred(driver: &str, key: &str, option: &Value) -> bool {
    match driver {
        "claude_sdk" => key == "permission_mode",
        "pi" => key == "pi_approval",
        "kiro" => key == "autopilot",
        "antigravity" => key == "antigravity_mode",
        "cursor" => key == "cursor_auto_review",
        "opencode" => key == "opencode_permission",
        "acp" => [Some(key), option["category"].as_str()].into_iter().flatten().any(|s|
            matches!(s.to_ascii_lowercase().replace(['_', '-'], "").as_str(),
                "mode" | "permission" | "permissions" | "permissionmode" | "approval" | "approvalpolicy" |
                "approvalmode" | "sandbox" | "sandboxmode" | "autopilot")),
        _ => false,
    }
}

#[cfg(test)]
#[path = "next_permissions_tests.rs"]
mod tests;

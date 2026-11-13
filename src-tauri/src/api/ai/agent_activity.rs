//! Shared native activity snapshots. First-seen order is independent of revisions.
use serde_json::{json, Value};
use std::collections::HashMap;
use crate::db::conversation_db::{MessageRepository, Repository};

pub(crate) fn merge_activity(previous: Option<&Value>, frame: &Value, offset: usize, order: u64) -> Value {
    let mut activity = previous.cloned().unwrap_or_else(|| json!({"item_id":frame["id"],"content_offset":offset,"order":order,"started_at":chrono::Utc::now(),"metadata":{}}));
    activity["sequence"] = json!(previous.and_then(|p|p["sequence"].as_u64()).unwrap_or(0) + 1);
    for key in ["kind", "title", "input", "error"] {
        // Absent/null terminal fields must not erase the invocation's identity or arguments.
        if let Some(value) = frame.get(key).filter(|v| !v.is_null()) { activity[key] = value.clone(); }
    }
    if let Some(metadata) = frame.get("metadata").and_then(Value::as_object) {
        if !activity["metadata"].is_object() { activity["metadata"] = json!({}); }
        for (key, value) in metadata { activity["metadata"][key] = value.clone(); }
    }
    activity["status"] = match frame.get("status").and_then(Value::as_str) {
        Some("running") => json!("executing"),
        Some(status) => json!(status),
        None => activity.get("status").cloned().unwrap_or_else(||json!("pending")),
    };
    if previous.is_some_and(terminal) && !terminal(&activity) {
        activity["status"] = previous.unwrap()["status"].clone();
    }
    if frame["append"] == true {
        activity["output"] = json!(format!("{}{}", previous.and_then(|p|p["output"].as_str()).unwrap_or_default(), frame["output"].as_str().unwrap_or_default()));
    } else if let Some(value) = frame.get("output").filter(|v| !v.is_null()) { activity["output"] = value.clone(); }
    if terminal(&activity) && activity.get("finished_at").is_none() { activity["finished_at"] = json!(chrono::Utc::now()); }
    activity
}

fn terminal(activity: &Value) -> bool { matches!(activity["status"].as_str(), Some("success" | "failed" | "cancelled")) }

pub(crate) fn settle_activity(activity: &mut Value, cancelled: bool, failure: Option<&str>) -> bool {
    if terminal(activity) { return false; }
    activity["sequence"] = json!(activity["sequence"].as_u64().unwrap_or(0) + 1);
    activity["finished_at"] = json!(chrono::Utc::now());
    let unfinished = activity["kind"] != "reasoning";
    activity["status"] = json!(if failure.is_some() { "failed" } else if cancelled || unfinished { "cancelled" } else { "success" });
    if let Some(detail) = failure { activity["error"] = json!(detail); }
    else if unfinished { activity["error"] = json!("智能体已结束本轮，但未返回工具终态，执行结果尚未确认"); }
    true
}

pub(crate) fn ordered_activities(activities: &HashMap<String, Value>) -> Vec<&Value> {
    let mut ordered = activities.values().collect::<Vec<_>>();
    ordered.sort_by_key(|a| a["order"].as_u64().unwrap_or(u64::MAX));
    ordered
}

pub(crate) fn persist_activities(repo: &MessageRepository, message_id: i64, activities: &HashMap<String, Value>) -> Result<(), String> {
    let message = repo.read(message_id).map_err(|e|e.to_string())?.ok_or_else(||format!("Agent response message {message_id} disappeared"))?;
    let mut metadata: Value = message.metadata_json.as_deref().and_then(|s|serde_json::from_str(s).ok()).filter(Value::is_object).unwrap_or_else(||json!({}));
    metadata["agent_activities"] = json!(ordered_activities(activities));
    repo.update_metadata(message_id, Some(&metadata.to_string())).map_err(|e|e.to_string())
}

fn string_at(value: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path|value.pointer(path).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string))
}

/// Only explicit protocol kinds and known built-in names are classified; MCP names
/// and arbitrary human-readable titles never impersonate built-in operations.
pub(crate) fn normalize_tool_activity(driver: &str, activity: &mut Value) {
    if activity["kind"] == "reasoning" { return; }
    let tool = activity.pointer("/metadata/tool").and_then(Value::as_str).unwrap_or_default().to_string();
    let native = activity.pointer("/metadata/native_kind").and_then(Value::as_str).unwrap_or_default().to_string();
    let input = activity.get("input").cloned().unwrap_or(Value::Null);
    let path = string_at(&input, &["/path", "/file_path", "/filePath", "/target_file", "/targetFile", "/targetDirectory"]);
    let command = string_at(&input, &["/command", "/commandLine", "/command_line"]);
    let pattern = string_at(&input, &["/pattern", "/query", "/glob_pattern", "/globPattern", "/search_term"]);
    let is_mcp = activity.pointer("/metadata/server").and_then(Value::as_str).is_some() || tool.starts_with("mcp__");
    let name = tool.to_ascii_lowercase();
    let builtin = match driver {
        "claude_sdk" => matches!(name.as_str(), "bash" | "read" | "edit" | "multiedit" | "write" | "glob" | "grep" | "ls" | "task" | "agent"),
        "cursor" => matches!(name.as_str(), "shell" | "read" | "edit" | "write" | "delete" | "glob" | "grep" | "ls" | "semsearch" | "task"),
        "opencode" => matches!(name.as_str(), "bash" | "read" | "edit" | "write" | "glob" | "grep" | "list" | "apply_patch" | "task"),
        _ => false,
    };
    let semantic = if is_mcp { "tool" } else if matches!(driver, "kiro" | "antigravity") {
        match native.as_str() { "execute" => "command", "read" => "file_read", "search" => "file_search", "edit" | "move" => "file_write", "delete" => "file_delete", _ => "tool" }
    } else if builtin {
        match name.as_str() {
            "bash" | "powershell" | "shell" | "run_terminal_cmd" => "command",
            "read" | "read_file" => "file_read",
            "grep" | "glob" | "find" | "ls" | "list" | "list_dir" | "search" | "ripgrep" | "semsearch" => "file_search",
            "edit" | "multiedit" | "edit_file" | "str_replace" | "apply_patch" => "file_write",
            "write" | "write_file" => "file_write",
            "delete" => "file_delete",
            "task" | "agent" => "sub_agent",
            _ => "tool",
        }
    } else { "tool" };
    if !activity["metadata"].is_object() { activity["metadata"] = json!({}); }
    activity["metadata"]["presentation"] = json!("operation");
    if is_mcp {
        if let Some(rest) = tool.strip_prefix("mcp__") {
            if let Some((server, name)) = rest.split_once("__") {
                activity["metadata"]["server"] = json!(server);
                activity["metadata"]["display_tool"] = json!(name);
            }
        }
    }
    if semantic == "tool" && activity["kind"] != "tool" { return; }
    activity["kind"] = json!(semantic);
    let title = match semantic {
        "command" => command.clone(),
        "file_read" => path.as_ref().map(|p|format!("读取 {p}")),
        "file_search" => {
            let label = if matches!(name.as_str(), "ls" | "list" | "list_dir") { "列出目录" } else { "搜索" };
            (path.is_some() || pattern.is_some()).then(|| [Some(label.to_string()),pattern,path.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))
        }
        "file_write" => path.as_ref().map(|p|format!("{} {p}", if matches!(name.as_str(), "write" | "write_file") { "写入" } else { "编辑" })),
        "file_delete" => path.as_ref().map(|p|format!("删除 {p}")),
        _ => None,
    };
    if let Some(title) = title { activity["title"] = json!(title); }
    if let Some(command) = command { activity["metadata"]["command"] = json!(command); }
    // A real diff may be supplied by OpenCode/ACP/SDK. Never infer one from input.
    if semantic == "file_write" {
        let diff = string_at(&activity["metadata"], &["/diff", "/rawResult/details/patch", "/rawResult/details/diff"]);
        if let (Some(path), Some(diff)) = (path, diff) {
            activity["kind"] = json!("patch");
            activity["metadata"]["changes"] = json!([{"path":path,"kind":"update","diff":diff}]);
        }
    }
}

#[cfg(test)]
#[path = "agent_activity_tests.rs"]
mod tests;

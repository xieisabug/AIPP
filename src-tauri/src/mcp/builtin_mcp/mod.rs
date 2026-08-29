use crate::api::ai::acp::{
    AcpPermissionOptionPayload, AcpPermissionRequestSnapshot, AcpPermissionState,
};
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::mcp::builtin_mcp::operation::state::PermissionRequestSnapshot;
use serde::Deserialize;
use tauri::{AppHandle, Manager};
use tracing::{debug, error, instrument};

pub mod agent;
pub mod interaction;
pub mod operation;
pub mod preview_resources;
pub mod search;
pub mod superadmin;
pub mod templates;

pub use agent::{AgentHandler, TodoState};
pub use interaction::{
    handle_preview_file_relay_request, list_preview_code_requests_for_conversation,
    prepare_preview_file_request_for_ui, submit_ask_user_question_response,
    submit_preview_code_response, InteractionState, PreviewFileRelayState,
    PREVIEW_FILE_RELAY_SCHEME,
};
pub use operation::{OperationHandler, OperationState};
pub use preview_resources::{
    authorize_preview_code_external_resource_urls, authorize_preview_external_resources,
    get_preview_external_resource_policy, prepare_preview_code_request_for_ui,
    save_preview_external_resource_policy, scan_preview_code_external_resources_for_ui,
    PreviewResourceState,
};
pub use search::SearchHandler;
pub use templates::{
    add_or_update_aipp_builtin_server, get_builtin_tools_for_command, init_builtin_mcp_servers,
    list_aipp_builtin_templates,
};

pub fn is_builtin_command(command: &str) -> bool {
    command.trim().starts_with("aipp:")
}

pub fn builtin_command_id(command: &str) -> Option<String> {
    if is_builtin_command(command) {
        Some(command.trim().trim_start_matches("aipp:").to_string())
    } else {
        None
    }
}

// Legacy function alias for backward compatibility
pub fn is_builtin_mcp_call(command: &str) -> bool {
    is_builtin_command(command)
}

/// Look up conversation_kind for a given conversation_id.
/// Scopes the Repository trait import to avoid conflicts.
fn get_conversation_kind(app_handle: &AppHandle, conversation_id: i64) -> Option<String> {
    use crate::db::conversation_db::Repository;
    crate::db::conversation_db::ConversationDatabase::new(app_handle)
        .ok()
        .and_then(|db| db.conversation_repo().ok())
        .and_then(|repo| repo.read(conversation_id).ok().flatten())
        .map(|c| c.conversation_kind)
}

fn matches_keyword(value: &str, keyword: &str) -> bool {
    value.to_lowercase().contains(&keyword.to_lowercase())
}

fn parse_tool_selector(selector: &str) -> (Option<String>, String) {
    let trimmed = selector.trim();
    if let Some((server_name, tool_name)) = trimmed.split_once("::") {
        let server_name = server_name.trim();
        let tool_name = tool_name.trim();
        if !server_name.is_empty() && !tool_name.is_empty() {
            return (Some(server_name.to_lowercase()), tool_name.to_string());
        }
    }
    (None, trimmed.to_string())
}

fn should_filter_acp_native_dynamic_tools(
    app_handle: &AppHandle,
    conversation_id: Option<i64>,
) -> bool {
    let Some(conversation_id) = conversation_id else {
        return false;
    };

    crate::db::conversation_db::ConversationDatabase::new(app_handle)
        .ok()
        .and_then(|db| db.get_acp_session_id(conversation_id).ok().flatten())
        .is_some()
}

fn acp_native_operation_server_ids(
    db: &MCPDatabase,
) -> Result<std::collections::HashSet<i64>, String> {
    let servers = db
        .get_mcp_servers()
        .map_err(|e| format!("Failed to load ACP duplicate tool filter: {}", e))?;

    Ok(servers
        .into_iter()
        .filter(|server| {
            server.is_builtin && server.command.as_deref() == Some("aipp:operation")
        })
        .map(|server| server.id)
        .collect())
}

fn is_acp_native_duplicate_dynamic_tool(
    operation_server_ids: &std::collections::HashSet<i64>,
    server_id: i64,
    tool_name: &str,
) -> bool {
    operation_server_ids.contains(&server_id)
        && matches!(
            tool_name,
            "read_file" | "write_file" | "execute_bash" | "get_bash_output"
        )
}

fn parse_builtin_parameters(parameters: &str) -> Result<serde_json::Value, String> {
    let trimmed = parameters.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }

    // Strict parse first (expected path).
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(value);
    }

    // Fallback: some providers append trailing garbage after a valid JSON object.
    // Deserialize the first JSON value and ignore trailing characters.
    let mut de = serde_json::Deserializer::from_str(trimmed);
    match serde_json::Value::deserialize(&mut de) {
        Ok(value) => {
            debug!("Parsed builtin parameters with tolerant deserializer");
            Ok(value)
        }
        Err(e) => {
            error!(error = %e, "Invalid parameters JSON");
            Err(format!("Invalid parameters: {}", e))
        }
    }
}

fn value_as_i64(value: &serde_json::Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str().and_then(|raw| raw.trim().parse::<i64>().ok()))
}

fn argument_i64(args: &serde_json::Value, key: &str) -> Option<i64> {
    args.get(key).and_then(value_as_i64)
}

fn argument_string_array(args: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| format!("{key} must be an array of strings"))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(|entry| entry.to_string())
                .ok_or_else(|| format!("{key} must be an array of strings"))
        })
        .collect()
}

fn build_dynamic_mcp_server_tool_item(tool_name: &str, summary: &str) -> serde_json::Value {
    serde_json::json!({
        "tool": tool_name,
        "summary": summary,
    })
}

fn build_dynamic_mcp_server_item(
    server_name: &str,
    summary: &str,
    tools: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "server": server_name,
        "summary": summary,
        "tools": tools,
    })
}

fn build_dynamic_mcp_loaded_tool_item(
    server_name: &str,
    tool_name: &str,
    description: String,
    parameters: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "server": server_name,
        "tool": tool_name,
        "description": description,
        "parameters": parameters,
    })
}

fn contains_manual_review_command_keyword(value: &str) -> bool {
    let lowered = value.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return false;
    }

    if lowered.contains("remove-item") {
        return true;
    }

    lowered
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .any(|token| matches!(token, "rm" | "rmdir"))
}

fn operation_permission_manual_review_reason(
    snapshot: &PermissionRequestSnapshot,
) -> Option<String> {
    if contains_manual_review_command_keyword(&snapshot.event.operation)
        || contains_manual_review_command_keyword(&snapshot.event.path)
    {
        Some("请求内容包含 rm/删除类命令，必须由用户人工审核。".to_string())
    } else {
        None
    }
}

fn find_acp_permission_option<'a>(
    snapshot: &'a AcpPermissionRequestSnapshot,
    option_id: &str,
) -> Option<&'a AcpPermissionOptionPayload> {
    snapshot.event.options.iter().find(|option| option.option_id == option_id)
}

fn acp_permission_manual_review_reason(
    snapshot: &AcpPermissionRequestSnapshot,
    selected_option_id: Option<&str>,
) -> Option<String> {
    if snapshot.event.title.as_deref().is_some_and(contains_manual_review_command_keyword)
        || snapshot.event.parameters.as_deref().is_some_and(contains_manual_review_command_keyword)
    {
        return Some("ACP 请求内容包含 rm/删除类命令，必须由用户人工审核。".to_string());
    }

    if let Some(option_id) = selected_option_id {
        if let Some(option) = find_acp_permission_option(snapshot, option_id) {
            if option.kind == "allow_always" {
                return Some("ACP 持久授权必须由用户人工审核。".to_string());
            }
        }
    }

    None
}

fn build_butler_review_payload(
    manual_review_reason: Option<String>,
    default_guidance: &str,
) -> serde_json::Value {
    let manual_review_required = manual_review_reason.is_some();
    let guidance = if manual_review_required {
        "总管家不得直接确认该请求，需等待用户在桌面端或飞书端人工审核。"
    } else {
        default_guidance
    };

    serde_json::json!({
        "manual_review_required": manual_review_required,
        "risk_level": if manual_review_required { "high" } else { "normal" },
        "reason": manual_review_reason,
        "guidance": guidance,
    })
}

fn build_operation_permission_snapshot_payload(
    snapshot: &PermissionRequestSnapshot,
) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|e| format!("Failed to serialize operation permission snapshot: {e}"))?;
    let manual_review_reason = operation_permission_manual_review_reason(snapshot);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "butler_review".to_string(),
            build_butler_review_payload(
                manual_review_reason,
                "如果请求与当前任务目标一致且风险可控，总管家可以确认；优先选择最小权限，不要使用 allow_and_save。",
            ),
        );
    }
    Ok(value)
}

fn build_acp_permission_snapshot_payload(
    snapshot: &AcpPermissionRequestSnapshot,
) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(snapshot)
        .map_err(|e| format!("Failed to serialize ACP permission snapshot: {e}"))?;
    let persistent_option_present =
        snapshot.event.options.iter().any(|option| option.kind == "allow_always");
    let manual_review_reason = acp_permission_manual_review_reason(snapshot, None);
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "butler_review".to_string(),
            build_butler_review_payload(
                manual_review_reason,
                if persistent_option_present {
                    "若必须确认，优先选择一次性授权；allow_always 需要用户人工审核。"
                } else {
                    "如果请求与当前任务目标一致且风险可控，总管家可以确认；优先选择一次性授权。"
                },
            ),
        );
    }
    Ok(value)
}

fn tool_call_anchor_message_id(tool_call: &MCPToolCall) -> Option<i64> {
    tool_call.assistant_message_id.or(tool_call.message_id)
}

fn build_pending_mcp_tool_call_payload(tool_call: &MCPToolCall) -> serde_json::Value {
    let parameters = serde_json::from_str::<serde_json::Value>(&tool_call.parameters)
        .unwrap_or_else(|_| serde_json::Value::String(tool_call.parameters.clone()));
    serde_json::json!({
        "tool_call_id": tool_call.id,
        "llm_call_id": tool_call.llm_call_id,
        "server_name": tool_call.server_name,
        "tool_name": tool_call.tool_name,
        "parameters": parameters,
        "status": tool_call.status,
        "message_id": tool_call.message_id,
        "assistant_message_id": tool_call.assistant_message_id,
        "created_time": tool_call.created_time,
    })
}

fn build_pending_mcp_tool_calls_payload(tool_calls: &[MCPToolCall]) -> Vec<serde_json::Value> {
    tool_calls.iter().map(build_pending_mcp_tool_call_payload).collect()
}

#[allow(dead_code)]
fn has_active_anchor_group_calls(tool_calls: &[MCPToolCall], target_call: &MCPToolCall) -> bool {
    let anchor_message_id = tool_call_anchor_message_id(target_call);
    tool_calls.iter().any(|tool_call| {
        tool_call.id != target_call.id
            && tool_call_anchor_message_id(tool_call) == anchor_message_id
            && matches!(tool_call.status.as_str(), "pending" | "executing")
    })
}

fn collect_anchor_group_tool_call_ids(
    tool_calls: &[MCPToolCall],
    anchor_message_id: Option<i64>,
) -> Vec<i64> {
    tool_calls
        .iter()
        .filter(|tool_call| tool_call_anchor_message_id(tool_call) == anchor_message_id)
        .map(|tool_call| tool_call.id)
        .collect()
}

async fn trigger_task_conversation_tool_batch_if_ready(
    app_handle: &AppHandle,
    window: tauri::Window,
    task_conversation_id: i64,
    anchor_message_id: Option<i64>,
) -> Result<(), String> {
    let all_tool_calls = MCPDatabase::new(app_handle)
        .map_err(|e| e.to_string())?
        .get_mcp_tool_calls_by_conversation(task_conversation_id)
        .map_err(|e| e.to_string())?;
    let same_anchor_tool_call_ids =
        collect_anchor_group_tool_call_ids(&all_tool_calls, anchor_message_id);
    if same_anchor_tool_call_ids.is_empty() {
        return Ok(());
    }

    let has_active_calls = all_tool_calls.iter().any(|tool_call| {
        tool_call_anchor_message_id(tool_call) == anchor_message_id
            && matches!(tool_call.status.as_str(), "pending" | "executing")
    });
    if has_active_calls {
        return Ok(());
    }

    crate::mcp::execution_api::trigger_conversation_continuation_batch(
        app_handle,
        app_handle.state::<crate::AppState>(),
        app_handle.state::<crate::FeatureConfigState>(),
        window,
        task_conversation_id,
        same_anchor_tool_call_ids,
    )
    .await
    .map_err(|e| e.to_string())
}

fn spawn_task_conversation_mcp_tool_execution(
    app_handle: AppHandle,
    window: tauri::Window,
    task_conversation_id: i64,
    pending_tool_call: MCPToolCall,
) {
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            let anchor_message_id = tool_call_anchor_message_id(&pending_tool_call);
            match crate::mcp::execution_api::execute_mcp_tool_call(
                app_handle.clone(),
                app_handle.state::<crate::AppState>(),
                app_handle.state::<crate::FeatureConfigState>(),
                window.clone(),
                pending_tool_call.id,
                false,
            )
            .await
            {
                Ok(_) => {
                    if let Err(error) = trigger_task_conversation_tool_batch_if_ready(
                        &app_handle,
                        window,
                        task_conversation_id,
                        anchor_message_id,
                    )
                    .await
                    {
                        error!(
                            task_conversation_id,
                            tool_call_id = pending_tool_call.id,
                            error = %error,
                            "failed to continue task conversation after dispatched MCP tool execution"
                        );
                    }
                }
                Err(error) => {
                    error!(
                        task_conversation_id,
                        tool_call_id = pending_tool_call.id,
                        error = %error,
                        "task_conversation_operation background mcp_tool_execute failed"
                    );
                }
            }
        });
    });
}

fn resolve_artifact_tool_conversation_id(
    tool_name: &str,
    args: &serde_json::Value,
    conversation_id: Option<i64>,
) -> Result<i64, String> {
    if tool_name == "get_artifact_workspace" {
        return conversation_id
            .ok_or_else(|| "Artifact tools require conversation context".to_string());
    }

    argument_i64(args, "conversation_id")
        .or(conversation_id)
        .ok_or_else(|| "Artifact tools require conversation context".to_string())
}

fn build_capture_artifact_screenshot_result(
    response: &crate::artifacts::workspace::CaptureArtifactScreenshotResponse,
) -> serde_json::Value {
    let summary = format!(
        "Captured artifact screenshot for {} at {}x{}.",
        response.entry_file, response.width, response.height
    );
    let structured = serde_json::json!({
        "artifact_key": response.artifact_key,
        "entry_file": response.entry_file,
        "language": response.language,
        "preview_type": response.preview_type,
        "output_mode": response.output_mode,
        "width": response.width,
        "height": response.height,
        "mime_type": response.mime_type,
        "path": response.path,
    });

    if let Some(base64) = response.base64.as_deref() {
        serde_json::json!({
            "content": [
                {
                    "type": "text",
                    "text": summary,
                },
                {
                    "type": "image",
                    "mimeType": response.mime_type,
                    "data": base64,
                }
            ],
            "structuredContent": structured,
            "isError": false
        })
    } else if let Some(path) = response.path.as_deref() {
        serde_json::json!({
            "content": [
                {
                    "type": "text",
                    "text": format!("{}\nScreenshot saved to {}.", summary, path),
                },
                {
                    "type": "resource_link",
                    "uri": format!("file://{}", path),
                    "name": response.entry_file,
                    "mimeType": response.mime_type,
                }
            ],
            "structuredContent": structured,
            "isError": false
        })
    } else {
        serde_json::json!({
            "content": [
                {
                    "type": "text",
                    "text": summary,
                }
            ],
            "structuredContent": structured,
            "isError": false
        })
    }
}

fn execute_dynamic_mcp_tool(
    app_handle: &AppHandle,
    tool_name: &str,
    args: &serde_json::Value,
    conversation_id: Option<i64>,
) -> Result<serde_json::Value, String> {
    let db =
        MCPDatabase::new(app_handle).map_err(|e| format!("Failed to open MCP database: {}", e))?;
    let _ = db.rebuild_dynamic_mcp_catalog();
    let acp_native_operation_server_ids = if should_filter_acp_native_dynamic_tools(
        app_handle,
        conversation_id,
    ) {
        acp_native_operation_server_ids(&db)?
    } else {
        std::collections::HashSet::new()
    };

    match tool_name {
        "load_mcp_server" => {
            let keyword = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter: name".to_string())?;
            let catalogs = db
                .list_server_capability_catalog()
                .map_err(|e| format!("Failed to list MCP toolset catalog: {}", e))?;
            let tool_catalog = db
                .list_tool_catalog(None)
                .map_err(|e| format!("Failed to list MCP tool catalog: {}", e))?;
            let mut matched_servers = Vec::new();
            for server in catalogs {
                if server.summary_generated_at.is_none() {
                    continue;
                }
                if !matches_keyword(&server.server_name, keyword)
                    && !matches_keyword(&server.summary, keyword)
                {
                    continue;
                }
                let tools: Vec<serde_json::Value> = tool_catalog
                    .iter()
                    .filter(|tool| {
                        tool.server_id == server.server_id
                            && tool.server_enabled
                            && tool.tool_enabled
                            && tool.summary_generated_at.is_some()
                            && tool.server_name != "MCP 动态加载工具"
                            && !is_acp_native_duplicate_dynamic_tool(
                                &acp_native_operation_server_ids,
                                tool.server_id,
                                &tool.tool_name,
                            )
                    })
                    .map(|tool| build_dynamic_mcp_server_tool_item(&tool.tool_name, &tool.summary))
                    .collect();
                if acp_native_operation_server_ids.contains(&server.server_id) && tools.is_empty() {
                    continue;
                }
                matched_servers.push(build_dynamic_mcp_server_item(
                    &server.server_name,
                    &server.summary,
                    tools,
                ));
            }

            if matched_servers.is_empty() {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("No MCP toolset matched '{}'. Try another keyword.", keyword)
                    }],
                    "isError": true
                }))
            } else {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "json",
                        "json": {
                            "servers": matched_servers
                        }
                    }],
                    "isError": false
                }))
            }
        }
        "load_mcp_tool" => {
            let names = if let Some(values) = args.get("names").and_then(|v| v.as_array()) {
                values.iter().filter_map(|v| v.as_str()).map(|v| v.to_string()).collect::<Vec<_>>()
            } else if let Some(single) = args.get("name").and_then(|v| v.as_str()) {
                vec![single.to_string()]
            } else {
                Vec::new()
            };
            if names.is_empty() {
                return Err("Missing required parameter: names".to_string());
            }
            let conversation_id = conversation_id
                .ok_or_else(|| "load_mcp_tool requires conversation context".to_string())?;
            let server_filter =
                args.get("server_name").and_then(|v| v.as_str()).map(|v| v.to_lowercase());
            let tool_catalog = db
                .list_tool_catalog(None)
                .map_err(|e| format!("Failed to list MCP tool catalog: {}", e))?;
            let mut selected = Vec::new();
            let mut selected_ids = std::collections::HashSet::new();

            for keyword in &names {
                let (name_server_filter, name_keyword) = parse_tool_selector(keyword);
                if name_keyword.is_empty() {
                    continue;
                }
                for tool in &tool_catalog {
                    if !tool.server_enabled || !tool.tool_enabled {
                        continue;
                    }
                    if tool.summary_generated_at.is_none() {
                        continue;
                    }
                    if tool.server_name == "MCP 动态加载工具" {
                        continue;
                    }
                    if is_acp_native_duplicate_dynamic_tool(
                        &acp_native_operation_server_ids,
                        tool.server_id,
                        &tool.tool_name,
                    ) {
                        continue;
                    }
                    if let Some(filter) = &server_filter {
                        if !matches_keyword(&tool.server_name, filter) {
                            continue;
                        }
                    }
                    if let Some(filter) = &name_server_filter {
                        if !matches_keyword(&tool.server_name, filter) {
                            continue;
                        }
                    }
                    let matched = if name_server_filter.is_some() {
                        matches_keyword(&tool.tool_name, &name_keyword)
                    } else {
                        matches_keyword(&tool.tool_name, &name_keyword)
                            || matches_keyword(&tool.summary, &name_keyword)
                            || matches_keyword(&tool.server_name, &name_keyword)
                    };
                    if !matched {
                        continue;
                    }
                    if selected_ids.insert(tool.tool_id) {
                        selected.push(tool.clone());
                    }
                }
            }

            if selected.is_empty() {
                Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": format!("No MCP tool matched {:?}. Try more specific keywords.", names)
                    }],
                    "isError": true
                }))
            } else {
                let mut server_ids = Vec::new();
                let mut seen_server_ids = std::collections::HashSet::new();
                for tool in &selected {
                    if seen_server_ids.insert(tool.server_id) {
                        server_ids.push(tool.server_id);
                    }
                }
                let mut tool_definition_map: std::collections::HashMap<i64, (String, String)> =
                    std::collections::HashMap::new();
                if !server_ids.is_empty() {
                    let server_tool_pairs = db
                        .get_mcp_servers_with_tools_by_ids(&server_ids)
                        .map_err(|e| format!("Failed to load MCP tool definitions: {}", e))?;
                    for (_server, tools) in server_tool_pairs {
                        for actual_tool in tools {
                            if actual_tool.is_enabled {
                                tool_definition_map.insert(
                                    actual_tool.id,
                                    (
                                        actual_tool.tool_description.unwrap_or_default(),
                                        actual_tool.parameters.unwrap_or_else(|| "{}".to_string()),
                                    ),
                                );
                            }
                        }
                    }
                }
                let mut loaded = Vec::new();
                for tool in &selected {
                    db.upsert_conversation_loaded_tool(
                        conversation_id,
                        tool.tool_id,
                        Some("manual"),
                    )
                    .map_err(|e| {
                        format!("Failed to persist loaded tool {}: {}", tool.tool_name, e)
                    })?;
                    let (description, parameters_json) = tool_definition_map
                        .get(&tool.tool_id)
                        .cloned()
                        .unwrap_or_else(|| (String::new(), "{}".to_string()));
                    let resolved_description = if description.trim().is_empty() {
                        tool.summary.clone()
                    } else {
                        description
                    };
                    let parameters_schema = serde_json::from_str::<serde_json::Value>(
                        &parameters_json,
                    )
                    .unwrap_or_else(|_| {
                        serde_json::json!({
                            "type": "object",
                            "additionalProperties": true
                        })
                    });
                    loaded.push(build_dynamic_mcp_loaded_tool_item(
                        &tool.server_name,
                        &tool.tool_name,
                        resolved_description,
                        parameters_schema,
                    ));
                }
                Ok(serde_json::json!({
                    "content": [{
                        "type": "json",
                        "json": {
                            "tools": loaded
                        }
                    }],
                    "isError": false
                }))
            }
        }
        _ => Ok(serde_json::json!({
            "content": [{"type": "text", "text": format!("Unknown dynamic_mcp tool: {}", tool_name)}],
            "isError": true
        })),
    }
}

fn resolve_butler_spawn_window(app_handle: &AppHandle) -> Result<tauri::Window, String> {
    for label in ["butler_experiment", "chat_ui", "ask"] {
        if let Some(window) = app_handle.get_webview_window(label) {
            return Ok(window.as_ref().window());
        }
    }
    Err("No available window for butler task execution".to_string())
}

fn dedupe_task_messages(
    rows: Vec<(
        crate::db::conversation_db::Message,
        Option<crate::db::conversation_db::MessageAttachment>,
    )>,
) -> Vec<crate::db::conversation_db::Message> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut messages = Vec::new();
    for (message, _) in rows {
        if seen.insert(message.id) {
            messages.push(message);
        }
    }
    messages
}

async fn resolve_accessible_butler_task_detail(
    app_handle: &AppHandle,
    current_conversation_id: Option<i64>,
    task_conversation_id: i64,
) -> Result<crate::api::butler_api::ButlerTaskDetailResponse, String> {
    use crate::api::butler_api::get_butler_task_detail;
    use crate::db::conversation_db::{ConversationDatabase, Repository};

    let current_conversation_id = current_conversation_id.ok_or_else(|| {
        "task_conversation_operation requires Butler conversation context".to_string()
    })?;
    let detail = get_butler_task_detail(app_handle.clone(), task_conversation_id).await?;

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let current_conversation = conversation_repo
        .read(current_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Current Butler conversation not found".to_string())?;

    match current_conversation.conversation_kind.as_str() {
        "butler_main" => {
            if detail.definition.butler_conversation_id != current_conversation_id {
                return Err(
                    "Target task conversation does not belong to the current Butler main conversation"
                        .to_string(),
                );
            }
        }
        "butler_task" => {
            if current_conversation_id != task_conversation_id
                && current_conversation.parent_butler_conversation_id
                    != Some(detail.definition.butler_conversation_id)
            {
                return Err(
                    "Target task conversation is outside the current Butler task scope".to_string()
                );
            }
        }
        _ => {
            return Err(
                "task_conversation_operation can only be used inside Butler main/task conversations"
                    .to_string(),
            );
        }
    }

    Ok(detail)
}

async fn build_task_conversation_read_payload(
    app_handle: &AppHandle,
    detail: &crate::api::butler_api::ButlerTaskDetailResponse,
    latest_count: usize,
    verbose: bool,
) -> Result<serde_json::Value, String> {
    use crate::db::conversation_db::ConversationDatabase;

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let messages = dedupe_task_messages(
        db.message_repo()
            .map_err(|e| e.to_string())?
            .list_by_conversation_id(detail.conversation.id)
            .map_err(|e| e.to_string())?,
    );
    let latest_count = latest_count.clamp(1, 10);
    let start = messages.len().saturating_sub(latest_count);
    let latest_messages = messages.into_iter().skip(start).collect::<Vec<_>>();

    let pending_mcp_tool_calls = MCPDatabase::new(app_handle)
        .map_err(|e| e.to_string())?
        .get_mcp_tool_calls_by_conversation(detail.conversation.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|tool_call| tool_call.status == "pending")
        .collect::<Vec<_>>();
    let operation_permissions = app_handle
        .state::<OperationState>()
        .list_permission_requests_for_conversation(detail.conversation.id)
        .await;
    let acp_permissions = app_handle
        .state::<AcpPermissionState>()
        .list_requests_for_conversation(detail.conversation.id)
        .await;
    let ask_user_questions = app_handle
        .state::<interaction::InteractionState>()
        .list_requests_for_conversation(detail.conversation.id)
        .await;
    let operation_permissions = operation_permissions
        .iter()
        .map(build_operation_permission_snapshot_payload)
        .collect::<Result<Vec<_>, _>>()?;
    let pending_mcp_tool_calls = build_pending_mcp_tool_calls_payload(&pending_mcp_tool_calls);
    let acp_permissions = acp_permissions
        .iter()
        .map(build_acp_permission_snapshot_payload)
        .collect::<Result<Vec<_>, _>>()?;

    if verbose {
        Ok(serde_json::json!({
            "task": detail.task,
            "conversation": detail.conversation,
            "definition": detail.definition,
            "result": detail.result,
            "runtime_state": detail.runtime_state,
            "latest_messages": latest_messages,
            "pending_mcp_tool_calls": pending_mcp_tool_calls,
            "pending_operation_permissions": operation_permissions,
            "pending_acp_permissions": acp_permissions,
            "pending_ask_user_questions": ask_user_questions,
        }))
    } else {
        // Slim payload: only essential status + messages + pending items.
        Ok(serde_json::json!({
            "task_conversation_id": detail.task.task_conversation_id,
            "title": detail.task.title,
            "status": detail.task.status,
            "is_running": detail.runtime_state.is_running,
            "is_finalized": detail.task.is_finalized,
            "last_summary": detail.task.last_summary,
            "latest_messages": latest_messages,
            "pending_mcp_tool_calls": pending_mcp_tool_calls,
            "pending_operation_permissions": operation_permissions,
            "pending_acp_permissions": acp_permissions,
            "pending_ask_user_questions": ask_user_questions,
        }))
    }
}

async fn resolve_pending_mcp_tool_call(
    app_handle: &AppHandle,
    task_conversation_id: i64,
    args: &serde_json::Value,
) -> Result<MCPToolCall, String> {
    let pending_tool_calls = MCPDatabase::new(app_handle)
        .map_err(|e| e.to_string())?
        .get_mcp_tool_calls_by_conversation(task_conversation_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|tool_call| tool_call.status == "pending")
        .collect::<Vec<_>>();

    if let Some(tool_call_id) = args.get("tool_call_id").and_then(value_as_i64) {
        return pending_tool_calls
            .into_iter()
            .find(|tool_call| tool_call.id == tool_call_id)
            .ok_or_else(|| {
                "Pending MCP tool call not found for this task conversation".to_string()
            });
    }

    if let Some(llm_call_id) = args.get("llm_call_id").and_then(|value| value.as_str()) {
        return pending_tool_calls
            .into_iter()
            .find(|tool_call| tool_call.llm_call_id.as_deref() == Some(llm_call_id))
            .ok_or_else(|| "Pending MCP tool call matching llm_call_id was not found".to_string());
    }

    if pending_tool_calls.len() == 1 {
        Ok(pending_tool_calls.into_iter().next().expect("single pending tool call"))
    } else if pending_tool_calls.is_empty() {
        Err("No pending MCP tool call exists for this task conversation".to_string())
    } else {
        Err("Multiple pending MCP tool calls exist; please specify tool_call_id or llm_call_id"
            .to_string())
    }
}

async fn resolve_operation_permission_request_id(
    app_handle: &AppHandle,
    task_conversation_id: i64,
    args: &serde_json::Value,
) -> Result<String, String> {
    let state = app_handle.state::<OperationState>();
    if let Some(request_id) = args.get("request_id").and_then(|v| v.as_str()) {
        return Ok(request_id.to_string());
    }
    if let Some(review_code) = args.get("review_code").and_then(|v| v.as_str()) {
        return state
            .find_permission_request_by_review_code(review_code)
            .await
            .map(|snapshot| snapshot.event.request_id)
            .ok_or_else(|| "No pending operation permission matched the review_code".to_string());
    }
    let pending = state.list_permission_requests_for_conversation(task_conversation_id).await;
    if pending.len() == 1 {
        Ok(pending[0].event.request_id.clone())
    } else if pending.is_empty() {
        Err("No pending operation permission exists for this task conversation".to_string())
    } else {
        Err("Multiple pending operation permissions exist; please specify request_id or review_code".to_string())
    }
}

async fn resolve_acp_permission_request_id(
    app_handle: &AppHandle,
    task_conversation_id: i64,
    args: &serde_json::Value,
) -> Result<String, String> {
    let state = app_handle.state::<crate::api::ai::acp::AcpPermissionState>();
    if let Some(request_id) = args.get("request_id").and_then(|v| v.as_str()) {
        return Ok(request_id.to_string());
    }
    if let Some(review_code) = args.get("review_code").and_then(|v| v.as_str()) {
        return state
            .find_request_by_review_code(review_code)
            .await
            .map(|snapshot| snapshot.event.request_id)
            .ok_or_else(|| "No pending ACP permission matched the review_code".to_string());
    }
    let pending = state.list_requests_for_conversation(task_conversation_id).await;
    if pending.len() == 1 {
        Ok(pending[0].event.request_id.clone())
    } else if pending.is_empty() {
        Err("No pending ACP permission exists for this task conversation".to_string())
    } else {
        Err("Multiple pending ACP permissions exist; please specify request_id or review_code"
            .to_string())
    }
}

async fn resolve_ask_user_request_id(
    app_handle: &AppHandle,
    task_conversation_id: i64,
    args: &serde_json::Value,
) -> Result<String, String> {
    let state = app_handle.state::<interaction::InteractionState>();
    if let Some(request_id) = args.get("request_id").and_then(|v| v.as_str()) {
        return Ok(request_id.to_string());
    }
    let pending = state.list_requests_for_conversation(task_conversation_id).await;
    if pending.len() == 1 {
        Ok(pending[0].request_id.clone())
    } else if pending.is_empty() {
        Err("No pending ask_user_question exists for this task conversation".to_string())
    } else {
        Err("Multiple pending ask_user_question requests exist; please specify request_id"
            .to_string())
    }
}

#[tauri::command]
#[instrument(skip(app_handle, parameters), fields(command = %server_command, tool = %tool_name))]
pub async fn execute_aipp_builtin_tool(
    app_handle: AppHandle,
    server_command: String,
    tool_name: String,
    parameters: String,
    conversation_id: Option<i64>,
) -> Result<String, String> {
    use search::types::{SearchRequest, SearchResponse, SearchResultType};

    let args = parse_builtin_parameters(&parameters)?;

    let cmd_id = builtin_command_id(&server_command).ok_or("Not a builtin command")?;

    let result_value = match cmd_id.as_str() {
        "search" => {
            let handler = SearchHandler::new(app_handle.clone());
            match tool_name.as_str() {
                "search_web" => {
                    let query = args
                        .get("query")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: query".to_string())?;

                    // 获取result_type参数，默认为markdown
                    let result_type_str = args.get("result_type").and_then(|v| v.as_str());

                    let result_type = SearchResultType::from_str(result_type_str);
                    let request = SearchRequest { query: query.to_string(), result_type };

                    match handler.search_web_with_type(request).await {
                        Ok(response) => {
                            // 根据result_type返回不同格式的内容
                            match response {
                                SearchResponse::Html { html_content, search_engine, search_time_ms, .. } => {
                                    serde_json::json!({
                                        "content": [{"type": "text", "text": html_content}],
                                        "isError": false,
                                        "search_engine": search_engine,
                                        "search_time_ms": search_time_ms
                                    })
                                }
                                SearchResponse::Markdown { markdown_content, search_engine, search_time_ms, .. } => {
                                    serde_json::json!({
                                        "content": [{"type": "text", "text": markdown_content}],
                                        "isError": false,
                                        "search_engine": search_engine,
                                        "search_time_ms": search_time_ms
                                    })
                                }
                                SearchResponse::Items(search_results) => {
                                    serde_json::json!({
                                        "content": [{"type": "json", "json": search_results}],
                                        "isError": false
                                    })
                                }
                                SearchResponse::ItemsOnly(items) => {
                                    serde_json::json!({
                                        "content": [{"type": "json", "json": items}],
                                        "isError": false
                                    })
                                }
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "search_web tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "fetch_url" => {
                    let url = args
                        .get("url")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: url".to_string())?;

                    // 获取result_type参数，默认为markdown
                    let result_type =
                        args.get("result_type").and_then(|v| v.as_str()).unwrap_or("markdown");

                    match handler.fetch_url_with_type(url, result_type).await {
                        Ok(v) => serde_json::json!({
                            "content": [{"type": "text", "text": v}],
                            "isError": false
                        }),
                        Err(e) => {
                            error!(error = %e, url = %url, "fetch_url tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown search tool: {}", tool_name)}],
                    "isError": true
                }),
            }
        }
        "operation" => {
            use operation::types::*;

            // 获取或创建 OperationState（从 app state 管理）
            let state = app_handle
                .try_state::<OperationState>()
                .map(|s| s.inner().clone())
                .unwrap_or_else(|| {
                    let state = OperationState::new();
                    // 注意：这里无法动态添加 state，需要在 lib.rs 中预先注册
                    // 这里创建临时 state，每次调用独立
                    state
                });

            let handler = OperationHandler::new(app_handle.clone());
            // conversation_id 从函数参数传入，不再从 args 中获取

            match tool_name.as_str() {
                "read_file" => {
                    let file_path = args
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: file_path".to_string())?;
                    let offset = args.get("offset").and_then(|v| v.as_u64()).map(|v| v as usize);
                    let limit = args.get("limit").and_then(|v| v.as_u64()).map(|v| v as usize);

                    let request =
                        ReadFileRequest { file_path: file_path.to_string(), offset, limit };

                    match handler.read_file(&state, request, conversation_id).await {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "text", "text": response.content}],
                            "isError": false,
                            "metadata": {
                                "file_path": response.file_path,
                                "start_line": response.start_line,
                                "end_line": response.end_line,
                                "total_lines": response.total_lines,
                                "has_more": response.has_more
                            }
                        }),
                        Err(e) => {
                            error!(error = %e, "read_file tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "write_file" => {
                    let file_path = args
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: file_path".to_string())?;
                    let content = args
                        .get("content")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: content".to_string())?;

                    let request = WriteFileRequest {
                        file_path: file_path.to_string(),
                        content: content.to_string(),
                    };

                    match handler.write_file(&state, request, conversation_id).await {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "text", "text": response.message}],
                            "isError": false,
                            "metadata": {
                                "file_path": response.file_path,
                                "bytes_written": response.bytes_written
                            }
                        }),
                        Err(e) => {
                            error!(error = %e, "write_file tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "edit_file" => {
                    let file_path = args
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: file_path".to_string())?;
                    let old_string = args
                        .get("old_string")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: old_string".to_string())?;
                    let new_string = args
                        .get("new_string")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: new_string".to_string())?;
                    let replace_all = args.get("replace_all").and_then(|v| v.as_bool());

                    let request = EditFileRequest {
                        file_path: file_path.to_string(),
                        old_string: old_string.to_string(),
                        new_string: new_string.to_string(),
                        replace_all,
                    };

                    match handler.edit_file(&state, request, conversation_id).await {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "text", "text": response.message}],
                            "isError": false,
                            "metadata": {
                                "file_path": response.file_path,
                                "replacements_made": response.replacements_made
                            }
                        }),
                        Err(e) => {
                            error!(error = %e, "edit_file tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "list_directory" => {
                    let path = args
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: path".to_string())?;
                    let pattern =
                        args.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let recursive = args.get("recursive").and_then(|v| v.as_bool());

                    let request =
                        ListDirectoryRequest { path: path.to_string(), pattern, recursive };

                    match handler.list_directory(&state, request, conversation_id).await {
                        Ok(response) => {
                            let entries_text = response
                                .entries
                                .iter()
                                .map(|e| {
                                    let type_indicator = if e.is_directory { "/" } else { "" };
                                    format!("{}{}", e.name, type_indicator)
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            serde_json::json!({
                                "content": [{"type": "text", "text": entries_text}],
                                "isError": false,
                                "metadata": {
                                    "path": response.path,
                                    "total_count": response.total_count,
                                    "entries": response.entries
                                }
                            })
                        }
                        Err(e) => {
                            error!(error = %e, "list_directory tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "execute_bash" => {
                    let command = args
                        .get("command")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: command".to_string())?;
                    let description =
                        args.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let timeout = args.get("timeout").and_then(|v| v.as_u64());
                    let run_in_background = args.get("run_in_background").and_then(|v| v.as_bool());

                    let request = ExecuteBashRequest {
                        command: command.to_string(),
                        description,
                        timeout,
                        run_in_background,
                    };

                    match handler.execute_bash(&state, request).await {
                        Ok(response) => {
                            let text = if let Some(output) = &response.output {
                                output.clone()
                            } else {
                                response.message.clone()
                            };
                            // 如果退出码非零，标记为错误
                            let is_error = response.exit_code.map(|c| c != 0).unwrap_or(false);
                            serde_json::json!({
                                "content": [{"type": "text", "text": text}],
                                "isError": is_error,
                                "metadata": {
                                    "bash_id": response.bash_id,
                                    "exit_code": response.exit_code,
                                    "message": response.message
                                }
                            })
                        }
                        Err(e) => {
                            error!(error = %e, "execute_bash tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "get_bash_output" => {
                    let bash_id = args
                        .get("bash_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: bash_id".to_string())?;
                    let filter = args.get("filter").and_then(|v| v.as_str()).map(|s| s.to_string());

                    let request = GetBashOutputRequest { bash_id: bash_id.to_string(), filter };

                    match handler.get_bash_output(&state, request).await {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "text", "text": response.output}],
                            "isError": false,
                            "metadata": {
                                "bash_id": response.bash_id,
                                "status": response.status,
                                "exit_code": response.exit_code
                            }
                        }),
                        Err(e) => {
                            error!(error = %e, "get_bash_output tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown operation tool: {}", tool_name)}],
                    "isError": true
                }),
            }
        }
        "artifact" => {
            use crate::artifacts::workspace::{
                capture_artifact_screenshot, get_artifact_workspace, show_artifact,
                CaptureArtifactScreenshotRequest, ShowArtifactRequest,
            };

            match tool_name.as_str() {
                "get_artifact_workspace" => {
                    let resolved_conversation_id = resolve_artifact_tool_conversation_id(
                        tool_name.as_str(),
                        &args,
                        conversation_id,
                    )?;
                    match get_artifact_workspace(&app_handle, resolved_conversation_id) {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "json", "json": response}],
                            "isError": false
                        }),
                        Err(e) => {
                            error!(error = %e, "get_artifact_workspace tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "show_artifact" => {
                    let resolved_conversation_id = resolve_artifact_tool_conversation_id(
                        tool_name.as_str(),
                        &args,
                        conversation_id,
                    )?;
                    let artifact_key = args
                        .get("artifact_key")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: artifact_key".to_string())?;
                    let entry_file = args
                        .get("entry_file")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: entry_file".to_string())?;
                    let request = ShowArtifactRequest {
                        conversation_id: resolved_conversation_id,
                        artifact_key: artifact_key.to_string(),
                        entry_file: entry_file.to_string(),
                        title: args.get("title").and_then(|v| v.as_str()).map(|v| v.to_string()),
                        language: args
                            .get("language")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        preview_type: args
                            .get("preview_type")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        db_id: args.get("db_id").and_then(|v| v.as_str()).map(|v| v.to_string()),
                        assistant_id: argument_i64(&args, "assistant_id"),
                    };
                    match show_artifact(&app_handle, request) {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "json", "json": response}],
                            "isError": false
                        }),
                        Err(e) => {
                            error!(error = %e, "show_artifact tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "capture_artifact_screenshot" => {
                    let resolved_conversation_id = resolve_artifact_tool_conversation_id(
                        tool_name.as_str(),
                        &args,
                        conversation_id,
                    )?;
                    let artifact_key = args
                        .get("artifact_key")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: artifact_key".to_string())?;
                    let entry_file = args
                        .get("entry_file")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: entry_file".to_string())?;
                    let request = CaptureArtifactScreenshotRequest {
                        conversation_id: resolved_conversation_id,
                        artifact_key: artifact_key.to_string(),
                        entry_file: entry_file.to_string(),
                        language: args
                            .get("language")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        preview_type: args
                            .get("preview_type")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        output_mode: args
                            .get("output_mode")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        selector: args
                            .get("selector")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        width: args.get("width").and_then(|v| v.as_u64()).map(|v| v as u32),
                        height: args.get("height").and_then(|v| v.as_u64()).map(|v| v as u32),
                        delay_ms: args.get("delay_ms").and_then(|v| v.as_u64()),
                    };
                    match capture_artifact_screenshot(&app_handle, request).await {
                        Ok(response) => build_capture_artifact_screenshot_result(&response),
                        Err(e) => {
                            error!(error = %e, "capture_artifact_screenshot tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown artifact tool: {}", tool_name)}],
                    "isError": true
                }),
            }
        }
        "dynamic_mcp" => execute_dynamic_mcp_tool(&app_handle, &tool_name, &args, conversation_id)?,
        "ui_interaction" => match tool_name.as_str() {
            "ask_user_question" => {
                use interaction::{request_ask_user_question, AskUserQuestionRequest};

                let request: AskUserQuestionRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid AskUserQuestion parameters: {}", e))?;

                let state = app_handle
                    .try_state::<InteractionState>()
                    .ok_or_else(|| "InteractionState not found".to_string())?;

                match request_ask_user_question(
                    &app_handle,
                    state.inner(),
                    conversation_id,
                    request,
                )
                .await
                {
                    Ok(answers) => serde_json::json!({
                        "content": [{"type": "json", "json": {"answers": answers}}],
                        "isError": false
                    }),
                    Err(e) => {
                        error!(error = %e, "AskUserQuestion tool execution failed");
                        serde_json::json!({
                            "content": [{"type": "text", "text": e}],
                            "isError": true
                        })
                    }
                }
            }
            "preview_file" => {
                use interaction::{emit_preview_file_request, PreviewFileRequest};

                let request: PreviewFileRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid PreviewFile parameters: {}", e))?;

                match emit_preview_file_request(&app_handle, conversation_id, request).await {
                    Ok(request_id) => serde_json::json!({
                        "content": [{"type": "json", "json": {"status": "preview_shown", "request_id": request_id}}],
                        "isError": false
                    }),
                    Err(e) => {
                        error!(error = %e, "PreviewFile tool execution failed");
                        serde_json::json!({
                            "content": [{"type": "text", "text": e}],
                            "isError": true
                        })
                    }
                }
            }
            "preview_code" => {
                use interaction::{request_preview_code, PreviewCodeRequest};

                let request: PreviewCodeRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid PreviewCode parameters: {}", e))?;

                let state = app_handle
                    .try_state::<InteractionState>()
                    .ok_or_else(|| "InteractionState not found".to_string())?;

                match request_preview_code(&app_handle, state.inner(), conversation_id, request)
                    .await
                {
                    Ok(result) => serde_json::json!({
                        "content": [{"type": "json", "json": result}],
                        "isError": false
                    }),
                    Err(e) => {
                        error!(error = %e, "PreviewCode tool execution failed");
                        serde_json::json!({
                            "content": [{"type": "text", "text": e}],
                            "isError": true
                        })
                    }
                }
            }
            _ => serde_json::json!({
                "content": [{"type": "text", "text": format!("Unknown ui_interaction tool: {}", tool_name)}],
                "isError": true
            }),
        },
        "agent" => {
            use crate::api::ai::types::AiRequest;
            use crate::api::ai_api::ask_ai;
            use crate::api::butler_api::{
                spawn_butler_task_watcher, spawn_butler_task_with_window, SpawnButlerTaskRequest,
            };
            use crate::api::operation_api::{confirm_acp_permission, confirm_operation_permission};
            use crate::mcp::builtin_mcp::templates::{
                is_butler_conversation_kind, is_butler_only_agent_tool,
            };
            use agent::types::*;

            // Runtime guard: butler-only agent tools require butler conversation
            if is_butler_only_agent_tool(&tool_name) {
                let is_butler_conv = conversation_id
                    .and_then(|cid| get_conversation_kind(&app_handle, cid))
                    .map(|kind| is_butler_conversation_kind(&kind))
                    .unwrap_or(false);
                if !is_butler_conv {
                    return Ok(serde_json::to_string(&serde_json::json!({
                        "content": [{"type": "text", "text": format!("Tool '{}' is only available in Butler conversations", tool_name)}],
                        "isError": true
                    })).unwrap());
                }
            }

            let handler = AgentHandler::new(app_handle.clone());

            match tool_name.as_str() {
                "load_skill" => {
                    let command = args
                        .get("command")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: command".to_string())?;
                    let source_type = args
                        .get("source_type")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: source_type".to_string())?;

                    let request = LoadSkillRequest {
                        command: command.to_string(),
                        source_type: source_type.to_string(),
                    };

                    match handler.load_skill(request).await {
                        Ok(response) => {
                            if response.found {
                                // Build the content text
                                let mut text = response.content.clone();

                                // Append additional files if any
                                if !response.additional_files.is_empty() {
                                    text.push_str("\n\n---\n## Additional Files\n\n");
                                    for file in &response.additional_files {
                                        text.push_str(&format!(
                                            "### {}\n```\n{}\n```\n\n",
                                            file.path, file.content
                                        ));
                                    }
                                }

                                serde_json::json!({
                                    "content": [{"type": "text", "text": text}],
                                    "isError": false,
                                    "metadata": {
                                        "identifier": response.identifier,
                                        "found": true,
                                        "additional_files_count": response.additional_files.len()
                                    }
                                })
                            } else {
                                serde_json::json!({
                                    "content": [{"type": "text", "text": response.error.unwrap_or_else(|| "Skill not found".to_string())}],
                                    "isError": true,
                                    "metadata": {
                                        "identifier": response.identifier,
                                        "found": false
                                    }
                                })
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "load_skill tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "todo_write" => {
                    use crate::api::todo_api::{emit_todo_update, TodoItemResponse};
                    use agent::todo::{TodoHandler, TodoItem, TodoState, TodoWriteRequest};

                    // Get TodoState from app state (must use the managed state)
                    let state = app_handle
                        .try_state::<TodoState>()
                        .map(|s| s.inner().clone())
                        .unwrap_or_else(TodoState::new);

                    let todo_handler = TodoHandler::new(state.clone());

                    // Parse todos array
                    let todos_value = match args.get("todos") {
                        Some(value) => {
                            debug!(has_todos = true, "todo_write args parsed");
                            value
                        }
                        None => {
                            debug!(has_todos = false, args = ?args, "todo_write args missing todos");
                            return Err("Missing required parameter: todos".to_string());
                        }
                    };

                    let todos: Vec<TodoItem> = serde_json::from_value(todos_value.clone())
                        .map_err(|e| format!("Invalid todos format: {}", e))?;

                    let request = TodoWriteRequest { todos };

                    match todo_handler.todo_write(request, conversation_id) {
                        Ok(response) => {
                            // Emit todo_update event to frontend
                            if let Some(conv_id) = conversation_id {
                                let stored_todos = state.get_todos(conv_id);
                                let todo_responses: Vec<TodoItemResponse> = stored_todos
                                    .into_iter()
                                    .map(|t| TodoItemResponse {
                                        content: t.content,
                                        status: t.status.to_string(),
                                        active_form: t.active_form,
                                    })
                                    .collect();
                                emit_todo_update(&app_handle, conv_id, &todo_responses);
                            }

                            let text = format!(
                                "{}\n\nCurrent task: {}",
                                response.message,
                                response.current_task.as_deref().unwrap_or("None")
                            );
                            serde_json::json!({
                                "content": [{"type": "text", "text": text}],
                                "isError": false,
                                "metadata": {
                                    "total": response.total,
                                    "pending": response.pending,
                                    "in_progress": response.in_progress,
                                    "completed": response.completed,
                                    "current_task": response.current_task
                                }
                            })
                        }
                        Err(e) => {
                            error!(error = %e, "todo_write tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "load_mcp_server" | "load_mcp_tool" => {
                    execute_dynamic_mcp_tool(&app_handle, &tool_name, &args, conversation_id)?
                }
                "spawn_task_conversation" => {
                    let butler_conversation_id = args
                        .get("butler_conversation_id")
                        .and_then(value_as_i64)
                        .or(conversation_id)
                        .ok_or_else(|| {
                            "spawn_task_conversation requires butler conversation context"
                                .to_string()
                        })?;
                    let title = args
                        .get("title")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: title".to_string())?;
                    let goal = args
                        .get("goal")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: goal".to_string())?;

                    let request = SpawnButlerTaskRequest {
                        butler_conversation_id,
                        title: title.to_string(),
                        goal: goal.to_string(),
                        executor_assistant_id: argument_i64(&args, "executor_assistant_id"),
                        executor_assistant_name: args
                            .get("executor_assistant_name")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        handoff_contract_json: args
                            .get("handoff_contract_json")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        result_handling_mode: args
                            .get("result_handling_mode")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        notification_policy: args
                            .get("notification_policy")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        temporary_trusted_paths: argument_string_array(
                            &args,
                            "temporary_trusted_paths",
                        )?,
                        temporary_skill_identifiers: argument_string_array(
                            &args,
                            "temporary_skill_identifiers",
                        )?,
                    };

                    let window = resolve_butler_spawn_window(&app_handle)?;
                    match spawn_butler_task_with_window(&app_handle, &window, request).await {
                        Ok(response) => serde_json::json!({
                            "content": [{"type": "json", "json": response}],
                            "isError": false
                        }),
                        Err(e) => {
                            error!(error = %e, "spawn_task_conversation tool execution failed");
                            serde_json::json!({
                                "content": [{"type": "text", "text": e}],
                                "isError": true
                            })
                        }
                    }
                }
                "task_conversation_operation" => {
                    let task_conversation_id =
                        args.get("task_conversation_id").and_then(value_as_i64).ok_or_else(
                            || "Missing required parameter: task_conversation_id".to_string(),
                        )?;
                    let action = args
                        .get("action")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "Missing required parameter: action".to_string())?;
                    let detail = resolve_accessible_butler_task_detail(
                        &app_handle,
                        conversation_id,
                        task_conversation_id,
                    )
                    .await?;

                    match action {
                        "read" => {
                            let latest_count = args
                                .get("latest_count")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as usize)
                                .unwrap_or(1);
                            let verbose =
                                args.get("verbose").and_then(|v| v.as_bool()).unwrap_or(false);
                            match build_task_conversation_read_payload(
                                &app_handle,
                                &detail,
                                latest_count,
                                verbose,
                            )
                            .await
                            {
                                Ok(payload) => serde_json::json!({
                                    "content": [{"type": "json", "json": payload}],
                                    "isError": false
                                }),
                                Err(e) => {
                                    error!(error = %e, "task_conversation_operation read failed");
                                    serde_json::json!({
                                        "content": [{"type": "text", "text": e}],
                                        "isError": true
                                    })
                                }
                            }
                        }
                        "reply_prompt" => {
                            if detail.task.is_finalized {
                                serde_json::json!({
                                    "content": [{"type": "text", "text": "Task conversation is already finalized and cannot accept a new prompt."}],
                                    "isError": true
                                })
                            } else if detail.runtime_state.is_running {
                                serde_json::json!({
                                    "content": [{"type": "text", "text": "Task conversation is currently running. Read the latest state or resolve pending permissions before sending another prompt."}],
                                    "isError": true
                                })
                            } else {
                                let prompt = args
                                    .get("prompt")
                                    .and_then(|v| v.as_str())
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .ok_or_else(|| {
                                        "Missing required parameter: prompt".to_string()
                                    })?;
                                let assistant_id =
                                    detail.conversation.assistant_id.ok_or_else(|| {
                                        "Task conversation has no assigned assistant".to_string()
                                    })?;
                                let window = resolve_butler_spawn_window(&app_handle)?;
                                let app_handle_clone = app_handle.clone();
                                let window_clone = window.clone();
                                let prompt_owned = prompt.to_string();
                                // std::thread::spawn + block_on is intentional here to break
                                // the recursive async cycle between execute_aipp_builtin_tool
                                // and ask_ai (which invokes tools that call back into
                                // execute_aipp_builtin_tool).
                                std::thread::spawn(move || {
                                    tauri::async_runtime::block_on(async move {
                                        let task_conversation_id_for_spawn = task_conversation_id;
                                        let request = AiRequest {
                                            conversation_id: task_conversation_id_for_spawn
                                                .to_string(),
                                            assistant_id,
                                            prompt: prompt_owned,
                                            model: None,
                                            override_model_id: None,
                                            temperature: None,
                                            top_p: None,
                                            max_tokens: None,
                                            stream: Some(true),
                                            attachment_list: None,
                                        };
                                        match ask_ai(
                                        app_handle_clone.clone(),
                                        app_handle_clone.state::<crate::AppState>(),
                                        app_handle_clone.state::<crate::AcpSessionState>(),
                                        app_handle_clone.state::<crate::FeatureConfigState>(),
                                        app_handle_clone
                                            .state::<crate::state::message_token::MessageTokenManager>(),
                                        app_handle_clone.state::<
                                            crate::state::activity_state::ConversationActivityManager,
                                        >(),
                                        window_clone,
                                        request,
                                        None,
                                        None,
                                        None,
                                        None,
                                        Some("internal".to_string()),
                                    )
                                    .await
                                    {
                                        Ok(_) => {
                                            spawn_butler_task_watcher(
                                                app_handle_clone.clone(),
                                                task_conversation_id_for_spawn,
                                            );
                                        }
                                        Err(error) => {
                                            error!(
                                                task_conversation_id = task_conversation_id_for_spawn,
                                                error = %error,
                                                "task_conversation_operation reply_prompt failed"
                                            );
                                        }
                                    }
                                    });
                                });

                                serde_json::json!({
                                    "content": [{
                                        "type": "json",
                                        "json": {
                                            "status": "prompt_dispatched",
                                            "task_conversation_id": task_conversation_id,
                                            "prompt": prompt
                                        }
                                    }],
                                    "isError": false
                                })
                            }
                        }
                        "mcp_tool_execute" => {
                            let pending_tool_call = resolve_pending_mcp_tool_call(
                                &app_handle,
                                task_conversation_id,
                                &args,
                            )
                            .await?;
                            let window = resolve_butler_spawn_window(&app_handle)?;
                            let task_tool_calls = MCPDatabase::new(&app_handle)
                                .map_err(|e| e.to_string())?
                                .get_mcp_tool_calls_by_conversation(task_conversation_id)
                                .map_err(|e| e.to_string())?;
                            let same_anchor_tool_call_ids = collect_anchor_group_tool_call_ids(
                                &task_tool_calls,
                                tool_call_anchor_message_id(&pending_tool_call),
                            );
                            spawn_task_conversation_mcp_tool_execution(
                                app_handle.clone(),
                                window,
                                task_conversation_id,
                                pending_tool_call.clone(),
                            );

                            serde_json::json!({
                                "content": [{
                                    "type": "json",
                                    "json": {
                                        "status": "mcp_tool_execution_dispatched",
                                        "tool_call_id": pending_tool_call.id,
                                        "task_conversation_id": task_conversation_id,
                                        "same_anchor_tool_call_ids": same_anchor_tool_call_ids,
                                    }
                                }],
                                "isError": false
                            })
                        }
                        "permission_confirm" | "operate_confirm" => {
                            let decision =
                                args.get("decision").and_then(|v| v.as_str()).ok_or_else(|| {
                                    "Missing required parameter: decision".to_string()
                                })?;
                            let request_id = resolve_operation_permission_request_id(
                                &app_handle,
                                task_conversation_id,
                                &args,
                            )
                            .await?;
                            let snapshot = app_handle
                                .state::<OperationState>()
                                .get_permission_request(&request_id)
                                .await
                                .ok_or_else(|| {
                                    "Pending operation permission request not found".to_string()
                                })?;
                            if decision == "allow_and_save" {
                                serde_json::json!({
                                    "content": [{
                                        "type": "text",
                                        "text": "Butler cannot use allow_and_save automatically. Persistent whitelist grants require explicit user review."
                                    }],
                                    "isError": true
                                })
                            } else if let Some(reason) =
                                operation_permission_manual_review_reason(&snapshot)
                            {
                                serde_json::json!({
                                    "content": [{
                                        "type": "text",
                                        "text": format!("This permission requires explicit user review and cannot be resolved by Butler automatically: {}", reason)
                                    }],
                                    "isError": true
                                })
                            } else {
                                match confirm_operation_permission(
                                    app_handle.clone(),
                                    request_id.clone(),
                                    decision.to_string(),
                                )
                                .await
                                {
                                    Ok(result) => serde_json::json!({
                                        "content": [{
                                            "type": "json",
                                            "json": {
                                                "status": "permission_confirmed",
                                                "request_id": request_id,
                                                "decision": decision,
                                                "resolved": result
                                            }
                                        }],
                                        "isError": false
                                    }),
                                    Err(e) => {
                                        error!(error = %e, "task_conversation_operation permission_confirm failed");
                                        serde_json::json!({
                                            "content": [{"type": "text", "text": e}],
                                            "isError": true
                                        })
                                    }
                                }
                            }
                        }
                        "acp_permission_confirm" => {
                            let request_id = resolve_acp_permission_request_id(
                                &app_handle,
                                task_conversation_id,
                                &args,
                            )
                            .await?;
                            let option_id = args
                                .get("option_id")
                                .and_then(|v| v.as_str())
                                .map(|v| v.to_string());
                            let cancelled =
                                args.get("cancelled").and_then(|v| v.as_bool()).unwrap_or(false);
                            if !cancelled && option_id.is_none() {
                                return Err(
                                    "acp_permission_confirm requires option_id or cancelled=true"
                                        .to_string(),
                                );
                            }
                            let snapshot = app_handle
                                .state::<AcpPermissionState>()
                                .get_request(&request_id)
                                .await
                                .ok_or_else(|| {
                                    "Pending ACP permission request not found".to_string()
                                })?;
                            if let Some(reason) =
                                acp_permission_manual_review_reason(&snapshot, option_id.as_deref())
                            {
                                serde_json::json!({
                                    "content": [{
                                        "type": "text",
                                        "text": format!("This ACP permission requires explicit user review and cannot be resolved by Butler automatically: {}", reason)
                                    }],
                                    "isError": true
                                })
                            } else {
                                match confirm_acp_permission(
                                    app_handle.clone(),
                                    request_id.clone(),
                                    option_id.clone(),
                                    Some(cancelled),
                                )
                                .await
                                {
                                    Ok(result) => serde_json::json!({
                                        "content": [{
                                            "type": "json",
                                            "json": {
                                                "status": "acp_permission_confirmed",
                                                "request_id": request_id,
                                                "option_id": option_id,
                                                "cancelled": cancelled,
                                                "resolved": result
                                            }
                                        }],
                                        "isError": false
                                    }),
                                    Err(e) => {
                                        error!(error = %e, "task_conversation_operation acp_permission_confirm failed");
                                        serde_json::json!({
                                            "content": [{"type": "text", "text": e}],
                                            "isError": true
                                        })
                                    }
                                }
                            }
                        }
                        "ask_user_respond" => {
                            let cancelled =
                                args.get("cancelled").and_then(|v| v.as_bool()).unwrap_or(false);
                            let answers: Option<std::collections::HashMap<String, String>> =
                                if cancelled {
                                    None
                                } else {
                                    let raw = args
                                    .get("answers")
                                    .and_then(|v| v.as_object())
                                    .ok_or_else(|| "ask_user_respond requires answers object or cancelled=true".to_string())?;
                                    let map: std::collections::HashMap<String, String> = raw
                                        .iter()
                                        .map(|(k, v)| {
                                            (k.clone(), v.as_str().unwrap_or("").to_string())
                                        })
                                        .collect();
                                    if map.is_empty() {
                                        return Err("answers object must not be empty".to_string());
                                    }
                                    Some(map)
                                };

                            let request_id = resolve_ask_user_request_id(
                                &app_handle,
                                task_conversation_id,
                                &args,
                            )
                            .await?;

                            match interaction::resolve_ask_user_question_response(
                                &app_handle,
                                &request_id,
                                answers.clone(),
                                cancelled,
                            )
                            .await
                            {
                                Ok(_) => serde_json::json!({
                                    "content": [{
                                        "type": "json",
                                        "json": {
                                            "status": "ask_user_responded",
                                            "request_id": request_id,
                                            "cancelled": cancelled,
                                            "answers": answers
                                        }
                                    }],
                                    "isError": false
                                }),
                                Err(e) => {
                                    error!(error = %e, "task_conversation_operation ask_user_respond failed");
                                    serde_json::json!({
                                        "content": [{"type": "text", "text": e}],
                                        "isError": true
                                    })
                                }
                            }
                        }
                        _ => serde_json::json!({
                            "content": [{"type": "text", "text": format!("Unknown task_conversation_operation action: {}", action)}],
                            "isError": true
                        }),
                    }
                }
                "schedule_task" => {
                    use crate::api::scheduled_task_api::{
                        create_scheduled_task, delete_scheduled_task, list_scheduled_tasks,
                        CreateScheduledTaskRequest, UpdateScheduledTaskRequest,
                    };
                    use crate::db::assistant_db::AssistantDatabase;
                    use crate::db::scheduled_task_db::ScheduledTaskDatabase;

                    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
                    let butler_cid = args
                        .get("butler_conversation_id")
                        .and_then(|v| v.as_i64())
                        .or(conversation_id);

                    match action {
                        "create" => {
                            let name =
                                args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            if name.is_empty() {
                                return Ok(r#"{"content":[{"type":"text","text":"缺少必要参数 name"}],"isError":true}"#.to_string());
                            }
                            let schedule_type = args
                                .get("schedule_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("interval")
                                .to_string();
                            let task_prompt = args
                                .get("task_prompt")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if task_prompt.is_empty() {
                                return Ok(r#"{"content":[{"type":"text","text":"缺少必要参数 task_prompt"}],"isError":true}"#.to_string());
                            }

                            // Resolve assistant_id from assistant_name if needed
                            let mut assistant_id =
                                args.get("assistant_id").and_then(|v| v.as_i64()).unwrap_or(0);
                            if assistant_id == 0 {
                                if let Some(name_str) =
                                    args.get("assistant_name").and_then(|v| v.as_str())
                                {
                                    if let Ok(db) = AssistantDatabase::new(&app_handle) {
                                        if let Ok(assistants) = db.get_assistants() {
                                            if let Some(found) =
                                                assistants.iter().find(|a| a.name == name_str)
                                            {
                                                assistant_id = found.id;
                                            }
                                        }
                                    }
                                }
                            }
                            if assistant_id == 0 {
                                return Ok(r#"{"content":[{"type":"text","text":"无法确定执行助手，请提供 assistant_id 或有效的 assistant_name"}],"isError":true}"#.to_string());
                            }

                            let notify_prompt = args
                                .get("notify_prompt")
                                .and_then(|v| v.as_str())
                                .unwrap_or("如果任务结果包含重要信息或需要用户关注的内容则通知")
                                .to_string();

                            let request = CreateScheduledTaskRequest {
                                name,
                                is_enabled: true,
                                schedule_type,
                                interval_value: args.get("interval_value").and_then(|v| v.as_i64()),
                                interval_unit: args
                                    .get("interval_unit")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                start_time: args
                                    .get("start_time")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                week_days: args.get("week_days").and_then(|v| v.as_array()).map(
                                    |arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_i64().map(|n| n as i32))
                                            .collect()
                                    },
                                ),
                                month_days: args.get("month_days").and_then(|v| v.as_array()).map(
                                    |arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_i64().map(|n| n as i32))
                                            .collect()
                                    },
                                ),
                                run_at: args
                                    .get("run_at")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                                assistant_id,
                                task_prompt,
                                notify_prompt,
                                butler_conversation_id: butler_cid,
                            };

                            match create_scheduled_task(app_handle.clone(), request).await {
                                Ok(dto) => serde_json::json!({
                                    "content": [{"type": "json", "json": dto}],
                                    "isError": false
                                }),
                                Err(e) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("创建定时任务失败: {}", e)}],
                                    "isError": true
                                }),
                            }
                        }
                        "list" => {
                            if let Some(bcid) = butler_cid {
                                match crate::api::scheduled_task_api::list_butler_scheduled_tasks(
                                    app_handle.clone(),
                                    bcid,
                                )
                                .await
                                {
                                    Ok(tasks) => serde_json::json!({
                                        "content": [{"type": "json", "json": tasks}],
                                        "isError": false
                                    }),
                                    Err(e) => serde_json::json!({
                                        "content": [{"type": "text", "text": format!("列出定时任务失败: {}", e)}],
                                        "isError": true
                                    }),
                                }
                            } else {
                                match list_scheduled_tasks(app_handle.clone()).await {
                                    Ok(tasks) => serde_json::json!({
                                        "content": [{"type": "json", "json": tasks}],
                                        "isError": false
                                    }),
                                    Err(e) => serde_json::json!({
                                        "content": [{"type": "text", "text": format!("列出定时任务失败: {}", e)}],
                                        "isError": true
                                    }),
                                }
                            }
                        }
                        "get" => {
                            let task_id = match args.get("task_id").and_then(|v| v.as_i64()) {
                                Some(id) => id,
                                None => return Ok(r#"{"content":[{"type":"text","text":"缺少必要参数 task_id"}],"isError":true}"#.to_string()),
                            };
                            let db = ScheduledTaskDatabase::new(&app_handle)
                                .map_err(|e| e.to_string())?;
                            match db.read_task(task_id) {
                                Ok(Some(task)) => {
                                    let runs = db.list_runs_by_task(task_id, 5).unwrap_or_default();
                                    let dto = crate::api::scheduled_task_api::to_dto(task);
                                    serde_json::json!({
                                        "content": [{"type": "json", "json": {
                                            "task": dto,
                                            "recent_runs": runs
                                        }}],
                                        "isError": false
                                    })
                                }
                                Ok(None) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("定时任务 {} 不存在", task_id)}],
                                    "isError": true
                                }),
                                Err(e) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("查询失败: {}", e)}],
                                    "isError": true
                                }),
                            }
                        }
                        "update" => {
                            let task_id = match args.get("task_id").and_then(|v| v.as_i64()) {
                                Some(id) => id,
                                None => return Ok(r#"{"content":[{"type":"text","text":"缺少必要参数 task_id"}],"isError":true}"#.to_string()),
                            };
                            let db = ScheduledTaskDatabase::new(&app_handle)
                                .map_err(|e| e.to_string())?;
                            let existing = match db.read_task(task_id) {
                                Ok(Some(t)) => t,
                                Ok(None) => {
                                    return Ok(format!(
                                        r#"{{"content":[{{"type":"text","text":"定时任务 {} 不存在"}}],"isError":true}}"#,
                                        task_id
                                    ))
                                }
                                Err(e) => {
                                    return Ok(format!(
                                        r#"{{"content":[{{"type":"text","text":"查询失败: {}"}}],"isError":true}}"#,
                                        e
                                    ))
                                }
                            };

                            let mut new_assistant_id = args
                                .get("assistant_id")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(existing.assistant_id);
                            if let Some(name_str) =
                                args.get("assistant_name").and_then(|v| v.as_str())
                            {
                                if let Ok(adb) = AssistantDatabase::new(&app_handle) {
                                    if let Ok(assistants) = adb.get_assistants() {
                                        if let Some(found) =
                                            assistants.iter().find(|a| a.name == name_str)
                                        {
                                            new_assistant_id = found.id;
                                        }
                                    }
                                }
                            }

                            let request = UpdateScheduledTaskRequest {
                                id: task_id,
                                name: args
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&existing.name)
                                    .to_string(),
                                is_enabled: existing.is_enabled,
                                schedule_type: args
                                    .get("schedule_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&existing.schedule_type)
                                    .to_string(),
                                interval_value: args
                                    .get("interval_value")
                                    .and_then(|v| v.as_i64())
                                    .or(existing.interval_value),
                                interval_unit: args
                                    .get("interval_unit")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .or(existing.interval_unit),
                                start_time: args
                                    .get("start_time")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .or(existing.start_time),
                                week_days: args
                                    .get("week_days")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_i64().map(|n| n as i32))
                                            .collect()
                                    })
                                    .or_else(|| {
                                        crate::api::scheduled_task_api::parse_json_array(
                                            &existing.week_days,
                                        )
                                    }),
                                month_days: args
                                    .get("month_days")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_i64().map(|n| n as i32))
                                            .collect()
                                    })
                                    .or_else(|| {
                                        crate::api::scheduled_task_api::parse_json_array(
                                            &existing.month_days,
                                        )
                                    }),
                                run_at: args
                                    .get("run_at")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .or_else(|| existing.run_at.map(|dt| dt.to_rfc3339())),
                                assistant_id: new_assistant_id,
                                task_prompt: args
                                    .get("task_prompt")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&existing.task_prompt)
                                    .to_string(),
                                notify_prompt: args
                                    .get("notify_prompt")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(&existing.notify_prompt)
                                    .to_string(),
                            };

                            match crate::api::scheduled_task_api::update_scheduled_task(
                                app_handle.clone(),
                                request,
                            )
                            .await
                            {
                                Ok(dto) => serde_json::json!({
                                    "content": [{"type": "json", "json": dto}],
                                    "isError": false
                                }),
                                Err(e) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("更新定时任务失败: {}", e)}],
                                    "isError": true
                                }),
                            }
                        }
                        "delete" => {
                            let task_id = match args.get("task_id").and_then(|v| v.as_i64()) {
                                Some(id) => id,
                                None => return Ok(r#"{"content":[{"type":"text","text":"缺少必要参数 task_id"}],"isError":true}"#.to_string()),
                            };
                            match delete_scheduled_task(app_handle.clone(), task_id).await {
                                Ok(()) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("定时任务 {} 已删除", task_id)}],
                                    "isError": false
                                }),
                                Err(e) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("删除定时任务失败: {}", e)}],
                                    "isError": true
                                }),
                            }
                        }
                        "enable" | "disable" => {
                            let task_id = match args.get("task_id").and_then(|v| v.as_i64()) {
                                Some(id) => id,
                                None => return Ok(r#"{"content":[{"type":"text","text":"缺少必要参数 task_id"}],"isError":true}"#.to_string()),
                            };
                            let db = ScheduledTaskDatabase::new(&app_handle)
                                .map_err(|e| e.to_string())?;
                            match db.read_task(task_id) {
                                Ok(Some(mut task)) => {
                                    task.is_enabled = action == "enable";
                                    task.updated_time = chrono::Utc::now();
                                    match db.update_task(&task) {
                                        Ok(()) => {
                                            let state_str = if action == "enable" {
                                                "已启用"
                                            } else {
                                                "已禁用"
                                            };
                                            serde_json::json!({
                                                "content": [{"type": "text", "text": format!("定时任务 {} {}", task_id, state_str)}],
                                                "isError": false
                                            })
                                        }
                                        Err(e) => serde_json::json!({
                                            "content": [{"type": "text", "text": format!("操作失败: {}", e)}],
                                            "isError": true
                                        }),
                                    }
                                }
                                Ok(None) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("定时任务 {} 不存在", task_id)}],
                                    "isError": true
                                }),
                                Err(e) => serde_json::json!({
                                    "content": [{"type": "text", "text": format!("查询失败: {}", e)}],
                                    "isError": true
                                }),
                            }
                        }
                        _ => serde_json::json!({
                            "content": [{"type": "text", "text": format!("Unknown schedule_task action: {}", action)}],
                            "isError": true
                        }),
                    }
                }
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown agent tool: {}", tool_name)}],
                    "isError": true
                }),
            }
        }
        "superadmin" => {
            // Runtime guard: superadmin tools require butler conversation
            let is_butler_conv = conversation_id
                .and_then(|cid| get_conversation_kind(&app_handle, cid))
                .map(|kind| templates::is_butler_conversation_kind(&kind))
                .unwrap_or(false);
            if !is_butler_conv {
                serde_json::json!({
                    "content": [{"type": "text", "text": format!("Tool '{}' is only available in Butler conversations", tool_name)}],
                    "isError": true
                })
            } else {
                superadmin::dispatch(&app_handle, &tool_name, &args, conversation_id).await
            }
        }
        _ => serde_json::json!({
            "content": [{"type": "text", "text": format!("Unknown builtin command: {}", cmd_id)}],
            "isError": true
        }),
    };

    Ok(serde_json::to_string(&result_value).unwrap_or_else(|_| "{}".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ai::acp::{
        AcpPermissionOptionPayload, AcpPermissionRequestEvent, AcpPermissionRequestSnapshot,
    };
    use crate::db::mcp_db::MCPToolCall;
    use crate::mcp::builtin_mcp::operation::types::PermissionRequestEvent;

    #[test]
    fn dynamic_mcp_server_payload_is_compact() {
        let payload = build_dynamic_mcp_server_item(
            "Search",
            "Web search and fetch tools",
            vec![build_dynamic_mcp_server_tool_item("web_fetch", "Fetch a URL as markdown")],
        );

        let object = payload.as_object().expect("server payload should be an object");
        assert_eq!(object.len(), 3);
        assert_eq!(payload["server"], "Search");
        assert_eq!(payload["summary"], "Web search and fetch tools");
        assert_eq!(payload["tools"][0]["tool"], "web_fetch");
        assert_eq!(payload["tools"][0]["summary"], "Fetch a URL as markdown");
        assert!(payload.get("server_id").is_none());
        assert!(payload.get("toolset_id").is_none());
        assert!(payload.get("toolset_name").is_none());
        assert!(payload.get("epoch").is_none());
    }

    #[test]
    fn dynamic_mcp_loaded_tool_payload_is_compact() {
        let payload = build_dynamic_mcp_loaded_tool_item(
            "Search",
            "web_fetch",
            "Fetch a URL as markdown".to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
        );

        let object = payload.as_object().expect("loaded tool payload should be an object");
        assert_eq!(object.len(), 4);
        assert_eq!(payload["server"], "Search");
        assert_eq!(payload["tool"], "web_fetch");
        assert_eq!(payload["description"], "Fetch a URL as markdown");
        assert_eq!(payload["parameters"]["required"][0], "url");
        assert!(payload.get("tool_id").is_none());
        assert!(payload.get("summary").is_none());
        assert!(payload.get("parameters_json").is_none());
        assert!(payload.get("is_auto_run").is_none());
        assert!(payload.get("tool_definition").is_none());
    }

    #[test]
    fn acp_native_duplicate_filter_hides_file_and_terminal_overlap() {
        let operation_server_ids = std::collections::HashSet::from([7]);

        assert!(is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "read_file"
        ));
        assert!(is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "write_file"
        ));
        assert!(is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "execute_bash"
        ));
        assert!(is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "get_bash_output"
        ));
    }

    #[test]
    fn acp_native_duplicate_filter_keeps_non_overlapping_tools() {
        let operation_server_ids = std::collections::HashSet::from([7]);

        assert!(!is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "edit_file"
        ));
        assert!(!is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "list_directory"
        ));
        assert!(!is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            7,
            "ask_user_question"
        ));
        assert!(!is_acp_native_duplicate_dynamic_tool(
            &operation_server_ids,
            8,
            "read_file"
        ));
    }

    #[test]
    fn get_artifact_workspace_uses_bound_conversation_context() {
        let args = serde_json::json!({ "conversation_id": 999 });
        let resolved =
            resolve_artifact_tool_conversation_id("get_artifact_workspace", &args, Some(123))
                .expect("bound conversation context should resolve");

        assert_eq!(resolved, 123);
    }

    #[test]
    fn show_artifact_can_still_override_conversation_context() {
        let args = serde_json::json!({ "conversation_id": 999 });
        let resolved = resolve_artifact_tool_conversation_id("show_artifact", &args, Some(123))
            .expect("show_artifact should resolve");

        assert_eq!(resolved, 999);
    }

    #[test]
    fn value_as_i64_accepts_numeric_strings() {
        assert_eq!(value_as_i64(&serde_json::json!(44)), Some(44));
        assert_eq!(value_as_i64(&serde_json::json!("44")), Some(44));
        assert_eq!(value_as_i64(&serde_json::json!(" 570 ")), Some(570));
        assert_eq!(value_as_i64(&serde_json::json!("abc")), None);
    }

    #[test]
    fn spawn_task_related_ids_accept_numeric_strings() {
        let args = serde_json::json!({
            "butler_conversation_id": "568",
            "executor_assistant_id": "44",
            "task_conversation_id": "570"
        });

        assert_eq!(argument_i64(&args, "butler_conversation_id"), Some(568));
        assert_eq!(argument_i64(&args, "executor_assistant_id"), Some(44));
        assert_eq!(argument_i64(&args, "task_conversation_id"), Some(570));
    }

    #[test]
    fn show_artifact_accepts_string_conversation_override() {
        let args = serde_json::json!({ "conversation_id": "999" });
        let resolved = resolve_artifact_tool_conversation_id("show_artifact", &args, Some(123))
            .expect("show_artifact should resolve string conversation ids");

        assert_eq!(resolved, 999);
    }

    #[test]
    fn capture_artifact_screenshot_accepts_string_conversation_override() {
        let args = serde_json::json!({ "conversation_id": "999" });
        let resolved =
            resolve_artifact_tool_conversation_id("capture_artifact_screenshot", &args, Some(123))
                .expect("capture_artifact_screenshot should resolve string conversation ids");

        assert_eq!(resolved, 999);
    }

    #[test]
    fn capture_artifact_screenshot_result_uses_mcp_image_content() {
        let response = crate::artifacts::workspace::CaptureArtifactScreenshotResponse {
            artifact_key: "demo/card".to_string(),
            entry_file: "src/App.tsx".to_string(),
            language: "tsx".to_string(),
            preview_type: "react".to_string(),
            output_mode: "base64".to_string(),
            width: 800,
            height: 600,
            mime_type: "image/png".to_string(),
            base64: Some("iVBORw0KGgoAAAANSUhEUgAA".to_string()),
            path: None,
        };

        let result = build_capture_artifact_screenshot_result(&response);

        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][1]["type"], "image");
        assert_eq!(result["content"][1]["mimeType"], "image/png");
        assert_eq!(result["content"][1]["data"], "iVBORw0KGgoAAAANSUhEUgAA");
        assert_eq!(result["structuredContent"]["artifact_key"], "demo/card");
        assert_eq!(result["structuredContent"]["entry_file"], "src/App.tsx");
        assert_eq!(result["structuredContent"]["output_mode"], "base64");
        assert_eq!(result["structuredContent"]["width"], 800);
        assert_eq!(result["structuredContent"]["height"], 600);
    }

    #[test]
    fn capture_artifact_screenshot_result_can_return_path_resource() {
        let response = crate::artifacts::workspace::CaptureArtifactScreenshotResponse {
            artifact_key: "demo/card".to_string(),
            entry_file: "src/App.tsx".to_string(),
            language: "tsx".to_string(),
            preview_type: "react".to_string(),
            output_mode: "path".to_string(),
            width: 800,
            height: 600,
            mime_type: "image/png".to_string(),
            base64: None,
            path: Some("/tmp/aipp/artifact-shot.png".to_string()),
        };

        let result = build_capture_artifact_screenshot_result(&response);

        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][1]["type"], "resource_link");
        assert_eq!(result["content"][1]["uri"], "file:///tmp/aipp/artifact-shot.png");
        assert_eq!(result["structuredContent"]["output_mode"], "path");
        assert_eq!(result["structuredContent"]["path"], "/tmp/aipp/artifact-shot.png");
    }

    #[test]
    fn operation_permission_with_rm_requires_manual_review() {
        let snapshot = PermissionRequestSnapshot {
            conversation_id: Some(1),
            event: PermissionRequestEvent {
                request_id: "req-1".to_string(),
                operation: "execute_bash".to_string(),
                path: "rm -rf /tmp/demo".to_string(),
                conversation_id: Some(1),
            },
            review_code: "OP-REQ1".to_string(),
            feishu_message_id: None,
            allowed_open_id: None,
            allowed_chat_id: None,
        };

        let reason = operation_permission_manual_review_reason(&snapshot);
        assert!(reason.is_some());
    }

    #[test]
    fn acp_permission_with_rm_requires_manual_review() {
        let snapshot = AcpPermissionRequestSnapshot {
            conversation_id: Some(1),
            event: AcpPermissionRequestEvent {
                request_id: "req-2".to_string(),
                conversation_id: Some(1),
                agent_kind: Some("acp".to_string()),
                tool_call_id: "tool-1".to_string(),
                title: Some("Run shell command".to_string()),
                kind: Some("bash".to_string()),
                parameters: Some("{\"command\":\"rm -rf /tmp/demo\"}".to_string()),
                options: vec![AcpPermissionOptionPayload {
                    option_id: "allow_once".to_string(),
                    name: "Allow once".to_string(),
                    kind: "allow_once".to_string(),
                }],
            },
            review_code: "ACP-REQ2".to_string(),
            feishu_message_id: None,
            allowed_open_id: None,
            allowed_chat_id: None,
        };

        let reason = acp_permission_manual_review_reason(&snapshot, Some("allow_once"));
        assert!(reason.is_some());
    }

    #[test]
    fn acp_persistent_allow_option_requires_manual_review() {
        let snapshot = AcpPermissionRequestSnapshot {
            conversation_id: Some(1),
            event: AcpPermissionRequestEvent {
                request_id: "req-3".to_string(),
                conversation_id: Some(1),
                agent_kind: Some("acp".to_string()),
                tool_call_id: "tool-2".to_string(),
                title: Some("Tool permission".to_string()),
                kind: Some("tool".to_string()),
                parameters: Some("{\"path\":\"/tmp/demo\"}".to_string()),
                options: vec![
                    AcpPermissionOptionPayload {
                        option_id: "allow_once".to_string(),
                        name: "Allow once".to_string(),
                        kind: "allow_once".to_string(),
                    },
                    AcpPermissionOptionPayload {
                        option_id: "allow_always".to_string(),
                        name: "Allow always".to_string(),
                        kind: "allow_always".to_string(),
                    },
                ],
            },
            review_code: "ACP-REQ3".to_string(),
            feishu_message_id: None,
            allowed_open_id: None,
            allowed_chat_id: None,
        };

        let reason = acp_permission_manual_review_reason(&snapshot, Some("allow_always"));
        assert!(reason.is_some());
    }

    fn build_mcp_tool_call(
        id: i64,
        status: &str,
        message_id: Option<i64>,
        assistant_message_id: Option<i64>,
    ) -> MCPToolCall {
        MCPToolCall {
            id,
            conversation_id: 77,
            message_id,
            subtask_id: None,
            server_id: 1,
            server_name: "search".to_string(),
            tool_name: "web_fetch".to_string(),
            parameters: r#"{"url":"https://example.com"}"#.to_string(),
            status: status.to_string(),
            result: None,
            error: None,
            created_time: "2026-01-01T00:00:00Z".to_string(),
            started_time: None,
            finished_time: None,
            llm_call_id: Some(format!("llm-{}", id)),
            assistant_message_id,
        }
    }

    #[test]
    fn pending_mcp_tool_payload_preserves_key_fields() {
        let tool_call = build_mcp_tool_call(12, "pending", Some(31), Some(41));

        let payload = build_pending_mcp_tool_call_payload(&tool_call);

        assert_eq!(payload["tool_call_id"], 12);
        assert_eq!(payload["llm_call_id"], "llm-12");
        assert_eq!(payload["server_name"], "search");
        assert_eq!(payload["tool_name"], "web_fetch");
        assert_eq!(payload["parameters"]["url"], "https://example.com");
        assert_eq!(payload["assistant_message_id"], 41);
    }

    #[test]
    fn active_anchor_group_detection_ignores_other_batches() {
        let target = build_mcp_tool_call(1, "success", Some(10), Some(100));
        let sibling_pending = build_mcp_tool_call(2, "pending", Some(10), Some(100));
        let other_anchor_pending = build_mcp_tool_call(3, "pending", Some(11), Some(101));

        assert!(has_active_anchor_group_calls(
            &[target.clone(), sibling_pending, other_anchor_pending],
            &target
        ));
        assert!(!has_active_anchor_group_calls(
            &[target.clone(), build_mcp_tool_call(4, "pending", Some(12), Some(102))],
            &target
        ));
    }
}

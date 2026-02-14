use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tracing::{debug, error, instrument};
use crate::db::mcp_db::MCPDatabase;

pub mod agent;
pub mod operation;
pub mod search;
pub mod templates;

pub use agent::{AgentHandler, TodoHandler, TodoState};
pub use operation::{OperationHandler, OperationState};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinExecutionResult {
    pub content: Vec<serde_json::Value>,
    pub is_error: bool,
}

fn matches_keyword(value: &str, keyword: &str) -> bool {
    value.to_lowercase().contains(&keyword.to_lowercase())
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

    let args: serde_json::Value = serde_json::from_str(&parameters).map_err(|e| {
        error!(error = %e, "Invalid parameters JSON");
        format!("Invalid parameters: {}", e)
    })?;

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
                                SearchResponse::Html { html_content, .. } => {
                                    serde_json::json!({
                                        "content": [{"type": "text", "text": html_content}],
                                        "isError": false
                                    })
                                }
                                SearchResponse::Markdown { markdown_content, .. } => {
                                    serde_json::json!({
                                        "content": [{"type": "text", "text": markdown_content}],
                                        "isError": false
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
        "dynamic_mcp" => {
            let db = MCPDatabase::new(&app_handle)
                .map_err(|e| format!("Failed to open MCP database: {}", e))?;
            let _ = db.rebuild_dynamic_mcp_catalog();

            match tool_name.as_str() {
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
                            })
                            .map(|tool| {
                                serde_json::json!({
                                    "tool_name": tool.tool_name,
                                    "summary": tool.summary,
                                })
                            })
                            .collect();
                        matched_servers.push(serde_json::json!({
                            "toolset_id": server.server_id,
                            "toolset_name": server.server_name,
                            "server_id": server.server_id,
                            "server_name": server.server_name,
                            "summary": server.summary,
                            "epoch": server.epoch,
                            "tools": tools,
                        }));
                    }

                    if matched_servers.is_empty() {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("No MCP toolset matched '{}'. Try another keyword.", keyword)
                            }],
                            "isError": true
                        })
                    } else {
                        serde_json::json!({
                            "content": [{
                                "type": "json",
                                "json": {
                                    "toolsets": matched_servers.clone(),
                                    "servers": matched_servers
                                }
                            }],
                            "isError": false
                        })
                    }
                }
                "load_mcp_tool" => {
                    let names = if let Some(values) = args.get("names").and_then(|v| v.as_array()) {
                        values
                            .iter()
                            .filter_map(|v| v.as_str())
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
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
                    let server_filter = args
                        .get("server_name")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_lowercase());
                    let tool_catalog = db
                        .list_tool_catalog(None)
                        .map_err(|e| format!("Failed to list MCP tool catalog: {}", e))?;
                    let mut selected = Vec::new();
                    let mut selected_ids = std::collections::HashSet::new();

                    for keyword in &names {
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
                            if let Some(filter) = &server_filter {
                                if !matches_keyword(&tool.server_name, filter) {
                                    continue;
                                }
                            }
                            let matched = matches_keyword(&tool.tool_name, keyword)
                                || matches_keyword(&tool.summary, keyword)
                                || matches_keyword(&tool.server_name, keyword);
                            if !matched {
                                continue;
                            }
                            if selected_ids.insert(tool.tool_id) {
                                selected.push(tool.clone());
                            }
                        }
                    }

                    if selected.is_empty() {
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("No MCP tool matched {:?}. Try more specific keywords.", names)
                            }],
                            "isError": true
                        })
                    } else {
                        let mut server_ids = Vec::new();
                        let mut seen_server_ids = std::collections::HashSet::new();
                        for tool in &selected {
                            if seen_server_ids.insert(tool.server_id) {
                                server_ids.push(tool.server_id);
                            }
                        }
                        let mut tool_definition_map: std::collections::HashMap<
                            i64,
                            (String, String, bool),
                        > = std::collections::HashMap::new();
                        if !server_ids.is_empty() {
                            let server_tool_pairs = db
                                .get_mcp_servers_with_tools_by_ids(&server_ids)
                                .map_err(|e| {
                                    format!("Failed to load MCP tool definitions: {}", e)
                                })?;
                            for (_server, tools) in server_tool_pairs {
                                for actual_tool in tools {
                                    if actual_tool.is_enabled {
                                        tool_definition_map.insert(
                                            actual_tool.id,
                                            (
                                                actual_tool.tool_description.unwrap_or_default(),
                                                actual_tool
                                                    .parameters
                                                    .unwrap_or_else(|| "{}".to_string()),
                                                actual_tool.is_auto_run,
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                        let mut loaded = Vec::new();
                        for tool in &selected {
                            db.upsert_conversation_loaded_tool(conversation_id, tool.tool_id, Some("manual"))
                                .map_err(|e| format!("Failed to persist loaded tool {}: {}", tool.tool_name, e))?;
                            let (description, parameters_json, is_auto_run) = tool_definition_map
                                .get(&tool.tool_id)
                                .cloned()
                                .unwrap_or_else(|| (String::new(), "{}".to_string(), false));
                            let resolved_description = if description.trim().is_empty() {
                                tool.summary.clone()
                            } else {
                                description
                            };
                            let parameters_schema = serde_json::from_str::<serde_json::Value>(&parameters_json)
                                .unwrap_or_else(|_| {
                                    serde_json::json!({
                                        "type": "object",
                                        "additionalProperties": true
                                    })
                                });
                            loaded.push(serde_json::json!({
                                "tool_id": tool.tool_id,
                                "toolset_name": tool.server_name,
                                "server_name": tool.server_name,
                                "tool_name": tool.tool_name,
                                "summary": tool.summary,
                                "description": resolved_description.clone(),
                                "parameters": parameters_schema.clone(),
                                "parameters_json": parameters_json,
                                "is_auto_run": is_auto_run,
                                "tool_definition": {
                                    "server_name": tool.server_name,
                                    "tool_name": tool.tool_name,
                                    "description": resolved_description,
                                    "parameters": parameters_schema
                                }
                            }));
                        }
                        serde_json::json!({
                            "content": [{
                                "type": "json",
                                "json": {
                                    "loaded_count": loaded.len(),
                                    "loaded_tools": loaded
                                }
                            }],
                            "isError": false
                        })
                    }
                }
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown dynamic_mcp tool: {}", tool_name)}],
                    "isError": true
                }),
            }
        }
        "agent" => {
            use agent::types::*;

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
                    use agent::todo::{TodoHandler, TodoItem, TodoState, TodoWriteRequest};
                    use crate::api::todo_api::{emit_todo_update, TodoItemResponse};

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
                _ => serde_json::json!({
                    "content": [{"type": "text", "text": format!("Unknown agent tool: {}", tool_name)}],
                    "isError": true
                }),
            }
        }
        _ => serde_json::json!({
            "content": [{"type": "text", "text": format!("Unknown builtin command: {}", cmd_id)}],
            "isError": true
        }),
    };

    Ok(serde_json::to_string(&result_value).unwrap_or_else(|_| "{}".to_string()))
}

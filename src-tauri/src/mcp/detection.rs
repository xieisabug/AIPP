use crate::api::ai::events::{ConversationEvent, MCPToolCallUpdateEvent};
use crate::api::ai_api::sanitize_tool_name;
use crate::db::conversation_db::Repository;
use crate::db::mcp_db::MCPToolCall;
use crate::utils::window_utils::send_conversation_event_to_chat_windows;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tauri::Manager; // for AppHandle.state
use tokio::sync::Mutex; // for repository.read
use tracing::{debug, error, instrument, warn};

// 对话级别的 MCP 执行状态管理
type ConversationMcpState = Arc<Mutex<HashMap<i64, u32>>>;

static CONVERSATION_MCP_DEPTH: OnceLock<ConversationMcpState> = OnceLock::new();

/// 构建并广播 MCP 工具调用状态更新事件
fn broadcast_mcp_tool_call_update(app_handle: &tauri::AppHandle, tool_call: &MCPToolCall) {
    let parse_ts = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc)
    };

    let update_event = ConversationEvent {
        r#type: "mcp_tool_call_update".to_string(),
        data: serde_json::to_value(MCPToolCallUpdateEvent {
            call_id: tool_call.id,
            conversation_id: tool_call.conversation_id,
            status: tool_call.status.clone(),
            result: tool_call.result.clone(),
            error: tool_call.error.clone(),
            started_time: tool_call.started_time.as_deref().map(parse_ts),
            finished_time: tool_call.finished_time.as_deref().map(parse_ts),
        })
        .unwrap(),
    };

    send_conversation_event_to_chat_windows(app_handle, tool_call.conversation_id, update_event);
}

const MAX_MCP_RECURSION_DEPTH: u32 = 3;

/// 专为子任务设计的 MCP 调用检测和处理函数（复用核心逻辑）
#[instrument(level = "debug", skip(app_handle, content, enabled_tools), fields(conversation_id, subtask_id, content_len = content.len()))]
pub async fn detect_and_process_mcp_calls_for_subtask(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    subtask_id: i64,
    content: &str,
    enabled_servers: &[String],
    enabled_tools: &Option<HashMap<String, Vec<String>>>,
) -> Result<Vec<crate::db::mcp_db::MCPToolCall>, anyhow::Error> {
    debug!("Detecting MCP calls for subtask in conversation");
    let mcp_regex = regex::Regex::new(r"<mcp_tool_call>\s*<server_name>([^<]*)</server_name>\s*<tool_name>([^<]*)</tool_name>\s*<parameters>([\s\S]*?)</parameters>\s*</mcp_tool_call>").unwrap();

    let mut executed_calls = Vec::new();

    // 处理所有匹配的 MCP 调用
    for cap in mcp_regex.captures_iter(content) {
        let server_name = cap[1].trim().to_string();
        let tool_name = cap[2].trim().to_string();
        let parameters = cap[3].trim().to_string();

        debug!(server = %server_name, tool = %tool_name, "Detected MCP call for subtask");

        // 检查工具是否在允许列表中
        if let Some(ref tools_map) = enabled_tools {
            if let Some(allowed_tools) = tools_map.get(&server_name) {
                if !allowed_tools.contains(&tool_name) {
                    debug!(tool = %tool_name, server = %server_name, "Tool not in enabled list for server");
                    continue;
                }
            }
        }

        // 查找服务器（复用原逻辑）
        // 支持精确匹配和清理后名称匹配（处理大模型返回的 sanitized 名称）
        let mcp_db = crate::db::mcp_db::MCPDatabase::new(app_handle)?;
        let servers = mcp_db.get_mcp_servers()?;
        let server_opt = servers.iter().find(|s| {
            s.is_enabled && (s.name == server_name || sanitize_tool_name(&s.name) == server_name)
        });

        if let Some(server) = server_opt {
            // 现在基于 server.id 来判断是否启用（enabled_servers 存储的是 id 而不是名称）
            let server_id_str = server.id.to_string();
            if !enabled_servers.contains(&server_id_str) {
                debug!(server_id = server.id, server_name = %server.name, "Server not in enabled server id list for subtask");
                continue;
            }

            // 创建工具调用记录（用于子任务）
            // 使用 server.name（原始名称）而不是 server_name（可能是清理后的名称）
            let tool_call = mcp_db.create_mcp_tool_call_for_subtask(
                conversation_id,
                subtask_id,
                server.id,
                &server.name,
                &tool_name,
                &parameters,
                None,
            )?;

            // 广播 pending 状态事件，确保前端及时显示工具调用
            broadcast_mcp_tool_call_update(app_handle, &tool_call);

            // 直接执行工具调用（复用现有执行逻辑）
            let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
            let execution_result = crate::mcp::execution_api::execute_tool_by_transport(
                app_handle,
                &feature_config_state,
                server,
                &tool_name,
                &parameters,
                Some(conversation_id),
            )
            .await;

            // 更新工具调用状态
            match execution_result {
                Ok(result) => {
                    let result_preview: String = result.chars().take(160).collect();
                    debug!(
                        call_id=tool_call.id,
                        server=%server.name,
                        tool=%tool_name,
                        result_preview=%result_preview,
                        "MCP tool call executed successfully for subtask"
                    );
                    let _ = mcp_db.update_mcp_tool_call_status(
                        tool_call.id,
                        "success",
                        Some(&result),
                        None,
                    );
                    // 手动构造更新后的结构（避免再次查询数据库）
                    let mut updated = tool_call.clone();
                    updated.status = "success".to_string();
                    updated.result = Some(result);
                    updated.error = None;
                    // 广播 success 状态事件
                    broadcast_mcp_tool_call_update(app_handle, &updated);
                    executed_calls.push(updated);
                }
                Err(error) => {
                    let chain_str = error.to_string();
                    let truncated_chain = if chain_str.len() > 400 {
                        format!("{}...", &chain_str[..400])
                    } else {
                        chain_str
                    };
                    warn!(
                        call_id=tool_call.id,
                        server=%server.name,
                        tool=%tool_name,
                        error_chain=%truncated_chain,
                        params_preview=%parameters.chars().take(120).collect::<String>(),
                        "MCP tool call failed for subtask"
                    );
                    let _ = mcp_db.update_mcp_tool_call_status(
                        tool_call.id,
                        "failed",
                        None,
                        Some(&truncated_chain),
                    );
                    let mut updated = tool_call.clone();
                    updated.status = "failed".to_string();
                    updated.result = None;
                    updated.error = Some(truncated_chain);
                    // 广播 failed 状态事件
                    broadcast_mcp_tool_call_update(app_handle, &updated);
                    executed_calls.push(updated);
                }
            }
        } else {
            debug!(server = %server_name, "Server not found or disabled");
        }
    }

    Ok(executed_calls)
}

#[instrument(level = "debug", skip(app_handle, window, content), fields(conversation_id, message_id, content_len = content.len()))]
pub async fn detect_and_process_mcp_calls(
    app_handle: &tauri::AppHandle,
    window: &tauri::Window,
    conversation_id: i64,
    message_id: i64,
    content: &str,
) -> Result<Option<String>, anyhow::Error> {
    // Check conversation-level recursion depth to prevent infinite loops
    let depth_state = CONVERSATION_MCP_DEPTH.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
    let mut depth_map = depth_state.lock().await;
    let current_depth = *depth_map.get(&conversation_id).unwrap_or(&0);

    if current_depth >= MAX_MCP_RECURSION_DEPTH {
        warn!(depth = current_depth, "MCP recursion depth limit reached, skipping detection");
        return Ok(None);
    }

    // Increment conversation-level recursion depth
    depth_map.insert(conversation_id, current_depth + 1);
    drop(depth_map); // 释放锁

    let result = async {
        let mcp_regex = regex::Regex::new(r"<mcp_tool_call>\s*<server_name>([^<]*)</server_name>\s*<tool_name>([^<]*)</tool_name>\s*<parameters>([\s\S]*?)</parameters>\s*</mcp_tool_call>").unwrap();
        let mut updated_content: Option<String> = None;

        // 只处理第一个匹配的 MCP 调用，避免单次回复中执行多个工具
        if let Some(cap) = mcp_regex.captures_iter(content).next() {
            let server_name = cap[1].trim().to_string();
            let tool_name = cap[2].trim().to_string();
            let parameters = cap[3].trim().to_string();

            debug!(server = %server_name, tool = %tool_name, "Detected MCP call in message");

            // 避免重复：若已存在相同 message_id/server/tool/parameters 的 pending/failed/success 记录，则复用
            let existing_call_opt = {
                let db = crate::db::mcp_db::MCPDatabase::new(app_handle).ok();
                db.and_then(|db| db.get_mcp_tool_calls_by_conversation(conversation_id).ok())
                    .and_then(|calls| {
                        calls.into_iter().find(|c| {
                            c.message_id == Some(message_id)
                                && c.server_name == server_name
                                && c.tool_name == tool_name
                                && c.parameters.trim() == parameters.trim()
                        })
                    })
            };

            let create_result = if let Some(existing) = existing_call_opt {
                Ok(existing)
            } else {
                crate::mcp::execution_api::create_mcp_tool_call_with_llm_id(
                    app_handle.clone(),
                    conversation_id,
                    Some(message_id),
                    server_name.clone(),
                    tool_name.clone(),
                    parameters.clone(),
                    None,
                    None,
                )
                .await
            };

            match create_result {
                Ok(tool_call) => {
                    debug!(call_id = tool_call.id, "Created MCP tool call");

                    // 将 MCP 标签替换为包含 call_id 的 UI 注释，确保前端能正确匹配工具调用的状态
                    let ui_hint = format!(
                        "<!-- MCP_TOOL_CALL:{} -->",
                        json!({
                            "server_name": server_name,
                            "tool_name": tool_name,
                            "parameters": parameters,
                            "call_id": tool_call.id,
                            "llm_call_id": tool_call.llm_call_id,
                        })
                    );
                    updated_content = Some(mcp_regex.replacen(content, 1, ui_hint).to_string());

                    // 尝试根据助手配置自动执行（is_auto_run）
                    if let Ok(conversation_db) = crate::db::conversation_db::ConversationDatabase::new(app_handle) {
                        if let Ok(repository) = conversation_db.conversation_repo() {
                            if let Ok(Some(conversation)) = repository.read(conversation_id) {
                                if let Some(assistant_id) = conversation.assistant_id {
                                    match crate::api::assistant_api::get_assistant_mcp_servers_with_tools(
                                        app_handle.clone(),
                                        assistant_id,
                                    )
                                    .await
                                    {
                                        Ok(servers_with_tools) => {
                                            let mut should_auto_run = false;
                                            for s in servers_with_tools.iter() {
                                                // 支持精确匹配和清理后名称匹配
                                                let name_matches = s.name == server_name
                                                    || sanitize_tool_name(&s.name) == server_name;
                                                if name_matches && s.is_enabled {
                                                    if let Some(tool) = s.tools.iter().find(|t| t.name == tool_name && t.is_enabled) {
                                                        if tool.is_auto_run {
                                                            should_auto_run = true;
                                                        }
                                                    }
                                                }
                                            }

                                            if should_auto_run {
                                                let state = app_handle.state::<crate::AppState>();
                                                let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
                                                if let Err(e) = crate::mcp::execution_api::execute_mcp_tool_call(
                                                    app_handle.clone(),
                                                    state,
                                                    feature_config_state,
                                                    window.clone(),
                                                    tool_call.id,
                                                )
                                                .await
                                                {
                                                    error!(call_id = tool_call.id, error = %e, "Auto-execute MCP tool failed");
                                                }
                                            } else {
                                                debug!(server = %server_name, tool = %tool_name, "MCP tool auto-run disabled");
                                            }
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Failed to load assistant MCP configs for auto-run");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to create MCP tool call");
                }
            }
        } else {
            debug!("No MCP tool calls detected in message content");
        }
        Ok(updated_content)
    }
    .await;

    // Decrement conversation-level recursion depth
    let depth_state = CONVERSATION_MCP_DEPTH.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
    let mut depth_map = depth_state.lock().await;
    if let Some(depth) = depth_map.get_mut(&conversation_id) {
        if *depth > 0 {
            *depth -= 1;
        }
        if *depth == 0 {
            depth_map.remove(&conversation_id);
        }
    }

    result
}

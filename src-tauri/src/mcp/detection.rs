use crate::api::ai::events::{ConversationEvent, MCPToolCallUpdateEvent};
use crate::api::ai::types::McpOverrideConfig;
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
            server_name: Some(tool_call.server_name.clone()),
            tool_name: Some(tool_call.tool_name.clone()),
            parameters: Some(tool_call.parameters.clone()),
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

fn should_auto_run_tool(
    mcp_override_config: Option<&McpOverrideConfig>,
    server_name: &str,
    tool_name: &str,
    default_auto_run: bool,
) -> bool {
    if let Some(all_auto_run) = mcp_override_config.and_then(|cfg| cfg.all_tool_auto_run) {
        return all_auto_run;
    }
    let Some(tool_overrides) = mcp_override_config.and_then(|cfg| cfg.tool_auto_run.as_ref()) else {
        return default_auto_run;
    };

    let exact_key = format!("{}/{}", server_name, tool_name);
    if let Some(value) = tool_overrides.get(&exact_key) {
        return *value;
    }
    let normalized_key = format!("{}/{}", sanitize_tool_name(server_name), tool_name);
    if let Some(value) = tool_overrides.get(&normalized_key) {
        return *value;
    }

    default_auto_run
}

#[instrument(level = "debug", skip(app_handle, window, content), fields(conversation_id, message_id, content_len = content.len()))]
pub async fn detect_and_process_mcp_calls(
    app_handle: &tauri::AppHandle,
    window: &tauri::Window,
    conversation_id: i64,
    message_id: i64,
    content: &str,
    mcp_override_config: Option<&McpOverrideConfig>,
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
        let mut temp_content = content.to_string();

        // 处理所有匹配的 MCP 调用，支持多工具并发执行
        for cap in mcp_regex.captures_iter(content) {
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
                    // 替换当前内容中的第一个匹配（使用 temp_content 累积更新）
                    temp_content = mcp_regex.replacen(&temp_content, 1, &ui_hint).to_string();
                    updated_content = Some(temp_content.clone());

                    // 尝试根据助手配置自动执行（is_auto_run）
                    if let Ok(conversation_db) = crate::db::conversation_db::ConversationDatabase::new(app_handle) {
                        if let Ok(repository) = conversation_db.conversation_repo() {
                            if let Ok(Some(conversation)) = repository.read(conversation_id) {
                                if let Some(assistant_id) = conversation.assistant_id {
                                    match crate::mcp::collect_mcp_info_for_assistant(
                                        app_handle,
                                        assistant_id,
                                        None,
                                        None,
                                    )
                                    .await
                                    {
                                        Ok(mcp_info) => {
                                            let servers_with_tools = mcp_info.enabled_servers;
                                            let mut should_auto_run = false;
                                            for s in servers_with_tools.iter() {
                                                // 支持精确匹配和清理后名称匹配
                                                let name_matches = s.name == server_name
                                                    || sanitize_tool_name(&s.name) == server_name;
                                                if name_matches && s.is_enabled {
                                                    if let Some(tool) = s.tools.iter().find(|t| t.name == tool_name && t.is_enabled) {
                                                        let auto_run = should_auto_run_tool(
                                                            mcp_override_config,
                                                            &server_name,
                                                            &tool_name,
                                                            tool.is_auto_run,
                                                        );
                                                        if auto_run {
                                                            should_auto_run = true;
                                                        }
                                                    }
                                                }
                                            }

                                            if should_auto_run {
                                                let app_handle_clone = app_handle.clone();
                                                let window_clone = window.clone();
                                                let tool_call_id = tool_call.id;
                                                tauri::async_runtime::spawn_blocking(move || {
                                                    let app_handle_for_state = app_handle_clone.clone();
                                                    tauri::async_runtime::block_on(async move {
                                                        let state =
                                                            app_handle_for_state.state::<crate::AppState>();
                                                        let feature_config_state =
                                                            app_handle_for_state
                                                                .state::<crate::FeatureConfigState>();
                                                        if let Err(e) =
                                                            crate::mcp::execution_api::execute_mcp_tool_call(
                                                                app_handle_clone,
                                                                state,
                                                                feature_config_state,
                                                                window_clone,
                                                                tool_call_id,
                                                                true, // trigger_continuation
                                                            )
                                                            .await
                                                        {
                                                            error!(
                                                                call_id = tool_call_id,
                                                                error = %e,
                                                                "Auto-execute MCP tool failed"
                                                            );
                                                        }
                                                    });
                                                });
                                            } else {
                                                debug!(server = %server_name, tool = %tool_name, "MCP tool auto-run disabled");
                                            }
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Failed to load MCP configs for auto-run");
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
        }

        // 如果没有检测到任何工具调用，输出日志
        if updated_content.is_none() {
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

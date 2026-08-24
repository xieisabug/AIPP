use super::assistant_api::AssistantDetail;
use crate::api::ai::acp::{
    apply_network_proxy_to_env_vars, build_selected_mcp_tools_payload, extract_acp_config,
    refresh_acp_config_signature,
    refresh_acp_selected_mcp_tools_payload, spawn_acp_idle_reaper_once,
    spawn_acp_session_task,
};
use crate::api::ai::chat::{
    extract_assistant_from_message, handle_non_stream_chat as ai_handle_non_stream_chat,
    handle_stream_chat as ai_handle_stream_chat,
};
use crate::api::ai::codex_app_server::{
    emit_codex_failure, extract_codex_app_server_config, refresh_codex_session_signature,
    spawn_codex_session_task, CodexSessionEntry, CODEX_APP_SERVER_API_TYPE,
    CODEX_APPROVAL_POLICIES, CODEX_SANDBOX_MODES,
};
use crate::api::ai::claude_sdk::{claude_model_choices, emit_claude_failure, extract_claude_sdk_config, is_claude_code_provider, prepare_claude_mcp_bridge, refresh_claude_session_signature, spawn_claude_session_task, ClaudeSessionEntry, CLAUDE_SDK_API_TYPE};
use crate::api::assistant_api::CLAUDE_CODE_DEFAULT_MODEL;
use crate::api::ai::config::{
    get_network_proxy_from_config, get_openai_prompt_cache_key_enabled,
    get_openai_responses_stateful_enabled, get_request_timeout_from_config,
    should_use_openai_responses_features, ChatConfig, ConfigBuilder, OpenAiCacheContext,
};
use crate::api::ai::context_manager::{self, budget::ContextBudget, CompactionContext};
use crate::api::ai::conversation::{
    build_chat_request_from_messages, build_message_list_with_metadata_from_db,
    filter_messages_for_parent_group, init_conversation, select_responses_stateful_request,
    BranchSelection, ChatRequestBuildResult, ToolCallStrategy, ToolConfig,
};
use crate::api::ai::events::{
    ActivityFocus, ConversationEvent, ConversationRuntimeState, ConversationShineState,
    MessageAddEvent, MessageUpdateEvent,
};
use crate::api::ai::title::{generate_title, maybe_generate_title_from_conversation_if_needed};
use crate::api::ai::types::{AiRequest, AiResponse, McpOverrideConfig};
use crate::api::assistant_api::{get_assistant, get_assistants, resolve_acp_provider_id};
use crate::api::butler_api::{is_butler_system_assistant_name, mark_butler_task_cancelled};

use crate::api::genai_client;
use crate::db::conversation_db::{AttachmentType, Repository};
use crate::db::conversation_db::{
    ConversationDatabase, Message, MessageAttachment, QueuedConversationMessage,
};
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use crate::feishu::maybe_schedule_butler_feishu_relay_for_aipp_turn;
use crate::mcp::execution_api::cancel_mcp_tool_calls_by_conversation;
use crate::mcp::{collect_mcp_info_for_assistant, format_mcp_prompt, MCPInfoForAssistant};
use crate::plugin::hook_bus::PluginHookBus;
use crate::skills::{
    build_active_skill_attachments, collect_skills_info_for_assistant_with_additions,
    compose_user_message_with_active_skills, format_skills_prompt,
};
use crate::slash::parse_slash_prompt;
use crate::state::activity_state::ConversationActivityManager;
use crate::state::message_token::MessageTokenManager;
use crate::template_engine::build_template_engine;
use crate::utils::window_utils::send_conversation_event_to_chat_windows;
use crate::{AcpSessionState, AppState, ClaudeSessionState, CodexSessionState, FeatureConfigState};
use anyhow::Context;
use genai::chat::Tool;
use std::collections::{HashMap, HashSet};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;
use tracing::{debug, error, info, instrument, warn};

const QUEUE_KIND_NORMAL: &str = "normal";
const QUEUE_KIND_INTERRUPT: &str = "interrupt";

fn parse_agent_model_override(value: &str) -> Option<(&str, i64)> {
    let (model, provider_id) = value.split_once("%%")?;
    let provider_id = provider_id.parse::<i64>().ok().filter(|id| *id > 0)?;
    let model = model.trim();
    (!model.is_empty()).then_some((model, provider_id))
}

/// 计算字符串的简短 hash（用于确保唯一性）
fn short_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    // 取 hash 的前 8 位十六进制
    format!("{:08x}", hasher.finish() as u32)
}

/// 将字符串清理为符合 OpenAI 工具名称规范的格式
/// OpenAI 要求工具名称匹配正则表达式: ^[a-zA-Z0-9_\.-]+$
/// 即只能包含字母、数字、下划线、点号和连字符
///
/// 当清理后的名称为空或太短时，会附加原始字符串的 hash 以确保唯一性
pub fn sanitize_tool_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                // 将不允许的字符替换为下划线
                '_'
            }
        })
        .collect::<String>()
        // 去除连续的下划线
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");

    // 如果清理后的名称为空或太短（少于2个字符），附加 hash 以确保唯一性
    if sanitized.len() < 2 {
        if sanitized.is_empty() {
            format!("h{}", short_hash(name))
        } else {
            format!("{}_{}", sanitized, short_hash(name))
        }
    } else {
        sanitized
    }
}

/// 构建符合 API 规范的工具名称
/// 格式: {server_name}__{tool_name}
///
/// 注意：此函数会对服务器名称和工具名称进行清理，
/// 当原始名称包含大量非法字符（如中文）时，会使用 hash 确保唯一性
pub fn build_tool_name(server_name: &str, tool_name: &str) -> String {
    format!("{}__{}", sanitize_tool_name(server_name), sanitize_tool_name(tool_name))
}

fn enforce_butler_mcp_override(
    assistant_name: &str,
    override_mcp_config: Option<McpOverrideConfig>,
) -> Option<McpOverrideConfig> {
    if is_butler_system_assistant_name(assistant_name) {
        let mut enforced = override_mcp_config.unwrap_or(McpOverrideConfig {
            all_tool_auto_run: None,
            tool_auto_run: None,
            use_native_toolcall: None,
            tool_call_timeout: None,
        });
        enforced.all_tool_auto_run = Some(true);
        enforced.use_native_toolcall = Some(true);
        Some(enforced)
    } else {
        override_mcp_config
    }
}

#[cfg(test)]
mod butler_mcp_override_tests {
    use super::enforce_butler_mcp_override;
    use crate::api::ai::types::McpOverrideConfig;
    use crate::api::butler_api::BUTLER_SYSTEM_ASSISTANT_NAME;

    #[test]
    fn butler_override_enables_native_toolcall_and_auto_run() {
        let enforced = enforce_butler_mcp_override(BUTLER_SYSTEM_ASSISTANT_NAME, None)
            .expect("Butler assistant should always receive an override");

        assert_eq!(enforced.all_tool_auto_run, Some(true));
        assert_eq!(enforced.use_native_toolcall, Some(true));
    }

    #[test]
    fn non_butler_override_is_left_unchanged() {
        let original = McpOverrideConfig {
            all_tool_auto_run: Some(false),
            tool_auto_run: None,
            use_native_toolcall: Some(false),
            tool_call_timeout: Some(15_000),
        };

        let preserved = enforce_butler_mcp_override("普通助手", Some(original.clone()))
            .expect("Existing override should be preserved for non-Butler assistants");

        assert_eq!(preserved.all_tool_auto_run, original.all_tool_auto_run);
        assert_eq!(preserved.use_native_toolcall, original.use_native_toolcall);
        assert_eq!(preserved.tool_call_timeout, original.tool_call_timeout);
    }
}

fn build_prompt_with_attachment_context(prompt: &str, context: &str) -> String {
    if context.trim().is_empty() {
        prompt.to_string()
    } else if prompt.trim().is_empty() {
        context.to_string()
    } else {
        format!("{}\n{}", prompt, context)
    }
}

fn normalize_queue_kind(queue_kind: &str) -> Result<&'static str, AppError> {
    match queue_kind {
        QUEUE_KIND_NORMAL => Ok(QUEUE_KIND_NORMAL),
        QUEUE_KIND_INTERRUPT => Ok(QUEUE_KIND_INTERRUPT),
        other => Err(AppError::UnknownError(format!("不支持的消息队列类型: {}", other))),
    }
}

fn emit_queued_message_event(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    event_type: &str,
    data: serde_json::Value,
) {
    send_conversation_event_to_chat_windows(
        app_handle,
        conversation_id,
        ConversationEvent { r#type: event_type.to_string(), data },
    );
}

fn emit_queued_message_payload(
    app_handle: &tauri::AppHandle,
    event_type: &str,
    queued: &QueuedConversationMessage,
) {
    emit_queued_message_event(
        app_handle,
        queued.conversation_id,
        event_type,
        serde_json::to_value(queued).unwrap_or_else(|_| serde_json::Value::Null),
    );
}

fn emit_queued_message_remove(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    queue_id: i64,
) {
    emit_queued_message_event(
        app_handle,
        conversation_id,
        "queued_message_remove",
        serde_json::json!({
            "id": queue_id,
            "conversation_id": conversation_id,
        }),
    );
}

fn has_active_mcp_calls(app_handle: &tauri::AppHandle, conversation_id: i64) -> bool {
    let Ok(db) = MCPDatabase::new(app_handle) else {
        return false;
    };
    db.get_mcp_tool_calls_by_conversation(conversation_id)
        .map(|calls| {
            calls
                .iter()
                .any(|call| call.status == "pending" || call.status == "executing")
        })
        .unwrap_or(false)
}

fn select_tool_call_strategy(has_available_tools: bool) -> ToolCallStrategy {
    if has_available_tools {
        ToolCallStrategy::NativeWithToolResponsePairing
    } else {
        ToolCallStrategy::NonNative
    }
}

async fn backfill_request_message_list(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    mut message_list: Vec<(String, String, Vec<MessageAttachment>)>,
) -> Result<Vec<(String, String, Vec<MessageAttachment>)>, AppError> {
    crate::mcp::execution_api::backfill_missing_tool_results(
        app_handle,
        conversation_id,
        &mut message_list,
    )
    .await
    .map_err(|error| AppError::DatabaseError(format!("回填工具结果失败: {}", error)))?;
    Ok(message_list)
}

fn collect_openai_responses_instructions(
    messages: &[(String, String, Vec<MessageAttachment>)],
) -> Option<String> {
    let instructions = messages
        .iter()
        .filter(|(message_type, content, _)| {
            message_type == "system" && !content.trim().is_empty()
        })
        .map(|(_, content, _)| content.trim())
        .collect::<Vec<_>>()
        .join("\n\n");
    if instructions.is_empty() {
        None
    } else {
        Some(instructions)
    }
}

fn prepare_openai_responses_request_messages(
    provider_api_type: &str,
    request_mode: &str,
    messages: Vec<(String, String, Vec<MessageAttachment>)>,
    instructions: Option<String>,
) -> (Vec<(String, String, Vec<MessageAttachment>)>, Option<String>) {
    if !should_use_openai_responses_features(provider_api_type, request_mode) {
        return (messages, instructions);
    }

    let instructions =
        instructions.or_else(|| collect_openai_responses_instructions(&messages));
    if instructions.is_none() {
        return (messages, None);
    }

    let messages = messages
        .into_iter()
        .filter(|(message_type, _, _)| message_type != "system")
        .collect();

    (messages, instructions)
}

fn maybe_select_openai_responses_stateful_messages(
    config_feature_map: &HashMap<String, HashMap<String, crate::db::system_db::FeatureConfig>>,
    provider_api_type: &str,
    request_mode: &str,
    conversation_id: i64,
    init_message_list: &[(String, String, Vec<MessageAttachment>)],
    init_message_ids: &[i64],
    all_messages: &[(Message, Option<MessageAttachment>)],
    is_regeneration: bool,
    has_unfinished_tool_call: bool,
    has_native_tools: bool,
) -> (Vec<(String, String, Vec<MessageAttachment>)>, Option<String>, bool, Option<String>) {
    if !should_use_openai_responses_features(provider_api_type, request_mode)
        || !get_openai_responses_stateful_enabled(config_feature_map)
    {
        return (init_message_list.to_vec(), None, false, None);
    }

    if get_openai_prompt_cache_key_enabled(config_feature_map) {
        debug!(
            conversation_id,
            "OpenAI Responses stateful continuation disabled because prompt cache key is enabled; using full history for prompt cache"
        );
        return (init_message_list.to_vec(), None, false, None);
    }

    if has_native_tools {
        debug!(
            conversation_id,
            "OpenAI Responses stateful continuation disabled for native tool request; using full history"
        );
        return (init_message_list.to_vec(), None, true, None);
    }

    match select_responses_stateful_request(
        init_message_list,
        init_message_ids,
        all_messages,
        has_unfinished_tool_call,
        is_regeneration,
    ) {
        Ok(Some(selection)) => {
            debug!(
                conversation_id,
                skipped_prefix_len = selection.skipped_prefix_len,
                incremental_messages = selection.messages.len(),
                previous_response_id = %selection.previous_response_id,
                "using OpenAI Responses stateful continuation"
            );
            let instructions = collect_openai_responses_instructions(init_message_list);
            (selection.messages, Some(selection.previous_response_id), true, instructions)
        }
        Ok(None) => (init_message_list.to_vec(), None, true, None),
        Err(reason) => {
            warn!(
                conversation_id,
                reason,
                "OpenAI Responses stateful continuation disabled for this request; using full history"
            );
            (init_message_list.to_vec(), None, true, None)
        }
    }
}

fn apply_openai_responses_stateful_request_options(
    mut chat_request: genai::chat::ChatRequest,
    previous_response_id: Option<String>,
    store_response: bool,
    instructions: Option<String>,
) -> genai::chat::ChatRequest {
    if let Some(previous_response_id) = previous_response_id {
        chat_request = chat_request.with_previous_response_id(previous_response_id);
    }
    if let Some(instructions) = instructions {
        chat_request = chat_request.with_system(instructions);
    }
    if store_response {
        chat_request = chat_request.with_store(true);
    }
    chat_request
}

fn messages_to_hook_value(
    messages: &[(String, String, Vec<MessageAttachment>)],
) -> Vec<serde_json::Value> {
    messages
        .iter()
        .enumerate()
        .map(|(index, (message_type, content, _))| {
            serde_json::json!({
                "index": index,
                "messageType": message_type,
                "content": content,
            })
        })
        .collect()
}

fn apply_hook_messages(
    messages: Vec<(String, String, Vec<MessageAttachment>)>,
    hook_context: &serde_json::Value,
) -> Vec<(String, String, Vec<MessageAttachment>)> {
    let Some(hook_messages) = hook_context.get("messages").and_then(|value| value.as_array()) else {
        return messages;
    };

    let mut rebuilt_messages = Vec::with_capacity(hook_messages.len());

    for item in hook_messages {
        let original_index = item
            .get("index")
            .or_else(|| item.get("sourceIndex"))
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);
        let original_message = original_index.and_then(|index| messages.get(index));
        let message_type = item
            .get("messageType")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .or_else(|| original_message.map(|message| message.0.clone()));
        let content = item
            .get("content")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .or_else(|| original_message.map(|message| message.1.clone()));

        let (Some(message_type), Some(content)) = (message_type, content) else {
            continue;
        };

        rebuilt_messages.push((
            message_type,
            content,
            original_message.map(|message| message.2.clone()).unwrap_or_default(),
        ));
    }

    if rebuilt_messages.is_empty() {
        messages
    } else {
        rebuilt_messages
    }
}

async fn run_before_model_request_hook(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    assistant_id: i64,
    messages: Vec<(String, String, Vec<MessageAttachment>)>,
) -> anyhow::Result<Vec<(String, String, Vec<MessageAttachment>)>> {
    let context = serde_json::json!({
        "conversationId": conversation_id,
        "assistantId": assistant_id,
        "messageCount": messages.len(),
        "messages": messages_to_hook_value(&messages),
        "metadata": {}
    });
    let result = PluginHookBus::new(app_handle.clone())
        .run_guard_filter("chat.beforeModelRequest", context)
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    Ok(apply_hook_messages(messages, &result.context))
}

async fn emit_chat_context_event(
    app_handle: &tauri::AppHandle,
    hook_name: &'static str,
    conversation_id: i64,
    assistant_id: i64,
    messages: &[(String, String, Vec<MessageAttachment>)],
) {
    let _ = PluginHookBus::new(app_handle.clone())
        .emit_event(
            hook_name,
            serde_json::json!({
                "conversationId": conversation_id,
                "assistantId": assistant_id,
                "messageCount": messages.len(),
                "messages": messages_to_hook_value(messages),
                "metadata": {}
            }),
        )
        .await;
}

#[tauri::command]
#[instrument(skip(app_handle), fields(conversation_id))]
pub async fn list_queued_conversation_messages(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<Vec<QueuedConversationMessage>, AppError> {
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    db.queued_message_repo()?.list_queued_by_conversation(conversation_id).map_err(AppError::from)
}

#[tauri::command]
#[instrument(skip(app_handle, request), fields(conversation_id = %request.conversation_id, assistant_id = request.assistant_id, queue_kind = %queue_kind))]
pub async fn enqueue_conversation_message(
    app_handle: tauri::AppHandle,
    request: AiRequest,
    queue_kind: String,
) -> Result<QueuedConversationMessage, AppError> {
    let queue_kind = normalize_queue_kind(&queue_kind)?;
    if request.prompt.trim().is_empty() {
        return Err(AppError::UnknownError("排队消息不能为空".to_string()));
    }
    let conversation_id = request
        .conversation_id
        .trim()
        .parse::<i64>()
        .map_err(|_| AppError::UnknownError("排队消息必须关联已有对话".to_string()))?;
    if conversation_id <= 0 {
        return Err(AppError::UnknownError("排队消息必须关联已有对话".to_string()));
    }

    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    let conversation = db
        .conversation_repo()?
        .read(conversation_id)?
        .ok_or_else(|| AppError::DatabaseError("对话未找到".to_string()))?;
    let assistant_id = if request.assistant_id > 0 {
        request.assistant_id
    } else {
        conversation
            .assistant_id
            .ok_or_else(|| AppError::DatabaseError("对话未关联助手".to_string()))?
    };
    let mut queued_request = request;
    queued_request.conversation_id = conversation_id.to_string();
    queued_request.assistant_id = assistant_id;
    let request_json = serde_json::to_string(&queued_request)
        .map_err(|error| AppError::UnknownError(format!("序列化排队消息失败: {}", error)))?;
    let queued = db.queued_message_repo()?.enqueue(
        conversation_id,
        queue_kind,
        &request_json,
        &queued_request.prompt,
        assistant_id,
    )?;

    emit_queued_message_payload(&app_handle, "queued_message_add", &queued);
    Ok(queued)
}

#[tauri::command]
#[instrument(skip(app_handle), fields(queue_id))]
pub async fn promote_queued_conversation_message(
    app_handle: tauri::AppHandle,
    queue_id: i64,
) -> Result<QueuedConversationMessage, AppError> {
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    let queued = db
        .queued_message_repo()?
        .promote_to_interrupt(queue_id)?
        .ok_or_else(|| AppError::DatabaseError("排队消息不存在或已经被消费".to_string()))?;
    emit_queued_message_payload(&app_handle, "queued_message_update", &queued);
    Ok(queued)
}

fn dispatch_queued_message(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    queued: QueuedConversationMessage,
) -> Result<(), AppError> {
    emit_queued_message_remove(&app_handle, queued.conversation_id, queued.id);

    let request: AiRequest = serde_json::from_str(&queued.request_json)
        .map_err(|error| AppError::UnknownError(format!("解析排队消息失败: {}", error)))?;

    tokio::spawn(async move {
        let queued_id = queued.id;
        let response = ask_ai(
            app_handle.clone(),
            app_handle.state::<AppState>(),
            app_handle.state::<AcpSessionState>(),
            app_handle.state::<FeatureConfigState>(),
            app_handle.state::<MessageTokenManager>(),
            app_handle.state::<ConversationActivityManager>(),
            window,
            request,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        let db = match ConversationDatabase::new(&app_handle).map_err(AppError::from) {
            Ok(db) => db,
            Err(error) => {
                warn!(queued_id, error = %error, "failed to open conversation db after queued dispatch");
                return;
            }
        };

        match response {
            Ok(_) => {
                match db.queued_message_repo() {
                    Ok(repo) => {
                        if let Err(error) = repo.finish_dispatch(queued_id) {
                            warn!(queued_id, error = %error, "failed to finish queued message dispatch");
                        }
                    }
                    Err(error) => {
                        warn!(queued_id, error = %error, "failed to get queued message repo after dispatch");
                    }
                }
            }
            Err(error) => match db.queued_message_repo() {
                Ok(repo) => match repo.reset_dispatch(queued_id) {
                    Ok(Some(reset)) => {
                        emit_queued_message_payload(&app_handle, "queued_message_add", &reset);
                        warn!(queued_id, error = %error, "queued message dispatch failed and was reset");
                    }
                    Ok(None) => {
                        warn!(queued_id, error = %error, "queued message dispatch failed but queue row was not reset");
                    }
                    Err(reset_error) => {
                        warn!(
                            queued_id,
                            error = %error,
                            reset_error = %reset_error,
                            "queued message dispatch failed and reset also failed"
                        );
                    }
                },
                Err(reset_error) => {
                    warn!(
                        queued_id,
                        error = %error,
                        reset_error = %reset_error,
                        "queued message dispatch failed and queued repo could not be opened"
                    );
                }
            },
        }
    });

    Ok(())
}

pub(crate) async fn try_dispatch_queued_message(
    app_handle: &tauri::AppHandle,
    window: &tauri::Window,
    conversation_id: i64,
    interrupt_only: bool,
) -> bool {
    let result = async {
        let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
        let queued = db
            .queued_message_repo()?
            .take_next_for_dispatch(conversation_id, interrupt_only)?;
        let Some(queued) = queued else {
            return Ok(false);
        };

        dispatch_queued_message(app_handle.clone(), window.clone(), queued)?;
        Ok::<bool, AppError>(true)
    }
    .await;

    match result {
        Ok(dispatched) => dispatched,
        Err(error) => {
            warn!(
                conversation_id,
                interrupt_only,
                error = %error,
                "failed to dispatch queued conversation message"
            );
            false
        }
    }
}

pub(crate) async fn try_dispatch_queued_message_after_completion(
    app_handle: &tauri::AppHandle,
    window: &tauri::Window,
    conversation_id: i64,
) -> bool {
    if has_active_mcp_calls(app_handle, conversation_id) {
        return false;
    }
    if let Some(activity_manager) = app_handle.try_state::<ConversationActivityManager>() {
        let runtime_state = activity_manager.get_runtime_state(conversation_id).await;
        if runtime_state.is_running {
            return false;
        }
    }

    try_dispatch_queued_message(app_handle, window, conversation_id, false).await
}

fn persist_active_skill_attachments(
    app_handle: &tauri::AppHandle,
    attachments: Vec<MessageAttachment>,
) -> Result<Vec<MessageAttachment>, AppError> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }

    let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    let attachment_repo = db.attachment_repo().map_err(AppError::from)?;
    attachments
        .into_iter()
        .map(|attachment| attachment_repo.create(&attachment).map_err(AppError::from))
        .collect()
}

fn get_butler_task_temporary_skill_identifiers(
    app_handle: &tauri::AppHandle,
    conversation_id: &str,
) -> Result<Vec<String>, AppError> {
    let trimmed = conversation_id.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let Ok(task_conversation_id) = trimmed.parse::<i64>() else {
        return Ok(Vec::new());
    };

    let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    let butler_repo = db.butler_repo().map_err(AppError::from)?;
    Ok(butler_repo
        .get_task_definition_by_task_conversation_id(task_conversation_id)
        .map_err(AppError::from)?
        .map(|definition| definition.temporary_skill_identifiers)
        .unwrap_or_default())
}

/// 工具名称映射表，用于在 sanitized 名称和原始名称之间进行转换
/// key: sanitized 工具名称 (如 "h1234abcd__search_web")
/// value: (原始服务器名称, 原始工具名称) (如 ("搜索服务", "网页搜索"))
pub type ToolNameMapping = HashMap<String, (String, String)>;

/// 工具名分割助手（从 sanitized 名称中分割）
pub fn split_tool_name(fn_name: &str) -> (String, String) {
    if let Some((s, t)) = fn_name.split_once("__") {
        (s.to_string(), t.to_string())
    } else {
        (String::from("default"), fn_name.to_string())
    }
}

/// 从 sanitized 工具全名中解析出原始的服务器名和工具名
/// 如果在映射表中找到，返回原始名称；否则返回 sanitized 名称
pub fn resolve_tool_name(sanitized_full_name: &str, mapping: &ToolNameMapping) -> (String, String) {
    if let Some((server, tool)) = mapping.get(sanitized_full_name) {
        (server.clone(), tool.clone())
    } else if !sanitized_full_name.contains("__") {
        let mut matched_tools: Vec<(String, String)> = mapping
            .values()
            .filter(|(_, tool)| {
                tool == sanitized_full_name || sanitize_tool_name(tool) == sanitized_full_name
            })
            .cloned()
            .collect();
        matched_tools.sort();
        matched_tools.dedup();
        if matched_tools.len() == 1 {
            matched_tools.remove(0)
        } else {
            // 回退：从 sanitized 名称中分割
            split_tool_name(sanitized_full_name)
        }
    } else {
        // 回退：从 sanitized 名称中分割
        split_tool_name(sanitized_full_name)
    }
}

fn has_missing_required_parameter_tool_error(messages: &[Message]) -> bool {
    messages.iter().filter(|message| message.message_type == "tool_result").any(|message| {
        let result_text = crate::api::ai::conversation::extract_tool_result(&message.content)
            .unwrap_or_else(|| message.content.clone());
        result_text.contains("Missing required parameter:")
    })
}

fn has_missing_required_parameter_tool_error_in_message_list(
    messages: &[(String, String, Vec<MessageAttachment>)],
) -> bool {
    messages.iter().any(|(message_type, content, _)| {
        if message_type != "tool_result" {
            return false;
        }
        let result_text = crate::api::ai::conversation::extract_tool_result(content)
            .unwrap_or_else(|| content.clone());
        result_text.contains("Missing required parameter:")
    })
}

fn find_existing_tool_result_message(
    messages: &[(Message, Option<MessageAttachment>)],
    tool_call_id: &str,
) -> Option<Message> {
    messages
        .iter()
        .filter(|(message, _)| message.message_type == "tool_result")
        .filter_map(|(message, _)| {
            crate::api::ai::conversation::extract_tool_call_id(&message.content)
                .filter(|existing_tool_call_id| existing_tool_call_id == tool_call_id)
                .map(|_| message.clone())
        })
        .max_by_key(|message| message.id)
}

/// 从 MCP 服务器列表构建 genai 工具列表和名称映射表
/// 返回 (工具列表, 映射表)
pub fn build_tools_with_mapping(
    servers: &[crate::api::assistant_api::MCPServerWithTools],
) -> (Vec<Tool>, ToolNameMapping) {
    let mut tools = Vec::new();
    let mut mapping = HashMap::new();

    for server in servers {
        for tool in &server.tools {
            let sanitized_name = build_tool_name(&server.name, &tool.name);

            // 保存映射关系
            mapping.insert(sanitized_name.clone(), (server.name.clone(), tool.name.clone()));

            let schema = serde_json::from_str::<serde_json::Value>(&tool.parameters)
                .unwrap_or_else(|_| {
                    serde_json::json!({
                        "type": "object",
                        "additionalProperties": true
                    })
                });

            tools.push(
                Tool::new(sanitized_name)
                    .with_description(tool.description.clone())
                    .with_schema(schema),
            );
        }
    }

    (tools, mapping)
}

fn build_tool_config(
    app_handle: &tauri::AppHandle,
    mcp_info: &crate::mcp::MCPInfoForAssistant,
    enable_tools: bool,
    conversation_id: Option<i64>,
) -> Option<ToolConfig> {
    use crate::mcp::builtin_mcp::templates::{
        is_butler_conversation_kind, is_butler_only_agent_tool, is_butler_only_builtin_command,
    };

    if !enable_tools {
        return None;
    }

    // Determine if this is a butler conversation for tool filtering
    let is_butler = conversation_id
        .and_then(|cid| {
            ConversationDatabase::new(app_handle)
                .ok()
                .and_then(|db| db.conversation_repo().ok())
                .and_then(|repo| repo.read(cid).ok().flatten())
                .map(|c| is_butler_conversation_kind(&c.conversation_kind))
        })
        .unwrap_or(false);

    let servers_for_injection = if mcp_info.dynamic_loading_enabled {
        let mut allowed: HashSet<(i64, String)> = HashSet::new();
        if let Some(cid) = conversation_id {
            if let Ok(db) = MCPDatabase::new(app_handle) {
                let _ = db.refresh_conversation_loaded_tool_statuses(cid);
                if let Ok(loaded) = db.get_valid_loaded_tools_for_conversation(cid) {
                    for tool in loaded {
                        allowed.insert((tool.server_id, tool.tool_name));
                    }
                }
            }
        }

        let mut filtered = Vec::new();
        for server in &mcp_info.enabled_servers {
            // Skip entire server if butler-only and not butler
            if !is_butler && server.command.as_deref().map_or(false, is_butler_only_builtin_command)
            {
                continue;
            }
            let mut tools = Vec::new();
            let is_dynamic_builtin = server.command.as_deref() == Some("aipp:dynamic_mcp");
            let is_agent_server = server.command.as_deref() == Some("aipp:agent");
            for tool in &server.tools {
                // Skip butler-only agent tools in non-butler conversations
                if !is_butler && is_agent_server && is_butler_only_agent_tool(&tool.name) {
                    continue;
                }
                let is_agent_loader_tool = is_agent_server
                    && (tool.name == "load_mcp_server" || tool.name == "load_mcp_tool");
                if is_dynamic_builtin
                    || is_agent_loader_tool
                    || allowed.contains(&(server.id, tool.name.clone()))
                {
                    tools.push(tool.clone());
                }
            }
            if !tools.is_empty() {
                let mut server_cloned = server.clone();
                server_cloned.tools = tools;
                filtered.push(server_cloned);
            }
        }
        filtered
    } else {
        // Non-dynamic mode: filter butler-only tools
        let mut filtered = Vec::new();
        for server in &mcp_info.enabled_servers {
            if !is_butler && server.command.as_deref().map_or(false, is_butler_only_builtin_command)
            {
                continue;
            }
            let is_agent_server = server.command.as_deref() == Some("aipp:agent");
            if !is_butler && is_agent_server {
                let tools: Vec<_> = server
                    .tools
                    .iter()
                    .filter(|t| !is_butler_only_agent_tool(&t.name))
                    .cloned()
                    .collect();
                if !tools.is_empty() {
                    let mut server_cloned = server.clone();
                    server_cloned.tools = tools;
                    filtered.push(server_cloned);
                }
            } else {
                filtered.push(server.clone());
            }
        }
        filtered
    };

    let (tools, tool_name_mapping) = build_tools_with_mapping(&servers_for_injection);
    debug!(tools = ?tools, "injected MCP tools");
    Some(ToolConfig { tools, tool_name_mapping })
}

#[tauri::command]
#[instrument(skip(app_handle, state, acp_session_state, feature_config_state, message_token_manager, activity_manager, window, request, override_model_config, override_prompt, override_mcp_config), fields(assistant_id = request.assistant_id, conversation_id = %request.conversation_id, override_model_id = request.override_model_id))]
pub async fn ask_ai(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    acp_session_state: State<'_, AcpSessionState>,
    feature_config_state: State<'_, FeatureConfigState>,
    message_token_manager: State<'_, MessageTokenManager>,
    activity_manager: State<'_, ConversationActivityManager>,
    window: tauri::Window,
    request: AiRequest,
    override_model_config: Option<HashMap<String, serde_json::Value>>,
    override_prompt: Option<String>,
    override_mcp_config: Option<McpOverrideConfig>,
    runtime_user_prompt_prefix: Option<String>,
    relay_origin: Option<String>,
) -> Result<AiResponse, AppError> {
    let codex_session_state = app_handle.state::<CodexSessionState>();
    info!("Ask AI start");
    debug!(
        ?request,
        ?override_model_config,
        ?override_prompt,
        ?override_mcp_config,
        ?runtime_user_prompt_prefix,
        ?relay_origin,
        "ask_ai input parameters"
    );
    let mut runtime_user_prompt_prefix = runtime_user_prompt_prefix;

    let assistants = get_assistants(app_handle.clone())
        .map_err(|e| AppError::UnknownError(format!("Failed to get assistants: {}", e)))?;

    // 处理 @assistant_name 提取和消息清理
    let (actual_assistant_id, cleaned_prompt) =
        extract_assistant_from_message(&assistants, &request.prompt, request.assistant_id).await?;
    let slash_parse_result = parse_slash_prompt(&app_handle, &cleaned_prompt).await?;

    debug!(?actual_assistant_id, ?cleaned_prompt, "assistant extraction result");

    // 创建一个新的请求对象，使用处理后的数据
    let mut processed_request = request.clone();
    processed_request.assistant_id = actual_assistant_id;
    processed_request.prompt = slash_parse_result.runtime_user_prompt.clone();

    let before_send_context = serde_json::json!({
        "conversationId": processed_request.conversation_id.clone(),
        "assistantId": processed_request.assistant_id,
        "prompt": processed_request.prompt.clone(),
        "source": relay_origin.as_deref().unwrap_or("chat_ui"),
        "relayOrigin": relay_origin.clone(),
        "attachments": processed_request.attachment_list.clone().unwrap_or_default(),
        "runtimeUserPromptPrefix": runtime_user_prompt_prefix.clone(),
        "metadata": {}
    });
    let before_send_result = PluginHookBus::new(app_handle.clone())
        .run_guard_filter("chat.beforeSend", before_send_context)
        .await
        .map_err(AppError::UnknownError)?;
    if let Some(assistant_id) =
        before_send_result.context.get("assistantId").and_then(|value| value.as_i64())
    {
        processed_request.assistant_id = assistant_id;
    }
    if let Some(prompt) = before_send_result.context.get("prompt").and_then(|value| value.as_str()) {
        processed_request.prompt = prompt.to_string();
    }
    if let Some(prefix) = before_send_result
        .context
        .get("runtimeUserPromptPrefix")
        .and_then(|value| value.as_str())
    {
        runtime_user_prompt_prefix = Some(prefix.to_string());
    }
    if let Some(attachments) =
        before_send_result.context.get("attachments").and_then(|value| value.as_array())
    {
        let attachment_ids = attachments.iter().filter_map(|value| value.as_i64()).collect();
        processed_request.attachment_list = Some(attachment_ids);
    }

    let template_engine = build_template_engine(&app_handle)
        .map_err(|e| AppError::UnknownError(format!("Failed to build template engine: {}", e)))?;
    let mut template_context = HashMap::new();

    let selected_text = state.inner().selected_text.lock().await.clone();
    template_context.insert("selected_text".to_string(), selected_text);
    if !processed_request.conversation_id.trim().is_empty() {
        template_context.insert(
            "conversation_id".to_string(),
            processed_request.conversation_id.trim().to_string(),
        );
    }

    let app_handle_clone = app_handle.clone();
    let assistant_detail = get_assistant(app_handle_clone, processed_request.assistant_id).unwrap();
    let assistant_prompt_origin = &assistant_detail.prompts[0].prompt;
    let assistant_prompt_result =
        template_engine.parse(&assistant_prompt_origin, &template_context).await;
    debug!(
        assistant_prompt_result = assistant_prompt_result.as_str(),
        "assistant prompt after template"
    );

    if assistant_detail.model.is_empty() {
        return Err(AppError::NoModelFound);
    }

    let override_mcp_config =
        enforce_butler_mcp_override(&assistant_detail.assistant.name, override_mcp_config);

    // 收集 MCP 信息
    let mcp_info = collect_mcp_info_for_assistant(
        &app_handle,
        processed_request.assistant_id,
        override_mcp_config.as_ref(),
        None,
    )
    .await?;

    // Filter butler-only tools for non-butler conversations
    let mcp_info = {
        use crate::mcp::builtin_mcp::templates::{
            is_butler_conversation_kind, is_butler_only_agent_tool, is_butler_only_builtin_command,
        };
        let is_butler = processed_request
            .conversation_id
            .parse::<i64>()
            .ok()
            .and_then(|cid| {
                ConversationDatabase::new(&app_handle)
                    .ok()
                    .and_then(|db| db.conversation_repo().ok())
                    .and_then(|repo| repo.read(cid).ok().flatten())
                    .map(|c| is_butler_conversation_kind(&c.conversation_kind))
            })
            .unwrap_or(false);
        if is_butler {
            mcp_info
        } else {
            let filtered_servers = mcp_info
                .enabled_servers
                .into_iter()
                .filter_map(|mut server| {
                    if server.command.as_deref().map_or(false, is_butler_only_builtin_command) {
                        return None;
                    }
                    if server.command.as_deref() == Some("aipp:agent") {
                        server.tools.retain(|t| !is_butler_only_agent_tool(&t.name));
                    }
                    if server.tools.is_empty() {
                        None
                    } else {
                        Some(server)
                    }
                })
                .collect();
            MCPInfoForAssistant { enabled_servers: filtered_servers, ..mcp_info }
        }
    };

    info!(
        enabled_servers = mcp_info.enabled_servers.len(),
        native_toolcall = mcp_info.use_native_toolcall,
        "MCP configuration"
    );
    let is_native_toolcall = mcp_info.use_native_toolcall;

    // 动态加载模式：即使原生 toolcall 也需要注入 MCP 动态加载规范
    // 非动态加载模式：仅非原生时拼接 XML 约束
    let assistant_prompt_result = if mcp_info.enabled_servers.len() > 0 {
        if mcp_info.dynamic_loading_enabled {
            // 动态加载模式：总是注入 prompt（根据是否原生 toolcall 提供不同内容）
            let prompt = format_mcp_prompt(assistant_prompt_result, &mcp_info).await;
            debug!(formatted_prompt = prompt.as_str(), "MCP formatted prompt (dynamic loading)");
            prompt
        } else if !mcp_info.use_native_toolcall {
            // 非动态加载模式 + 非原生 toolcall：注入 XML 约束
            let prompt = format_mcp_prompt(assistant_prompt_result, &mcp_info).await;
            debug!(formatted_prompt = prompt.as_str(), "MCP formatted prompt");
            prompt
        } else {
            // 非动态加载模式 + 原生 toolcall：不注入 XML 约束
            assistant_prompt_result
        }
    } else {
        assistant_prompt_result
    };

    // Collect and format Skills prompt
    let temporary_skill_identifiers = get_butler_task_temporary_skill_identifiers(
        &app_handle,
        &processed_request.conversation_id,
    )?;
    let skills_info = collect_skills_info_for_assistant_with_additions(
        &app_handle,
        processed_request.assistant_id,
        &temporary_skill_identifiers,
    )
    .await?;
    let assistant_prompt_result = if !skills_info.enabled_skills.is_empty() {
        let prompt = format_skills_prompt(&app_handle, assistant_prompt_result, &skills_info).await;
        info!(enabled_skills = skills_info.enabled_skills.len(), "Skills formatted into prompt");
        debug!(formatted_prompt = prompt.as_str(), "Skills formatted prompt");
        prompt
    } else {
        assistant_prompt_result
    };

    let _need_generate_title = processed_request.conversation_id.is_empty();
    let request_prompt_result = compose_user_message_with_active_skills(
        &template_engine.parse(&slash_parse_result.runtime_user_prompt, &template_context).await,
        &slash_parse_result.active_skills,
    );
    let runtime_prompt_result = runtime_user_prompt_prefix
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|prefix| format!("{}\n\n{}", prefix, request_prompt_result))
        .unwrap_or_else(|| request_prompt_result.clone());
    let display_prompt_result = compose_user_message_with_active_skills(
        &template_engine.parse(&slash_parse_result.display_prompt, &template_context).await,
        &slash_parse_result.active_skills,
    );
    let active_skill_attachments = persist_active_skill_attachments(
        &app_handle,
        build_active_skill_attachments(&slash_parse_result.active_skills)?,
    )?;

    let app_handle_clone = app_handle.clone();
    let (
        conversation_id,
        _new_message_id,
        user_message_id,
        request_prompt_result_with_context,
        init_message_list,
        init_message_ids,
        init_db_token_counts,
    ) = initialize_conversation(
        &app_handle_clone,
        &processed_request,
        &assistant_detail,
        assistant_prompt_result,
        display_prompt_result,
        runtime_prompt_result.clone(),
        override_prompt.clone(),
        active_skill_attachments,
    )
    .await?;
    let initial_message_ids_for_after_response = init_message_ids.clone();

    let _ = PluginHookBus::new(app_handle.clone())
        .emit_event(
            "chat.afterUserMessageCreated",
            serde_json::json!({
                "conversationId": conversation_id,
                "messageId": user_message_id,
                "assistantId": processed_request.assistant_id,
                "prompt": request_prompt_result_with_context.clone(),
                "metadata": {}
            }),
        )
        .await;

    // 设置用户消息的活动状态（闪亮边框）
    activity_manager.set_user_pending(&app_handle, conversation_id, user_message_id).await;

    if let Err(error) = maybe_schedule_butler_feishu_relay_for_aipp_turn(
        &app_handle,
        conversation_id,
        user_message_id.saturating_sub(1),
        relay_origin.as_deref(),
    )
    .await
    {
        warn!(
            conversation_id,
            user_message_id,
            error = %error,
            "failed to schedule butler feishu relay scope"
        );
    }

    message_token_manager.reset_cancel_token(conversation_id).await;

    // 总是启动流式处理，即使没有预先创建消息
    let _config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    let _request_prompt_result_with_context_clone = request_prompt_result_with_context.clone();

    let app_handle_clone = app_handle.clone();
    let window_clone = window.clone(); // 提前克隆，供 ACP 分支使用

    // 检查是否是 ACP 助手类型（assistant_type === 4）
    // 这个检查必须在获取 model_detail 之前，因为 ACP 助手可能没有有效的模型配置
    if assistant_detail.assistant.assistant_type == Some(4) {
        info!("ACP assistant detected (type=4), routing to ACP session");

        // 获取 provider 配置
        // ACP 配置可能在 llm_provider_config 表中（如 acp_cli_command）
        let agent_provider_id = resolve_acp_provider_id(&assistant_detail.model, &assistant_detail.model_configs);
        let override_provider_id = processed_request
            .override_model_id
            .as_deref()
            .and_then(parse_agent_model_override)
            .map(|(_, provider_id)| provider_id);
        let routing_provider_id = override_provider_id.or(agent_provider_id);
        if let Some(provider_id) = override_provider_id {
            info!(
                default_provider_id = ?agent_provider_id,
                override_provider_id = provider_id,
                "Agent model override selects provider for routing"
            );
        }
        let (provider_api_type, provider_configs) = if let Some(provider_id) = routing_provider_id {
            debug!("ACP: Getting provider config for provider_id={}", provider_id);

            let llm_db = LLMDatabase::new(&app_handle).map_err(|e| {
                AppError::UnknownError(format!("Failed to open LLM database: {}", e))
            })?;

            let provider = llm_db.get_llm_provider(provider_id).map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::UnknownError(format!(
                    "Agent 助手 {} 引用的提供商 ID {provider_id} 不存在，请在助手配置中重新选择提供商",
                    assistant_detail.assistant.id
                )),
                other => AppError::from(other),
            })?;
            let configs = llm_db
                .get_llm_provider_config(provider_id)
                .map_err(AppError::from)?;
            (provider.api_type, configs)
        } else {
            return Err(AppError::UnknownError(
                "Agent 助手尚未配置模型提供商".to_string(),
            ));
        };

        let provider_api_type = if is_claude_code_provider(&provider_api_type, &provider_configs) {
            CLAUDE_SDK_API_TYPE.to_string()
        } else {
            provider_api_type
        };
        if provider_api_type == CLAUDE_SDK_API_TYPE {
            info!("Agent provider routes through Claude Code stream-json");
        }

        debug!(
            provider_api_type = %provider_api_type,
            config_names = ?provider_configs.iter().map(|config| config.name.as_str()).collect::<Vec<_>>(),
            "Agent provider configuration loaded"
        );

        if provider_api_type == CODEX_APP_SERVER_API_TYPE {
            info!(conversation_id, "Codex app-server provider detected");
            let model_code = assistant_detail.model.first().map(|model| model.model_code.clone()).filter(|value| !value.trim().is_empty());
            let mut codex_config = extract_codex_app_server_config(
                &assistant_detail.model_configs,
                &provider_configs,
                model_code,
            )?;
            if let Some((model, _)) = processed_request.override_model_id.as_deref().and_then(parse_agent_model_override) {
                codex_config.model = Some(model.to_string());
            }
            if let Some(config) = override_model_config.as_ref() {
                if let Some(effort) = config.get("reasoning_effort").and_then(|value| value.as_str()) { codex_config.reasoning_effort = Some(effort.to_string()); }
                // 新对话界面传入的会话级审批策略/沙箱覆盖，非法值直接报错（不做回退）
                if let Some(approval) = config.get("approval_policy").and_then(|value| value.as_str()).filter(|value| !value.trim().is_empty()) {
                    if !CODEX_APPROVAL_POLICIES.contains(&approval) { return Err(AppError::UnknownError(format!("Codex 不支持审批策略：{approval}"))); }
                    codex_config.approval_policy = Some(approval.to_string());
                }
                if let Some(sandbox) = config.get("sandbox").and_then(|value| value.as_str()).filter(|value| !value.trim().is_empty()) {
                    if !CODEX_SANDBOX_MODES.contains(&sandbox) { return Err(AppError::UnknownError(format!("Codex 不支持沙箱模式：{sandbox}"))); }
                    codex_config.sandbox = Some(sandbox.to_string());
                }
            }
            // 与 ACP 通道一致：挂载助手绑定的 MCP 工具（桥接注入），payload 计入签名
            codex_config.selected_mcp_tools_payload =
                build_selected_mcp_tools_payload(&app_handle, assistant_detail.assistant.id)
                    .map_err(AppError::UnknownError)?;
            refresh_codex_session_signature(&mut codex_config);
            let response_message = add_message(
                &app_handle,
                None,
                conversation_id,
                "response".to_string(),
                String::new(),
                Some(0),
                Some("codex-app-server".to_string()),
                Some(chrono::Utc::now()),
                None,
                0,
                None,
                None,
            )?;
            let add_event = ConversationEvent {
                r#type: "message_add".to_string(),
                data: serde_json::to_value(MessageAddEvent {
                    message_id: response_message.id,
                    message_type: "response".to_string(),
                })
                .unwrap(),
            };
            let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), add_event);
            let handle = {
                let mut sessions = codex_session_state.sessions.lock().await;
                if sessions.get(&conversation_id).is_some_and(|entry| entry.config_signature == codex_config.session_signature) {
                    sessions.get(&conversation_id).unwrap().handle.clone()
                } else {
                    let handle = spawn_codex_session_task(app_handle.clone(), conversation_id, codex_config.clone());
                    if let Some(previous) = sessions.get(&conversation_id) {
                        let reason = format!(
                            "ask_ai 因用户本次请求的 Codex 配置发生变化而替换现有会话（old_run_id={}, new_run_id={}）",
                            previous.run_id,
                            handle.run_id()
                        );
                        warn!(conversation_id, reason, "Replacing Codex session from ask_ai");
                        previous.handle.shutdown(reason);
                    }
                    sessions.insert(conversation_id, CodexSessionEntry::new(handle.clone(), conversation_id, codex_config.session_signature.clone()));
                    handle
                }
            };
            if let Err(error) = handle.send_prompt(
                response_message.id,
                runtime_prompt_result.clone(),
                window_clone.clone(),
            ) {
                emit_codex_failure(
                    &app_handle,
                    conversation_id,
                    response_message.id,
                    &window_clone,
                    &error.to_string(),
                );
                activity_manager.clear_focus(&app_handle, conversation_id).await;
                return Err(error);
            }
            return Ok(AiResponse { conversation_id, request_prompt_result_with_context: processed_request.prompt });
        }

        if provider_api_type == CLAUDE_SDK_API_TYPE || provider_api_type == "anthropic" {
            info!(conversation_id, "Entering Claude Code stream-json provider branch");
            let llm_db = LLMDatabase::new(&app_handle).map_err(AppError::from)?;
            let model_code = assistant_detail.model.first().map(|model| model.model_code.clone()).filter(|value| !value.trim().is_empty());
            let default_provider_id = agent_provider_id.ok_or_else(|| {
                AppError::UnknownError("Claude Code 助手没有配置模型提供商".to_string())
            })?;
            let selected_provider_id = processed_request.override_model_id.as_deref()
                .and_then(parse_agent_model_override)
                .map(|(_, provider_id)| provider_id)
                .unwrap_or(default_provider_id);
            let selected_provider_configs = llm_db.get_llm_provider_config(selected_provider_id).map_err(AppError::from)?;
            let selected_provider = llm_db.get_llm_provider(selected_provider_id).map_err(AppError::from)?;
            if !is_claude_code_provider(&selected_provider.api_type, &selected_provider_configs) {
                return Err(AppError::UnknownError("Claude Code 只能使用 Claude Code 默认配置或 Anthropic Provider".into()));
            }
            debug!(
                provider_id = selected_provider_id,
                config_names = ?selected_provider_configs.iter().map(|config| config.name.as_str()).collect::<Vec<_>>(),
                "Loaded Claude Code provider configs"
            );
            let model_choices = claude_model_choices(
                &llm_db.get_llm_models(selected_provider_id.to_string()).map_err(|e| AppError::UnknownError(e.to_string()))?,
            );
            let selected_model = processed_request.override_model_id.as_deref()
                .and_then(parse_agent_model_override)
                .map(|(model, _)| model.to_string())
                .or(model_code);
            let selected_model = selected_model.filter(|model| model != CLAUDE_CODE_DEFAULT_MODEL);
            let mut config = extract_claude_sdk_config(&assistant_detail.model_configs, &selected_provider_configs, selected_model, model_choices)?;
            if let Some((model, _)) = processed_request.override_model_id.as_deref().and_then(parse_agent_model_override) {
                config.model = (model != CLAUDE_CODE_DEFAULT_MODEL).then(|| model.to_string());
            }
            if let Some(overrides) = override_model_config.as_ref() {
                if let Some(effort) = overrides.get("claude_effort").and_then(|value| value.as_str()) { config.effort = (effort != "default").then_some(effort.to_string()); }
            }
            config.selected_mcp_tools_payload =
                build_selected_mcp_tools_payload(&app_handle, assistant_detail.assistant.id)
                    .map_err(AppError::UnknownError)?;
            refresh_claude_session_signature(&mut config);
            prepare_claude_mcp_bridge(&app_handle, conversation_id, &mut config)
                .await
                .map_err(|error| AppError::UnknownError(format!("Claude Code MCP bridge 初始化失败：{error}")))?;
            info!(
                conversation_id,
                provider_id = selected_provider_id,
                model = ?config.model,
                cli = %config.cli_command,
                "Claude Code provider selected; starting stream-json session"
            );
            let response_message = add_message(&app_handle, None, conversation_id, "response".to_string(), String::new(), Some(0), Some("claude-code".to_string()), Some(chrono::Utc::now()), None, 0, None, None)?;
            let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), ConversationEvent { r#type: "message_add".to_string(), data: serde_json::to_value(MessageAddEvent { message_id: response_message.id, message_type: "response".to_string() }).unwrap() });
            let claude_state = app_handle.state::<ClaudeSessionState>();
            let handle = { let mut sessions = claude_state.sessions.lock().await; if let Some(entry)=sessions.get(&conversation_id).filter(|entry| entry.config_signature == config.session_signature) { entry.handle.clone() } else { let handle=spawn_claude_session_task(app_handle.clone(), conversation_id, config.clone()); sessions.insert(conversation_id, ClaudeSessionEntry::new(handle.clone(), conversation_id, config.session_signature.clone(), config.model.clone(), config.permission_mode.clone(), config.effort.clone())); handle } };
            if let Err(error) = handle.send_prompt(
                response_message.id,
                runtime_prompt_result.clone(),
                window_clone.clone(),
            ) {
                emit_claude_failure(
                    &app_handle,
                    conversation_id,
                    response_message.id,
                    &window_clone,
                    &error.to_string(),
                );
                activity_manager.clear_focus(&app_handle, conversation_id).await;
                return Err(error);
            }
            return Ok(AiResponse { conversation_id, request_prompt_result_with_context: processed_request.prompt });
        }

        if provider_api_type != "acp" {
            return Err(AppError::UnknownError(format!("不支持的 Agent provider api_type: {provider_api_type}")));
        }

        // 从 assistant_model_configs 和 llm_provider_configs 提取 ACP 配置
        let proxy_enabled = provider_configs
            .iter()
            .find(|config| config.name == "proxy_enabled")
            .and_then(|config| config.value.parse::<bool>().ok())
            .unwrap_or(false);
        let network_proxy =
            proxy_enabled.then(|| get_network_proxy_from_config(&_config_feature_map)).flatten();
        let mut acp_config =
            extract_acp_config(&assistant_detail.model_configs, &provider_configs)?;
        if let Some(proxy_url) = network_proxy.as_deref() {
            let injected = apply_network_proxy_to_env_vars(&mut acp_config.env_vars, proxy_url);
            info!(
                proxy_url = %proxy_url,
                injected,
                conversation_id,
                "ACP proxy env vars applied"
            );
        }
        refresh_acp_selected_mcp_tools_payload(
            &app_handle,
            assistant_detail.assistant.id,
            &mut acp_config,
        )
        .map_err(AppError::UnknownError)?;
        refresh_acp_config_signature(&mut acp_config);
        info!(
            "ACP config: cli_command={}, working_directory={}, env_vars={}, additional_args={}",
            acp_config.cli_command,
            acp_config.working_directory.display(),
            acp_config.env_vars.len(),
            acp_config.additional_args.len()
        );

        // 创建初始响应消息（ACP 不需要真实的 model_id，使用占位值）
        let response_message = add_message(
            &app_handle,
            None,
            conversation_id,
            "response".to_string(),
            String::new(),           // 初始为空，通过流式更新
            Some(0),                 // ACP 使用占位 model_id = 0
            Some("acp".to_string()), // ACP 使用占位 model_code
            Some(chrono::Utc::now()),
            None,
            0,
            None,
            None,
        )?;

        // 发送消息添加事件
        let add_event = ConversationEvent {
            r#type: "message_add".to_string(),
            data: serde_json::to_value(MessageAddEvent {
                message_id: response_message.id,
                message_type: "response".to_string(),
            })
            .unwrap(),
        };
        let _ = window.emit(format!("conversation_event_{}", conversation_id).as_str(), add_event);

        let prompt_clone = runtime_prompt_result.clone();
        let acp_attachments = init_message_list
            .iter()
            .rev()
            .find(|(message_type, _, _)| message_type == "user")
            .map(|(_, _, attachments)| attachments.clone())
            .unwrap_or_default();

        spawn_acp_idle_reaper_once(app_handle_clone.clone());

        let session_handle = {
            let mut sessions = acp_session_state.sessions.lock().await;
            if sessions
                .get(&conversation_id)
                .is_some_and(|entry| entry.config_signature == acp_config.session_signature)
            {
                let entry = sessions.get_mut(&conversation_id).expect("checked above");
                entry.touch();
                entry.handle.clone()
            } else {
                if sessions.contains_key(&conversation_id) {
                    info!(
                        conversation_id,
                        "ACP session config changed; replacing existing session"
                    );
                }
                let handle = spawn_acp_session_task(
                    app_handle_clone.clone(),
                    conversation_id,
                    acp_config.clone(),
                );
                sessions.insert(
                    conversation_id,
                    crate::api::ai::acp::AcpSessionEntry::new(
                        handle.clone(),
                        conversation_id,
                        acp_config.session_signature.clone(),
                    ),
                );
                handle
            }
        };

        if let Err(e) = session_handle.send_prompt(
            response_message.id,
            prompt_clone.clone(),
            acp_attachments.clone(),
            window_clone.clone(),
        ) {
            warn!(conversation_id, error = %e, "ACP session send prompt failed, respawning session");
            let replacement_handle = {
                let mut sessions = acp_session_state.sessions.lock().await;
                let handle = spawn_acp_session_task(
                    app_handle_clone.clone(),
                    conversation_id,
                    acp_config.clone(),
                );
                sessions.insert(
                    conversation_id,
                    crate::api::ai::acp::AcpSessionEntry::new(
                        handle.clone(),
                        conversation_id,
                        acp_config.session_signature.clone(),
                    ),
                );
                handle
            };
            replacement_handle
                .send_prompt(response_message.id, prompt_clone, acp_attachments, window_clone)
                .map_err(|error| {
                    error!(conversation_id, error = %error, "ACP session resend prompt failed");
                    error
                })?;
        }

        return Ok(AiResponse {
            conversation_id,
            request_prompt_result_with_context: processed_request.prompt,
        });
    }

    // 非 ACP 助手，继续原有流程
    // 在异步任务外获取模型详情（避免线程安全问题）
    let llm_db = LLMDatabase::new(&app_handle).map_err(AppError::from)?;

    // 检查是否需要覆盖模型
    let model_detail = if let Some(override_model_id) = &processed_request.override_model_id {
        info!(override_model_id, "using override model id");
        let parts: Vec<&str> = override_model_id.split("%%").collect();
        if parts.len() != 2 {
            return Err(AppError::UnknownError("Invalid override model ID format".to_string()));
        }
        let (model_code, provider_id) = (parts[0], parts[1]);
        let provider_id_i64 = provider_id
            .parse::<i64>()
            .map_err(|e| AppError::UnknownError(format!("Invalid provider_id: {}", e)))?;
        let model_code_string = model_code.to_string();
        llm_db
            .get_llm_model_detail(&provider_id_i64, &model_code_string)
            .context("Failed to get LLM model detail")?
    } else {
        // 使用助手的默认模型
        let provider_id = &assistant_detail.model[0].provider_id;
        let model_code = &assistant_detail.model[0].model_code;
        llm_db
            .get_llm_model_detail(provider_id, model_code)
            .context("Failed to get LLM model detail")?
    };

    // 重新克隆 window，因为前面的 ACP 分支可能已经消费了
    let window_clone = window.clone(); // 在移动之前克隆
    let model_id = model_detail.model.id; // 提前获取模型ID
    let provider_id = model_detail.provider.id;
    let model_code = model_detail.model.code.clone(); // 提前获取模型代码
    let model_request_mode = model_detail.model.request_mode.clone(); // 提前获取模型请求模式
    let model_configs = model_detail.configs.clone(); // 提前获取模型配置
    let provider_api_type = model_detail.provider.api_type.clone(); // 提前获取API类型
    let assistant_model_configs = assistant_detail.model_configs.clone(); // 提前获取助手模型配置

    info!(
        "ask_ai: provider_api_type={}, conversation_id={}, assistant_id={}",
        provider_api_type, conversation_id, request.assistant_id
    );

    let task_handle = tokio::spawn(async move {
        // 直接创建数据库连接（避免线程安全问题）
        let conversation_db = ConversationDatabase::new(&app_handle_clone).unwrap();

        // 构建聊天配置
        // 从配置中获取网络代理和超时设置
        let network_proxy = get_network_proxy_from_config(&_config_feature_map);
        let request_timeout = get_request_timeout_from_config(&_config_feature_map);

        // 检查供应商是否启用了代理
        let proxy_enabled = model_configs
            .iter()
            .find(|config| config.name == "proxy_enabled")
            .and_then(|config| config.value.parse::<bool>().ok())
            .unwrap_or(false);

        // 创建一个临时的 ModelDetail 用于配置合并
        let temp_model_detail = crate::db::llm_db::ModelDetail {
            model: crate::db::llm_db::LLMModel {
                id: model_id,
                name: model_code.clone(),
                code: model_code.clone(),
                llm_provider_id: 0,         // 临时值
                description: String::new(), // 临时值
                vision_support: false,      // 临时值
                audio_support: false,       // 临时值
                video_support: false,       // 临时值
                request_mode: model_request_mode.clone(),
            },
            provider: crate::db::llm_db::LLMProvider {
                id: 0,               // 临时值
                name: String::new(), // 临时值
                api_type: provider_api_type.clone(),
                description: String::new(), // 临时值
                is_official: false,         // 临时值
                is_enabled: true,           // 临时值
            },
            configs: model_configs.clone(),
        };

        let model_config_clone = ConfigBuilder::merge_model_configs(
            assistant_model_configs,
            &temp_model_detail,
            override_model_config,
        );

        let config_map = model_config_clone
            .iter()
            .filter_map(|config| {
                config.value.as_ref().map(|value| (config.name.clone(), value.clone()))
            })
            .collect::<HashMap<String, String>>();

        let stream = config_map.get("stream").and_then(|v| v.parse().ok()).unwrap_or(false);

        let model_name = config_map.get("model").cloned().unwrap_or_else(|| model_code.clone());

        let openai_cache_context = OpenAiCacheContext {
            provider_id,
            provider_api_type: provider_api_type.clone(),
            model_code: model_code.clone(),
            request_mode: model_request_mode.clone(),
            assistant_id: processed_request.assistant_id,
            conversation_id,
        };
        let chat_options = ConfigBuilder::build_chat_options(
            &config_map,
            Some(&_config_feature_map),
            Some(&openai_cache_context),
        );
        let force_non_native_for_invalid_tool_args =
            has_missing_required_parameter_tool_error_in_message_list(&init_message_list);
        if force_non_native_for_invalid_tool_args {
            warn!(
                conversation_id,
                "detected missing required parameter tool error in history; forcing non-native ask_ai"
            );
        }

        let client = genai_client::create_client_with_config(
            &crate::api::copilot_token_manager::prepare_provider_configs(
                &app_handle_clone,
                &provider_api_type,
                &model_configs,
                network_proxy.as_deref(),
            )
            .await
            .map_err(|e| AppError::ProviderError(e))?,
            &model_code,
            &provider_api_type,
            Some(&model_request_mode),
            network_proxy.as_deref(),
            proxy_enabled,
            Some(request_timeout),
            stream,
            &_config_feature_map,
        )?;

        // 动态判断是否有可用的工具
        let has_available_tools = is_native_toolcall
            && !mcp_info.enabled_servers.is_empty()
            && !force_non_native_for_invalid_tool_args;

        // 某些 OpenAI 兼容通道在使用 Gemini 模型时不会返回 usage（或返回 null），
        // 而 genai 的 OpenAI 适配器会尝试严格反序列化 usage，从而在日志中出现错误。
        // 为避免该无害错误噪音，这里对「provider_api_type=openai 且 model_code 含 gemini」的组合禁用 usage 捕获。
        let provider_api_type_lc = provider_api_type.to_lowercase();
        let model_code_lc = model_code.to_lowercase();
        let is_openai_like =
            provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
        let is_gemini = model_code_lc.contains("gemini");
        let capture_usage = !(is_openai_like && is_gemini);
        let capture_content = stream && model_request_mode.eq_ignore_ascii_case("responses");

        let capture_reasoning_content = stream
            && is_openai_like
            && has_available_tools
            && config_map.contains_key("reasoning_effort");

        let chat_config = ChatConfig {
            model_name,
            stream,
            chat_options: chat_options
                .with_normalize_reasoning_content(true)
                .with_capture_content(capture_content)
                .with_capture_reasoning_content(capture_reasoning_content)
                .with_capture_usage(capture_usage)
                .with_capture_tool_calls(has_available_tools), // 动态设置
            client,
        };

        info!(
            model = chat_config.model_name,
            stream = chat_config.stream,
            has_tools = has_available_tools,
            provider_api_type = %provider_api_type,
            capture_content = capture_content,
            capture_usage = capture_usage,
            capture_reasoning_content = capture_reasoning_content,
            is_openai_like = is_openai_like,
            is_gemini = is_gemini,
            force_non_native_for_invalid_tool_args,
            "chat configuration established"
        );

        let tool_call_strategy = select_tool_call_strategy(has_available_tools);
        let tool_config = build_tool_config(
            &app_handle_clone,
            &mcp_info,
            has_available_tools,
            Some(conversation_id),
        );

        let all_messages_for_stateful = match conversation_db.message_repo() {
            Ok(repo) => repo.list_by_conversation_id(conversation_id).unwrap_or_else(|error| {
                warn!(
                    conversation_id,
                    error = %error,
                    "failed to read messages for OpenAI Responses stateful selector"
                );
                Vec::new()
            }),
            Err(error) => {
                warn!(
                    conversation_id,
                    error = %error,
                    "failed to open message repo for OpenAI Responses stateful selector"
                );
                Vec::new()
            }
        };
        let has_unfinished_tool_call = has_active_mcp_calls(&app_handle_clone, conversation_id);
        let init_message_ids_for_stateful = init_message_ids.clone();

        // Context budget management with LLM compaction
        let budget = ContextBudget::from_config(&_config_feature_map);
        let is_butler = is_butler_system_assistant_name(&assistant_detail.assistant.name);
        let compaction_ctx = CompactionContext {
            client: &chat_config.client,
            model_name: &chat_config.model_name,
            conversation_id,
            conversation_db: &conversation_db,
            is_butler,
            message_ids: init_message_ids,
        };
        emit_chat_context_event(
            &app_handle_clone,
            "chat.beforeBuildContext",
            conversation_id,
            processed_request.assistant_id,
            &init_message_list,
        )
        .await;
        let fit_result = context_manager::fit_to_budget_with_compaction(
            init_message_list,
            &budget,
            &init_db_token_counts,
            compaction_ctx,
        )
        .await;
        emit_chat_context_event(
            &app_handle_clone,
            "chat.afterBuildContext",
            conversation_id,
            processed_request.assistant_id,
            &fit_result.messages,
        )
        .await;
        let init_message_list = run_before_model_request_hook(
            &app_handle_clone,
            conversation_id,
            processed_request.assistant_id,
            fit_result.messages,
        )
        .await?;
        if fit_result.estimated_tokens > 0 {
            debug!(
                conversation_id,
                estimated_tokens = fit_result.estimated_tokens,
                compacted = fit_result.compacted,
                "context budget fit result for ask_ai"
            );
        }

        let (request_message_list, previous_response_id, store_response, instructions) =
            maybe_select_openai_responses_stateful_messages(
                &_config_feature_map,
                &provider_api_type,
                &model_request_mode,
                conversation_id,
                &init_message_list,
                &init_message_ids_for_stateful,
                &all_messages_for_stateful,
                false,
                has_unfinished_tool_call,
                has_available_tools,
            );
        let (request_message_list, instructions) = prepare_openai_responses_request_messages(
            &provider_api_type,
            &model_request_mode,
            request_message_list,
            instructions,
        );
        let request_message_list = backfill_request_message_list(
            &app_handle_clone,
            conversation_id,
            request_message_list,
        )
        .await?;

        let ChatRequestBuildResult { chat_request, tool_name_mapping } =
            build_chat_request_from_messages(&request_message_list, tool_call_strategy, tool_config);
        let chat_request = apply_openai_responses_stateful_request_options(
            chat_request,
            previous_response_id,
            store_response,
            instructions,
        );

        if chat_config.stream {
            // 使用 genai 流式处理
            ai_handle_stream_chat(
                &chat_config.client,
                &chat_config.model_name,
                &chat_request,
                &chat_config.chat_options,
                conversation_id,
                &conversation_db,
                &window_clone,
                &app_handle_clone,
                _need_generate_title,
                processed_request.prompt.clone(),
                _config_feature_map.clone(),
                None,                      // 普通ask_ai不需要复用generation_group_id
                None,                      // 普通ask_ai不需要parent_group_id
                model_id,                  // 传递模型ID
                model_code.clone(),        // 传递模型名称
                override_mcp_config,       // MCP override配置
                tool_name_mapping.clone(), // 工具名称映射表
            )
            .await?;
        } else {
            // Use genai non-streaming
            ai_handle_non_stream_chat(
                &chat_config.client,
                &chat_config.model_name,
                &chat_request,
                &chat_config.chat_options,
                conversation_id,
                &conversation_db,
                &window_clone,
                &app_handle_clone,
                _need_generate_title,
                processed_request.prompt.clone(),
                _config_feature_map.clone(),
                None,                // 普通ask_ai不需要复用generation_group_id
                None,                // 普通ask_ai不需要parent_group_id
                model_id,            // 传递模型ID
                model_code.clone(),  // 传递模型名称
                override_mcp_config, // MCP override配置
                tool_name_mapping,   // 工具名称映射表
            )
            .await?;
        }

        let _ = PluginHookBus::new(app_handle_clone.clone())
            .emit_event(
                "chat.afterResponseCompleted",
                {
                    let initial_ids: std::collections::HashSet<i64> = initial_message_ids_for_after_response.iter().copied().collect();
                    let assistant_message_id = ConversationDatabase::new(&app_handle_clone)
                        .ok()
                        .and_then(|db| db.message_repo().ok())
                        .and_then(|repo| repo.list_by_conversation_id(conversation_id).ok())
                        .and_then(|messages| messages.into_iter()
                            .filter(|(message, _)| {
                                (message.message_type == "response" || message.message_type == "assistant")
                                    && !initial_ids.contains(&message.id)
                            })
                            .max_by_key(|(message, _)| message.created_time)
                            .map(|(message, _)| message.id));
                    serde_json::json!({
                    "conversationId": conversation_id,
                    "userMessageId": user_message_id,
                    "assistantMessageId": assistant_message_id,
                    "assistantId": processed_request.assistant_id,
                    "modelId": model_id,
                    "modelCode": model_code,
                    "metadata": {}
                    })
                },
            )
            .await;

        Ok::<(), anyhow::Error>(())
    });

    // Store the task handle for proper cancellation
    message_token_manager.store_task_handle(conversation_id, task_handle).await;

    info!("Ask AI end");

    Ok(AiResponse { conversation_id, request_prompt_result_with_context })
}

#[instrument(skip(app_handle, feature_config_state, window, tool_result), fields(conversation_id = %conversation_id, assistant_id, tool_call_id))]
pub(crate) async fn tool_result_continue_ask_ai_impl(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    window: tauri::Window,
    conversation_id: String,
    assistant_id: i64,
    tool_call_id: String,
    tool_result: String,
) -> Result<AiResponse, AppError> {
    info!("Tool result continuation start");
    debug!(
        tool_result_preview = tool_result.chars().take(200).collect::<String>(),
        "incoming tool result (truncated)"
    );

    let conversation_id_i64 = conversation_id.parse::<i64>()?;
    if let Some(token_manager) = app_handle.try_state::<MessageTokenManager>() {
        token_manager.reset_cancel_token(conversation_id_i64).await;
    }
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;

    // Get conversation details (validate exists)
    let _conversation = db
        .conversation_repo()
        .unwrap()
        .read(conversation_id_i64)
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::DatabaseError("对话未找到".to_string()))?;

    // Get assistant details
    let assistant_detail = get_assistant(app_handle.clone(), assistant_id).unwrap();
    if assistant_detail.model.is_empty() {
        return Err(AppError::NoModelFound);
    }

    // Create tool_result message in database
    let tool_result_content = format!(
        "Tool execution completed:\n\nTool Call ID: {}\nResult:\n{}",
        tool_call_id, tool_result
    );
    let now = chrono::Utc::now();

    // 查找对应的 response 消息的 generation_group_id，使 tool_result 与 response 同组
    let all_msgs_for_group =
        db.message_repo().unwrap().list_by_conversation_id(conversation_id_i64)?;
    let tool_result_group_id = all_msgs_for_group
        .iter()
        .filter(|(m, _)| m.message_type == "response")
        .max_by_key(|(m, _)| m.id)
        .and_then(|(m, _)| m.generation_group_id.clone());
    let existing_tool_result_message =
        find_existing_tool_result_message(&all_msgs_for_group, &tool_call_id);
    let tool_result_message = if let Some(mut existing_message) = existing_tool_result_message {
        existing_message.content = tool_result_content.clone();
        existing_message.finish_time = Some(now);
        if existing_message.start_time.is_none() {
            existing_message.start_time = Some(now);
        }
        db.message_repo().unwrap().update(&existing_message)?;
        existing_message
    } else {
        let tool_result_message = add_message(
            &app_handle,
            None,
            conversation_id_i64,
            "tool_result".to_string(),
            tool_result_content.clone(),
            Some(assistant_detail.model[0].id),
            Some(assistant_detail.model[0].model_code.clone()),
            Some(now),
            Some(now),
            0,
            tool_result_group_id,
            None,
        )?;

        let add_event = ConversationEvent {
            r#type: "message_add".to_string(),
            data: serde_json::to_value(MessageAddEvent {
                message_id: tool_result_message.id,
                message_type: "tool_result".to_string(),
            })
            .unwrap(),
        };
        let _ =
            window.emit(format!("conversation_event_{}", conversation_id_i64).as_str(), add_event);
        tool_result_message
    };

    // 2) message_update (is_done = true)
    let update_event = ConversationEvent {
        r#type: "message_update".to_string(),
        data: serde_json::to_value(MessageUpdateEvent {
            message_id: tool_result_message.id,
            message_type: "tool_result".to_string(),
            content: tool_result_message.content.clone(),
            is_done: true,
            token_count: None,
            input_token_count: None,
            output_token_count: None,
            ttft_ms: None,
            tps: None,
        })
        .unwrap(),
    };
    let _ =
        window.emit(format!("conversation_event_{}", conversation_id_i64).as_str(), update_event);

    if try_dispatch_queued_message(&app_handle, &window, conversation_id_i64, true).await {
        info!(
            conversation_id = conversation_id_i64,
            tool_call_id = %tool_call_id,
            "interrupt queued message dispatched before tool-result continuation"
        );
        return Ok(AiResponse {
            conversation_id: conversation_id_i64,
            request_prompt_result_with_context: "Queued interrupt message dispatched".to_string(),
        });
    }

    // Get all existing messages
    let all_messages = db.message_repo().unwrap().list_by_conversation_id(conversation_id_i64)?;

    // 使用 get_latest_branch_messages 获取最新分支的消息（正确过滤掉废弃分支）
    let latest_branch = crate::api::ai::summary::get_latest_branch_messages(&all_messages);
    debug!(
        total_messages = all_messages.len(),
        branch_messages = latest_branch.len(),
        "filtered messages to latest branch for tool_result_continue"
    );

    // 复用当前分支中最近一条 assistant response 的 generation_group_id，
    // 这样工具调用、工具结果和续写响应仍然归属于同一个逻辑 generation。
    let reuse_generation_group_id: Option<String> = latest_branch
        .iter()
        .filter(|msg| msg.message_type == "response")
        .max_by_key(|msg| msg.id)
        .and_then(|m| m.generation_group_id.clone());

    let (init_message_list, init_metadata) =
        build_message_list_with_metadata_from_db(&all_messages, BranchSelection::LatestBranch);

    let override_mcp_config = enforce_butler_mcp_override(&assistant_detail.assistant.name, None);

    // 收集 MCP 信息
    let mcp_info = collect_mcp_info_for_assistant(
        &app_handle,
        assistant_id,
        override_mcp_config.as_ref(),
        None,
    )
    .await?;

    // Filter butler-only tools for non-butler conversations
    let mcp_info = {
        use crate::mcp::builtin_mcp::templates::{
            is_butler_conversation_kind, is_butler_only_agent_tool, is_butler_only_builtin_command,
        };
        let is_butler_conv = ConversationDatabase::new(&app_handle)
            .ok()
            .and_then(|db| db.conversation_repo().ok())
            .and_then(|repo| repo.read(conversation_id_i64).ok().flatten())
            .map(|c| is_butler_conversation_kind(&c.conversation_kind))
            .unwrap_or(false);
        if is_butler_conv {
            mcp_info
        } else {
            let filtered_servers = mcp_info
                .enabled_servers
                .into_iter()
                .filter_map(|mut server| {
                    if server.command.as_deref().map_or(false, is_butler_only_builtin_command) {
                        return None;
                    }
                    if server.command.as_deref() == Some("aipp:agent") {
                        server.tools.retain(|t| !is_butler_only_agent_tool(&t.name));
                    }
                    if server.tools.is_empty() {
                        None
                    } else {
                        Some(server)
                    }
                })
                .collect();
            MCPInfoForAssistant { enabled_servers: filtered_servers, ..mcp_info }
        }
    };
    let is_native_toolcall = mcp_info.use_native_toolcall;

    // Get model details (same as ask_ai)
    let llm_db = LLMDatabase::new(&app_handle).map_err(AppError::from)?;
    let provider_id = &assistant_detail.model[0].provider_id;
    let model_code = &assistant_detail.model[0].model_code;
    let model_detail = llm_db
        .get_llm_model_detail(provider_id, model_code)
        .context("Failed to get LLM model detail")?;

    let window_clone = window.clone();
    let model_id = model_detail.model.id;
    let provider_id = model_detail.provider.id;
    let model_code = model_detail.model.code.clone();
    let model_request_mode = model_detail.model.request_mode.clone();
    let model_configs = model_detail.configs.clone();
    let provider_api_type = model_detail.provider.api_type.clone();
    let assistant_model_configs = assistant_detail.model_configs.clone();

    // 获取配置
    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();

    let conversation_db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    // Build chat configuration (same as ask_ai)
    let temp_model_detail = crate::db::llm_db::ModelDetail {
        model: crate::db::llm_db::LLMModel {
            id: model_id,
            name: model_code.clone(),
            code: model_code.clone(),
            llm_provider_id: 0,
            description: String::new(),
            vision_support: false,
            audio_support: false,
            video_support: false,
            request_mode: model_request_mode.clone(),
        },
        provider: crate::db::llm_db::LLMProvider {
            id: 0,
            name: String::new(),
            api_type: provider_api_type.clone(),
            description: String::new(),
            is_official: false,
            is_enabled: true,
        },
        configs: model_configs.clone(),
    };

    let model_config_clone =
        ConfigBuilder::merge_model_configs(assistant_model_configs, &temp_model_detail, None);

    let config_map = model_config_clone
        .iter()
        .filter_map(|config| {
            config.value.as_ref().map(|value| (config.name.clone(), value.clone()))
        })
        .collect::<HashMap<String, String>>();

    let stream = config_map.get("stream").and_then(|v| v.parse().ok()).unwrap_or(false);

    let model_name = config_map.get("model").cloned().unwrap_or_else(|| model_code.clone());

    let openai_cache_context = OpenAiCacheContext {
        provider_id,
        provider_api_type: provider_api_type.clone(),
        model_code: model_code.clone(),
        request_mode: model_request_mode.clone(),
        assistant_id,
        conversation_id: conversation_id_i64,
    };
    let chat_options = ConfigBuilder::build_chat_options(
        &config_map,
        Some(&config_feature_map),
        Some(&openai_cache_context),
    );

    // 先计算强制降级条件
    let force_non_native_for_gemini_toolresult =
        provider_api_type == "openai" && model_code.to_lowercase().contains("gemini");
    let force_non_native_for_invalid_tool_args =
        has_missing_required_parameter_tool_error(&latest_branch);
    if force_non_native_for_invalid_tool_args {
        warn!(
            conversation_id = conversation_id_i64,
            "detected missing required parameter tool error; forcing non-native continuation"
        );
    }

    let client = genai_client::create_client_with_config(
        &crate::api::copilot_token_manager::prepare_provider_configs(
            &app_handle,
            &provider_api_type,
            &model_configs,
            None,
        )
        .await
        .map_err(|e| AppError::ProviderError(e))?,
        &model_code,
        &provider_api_type,
        Some(&model_request_mode),
        None,
        false,
        None,
        stream,
        &config_feature_map,
    )
    .map_err(|e| {
        error!(error = %e, "failed to create client in tool_result_continue_ask_ai");
        e
    })?;

    // 动态判断是否有可用的工具（考虑强制降级的情况）
    let has_available_tools = is_native_toolcall
        && !mcp_info.enabled_servers.is_empty()
        && !force_non_native_for_gemini_toolresult
        && !force_non_native_for_invalid_tool_args;

    // 同 ask_ai：避免 OpenAI 兼容通道 + Gemini 模型导致的 usage 反序列化报错日志
    let provider_api_type_lc = provider_api_type.to_lowercase();
    let model_code_lc = model_code.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let is_gemini = model_code_lc.contains("gemini");
    let capture_usage = !(is_openai_like && is_gemini);
    let capture_content = stream && model_request_mode.eq_ignore_ascii_case("responses");

    let capture_reasoning_content = stream
        && is_openai_like
        && has_available_tools
        && config_map.contains_key("reasoning_effort");

    let chat_config = ChatConfig {
        model_name,
        stream,
        chat_options: chat_options
            .with_normalize_reasoning_content(true)
            .with_capture_content(capture_content)
            .with_capture_reasoning_content(capture_reasoning_content)
            .with_capture_usage(capture_usage)
            .with_capture_tool_calls(has_available_tools), // 动态设置
        client,
    };

    info!(
        model = chat_config.model_name,
        stream = chat_config.stream,
        has_tools = has_available_tools,
        provider_api_type = %provider_api_type,
        capture_content = capture_content,
        capture_usage = capture_usage,
        capture_reasoning_content = capture_reasoning_content,
        is_openai_like = is_openai_like,
        is_gemini = is_gemini,
        force_non_native_for_gemini_toolresult,
        force_non_native_for_invalid_tool_args,
        "chat configuration (tool_result_continue)"
    );

    info!(
        model = chat_config.model_name,
        stream = chat_config.stream,
        has_tools = has_available_tools,
        "chat configuration (tool_result_continue)"
    );

    let tool_call_strategy = select_tool_call_strategy(has_available_tools);
    let tool_config =
        build_tool_config(&app_handle, &mcp_info, has_available_tools, Some(conversation_id_i64));
    let init_message_ids_for_stateful = init_metadata.message_ids.clone();
    let has_unfinished_tool_call = has_active_mcp_calls(&app_handle, conversation_id_i64);

    // Context budget management with compaction for continuation
    let budget = ContextBudget::from_config(&config_feature_map);
    let is_butler_cont = is_butler_system_assistant_name(&assistant_detail.assistant.name);
    let compaction_ctx = CompactionContext {
        client: &chat_config.client,
        model_name: &chat_config.model_name,
        conversation_id: conversation_id_i64,
        conversation_db: &conversation_db,
        is_butler: is_butler_cont,
        message_ids: init_metadata.message_ids,
    };
    emit_chat_context_event(
        &app_handle,
        "chat.beforeBuildContext",
        conversation_id_i64,
        assistant_detail.assistant.id,
        &init_message_list,
    )
    .await;
    let fit_result = context_manager::fit_to_budget_with_compaction(
        init_message_list,
        &budget,
        &init_metadata.db_token_counts,
        compaction_ctx,
    )
    .await;
    emit_chat_context_event(
        &app_handle,
        "chat.afterBuildContext",
        conversation_id_i64,
        assistant_detail.assistant.id,
        &fit_result.messages,
    )
    .await;
    let init_message_list = run_before_model_request_hook(
        &app_handle,
        conversation_id_i64,
        assistant_detail.assistant.id,
        fit_result.messages,
    )
    .await?;
    if fit_result.compacted {
        info!(
            conversation_id = conversation_id_i64,
            estimated_tokens = fit_result.estimated_tokens,
            "compaction triggered in tool_result_continue"
        );
    }

    let (request_message_list, previous_response_id, store_response, instructions) =
        maybe_select_openai_responses_stateful_messages(
            &config_feature_map,
            &provider_api_type,
            &model_request_mode,
            conversation_id_i64,
            &init_message_list,
            &init_message_ids_for_stateful,
        &all_messages,
        false,
        has_unfinished_tool_call,
        has_available_tools,
    );
    let (request_message_list, instructions) = prepare_openai_responses_request_messages(
        &provider_api_type,
        &model_request_mode,
        request_message_list,
        instructions,
    );
    let request_message_list = backfill_request_message_list(
        &app_handle,
        conversation_id_i64,
        request_message_list,
    )
    .await?;

    let ChatRequestBuildResult { chat_request, tool_name_mapping } =
        build_chat_request_from_messages(&request_message_list, tool_call_strategy, tool_config);
    let chat_request = apply_openai_responses_stateful_request_options(
        chat_request,
        previous_response_id,
        store_response,
        instructions,
    );

    if chat_config.stream {
        ai_handle_stream_chat(
            &chat_config.client,
            &chat_config.model_name,
            &chat_request,
            &chat_config.chat_options,
            conversation_id_i64,
            &conversation_db,
            &window_clone,
            &app_handle,
            false,                             // no title generation needed
            String::new(),                     // no user prompt
            HashMap::new(),                    // no feature config needed
            reuse_generation_group_id.clone(), // 复用上一条assistant响应的generation_group_id
            None,                              // no parent_group_id
            model_id,
            model_code.clone(),
            override_mcp_config.clone(), // preserve Butler MCP override config
            tool_name_mapping.clone(),   // 工具名称映射表
        )
        .await?;
    } else {
        ai_handle_non_stream_chat(
            &chat_config.client,
            &chat_config.model_name,
            &chat_request,
            &chat_config.chat_options,
            conversation_id_i64,
            &conversation_db,
            &window_clone,
            &app_handle,
            false,                             // no title generation needed
            String::new(),                     // no user prompt
            HashMap::new(),                    // no feature config needed
            reuse_generation_group_id.clone(), // 复用上一条assistant响应的generation_group_id
            None,                              // no parent_group_id
            model_id,
            model_code.clone(),
            override_mcp_config,       // preserve Butler MCP override config
            tool_name_mapping.clone(), // 工具名称映射表
        )
        .await?;
    }

    info!("Tool result continuation end");

    Ok(AiResponse {
        conversation_id: conversation_id_i64,
        request_prompt_result_with_context: format!("Tool result: {}", tool_result),
    })
}

/// 批量工具结果续写：不创建新的 tool_result 消息，只触发 AI 续写
/// 用于 send_mcp_tool_results 已经创建了所有 tool_result 消息后的续写
#[instrument(skip(app_handle, feature_config_state, window), fields(conversation_id, assistant_id))]
pub(crate) async fn batch_tool_result_continue_ask_ai_impl(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    window: tauri::Window,
    conversation_id: i64,
    assistant_id: i64,
) -> Result<AiResponse, AppError> {
    info!("Batch tool result continuation start");
    if let Some(token_manager) = app_handle.try_state::<MessageTokenManager>() {
        token_manager.reset_cancel_token(conversation_id).await;
    }

    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;

    // Get conversation details (validate exists)
    let _conversation = db
        .conversation_repo()
        .unwrap()
        .read(conversation_id)
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::DatabaseError("对话未找到".to_string()))?;

    // Get assistant details
    let assistant_detail = get_assistant(app_handle.clone(), assistant_id).unwrap();
    if assistant_detail.model.is_empty() {
        return Err(AppError::NoModelFound);
    }

    if try_dispatch_queued_message(&app_handle, &window, conversation_id, true).await {
        info!(
            conversation_id,
            "interrupt queued message dispatched before batch tool-result continuation"
        );
        return Ok(AiResponse {
            conversation_id,
            request_prompt_result_with_context: "Queued interrupt message dispatched".to_string(),
        });
    }

    // Get all existing messages (including the just-created tool_result messages)
    let all_messages = db.message_repo().unwrap().list_by_conversation_id(conversation_id)?;

    // 使用 get_latest_branch_messages 获取最新分支的消息（正确过滤掉废弃分支）
    let latest_branch = crate::api::ai::summary::get_latest_branch_messages(&all_messages);
    debug!(
        total_messages = all_messages.len(),
        branch_messages = latest_branch.len(),
        "filtered messages to latest branch"
    );

    // 尝试复用上一次包含工具调用的 assistant 响应的 generation_group_id
    let reuse_generation_group_id: Option<String> = {
        latest_branch
            .iter()
            .filter(|msg| msg.message_type == "response")
            .max_by_key(|msg| msg.id)
            .and_then(|m| m.generation_group_id.clone())
    };

    let (init_message_list, init_metadata) =
        build_message_list_with_metadata_from_db(&all_messages, BranchSelection::LatestBranch);

    let override_mcp_config = enforce_butler_mcp_override(&assistant_detail.assistant.name, None);

    // 收集 MCP 信息
    let mcp_info = collect_mcp_info_for_assistant(
        &app_handle,
        assistant_id,
        override_mcp_config.as_ref(),
        None,
    )
    .await?;

    // Filter butler-only tools for non-butler conversations
    let mcp_info = {
        use crate::mcp::builtin_mcp::templates::{
            is_butler_conversation_kind, is_butler_only_agent_tool, is_butler_only_builtin_command,
        };
        let is_butler_conv = ConversationDatabase::new(&app_handle)
            .ok()
            .and_then(|db| db.conversation_repo().ok())
            .and_then(|repo| repo.read(conversation_id).ok().flatten())
            .map(|c| is_butler_conversation_kind(&c.conversation_kind))
            .unwrap_or(false);
        if is_butler_conv {
            mcp_info
        } else {
            let filtered_servers = mcp_info
                .enabled_servers
                .into_iter()
                .filter_map(|mut server| {
                    if server.command.as_deref().map_or(false, is_butler_only_builtin_command) {
                        return None;
                    }
                    if server.command.as_deref() == Some("aipp:agent") {
                        server.tools.retain(|t| !is_butler_only_agent_tool(&t.name));
                    }
                    if server.tools.is_empty() {
                        None
                    } else {
                        Some(server)
                    }
                })
                .collect();
            MCPInfoForAssistant { enabled_servers: filtered_servers, ..mcp_info }
        }
    };
    let is_native_toolcall = mcp_info.use_native_toolcall;

    // Get model details
    let llm_db = LLMDatabase::new(&app_handle).map_err(AppError::from)?;
    let provider_id = &assistant_detail.model[0].provider_id;
    let model_code = &assistant_detail.model[0].model_code;
    let model_detail = llm_db
        .get_llm_model_detail(provider_id, model_code)
        .context("Failed to get LLM model detail")?;

    let window_clone = window.clone();
    let model_id = model_detail.model.id;
    let provider_id = model_detail.provider.id;
    let model_code = model_detail.model.code.clone();
    let model_request_mode = model_detail.model.request_mode.clone();
    let model_configs = model_detail.configs.clone();
    let provider_api_type = model_detail.provider.api_type.clone();
    let assistant_model_configs = assistant_detail.model_configs.clone();

    // 获取配置
    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();

    let conversation_db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    // Build chat configuration
    let temp_model_detail = crate::db::llm_db::ModelDetail {
        model: crate::db::llm_db::LLMModel {
            id: model_id,
            name: model_code.clone(),
            code: model_code.clone(),
            llm_provider_id: 0,
            description: String::new(),
            vision_support: false,
            audio_support: false,
            video_support: false,
            request_mode: model_request_mode.clone(),
        },
        provider: crate::db::llm_db::LLMProvider {
            id: 0,
            name: String::new(),
            api_type: provider_api_type.clone(),
            description: String::new(),
            is_official: false,
            is_enabled: true,
        },
        configs: model_configs.clone(),
    };

    let model_config_clone =
        ConfigBuilder::merge_model_configs(assistant_model_configs, &temp_model_detail, None);

    let config_map = model_config_clone
        .iter()
        .filter_map(|config| {
            config.value.as_ref().map(|value| (config.name.clone(), value.clone()))
        })
        .collect::<HashMap<String, String>>();

    let stream = config_map.get("stream").and_then(|v| v.parse().ok()).unwrap_or(false);

    let model_name = config_map.get("model").cloned().unwrap_or_else(|| model_code.clone());

    let openai_cache_context = OpenAiCacheContext {
        provider_id,
        provider_api_type: provider_api_type.clone(),
        model_code: model_code.clone(),
        request_mode: model_request_mode.clone(),
        assistant_id,
        conversation_id,
    };
    let chat_options = ConfigBuilder::build_chat_options(
        &config_map,
        Some(&config_feature_map),
        Some(&openai_cache_context),
    );

    // 先计算强制降级条件
    let force_non_native_for_gemini_toolresult =
        provider_api_type == "openai" && model_code.to_lowercase().contains("gemini");
    let force_non_native_for_invalid_tool_args =
        has_missing_required_parameter_tool_error(&latest_branch);
    if force_non_native_for_invalid_tool_args {
        warn!(
            conversation_id,
            "detected missing required parameter tool error; forcing non-native continuation"
        );
    }

    let client = genai_client::create_client_with_config(
        &crate::api::copilot_token_manager::prepare_provider_configs(
            &app_handle,
            &provider_api_type,
            &model_configs,
            None,
        )
        .await
        .map_err(|e| AppError::ProviderError(e))?,
        &model_code,
        &provider_api_type,
        Some(&model_request_mode),
        None,
        false,
        None,
        stream,
        &config_feature_map,
    )
    .map_err(|e| {
        error!(error = %e, "failed to create client in batch_tool_result_continue_ask_ai");
        e
    })?;

    // 动态判断是否有可用的工具（考虑强制降级的情况）
    let has_available_tools = is_native_toolcall
        && !mcp_info.enabled_servers.is_empty()
        && !force_non_native_for_gemini_toolresult
        && !force_non_native_for_invalid_tool_args;

    let provider_api_type_lc = provider_api_type.to_lowercase();
    let model_code_lc = model_code.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let is_gemini = model_code_lc.contains("gemini");
    let capture_usage = !(is_openai_like && is_gemini);
    let capture_content = stream && model_request_mode.eq_ignore_ascii_case("responses");

    let capture_reasoning_content = stream
        && is_openai_like
        && has_available_tools
        && config_map.contains_key("reasoning_effort");

    let chat_config = ChatConfig {
        model_name,
        stream,
        chat_options: chat_options
            .with_normalize_reasoning_content(true)
            .with_capture_content(capture_content)
            .with_capture_reasoning_content(capture_reasoning_content)
            .with_capture_usage(capture_usage)
            .with_capture_tool_calls(has_available_tools),
        client,
    };

    info!(
        model = chat_config.model_name,
        stream = chat_config.stream,
        has_tools = has_available_tools,
        provider_api_type = %provider_api_type,
        capture_content = capture_content,
        capture_reasoning_content = capture_reasoning_content,
        force_non_native_for_gemini_toolresult,
        force_non_native_for_invalid_tool_args,
        "chat configuration (batch_tool_result_continue)"
    );

    let tool_call_strategy = select_tool_call_strategy(has_available_tools);
    let tool_config =
        build_tool_config(&app_handle, &mcp_info, has_available_tools, Some(conversation_id));
    let init_message_ids_for_stateful = init_metadata.message_ids.clone();
    let has_unfinished_tool_call = has_active_mcp_calls(&app_handle, conversation_id);

    // Context budget management with compaction for batch continuation
    let budget = ContextBudget::from_config(&config_feature_map);
    let is_butler_batch = is_butler_system_assistant_name(&assistant_detail.assistant.name);
    let compaction_ctx = CompactionContext {
        client: &chat_config.client,
        model_name: &chat_config.model_name,
        conversation_id,
        conversation_db: &conversation_db,
        is_butler: is_butler_batch,
        message_ids: init_metadata.message_ids,
    };
    emit_chat_context_event(
        &app_handle,
        "chat.beforeBuildContext",
        conversation_id,
        assistant_detail.assistant.id,
        &init_message_list,
    )
    .await;
    let fit_result = context_manager::fit_to_budget_with_compaction(
        init_message_list,
        &budget,
        &init_metadata.db_token_counts,
        compaction_ctx,
    )
    .await;
    emit_chat_context_event(
        &app_handle,
        "chat.afterBuildContext",
        conversation_id,
        assistant_detail.assistant.id,
        &fit_result.messages,
    )
    .await;
    let init_message_list = run_before_model_request_hook(
        &app_handle,
        conversation_id,
        assistant_detail.assistant.id,
        fit_result.messages,
    )
    .await?;
    if fit_result.compacted {
        info!(
            conversation_id,
            estimated_tokens = fit_result.estimated_tokens,
            "compaction triggered in batch_tool_result_continue"
        );
    }

    let (request_message_list, previous_response_id, store_response, instructions) =
        maybe_select_openai_responses_stateful_messages(
            &config_feature_map,
            &provider_api_type,
            &model_request_mode,
            conversation_id,
            &init_message_list,
            &init_message_ids_for_stateful,
            &all_messages,
            false,
            has_unfinished_tool_call,
            has_available_tools,
        );
    let (request_message_list, instructions) = prepare_openai_responses_request_messages(
        &provider_api_type,
        &model_request_mode,
        request_message_list,
        instructions,
    );
    let request_message_list =
        backfill_request_message_list(&app_handle, conversation_id, request_message_list).await?;

    let ChatRequestBuildResult { chat_request, tool_name_mapping } =
        build_chat_request_from_messages(&request_message_list, tool_call_strategy, tool_config);
    let chat_request = apply_openai_responses_stateful_request_options(
        chat_request,
        previous_response_id,
        store_response,
        instructions,
    );

    if chat_config.stream {
        Box::pin(ai_handle_stream_chat(
            &chat_config.client,
            &chat_config.model_name,
            &chat_request,
            &chat_config.chat_options,
            conversation_id,
            &conversation_db,
            &window_clone,
            &app_handle,
            false,
            String::new(),
            HashMap::new(),
            reuse_generation_group_id.clone(),
            None,
            model_id,
            model_code.clone(),
            override_mcp_config.clone(),
            tool_name_mapping.clone(),
        ))
        .await?;
    } else {
        Box::pin(ai_handle_non_stream_chat(
            &chat_config.client,
            &chat_config.model_name,
            &chat_request,
            &chat_config.chat_options,
            conversation_id,
            &conversation_db,
            &window_clone,
            &app_handle,
            false,
            String::new(),
            HashMap::new(),
            reuse_generation_group_id,
            None,
            model_id,
            model_code.clone(),
            override_mcp_config,
            tool_name_mapping,
        ))
        .await?;
    }

    info!("Batch tool result continuation end");

    Ok(AiResponse {
        conversation_id,
        request_prompt_result_with_context: "Batch tool results sent".to_string(),
    })
}

#[tauri::command]
#[instrument(skip(app_handle, _state, feature_config_state, window, tool_result), fields(conversation_id = %conversation_id, assistant_id, tool_call_id))]
pub async fn tool_result_continue_ask_ai(
    app_handle: tauri::AppHandle,
    _state: State<'_, AppState>,
    feature_config_state: State<'_, FeatureConfigState>,
    window: tauri::Window,
    conversation_id: String,
    assistant_id: i64,
    tool_call_id: String,
    tool_result: String,
) -> Result<AiResponse, AppError> {
    tool_result_continue_ask_ai_impl(
        app_handle,
        feature_config_state,
        window,
        conversation_id,
        assistant_id,
        tool_call_id,
        tool_result,
    )
    .await
}

#[tauri::command]
pub async fn cancel_ai(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    acp_session_state: State<'_, AcpSessionState>,
    message_token_manager: State<'_, MessageTokenManager>,
    window: tauri::Window,
    conversation_id: i64,
) -> Result<(), String> {
    let codex_session_state = app_handle.state::<CodexSessionState>();
    let claude_session_state = app_handle.state::<ClaudeSessionState>();
    let acp_session_handle = {
        let sessions = acp_session_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.handle.clone())
    };

    let codex_session_handle = {
        let sessions = codex_session_state.sessions.lock().await;
        sessions.get(&conversation_id).map(|entry| entry.handle.clone())
    };
    let claude_session_handle = { let sessions = claude_session_state.sessions.lock().await; sessions.get(&conversation_id).map(|entry| entry.handle.clone()) };

    if let Some(handle) = claude_session_handle {
        handle.cancel_current_prompt().await.map_err(|error| error.to_string())?;
    } else if let Some(handle) = codex_session_handle {
        handle.cancel_current_prompt().await.map_err(|error| error.to_string())?;
    } else if let Some(handle) = acp_session_handle {
        handle.cancel_current_prompt().await.map_err(|error| error.to_string())?;
    } else {
        message_token_manager.cancel_request(conversation_id).await;
    }

    if let Err(e) = cancel_mcp_tool_calls_by_conversation(&app_handle, conversation_id).await {
        warn!(conversation_id, error = %e, "failed to cancel MCP tool calls for conversation");
    }

    // 更新所有正在进行中的消息的 finish_time
    if let Ok(db) = ConversationDatabase::new(&app_handle) {
        if let Ok(message_repo) = db.message_repo() {
            match message_repo.finish_pending_messages(conversation_id) {
                Ok(count) => {
                    if count > 0 {
                        debug!(conversation_id, count, "finished pending messages on cancel");
                    }
                }
                Err(e) => {
                    warn!(conversation_id, error = %e, "failed to finish pending messages on cancel");
                }
            }
        }
    }

    if let Some(activity_manager) = app_handle.try_state::<ConversationActivityManager>() {
        activity_manager.clear_focus(&app_handle, conversation_id).await;
    }

    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    if let Err(err) = maybe_generate_title_from_conversation_if_needed(
        &app_handle,
        conversation_id,
        config_feature_map,
        window,
        "manual cancel",
    )
    .await
    {
        warn!(
            conversation_id,
            error = %err,
            "failed to schedule title generation after manual cancel"
        );
    }

    // Send cancellation event to both ask and chat_ui windows
    let cancel_event = crate::api::ai::events::ConversationEvent {
        r#type: "conversation_cancel".to_string(),
        data: serde_json::json!({
            "conversation_id": conversation_id,
            "cancelled_at": chrono::Utc::now(),
        }),
    };

    send_conversation_event_to_chat_windows(&app_handle, conversation_id, cancel_event);

    if let Err(error) = mark_butler_task_cancelled(&app_handle, conversation_id).await {
        warn!(conversation_id, error = %error, "failed to finalize butler task on cancel");
    }

    Ok(())
}

#[tauri::command]
#[instrument(
    skip(app_handle, feature_config_state, activity_manager, message_token_manager, window),
    fields(message_id)
)]
pub async fn regenerate_ai(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    activity_manager: State<'_, ConversationActivityManager>,
    message_token_manager: State<'_, MessageTokenManager>,
    window: tauri::Window,
    message_id: i64,
) -> Result<AiResponse, AppError> {
    info!("Regenerate AI start");
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    let message = db
        .message_repo()
        .unwrap()
        .read(message_id)?
        .ok_or(AppError::DatabaseError("未找到消息".to_string()))?;

    let conversation_id = message.conversation_id;
    let conversation = db
        .conversation_repo()
        .unwrap()
        .read(conversation_id)?
        .ok_or(AppError::DatabaseError("未找到对话".to_string()))?;
    let messages = db.message_repo().unwrap().list_by_conversation_id(conversation_id)?;

    // 重新生成开始时，优先让被点击的消息闪亮（可被后续 streaming 覆盖）
    if message.message_type == "user" {
        activity_manager.set_user_pending(&app_handle, conversation_id, message_id).await;
    } else {
        activity_manager.set_assistant_streaming(&app_handle, conversation_id, message_id).await;
    }

    message_token_manager.reset_cancel_token(conversation_id).await;

    // 根据消息类型决定处理逻辑
    let (filtered_messages, _parent_message_id) = if message.message_type == "user" {
        // 用户消息重发：包含当前用户消息和之前的所有消息，新生成的assistant消息没有parent（新一轮对话）
        let filtered_messages: Vec<(Message, Option<MessageAttachment>)> = messages
            .into_iter()
            .filter(|m| m.0.id <= message_id) // 包含当前消息
            .collect();
        (filtered_messages, None) // 用户消息重发时，新的AI回复没有parent_id
    } else {
        // AI消息重新生成：仅保留在待重新生成消息之前的历史消息，新消息以被重发的原消息为parent
        let filtered_messages: Vec<(Message, Option<MessageAttachment>)> =
            messages.into_iter().filter(|m| m.0.id < message_id).collect();
        (filtered_messages, Some(message_id)) // 使用被重发消息的ID作为parent_id表示这是它的一个版本
    };

    // 确定要使用的generation_group_id和parent_group_id
    let (regenerate_generation_group_id, regenerate_parent_group_id) = if message.message_type
        == "user"
    {
        // 用户消息重发：为新的AI回复生成全新的group_id
        // 查找该user message后面第一条非user、非system的消息，用它的generation_group_id作为parent_group_id
        let mut parent_group_id: Option<String> = None;

        // 获取对话中的所有消息，按ID排序
        let all_messages = db.message_repo().unwrap().list_by_conversation_id(conversation_id)?;

        // 找到当前user message在列表中的位置
        if let Some(message_index) = all_messages.iter().position(|(msg, _)| msg.id == message_id) {
            // 查找该user message后面第一条非user、非system的消息
            for (next_msg, _) in all_messages.iter().skip(message_index + 1) {
                if next_msg.message_type != "user"
                    && next_msg.message_type != "system"
                    && next_msg.generation_group_id.is_some()
                {
                    parent_group_id = next_msg.generation_group_id.clone();
                    debug!(?parent_group_id, "parent_group_id for user message regenerate");
                    break;
                }
            }
        }

        (Some(uuid::Uuid::new_v4().to_string()), parent_group_id)
    } else {
        // AI消息重发：生成新的group_id，并将原消息的group_id作为parent_group_id
        let original_group_id = message.generation_group_id.clone();
        (Some(uuid::Uuid::new_v4().to_string()), original_group_id)
    };

    let filtered_messages =
        filter_messages_for_parent_group(filtered_messages, regenerate_parent_group_id.as_deref());

    let (init_message_list, init_metadata) =
        build_message_list_with_metadata_from_db(&filtered_messages, BranchSelection::LatestBranch);

    debug!(?init_message_list, "initial message list for regenerate");

    // 获取助手信息（在构建消息列表之后，以确保对话已确定）
    let assistant_id = conversation.assistant_id.unwrap();
    let assistant_detail = get_assistant(app_handle.clone(), assistant_id).unwrap();

    if assistant_detail.model.is_empty() {
        return Err(AppError::NoModelFound);
    }

    // 兼容 MCP：根据助手配置判断是否使用提供商原生 toolcall
    let mcp_info =
        crate::mcp::collect_mcp_info_for_assistant(&app_handle, assistant_id, None, None).await?;
    let is_native_toolcall = mcp_info.use_native_toolcall;

    // 在异步任务外获取模型详情（避免线程安全问题）
    let llm_db = LLMDatabase::new(&app_handle).map_err(AppError::from)?;
    let provider_id = &assistant_detail.model[0].provider_id;
    let model_code = &assistant_detail.model[0].model_code;
    let model_detail = llm_db
        .get_llm_model_detail(provider_id, model_code)
        .context("Failed to get LLM model detail")?;

    let window_clone = window.clone(); // 在移动之前克隆
    let app_handle_clone = app_handle.clone(); // 添加这行
    let regenerate_model_id = model_detail.model.id; // 提前获取模型ID
    let regenerate_provider_id = model_detail.provider.id;
    let regenerate_model_code = model_detail.model.code.clone(); // 提前获取模型代码
    let regenerate_model_request_mode = model_detail.model.request_mode.clone(); // 提前获取模型请求模式
    let regenerate_model_configs = model_detail.configs.clone(); // 提前获取模型配置
    let regenerate_provider_api_type = model_detail.provider.api_type.clone(); // 提前获取API类型
    let regenerate_assistant_model_configs = assistant_detail.model_configs.clone(); // 提前获取助手模型配置

    // 获取网络配置
    let _config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    let regenerate_task_handle = tokio::spawn(async move {
        // 直接创建数据库连接（避免线程安全问题）
        let conversation_db = ConversationDatabase::new(&app_handle_clone).unwrap();

        // 构建聊天配置
        // 从配置中获取网络代理和超时设置
        let network_proxy = get_network_proxy_from_config(&_config_feature_map);
        let request_timeout = get_request_timeout_from_config(&_config_feature_map);

        // 检查供应商是否启用了代理
        let proxy_enabled = regenerate_model_configs
            .iter()
            .find(|config| config.name == "proxy_enabled")
            .and_then(|config| config.value.parse::<bool>().ok())
            .unwrap_or(false);

        // 创建一个临时的 ModelDetail 用于配置合并
        let temp_model_detail = crate::db::llm_db::ModelDetail {
            model: crate::db::llm_db::LLMModel {
                id: regenerate_model_id,
                name: regenerate_model_code.clone(),
                code: regenerate_model_code.clone(),
                llm_provider_id: 0,         // 临时值
                description: String::new(), // 临时值
                vision_support: false,      // 临时值
                audio_support: false,       // 临时值
                video_support: false,       // 临时值
                request_mode: regenerate_model_request_mode.clone(),
            },
            provider: crate::db::llm_db::LLMProvider {
                id: 0,               // 临时值
                name: String::new(), // 临时值
                api_type: regenerate_provider_api_type.clone(),
                description: String::new(), // 临时值
                is_official: false,         // 临时值
                is_enabled: true,           // 临时值
            },
            configs: regenerate_model_configs.clone(),
        };

        let model_config_clone = ConfigBuilder::merge_model_configs(
            regenerate_assistant_model_configs,
            &temp_model_detail,
            None, // regenerate 不使用覆盖配置
        );

        let config_map = model_config_clone
            .iter()
            .filter_map(|config| {
                config.value.as_ref().map(|value| (config.name.clone(), value.clone()))
            })
            .collect::<HashMap<String, String>>();

        let stream = config_map.get("stream").and_then(|v| v.parse().ok()).unwrap_or(false);

        let model_name =
            config_map.get("model").cloned().unwrap_or_else(|| regenerate_model_code.clone());

        let openai_cache_context = OpenAiCacheContext {
            provider_id: regenerate_provider_id,
            provider_api_type: regenerate_provider_api_type.clone(),
            model_code: regenerate_model_code.clone(),
            request_mode: regenerate_model_request_mode.clone(),
            assistant_id,
            conversation_id,
        };
        let chat_options = ConfigBuilder::build_chat_options(
            &config_map,
            Some(&_config_feature_map),
            Some(&openai_cache_context),
        );

        let force_non_native_for_invalid_tool_args =
            has_missing_required_parameter_tool_error_in_message_list(&init_message_list);
        if force_non_native_for_invalid_tool_args {
            warn!(
                conversation_id,
                "detected missing required parameter tool error in history; forcing non-native regenerate"
            );
        }

        let client = genai_client::create_client_with_config(
            &crate::api::copilot_token_manager::prepare_provider_configs(
                &app_handle_clone,
                &regenerate_provider_api_type,
                &regenerate_model_configs,
                network_proxy.as_deref(),
            )
            .await
            .map_err(|e| AppError::ProviderError(e))?,
            &regenerate_model_code,
            &regenerate_provider_api_type,
            Some(&regenerate_model_request_mode),
            network_proxy.as_deref(),
            proxy_enabled,
            Some(request_timeout),
            stream,
            &_config_feature_map,
        )?;

        // 动态判断是否有可用的工具
        let has_available_tools = is_native_toolcall
            && !mcp_info.enabled_servers.is_empty()
            && !force_non_native_for_invalid_tool_args;

        // 同 ask_ai：避免 OpenAI 兼容通道 + Gemini 模型导致的 usage 反序列化报错日志
        let provider_api_type_lc = regenerate_provider_api_type.to_lowercase();
        let model_code_lc = regenerate_model_code.to_lowercase();
        let is_openai_like =
            provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
        let is_gemini = model_code_lc.contains("gemini");
        let capture_usage = !(is_openai_like && is_gemini);
        let capture_content =
            stream && regenerate_model_request_mode.eq_ignore_ascii_case("responses");

        let capture_reasoning_content = stream
            && is_openai_like
            && has_available_tools
            && config_map.contains_key("reasoning_effort");

        let chat_config = ChatConfig {
            model_name,
            stream,
            chat_options: chat_options
                .with_normalize_reasoning_content(true)
                .with_capture_content(capture_content)
                .with_capture_reasoning_content(capture_reasoning_content)
                .with_capture_usage(capture_usage)
                .with_capture_tool_calls(has_available_tools), // 动态设置
            client,
        };

        info!(
            model = chat_config.model_name,
            stream = chat_config.stream,
            has_tools = has_available_tools,
            provider_api_type = %regenerate_provider_api_type,
            capture_content = capture_content,
            capture_usage = capture_usage,
            capture_reasoning_content = capture_reasoning_content,
            is_openai_like = is_openai_like,
            is_gemini = is_gemini,
            force_non_native_for_invalid_tool_args,
            "chat configuration (regenerate)"
        );

        let tool_call_strategy = select_tool_call_strategy(has_available_tools);
        let tool_config = if has_available_tools {
            if let Ok(mcp_info) = crate::mcp::collect_mcp_info_for_assistant(
                &app_handle_clone,
                assistant_id,
                None,
                None,
            )
            .await
            {
                build_tool_config(&app_handle_clone, &mcp_info, true, Some(conversation_id))
            } else {
                None
            }
        } else {
            None
        };

        // Context budget management with compaction for regenerate
        let budget = ContextBudget::from_config(&_config_feature_map);
        let is_butler_regen = is_butler_system_assistant_name(&assistant_detail.assistant.name);
        let compaction_ctx = CompactionContext {
            client: &chat_config.client,
            model_name: &chat_config.model_name,
            conversation_id,
            conversation_db: &conversation_db,
            is_butler: is_butler_regen,
            message_ids: init_metadata.message_ids,
        };
        emit_chat_context_event(
            &app_handle_clone,
            "chat.beforeBuildContext",
            conversation_id,
            assistant_detail.assistant.id,
            &init_message_list,
        )
        .await;
        let fit_result = context_manager::fit_to_budget_with_compaction(
            init_message_list,
            &budget,
            &init_metadata.db_token_counts,
            compaction_ctx,
        )
        .await;
        emit_chat_context_event(
            &app_handle_clone,
            "chat.afterBuildContext",
            conversation_id,
            assistant_detail.assistant.id,
            &fit_result.messages,
        )
        .await;
        let init_message_list = run_before_model_request_hook(
            &app_handle_clone,
            conversation_id,
            assistant_detail.assistant.id,
            fit_result.messages,
        )
        .await?;

        let (request_message_list, instructions) = prepare_openai_responses_request_messages(
            &regenerate_provider_api_type,
            &regenerate_model_request_mode,
            init_message_list,
            None,
        );
        let request_message_list = backfill_request_message_list(
            &app_handle_clone,
            conversation_id,
            request_message_list,
        )
        .await?;
        let ChatRequestBuildResult { chat_request, tool_name_mapping } =
            build_chat_request_from_messages(&request_message_list, tool_call_strategy, tool_config);
        let chat_request = apply_openai_responses_stateful_request_options(
            chat_request,
            None,
            false,
            instructions,
        );

        if chat_config.stream {
            // 使用 genai 流式处理
            ai_handle_stream_chat(
                &chat_config.client,
                &chat_config.model_name,
                &chat_request,
                &chat_config.chat_options,
                conversation_id,
                &conversation_db,
                &window_clone,
                &app_handle_clone,
                false,                                  // regenerate 不需要生成标题
                String::new(),                          // regenerate 不需要用户提示
                HashMap::new(),                         // regenerate 不需要配置
                regenerate_generation_group_id.clone(), // 传递generation_group_id用于复用
                regenerate_parent_group_id.clone(),     // 传递parent_group_id设置版本关系
                regenerate_model_id,                    // 传递模型ID
                regenerate_model_code.clone(),          // 传递模型名称
                None,                                   // regenerate 不使用 MCP override
                tool_name_mapping.clone(),              // 工具名称映射表
            )
            .await?;
        } else {
            // Use genai non-streaming
            ai_handle_non_stream_chat(
                &chat_config.client,
                &chat_config.model_name,
                &chat_request,
                &chat_config.chat_options,
                conversation_id,
                &conversation_db,
                &window_clone,
                &app_handle_clone,
                false,                                  // regenerate 不需要生成标题
                String::new(),                          // regenerate 不需要用户提示
                HashMap::new(),                         // regenerate 不需要配置
                regenerate_generation_group_id.clone(), // 传递generation_group_id用于复用
                regenerate_parent_group_id.clone(),     // 传递parent_group_id设置版本关系
                regenerate_model_id,                    // 传递模型ID
                regenerate_model_code.clone(),          // 传递模型名称
                None,                                   // regenerate 不使用 MCP override
                tool_name_mapping,                      // 工具名称映射表
            )
            .await?;
        }

        Ok::<(), anyhow::Error>(())
    });

    // Store the task handle for proper cancellation
    message_token_manager.store_task_handle(conversation_id, regenerate_task_handle).await;

    info!("Regenerate AI dispatched (background task started)");

    Ok(AiResponse { conversation_id, request_prompt_result_with_context: String::new() })
}

pub(crate) fn add_message(
    app_handle: &tauri::AppHandle,
    parent_id: Option<i64>,
    conversation_id: i64,
    message_type: String,
    content: String,
    llm_model_id: Option<i64>,
    llm_model_name: Option<String>,
    start_time: Option<chrono::DateTime<chrono::Utc>>,
    finish_time: Option<chrono::DateTime<chrono::Utc>>,
    token_count: i32,
    generation_group_id: Option<String>,
    parent_group_id: Option<String>,
) -> Result<Message, AppError> {
    let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    let message = db
        .message_repo()
        .unwrap()
        .create(&Message {
            id: 0,
            parent_id,
            conversation_id,
            message_type,
            content,
            llm_model_id,
            llm_model_name,
            start_time,
            finish_time,
            created_time: chrono::Utc::now(),
            token_count,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id,
            parent_group_id,
            tool_calls_json: None,
            metadata_json: None,
            first_token_time: None,
            ttft_ms: None,
        })
        .map_err(AppError::from)?;

    // 如果是用户消息，删除已有的对话总结，下次空闲时自动重新生成
    if message.message_type == "user" {
        if let Ok(summary_repo) = db.conversation_summary_repo() {
            let _ = summary_repo.delete_by_conversation_id(conversation_id);
        }
    }

    Ok(message.clone())
}

async fn initialize_conversation(
    app_handle: &tauri::AppHandle,
    request: &AiRequest,
    assistant_detail: &AssistantDetail,
    assistant_prompt_result: String,
    display_user_prompt: String,
    runtime_user_prompt: String,
    override_prompt: Option<String>,
    extra_user_attachments: Vec<MessageAttachment>,
) -> Result<
    (
        i64,
        Option<i64>,
        i64,
        String,
        Vec<(String, String, Vec<MessageAttachment>)>,
        Vec<i64>,
        Vec<i32>,
    ),
    AppError,
> {
    // 返回值：(conversation_id, add_message_id, user_message_id, request_prompt_with_context, init_message_list, message_ids, db_token_counts)
    let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;

    let system_prompt = override_prompt.unwrap_or(assistant_prompt_result);

    let (
        conversation_id,
        add_message_id,
        user_message_id,
        request_prompt_result_with_context,
        init_message_list,
        message_ids,
        db_token_counts,
    ) = if request.conversation_id.is_empty() {
        let mut message_attachment_list = db
            .attachment_repo()
            .unwrap()
            .list_by_id(&request.attachment_list.clone().unwrap_or(vec![]))?;
        message_attachment_list.extend(extra_user_attachments.clone());
        // 新对话逻辑
        let text_attachments: Vec<String> = message_attachment_list
            .iter()
            .filter(|a| matches!(a.attachment_type, AttachmentType::Text))
            .filter_map(|a| {
                Some(format!(
                    r#"<fileattachment name="{}">{}</fileattachment>"#,
                    a.attachment_url.clone().unwrap(),
                    a.attachment_content.clone().unwrap().as_str()
                ))
            })
            .collect();
        let context = text_attachments.join("\n");
        let display_user_prompt_with_context =
            build_prompt_with_attachment_context(&display_user_prompt, &context);
        let runtime_user_prompt_with_context =
            build_prompt_with_attachment_context(&runtime_user_prompt, &context);
        let db_init_message_list = vec![
            (String::from("system"), system_prompt.clone(), vec![]),
            (
                String::from("user"),
                display_user_prompt_with_context.clone(),
                message_attachment_list.clone(),
            ),
        ];
        let runtime_init_message_list = vec![
            (String::from("system"), system_prompt, vec![]),
            (
                String::from("user"),
                runtime_user_prompt_with_context.clone(),
                message_attachment_list,
            ),
        ];
        debug!(
            assistant_id = request.assistant_id,
            ?runtime_init_message_list,
            "initialize new conversation"
        );
        let (conversation, created_messages) = init_conversation(
            app_handle,
            request.assistant_id,
            assistant_detail.model[0].id,
            assistant_detail.model[0].model_code.clone(),
            &db_init_message_list,
        )?;
        // 获取用户消息的 ID（第二条消息是 user 类型）
        let user_msg_id =
            created_messages.iter().find(|m| m.message_type == "user").map(|m| m.id).unwrap_or(0);
        (
            conversation.id,
            None, // 不预先创建空的assistant消息，让流式处理动态创建
            user_msg_id,
            runtime_user_prompt_with_context,
            runtime_init_message_list,
            vec![], // New conversation: no message IDs needed (too few for compaction)
            vec![], // New conversation: no DB token counts
        )
    } else {
        // 已存在对话逻辑
        let conversation_id = request.conversation_id.parse::<i64>()?;
        let all_messages = db.message_repo().unwrap().list_by_conversation_id(conversation_id)?;

        let (message_list, metadata) =
            build_message_list_with_metadata_from_db(&all_messages, BranchSelection::LatestBranch);
        let mut branch_message_ids = metadata.message_ids;
        let mut branch_db_token_counts = metadata.db_token_counts;
        let has_system_message =
            message_list.iter().any(|(message_type, _, _)| message_type == "system");

        if !has_system_message {
            let system_message_created_time = all_messages
                .first()
                .map(|(message, _)| {
                    message.created_time.clone() - chrono::Duration::milliseconds(1)
                })
                .unwrap_or_else(chrono::Utc::now);
            db.message_repo()
                .unwrap()
                .create_without_touch_conversation(&Message {
                    id: 0,
                    parent_id: None,
                    conversation_id,
                    message_type: "system".to_string(),
                    content: system_prompt.clone(),
                    llm_model_id: Some(assistant_detail.model[0].id),
                    llm_model_name: Some(assistant_detail.model[0].model_code.clone()),
                    created_time: system_message_created_time,
                    start_time: None,
                    finish_time: None,
                    token_count: 0,
                    input_token_count: 0,
                    output_token_count: 0,
                    generation_group_id: None,
                    parent_group_id: None,
                    tool_calls_json: None,
                    metadata_json: None,
                    first_token_time: None,
                    ttft_ms: None,
                })
                .map_err(AppError::from)?;
            debug!(
                conversation_id,
                assistant_id = request.assistant_id,
                "injected missing system prompt into existing conversation"
            );
        }

        // 获取到消息的附件列表
        let mut message_attachment_list = db
            .attachment_repo()
            .unwrap()
            .list_by_id(&request.attachment_list.clone().unwrap_or(vec![]))?;
        message_attachment_list.extend(extra_user_attachments.clone());
        // 过滤出文本附件
        let text_attachments: Vec<String> = message_attachment_list
            .iter()
            .filter(|a| matches!(a.attachment_type, AttachmentType::Text))
            .filter_map(|a| {
                Some(format!(
                    r#"<fileattachment name="{}">{}</fileattachment>"#,
                    a.attachment_url.clone().unwrap(),
                    a.attachment_content.clone().unwrap().as_str()
                ))
            })
            .collect();
        let context = text_attachments.join("\n");

        let display_user_prompt_with_context =
            build_prompt_with_attachment_context(&display_user_prompt, &context);
        let runtime_user_prompt_with_context =
            build_prompt_with_attachment_context(&runtime_user_prompt, &context);
        // 添加用户消息
        let user_message = add_message(
            app_handle,
            None,
            conversation_id,
            "user".to_string(),
            display_user_prompt_with_context.clone(),
            Some(assistant_detail.model[0].id),
            Some(assistant_detail.model[0].model_code.clone()),
            None,
            None,
            0,
            None, // 用户消息不需要 generation_group_id
            None, // 用户消息不需要 parent_group_id
        )?;

        // 更新 attachment 的 message_id，关联到新创建的用户消息
        // 这确保后续查询时能正确获取 attachment（通过 LEFT JOIN message.id = ma.message_id）
        for attachment in message_attachment_list.iter() {
            let mut updated_attachment = attachment.clone();
            updated_attachment.message_id = user_message.id;
            db.attachment_repo().unwrap().update(&updated_attachment).map_err(AppError::from)?;
        }

        // 发送消息添加事件
        let add_event = ConversationEvent {
            r#type: "message_add".to_string(),
            data: serde_json::to_value(MessageAddEvent {
                message_id: user_message.id,
                message_type: "user".to_string(),
            })
            .unwrap(),
        };

        let _ =
            app_handle.emit(format!("conversation_event_{}", conversation_id).as_str(), add_event);

        let update_event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id: user_message.id,
                message_type: "user".to_string(),
                content: display_user_prompt_with_context.clone(),
                is_done: false,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            })
            .unwrap(),
        };
        let _ = app_handle
            .emit(format!("conversation_event_{}", conversation_id).as_str(), update_event);

        let mut updated_message_list = message_list;
        if !has_system_message {
            updated_message_list.insert(0, (String::from("system"), system_prompt, vec![]));
            // system 消息不来自 DB，为其补充占位 id 和 token
            branch_message_ids.insert(0, 0);
            branch_db_token_counts.insert(0, 0);
        }
        updated_message_list.push((
            String::from("user"),
            runtime_user_prompt_with_context.clone(),
            message_attachment_list,
        ));
        // 新的 user 消息还没有 DB token count
        branch_message_ids.push(user_message.id);
        branch_db_token_counts.push(0);

        (
            conversation_id,
            None, // 不预先创建空的assistant消息，让流式处理动态创建
            user_message.id,
            runtime_user_prompt_with_context,
            updated_message_list,
            branch_message_ids,
            branch_db_token_counts,
        )
    };
    Ok((
        conversation_id,
        add_message_id,
        user_message_id,
        request_prompt_result_with_context,
        init_message_list,
        message_ids,
        db_token_counts,
    ))
}

/// 获取指定对话的当前活动焦点状态（用于前端闪亮边框同步）
#[tauri::command]
pub async fn get_activity_focus(
    activity_manager: State<'_, ConversationActivityManager>,
    conversation_id: i64,
) -> Result<ActivityFocus, String> {
    Ok(activity_manager.get_focus(conversation_id).await)
}

/// 获取指定对话的当前闪亮状态快照（用于前端单一状态源同步）
#[tauri::command]
pub async fn get_shine_state(
    activity_manager: State<'_, ConversationActivityManager>,
    conversation_id: i64,
) -> Result<ConversationShineState, String> {
    Ok(activity_manager.get_shine_state(conversation_id).await)
}

/// 获取指定对话的运行状态快照（用于发送按钮运行态判断）
#[tauri::command]
pub async fn get_conversation_runtime_state(
    activity_manager: State<'_, ConversationActivityManager>,
    conversation_id: i64,
) -> Result<ConversationRuntimeState, String> {
    Ok(activity_manager.get_runtime_state(conversation_id).await)
}

/// 列出当前正在运行的对话 ID（供侧边栏列表初始同步）
#[tauri::command]
pub async fn list_running_conversation_ids(
    activity_manager: State<'_, ConversationActivityManager>,
) -> Result<Vec<i64>, String> {
    Ok(activity_manager.list_running_conversation_ids().await)
}

/// 重新生成对话标题
#[tauri::command]
pub async fn regenerate_conversation_title(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    feature_config_state: State<'_, FeatureConfigState>,
    conversation_id: i64,
) -> Result<(), AppError> {
    let conversation_db = ConversationDatabase::new(&app_handle).map_err(|e| {
    tracing::error!(error = %e, "failed to create conversation_db in tool_result_continue_ask_ai");
        AppError::from(e)
    })?;

    // 获取对话的消息
    let messages =
        conversation_db.message_repo().unwrap().list_by_conversation_id(conversation_id)?;

    if messages.is_empty() {
        return Err(AppError::InsufficientMessages);
    }

    // 获取第一条用户消息（必须有）
    let user_message = messages
        .iter()
        .find(|(msg, _)| msg.message_type == "user")
        .map(|(msg, _)| msg)
        .ok_or_else(|| AppError::InsufficientMessages)?;

    // 获取第一条AI回答（可选）
    let response_message =
        messages.iter().find(|(msg, _)| msg.message_type == "response").map(|(msg, _)| msg);

    // 获取特性配置
    let config_feature_map = feature_config_state.config_feature_map.lock().await;

    // 调用内部的 generate_title 函数
    let response_content = response_message.map(|msg| msg.content.clone()).unwrap_or_default(); // 如果没有回答，使用空字符串

    generate_title(
        &app_handle,
        conversation_id,
        user_message.content.clone(),
        response_content,
        config_feature_map.clone(),
        window,
    )
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_hook_messages, apply_openai_responses_stateful_request_options,
        collect_openai_responses_instructions, maybe_select_openai_responses_stateful_messages,
        parse_agent_model_override, prepare_openai_responses_request_messages,
        resolve_tool_name, ToolNameMapping,
    };
    use crate::db::conversation_db::{AttachmentType, Message, MessageAttachment};
    use crate::db::system_db::FeatureConfig;
    use std::collections::HashMap;

    fn sample_attachment(hash: &str) -> MessageAttachment {
        MessageAttachment {
            id: 0,
            message_id: 0,
            attachment_type: AttachmentType::Text,
            attachment_url: None,
            attachment_content: Some("attachment".to_string()),
            attachment_hash: Some(hash.to_string()),
            use_vector: false,
            token_count: None,
        }
    }

    fn enabled_stateful_feature_config() -> HashMap<String, HashMap<String, FeatureConfig>> {
        let mut network_config = HashMap::new();
        network_config.insert(
            "openai_responses_stateful_enabled".to_string(),
            FeatureConfig {
                id: None,
                feature_code: "network_config".to_string(),
                key: "openai_responses_stateful_enabled".to_string(),
                value: "true".to_string(),
                data_type: "boolean".to_string(),
                description: None,
            },
        );
        network_config.insert(
            "openai_prompt_cache_key_enabled".to_string(),
            FeatureConfig {
                id: None,
                feature_code: "network_config".to_string(),
                key: "openai_prompt_cache_key_enabled".to_string(),
                value: "false".to_string(),
                data_type: "boolean".to_string(),
                description: None,
            },
        );
        HashMap::from([("network_config".to_string(), network_config)])
    }

    fn enabled_stateful_and_prompt_cache_feature_config() -> HashMap<String, HashMap<String, FeatureConfig>> {
        let mut config = enabled_stateful_feature_config();
        if let Some(network_config) = config.get_mut("network_config") {
            if let Some(prompt_cache_config) =
                network_config.get_mut("openai_prompt_cache_key_enabled")
            {
                prompt_cache_config.value = "true".to_string();
            }
        }
        config
    }

    fn api_test_message(id: i64, message_type: &str, metadata_json: Option<&str>) -> Message {
        Message {
            id,
            parent_id: None,
            conversation_id: 1,
            message_type: message_type.to_string(),
            content: format!("{message_type}-{id}"),
            llm_model_id: Some(1),
            llm_model_name: Some("gpt-test".to_string()),
            created_time: chrono::Utc::now(),
            start_time: Some(chrono::Utc::now()),
            finish_time: Some(chrono::Utc::now()),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: None,
            parent_group_id: None,
            tool_calls_json: None,
            metadata_json: metadata_json.map(ToOwned::to_owned),
            first_token_time: None,
            ttft_ms: None,
        }
    }

    #[test]
    fn agent_model_override_resolves_model_and_provider() {
        assert_eq!(
            parse_agent_model_override("deepseek-v4-flash%%54"),
            Some(("deepseek-v4-flash", 54))
        );
        assert_eq!(parse_agent_model_override("deepseek-v4-flash"), None);
        assert_eq!(parse_agent_model_override("model%%0"), None);
    }

    #[test]
    fn legacy_acp_claude_cli_provider_uses_stream_json_transport() {
        let config = crate::db::llm_db::LLMProviderConfig {
            id: 54,
            name: "acp_cli_command".into(),
            llm_provider_id: 54,
            value: "C:\\Users\\admin\\.local\\bin\\claude.exe".into(),
            append_location: "".into(),
            is_addition: false,
        };
        assert!(super::is_claude_code_provider("acp", &[config]));
    }

    #[test]
    fn regular_acp_provider_keeps_acp_transport() {
        let config = crate::db::llm_db::LLMProviderConfig {
            id: 1,
            name: "acp_cli_command".into(),
            llm_provider_id: 1,
            value: "my-agent".into(),
            append_location: "".into(),
            is_addition: false,
        };
        assert!(!super::is_claude_code_provider("acp", &[config]));
        assert!(super::is_claude_code_provider("anthropic", &[]));
    }

    #[test]
    fn test_stateful_bootstrap_stores_full_history_without_previous_response_id() {
        let messages = vec![("user".to_string(), "hello".to_string(), Vec::new())];
        let feature_config = enabled_stateful_feature_config();

        let (selected_messages, previous_response_id, store_response, instructions) =
            maybe_select_openai_responses_stateful_messages(
                &feature_config,
                "openai_api",
                "responses",
                42,
                &messages,
                &[1],
                &[],
                false,
                false,
                false,
            );

        assert_eq!(selected_messages.len(), 1);
        assert_eq!(selected_messages[0].0, "user");
        assert_eq!(selected_messages[0].1, "hello");
        assert_eq!(previous_response_id, None);
        assert!(store_response);
        assert_eq!(instructions, None);
    }

    #[test]
    fn test_apply_stateful_options_sets_previous_response_id_and_store() {
        let request = genai::chat::ChatRequest::from_user("again");
        let request = apply_openai_responses_stateful_request_options(
            request,
            Some("resp_123".to_string()),
            true,
            Some("system stays current".to_string()),
        );

        assert_eq!(request.previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(request.store, Some(true));
        assert_eq!(request.system.as_deref(), Some("system stays current"));
    }

    #[test]
    fn test_stateful_continuation_carries_system_as_instructions() {
        let messages = vec![
            ("system".to_string(), "follow this system prompt".to_string(), Vec::new()),
            ("user".to_string(), "hello".to_string(), Vec::new()),
        ];

        assert_eq!(
            collect_openai_responses_instructions(&messages).as_deref(),
            Some("follow this system prompt")
        );
    }

    #[test]
    fn test_stateful_continuation_selects_incremental_input_and_keeps_system_instructions() {
        let messages = vec![
            ("system".to_string(), "follow this system prompt".to_string(), Vec::new()),
            ("user".to_string(), "hello".to_string(), Vec::new()),
            ("response".to_string(), "hi".to_string(), Vec::new()),
            ("user".to_string(), "again".to_string(), Vec::new()),
        ];
        let all_messages = vec![
            (api_test_message(1, "system", None), None),
            (api_test_message(2, "user", None), None),
            (
                api_test_message(3, "response", Some(r#"{"response_id":"resp_1"}"#)),
                None,
            ),
            (api_test_message(4, "user", None), None),
        ];
        let feature_config = enabled_stateful_feature_config();

        let (selected_messages, previous_response_id, store_response, instructions) =
            maybe_select_openai_responses_stateful_messages(
                &feature_config,
                "openai_api",
                "responses",
                42,
                &messages,
                &[1, 2, 3, 4],
                &all_messages,
                false,
                false,
                false,
            );

        assert_eq!(selected_messages.len(), 1);
        assert_eq!(selected_messages[0].0, "user");
        assert_eq!(selected_messages[0].1, "again");
        assert_eq!(previous_response_id.as_deref(), Some("resp_1"));
        assert!(store_response);
        assert_eq!(
            instructions.as_deref(),
            Some("follow this system prompt")
        );
    }

    #[test]
    fn test_stateful_continuation_uses_full_history_when_native_tools_are_available() {
        let messages = vec![
            ("system".to_string(), "follow this system prompt".to_string(), Vec::new()),
            ("user".to_string(), "hello".to_string(), Vec::new()),
            ("response".to_string(), "hi".to_string(), Vec::new()),
            ("tool_result".to_string(), "loaded tool catalog".to_string(), Vec::new()),
            ("response".to_string(), "tool ready".to_string(), Vec::new()),
            ("user".to_string(), "again".to_string(), Vec::new()),
        ];
        let all_messages = vec![
            (api_test_message(1, "system", None), None),
            (api_test_message(2, "user", None), None),
            (
                api_test_message(3, "response", Some(r#"{"response_id":"resp_1"}"#)),
                None,
            ),
            (api_test_message(4, "tool_result", None), None),
            (
                api_test_message(5, "response", Some(r#"{"response_id":"resp_2"}"#)),
                None,
            ),
            (api_test_message(6, "user", None), None),
        ];
        let feature_config = enabled_stateful_feature_config();

        let (selected_messages, previous_response_id, store_response, instructions) =
            maybe_select_openai_responses_stateful_messages(
                &feature_config,
                "openai_api",
                "responses",
                42,
                &messages,
                &[1, 2, 3, 4, 5, 6],
                &all_messages,
                false,
                false,
                true,
            );

        let selected_roles_and_content = selected_messages
            .iter()
            .map(|(role, content, _)| (role.as_str(), content.as_str()))
            .collect::<Vec<_>>();
        let expected_roles_and_content = messages
            .iter()
            .map(|(role, content, _)| (role.as_str(), content.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(selected_roles_and_content, expected_roles_and_content);
        assert_eq!(previous_response_id, None);
        assert!(store_response);
        assert_eq!(instructions, None);
    }

    #[test]
    fn test_prompt_cache_takes_priority_over_stateful_continuation() {
        let messages = vec![
            ("system".to_string(), "stable system prompt".to_string(), Vec::new()),
            ("user".to_string(), "hello".to_string(), Vec::new()),
            ("response".to_string(), "hi".to_string(), Vec::new()),
            ("user".to_string(), "again".to_string(), Vec::new()),
        ];
        let all_messages = vec![
            (api_test_message(1, "system", None), None),
            (api_test_message(2, "user", None), None),
            (
                api_test_message(3, "response", Some(r#"{"response_id":"resp_1"}"#)),
                None,
            ),
            (api_test_message(4, "user", None), None),
        ];
        let feature_config = enabled_stateful_and_prompt_cache_feature_config();

        let (selected_messages, previous_response_id, store_response, instructions) =
            maybe_select_openai_responses_stateful_messages(
                &feature_config,
                "openai_api",
                "responses",
                42,
                &messages,
                &[1, 2, 3, 4],
                &all_messages,
                false,
                false,
                false,
            );

        assert_eq!(selected_messages.len(), messages.len());
        assert_eq!(selected_messages[0].0, "system");
        assert_eq!(selected_messages[3].1, "again");
        assert_eq!(previous_response_id, None);
        assert!(!store_response);
        assert_eq!(instructions, None);
    }

    #[test]
    fn test_openai_responses_full_history_promotes_system_to_instructions() {
        let messages = vec![
            ("system".to_string(), "stable system".to_string(), Vec::new()),
            ("user".to_string(), "hello".to_string(), Vec::new()),
        ];

        let (request_messages, instructions) = prepare_openai_responses_request_messages(
            "openai_api",
            "responses",
            messages,
            None,
        );

        assert_eq!(instructions.as_deref(), Some("stable system"));
        assert_eq!(request_messages.len(), 1);
        assert_eq!(request_messages[0].0, "user");
        assert_eq!(request_messages[0].1, "hello");
    }

    #[test]
    #[ignore = "temporary local probe: reads real AIPP conversation/mcp databases"]
    fn probe_openai_responses_prompt_cache_prefix_stability_for_latest_conversation() {
        use crate::api::ai::conversation::{build_message_list_with_metadata_from_db, BranchSelection};
        use crate::utils::db_utils::{get_datetime_from_row, get_required_datetime_from_row};
        use rusqlite::{Connection, OpenFlags};
        use sha2::{Digest, Sha256};
        use std::env;
        use std::fs;
        use std::path::PathBuf;

        fn app_db_dir() -> PathBuf {
            env::var("AIPP_CACHE_PREFIX_PROBE_DB_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    let home = env::var("HOME").expect("HOME is required on macOS");
                    PathBuf::from(home)
                        .join("Library/Application Support/com.xieisabug.aipp/db")
                })
        }

        fn snapshot_db_dir(source_db_dir: PathBuf) -> PathBuf {
            let snapshot_dir = env::temp_dir()
                .join(format!("aipp-cache-prefix-probe-{}", std::process::id()));
            fs::create_dir_all(&snapshot_dir).expect("create probe db snapshot dir");
            for db_name in ["conversation.db", "mcp.db"] {
                let source = source_db_dir.join(db_name);
                let target = snapshot_dir.join(db_name);
                fs::copy(&source, &target).unwrap_or_else(|err| {
                    panic!(
                        "copy probe db snapshot from {} to {}: {err}",
                        source.display(),
                        target.display()
                    )
                });
            }
            snapshot_dir
        }

        fn open_readonly(path: PathBuf) -> Connection {
            assert!(
                path.exists(),
                "probe sqlite db does not exist: {}",
                path.display()
            );
            Connection::open_with_flags(
                &path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .unwrap_or_else(|err| panic!("open readonly sqlite db {}: {err}", path.display()))
        }

        fn short_hash(value: &serde_json::Value) -> String {
            let bytes = serde_json::to_vec(value).expect("serialize value for hash");
            hex::encode(&Sha256::digest(bytes)[..8])
        }

        fn latest_conversation_id(conn: &Connection) -> i64 {
            conn.query_row(
                "select id from conversation order by datetime(created_time) desc limit 1",
                [],
                |row| row.get(0),
            )
            .expect("read latest conversation id")
        }

        fn load_messages(
            conn: &Connection,
            conversation_id: i64,
        ) -> Vec<(Message, Option<MessageAttachment>)> {
            let mut stmt = conn
                .prepare(
                    "select id,parent_id,conversation_id,message_type,content,llm_model_id,\
                            llm_model_name,created_time,start_time,finish_time,token_count,\
                            input_token_count,output_token_count,generation_group_id,parent_group_id,\
                            tool_calls_json,metadata_json,first_token_time,ttft_ms \
                     from message where conversation_id = ? order by id",
                )
                .expect("prepare message query");
            stmt.query_map([conversation_id], |row| {
                Ok((
                    Message {
                        id: row.get(0)?,
                        parent_id: row.get(1)?,
                        conversation_id: row.get(2)?,
                        message_type: row.get(3)?,
                        content: row.get(4)?,
                        llm_model_id: row.get(5)?,
                        llm_model_name: row.get(6)?,
                        created_time: get_required_datetime_from_row(row, 7, "created_time")?,
                        start_time: get_datetime_from_row(row, 8)?,
                        finish_time: get_datetime_from_row(row, 9)?,
                        token_count: row.get(10)?,
                        input_token_count: row.get(11)?,
                        output_token_count: row.get(12)?,
                        generation_group_id: row.get(13)?,
                        parent_group_id: row.get(14)?,
                        tool_calls_json: row.get(15)?,
                        metadata_json: row.get(16)?,
                        first_token_time: get_datetime_from_row(row, 17)?,
                        ttft_ms: row.get(18)?,
                    },
                    None,
                ))
            })
            .expect("query messages")
            .collect::<Result<Vec<_>, _>>()
            .expect("read messages")
        }

        fn load_tool_signature(db_dir: PathBuf, conversation_id: i64) -> serde_json::Value {
            let conn = open_readonly(db_dir.join("mcp.db"));
            let mut stmt = conn
                .prepare(
                    "select s.name, t.tool_name, coalesce(t.tool_description, ''), coalesce(t.parameters, '') \
                     from mcp_server_tool t \
                     join mcp_server s on s.id = t.server_id \
                     where t.is_enabled = 1 and s.is_enabled = 1 and ( \
                         (s.command = 'aipp:agent' and t.tool_name in ('load_mcp_server', 'load_mcp_tool')) \
                         or t.id in ( \
                             select tool_id from conversation_mcp_loaded_tool \
                             where conversation_id = ? and status = 'valid' \
                         ) \
                     ) \
                     order by s.name, t.tool_name",
                )
                .expect("prepare tool signature query");
            let tools = stmt
                .query_map([conversation_id], |row| {
                    Ok(serde_json::json!({
                        "server": row.get::<_, String>(0)?,
                        "name": row.get::<_, String>(1)?,
                        "description": row.get::<_, String>(2)?,
                        "parameters": serde_json::from_str::<serde_json::Value>(
                            &row.get::<_, String>(3)?
                        ).unwrap_or_else(|_| serde_json::json!({ "type": "object" })),
                    }))
                })
                .expect("query tool signature")
                .collect::<Result<Vec<_>, _>>()
                .expect("read tool signature");
            serde_json::json!(tools)
        }

        fn request_payload_shape(
            messages: &[(Message, Option<MessageAttachment>)],
            tools: &serde_json::Value,
        ) -> (serde_json::Value, Vec<serde_json::Value>) {
            let (message_list, _) =
                build_message_list_with_metadata_from_db(messages, BranchSelection::LatestBranch);
            let (input_messages, instructions) = prepare_openai_responses_request_messages(
                "openai_api",
                "responses",
                message_list,
                None,
            );
            let instructions = serde_json::json!(instructions.unwrap_or_default());
            let input = input_messages
                .iter()
                .map(|(role, content, _)| {
                    serde_json::json!({
                        "role": role,
                        "content": content,
                    })
                })
                .collect::<Vec<_>>();
            (
                serde_json::json!({
                    "instructions": instructions,
                    "tools": tools,
                    "input": input,
                }),
                input,
            )
        }

        fn append_synthetic_message(
            messages: &mut Vec<(Message, Option<MessageAttachment>)>,
            conversation_id: i64,
            message_type: &str,
            content: String,
        ) {
            let now = chrono::Utc::now();
            let next_id = messages.iter().map(|(message, _)| message.id).max().unwrap_or(0) + 1;
            messages.push((
                Message {
                    id: next_id,
                    parent_id: None,
                    conversation_id,
                    message_type: message_type.to_string(),
                    content,
                    llm_model_id: None,
                    llm_model_name: if message_type == "response" {
                        Some("gpt-5.5".to_string())
                    } else {
                        None
                    },
                    created_time: now,
                    start_time: Some(now),
                    finish_time: Some(now),
                    token_count: 0,
                    input_token_count: 0,
                    output_token_count: 0,
                    generation_group_id: None,
                    parent_group_id: None,
                    tool_calls_json: None,
                    metadata_json: None,
                    first_token_time: None,
                    ttft_ms: None,
                },
                None,
            ));
        }

        let source_db_dir = app_db_dir();
        let db_dir = snapshot_db_dir(source_db_dir.clone());
        println!(
            "cache-prefix-probe source_db_dir={} snapshot_db_dir={}",
            source_db_dir.display(),
            db_dir.display()
        );
        let conversation_conn = open_readonly(db_dir.join("conversation.db"));
        let conversation_id = env::var("AIPP_CACHE_PREFIX_PROBE_CONVERSATION_ID")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_else(|| latest_conversation_id(&conversation_conn));
        let mut messages = load_messages(&conversation_conn, conversation_id);
        let tools = load_tool_signature(db_dir, conversation_id);
        let tools_hash = short_hash(&tools);
        let mut previous_input: Option<Vec<serde_json::Value>> = None;

        println!(
            "cache-prefix-probe conversation_id={conversation_id} initial_messages={} tools={} tools_hash={tools_hash}",
            messages.len(),
            tools.as_array().map(|tools| tools.len()).unwrap_or(0)
        );

        for round in 1..=4 {
            append_synthetic_message(
                &mut messages,
                conversation_id,
                "user",
                format!("synthetic user follow-up round {round}"),
            );

            let (payload, input) = request_payload_shape(&messages, &tools);
            let input_hash = short_hash(&serde_json::json!(input));
            let instructions_hash = short_hash(payload.get("instructions").unwrap());
            let previous_is_prefix = previous_input
                .as_ref()
                .map(|previous| input.starts_with(previous))
                .unwrap_or(true);
            let last_role = input
                .last()
                .and_then(|value| value.get("role"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let system_in_input = input
                .iter()
                .any(|value| value.get("role").and_then(|role| role.as_str()) == Some("system"));
            println!(
                "round={round} instructions_hash={instructions_hash} tools_hash={tools_hash} input_items={} input_hash={input_hash} previous_input_is_prefix={previous_is_prefix} last_role={last_role} system_in_input={system_in_input}",
                input.len()
            );
            assert!(previous_is_prefix, "previous request input must remain next request prefix");
            assert_eq!(last_role, "user", "new user message should be at the end");
            assert!(!system_in_input, "Responses system prompt should stay in instructions");

            previous_input = Some(input);
            append_synthetic_message(
                &mut messages,
                conversation_id,
                "response",
                format!("synthetic assistant response round {round}"),
            );
        }
    }

    #[test]
    fn test_non_responses_request_keeps_system_in_message_list() {
        let messages = vec![
            ("system".to_string(), "stable system".to_string(), Vec::new()),
            ("user".to_string(), "hello".to_string(), Vec::new()),
        ];

        let (request_messages, instructions) = prepare_openai_responses_request_messages(
            "openai_api",
            "chat",
            messages,
            None,
        );

        assert_eq!(instructions, None);
        assert_eq!(request_messages.len(), 2);
        assert_eq!(request_messages[0].0, "system");
    }

    #[test]
    fn test_apply_hook_messages_updates_existing_message_and_preserves_attachments() {
        let messages = vec![(
            "user".to_string(),
            "original".to_string(),
            vec![sample_attachment("keep-me")],
        )];
        let hook_context = serde_json::json!({
            "messages": [
                {
                    "index": 0,
                    "messageType": "user",
                    "content": "patched"
                }
            ]
        });

        let updated = apply_hook_messages(messages, &hook_context);

        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, "user");
        assert_eq!(updated[0].1, "patched");
        assert_eq!(updated[0].2.len(), 1);
        assert_eq!(updated[0].2[0].attachment_hash.as_deref(), Some("keep-me"));
    }

    #[test]
    fn test_apply_hook_messages_can_insert_hidden_message_after_existing_message() {
        let messages = vec![
            (
                "user".to_string(),
                "question".to_string(),
                vec![sample_attachment("first")],
            ),
            ("assistant".to_string(), "answer".to_string(), Vec::new()),
        ];
        let hook_context = serde_json::json!({
            "messages": [
                {
                    "index": 0
                },
                {
                    "messageType": "system",
                    "content": "<plugin_hidden_context>secret</plugin_hidden_context>"
                },
                {
                    "index": 1
                }
            ]
        });

        let updated = apply_hook_messages(messages, &hook_context);

        assert_eq!(updated.len(), 3);
        assert_eq!(updated[0].0, "user");
        assert_eq!(updated[1].0, "system");
        assert_eq!(updated[1].1, "<plugin_hidden_context>secret</plugin_hidden_context>");
        assert!(updated[1].2.is_empty());
        assert_eq!(updated[2].0, "assistant");
        assert_eq!(updated[0].2[0].attachment_hash.as_deref(), Some("first"));
    }

    #[test]
    fn test_resolve_tool_name_recovers_unique_bare_tool_name_from_mapping() {
        let mut mapping = ToolNameMapping::new();
        mapping.insert(
            "operation__list_directory".to_string(),
            ("operation".to_string(), "list_directory".to_string()),
        );

        let resolved = resolve_tool_name("list_directory", &mapping);

        assert_eq!(
            resolved,
            ("operation".to_string(), "list_directory".to_string())
        );
    }

    #[test]
    fn test_resolve_tool_name_recovers_unique_sanitized_bare_tool_name_from_mapping() {
        let mut mapping = ToolNameMapping::new();
        mapping.insert(
            "server__list_directory".to_string(),
            ("文件工具".to_string(), "list directory".to_string()),
        );

        let resolved = resolve_tool_name("list_directory", &mapping);

        assert_eq!(
            resolved,
            ("文件工具".to_string(), "list directory".to_string())
        );
    }

    #[test]
    fn test_resolve_tool_name_keeps_default_fallback_for_ambiguous_bare_tool_name() {
        let mut mapping = ToolNameMapping::new();
        mapping.insert(
            "operation__list_directory".to_string(),
            ("operation".to_string(), "list_directory".to_string()),
        );
        mapping.insert(
            "workspace__list_directory".to_string(),
            ("workspace".to_string(), "list_directory".to_string()),
        );

        let resolved = resolve_tool_name("list_directory", &mapping);

        assert_eq!(
            resolved,
            ("default".to_string(), "list_directory".to_string())
        );
    }
}

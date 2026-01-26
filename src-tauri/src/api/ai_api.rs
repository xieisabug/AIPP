use super::assistant_api::AssistantDetail;
use crate::api::ai::acp::{extract_acp_config, spawn_acp_session_task};
use crate::api::ai::chat::{
    extract_assistant_from_message, handle_non_stream_chat as ai_handle_non_stream_chat,
    handle_stream_chat as ai_handle_stream_chat,
};
use crate::api::ai::config::{
    get_network_proxy_from_config, get_request_timeout_from_config, ChatConfig, ConfigBuilder,
};
use crate::api::ai::conversation::{
    build_chat_request_from_messages, build_message_list_from_db, init_conversation,
    BranchSelection, ChatRequestBuildResult, ToolCallStrategy, ToolConfig,
};
use crate::api::ai::events::{ActivityFocus, ConversationEvent, MessageAddEvent, MessageUpdateEvent};
use crate::api::ai::title::generate_title;
use crate::api::ai::types::{AiRequest, AiResponse, McpOverrideConfig};
use crate::api::assistant_api::{get_assistant, get_assistants};

use crate::api::genai_client;
use crate::db::conversation_db::{AttachmentType, Repository};
use crate::db::conversation_db::{ConversationDatabase, Message, MessageAttachment};
use crate::db::llm_db::LLMDatabase;
use crate::errors::AppError;
use crate::mcp::execution_api::cancel_mcp_tool_calls_by_conversation;
use crate::mcp::{collect_mcp_info_for_assistant, format_mcp_prompt};
use crate::skills::{collect_skills_info_for_assistant, format_skills_prompt};
use crate::state::activity_state::ConversationActivityManager;
use crate::state::message_token::MessageTokenManager;
use crate::template_engine::TemplateEngine;
use crate::utils::window_utils::send_conversation_event_to_chat_windows;
use crate::{AcpSessionState, AppState, FeatureConfigState};
use anyhow::Context;
use std::collections::HashMap;
use genai::chat::Tool;
use tauri::Emitter;
use tauri::State;
use tracing::{debug, error, info, instrument, warn};

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
    } else {
        // 回退：从 sanitized 名称中分割
        split_tool_name(sanitized_full_name)
    }
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
    mcp_info: &crate::mcp::MCPInfoForAssistant,
    enable_tools: bool,
) -> Option<ToolConfig> {
    if !enable_tools {
        return None;
    }
    let (tools, tool_name_mapping) = build_tools_with_mapping(&mcp_info.enabled_servers);
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
) -> Result<AiResponse, AppError> {
    info!("Ask AI start");
    debug!(
        ?request,
        ?override_model_config,
        ?override_prompt,
        ?override_mcp_config,
        "ask_ai input parameters"
    );

    let assistants = get_assistants(app_handle.clone())
        .map_err(|e| AppError::UnknownError(format!("Failed to get assistants: {}", e)))?;

    // 处理 @assistant_name 提取和消息清理
    let (actual_assistant_id, cleaned_prompt) =
        extract_assistant_from_message(&assistants, &request.prompt, request.assistant_id).await?;

    debug!(?actual_assistant_id, ?cleaned_prompt, "assistant extraction result");

    // 创建一个新的请求对象，使用处理后的数据
    let mut processed_request = request.clone();
    processed_request.assistant_id = actual_assistant_id;
    processed_request.prompt = cleaned_prompt;

    let template_engine = TemplateEngine::new();
    let mut template_context = HashMap::new();

    let selected_text = state.inner().selected_text.lock().await.clone();
    template_context.insert("selected_text".to_string(), selected_text);

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

    // 收集 MCP 信息
    let mcp_info = collect_mcp_info_for_assistant(
        &app_handle,
        processed_request.assistant_id,
        override_mcp_config.as_ref(),
        None,
    )
    .await?;
    info!(
        enabled_servers = mcp_info.enabled_servers.len(),
        native_toolcall = mcp_info.use_native_toolcall,
        "MCP configuration"
    );
    let is_native_toolcall = mcp_info.use_native_toolcall;

    // 注意：native toolcall 不改写 prompt，仅非原生时拼接 XML 约束
    let assistant_prompt_result =
        if mcp_info.enabled_servers.len() > 0 && !mcp_info.use_native_toolcall {
            let prompt = format_mcp_prompt(assistant_prompt_result, &mcp_info).await;
            debug!(formatted_prompt = prompt.as_str(), "MCP formatted prompt");
            prompt
        } else {
            assistant_prompt_result
        };

    // Collect and format Skills prompt
    let skills_info =
        collect_skills_info_for_assistant(&app_handle, processed_request.assistant_id).await?;
    let assistant_prompt_result = if !skills_info.enabled_skills.is_empty() {
        let prompt = format_skills_prompt(&app_handle, assistant_prompt_result, &skills_info).await;
        info!(enabled_skills = skills_info.enabled_skills.len(), "Skills formatted into prompt");
        debug!(formatted_prompt = prompt.as_str(), "Skills formatted prompt");
        prompt
    } else {
        assistant_prompt_result
    };

    let _need_generate_title = processed_request.conversation_id.is_empty();
    let request_prompt_result =
        template_engine.parse(&processed_request.prompt, &template_context).await;

    let app_handle_clone = app_handle.clone();
    let (conversation_id, _new_message_id, user_message_id, request_prompt_result_with_context, init_message_list) =
        initialize_conversation(
            &app_handle_clone,
            &processed_request,
            &assistant_detail,
            assistant_prompt_result,
            request_prompt_result.clone(),
            override_prompt.clone(),
        )
        .await?;

    // 设置用户消息的活动状态（闪亮边框）
    activity_manager
        .set_user_pending(&app_handle, conversation_id, user_message_id)
        .await;

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
        let provider_configs = if let Some(model) = assistant_detail.model.first() {
            let provider_id = model.provider_id;
            debug!("ACP: Getting provider config for provider_id={}", provider_id);
            
            let llm_db = LLMDatabase::new(&app_handle).map_err(|e| {
                AppError::UnknownError(format!("Failed to open LLM database: {}", e))
            })?;
            
            llm_db.get_llm_provider_config(provider_id).unwrap_or_else(|e| {
                warn!("ACP: Failed to get provider config: {}", e);
                Vec::new()
            })
        } else {
            debug!("ACP: No model found, using empty provider configs");
            Vec::new()
        };
        
        debug!("ACP: Loaded {} provider configs", provider_configs.len());

        // 从 assistant_model_configs 和 llm_provider_configs 提取 ACP 配置
        let acp_config = extract_acp_config(&assistant_detail.model_configs, &provider_configs)?;
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
            String::new(), // 初始为空，通过流式更新
            Some(0), // ACP 使用占位 model_id = 0
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

        // Clone prompt before moving into session dispatcher
        let prompt_clone = processed_request.prompt.clone();

        let session_handle = {
            let mut sessions = acp_session_state.sessions.lock().await;
            if let Some(handle) = sessions.get(&conversation_id) {
                handle.clone()
            } else {
                let (handle, join_handle) =
                    spawn_acp_session_task(app_handle_clone, conversation_id, acp_config);
                sessions.insert(conversation_id, handle.clone());
                message_token_manager
                    .store_task_handle(conversation_id, join_handle)
                    .await;
                handle
            }
        };

        if let Err(e) = session_handle.send_prompt(
            response_message.id,
            prompt_clone,
            window_clone,
        ) {
            error!(error = %e, "ACP session send prompt failed");
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
    let model_code = model_detail.model.code.clone(); // 提前获取模型代码
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

        let client = genai_client::create_client_with_config(
            &model_configs,
            &model_code,
            &provider_api_type,
            network_proxy.as_deref(),
            proxy_enabled,
            Some(request_timeout),
        )?;

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

        let chat_options = ConfigBuilder::build_chat_options(&config_map);

        // 动态判断是否有可用的工具
        let has_available_tools = is_native_toolcall && !mcp_info.enabled_servers.is_empty();

        // 某些 OpenAI 兼容通道在使用 Gemini 模型时不会返回 usage（或返回 null），
        // 而 genai 的 OpenAI 适配器会尝试严格反序列化 usage，从而在日志中出现错误。
        // 为避免该无害错误噪音，这里对「provider_api_type=openai 且 model_code 含 gemini」的组合禁用 usage 捕获。
        let provider_api_type_lc = provider_api_type.to_lowercase();
        let model_code_lc = model_code.to_lowercase();
        let is_openai_like =
            provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
        let is_gemini = model_code_lc.contains("gemini");
        let capture_usage = !(is_openai_like && is_gemini);

        let chat_config = ChatConfig {
            model_name,
            stream,
            chat_options: chat_options
                .with_normalize_reasoning_content(true)
                .with_capture_usage(capture_usage)
                .with_capture_tool_calls(has_available_tools), // 动态设置
            client,
        };

        info!(
            model = chat_config.model_name,
            stream = chat_config.stream,
            has_tools = has_available_tools,
            provider_api_type = %provider_api_type,
            capture_usage = capture_usage,
            is_openai_like = is_openai_like,
            is_gemini = is_gemini,
            "chat configuration established"
        );

        let tool_call_strategy = if has_available_tools {
            ToolCallStrategy::Native
        } else {
            ToolCallStrategy::NonNative
        };
        let tool_config = build_tool_config(&mcp_info, has_available_tools);
        let ChatRequestBuildResult { chat_request, tool_name_mapping } =
            build_chat_request_from_messages(&init_message_list, tool_call_strategy, tool_config);

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

        Ok::<(), anyhow::Error>(())
    });

    // Store the task handle for proper cancellation
    message_token_manager.store_task_handle(conversation_id, task_handle).await;

    info!("Ask AI end");

    Ok(AiResponse { conversation_id, request_prompt_result_with_context })
}

#[instrument(skip(app_handle, window, tool_result), fields(conversation_id = %conversation_id, assistant_id, tool_call_id))]
pub(crate) async fn tool_result_continue_ask_ai_impl(
    app_handle: tauri::AppHandle,
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

    let tool_result_message = add_message(
        &app_handle,
        None,
        conversation_id_i64,
        "tool_result".to_string(),
        tool_result_content,
        Some(assistant_detail.model[0].id),
        Some(assistant_detail.model[0].model_code.clone()),
        Some(chrono::Utc::now()),
        Some(chrono::Utc::now()),
        0,
        None,
        None,
    )?;

    // Emit events so UI can render the tool_result immediately without manual refresh
    // 1) message_add
    let add_event = ConversationEvent {
        r#type: "message_add".to_string(),
        data: serde_json::to_value(MessageAddEvent {
            message_id: tool_result_message.id,
            message_type: "tool_result".to_string(),
        })
        .unwrap(),
    };
    let _ = window.emit(format!("conversation_event_{}", conversation_id_i64).as_str(), add_event);

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

    // Get all existing messages
    let all_messages = db.message_repo().unwrap().list_by_conversation_id(conversation_id_i64)?;


    // 使用 get_latest_branch_messages 获取最新分支的消息（正确过滤掉废弃分支）
    let latest_branch = crate::api::ai::summary::get_latest_branch_messages(&all_messages);
    debug!(
        total_messages = all_messages.len(),
        branch_messages = latest_branch.len(),
        "filtered messages to latest branch for tool_result_continue"
    );

    // 尝试复用上一次包含工具调用的 assistant 响应的 generation_group_id，
    // 这样 tooluse 的"请求消息(assistant)"与"分析消息(assistant)"会处于同一分组
    let reuse_generation_group_id: Option<String> = {
        // 找到刚插入的 tool_result 之前最近的一条 response 消息
        let current_tool_result_id = tool_result_message.id;
        latest_branch
            .iter()
            .filter(|msg| msg.id < current_tool_result_id && msg.message_type == "response")
            .max_by_key(|msg| msg.id)
            .and_then(|m| m.generation_group_id.clone())
    };

    let init_message_list =
        build_message_list_from_db(&all_messages, BranchSelection::LatestBranch);

    // 收集 MCP 信息
    let mcp_info = collect_mcp_info_for_assistant(&app_handle, assistant_id, None, None).await?;
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
    let model_code = model_detail.model.code.clone();
    let model_configs = model_detail.configs.clone();
    let provider_api_type = model_detail.provider.api_type.clone();
    let assistant_model_configs = assistant_detail.model_configs.clone();

    let conversation_db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    // Build chat configuration (same as ask_ai)
    let client = genai_client::create_client_with_config(
        &model_configs,
        &model_code,
        &provider_api_type,
        None,
        false,
        None,
    )
    .map_err(|e| {
        error!(error = %e, "failed to create client in tool_result_continue_ask_ai");
        e
    })?;

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

    let chat_options = ConfigBuilder::build_chat_options(&config_map);

    // 先计算强制降级条件
    let force_non_native_for_toolresult =
        provider_api_type == "openai" && model_code.to_lowercase().contains("gemini");

    // 动态判断是否有可用的工具（考虑强制降级的情况）
    let has_available_tools = is_native_toolcall
        && !mcp_info.enabled_servers.is_empty()
        && !force_non_native_for_toolresult;

    // 同 ask_ai：避免 OpenAI 兼容通道 + Gemini 模型导致的 usage 反序列化报错日志
    let provider_api_type_lc = provider_api_type.to_lowercase();
    let model_code_lc = model_code.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let is_gemini = model_code_lc.contains("gemini");
    let capture_usage = !(is_openai_like && is_gemini);

    let chat_config = ChatConfig {
        model_name,
        stream,
        chat_options: chat_options
            .with_normalize_reasoning_content(true)
            .with_capture_usage(capture_usage)
            .with_capture_tool_calls(has_available_tools), // 动态设置
        client,
    };

    info!(
        model = chat_config.model_name,
        stream = chat_config.stream,
        has_tools = has_available_tools,
        provider_api_type = %provider_api_type,
        capture_usage = capture_usage,
        is_openai_like = is_openai_like,
        is_gemini = is_gemini,
        "chat configuration (tool_result_continue)"
    );

    info!(
        model = chat_config.model_name,
        stream = chat_config.stream,
        has_tools = has_available_tools,
        "chat configuration (tool_result_continue)"
    );

    let tool_call_strategy = if has_available_tools {
        ToolCallStrategy::Native
    } else {
        ToolCallStrategy::NonNative
    };
    let tool_config = build_tool_config(&mcp_info, has_available_tools);
    let ChatRequestBuildResult { chat_request, tool_name_mapping } =
        build_chat_request_from_messages(&init_message_list, tool_call_strategy, tool_config);

    if chat_config.stream {
        Box::pin(ai_handle_stream_chat(
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
            None,                      // no MCP override config
            tool_name_mapping.clone(), // 工具名称映射表
        ))
        .await?;
    } else {
        Box::pin(ai_handle_non_stream_chat(
            &chat_config.client,
            &chat_config.model_name,
            &chat_request,
            &chat_config.chat_options,
            conversation_id_i64,
            &conversation_db,
            &window_clone,
            &app_handle,
            false,                     // no title generation needed
            String::new(),             // no user prompt
            HashMap::new(),            // no feature config needed
            reuse_generation_group_id, // 复用上一条assistant响应的generation_group_id
            None,                      // no parent_group_id
            model_id,
            model_code.clone(),
            None,              // no MCP override config
            tool_name_mapping, // 工具名称映射表
        ))
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
#[instrument(skip(app_handle, window), fields(conversation_id, assistant_id))]
pub(crate) async fn batch_tool_result_continue_ask_ai_impl(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    conversation_id: i64,
    assistant_id: i64,
) -> Result<AiResponse, AppError> {
    info!("Batch tool result continuation start");

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

    let init_message_list =
        build_message_list_from_db(&all_messages, BranchSelection::LatestBranch);

    // 收集 MCP 信息
    let mcp_info = collect_mcp_info_for_assistant(&app_handle, assistant_id, None, None).await?;
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
    let model_code = model_detail.model.code.clone();
    let model_configs = model_detail.configs.clone();
    let provider_api_type = model_detail.provider.api_type.clone();
    let assistant_model_configs = assistant_detail.model_configs.clone();

    let conversation_db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    // Build chat configuration
    let client = genai_client::create_client_with_config(
        &model_configs,
        &model_code,
        &provider_api_type,
        None,
        false,
        None,
    )
    .map_err(|e| {
        error!(error = %e, "failed to create client in batch_tool_result_continue_ask_ai");
        e
    })?;

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

    let chat_options = ConfigBuilder::build_chat_options(&config_map);

    // 先计算强制降级条件
    let force_non_native_for_toolresult =
        provider_api_type == "openai" && model_code.to_lowercase().contains("gemini");

    // 动态判断是否有可用的工具（考虑强制降级的情况）
    let has_available_tools = is_native_toolcall
        && !mcp_info.enabled_servers.is_empty()
        && !force_non_native_for_toolresult;

    let provider_api_type_lc = provider_api_type.to_lowercase();
    let model_code_lc = model_code.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let is_gemini = model_code_lc.contains("gemini");
    let capture_usage = !(is_openai_like && is_gemini);

    let chat_config = ChatConfig {
        model_name,
        stream,
        chat_options: chat_options
            .with_normalize_reasoning_content(true)
            .with_capture_usage(capture_usage)
            .with_capture_tool_calls(has_available_tools),
        client,
    };

    info!(
        model = chat_config.model_name,
        stream = chat_config.stream,
        has_tools = has_available_tools,
        provider_api_type = %provider_api_type,
        "chat configuration (batch_tool_result_continue)"
    );

    let tool_call_strategy = if has_available_tools {
        ToolCallStrategy::Native
    } else {
        ToolCallStrategy::NonNative
    };
    let tool_config = build_tool_config(&mcp_info, has_available_tools);
    let ChatRequestBuildResult { chat_request, tool_name_mapping } =
        build_chat_request_from_messages(&init_message_list, tool_call_strategy, tool_config);

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
            None,
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
            None,
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
#[instrument(skip(app_handle, _state, _feature_config_state, window, tool_result), fields(conversation_id = %conversation_id, assistant_id, tool_call_id))]
pub async fn tool_result_continue_ask_ai(
    app_handle: tauri::AppHandle,
    _state: State<'_, AppState>,
    _feature_config_state: State<'_, FeatureConfigState>,
    window: tauri::Window,
    conversation_id: String,
    assistant_id: i64,
    tool_call_id: String,
    tool_result: String,
) -> Result<AiResponse, AppError> {
    tool_result_continue_ask_ai_impl(
        app_handle,
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
    message_token_manager: State<'_, MessageTokenManager>,
    conversation_id: i64,
) -> Result<(), String> {
    message_token_manager.cancel_request(conversation_id).await;

    if let Err(e) = cancel_mcp_tool_calls_by_conversation(&app_handle, conversation_id).await {
        warn!(conversation_id, error = %e, "failed to cancel MCP tool calls for conversation");
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
        activity_manager
            .set_user_pending(&app_handle, conversation_id, message_id)
            .await;
    } else {
        activity_manager
            .set_assistant_streaming(&app_handle, conversation_id, message_id)
            .await;
    }

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

    let init_message_list =
        build_message_list_from_db(&filtered_messages, BranchSelection::LatestChildren);

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
    let regenerate_model_code = model_detail.model.code.clone(); // 提前获取模型代码
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

        let client = genai_client::create_client_with_config(
            &regenerate_model_configs,
            &regenerate_model_code,
            &regenerate_provider_api_type,
            network_proxy.as_deref(),
            proxy_enabled,
            Some(request_timeout),
        )?;

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

        let chat_options = ConfigBuilder::build_chat_options(&config_map);

        // 动态判断是否有可用的工具
        let has_available_tools = is_native_toolcall && !mcp_info.enabled_servers.is_empty();

        // 同 ask_ai：避免 OpenAI 兼容通道 + Gemini 模型导致的 usage 反序列化报错日志
        let provider_api_type_lc = regenerate_provider_api_type.to_lowercase();
        let model_code_lc = regenerate_model_code.to_lowercase();
        let is_openai_like =
            provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
        let is_gemini = model_code_lc.contains("gemini");
        let capture_usage = !(is_openai_like && is_gemini);

        let chat_config = ChatConfig {
            model_name,
            stream,
            chat_options: chat_options
                .with_normalize_reasoning_content(true)
                .with_capture_usage(capture_usage)
                .with_capture_tool_calls(has_available_tools), // 动态设置
            client,
        };

        info!(
            model = chat_config.model_name,
            stream = chat_config.stream,
            has_tools = has_available_tools,
            provider_api_type = %regenerate_provider_api_type,
            capture_usage = capture_usage,
            is_openai_like = is_openai_like,
            is_gemini = is_gemini,
            "chat configuration (regenerate)"
        );

        let tool_call_strategy = if has_available_tools {
            ToolCallStrategy::Native
        } else {
            ToolCallStrategy::NonNative
        };
        let tool_config = if has_available_tools {
            if let Ok(mcp_info) = crate::mcp::collect_mcp_info_for_assistant(
                &app_handle_clone,
                assistant_id,
                None,
                None,
            )
            .await
            {
                build_tool_config(&mcp_info, true)
            } else {
                None
            }
        } else {
            None
        };
        let ChatRequestBuildResult { chat_request, tool_name_mapping } =
            build_chat_request_from_messages(&init_message_list, tool_call_strategy, tool_config);

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
    request_prompt_result: String,
    override_prompt: Option<String>,
) -> Result<(i64, Option<i64>, i64, String, Vec<(String, String, Vec<MessageAttachment>)>), AppError> {
    // 返回值：(conversation_id, add_message_id, user_message_id, request_prompt_with_context, init_message_list)
    let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;

    let (conversation_id, add_message_id, user_message_id, request_prompt_result_with_context, init_message_list) =
        if request.conversation_id.is_empty() {
            let message_attachment_list = db
                .attachment_repo()
                .unwrap()
                .list_by_id(&request.attachment_list.clone().unwrap_or(vec![]))?;
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
            let request_prompt_result_with_context =
                format!("{}\n{}", request_prompt_result, context);
            let init_message_list = vec![
                (
                    String::from("system"),
                    override_prompt.unwrap_or(assistant_prompt_result),
                    vec![],
                ),
                (
                    String::from("user"),
                    request_prompt_result_with_context.clone(),
                    message_attachment_list,
                ),
            ];
            debug!(
                assistant_id = request.assistant_id,
                ?init_message_list,
                "initialize new conversation"
            );
            let (conversation, created_messages) = init_conversation(
                app_handle,
                request.assistant_id,
                assistant_detail.model[0].id,
                assistant_detail.model[0].model_code.clone(),
                &init_message_list,
            )?;
            // 获取用户消息的 ID（第二条消息是 user 类型）
            let user_msg_id = created_messages
                .iter()
                .find(|m| m.message_type == "user")
                .map(|m| m.id)
                .unwrap_or(0);
            (
                conversation.id,
                None, // 不预先创建空的assistant消息，让流式处理动态创建
                user_msg_id,
                request_prompt_result_with_context,
                init_message_list,
            )
        } else {
            // 已存在对话逻辑
            let conversation_id = request.conversation_id.parse::<i64>()?;
            let all_messages =
                db.message_repo().unwrap().list_by_conversation_id(conversation_id)?;

            let message_list = build_message_list_from_db(&all_messages, BranchSelection::LatestChildren);

            // 获取到消息的附件列表
            let message_attachment_list = db
                .attachment_repo()
                .unwrap()
                .list_by_id(&request.attachment_list.clone().unwrap_or(vec![]))?;
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

            let request_prompt_result_with_context =
                format!("{}\n{}", request_prompt_result, context);
            // 添加用户消息
            let user_message = add_message(
                app_handle,
                None,
                conversation_id,
                "user".to_string(),
                request_prompt_result_with_context.clone(),
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
                db.attachment_repo()
                    .unwrap()
                    .update(&updated_attachment)
                    .map_err(AppError::from)?;
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

            let _ = app_handle
                .emit(format!("conversation_event_{}", conversation_id).as_str(), add_event);

            let update_event = ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(MessageUpdateEvent {
                    message_id: user_message.id,
                    message_type: "user".to_string(),
                    content: request_prompt_result_with_context.clone(),
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
            updated_message_list.push((
                String::from("user"),
                request_prompt_result_with_context.clone(),
                message_attachment_list,
            ));

            (
                conversation_id,
                None, // 不预先创建空的assistant消息，让流式处理动态创建
                user_message.id,
                request_prompt_result_with_context,
                updated_message_list,
            )
        };
    Ok((conversation_id, add_message_id, user_message_id, request_prompt_result_with_context, init_message_list))
}

/// 获取指定对话的当前活动焦点状态（用于前端闪亮边框同步）
#[tauri::command]
pub async fn get_activity_focus(
    activity_manager: State<'_, ConversationActivityManager>,
    conversation_id: i64,
) -> Result<ActivityFocus, String> {
    Ok(activity_manager.get_focus(conversation_id).await)
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

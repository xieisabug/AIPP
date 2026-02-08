use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config,
};
use crate::api::ai::events::TITLE_CHANGE_EVENT;
use crate::api::genai_client;
use crate::db::conversation_db::{Conversation, ConversationDatabase};
use crate::db::llm_db::LLMDatabase;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::utils::window_utils::send_error_to_appropriate_window;
use std::collections::HashMap;
use tauri::Emitter;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, warn};

pub async fn generate_title(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    user_prompt: String,
    content: String,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    window: tauri::Window,
) -> Result<(), AppError> {
    // 0) 检查总结标题功能是否启用
    let feature_config_opt = config_feature_map.get("conversation_summary");
    let title_enabled = feature_config_opt
        .and_then(|fc| fc.get("title_summary_enabled"))
        .map(|c| c.value.clone())
        .unwrap_or_else(|| "true".to_string());

    if title_enabled != "true" && title_enabled != "1" {
        debug!("总结标题功能已禁用，跳过标题生成");
        return Ok(());
    }

    // 1) 读取会话总结配置，允许缺省并做智能回退
    // 默认提示词（当未配置时回退使用）
    let default_prompt = "请根据提供的大模型问答对话,总结一个简洁明了的标题。标题要求:\n- 字数在5-15个字左右，必须是中文，不要包含标点符号\n- 准确概括对话的核心主题，尽量贴近用户的提问\n- 不要透露任何私人信息\n- 用祈使句或陈述句".to_string();

    // 解析 summary_length 与 prompt，缺省有默认值
    let (summary_length, prompt) = if let Some(feature_config) = feature_config_opt {
        let prompt = feature_config
            .get("title_prompt")
            .or(feature_config.get("prompt"))
            .map(|c| c.value.clone())
            .unwrap_or(default_prompt.clone());
        let summary_length = feature_config
            .get("title_summary_length")
            .or(feature_config.get("summary_length"))
            .map(|c| c.value.clone())
            .unwrap_or_else(|| "100".to_string());
        let summary_length = summary_length.parse::<i32>().unwrap_or(100);
        (summary_length, prompt)
    } else {
        (100, default_prompt.clone())
    };

    // 解析 provider_id 与 model_code，若缺失/非法，则回退为"使用当前对话的回复消息模型"
    enum ModelSource {
        FromConfig { provider_id: i64, model_code: String },
        FromConversationMessage,
    }

    let model_source = if let Some(feature_config) = feature_config_opt {
        // 优先读取新配置键
        let provider_id_res = feature_config
            .get("title_provider_id")
            .or(feature_config.get("provider_id"))
            .map(|c| c.value.clone())
            .and_then(|v| v.parse::<i64>().ok());
        let model_code_opt = feature_config.get("title_model").map(|c| c.value.clone());
        // 兼容旧配置
        let legacy_model_code_opt = feature_config.get("model_code").map(|c| c.value.clone());
        let legacy_provider_id_res = feature_config
            .get("provider_id")
            .map(|c| c.value.clone())
            .and_then(|v| v.parse::<i64>().ok());

        match (provider_id_res, model_code_opt) {
            (Some(pid), Some(mcode)) if !mcode.is_empty() => {
                ModelSource::FromConfig { provider_id: pid, model_code: mcode }
            }
            _ => {
                // 回退到旧配置
                match (legacy_provider_id_res, legacy_model_code_opt) {
                    (Some(pid), Some(mcode)) if !mcode.is_empty() => {
                        ModelSource::FromConfig { provider_id: pid, model_code: mcode }
                    }
                    _ => ModelSource::FromConversationMessage,
                }
            }
        }
    } else {
        ModelSource::FromConversationMessage
    };

    let mut context = String::new();
    if summary_length == -1 {
        if content.is_empty() {
            context.push_str(
                format!(
                    "# user\n {} \n\n请为上述用户问题生成一个简洁的标题，不需要包含标点符号",
                    user_prompt
                )
                .as_str(),
            );
        } else {
            context.push_str(
                format!(
                    "# user\n {} \n\n#assistant\n {} \n\n请总结上述对话为标题，不需要包含标点符号",
                    user_prompt, content
                )
                .as_str(),
            );
        }
    } else {
        // 仅在非 -1 的情况下安全地转换为 usize
        let unsize_summary_length: usize =
            if summary_length < 0 { 0 } else { summary_length as usize };
        if content.is_empty() {
            if user_prompt.len() > unsize_summary_length {
                context.push_str(
                    format!(
                        "# user\n {} \n\n请为上述用户问题生成一个简洁的标题，不需要包含标点符号",
                        user_prompt.chars().take(unsize_summary_length).collect::<String>()
                    )
                    .as_str(),
                );
            } else {
                context.push_str(
                    format!(
                        "# user\n {} \n\n请为上述用户问题生成一个简洁的标题，不需要包含标点符号",
                        user_prompt
                    )
                    .as_str(),
                );
            }
        } else {
            if user_prompt.len() > unsize_summary_length {
                context.push_str(
                    format!(
                        "# user\n {} \n\n请总结上述对话为标题，不需要包含标点符号",
                        user_prompt.chars().take(unsize_summary_length).collect::<String>()
                    )
                    .as_str(),
                );
            } else {
                let assistant_summary_length = unsize_summary_length - user_prompt.len();
                if content.len() > assistant_summary_length {
                    context.push_str(format!("# user\n {} \n\n#assistant\n {} \n\n请总结上述对话为标题，不需要包含标点符号", user_prompt, content.chars().take(assistant_summary_length).collect::<String>()).as_str());
                } else {
                    context.push_str(format!("# user\n {} \n\n#assistant\n {} \n\n请总结上述对话为标题，不需要包含标点符号", user_prompt, content).as_str());
                }
            }
        }
    }

    // 2) 获取用于标题生成的模型详情：优先配置，其次回退到最近的一条 response 消息所用模型
    let llm_db = LLMDatabase::new(app_handle).map_err(AppError::from)?;
    let model_detail = match model_source {
        ModelSource::FromConfig { provider_id, model_code } => llm_db
            .get_llm_model_detail(&provider_id, &model_code)
            .map_err(|e| AppError::DatabaseError(format!("获取模型配置失败: {}", e)))?,
        ModelSource::FromConversationMessage => {
            // 从当前对话中找出最近的一条 response 消息，读取其 llm_model_id
            let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
            let messages = conversation_db
                .message_repo()
                .map_err(AppError::from)?
                .list_by_conversation_id(conversation_id)
                .map_err(AppError::from)?;
            // 选取 id 最大的 response 消息（通常是最新的）
            let last_response_model_id = messages
                .iter()
                .filter(|(m, _)| m.message_type == "response")
                .max_by_key(|(m, _)| m.id)
                .and_then(|(m, _)| m.llm_model_id);

            if let Some(model_id) = last_response_model_id {
                llm_db.get_llm_model_detail_by_id(&model_id).map_err(|e| {
                    AppError::DatabaseError(format!("获取标题模型失败(根据消息推断): {}", e))
                })?
            } else {
                return Err(AppError::UnknownError(
                    "未配置会话标题生成且无法从对话消息推断模型，请在设置中为辅助AI-总结标题选择模型"
                        .to_string(),
                ));
            }
        }
    };

    // 从配置中获取网络代理和超时设置
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let request_timeout = get_request_timeout_from_config(&config_feature_map);

    // 检查供应商是否启用了代理（标题生成通常不需要代理，设为false）
    let proxy_enabled = false;

    let client = genai_client::create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
        &config_feature_map,
    )?;

    let chat_request = crate::api::ai::conversation::build_chat_request_from_messages(
        &[
            ("system".to_string(), prompt.clone(), Vec::new()),
            ("user".to_string(), context.clone(), Vec::new()),
        ],
        crate::api::ai::conversation::ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;
    let model_name = &model_detail.model.code;

    // 从配置中获取最大重试次数
    let max_retry_attempts = get_retry_attempts_from_config(&config_feature_map);

    let mut attempts = 0;
    let response = loop {
        match client.exec_chat(model_name, chat_request.clone(), None).await {
            Ok(chat_response) => break Ok(chat_response.first_text().unwrap_or("").to_string()),
            Err(e) => {
                attempts += 1;
                if attempts >= max_retry_attempts {
                    error!(attempts, error = %e, "Title generation failed after max attempts");
                    break Err(e.to_string());
                }
                warn!(attempts, error = %e, "Title generation attempt failed, retrying");
                let delay = calculate_retry_delay(attempts);
                sleep(Duration::from_millis(delay)).await;
            }
        }
    };
    match response {
        Err(e) => {
            error!(error = %e, conversation_id, "chat error during title generation");
            // 将错误发送到前端，并同时返回错误供调用方处理
            send_error_to_appropriate_window(
                &window,
                "生成对话标题失败，请检查配置",
                Some(conversation_id),
            );
            return Err(AppError::UnknownError(format!("生成对话标题失败: {}", e)));
        }
        Ok(response_text) => {
            debug!(conversation_id, response_text, "generated title successfully");
            let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
            if let Err(e) =
                conversation_db.conversation_repo().unwrap().update_name(&Conversation {
                    id: conversation_id,
                    name: response_text.clone(),
                    assistant_id: None,
                    created_time: chrono::Utc::now(),
                })
            {
                error!(error = %e, conversation_id, "failed to update conversation name after title generation");
                return Err(AppError::DatabaseError("更新对话标题失败".to_string()));
            }
            if let Err(e) =
                window.emit(TITLE_CHANGE_EVENT, (conversation_id, response_text.clone()))
            {
                warn!(error = %e, conversation_id, "failed to emit TITLE_CHANGE_EVENT");
            }
        }
    }
    Ok(())
}

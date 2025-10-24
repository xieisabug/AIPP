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
use genai::chat::{ChatMessage, ChatRequest};
use std::collections::HashMap;
use tauri::Emitter;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

pub async fn generate_title(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    user_prompt: String,
    content: String,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    window: tauri::Window,
) -> Result<(), AppError> {
    let feature_config = config_feature_map
        .get("conversation_summary")
        .ok_or_else(|| AppError::UnknownError("未配置『会话标题生成』(conversation_summary)，请在设置中完成配置".to_string()))?;

    // provider_id
    let provider_id_str = feature_config
        .get("provider_id")
        .ok_or_else(|| AppError::UnknownError("标题生成配置错误: 缺少 provider_id".to_string()))?
        .value
        .clone();
    let provider_id = provider_id_str.parse::<i64>().map_err(|_| {
        AppError::UnknownError("标题生成配置错误: provider_id 必须是数字".to_string())
    })?;

    // model_code
    let model_code = feature_config
        .get("model_code")
        .ok_or_else(|| AppError::UnknownError("标题生成配置错误: 缺少 model_code".to_string()))?
        .value
        .clone();

    // prompt
    let prompt = feature_config
        .get("prompt")
        .ok_or_else(|| AppError::UnknownError("标题生成配置错误: 缺少 prompt".to_string()))?
        .value
        .clone();

    // summary_length
    let summary_length_str = feature_config
        .get("summary_length")
        .ok_or_else(|| AppError::UnknownError("标题生成配置错误: 缺少 summary_length".to_string()))?
        .value
        .clone();
    let summary_length = summary_length_str.parse::<i32>().map_err(|_| {
        AppError::UnknownError("标题生成配置错误: summary_length 必须是整数".to_string())
    })?;

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
            let unsize_summary_length: usize = if summary_length < 0 {
                0
            } else {
                summary_length as usize
            };
            if content.is_empty() {
                if user_prompt.len() > unsize_summary_length {
                    context.push_str(
                        format!(
                            "# user\n {} \n\n请为上述用户问题生成一个简洁的标题，不需要包含标点符号",
                            user_prompt
                                .chars()
                                .take(unsize_summary_length)
                                .collect::<String>()
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

        let llm_db = LLMDatabase::new(app_handle).map_err(AppError::from)?;
        let model_detail = llm_db
            .get_llm_model_detail(&provider_id, &model_code)
            .map_err(|e| AppError::DatabaseError(format!("获取模型配置失败: {}", e)))?;

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
        )?;

        let chat_messages = vec![ChatMessage::system(&prompt), ChatMessage::user(&context)];
        let chat_request = ChatRequest::new(chat_messages);
        let model_name = &model_detail.model.code;

        // 从配置中获取最大重试次数
        let max_retry_attempts = get_retry_attempts_from_config(&config_feature_map);

        let mut attempts = 0;
        let response = loop {
            match client.exec_chat(model_name, chat_request.clone(), None).await {
                Ok(chat_response) => {
                    break Ok(chat_response.first_text().unwrap_or("").to_string())
                }
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
                send_error_to_appropriate_window(&window, "生成对话标题失败，请检查配置");
                return Err(AppError::UnknownError(format!("生成对话标题失败: {}", e)));
            }
            Ok(response_text) => {
                debug!(conversation_id, response_text, "generated title successfully");
                let conversation_db =
                    ConversationDatabase::new(app_handle).map_err(AppError::from)?;
                if let Err(e) = conversation_db.conversation_repo().unwrap().update_name(&Conversation {
                    id: conversation_id,
                    name: response_text.clone(),
                    assistant_id: None,
                    created_time: chrono::Utc::now(),
                }) {
                    error!(error = %e, conversation_id, "failed to update conversation name after title generation");
                    return Err(AppError::DatabaseError("更新对话标题失败".to_string()));
                }
                if let Err(e) = window.emit(TITLE_CHANGE_EVENT, (conversation_id, response_text.clone())) {
                    warn!(error = %e, conversation_id, "failed to emit TITLE_CHANGE_EVENT");
                }
            }
        }
    Ok(())
}

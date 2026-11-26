use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config,
};
use crate::api::ai::events::TITLE_CHANGE_EVENT;
use crate::api::genai_client;
use crate::db::conversation_db::{Conversation, ConversationDatabase, Repository};
use crate::db::llm_db::LLMDatabase;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::utils::window_utils::send_error_to_appropriate_window;
use std::collections::HashMap;
use tauri::Emitter;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, warn};

pub const DEFAULT_CONVERSATION_TITLE: &str = "新对话";

pub fn should_auto_generate_title(conversation_name: &str) -> bool {
    let trimmed = conversation_name.trim();
    trimmed.is_empty() || trimmed == DEFAULT_CONVERSATION_TITLE
}

fn has_explicit_title_model_config(
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> bool {
    if crate::api::ai::config::resolve_default_light_model(config_feature_map).is_some() {
        return true;
    }
    config_feature_map.get("conversation_summary").is_some_and(|feature_config| {
        let provider_id = feature_config
            .get("title_provider_id")
            .or(feature_config.get("provider_id"))
            .and_then(|config| config.value.parse::<i64>().ok());
        let model_code = feature_config
            .get("title_model")
            .or(feature_config.get("model_code"))
            .map(|config| config.value.trim())
            .filter(|value| !value.is_empty());

        provider_id.is_some() && model_code.is_some()
    })
}

pub fn spawn_title_generation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    user_prompt: String,
    content: String,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    window: tauri::Window,
    reason: &'static str,
) {
    tokio::spawn(async move {
        if let Err(e) = generate_title(
            &app_handle,
            conversation_id,
            user_prompt,
            content,
            config_feature_map,
            window,
        )
        .await
        {
            warn!(conversation_id, reason, error = %e, "title generation failed");
        }
    });
}

pub async fn maybe_generate_title_from_conversation_if_needed(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
    window: tauri::Window,
    reason: &'static str,
) -> Result<bool, AppError> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    let conversation_repo = conversation_db.conversation_repo().map_err(AppError::from)?;
    let message_repo = conversation_db.message_repo().map_err(AppError::from)?;

    let Some(conversation) = conversation_repo.read(conversation_id).map_err(AppError::from)?
    else {
        warn!(
            conversation_id,
            reason, "skipping title generation because conversation was not found"
        );
        return Ok(false);
    };

    if !should_auto_generate_title(&conversation.name) {
        debug!(
            conversation_id,
            reason,
            current_title = conversation.name.as_str(),
            "skipping title generation because conversation already has a custom title"
        );
        return Ok(false);
    }

    let messages = message_repo.list_by_conversation_id(conversation_id).map_err(AppError::from)?;
    // 标题只按用户原话生成，不含本轮附加上下文。
    let Some(user_prompt) = messages
        .iter()
        .map(|(message, _)| (message, crate::utils::turn_context::visible(&message.content)))
        .find(|(message, text)| message.message_type == "user" && !text.trim().is_empty())
        .map(|(_, text)| text.to_string())
    else {
        warn!(
            conversation_id,
            reason, "skipping title generation because no user message was found"
        );
        return Ok(false);
    };

    let response_content = messages
        .iter()
        .find(|(message, _)| {
            message.message_type == "response" && !message.content.trim().is_empty()
        })
        .map(|(message, _)| message.content.clone())
        .unwrap_or_default();

    if response_content.is_empty() && !has_explicit_title_model_config(&config_feature_map) {
        debug!(
            conversation_id,
            reason,
            "skipping title generation because there is no response content and no dedicated title model"
        );
        return Ok(false);
    }

    spawn_title_generation(
        app_handle.clone(),
        conversation_id,
        user_prompt,
        response_content,
        config_feature_map,
        window,
        reason,
    );

    Ok(true)
}

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

    // 收集配置的候选模型（按优先级：标题模型 → 旧配置 → 默认轻量模型），跳过本机智能体/禁用/已删除项
    let mut config_candidates: Vec<(i64, String)> = Vec::new();
    if let Some(feature_config) = feature_config_opt {
        let provider_id_res = feature_config
            .get("title_provider_id")
            .or(feature_config.get("provider_id"))
            .map(|c| c.value.clone())
            .and_then(|v| v.parse::<i64>().ok());
        let model_code_opt = feature_config.get("title_model").map(|c| c.value.clone());
        if let (Some(pid), Some(mcode)) = (provider_id_res, model_code_opt) {
            if !mcode.trim().is_empty() {
                config_candidates.push((pid, mcode));
            }
        }
        let legacy_model_code_opt = feature_config.get("model_code").map(|c| c.value.clone());
        let legacy_provider_id_res = feature_config
            .get("provider_id")
            .map(|c| c.value.clone())
            .and_then(|v| v.parse::<i64>().ok());
        if let (Some(pid), Some(mcode)) = (legacy_provider_id_res, legacy_model_code_opt) {
            if !mcode.trim().is_empty() {
                config_candidates.push((pid, mcode));
            }
        }
        if let Some((mcode, pid)) = crate::api::ai::config::resolve_default_light_model(&config_feature_map) {
            config_candidates.push((pid, mcode));
        }
    }

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

    // 2) 获取用于标题生成的模型详情：按配置回退链查找可用 API 模型，再回退到最近回复消息的模型；都没有则截断用户首句
    let llm_db = LLMDatabase::new(app_handle).map_err(AppError::from)?;
    let mut resolved_model = None;
    for (provider_id, model_code) in config_candidates {
        match llm_db.get_model_for_request(&provider_id, &model_code) {
            Ok(detail) => {
                resolved_model = Some(detail);
                break;
            }
            Err(
                crate::db::llm_db::ModelRequestError::Lookup(rusqlite::Error::QueryReturnedNoRows)
                | crate::db::llm_db::ModelRequestError::LocalAgent { .. }
                | crate::db::llm_db::ModelRequestError::Disabled { .. },
            ) => continue,
            Err(e) => return Err(AppError::ProviderError(e.to_string())),
        }
    }
    if resolved_model.is_none() {
        let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
        let messages = conversation_db
            .message_repo()
            .map_err(AppError::from)?
            .list_by_conversation_id(conversation_id)
            .map_err(AppError::from)?;
        let last_response_model_id = messages
            .iter()
            .filter(|(m, _)| m.message_type == "response")
            .max_by_key(|(m, _)| m.id)
            .and_then(|(m, _)| m.llm_model_id);
        if let Some(model_id) = last_response_model_id {
            match llm_db.get_llm_model_detail_by_id(&model_id) {
                Ok(detail)
                    if detail.provider.is_enabled
                        && !crate::db::llm_db::is_local_agent_api_type(&detail.provider.api_type) =>
                {
                    resolved_model = Some(detail);
                }
                Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => {}
                Err(e) => {
                    return Err(AppError::DatabaseError(format!(
                        "获取标题模型失败(根据消息推断): {}",
                        e
                    )));
                }
            }
        }
    }
    let Some(model_detail) = resolved_model else {
        let fallback = first_line_title(&user_prompt);
        if fallback.is_empty() {
            return Ok(());
        }
        return save_title(app_handle, &window, conversation_id, fallback);
    };

    // 从配置中获取网络代理和超时设置
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let request_timeout = get_request_timeout_from_config(&config_feature_map);

    // 检查供应商是否启用了代理（标题生成通常不需要代理，设为false）
    let proxy_enabled = false;

    let client = genai_client::create_client_with_config(
        Some((app_handle, conversation_id)),
        &crate::api::copilot_token_manager::prepare_provider_configs(
            app_handle,
            &model_detail.provider.api_type,
            &model_detail.configs,
            network_proxy.as_deref(),
        )
        .await
        .map_err(|e| AppError::ProviderError(e))?,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        Some(&model_detail.model.request_mode),
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
        false,
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
            save_title(app_handle, &window, conversation_id, response_text)?;
        }
    }
    Ok(())
}

fn save_title(app_handle: &tauri::AppHandle, window: &tauri::Window, conversation_id: i64, response_text: String) -> Result<(), AppError> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    if let Err(e) =
        conversation_db.conversation_repo()?.update_name(&Conversation {
            id: conversation_id,
            name: response_text.clone(),
            assistant_id: None,
            created_time: chrono::Utc::now(),
            updated_time: chrono::Utc::now(),
            conversation_kind: "normal".to_string(),
            parent_butler_conversation_id: None,
            source_task_title: None,
            is_hidden_from_normal_chat_list: false,
            channel_source: None,
            butler_task_status: None,
            butler_task_summary: None,
            butler_task_finalized_at: None,
        })
    {
        error!(error = %e, conversation_id, "failed to update conversation name after title generation");
        return Err(AppError::DatabaseError(format!("更新会话 {conversation_id} 的标题失败：{e}")));
    }
    if let Err(e) =
        window.emit(TITLE_CHANGE_EVENT, (conversation_id, response_text))
    {
        warn!(error = %e, conversation_id, "failed to emit TITLE_CHANGE_EVENT");
    }
    Ok(())
}

fn first_line_title(prompt: &str) -> String {
    crate::utils::turn_context::visible(prompt).trim().lines().next().unwrap_or("").chars().take(20).collect()
}

#[cfg(test)]
mod tests {
    use super::{should_auto_generate_title, DEFAULT_CONVERSATION_TITLE};

    #[test]
    fn test_onboarding_aux_title_without_api_truncates_first_line_by_unicode_character() {
        let title = super::first_line_title("  测试标题😀一二三四五六七八九十一二三四五六七八九十\n后续内容");
        assert_eq!(title.chars().count(), 20);
        assert_eq!(super::first_line_title("  第一行\n第二行"), "第一行");
    }

    #[test]
    fn should_generate_for_default_titles() {
        assert!(should_auto_generate_title(""));
        assert!(should_auto_generate_title("   "));
        assert!(should_auto_generate_title(DEFAULT_CONVERSATION_TITLE));
    }

    #[test]
    fn should_not_generate_for_custom_titles() {
        assert!(!should_auto_generate_title("自定义标题"));
        assert!(!should_auto_generate_title("Project kickoff"));
    }
}

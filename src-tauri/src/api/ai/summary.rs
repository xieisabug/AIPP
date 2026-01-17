use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config,
};
use crate::api::genai_client;
use crate::db::conversation_db::{ConversationDatabase, ConversationSummary};
use crate::db::llm_db::LLMDatabase;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use genai::chat::{ChatMessage, ChatRequest};
use regex::Regex;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// 生成对话总结
pub async fn generate_conversation_summary(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    config_feature_map: HashMap<String, HashMap<String, FeatureConfig>>,
) -> Result<(), AppError> {
    // 0) 检查对话总结功能是否启用
    let feature_config_opt = config_feature_map.get("conversation_summary");
    let summary_enabled = feature_config_opt
        .and_then(|fc| fc.get("conversation_summary_enabled"))
        .map(|c| c.value.clone())
        .unwrap_or_else(|| "true".to_string());

    if summary_enabled != "true" && summary_enabled != "1" {
        debug!("对话总结功能已禁用，跳过总结生成");
        return Ok(());
    }

    // 检查是否已经总结过
    let conversation_db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    if let Ok(repo) = conversation_db.conversation_summary_repo() {
        if repo.exists(conversation_id)? {
            debug!(conversation_id, "对话已经总结过，跳过");
            return Ok(());
        }
    }

    // 1) 获取对话的所有消息
    let messages = conversation_db
        .message_repo()
        .map_err(AppError::from)?
        .list_by_conversation_id(conversation_id)
        .map_err(AppError::from)?;

    // 过滤出 user 和 response 类型的消息，并清理 base64 图片数据
    let relevant_messages: Vec<(String, String)> = messages
        .iter()
        .filter(|(m, _)| m.message_type == "user" || m.message_type == "response")
        .map(|(m, _)| (m.message_type.clone(), strip_base64_images(&m.content)))
        .collect();

    if relevant_messages.is_empty() {
        debug!(conversation_id, "对话没有有效消息，跳过总结");
        return Ok(());
    }

    // 2) 获取总结模型配置（同步获取，避免捕获非 Send 类型）
    let llm_db = LLMDatabase::new(app_handle).map_err(AppError::from)?;

    // 解析 provider_id 与 model_code，支持分开存储和组合格式
    // 格式1（分开存储）: conversation_summary_model + conversation_summary_provider_id
    // 格式2（组合值）: conversation_summary_model = "model_code%%provider_id"
    let (model_code, provider_id) = {
        let model_value = feature_config_opt
            .and_then(|fc| fc.get("conversation_summary_model"))
            .map(|c| c.value.clone())
            .unwrap_or_default();

        let provider_id_value = feature_config_opt
            .and_then(|fc| fc.get("conversation_summary_provider_id"))
            .map(|c| c.value.clone())
            .unwrap_or_default();

        // 优先使用分开存储的配置
        if !model_value.is_empty() && !provider_id_value.is_empty() {
            let provider_id = provider_id_value.parse::<i64>().map_err(|_| {
                AppError::UnknownError("对话总结模型 provider_id 解析失败".to_string())
            })?;
            (model_value, provider_id)
        } else if !model_value.is_empty() {
            // 回退到组合格式 "model_code%%provider_id"
            let parts: Vec<&str> = model_value.split("%%").collect();
            if parts.len() < 2 {
                return Err(AppError::UnknownError(
                    "对话总结模型配置格式错误，应为 model_code%%provider_id".to_string(),
                ));
            }
            let model_code = parts[0].to_string();
            let provider_id = parts[1].parse::<i64>().map_err(|_| {
                AppError::UnknownError("对话总结模型 provider_id 解析失败".to_string())
            })?;
            (model_code, provider_id)
        } else {
            return Err(AppError::UnknownError(
                "对话总结模型未配置，请在设置中配置 conversation_summary_model".to_string(),
            ));
        }
    };

    let model_detail = llm_db.get_llm_model_detail(&provider_id, &model_code).map_err(|e| {
        AppError::DatabaseError(format!(
            "配置的对话总结模型不存在 (model_code={}, provider_id={})，请检查设置: {}",
            model_code, provider_id, e
        ))
    })?;

    // 3) 构建对话消息序列（system + user/assistant 对话 + 总结请求）
    let system_prompt = r#"你是一个对话总结助手。你的任务是阅读用户与AI助手之间的对话，然后根据要求生成结构化的总结。请认真理解对话内容，准确提取关键信息。"#;

    let summary_request_prompt = r#"请对以上对话进行总结，重点关注：
1. 用户的核心目的/需求是什么
2. AI 提供了哪些关键成果/解决方案
3. 对话中最关键的信息

请用简洁的中文回复，严格按照以下JSON格式（不要添加任何其他内容）：
{"summary": "对话整体总结（100-200字）", "user_intent": "用户目的（一句话）", "key_outcomes": "关键成果（关键成果点列表，用分号分隔）"}"#;

    // 4) 获取网络代理和超时设置
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let request_timeout = get_request_timeout_from_config(&config_feature_map);
    let proxy_enabled = false;

    let client = genai_client::create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
    )?;

    // 构建消息列表：system + 原始对话 + 总结请求
    let mut chat_messages = vec![ChatMessage::system(system_prompt)];

    // 添加原始对话消息（保持 user/assistant 角色）
    for (msg_type, content) in &relevant_messages {
        if msg_type == "user" {
            chat_messages.push(ChatMessage::user(content));
        } else {
            chat_messages.push(ChatMessage::assistant(content));
        }
    }

    // 添加总结请求
    chat_messages.push(ChatMessage::user(summary_request_prompt));

    let chat_request = ChatRequest::new(chat_messages);
    let model_name = model_detail.model.code.clone();

    // 5) 调用 AI 生成总结
    let max_retry_attempts = get_retry_attempts_from_config(&config_feature_map);

    let mut attempts = 0;
    let response = loop {
        match client.exec_chat(&model_name, chat_request.clone(), None).await {
            Ok(chat_response) => break Ok(chat_response.first_text().unwrap_or("").to_string()),
            Err(e) => {
                attempts += 1;
                if attempts >= max_retry_attempts {
                    error!(attempts, error = %e, conversation_id, "对话总结生成失败，已达最大重试次数");
                    break Err(e.to_string());
                }
                warn!(attempts, error = %e, conversation_id, "对话总结生成失败，正在重试");
                let delay = calculate_retry_delay(attempts);
                sleep(Duration::from_millis(delay)).await;
            }
        }
    };

    match response {
        Err(e) => {
            error!(error = %e, conversation_id, "对话总结生成失败");
            Err(AppError::UnknownError(format!("生成对话总结失败: {}", e)))
        }
        Ok(response_text) => {
            debug!(conversation_id, response_text, "对话总结生成成功");
            parse_and_save_summary(&conversation_db, conversation_id, &response_text)
        }
    }
}

/// 解析 AI 返回的 JSON 并保存到数据库
fn parse_and_save_summary(
    conversation_db: &ConversationDatabase,
    conversation_id: i64,
    response_text: &str,
) -> Result<(), AppError> {
    // 清理可能的 markdown 代码块标记
    let cleaned = response_text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string();

    // 解析 JSON
    let json_value: serde_json::Value = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, response = %response_text, "无法解析对话总结JSON，尝试清理后重试");
            // 尝试提取 JSON 部分
            if let Some(start) = cleaned.find('{') {
                if let Some(end) = cleaned.rfind('}') {
                    let json_str = &cleaned[start..=end];
                    match serde_json::from_str(json_str) {
                        Ok(v) => v,
                        Err(e) => {
                            return Err(AppError::UnknownError(format!(
                                "无法解析对话总结JSON: {}",
                                e
                            )));
                        }
                    }
                } else {
                    return Err(AppError::UnknownError(
                        "对话总结响应中未找到有效的JSON结构".to_string(),
                    ));
                }
            } else {
                return Err(AppError::UnknownError(
                    "对话总结响应中未找到有效的JSON结构".to_string(),
                ));
            }
        }
    };

    let summary = json_value.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let user_intent =
        json_value.get("user_intent").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let key_outcomes =
        json_value.get("key_outcomes").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // 如果解析结果为空，返回错误
    if summary.is_empty() && user_intent.is_empty() && key_outcomes.is_empty() {
        return Err(AppError::UnknownError("对话总结解析结果为空".to_string()));
    }

    save_summary_to_db(conversation_db, conversation_id, &summary, &user_intent, &key_outcomes)
}

/// 清理消息中的 base64 图片数据，将 ![alt](data:image/...) 替换为 ![alt]
fn strip_base64_images(content: &str) -> String {
    // 匹配 ![任意文本](data:image/任意内容)
    // 例如: ![image](data:image/png;base64,iVBORw0KGgo...)
    let re = Regex::new(r"!\[([^\]]*)\]\(data:image/[^)]+\)").unwrap();
    re.replace_all(content, "![$1]").to_string()
}

/// 保存总结到数据库
fn save_summary_to_db(
    conversation_db: &ConversationDatabase,
    conversation_id: i64,
    summary: &str,
    user_intent: &str,
    key_outcomes: &str,
) -> Result<(), AppError> {
    let conversation_summary = ConversationSummary {
        id: 0,
        conversation_id,
        summary: summary.to_string(),
        user_intent: user_intent.to_string(),
        key_outcomes: key_outcomes.to_string(),
        created_time: chrono::Utc::now(),
    };

    conversation_db
        .conversation_summary_repo()
        .map_err(AppError::from)?
        .create(&conversation_summary)
        .map_err(AppError::from)?;

    info!(conversation_id, "对话总结已保存");
    Ok(())
}

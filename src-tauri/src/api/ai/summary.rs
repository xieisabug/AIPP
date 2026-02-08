use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config,
};
use crate::api::genai_client;
use crate::db::conversation_db::{ConversationDatabase, ConversationSummary, Message};
use crate::db::llm_db::LLMDatabase;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use regex::Regex;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Get latest branch messages (keep the latest branch flow intact).
///
/// Algorithm:
/// 1. Sort messages by created_time then id.
/// 2. When a parent_group_id appears, truncate before the parent group (replace it).
/// 3. For messages with the same generation_group_id, only keep the latest one.
/// 4. Append current message to form the latest branch.
pub(crate) fn get_latest_branch_messages(
    messages: &[(Message, Option<crate::db::conversation_db::MessageAttachment>)],
) -> Vec<Message> {
    if messages.is_empty() {
        return Vec::new();
    }

    let mut ordered: Vec<&Message> = messages.iter().map(|(m, _)| m).collect();
    ordered.sort_by(|a, b| {
        let by_time = a.created_time.cmp(&b.created_time);
        if by_time == std::cmp::Ordering::Equal {
            a.id.cmp(&b.id)
        } else {
            by_time
        }
    });

    let mut result: Vec<Message> = Vec::new();
    for msg in ordered {
        // 处理分支：当遇到带有 parent_group_id 的消息时，移除 parent group 并截断后续消息
        if let Some(parent_group_id) = &msg.parent_group_id {
            if let Some(first_index) = result.iter().position(|m| {
                m.generation_group_id.as_ref().map(|id| id == parent_group_id).unwrap_or(false)
            }) {
                // 移除 parent group 消息，截断之后的所有消息
                result.truncate(first_index);
            }
        }

        // 处理同一 generation_group_id 的多个消息：只保留最新的
        // 如果当前消息有 generation_group_id，检查 result 中是否已有相同 group_id 的消息
        if let Some(current_group_id) = &msg.generation_group_id {
            // 找到并移除所有具有相同 generation_group_id 的旧消息
            result.retain(|m| {
                m.generation_group_id.as_ref().map(|id| id != current_group_id).unwrap_or(true)
            });
        }

        result.push(msg.clone());
    }

    result
}

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
    let all_messages = conversation_db
        .message_repo()
        .map_err(AppError::from)?
        .list_by_conversation_id(conversation_id)
        .map_err(AppError::from)?;

    // 只获取最新分支的消息（过滤掉废弃分支的消息）
    let messages = get_latest_branch_messages(&all_messages);

    // For conversation summary, convert all messages to simple user/assistant text format.
    // This avoids ToolCall/ToolResponse format issues with the API.
    // - user -> user message
    // - response -> assistant message (strip MCP_TOOL_CALL comments, just keep text)
    // - tool_result -> assistant message (simplified description)
    let mcp_tool_call_regex = Regex::new(r"<!--\s*MCP_TOOL_CALL:.*?-->").unwrap();

    let relevant_messages: Vec<(
        String,
        String,
        Vec<crate::db::conversation_db::MessageAttachment>,
    )> = messages
        .iter()
        .filter(|m| {
            m.message_type == "user"
                || m.message_type == "response"
                || m.message_type == "tool_result"
        })
        .map(|m| {
            let content = strip_base64_images(&m.content);

            match m.message_type.as_str() {
                "user" => ("user".to_string(), content.trim().to_string(), Vec::new()),
                "response" => {
                    // Remove MCP_TOOL_CALL comments, keep only the text content
                    let content = mcp_tool_call_regex.replace_all(&content, "").to_string();
                    ("response".to_string(), content.trim().to_string(), Vec::new())
                }
                "tool_result" => {
                    // Simplify tool_result to a brief description for summary purposes
                    // Extract tool name and result status, skip detailed content
                    let simplified = simplify_tool_result_for_summary(&content);
                    ("response".to_string(), simplified, Vec::new())
                }
                _ => (m.message_type.clone(), content.trim().to_string(), Vec::new()),
            }
        })
        .filter(|(_, content, _)| !content.is_empty())
        .collect();

    // Merge consecutive messages of the same role to avoid API errors
    // This can happen after filtering out tool_result and empty response messages
    let mut merged_messages: Vec<(
        String,
        String,
        Vec<crate::db::conversation_db::MessageAttachment>,
    )> = Vec::new();
    for (msg_type, content, attachments) in relevant_messages {
        if let Some((last_type, last_content, _)) = merged_messages.last_mut() {
            if last_type == &msg_type {
                // Merge consecutive messages of the same type
                last_content.push_str("\n\n");
                last_content.push_str(&content);
                continue;
            }
        }
        merged_messages.push((msg_type, content, attachments));
    }
    let mut relevant_messages = merged_messages;

    // Remove trailing user message to avoid consecutive user messages
    // (since we'll add a summary request as user message later)
    if let Some((msg_type, _, _)) = relevant_messages.last() {
        if msg_type == "user" {
            relevant_messages.pop();
        }
    }

    if relevant_messages.is_empty() {
        debug!(conversation_id, "对话没有有效消息，跳过总结");
        // 创建一个空的总结记录，避免重复处理
        save_empty_summary_to_db(&conversation_db, conversation_id)?;
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
        &config_feature_map,
    )?;

    // 构建消息列表：system + 原始对话 + 总结请求
    let mut summary_message_list: Vec<(
        String,
        String,
        Vec<crate::db::conversation_db::MessageAttachment>,
    )> = Vec::new();
    summary_message_list.push(("system".to_string(), system_prompt.to_string(), Vec::new()));
    for (msg_type, content, attachments) in &relevant_messages {
        summary_message_list.push((msg_type.clone(), content.clone(), attachments.clone()));
    }
    summary_message_list.push(("user".to_string(), summary_request_prompt.to_string(), Vec::new()));

    let chat_request = crate::api::ai::conversation::build_chat_request_from_messages(
        &summary_message_list,
        crate::api::ai::conversation::ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;
    let model_name = model_detail.model.code.clone();

    // 5) 调用 AI 生成总结
    let max_retry_attempts = get_retry_attempts_from_config(&config_feature_map);

    let mut attempts = 0;
    let response = loop {
        match client.exec_chat(&model_name, chat_request.clone(), None).await {
            Ok(chat_response) => break Ok(chat_response.first_text().unwrap_or("").to_string()),
            Err(e) => {
                attempts += 1;
                // 400/422 错误时打印完整请求内容用于调试
                let error_text = e.to_string();
                let is_400_or_422 = error_text.contains("400") || error_text.contains("422");
                if is_400_or_422 {
                    if let Ok(request_json) = serde_json::to_string_pretty(&chat_request) {
                        error!(
                            conversation_id,
                            "
========== 总结请求内容（调试 400/422 错误）==========
{}
==========================================",
                            request_json
                        );
                    }
                }
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
    // 使用健壮的 JSON 提取函数
    let json_value = extract_json_from_response(response_text)
        .ok_or_else(|| AppError::UnknownError("对话总结响应中未找到有效的JSON结构".to_string()))?;

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

// Keep system/user/response messages and strip base64 images.
fn strip_base64_images(content: &str) -> String {
    // 匹配 ![任意文本](data:image/任意内容)
    // Keep system/user/response messages and strip base64 images.
    let re = Regex::new(r"!\[([^\]]*)\]\(data:image/[^)]+\)").unwrap();
    re.replace_all(content, "![$1]").to_string()
}

/// Simplify tool_result content for summary purposes.
/// Extract tool name and basic status, skip detailed result content.
fn simplify_tool_result_for_summary(content: &str) -> String {
    // Expected format: "Tool execution completed:\n\nTool Call ID: {id}\nTool: {name}\nServer: {server}\nParameters: {params}\nResult:\n{result}"
    // Or: "Tool execution failed:\n\n..."

    let is_success = content.contains("Tool execution completed");
    let is_failure = content.contains("Tool execution failed");

    // Extract tool name
    let tool_name = if let Some(start) = content.find("Tool: ") {
        let start_pos = start + "Tool: ".len();
        if let Some(end) = content[start_pos..].find('\n') {
            Some(content[start_pos..start_pos + end].to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Extract server name
    let server_name = if let Some(start) = content.find("Server: ") {
        let start_pos = start + "Server: ".len();
        if let Some(end) = content[start_pos..].find('\n') {
            Some(content[start_pos..start_pos + end].to_string())
        } else {
            None
        }
    } else {
        None
    };

    match (tool_name, server_name, is_success, is_failure) {
        (Some(tool), Some(server), true, _) => {
            format!("[已执行工具 {}/{} 并获得结果]", server, tool)
        }
        (Some(tool), Some(server), _, true) => {
            format!("[执行工具 {}/{} 失败]", server, tool)
        }
        (Some(tool), None, true, _) => {
            format!("[已执行工具 {} 并获得结果]", tool)
        }
        (Some(tool), None, _, true) => {
            format!("[执行工具 {} 失败]", tool)
        }
        _ => {
            // Fallback: just indicate a tool was executed
            if is_success {
                "[已执行工具调用并获得结果]".to_string()
            } else if is_failure {
                "[工具调用执行失败]".to_string()
            } else {
                "[工具调用结果]".to_string()
            }
        }
    }
}

/// 从 AI 响应中提取 JSON，支持多种格式：
/// 1. 纯 JSON 字符串
/// 2. Markdown 代码块包裹的 JSON (```json ... ```)
/// 3. 包含前后缀文本的 JSON
/// 4. 嵌套的 JSON 结构（查找最外层的 {}）
fn extract_json_from_response(response: &str) -> Option<serde_json::Value> {
    let trimmed = response.trim();

    // 1. 直接尝试解析
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(v);
    }

    // 2. 尝试去除 markdown 代码块
    let without_markdown = strip_markdown_code_block(trimmed);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&without_markdown) {
        return Some(v);
    }

    // 3. 查找并提取 JSON 对象（查找匹配的 {} 对）
    if let Some(json_str) = extract_balanced_json(&without_markdown) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&json_str) {
            return Some(v);
        }
    }

    // 4. 回退：简单的 find/rfind 方法
    if let Some(start) = without_markdown.find('{') {
        if let Some(end) = without_markdown.rfind('}') {
            if end > start {
                let json_str = &without_markdown[start..=end];
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                    return Some(v);
                }
            }
        }
    }

    None
}

/// 去除 markdown 代码块包裹
fn strip_markdown_code_block(s: &str) -> String {
    let trimmed = s.trim();

    // 处理 ```json ... ``` 或 ``` ... ``` 格式
    if trimmed.starts_with("```") {
        let without_start = trimmed.trim_start_matches("```");
        // 跳过语言标识符（如 json）
        let content = match without_start.find('\n') {
            Some(idx) => &without_start[idx + 1..],
            None => without_start,
        };
        // 去掉结尾的 ```
        let without_end = content.trim_end_matches("```").trim();
        return without_end.to_string();
    }

    trimmed.to_string()
}

/// 提取平衡的 JSON 对象（正确匹配 {} 对）
fn extract_balanced_json(s: &str) -> Option<String> {
    let mut start_idx = None;
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in s.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match c {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                if depth == 0 {
                    start_idx = Some(i);
                }
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = start_idx {
                        return Some(s[start..=i].to_string());
                    }
                }
            }
            _ => {}
        }
    }

    None
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

/// 为没有有效消息的对话创建空总结记录，避免定时任务重复处理
fn save_empty_summary_to_db(
    conversation_db: &ConversationDatabase,
    conversation_id: i64,
) -> Result<(), AppError> {
    let conversation_summary = ConversationSummary {
        id: 0,
        conversation_id,
        summary: "(无有效内容可总结)".to_string(),
        user_intent: "".to_string(),
        key_outcomes: "".to_string(),
        created_time: chrono::Utc::now(),
    };

    conversation_db
        .conversation_summary_repo()
        .map_err(AppError::from)?
        .create(&conversation_summary)
        .map_err(AppError::from)?;

    info!(conversation_id, "已为空对话创建占位总结记录");
    Ok(())
}

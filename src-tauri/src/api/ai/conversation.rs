use crate::api::ai::summary::get_latest_branch_messages;
use crate::api::ai_api::{build_tool_name, ToolNameMapping};
use crate::db::conversation_db::AttachmentType;
use crate::db::conversation_db::Repository;
use crate::db::conversation_db::{Conversation, ConversationDatabase, Message, MessageAttachment};
use crate::errors::AppError;
use base64::Engine;
use genai::chat::{ChatMessage, ChatRequest, Tool, ToolCall, ToolResponse};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::Emitter;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

fn build_user_message_with_attachments(
    content: &str,
    attachment_list: &[MessageAttachment],
) -> ChatMessage {
    if attachment_list.is_empty() {
        return ChatMessage::user(content);
    }

    let mut parts = Vec::new();
    parts.push(genai::chat::ContentPart::from_text(content));
    for attachment in attachment_list {
        // 优先处理图片附件（OpenAI 不支持 file:// 本地 URL，需要转为 base64）
        if attachment.attachment_type == AttachmentType::Image {
            // 1) 若 attachment_content 为 data:URL，直接解析
            if let Some(content) = &attachment.attachment_content {
                if content.starts_with("data:") {
                    if let Some((mime, b64)) = parse_data_url(content) {
                        parts.push(genai::chat::ContentPart::from_binary_base64(mime, b64, None));
                        continue;
                    }
                }
            }

            // 2) 若 attachment_url 为 http/https，则可直接使用 URL
            if let Some(url) = &attachment.attachment_url {
                let url_lower = url.to_lowercase();
                if url_lower.starts_with("http://") || url_lower.starts_with("https://") {
                    let mime = infer_media_type_from_url(url);
                    parts.push(genai::chat::ContentPart::from_binary_url(
                        mime,
                        url.clone(),
                        None,
                    ));
                    continue;
                }

                // 3) 若 attachment_url 是 data:URL，则解析为 base64
                if url_lower.starts_with("data:") {
                    if let Some((mime, b64)) = parse_data_url(url) {
                        parts.push(genai::chat::ContentPart::from_binary_base64(mime, b64, None));
                        continue;
                    }
                }

                // 4) 其他情况（如 file:// 或本地路径）：读取文件转 base64
                let path = if url_lower.starts_with("file://") {
                    // 去掉 file:// 前缀
                    url.trim_start_matches("file://").to_string()
                } else {
                    url.clone()
                };
                // 尝试读取文件并转换
                if let Ok(bytes) = std::fs::read(&path) {
                    let mime = infer_media_type_from_url(url);
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                    parts.push(genai::chat::ContentPart::from_binary_base64(mime, b64, None));
                    continue;
                } else {
                    // 无法读取则跳过为安全
                    warn!(url, "failed to read image file for attachment");
                }
            }
        }

        // 非图片类型或图片回退处理
        if let Some(attachment_content) = &attachment.attachment_content {
            if matches!(
                attachment.attachment_type,
                AttachmentType::Text
                    | AttachmentType::PDF
                    | AttachmentType::Word
                    | AttachmentType::PowerPoint
                    | AttachmentType::Excel
            ) {
                let file_name = attachment.attachment_url.as_deref().unwrap_or("未知文档");
                let file_type = match attachment.attachment_type {
                    AttachmentType::PDF => "PDF文档",
                    AttachmentType::Word => "Word文档",
                    AttachmentType::PowerPoint => "PowerPoint文档",
                    AttachmentType::Excel => "Excel文档",
                    _ => "文档",
                };
                parts.push(genai::chat::ContentPart::from_text(format!(
                    "\n\n[{}: {}]\n{}",
                    file_type, file_name, attachment_content
                )));
            }
        }
    }
    ChatMessage::user(parts)
}

fn extract_tool_call_ids_from_mcp_comments(content: &str) -> HashSet<String> {
    let mcp_call_regex = Regex::new(r"<!-- MCP_TOOL_CALL:(.*?) -->").unwrap();
    let mut ids = HashSet::new();
    for capture in mcp_call_regex.captures_iter(content) {
        if let Ok(tool_data) = serde_json::from_str::<serde_json::Value>(&capture[1]) {
            if let Some(llm_call_id) = tool_data["llm_call_id"].as_str() {
                let llm_call_id = llm_call_id.trim();
                if !llm_call_id.is_empty() {
                    ids.insert(llm_call_id.to_string());
                }
            }
            if let Some(call_id) = tool_data["call_id"].as_u64() {
                ids.insert(call_id.to_string());
            }
        }
    }
    ids
}

fn collect_sticky_response_ids(
    selected_messages: &[Message],
    all_messages: &[(Message, Option<MessageAttachment>)],
) -> HashSet<i64> {
    let mut tool_result_ids = HashSet::new();
    for message in selected_messages.iter() {
        if message.message_type != "tool_result" {
            continue;
        }
        if let Some(tool_call_id) = extract_tool_call_id(&message.content) {
            tool_result_ids.insert(tool_call_id);
        }
    }
    if tool_result_ids.is_empty() {
        return HashSet::new();
    }
    let mut sticky = HashSet::new();
    for (message, _) in all_messages.iter() {
        if message.message_type != "response" {
            continue;
        }
        let tool_call_ids = extract_tool_call_ids_from_mcp_comments(&message.content);
        if tool_call_ids.iter().any(|id| tool_result_ids.contains(id)) {
            sticky.insert(message.id);
        }
    }
    sticky
}

pub fn build_chat_messages(
    init_message_list: &[(String, String, Vec<MessageAttachment>)],
) -> Vec<ChatMessage> {
    build_chat_messages_with_context(init_message_list, None)
}

pub fn build_chat_messages_with_context(
    init_message_list: &[(String, String, Vec<MessageAttachment>)],
    current_tool_call_id: Option<String>,
) -> Vec<ChatMessage> {
    debug!(?current_tool_call_id, "build_chat_messages_with_context called");

    let mut valid_tool_call_ids: HashSet<String> = HashSet::new();
    let mcp_call_regex = Regex::new(r"<!-- MCP_TOOL_CALL:(.*?) -->").unwrap();
    for (message_type, content, _) in init_message_list.iter() {
        if message_type != "response" {
            continue;
        }
        for capture in mcp_call_regex.captures_iter(content) {
            if let Ok(tool_data) = serde_json::from_str::<serde_json::Value>(&capture[1]) {
                if let Some(llm_call_id) = tool_data["llm_call_id"].as_str() {
                    let llm_call_id = llm_call_id.trim();
                    if !llm_call_id.is_empty() {
                        valid_tool_call_ids.insert(llm_call_id.to_string());
                    }
                }
            }
        }
    }
    if let Some(ref tool_call_id) = current_tool_call_id {
        if !tool_call_id.trim().is_empty() {
            valid_tool_call_ids.insert(tool_call_id.clone());
        }
    }

    let mut chat_messages = Vec::new();
    for (message_type, content, attachment_list) in init_message_list.iter() {
        debug!(message_type, preview = %content.chars().take(50).collect::<String>(), "processing message");

        match message_type.as_str() {
            "system" => chat_messages.push(ChatMessage::system(content)),
            "user" => {
                chat_messages.push(build_user_message_with_attachments(content, attachment_list));
            }
            "tool_result" => {
                debug!("processing tool_result message");
                let tool_call_id = current_tool_call_id.clone().or_else(|| extract_tool_call_id(content));
                if let Some(tool_call_id) = tool_call_id {
                    if !valid_tool_call_ids.contains(&tool_call_id) {
                        debug!(tool_call_id, "tool_result without matching tool_call; downgrading");
                        chat_messages.push(ChatMessage::user(content));
                        continue;
                    }
                    debug!(tool_call_id, "using tool_call_id");

                    // Try to extract clean result, fallback to full content
                    let tool_result =
                        extract_tool_result(content).unwrap_or_else(|| content.to_string());

                    debug!(
                        preview = %tool_result.chars().take(100).collect::<String>(),
                        "tool result preview"
                    );

                    // Create ToolResponse from genai crate
                    let tool_response = ToolResponse::new(tool_call_id.clone(), tool_result);
                    debug!(tool_call_id, "created ToolResponse");
                    chat_messages.push(ChatMessage::from(tool_response));
                } else {
                    debug!("missing tool_call_id; downgrading tool_result to user message");
                    chat_messages.push(ChatMessage::user(content));
                }
            }
            other => {
                // 将 response 统一视为 assistant 历史
                if other == "response" {
                    // 检查是否包含 MCP_TOOL_CALL 注释，如果有则需要重建包含工具调用的 assistant 消息
                    if let Some(assistant_with_calls) =
                        reconstruct_assistant_with_tool_calls_from_content(content)
                    {
                        chat_messages.push(assistant_with_calls);
                    } else {
                        chat_messages.push(ChatMessage::assistant(content));
                    }
                } else if other == "system" {
                    chat_messages.push(ChatMessage::system(content));
                } else {
                    chat_messages.push(ChatMessage::assistant(content));
                }
            }
        }
    }
    chat_messages
}

/// 在非原生 toolcall 场景下，移除 MCP 注释并将 tool_result 转换为 user 消息，避免重新构建原生 tool_calls
pub fn sanitize_messages_for_non_native(
    init_message_list: &[(String, String, Vec<MessageAttachment>)],
) -> Vec<(String, String, Vec<MessageAttachment>)> {
    let mcp_hint_regex = Regex::new(r"<!--\s*MCP_TOOL_CALL:.*?-->").unwrap();

    init_message_list
        .iter()
        .map(|(message_type, content, attachments)| {
            let sanitized_content = mcp_hint_regex.replace_all(content, "").to_string();
            if message_type == "tool_result" {
                (String::from("user"), sanitized_content, Vec::new())
            } else {
                (message_type.clone(), sanitized_content, attachments.clone())
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub enum ToolCallStrategy {
    Native,
    NativeWithToolResponsePairing,
    NonNative,
}

#[derive(Debug, Clone)]
pub struct ToolConfig {
    pub tools: Vec<Tool>,
    pub tool_name_mapping: ToolNameMapping,
}

pub struct ChatRequestBuildResult {
    pub chat_request: ChatRequest,
    pub tool_name_mapping: ToolNameMapping,
}

pub fn build_chat_request_from_messages(
    init_message_list: &[(String, String, Vec<MessageAttachment>)],
    tool_call_strategy: ToolCallStrategy,
    tool_config: Option<ToolConfig>,
) -> ChatRequestBuildResult {
    let chat_messages = match tool_call_strategy {
        ToolCallStrategy::NativeWithToolResponsePairing => {
            build_native_toolcall_paired_messages(init_message_list)
        }
        ToolCallStrategy::NonNative => {
            let sanitized = sanitize_messages_for_non_native(init_message_list);
            build_chat_messages(&sanitized)
        }
        ToolCallStrategy::Native => build_chat_messages(init_message_list),
    };

    let mut chat_request = ChatRequest::new(chat_messages);
    let tool_name_mapping = tool_config
        .as_ref()
        .map(|config| config.tool_name_mapping.clone())
        .unwrap_or_default();
    if let Some(config) = tool_config {
        if !config.tools.is_empty() {
            chat_request = chat_request.with_tools(config.tools);
        }
    }

    ChatRequestBuildResult { chat_request, tool_name_mapping }
}

fn build_native_toolcall_paired_messages(
    init_message_list: &[(String, String, Vec<MessageAttachment>)],
) -> Vec<ChatMessage> {
    let mut chat_messages = Vec::new();
    let mut tool_call_to_response: HashMap<String, String> = HashMap::new();
    let mut message_trace: Vec<String> = Vec::new();

    for (message_type, content, _) in init_message_list.iter() {
        if message_type == "tool_result" {
            if let Some(call_id) = extract_tool_call_id(content) {
                if let Some(result) = extract_tool_result(content) {
                    tool_call_to_response.insert(call_id, result);
                }
            }
        }
    }

    for (message_type, content, attachment_list) in init_message_list.iter() {
        match message_type.as_str() {
            "system" => {
                chat_messages.push(ChatMessage::system(content));
                message_trace.push("system".to_string());
            }
            "user" => {
                chat_messages.push(build_user_message_with_attachments(content, attachment_list));
                message_trace.push("user".to_string());
            }
            "response" => {
                if let Some(assistant_with_calls) =
                    reconstruct_assistant_with_tool_calls_from_content(content)
                {
                    let tool_call_ids = assistant_with_calls
                        .content
                        .tool_calls()
                        .iter()
                        .map(|tc| tc.call_id.clone())
                        .collect::<Vec<_>>();
                    chat_messages.push(assistant_with_calls.clone());
                    message_trace.push(format!("assistant:tool_calls={:?}", tool_call_ids));

                    for tool_call in assistant_with_calls.content.tool_calls() {
                        if let Some(response_content) = tool_call_to_response.get(&tool_call.call_id)
                        {
                            let tool_response =
                                ToolResponse::new(tool_call.call_id.clone(), response_content.clone());
                            chat_messages.push(ChatMessage::from(tool_response));
                            message_trace.push(format!("tool:call_id={}", tool_call.call_id));
                        }
                    }
                } else {
                    chat_messages.push(ChatMessage::assistant(content));
                    message_trace.push("assistant".to_string());
                }
            }
            "tool_result" => {}
            _ => {
                chat_messages.push(ChatMessage::assistant(content));
                message_trace.push("assistant".to_string());
            }
        }
    }

    debug!(?message_trace, "toolcall paired message order");

    chat_messages
}

#[derive(Clone, Copy, Debug)]
pub enum BranchSelection {
    All,
    LatestBranch,
    LatestChildren,
}

pub fn build_message_list_from_selected_messages(
    selected_messages: &[Message],
    all_messages: &[(Message, Option<MessageAttachment>)],
) -> Vec<(String, String, Vec<MessageAttachment>)> {
    let msg_to_attachment: HashMap<i64, Option<MessageAttachment>> = all_messages
        .iter()
        .map(|(msg, att)| (msg.id, att.clone()))
        .collect();

    selected_messages
        .iter()
        .map(|message| {
            let attachment = msg_to_attachment.get(&message.id).cloned().flatten();
            (
                message.message_type.clone(),
                message.content.clone(),
                attachment.map(|a| vec![a]).unwrap_or_else(Vec::new),
            )
        })
        .collect()
}

pub fn build_message_list_from_db(
    all_messages: &[(Message, Option<MessageAttachment>)],
    branch_selection: BranchSelection,
) -> Vec<(String, String, Vec<MessageAttachment>)> {
    match branch_selection {
        BranchSelection::LatestBranch => {
            let mut latest_branch = get_latest_branch_messages(all_messages);
            let sticky_response_ids = collect_sticky_response_ids(&latest_branch, all_messages);
            if !sticky_response_ids.is_empty() {
                for (message, _) in all_messages.iter() {
                    if sticky_response_ids.contains(&message.id)
                        && !latest_branch.iter().any(|m| m.id == message.id)
                    {
                        latest_branch.push(message.clone());
                    }
                }
            }
            build_message_list_from_selected_messages(&latest_branch, all_messages)
        }
        BranchSelection::LatestChildren => {
            let (latest_children, child_ids) = get_latest_child_messages(all_messages);
            let mut message_list: Vec<(String, String, Vec<MessageAttachment>)> = Vec::new();
            let mut included_ids: HashSet<i64> = HashSet::new();
            let mut selected_messages: Vec<Message> = Vec::new();
            for (message, attachment) in all_messages.iter() {
                if child_ids.contains(&message.id) {
                    continue;
                }
                let (final_message, final_attachment) = latest_children
                    .get(&message.id)
                    .cloned()
                    .unwrap_or((message.clone(), attachment.clone()));

                included_ids.insert(final_message.id);
                selected_messages.push(final_message.clone());
                message_list.push((
                    final_message.message_type,
                    final_message.content,
                    final_attachment.map(|a| vec![a]).unwrap_or_else(Vec::new),
                ));
            }
            let sticky_response_ids = collect_sticky_response_ids(&selected_messages, all_messages);
            if !sticky_response_ids.is_empty() {
                for (message, attachment) in all_messages.iter() {
                    if sticky_response_ids.contains(&message.id) && !included_ids.contains(&message.id)
                    {
                        message_list.push((
                            message.message_type.clone(),
                            message.content.clone(),
                            attachment.clone().map(|a| vec![a]).unwrap_or_else(Vec::new),
                        ));
                    }
                }
            }
            sort_messages_by_group_and_id(message_list, all_messages)
        }
        BranchSelection::All => {
            let mut seen = HashSet::new();
            all_messages
                .iter()
                .filter(|(message, _)| seen.insert(message.id))
                .map(|(message, attachment)| {
                    (
                        message.message_type.clone(),
                        message.content.clone(),
                        attachment.clone().map(|a| vec![a]).unwrap_or_else(Vec::new),
                    )
                })
                .collect()
        }
    }
}

/// 获取每个父消息的最新子消息（统一的排序逻辑）
/// 返回: (latest_children_map, child_ids_set)
pub(crate) fn get_latest_child_messages(
    messages: &[(Message, Option<MessageAttachment>)],
) -> (HashMap<i64, (Message, Option<MessageAttachment>)>, HashSet<i64>) {
    let mut latest_children: HashMap<i64, (Message, Option<MessageAttachment>)> = HashMap::new();
    let mut child_ids: HashSet<i64> = HashSet::new();

    // 按 generation_group_id 分组，每组只保留 ID 最大的消息
    let mut group_to_latest: HashMap<String, (Message, Option<MessageAttachment>)> = HashMap::new();

    for (message, attachment) in messages.iter() {
        if let Some(parent_id) = message.parent_id {
            child_ids.insert(message.id);
            latest_children
                .entry(parent_id)
                .and_modify(|existing| {
                    // 选择ID更大的消息作为最新版本
                    if message.id > existing.0.id {
                        *existing = (message.clone(), attachment.clone());
                    }
                })
                .or_insert((message.clone(), attachment.clone()));
        }

        // 按 generation_group_id 分组，每组只保留最新的消息
        if let Some(ref group_id) = message.generation_group_id {
            group_to_latest
                .entry(group_id.clone())
                .and_modify(|existing| {
                    if message.id > existing.0.id {
                        *existing = (message.clone(), attachment.clone());
                    }
                })
                .or_insert((message.clone(), attachment.clone()));
        }
    }

    // 将被 group 机制过滤掉的消息加入 child_ids
    for (message, _) in messages.iter() {
        if let Some(ref group_id) = message.generation_group_id {
            if let Some((latest_msg, _)) = group_to_latest.get(group_id) {
                if message.id != latest_msg.id {
                    child_ids.insert(message.id);
                }
            }
        }
    }

    (latest_children, child_ids)
}

/// 按照group和ID排序消息列表
/// 规则：
/// 1. 按照root group的最小消息ID排序
/// 2. 同一group内的消息按ID排序
/// 3. 没有generation_group_id的消息排在最前面（按ID排序）
pub(crate) fn sort_messages_by_group_and_id(
    messages: Vec<(String, String, Vec<MessageAttachment>)>,
    original_messages: &[(Message, Option<MessageAttachment>)],
) -> Vec<(String, String, Vec<MessageAttachment>)> {
    let mut result = messages;

    // 创建消息内容到原始消息的映射，用于获取group信息
    let mut content_to_message: HashMap<String, &Message> = HashMap::new();
    for (msg, _) in original_messages {
        content_to_message.insert(msg.content.clone(), msg);
    }

    // 创建group到最小ID的映射
    let mut group_to_min_id: HashMap<String, i64> = HashMap::new();
    for (msg, _) in original_messages {
        if let Some(ref group_id) = msg.generation_group_id {
            group_to_min_id
                .entry(group_id.clone())
                .and_modify(|min_id| {
                    if msg.id < *min_id {
                        *min_id = msg.id;
                    }
                })
                .or_insert(msg.id);
        }
    }

    // 排序逻辑
    result.sort_by(|a, b| {
        let msg_a = content_to_message.get(&a.1);
        let msg_b = content_to_message.get(&b.1);

        match (msg_a, msg_b) {
            (Some(ma), Some(mb)) => match (&ma.generation_group_id, &mb.generation_group_id) {
                // 两个都有group_id
                (Some(group_a), Some(group_b)) => {
                    if group_a == group_b {
                        // 同一个group内，按消息ID排序
                        ma.id.cmp(&mb.id)
                    } else {
                        // 不同group，按group的最小ID排序
                        let min_a = group_to_min_id.get(group_a).unwrap_or(&ma.id);
                        let min_b = group_to_min_id.get(group_b).unwrap_or(&mb.id);
                        min_a.cmp(min_b)
                    }
                }
                // 只有A有group_id，按消息ID排序（而不是固定让B排前面）
                (Some(_), None) => ma.id.cmp(&mb.id),
                // 只有B有group_id，按消息ID排序（而不是固定让A排前面）
                (None, Some(_)) => ma.id.cmp(&mb.id),
                // 两个都没有group_id，按消息ID排序
                (None, None) => ma.id.cmp(&mb.id),
            },
            // 如果找不到对应的原始消息，保持原顺序
            _ => std::cmp::Ordering::Equal,
        }
    });

    result
}

// Helper function to extract tool call ID from tool result content
pub fn extract_tool_call_id(content: &str) -> Option<String> {
    // Expected format: "Tool execution completed:\n\nTool Call ID: {id}\nResult:\n{result}"
    if let Some(start) = content.find("Tool Call ID: ") {
        let start_pos = start + "Tool Call ID: ".len();
        if let Some(end) = content[start_pos..].find('\n') {
            return Some(content[start_pos..start_pos + end].to_string());
        } else {
            // If no newline found, take rest of string (shouldn't happen with our format)
            return Some(content[start_pos..].to_string());
        }
    }
    None
}

// Helper function to extract tool result from tool result content
pub fn extract_tool_result(content: &str) -> Option<String> {
    // Expected format: "Tool execution completed:\n\nTool Call ID: {id}\nResult:\n{result}"
    if let Some(start) = content.find("Result:\n") {
        let start_pos = start + "Result:\n".len();
        return Some(content[start_pos..].to_string());
    }
    None
}

// Helper function to reconstruct assistant message with tool calls from MCP_TOOL_CALL comments
pub fn reconstruct_assistant_with_tool_calls_from_content(content: &str) -> Option<ChatMessage> {
    // 查找所有 MCP_TOOL_CALL 注释
    let mcp_call_regex = Regex::new(r"<!-- MCP_TOOL_CALL:(.*?) -->").ok()?;
    let mut tool_calls = Vec::new();

    // 提取所有工具调用信息
    for capture in mcp_call_regex.captures_iter(content) {
        if let Ok(tool_data) = serde_json::from_str::<serde_json::Value>(&capture[1]) {
            if let (Some(server_name), Some(tool_name), Some(parameters)) = (
                tool_data["server_name"].as_str(),
                tool_data["tool_name"].as_str(),
                tool_data["parameters"].as_str(),
            ) {
                // 使用正确的格式：server__tool (双下划线)，并清理名称以符合 API 规范
                let fn_name = build_tool_name(server_name, tool_name);
                let fn_arguments =
                    serde_json::from_str(parameters).unwrap_or(serde_json::json!({}));

                // 优先使用 llm_call_id，如果没有则使用 call_id 转换为字符串
                let call_id = tool_data["llm_call_id"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| tool_data["call_id"].as_u64().map(|n| n.to_string()))
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                tool_calls.push(ToolCall { call_id, fn_name, fn_arguments });
            }
        }
    }

    if !tool_calls.is_empty() {
        // 创建包含工具调用的 assistant 消息（忽略文本内容以避免混合消息类型的复杂性）
        return Some(ChatMessage::from(tool_calls));
    }

    None
}

pub fn infer_media_type_from_url(url: &str) -> String {
    let url_lower = url.to_lowercase();
    if url_lower.ends_with(".jpg") || url_lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if url_lower.ends_with(".png") {
        "image/png".to_string()
    } else if url_lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if url_lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if url_lower.ends_with(".bmp") {
        "image/bmp".to_string()
    } else {
        "image/jpeg".to_string()
    }
}

pub fn parse_data_url(data_url: &str) -> Option<(String, String)> {
    if !data_url.starts_with("data:") {
        return None;
    }
    let parts: Vec<&str> = data_url.splitn(2, ',').collect();
    if parts.len() != 2 {
        return None;
    }
    let header = parts[0];
    let content = parts[1];
    let header_without_data = header.strip_prefix("data:")?;
    let mime_type = if let Some(semicolon_pos) = header_without_data.find(';') {
        &header_without_data[..semicolon_pos]
    } else {
        header_without_data
    };
    if !header.contains("base64") {
        return None;
    }
    Some((mime_type.to_string(), content.to_string()))
}

pub async fn cleanup_token(
    tokens: &Arc<tokio::sync::Mutex<HashMap<i64, CancellationToken>>>,
    message_id: i64,
) {
    let mut map = tokens.lock().await;
    map.remove(&message_id);
}

pub async fn handle_message_type_end(
    message_id: i64,
    message_type: &str,
    content: &str,
    start_time: chrono::DateTime<chrono::Utc>,
    conversation_db: &ConversationDatabase,
    window: &tauri::Window,
    conversation_id: i64,
    app_handle: &tauri::AppHandle,
    skip_mcp_detection: bool,
) -> Result<(), anyhow::Error> {
    let end_time = chrono::Utc::now();
    let duration_ms = end_time.timestamp_millis() - start_time.timestamp_millis();
    let mut final_content = content.to_string();

    conversation_db.message_repo()?.update_finish_time(message_id)?;

    // 对 response 和 reasoning 类型的消息都启用 MCP 检测
    if (message_type == "response" || message_type == "reasoning") && !skip_mcp_detection {
        match crate::mcp::detect_and_process_mcp_calls(
            app_handle,
            window,
            conversation_id,
            message_id,
            &final_content,
        )
        .await
        {
            Ok(updated_content) => {
                if let Some(new_content) = updated_content {
                    final_content = new_content;
                    if let Ok(repo) = conversation_db.message_repo() {
                        let _ = repo.update_content(message_id, &final_content);
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to detect MCP calls");
            }
        }
    }

    let type_end_event = crate::api::ai::events::ConversationEvent {
        r#type: "message_type_end".to_string(),
        data: serde_json::to_value(crate::api::ai::events::MessageTypeEndEvent {
            message_id,
            message_type: message_type.to_string(),
            duration_ms,
            end_time,
        })
        .unwrap(),
    };
    let _ = window.emit(format!("conversation_event_{}", conversation_id).as_str(), type_end_event);

    let final_update_event = crate::api::ai::events::ConversationEvent {
        r#type: "message_update".to_string(),
        data: serde_json::to_value(crate::api::ai::events::MessageUpdateEvent {
            message_id,
            message_type: message_type.to_string(),
            content: final_content,
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
        window.emit(format!("conversation_event_{}", conversation_id).as_str(), final_update_event);

    Ok(())
}

pub fn finish_stream_messages(
    conversation_db: &ConversationDatabase,
    reasoning_message_id: Option<i64>,
    response_message_id: Option<i64>,
    reasoning_content: &str,
    response_content: &str,
    window: &tauri::Window,
    conversation_id: i64,
) -> Result<(), anyhow::Error> {
    if let Some(msg_id) = reasoning_message_id {
        if response_message_id.is_none() {
            conversation_db.message_repo().unwrap().update_finish_time(msg_id).unwrap();
            let complete_event = crate::api::ai::events::ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(crate::api::ai::events::MessageUpdateEvent {
                    message_id: msg_id,
                    message_type: "reasoning".to_string(),
                    content: reasoning_content.to_string(),
                    is_done: true,
                    token_count: None,
                    input_token_count: None,
                    output_token_count: None,
                    ttft_ms: None,
                    tps: None,
                })
                .unwrap(),
            };
            let _ = window
                .emit(format!("conversation_event_{}", conversation_id).as_str(), complete_event);
        }
    }

    if let Some(msg_id) = response_message_id {
        conversation_db.message_repo().unwrap().update_finish_time(msg_id).unwrap();
        let complete_event = crate::api::ai::events::ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(crate::api::ai::events::MessageUpdateEvent {
                message_id: msg_id,
                message_type: "response".to_string(),
                content: response_content.to_string(),
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
            window.emit(format!("conversation_event_{}", conversation_id).as_str(), complete_event);
    }
    Ok(())
}

pub fn init_conversation(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
    llm_model_id: i64,
    llm_model_code: String,
    messages: &Vec<(String, String, Vec<MessageAttachment>)>,
) -> Result<(Conversation, Vec<Message>), AppError> {
    let db = ConversationDatabase::new(app_handle).map_err(AppError::from)?;
    let conversation = db
        .conversation_repo()
        .unwrap()
        .create(&Conversation {
            id: 0,
            name: "新对话".to_string(),
            assistant_id: Some(assistant_id),
            created_time: chrono::Utc::now(),
        })
        .map_err(AppError::from)?;
    let conversation_clone = conversation.clone();
    let conversation_id = conversation_clone.id;
    let mut message_result_array = vec![];
    for (message_type, content, attachment_list) in messages {
        let message = db
            .message_repo()
            .unwrap()
            .create(&Message {
                id: 0,
                parent_id: None,
                conversation_id,
                message_type: message_type.clone(),
                content: content.clone(),
                llm_model_id: Some(llm_model_id),
                llm_model_name: Some(llm_model_code.clone()),
                created_time: chrono::Utc::now(),
                start_time: None,
                finish_time: None,
                token_count: 0,
                input_token_count: 0,
                output_token_count: 0,
                generation_group_id: None,
                parent_group_id: None,
                tool_calls_json: None,
                first_token_time: None,
                ttft_ms: None,
            })
            .map_err(AppError::from)?;
        for attachment in attachment_list {
            let mut updated_attachment = attachment.clone();
            updated_attachment.message_id = message.id;
            db.attachment_repo().unwrap().update(&updated_attachment).map_err(AppError::from)?;
        }
        message_result_array.push(message.clone());
    }
    Ok((conversation_clone, message_result_array))
}

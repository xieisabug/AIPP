use std::collections::HashMap;

use chrono::{DateTime, Utc};
use regex;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::{
    db::conversation_db::{
        ConversationDatabase, Message, MessageAttachment, MessageDetail, Repository,
    },
    errors::AppError,
    NameCacheState,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationSearchHit {
    pub conversation_id: i64,
    pub conversation_name: String,
    pub assistant_name: String,
    pub message_id: Option<i64>,
    pub message_type: Option<String>,
    pub created_time: DateTime<Utc>,
    pub snippet: String,
    pub hit_type: String, // title | summary | message
}

/// 处理消息版本管理的纯函数 - 这是核心业务逻辑
/// 输入原始消息列表，返回经过版本管理处理的最终消息列表
pub fn process_message_versions(mut message_details: Vec<MessageDetail>) -> Vec<MessageDetail> {
    // 处理 regenerate 关系 - 支持 generation_group_id 系统
    let regenerate_map: HashMap<i64, Vec<MessageDetail>> = message_details
        .iter()
        .filter(|m| m.parent_id.is_some())
        .map(|m| (m.parent_id.unwrap(), m.clone()))
        .fold(HashMap::new(), |mut acc, (parent_id, message)| {
            acc.entry(parent_id).or_default().push(message);
            acc
        });

    // 为每个消息构建regenerate数组
    for message in &mut message_details {
        if let Some(regenerated) = regenerate_map.get(&message.id) {
            // 对regenerate消息按创建时间排序
            let mut sorted_regenerated = regenerated.clone();
            sorted_regenerated.sort_by_key(|m| m.created_time);
            message.regenerate = sorted_regenerated;
        }
    }

    // 过滤逻辑：显示最新版本的消息
    // 1. 如果消息没有parent_id，它是原始消息
    // 2. 如果消息有parent_id，它是某条消息的新版本
    // 3. 我们需要显示：原始消息（如果没有更新版本）或最新的更新版本

    // 构建parent_id到直接子消息的映射
    let mut direct_children: HashMap<i64, Vec<MessageDetail>> = HashMap::new();
    let mut child_message_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

    for message in &message_details {
        if let Some(parent_id) = message.parent_id {
            child_message_ids.insert(message.id);
            direct_children.entry(parent_id).or_default().push(message.clone());
        }
    }

    // 对每个父消息的子消息按时间排序
    for children in direct_children.values_mut() {
        children.sort_by_key(|m| m.created_time);
    }

    // 递归查找最终的最新版本
    fn find_latest_version(
        message_id: i64,
        direct_children: &HashMap<i64, Vec<MessageDetail>>,
    ) -> Option<MessageDetail> {
        if let Some(children) = direct_children.get(&message_id) {
            if let Some(latest_child) = children.last() {
                // 递归查找这个子版本的最新版本
                find_latest_version(latest_child.id, direct_children)
                    .or_else(|| Some(latest_child.clone()))
            } else {
                None
            }
        } else {
            None
        }
    }

    // 构建最终显示的消息列表
    let mut final_messages: Vec<MessageDetail> = Vec::new();
    for message in message_details {
        if child_message_ids.contains(&message.id) {
            // 这是某个消息的子版本，跳过（会在后续处理中添加最新版本）
            continue;
        }

        // 检查是否有这个消息的更新版本（递归查找）
        if let Some(latest_version) = find_latest_version(message.id, &direct_children) {
            // 有更新版本，使用最新版本
            final_messages.push(latest_version);
        } else {
            // 没有更新版本，使用原消息
            final_messages.push(message);
        }
    }

    // 按创建时间排序，使用复合排序键确保 reasoning 排在 response 之前
    // 对于同一 generation_group_id 的 reasoning 和 response，使用 group_id 作为次要排序键
    final_messages.sort_by(|a, b| {
        // 先按 created_time 排序
        match a.created_time.cmp(&b.created_time) {
            std::cmp::Ordering::Equal => {
                // 时间相同，按 message_type 优先级排序
                get_message_type_priority(&a.message_type)
                    .cmp(&get_message_type_priority(&b.message_type))
            }
            std::cmp::Ordering::Less => {
                // a 比 b 早，a 排在前面
                std::cmp::Ordering::Less
            }
            std::cmp::Ordering::Greater => {
                // a 比 b 晚，但检查是否是同一个 generation_group_id 中的 reasoning/response
                // 如果 a 是 reasoning，b 是 response，且属于同一组，则 a 应该排在 b 前面
                if belongs_to_same_group(a, b) && is_reasoning_before_response(a, b) {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            }
        }
    });
    final_messages
}

/// 检查两条消息是否属于同一个 generation_group_id
fn belongs_to_same_group(a: &MessageDetail, b: &MessageDetail) -> bool {
    match (&a.generation_group_id, &b.generation_group_id) {
        (Some(group_a), Some(group_b)) => group_a == group_b,
        _ => false,
    }
}

/// 检查 a 是否是 reasoning，b 是否是 response（即 a 应该排在 b 前面）
fn is_reasoning_before_response(a: &MessageDetail, b: &MessageDetail) -> bool {
    a.message_type == "reasoning" && b.message_type == "response"
}

/// 获取消息类型的优先级（用于时间相同时的排序）
fn get_message_type_priority(message_type: &str) -> i32 {
    match message_type {
        "system" => 0,
        "user" => 1,
        "reasoning" => 2,
        "response" => 3,
        "assistant" => 4,
        _ => 5,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationResult {
    pub id: i64,
    pub name: String,
    pub assistant_id: i64,
    pub assistant_name: String,
    pub created_time: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConversationWithMessages {
    pub conversation: ConversationResult,
    pub messages: Vec<MessageDetail>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CreateConversationResponse {
    pub conversation_id: i64,
    pub user_message_id: Option<i64>,
    pub system_message_id: Option<i64>,
}

#[tauri::command]
pub async fn create_conversation_with_messages(
    app_handle: tauri::AppHandle,
    name_cache_state: tauri::State<'_, NameCacheState>,
    assistant_id: i64,
    system_prompt: Option<String>,
    user_message: Option<String>,
    conversation_name: Option<String>,
) -> Result<CreateConversationResponse, AppError> {
    // 验证 assistant 是否存在
    let assistant_db =
        crate::db::assistant_db::AssistantDatabase::new(&app_handle).map_err(AppError::from)?;
    let assistant = assistant_db
        .get_assistant(assistant_id)
        .map_err(|e| AppError::DatabaseError(format!("Failed to get assistant: {}", e)))?;

    // 从助手配置中获取模型信息
    let assistant_models = assistant_db
        .get_assistant_model(assistant_id)
        .map_err(|e| AppError::DatabaseError(format!("Failed to get assistant models: {}", e)))?;

    // 使用第一个模型，如果没有模型则使用默认值
    let (model_id, model_code) = if let Some(first_model) = assistant_models.first() {
        (first_model.provider_id, first_model.model_code.clone())
    } else {
        (1i64, "default".to_string()) // 默认值
    };

    // 构建消息列表
    let mut messages = Vec::new();
    let mut system_message_id = None;
    let mut user_message_id = None;

    // 添加系统消息（如果提供）
    if let Some(system_prompt) = system_prompt {
        if !system_prompt.trim().is_empty() {
            messages.push(("system".to_string(), system_prompt, Vec::new()));
        }
    }

    // 添加用户消息（如果提供）
    if let Some(user_message) = user_message {
        if !user_message.trim().is_empty() {
            messages.push(("user".to_string(), user_message, Vec::new()));
        }
    }

    // 使用现有的 init_conversation 逻辑
    let (mut conversation, created_messages) = crate::api::ai::conversation::init_conversation(
        &app_handle,
        assistant_id,
        model_id,
        model_code,
        &messages,
    )?;

    // 更新对话名称（如果提供）
    if let Some(name) = conversation_name {
        if !name.trim().is_empty() {
            conversation.name = name;
            let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
            db.conversation_repo().unwrap().update(&conversation).map_err(AppError::from)?;
        }
    }

    // 获取消息ID
    if created_messages.len() >= 1
        && messages.get(0).map(|(t, _, _)| t) == Some(&"system".to_string())
    {
        system_message_id = Some(created_messages[0].id);
        if created_messages.len() >= 2 {
            user_message_id = Some(created_messages[1].id);
        }
    } else if created_messages.len() >= 1 {
        user_message_id = Some(created_messages[0].id);
    }

    // 发送事件通知前端更新
    let _ = app_handle.emit("conversation_created", conversation.id);

    // 更新助手名称缓存（确保UI显示正确）
    let mut assistant_name_cache = name_cache_state.assistant_names.lock().await;
    assistant_name_cache.insert(assistant_id, assistant.name.clone());

    Ok(CreateConversationResponse {
        conversation_id: conversation.id,
        user_message_id,
        system_message_id,
    })
}

#[tauri::command]
pub async fn list_conversations(
    app_handle: tauri::AppHandle,
    name_cache_state: tauri::State<'_, NameCacheState>,
    page: u32,
    page_size: u32,
) -> Result<Vec<ConversationResult>, AppError> {
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;

    let conversations =
        db.conversation_repo().unwrap().list(page, page_size).map_err(|e| e.to_string());

    let mut conversation_results = Vec::new();
    let assistant_name_cache = name_cache_state.assistant_names.lock().await.clone();
    if let Ok(conversations) = &conversations {
        for conversation in conversations {
            let assistant_name = assistant_name_cache.get(&conversation.assistant_id.unwrap());
            conversation_results.push(ConversationResult {
                id: conversation.id,
                name: conversation.name.clone(),
                assistant_id: conversation.assistant_id.unwrap_or(0),
                assistant_name: assistant_name.unwrap_or(&"未知".to_string()).clone(),
                created_time: conversation.created_time,
            });
        }
    }
    Ok(conversation_results)
}

#[tauri::command]
pub async fn get_conversation_with_messages(
    app_handle: tauri::AppHandle,
    name_cache_state: tauri::State<'_, NameCacheState>,
    conversation_id: i64,
) -> Result<ConversationWithMessages, String> {
    use std::time::Instant;
    let start_time = Instant::now();

    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // 查询 conversation
    let conv_query_start = Instant::now();
    let conversation = db
        .conversation_repo()
        .unwrap()
        .read(conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    let conv_query_duration = conv_query_start.elapsed();
    println!("[PERF] 查询 conversation 耗时: {:?}", conv_query_duration);

    // 查询 messages
    let msg_query_start = Instant::now();
    let messages = db
        .message_repo()
        .unwrap()
        .list_by_conversation_id(conversation_id)
        .map_err(|e| e.to_string())?;
    let msg_query_duration = msg_query_start.elapsed();
    println!("[PERF] 查询 messages 耗时: {:?}, 消息数量: {}", msg_query_duration, messages.len());

    let mut message_details: Vec<MessageDetail> = Vec::new();
    let mut attachment_map: HashMap<i64, Vec<MessageAttachment>> = HashMap::new();

    for (message, attachment) in messages.clone() {
        if let Some(attachment) = attachment {
            attachment_map.entry(message.id).or_default().push(attachment);
        }
    }

    // Convert messages to a HashMap to preserve it for the second pass
    let process_start = Instant::now();
    let message_map: HashMap<i64, Message> =
        messages.clone().into_iter().map(|(message, _)| (message.id, message)).collect();

    // Second pass: Create MessageDetail with the collected attachments
    for (message_id, message) in message_map {
        let attachment_list = attachment_map.get(&message_id).cloned().unwrap_or_default();
        message_details.push(MessageDetail {
            id: message.id,
            parent_id: message.parent_id,
            conversation_id: message.conversation_id,
            message_type: message.message_type,
            content: message.content,
            llm_model_id: message.llm_model_id,
            created_time: message.created_time,
            start_time: message.start_time,
            finish_time: message.finish_time,
            token_count: message.token_count,
            input_token_count: message.input_token_count,
            output_token_count: message.output_token_count,
            generation_group_id: message.generation_group_id,
            parent_group_id: message.parent_group_id,
            tool_calls_json: message.tool_calls_json,
            first_token_time: message.first_token_time,
            ttft_ms: message.ttft_ms,
            attachment_list,
            regenerate: Vec::new(),
        });
    }
    let process_duration = process_start.elapsed();
    println!("[PERF] 构建 MessageDetail 耗时: {:?}", process_duration);

    // 处理消息版本管理逻辑
    let version_start = Instant::now();
    let final_messages = process_message_versions(message_details);
    let version_duration = version_start.elapsed();
    println!("[PERF] 处理消息版本管理耗时: {:?}", version_duration);

    let assistant_name_cache = name_cache_state.assistant_names.lock().await;
    let assistant_name = assistant_name_cache
        .get(&conversation.assistant_id.unwrap_or(0))
        .cloned()
        .unwrap_or_else(|| "未知".to_string());

    let total_duration = start_time.elapsed();
    println!("[PERF] get_conversation_with_messages 总耗时: {:?}", total_duration);

    Ok(ConversationWithMessages {
        conversation: ConversationResult {
            id: conversation.id,
            name: conversation.name,
            assistant_id: conversation.assistant_id.unwrap_or(0),
            assistant_name,
            created_time: conversation.created_time,
        },
        messages: final_messages,
    })
}

#[tauri::command]
pub fn delete_conversation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<(), String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.conversation_repo().unwrap().delete(conversation_id).map_err(|e| e.to_string())?;

    // 发送删除事件通知前端更新列表
    let _ = app_handle.emit("conversation_deleted", conversation_id);

    Ok(())
}

#[tauri::command]
pub fn update_conversation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    name: String,
) -> Result<(), String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let mut conversation = db
        .conversation_repo()
        .unwrap()
        .read(conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;
    conversation.name = name.clone();
    db.conversation_repo().unwrap().update(&conversation).map_err(|e| e.to_string())?;

    let _ = app_handle.emit("title_change", (conversation_id, name));
    Ok(())
}

#[tauri::command]
pub fn update_message_content(
    app_handle: tauri::AppHandle,
    message_id: i64,
    content: String,
) -> Result<(), String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.message_repo().unwrap().update_content(message_id, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fork_conversation(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    message_id: i64,
) -> Result<i64, String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // 获取原对话信息
    let conversation_repo = db.conversation_repo().unwrap();
    let original_conversation = conversation_repo
        .read(conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;

    // 获取原对话的所有消息
    let message_repo = db.message_repo().unwrap();
    let all_messages_with_attachments =
        message_repo.list_by_conversation_id(conversation_id).map_err(|e| e.to_string())?;

    // 提取消息部分
    let all_messages: Vec<Message> =
        all_messages_with_attachments.iter().map(|(message, _)| message.clone()).collect();

    // 找到目标消息的位置
    let target_message_index = all_messages
        .iter()
        .position(|m| m.id == message_id)
        .ok_or_else(|| "Message not found".to_string())?;

    // 只复制到目标消息为止的消息
    let messages_to_copy = &all_messages[..=target_message_index];

    // 生成新对话标题（添加版本号）
    let version_pattern = regex::Regex::new(r"版本(\d+)$").unwrap();
    let mut version = 1;
    let base_name = if let Some(captures) = version_pattern.captures(&original_conversation.name) {
        version = captures.get(1).unwrap().as_str().parse::<i32>().unwrap_or(1) + 1;
        version_pattern.replace(&original_conversation.name, "").trim().to_string()
    } else {
        original_conversation.name.clone()
    };

    let new_conversation_name = format!("{} 版本{}", base_name, version);

    // 创建新对话
    let new_conversation = crate::db::conversation_db::Conversation {
        id: 0,
        name: new_conversation_name,
        assistant_id: original_conversation.assistant_id,
        created_time: chrono::Utc::now(),
    };

    let created_conversation =
        conversation_repo.create(&new_conversation).map_err(|e| e.to_string())?;

    // 复制消息到新对话
    for message in messages_to_copy {
        let mut new_message = message.clone();
        new_message.id = 0;
        new_message.conversation_id = created_conversation.id;
        new_message.created_time = chrono::Utc::now();

        message_repo.create(&new_message).map_err(|e| e.to_string())?;
    }

    Ok(created_conversation.id)
}

#[tauri::command]
pub async fn create_message(
    app_handle: tauri::AppHandle,
    markdown_text: String,
    conversation_id: i64,
) -> Result<crate::db::conversation_db::Message, String> {
    use crate::db::conversation_db::{ConversationDatabase, Message, Repository};
    use chrono::Utc;

    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let repo = db.message_repo().map_err(|e| e.to_string())?;

    let current_time = Utc::now();

    // Create new assistant message (not user message)
    let new_message = Message {
        id: 0, // Will be set by database
        parent_id: None,
        conversation_id,
        message_type: "assistant".to_string(), // Create assistant message for plugin responses
        content: markdown_text,
        llm_model_id: None,
        llm_model_name: None,
        created_time: current_time,
        start_time: Some(current_time),
        finish_time: Some(current_time), // Mark as completed immediately
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: None,
        parent_group_id: None,
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    };

    let created_message = repo.create(&new_message).map_err(|e| e.to_string())?;
    Ok(created_message)
}

#[tauri::command]
pub async fn update_assistant_message(
    app_handle: tauri::AppHandle,
    message_id: i64,
    markdown_text: String,
) -> Result<(), String> {
    use crate::db::conversation_db::{ConversationDatabase, Repository};

    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let repo = db.message_repo().map_err(|e| e.to_string())?;

    // First, verify the message exists and is an assistant message
    let existing_message = repo.read(message_id).map_err(|e| e.to_string())?;

    match existing_message {
        Some(message) => {
            // Only allow updating assistant messages
            if message.message_type != "assistant" {
                return Err(format!(
                    "Cannot update {} message. Only assistant messages can be updated.",
                    message.message_type
                ));
            }

            // Update message content
            repo.update_content(message_id, &markdown_text).map_err(|e| e.to_string())?;

            // Update finish time to mark when the update was completed
            repo.update_finish_time(message_id).map_err(|e| e.to_string())?;

            Ok(())
        }
        None => Err(format!("Message with ID {} not found", message_id)),
    }
}

#[tauri::command]
pub async fn search_conversations(
    app_handle: tauri::AppHandle,
    name_cache_state: tauri::State<'_, NameCacheState>,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<ConversationSearchHit>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let assistant_name_cache = name_cache_state.assistant_names.lock().await.clone();
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let search_value = format!("%{}%", trimmed);
    let max_rows = limit.unwrap_or(50).min(200);
    let offset_rows = offset.unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.name, c.assistant_id, c.created_time, \
                NULL as message_id, NULL as message_type, c.created_time as hit_time, \
                c.name as hit_text, 'title' as hit_type \
             FROM conversation c \
             WHERE c.name LIKE ?1 COLLATE NOCASE \
             UNION ALL \
             SELECT c.id, c.name, c.assistant_id, c.created_time, \
                NULL as message_id, NULL as message_type, cs.created_time as hit_time, \
                cs.summary as hit_text, 'summary' as hit_type \
             FROM conversation_summary cs \
             JOIN conversation c ON cs.conversation_id = c.id \
             WHERE cs.summary LIKE ?1 COLLATE NOCASE \
             UNION ALL \
             SELECT c.id, c.name, c.assistant_id, c.created_time, \
                m.id as message_id, m.message_type, m.created_time as hit_time, \
                m.content as hit_text, 'message' as hit_type \
             FROM message m \
             JOIN conversation c ON m.conversation_id = c.id \
             WHERE m.content LIKE ?1 COLLATE NOCASE \
             ORDER BY hit_time DESC \
             LIMIT ?2 OFFSET ?3",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map((&search_value, max_rows, offset_rows), |row| {
            let assistant_id: Option<i64> = row.get(2)?;
            let assistant_name = assistant_id
                .and_then(|id| assistant_name_cache.get(&id).cloned())
                .unwrap_or_else(|| "未知".to_string());

            let message_id: Option<i64> = row.get(4)?;
            let message_type: Option<String> = row.get(5)?;
            let hit_time: DateTime<Utc> = row.get(6)?;
            let hit_text: String = row.get(7)?;
            let hit_type: String = row.get(8)?;

            let conversation_id: i64 = row.get(0)?;
            let conversation_name: String = row.get(1)?;
            let _conversation_created_time: DateTime<Utc> = row.get(3)?;
            let snippet = build_snippet(&hit_text, trimmed, 120);

            Ok(ConversationSearchHit {
                conversation_id,
                conversation_name,
                assistant_name,
                message_id,
                message_type,
                created_time: hit_time,
                snippet,
                hit_type,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut hits: Vec<ConversationSearchHit> = Vec::new();
    for hit in rows {
        if let Ok(hit) = hit {
            hits.push(hit);
        }
    }

    Ok(hits)
}

fn build_snippet(text: &str, query: &str, max_len: usize) -> String {
    if text.is_empty() {
        return String::new();
    }

    let escaped_query = regex::escape(query);
    if let Ok(re) = regex::RegexBuilder::new(&escaped_query).case_insensitive(true).build() {
        if let Some(mat) = re.find(text) {
            let start_chars = text[..mat.start()].chars().count();
            let end_chars = text[..mat.end()].chars().count();
            let total_chars = text.chars().count();
            let snippet_start = start_chars.saturating_sub(40);
            let snippet_end = (end_chars + 40).min(total_chars);
            let mut snippet = slice_by_chars(text, snippet_start, snippet_end);
            if snippet_start > 0 {
                snippet = format!("...{}", snippet);
            }
            if snippet_end < total_chars {
                snippet.push_str("...");
            }
            return truncate_chars(&snippet, max_len);
        }
    }

    truncate_chars(text, max_len)
}

fn slice_by_chars(text: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }
    text.chars().skip(start).take(end - start).collect()
}

fn truncate_chars(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let total_chars = text.chars().count();
    if total_chars <= max_len {
        return text.to_string();
    }
    let mut snippet = text.chars().take(max_len).collect::<String>();
    snippet.push_str("...");
    snippet
}

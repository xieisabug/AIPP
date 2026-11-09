use crate::api::ai::context_manager::budget::ContextBudget;
use crate::db::conversation_db::{ConversationDatabase, ConversationTokenStats, MessageTokenStats};
use crate::db::token_stats::{self, AuxiliaryUsageInput, ContextBudgetInfo};
use tauri::{AppHandle, Manager};
use tracing::warn;

async fn context_budget_info(app_handle: &AppHandle) -> ContextBudgetInfo {
    let config = app_handle.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let budget = ContextBudget::from_config(&config);
    ContextBudgetInfo {
        enabled: budget.enabled,
        context_window_size: budget.context_window_size as i64,
        effective_input_limit: budget.effective_input_limit() as i64,
        compaction_trigger: budget.compaction_trigger() as i64,
    }
}

/// 获取对话的token统计信息
#[tauri::command]
pub async fn get_conversation_token_stats(
    app_handle: AppHandle,
    conversation_id: i64,
) -> Result<ConversationTokenStats, String> {
    let budget = context_budget_info(&app_handle).await;
    let db = ConversationDatabase::new(&app_handle)
        .map_err(|e| format!("读取对话 {conversation_id} 的用量统计失败：打开会话数据库出错：{e}"))?;
    db.get_conversation_token_stats(conversation_id, Some(budget))
        .map_err(|e| format!("读取对话 {conversation_id} 的用量统计失败：{e}"))
}

/// 获取管家群聊的token统计信息（成员执行会话 + 调度模型调用）
#[tauri::command]
pub async fn get_butler_group_token_stats(
    app_handle: AppHandle,
    group_id: i64,
) -> Result<ConversationTokenStats, String> {
    let db = ConversationDatabase::new(&app_handle)
        .map_err(|e| format!("读取群聊 {group_id} 的用量统计失败：打开会话数据库出错：{e}"))?;
    db.get_group_token_stats(group_id).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => format!("读取群聊 {group_id} 的用量统计失败：群聊不存在"),
        other => format!("读取群聊 {group_id} 的用量统计失败：{other}"),
    })
}

/// 获取单个消息的token统计信息
#[tauri::command]
pub async fn get_message_token_stats(
    app_handle: AppHandle,
    message_id: i64,
) -> Result<MessageTokenStats, String> {
    let db = ConversationDatabase::new(&app_handle)
        .map_err(|e| format!("读取消息 {message_id} 的用量失败：打开会话数据库出错：{e}"))?;
    db.get_message_token_stats(message_id)
        .map_err(|e| format!("读取消息 {message_id} 的用量失败：{e}"))
}

/// 把 genai 返回的用量换成辅助调用记录；口径与聊天回复一致（输入含缓存读写，输出含思考）。
/// 适配器没有返回任何用量时返回 None。
pub(crate) fn auxiliary_usage_from_genai(
    conversation_id: i64,
    purpose: &'static str,
    model_id: Option<i64>,
    model_name: &str,
    usage: &genai::chat::Usage,
) -> Option<AuxiliaryUsageInput> {
    if usage.prompt_tokens.is_none() && usage.completion_tokens.is_none() && usage.total_tokens.is_none() {
        return None;
    }
    let input = i64::from(usage.prompt_tokens.unwrap_or(0));
    let output = i64::from(usage.completion_tokens.unwrap_or(0));
    let total = usage.total_tokens.map(i64::from).unwrap_or(input + output);
    let mut metadata = crate::api::ai::chat::merge_chat_response_metadata(None, Some(usage), None, None)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    metadata["usage_source"] = serde_json::json!("reported");
    Some(AuxiliaryUsageInput {
        conversation_id,
        purpose,
        model_id,
        model_name: model_name.to_string(),
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
        metadata_json: Some(metadata.to_string()),
    })
}

/// 记录失败只影响统计，不能打断压缩或调度本身；但要留下可追溯的日志。
pub(crate) fn record_auxiliary_usage(db: &ConversationDatabase, usage: Option<AuxiliaryUsageInput>) {
    let Some(usage) = usage else {
        return;
    };
    let result = db
        .with_write_connection(|conn| token_stats::record_auxiliary_usage(conn, &usage).map_err(Into::into));
    if let Err(error) = result {
        warn!(
            conversation_id = usage.conversation_id,
            purpose = usage.purpose,
            model = %usage.model_name,
            error = %error,
            "failed to record auxiliary model usage"
        );
    }
}

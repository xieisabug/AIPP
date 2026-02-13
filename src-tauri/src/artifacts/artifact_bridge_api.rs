use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tauri::State;

use super::artifact_data_db::{ArtifactDataDatabase, ExecuteResult, QueryResult, TableInfo};
use crate::api::ai::config::{get_network_proxy_from_config, get_request_timeout_from_config};
use crate::api::genai_client;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::llm_db::LLMDatabase;
use crate::FeatureConfigState;

// ============================================
// 数据库操作相关
// ============================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbQueryRequest {
    pub db_id: String,
    pub sql: String,
    #[serde(default)]
    pub params: Vec<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbExecuteRequest {
    pub db_id: String,
    pub sql: String,
    #[serde(default)]
    pub params: Vec<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbBatchExecuteRequest {
    pub db_id: String,
    pub sql: String,
}

/// 执行 SQL 查询语句 (SELECT)
#[tauri::command]
pub fn artifact_db_query(
    app_handle: tauri::AppHandle,
    request: DbQueryRequest,
) -> Result<QueryResult, String> {
    let db = ArtifactDataDatabase::new(&app_handle, &request.db_id)?;
    db.query(&request.sql, request.params)
}

/// 执行 SQL 修改语句 (INSERT/UPDATE/DELETE/CREATE/DROP)
#[tauri::command]
pub fn artifact_db_execute(
    app_handle: tauri::AppHandle,
    request: DbExecuteRequest,
) -> Result<ExecuteResult, String> {
    let db = ArtifactDataDatabase::new(&app_handle, &request.db_id)?;
    db.execute(&request.sql, request.params)
}

/// 批量执行 SQL 语句（用于初始化表结构等）
#[tauri::command]
pub fn artifact_db_batch_execute(
    app_handle: tauri::AppHandle,
    request: DbBatchExecuteRequest,
) -> Result<(), String> {
    let db = ArtifactDataDatabase::new(&app_handle, &request.db_id)?;
    db.execute_batch(&request.sql)
}

/// 获取数据库中所有表的信息
#[tauri::command]
pub fn artifact_db_get_tables(
    app_handle: tauri::AppHandle,
    db_id: String,
) -> Result<Vec<TableInfo>, String> {
    let db = ArtifactDataDatabase::new(&app_handle, &db_id)?;
    db.get_tables()
}

/// 获取指定表的列信息
#[tauri::command]
pub fn artifact_db_get_columns(
    app_handle: tauri::AppHandle,
    db_id: String,
    table_name: String,
) -> Result<Vec<String>, String> {
    let db = ArtifactDataDatabase::new(&app_handle, &db_id)?;
    db.get_table_columns(&table_name)
}

/// 检查数据库是否存在
#[tauri::command]
pub fn artifact_db_exists(app_handle: tauri::AppHandle, db_id: String) -> Result<bool, String> {
    ArtifactDataDatabase::exists(&app_handle, &db_id)
}

/// 删除数据库
#[tauri::command]
pub fn artifact_db_delete(app_handle: tauri::AppHandle, db_id: String) -> Result<(), String> {
    ArtifactDataDatabase::delete(&app_handle, &db_id)
}

/// 列出所有 artifact 数据库
#[tauri::command]
pub fn artifact_db_list(app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    ArtifactDataDatabase::list_databases(&app_handle)
}

// ============================================
// AI 助手调用相关
// ============================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiAskRequest {
    pub assistant_id: i64,
    pub prompt: String,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiAskResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<AiUsage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AiUsage {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

/// 获取可用的助手列表（用于 artifact 选择）
#[tauri::command]
pub fn artifact_get_assistants(
    app_handle: tauri::AppHandle,
) -> Result<Vec<AssistantBasicInfo>, String> {
    let db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let assistants = db.get_assistants().map_err(|e| e.to_string())?;

    Ok(assistants
        .into_iter()
        .map(|a| AssistantBasicInfo {
            id: a.id,
            name: a.name,
            description: a.description.unwrap_or_default(),
            icon: "🤖".to_string(), // 默认图标
        })
        .collect())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssistantBasicInfo {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub icon: String,
}

/// Artifact 调用助手（非流式）
#[tauri::command]
pub async fn artifact_ai_ask(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    request: AiAskRequest,
) -> Result<AiAskResponse, String> {
    // 使用 assistant_api::get_assistant 获取完整的助手信息
    let assistant_detail = crate::api::assistant_api::get_assistant(app_handle.clone(), request.assistant_id)
        .map_err(|e| format!("Failed to get assistant: {}", e))?;

    // 获取模型信息
    let model = assistant_detail.model.first()
        .ok_or("Assistant has no model configured")?;

    let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    let model_detail = llm_db
        .get_llm_model_detail(&model.provider_id, &model.model_code)
        .map_err(|e| format!("Failed to get model detail: {}", e))?;

    // 获取网络配置
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let request_timeout = get_request_timeout_from_config(&config_feature_map);
    let proxy_enabled = false;

    // 创建 AI 客户端
    let client = genai_client::create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
        &config_feature_map,
    )
    .map_err(|e| format!("Failed to create AI client: {}", e))?;

    // 构建消息 - 从助手的 prompts 获取系统提示
    let assistant_prompt = assistant_detail.prompts.first()
        .map(|p| p.prompt.clone())
        .unwrap_or_else(|| "You are a helpful assistant.".to_string());

    let system_prompt = request.system_prompt.unwrap_or(assistant_prompt);

    let user_content = if let Some(ctx) = &request.context {
        format!("{}\n\nContext:\n{}", request.prompt, ctx)
    } else {
        request.prompt.clone()
    };

    let chat_request = crate::api::ai::conversation::build_chat_request_from_messages(
        &[
            ("system".to_string(), system_prompt, Vec::new()),
            ("user".to_string(), user_content, Vec::new()),
        ],
        crate::api::ai::conversation::ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;

    // 执行请求
    let response = client
        .exec_chat(&model_detail.model.code, chat_request, None)
        .await
        .map_err(|e| format!("AI request failed: {}", e))?;

    let content = response.first_text().unwrap_or("").to_string();

    // 提取 usage 信息
    let u = response.usage;
    let usage = Some(AiUsage {
        prompt_tokens: u.prompt_tokens.map(|t| t as i64),
        completion_tokens: u.completion_tokens.map(|t| t as i64),
        total_tokens: u.total_tokens.map(|t| t as i64),
    });

    Ok(AiAskResponse {
        content,
        model: model_detail.model.code,
        usage,
    })
}

// ============================================
// 配置获取
// ============================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ArtifactBridgeConfig {
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
    pub artifact_id: Option<i64>,
    pub artifact_name: Option<String>,
}

/// 获取 artifact 的配置（从 artifact collection 中读取）
#[tauri::command]
pub fn artifact_get_config(
    app_handle: tauri::AppHandle,
    artifact_id: i64,
) -> Result<ArtifactBridgeConfig, String> {
    use super::artifacts_db::ArtifactsDatabase;

    let db = ArtifactsDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let artifact = db
        .get_artifact_by_id(artifact_id)
        .map_err(|e| e.to_string())?
        .ok_or("Artifact not found")?;

    Ok(ArtifactBridgeConfig {
        db_id: artifact.db_id,
        assistant_id: artifact.assistant_id,
        artifact_id: Some(artifact.id),
        artifact_name: Some(artifact.name),
    })
}

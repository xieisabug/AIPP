use crate::api::ai::config::get_network_proxy_from_config;
use crate::api::genai_client;
use crate::db::llm_db::{http_provider_modes, resolve_request_mode_or_default, LLMDatabase};
use crate::utils::share_utils::{decrypt_provider_data, encrypt_provider_data, ProviderShareData};
use crate::FeatureConfigState;
use genai::Modality;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tauri::{Emitter, Manager};


fn emit_llm_provider_changed(app: &tauri::AppHandle) {
    let _ = app.emit("llm_provider_changed", ());
}

#[cfg(test)]
#[path = "llm_api_tests.rs"]
mod llm_api_tests;

#[derive(Serialize, Deserialize)]
pub struct LlmProvider {
    pub id: i64,
    pub name: String,
    pub api_type: String,
    pub description: String,
    pub is_official: bool,
    pub is_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmModel {
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub code: String,
    pub description: String,
    pub vision_support: bool,
    pub audio_support: bool,
    pub video_support: bool,
    pub request_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_modes: Option<Vec<String>>,
}

fn persisted_model(model: &LlmModel) -> crate::db::llm_db::LLMModel {
    crate::db::llm_db::LLMModel {
        id: model.id, name: model.name.clone(), llm_provider_id: model.llm_provider_id,
        code: model.code.clone(), description: model.description.clone(),
        vision_support: model.vision_support, audio_support: model.audio_support,
        video_support: model.video_support, request_mode: model.request_mode.clone(),
    }
}

#[derive(Serialize, Deserialize)]
pub struct LlmProviderConfig {
    pub id: i64,
    pub name: String,
    pub llm_provider_id: i64,
    pub value: String,
    pub append_location: Option<String>,
    pub is_addition: Option<bool>,
}

fn ordered_modes(default_mode: &str, all: &[&str]) -> Vec<String> {
    let mut modes = Vec::with_capacity(all.len());
    if all.contains(&default_mode) {
        modes.push(default_mode.to_string());
    }
    for &m in all {
        if m != default_mode {
            modes.push(m.to_string());
        }
    }
    modes
}

fn protocol_choice(db: &LLMDatabase, api_type: &str, provider_id: i64, code: &str, saved: Option<&String>) -> Result<(Option<Vec<String>>, String), String> {
    if api_type == "opencode_go" {
        let modes = crate::api::opencode_go::available_modes(db, code, saved.map(String::as_str)).map_err(|e| e.to_string())?;
        let request_mode = crate::api::opencode_go::selected_mode(&modes, saved);
        return Ok((Some(modes), request_mode));
    }
    if api_type == crate::api::magpie::PROVIDER {
        let modes = crate::api::magpie::available_modes(db, provider_id, code, saved.map(String::as_str)).map_err(|e| e.to_string())?;
        let request_mode = crate::api::magpie::selected_mode(&modes, saved);
        return Ok((Some(modes), request_mode));
    }
    let resolved = resolve_request_mode_or_default(api_type, code, saved.map(String::as_str)).to_string();
    let default_mode = crate::db::llm_db::default_request_mode_for_model(api_type, code);
    let available = http_provider_modes(api_type).map(|modes| ordered_modes(default_mode, modes));
    Ok((available, resolved))
}

fn provider_setting<'a>(configs: &'a [crate::db::llm_db::LLMProviderConfig], name: &str) -> Option<&'a str> {
    configs.iter().find(|config| config.name == name).map(|config| config.value.as_str())
}

pub(crate) fn validate_request_mode(db: &LLMDatabase, provider: &crate::db::llm_db::LLMProvider, code: &str, request_mode: &str) -> Result<(), String> {
    if provider.api_type == "opencode_go" {
        let saved = db.get_model_request_mode(provider.id, code).map_err(|e| e.to_string())?;
        let modes = crate::api::opencode_go::available_modes(db, code, saved.as_deref()).map_err(|e| e.to_string())?;
        if modes.contains(&request_mode.to_owned()) { return Ok(()); }
        return Err(format!("OpenCode Go 模型 {code} 不支持协议 {request_mode}，请刷新模型目录"));
    }
    if provider.api_type == crate::api::magpie::PROVIDER {
        return crate::api::magpie::mode(Some(request_mode)).map(|_| ()).ok_or_else(|| {
            format!("Magpie 模型 {code} 不支持协议 {request_mode}")
        });
    }
    let modes = http_provider_modes(&provider.api_type).unwrap_or(&["chat_completions", "responses"]);
    if modes.contains(&request_mode) {
        Ok(())
    } else {
        Err(format!("提供商 {} 不支持协议 {request_mode}，可选协议为 {}", provider.api_type, modes.join("、")))
    }
}

fn list_request_mode_map(
    db: &LLMDatabase,
    llm_provider_id: i64,
) -> Result<HashMap<String, String>, String> {
    Ok(db
        .list_model_request_modes(llm_provider_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect())
}

#[tauri::command]
pub async fn get_llm_providers(app_handle: tauri::AppHandle) -> Result<Vec<LlmProvider>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let providers = db.get_llm_providers().map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for (id, name, api_type, description, is_official, is_enabled) in providers {
        result.push(LlmProvider { id, name, api_type, description, is_official, is_enabled });
    }
    Ok(result)
}

/// 根据助手类型获取过滤后的提供商列表
/// Agent 助手 (assistant_type = 4): 返回 ACP 与原生 Agent 提供商
/// 普通助手: 排除 ACP 提供商
#[tauri::command]
pub async fn get_filtered_providers(
    app_handle: tauri::AppHandle,
    assistant_type: i64,
) -> Result<Vec<FilteredProvider>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let providers = db.get_filtered_providers(assistant_type).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for (id, name, api_type, description, is_official, is_enabled, tool_review_mode) in providers {
        result.push(FilteredProvider { provider: LlmProvider { id, name, api_type, description, is_official, is_enabled }, tool_review_mode });
    }
    Ok(result)
}

#[derive(Serialize)]
pub struct FilteredProvider {
    #[serde(flatten)]
    provider: LlmProvider,
    tool_review_mode: Option<String>,
}

#[tauri::command]
pub async fn add_llm_provider(
    app: tauri::AppHandle,
    name: String,
    api_type: String,
    onboarding_draft: Option<bool>,
) -> Result<i64, String> {
    let db = LLMDatabase::new(&app).map_err(|e| e.to_string())?;
    // 新建向导在创建后立即写入连接信息并获取模型列表，因此返回新 ID；完成向导时再启用。
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let id = db.add_llm_provider(&name, &api_type, "", false, false).map_err(|e| e.to_string())?;
    let draft = onboarding_draft.unwrap_or(false);
    if draft {
        db.mark_onboarding_draft(id).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    if !draft {
        crate::sync::schedule_sync_after_local_change(&app);
    }
    emit_llm_provider_changed(&app);
    Ok(id)
}

#[tauri::command]
pub async fn update_llm_provider(
    app_handle: tauri::AppHandle,
    id: i64,
    name: String,
    api_type: String,
    description: String,
    is_enabled: bool,
) -> Result<(), String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.update_llm_provider(id, &*name, &*api_type, &*description, is_enabled)
        .map_err(|e| e.to_string())?;
    confirm_vault_key(&app_handle, id);
    crate::sync::schedule_sync_after_local_change(&app_handle);
    emit_llm_provider_changed(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn delete_llm_provider(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
) -> Result<(), String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.delete_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    crate::sync::schedule_sync_after_local_change(&app_handle);
    emit_llm_provider_changed(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn get_llm_provider_config(
    app_handle: tauri::AppHandle,
    id: i64,
) -> Result<Vec<LlmProviderConfig>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let configs = db.get_llm_provider_config(id).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for config in configs {
        result.push(LlmProviderConfig {
            id: config.id,
            name: config.name,
            llm_provider_id: config.llm_provider_id,
            value: config.value,
            append_location: Some(config.append_location),
            is_addition: Some(config.is_addition),
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn update_llm_provider_config(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
    name: String,
    value: String,
) -> Result<(), String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    if name == "api_key" && crate::vault::config_ref::contains(&value) {
        crate::vault::config_ref::validate_provider_key(&app_handle, llm_provider_id, &value)?;
    }
    db.update_llm_provider_config(llm_provider_id, &*name, &*value).map_err(|e| e.to_string())?;
    match name.as_str() {
        "api_key" => crate::vault::config_ref::confirm_provider(&app_handle, llm_provider_id)?,
        "endpoint" => confirm_vault_key(&app_handle, llm_provider_id),
        _ => {}
    }
    if !db.is_onboarding_draft(llm_provider_id).unwrap_or(false) {
        crate::sync::schedule_sync_after_local_change(&app_handle);
    }
    emit_llm_provider_changed(&app_handle);
    Ok(())
}

/// Saving the provider in Settings confirms its current site for a vault-referenced API key. The
/// endpoint autosaves while being typed, so an unusable intermediate value is not an error here; a
/// request toward an unconfirmed site still fails with an explanation.
fn confirm_vault_key(app: &tauri::AppHandle, provider_id: i64) {
    if let Err(error) = crate::vault::config_ref::confirm_provider(app, provider_id) {
        tracing::warn!(provider_id, %error, "Vault reference not confirmed for the provider's current site");
    }
}

#[tauri::command]
pub async fn get_llm_models(
    app_handle: tauri::AppHandle,
    llm_provider_id: String,
) -> Result<Vec<LlmModel>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let models = db.get_llm_models(llm_provider_id).map_err(|e| e.to_string())?;
    let provider_id_num =
        models.first().map(|(_, _, llm_provider_id, _, _, _, _, _)| *llm_provider_id);
    let provider_api_type = if let Some(provider_id_num) = provider_id_num {
        Some(db.get_llm_provider(provider_id_num).map_err(|e| e.to_string())?.api_type)
    } else {
        None
    };
    let request_mode_map = if let Some(provider_id_num) = provider_id_num {
        list_request_mode_map(&db, provider_id_num)?
    } else {
        HashMap::new()
    };
    let mut result = Vec::new();
    for (
        id,
        name,
        llm_provider_id,
        code,
        description,
        vision_support,
        audio_support,
        video_support,
    ) in models
    {
        let saved = request_mode_map.get(&code);
        let api_type = provider_api_type.as_deref().unwrap_or("");
        let (available_modes, request_mode) = protocol_choice(&db, api_type, llm_provider_id, &code, saved)?;
        result.push(LlmModel {
            available_modes,
            id,
            name,
            llm_provider_id,
            code,
            description,
            vision_support,
            audio_support,
            video_support,
            request_mode,
        });
    }
    Ok(result)
}

/// Jev's built-in catalog is additive. Keep the saved row and its metadata for every existing code.
fn typesafe_catalog(db: &LLMDatabase, provider_id: i64) -> rusqlite::Result<Vec<LlmModel>> {
    let mut rows = db.get_llm_models(provider_id.to_string())?;
    for &(code, label) in crate::decision::TYPESAFE_JEV_PRESET_MODELS {
        if !rows.iter().any(|row| row.3 == code) {
            rows.push((0, label.into(), provider_id, code.into(),
                "TypeSafe System One 决策模型".into(), false, false, false));
        }
    }
    let mode = crate::decision::MODE_JEV_SYSTEMONE;
    Ok(rows.into_iter().map(|(id, name, llm_provider_id, code, description, vision_support, audio_support, video_support)| {
        LlmModel {
            id, name, llm_provider_id, code, description, vision_support, audio_support, video_support,
            request_mode: mode.into(), available_modes: Some(vec![mode.into()]),
        }
    }).collect())
}

fn sync_typesafe_models(db: &LLMDatabase, provider_id: i64) -> rusqlite::Result<Vec<LlmModel>> {
    let tx = rusqlite::Transaction::new_unchecked(&db.conn, rusqlite::TransactionBehavior::Immediate)?;
    let mut models = typesafe_catalog(db, provider_id)?;
    for model in models.iter_mut().filter(|model| model.id == 0) {
        db.add_llm_model(&model.name, provider_id, &model.code, &model.description,
            model.vision_support, model.audio_support, model.video_support)?;
        model.id = tx.last_insert_rowid();
        db.upsert_model_request_mode(provider_id, &model.code, &model.request_mode)?;
    }
    tx.commit()?;
    Ok(models)
}

#[tauri::command]
pub async fn fetch_model_list(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
) -> Result<Vec<LlmModel>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let llm_provider = db.get_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    if llm_provider.api_type.eq_ignore_ascii_case(crate::decision::PROVIDER_TYPESAFE_JEV) {
        let models = sync_typesafe_models(&db, llm_provider_id).map_err(|e| e.to_string())?;
        crate::sync::schedule_sync_after_local_change(&app_handle);
        emit_llm_provider_changed(&app_handle);
        return Ok(models);
    }
    let llm_provider_config =
        db.get_llm_provider_config(llm_provider_id).map_err(|e| e.to_string())?;
    let request_mode_map = list_request_mode_map(&db, llm_provider_id)?;

    // 获取代理配置
    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let proxy_enabled = llm_provider_config
        .iter()
        .find(|config| config.name == "proxy_enabled")
        .and_then(|config| config.value.parse::<bool>().ok())
        .unwrap_or(false);
    let effective_network_proxy = proxy_enabled.then_some(network_proxy).flatten();

    if llm_provider.api_type == "opencode_go" || llm_provider.api_type == crate::api::magpie::PROVIDER {
        drop(config_feature_map);
        let models = if llm_provider.api_type == crate::api::magpie::PROVIDER {
            crate::api::magpie::catalog(&app_handle, llm_provider_id,
                provider_setting(&llm_provider_config, "endpoint"),
                provider_setting(&llm_provider_config, "api_key"),
                effective_network_proxy.as_deref()).await?
        } else {
            crate::api::opencode_go::catalog(&app_handle, llm_provider_id, effective_network_proxy.as_deref()).await?
        };
        db.replace_llm_models(llm_provider_id, &models.iter().map(persisted_model).collect::<Vec<_>>(), true).map_err(|e| e.to_string())?;
        crate::sync::schedule_sync_after_local_change(&app_handle);
        emit_llm_provider_changed(&app_handle);
        return Ok(models);
    }

    // 对 Copilot 类型进行 token exchange，替换 api_key/endpoint
    let effective_configs = crate::api::copilot_token_manager::prepare_provider_configs(
        &app_handle,
        &llm_provider.api_type,
        &llm_provider_config,
        effective_network_proxy.as_deref(),
    )
    .await?;

    // 使用共用的客户端创建函数
    let client = genai_client::create_client_with_config(
        None,
        &effective_configs,
        "",
        &llm_provider.api_type,
        None,
        effective_network_proxy.as_deref(),
        proxy_enabled,
        None,
        false,
        &config_feature_map,
    )
    .map_err(|e| e.to_string())?;

    let adapter_kind = genai_client::infer_adapter_kind_simple(&llm_provider.api_type);

    match client.all_models(adapter_kind, ()).await {
        Ok(models) => {
            let mut result = Vec::new();
            for model in &models {
                let code = model.id.to_string();
                let (available_modes, request_mode) = protocol_choice(
                    &db, &llm_provider.api_type, llm_provider_id, &code, request_mode_map.get(&code),
                )?;
                let model = LlmModel {
                    available_modes,
                    id: 0,
                    name: model.name.to_string(),
                    llm_provider_id,
                    code,
                    description: format!("Model: {}", model.name),
                    vision_support: model.supports_input_modality(&Modality::Image),
                    audio_support: model.supports_input_modality(&Modality::Audio),
                    video_support: model.supports_input_modality(&Modality::Video),
                    request_mode,
                };

                result.push(model);
            }

            db.replace_llm_models(llm_provider_id, &result.iter().map(persisted_model).collect::<Vec<_>>(), false)
                .map_err(|e| e.to_string())?;
            crate::sync::schedule_sync_after_local_change(&app_handle);
            emit_llm_provider_changed(&app_handle);
            Ok(result)
        }
        Err(e) => {
            tracing::error!(error = %e, "获取模型列表错误");
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn add_llm_model(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
    code: String,
) -> Result<LlmModel, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let llm_provider = db.get_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    let code_str = code.clone();
    let saved = db.get_model_request_mode(llm_provider_id, &code).map_err(|e| e.to_string())?;
    let (available_modes, request_mode) = protocol_choice(&db, &llm_provider.api_type, llm_provider_id, &code, saved.as_ref())?;
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    db.add_llm_model(&code_str, llm_provider_id, &code_str, &code_str, false, false, false)
        .map_err(|e| e.to_string())?;
    if available_modes.is_some() && request_mode != "unsupported" {
        db.upsert_model_request_mode(llm_provider_id, &code, &request_mode).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    crate::sync::schedule_sync_after_local_change(&app_handle);
    emit_llm_provider_changed(&app_handle);
    Ok(LlmModel {
        available_modes,
        id: 0,
        name: code.clone(),
        llm_provider_id,
        code,
        description: code_str,
        vision_support: false,
        audio_support: false,
        video_support: false,
        request_mode,
    })
}

#[tauri::command]
pub async fn delete_llm_model(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
    code: String,
) -> Result<(), String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.delete_llm_model(llm_provider_id, code).map_err(|e| e.to_string())?;
    crate::sync::schedule_sync_after_local_change(&app_handle);
    emit_llm_provider_changed(&app_handle);
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct ModelForSelect {
    name: String,
    code: String,
    id: i64,
    llm_provider_id: i64,
    #[serde(default)]
    request_mode: String,
    #[serde(default)]
    is_decision: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ModelForSelection {
    pub name: String,
    pub code: String,
    pub description: String,
    pub vision_support: bool,
    pub audio_support: bool,
    pub video_support: bool,
    pub request_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_modes: Option<Vec<String>>,
    pub is_selected: bool, // 是否已在数据库中存在
}

#[derive(Serialize, Deserialize)]
pub struct ModelSelectionResponse {
    pub available_models: Vec<ModelForSelection>,
    pub missing_models: Vec<String>, // 在数据库中但远程不存在的模型
}

#[derive(Debug, Serialize)]
pub struct ProviderCheckError {
    pub kind: &'static str,
    pub message: String,
}

impl From<String> for ProviderCheckError {
    fn from(message: String) -> Self { classify_provider_error(None, message) }
}

fn classify_provider_error(status: Option<u16>, message: String) -> ProviderCheckError {
    let lower = message.to_lowercase();
    let kind = match status {
        Some(402) => "insufficient_balance",
        _ if lower.contains("insufficient balance")
            || lower.contains("insufficient_balance")
            || lower.contains("insufficient_quota")
            || lower.contains("exceeded your current quota")
            || lower.contains("credit balance is too low")
            || lower.contains("余额不足") => "insufficient_balance",
        Some(401 | 403) => "auth",
        Some(404) => "model_not_found",
        Some(429) => "rate_limited",
        Some(408 | 504) => "timeout",
        _ if lower.contains("timed out") || lower.contains("timeout") || lower.contains("超时") => "timeout",
        _ if lower.contains("connect") || lower.contains("dns") || lower.contains("网络连接") => "network",
        _ => "other",
    };
    ProviderCheckError { kind, message }
}

fn classify_genai_error(error: genai::Error) -> ProviderCheckError {
    classify_provider_error(error.status().map(|s| s.as_u16()), error.to_string())
}

/// OpenCode Go binds each request to a persistent conversation identity. Use one
/// device-local hidden conversation for setup probes; it contains no user messages.
fn onboarding_probe_conversation(conn: &rusqlite::Connection) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO conversation(name,conversation_kind,is_hidden_from_normal_chat_list)
        SELECT '模型连接检查','onboarding_probe',1 WHERE NOT EXISTS
        (SELECT 1 FROM conversation WHERE conversation_kind='onboarding_probe')", [])?;
    conn.query_row("SELECT id FROM conversation WHERE conversation_kind='onboarding_probe' ORDER BY id LIMIT 1", [], |r| r.get(0))
}

async fn probe_request(client: &genai_client::Client, code: &str) -> Result<(), ProviderCheckError> {
    use genai::chat::{ChatMessage, ChatOptions, ChatRequest};
    let request = ChatRequest::new(vec![ChatMessage::user("hi")]);
    let options = ChatOptions::default().with_max_tokens(32);
    client.exec_chat(code, request, Some(&options)).await.map(|_| ()).map_err(classify_genai_error)
}

#[derive(Serialize)]
pub struct ProbeResult {
    ok: bool,
    kind: &'static str,
    message: String,
    latency_ms: u64,
}

#[tauri::command]
pub async fn probe_chat_model(app: tauri::AppHandle, provider_id: i64, model_code: String) -> ProbeResult {
    let started = std::time::Instant::now();
    let probe = async {
        let db = LLMDatabase::new(&app).map_err(|e| e.to_string())?;
        let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
        if crate::db::llm_db::is_local_agent_api_type(&provider.api_type) {
            return Err(ProviderCheckError::from("本机智能体需通过对应通道检查".to_owned()));
        }
        let configs = db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;
        let saved = db.get_model_request_mode(provider_id, &model_code).map_err(|e| e.to_string())?;
        let (_, mode) = protocol_choice(&db, &provider.api_type, provider_id, &model_code, saved.as_ref())?;
        let features = app.state::<FeatureConfigState>().config_feature_map.lock().await.clone();
        let use_proxy = provider_setting(&configs, "proxy_enabled") == Some("true");
        let proxy = get_network_proxy_from_config(&features).filter(|_| use_proxy);
        let prepared = crate::api::copilot_token_manager::prepare_provider_configs(
            &app, &provider.api_type, &configs, proxy.as_deref()).await?;
        let session = if provider.api_type == crate::api::opencode_go::PROVIDER {
            let db = crate::db::conversation_db::ConversationDatabase::new(&app).map_err(|e| e.to_string())?;
            let connection = db.get_connection().map_err(|e| e.to_string())?;
            Some((&app, onboarding_probe_conversation(&connection).map_err(|e| e.to_string())?))
        } else { None };
        let client = genai_client::create_client_with_config(session, &prepared, &model_code, &provider.api_type,
            Some(&mode), proxy.as_deref(), use_proxy, Some(15), false, &features).map_err(|e| e.to_string())?;
        probe_request(&client, &model_code).await
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(15), probe).await
        .unwrap_or_else(|_| Err(ProviderCheckError { kind: "timeout", message: "模型试调用超过 15 秒".into() }));
    let latency_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(()) => ProbeResult { ok: true, kind: "ok", message: String::new(), latency_ms },
        Err(e) => ProbeResult { ok: false, kind: e.kind, message: e.message, latency_ms },
    }
}

#[tauri::command]
pub async fn preview_model_list(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
) -> Result<ModelSelectionResponse, ProviderCheckError> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let llm_provider = db.get_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    if llm_provider.api_type.eq_ignore_ascii_case(crate::decision::PROVIDER_TYPESAFE_JEV) {
        let models = typesafe_catalog(&db, llm_provider_id).map_err(|e| e.to_string())?;
        let available_models = models.into_iter().map(|m| ModelForSelection {
            is_selected: m.id != 0, code: m.code, name: m.name, description: m.description,
            vision_support: m.vision_support, audio_support: m.audio_support, video_support: m.video_support,
            request_mode: m.request_mode, available_modes: m.available_modes,
        }).collect();
        return Ok(ModelSelectionResponse { available_models, missing_models: Vec::new() });
    }
    let llm_provider_config =
        db.get_llm_provider_config(llm_provider_id).map_err(|e| e.to_string())?;

    // 获取现有的模型列表
    let existing_models =
        db.get_llm_models(llm_provider_id.to_string()).map_err(|e| e.to_string())?;
    let existing_model_codes: HashSet<String> =
        existing_models.iter().map(|(_, _, _, code, _, _, _, _)| code.clone()).collect();
    let request_mode_map = list_request_mode_map(&db, llm_provider_id)?;

    // 获取代理配置
    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let proxy_enabled = llm_provider_config
        .iter()
        .find(|config| config.name == "proxy_enabled")
        .and_then(|config| config.value.parse::<bool>().ok())
        .unwrap_or(false);
    let effective_network_proxy = proxy_enabled.then_some(network_proxy).flatten();
    tracing::info!(
        llm_provider_id,
        network_proxy = ?effective_network_proxy,
        proxy_enabled,
        "preview_model_list proxy config"
    );

    if llm_provider.api_type == "opencode_go" || llm_provider.api_type == crate::api::magpie::PROVIDER {
        let models = if llm_provider.api_type == crate::api::magpie::PROVIDER {
            crate::api::magpie::catalog(&app_handle, llm_provider_id,
                provider_setting(&llm_provider_config, "endpoint"),
                provider_setting(&llm_provider_config, "api_key"),
                effective_network_proxy.as_deref()).await?
        } else {
            crate::api::opencode_go::catalog(&app_handle, llm_provider_id, effective_network_proxy.as_deref()).await?
        };
        let mut missing = existing_model_codes.clone();
        let available_models = models.into_iter().map(|m| {
            let is_selected = missing.remove(&m.code);
            ModelForSelection { code: m.code, name: m.name, description: m.description,
                vision_support: m.vision_support, audio_support: m.audio_support, video_support: m.video_support,
                request_mode: m.request_mode, available_modes: m.available_modes, is_selected }
        }).collect();
        return Ok(ModelSelectionResponse { available_models, missing_models: missing.into_iter().collect() });
    }

    // 对 Copilot 类型进行 token exchange，替换 api_key/endpoint
    let effective_configs = crate::api::copilot_token_manager::prepare_provider_configs(
        &app_handle,
        &llm_provider.api_type,
        &llm_provider_config,
        effective_network_proxy.as_deref(),
    )
    .await?;

    // 使用共用的客户端创建函数
    let client = genai_client::create_client_with_config(
        None,
        &effective_configs,
        "",
        &llm_provider.api_type,
        None,
        effective_network_proxy.as_deref(),
        proxy_enabled,
        Some(15),
        false,
        &config_feature_map,
    )
    .map_err(|e| e.to_string())?;

    let adapter_kind = genai_client::infer_adapter_kind_simple(&llm_provider.api_type);
    tracing::info!(llm_provider_id, "preview_model_list with adapter_kind: {:?}", adapter_kind);

    match client.all_models(adapter_kind, ()).await {
        Ok(models) => {
            let mut available_models = Vec::new();
            let remote_model_codes: HashSet<String> =
                models.iter().map(|model| model.id.to_string()).collect();

            // 构建可选择的模型列表
            for model in &models {
                let model_code = model.id.to_string();
                let is_selected = existing_model_codes.contains(&model_code);
                let saved = request_mode_map.get(&model_code);
                let (available_modes, request_mode) = protocol_choice(
                    &db,
                    &llm_provider.api_type,
                    llm_provider_id,
                    &model_code,
                    saved,
                )?;

                available_models.push(ModelForSelection {
                    available_modes,
                    name: model.name.to_string(),
                    code: model_code,
                    description: format!("Model: {}", model.name),
                    vision_support: model.supports_input_modality(&Modality::Image),
                    audio_support: model.supports_input_modality(&Modality::Audio),
                    video_support: model.supports_input_modality(&Modality::Video),
                    request_mode,
                    is_selected,
                });
            }

            // 找出在数据库中但远程不存在的模型
            let missing_models: Vec<String> =
                existing_model_codes.difference(&remote_model_codes).cloned().collect();

            Ok(ModelSelectionResponse { available_models, missing_models })
        }
        Err(e) => {
            Err(classify_genai_error(e))
        }
    }
}

#[tauri::command]
pub fn get_models_for_select(
    app_handle: tauri::AppHandle,
    include_decision: Option<bool>,
) -> Result<Vec<ModelForSelect>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let result = db.get_models_for_select_with_mode(include_decision.unwrap_or(false))?;
    let models = result
        .into_iter()
        .map(|(name, code, id, llm_provider_id, request_mode, is_decision)| ModelForSelect {
            name,
            code,
            id,
            llm_provider_id,
            request_mode,
            is_decision,
        })
        .collect();
    Ok(models)
}

/// 根据助手类型获取过滤后的模型列表
/// ACP 助手 (assistant_type = 4): 只返回 ACP 提供商的模型
/// 普通助手: 排除 ACP 与决策模型
#[tauri::command]
pub fn get_filtered_models_for_select(
    app_handle: tauri::AppHandle,
    assistant_type: i64,
) -> Result<Vec<ModelForSelect>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let result = db.get_filtered_models_for_select(assistant_type).map_err(|e| e.to_string())?;
    let models = result
        .iter()
        .map(|(name, code, id, llm_provider_id)| ModelForSelect {
            name: name.clone(),
            code: code.clone(),
            id: *id,
            llm_provider_id: *llm_provider_id,
            request_mode: String::new(),
            is_decision: false,
        })
        .collect();
    Ok(models)
}

/// 决策模型连通性探针：验证端点、鉴权与协议是否匹配（兼容 TypeSafe / OpenAI / Gemini / New API）。
#[tauri::command]
pub async fn test_decision_model(
    app_handle: tauri::AppHandle,
    provider_id: i64,
    model_code: String,
    request_mode_override: Option<String>,
) -> Result<crate::decision::DecisionProbeResult, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let detail = db
        .get_llm_model_detail(&provider_id, &model_code)
        .map_err(|e| format!("读取模型配置失败（provider={provider_id}, model={model_code}）：{e}"))?;
    let effective_mode = request_mode_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&detail.model.request_mode);
    validate_request_mode(&db, &detail.provider, &model_code, effective_mode)?;
    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let prepared_configs = crate::api::copilot_token_manager::prepare_provider_configs(
        &app_handle,
        &detail.provider.api_type,
        &detail.configs,
        network_proxy.as_deref(),
    )
    .await?;

    crate::decision::probe_decision_model(
        &detail.provider.api_type,
        &prepared_configs,
        effective_mode,
        &detail.model.code,
        network_proxy.as_deref(),
        &config_feature_map,
    )
    .await
}

#[tauri::command]
pub async fn update_selected_models(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
    selected_models: Vec<ModelForSelection>,
) -> Result<(), String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let llm_provider = db.get_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    let supports_request_mode = genai_client::supports_model_request_mode(&llm_provider.api_type);

    if supports_request_mode {
        for model in selected_models.iter().filter(|m| m.is_selected) {
            validate_request_mode(&db, &llm_provider, &model.code, &model.request_mode)?;
        }
    }

    let models = selected_models.iter().filter(|model| model.is_selected).map(|model| crate::db::llm_db::LLMModel {
        id: 0, name: model.name.clone(), llm_provider_id,
        code: model.code.clone(), description: model.description.clone(),
        vision_support: model.vision_support, audio_support: model.audio_support,
        video_support: model.video_support,
        request_mode: model.request_mode.clone(),
    }).collect::<Vec<_>>();
    db.replace_llm_models(llm_provider_id, &models, supports_request_mode).map_err(|e| e.to_string())?;

    crate::sync::schedule_sync_after_local_change(&app_handle);
    emit_llm_provider_changed(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn update_llm_model_request_mode(
    app_handle: tauri::AppHandle,
    llm_provider_id: i64,
    model_code: String,
    request_mode: String,
) -> Result<(), String> {

    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let llm_provider = db.get_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    if !genai_client::supports_model_request_mode(&llm_provider.api_type) {
        return Err(format!(
            "Provider api_type={} does not support model request mode switching",
            llm_provider.api_type
        ));
    }

    validate_request_mode(&db, &llm_provider, &model_code, &request_mode)?;
    db.upsert_model_request_mode(
        llm_provider_id,
        &model_code,
        &request_mode,
    )
    .map_err(|e| e.to_string())?;
    crate::sync::schedule_sync_after_local_change(&app_handle);
    emit_llm_provider_changed(&app_handle);
    Ok(())
}

// Share and Import LLM Provider Commands

#[tauri::command]
pub async fn export_llm_provider(
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    provider_id: i64,
    password: String,
) -> Result<Option<String>, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Get provider information
    let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
    let configs = db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;

    // Extract endpoint and api_key from configs
    let mut endpoint = None;
    let mut api_key = String::new();

    for config in configs {
        match config.name.as_str() {
            "endpoint" | "base_url" => endpoint = Some(config.value),
            "api_key" => api_key = config.value,
            _ => {}
        }
    }

    if api_key.is_empty() {
        return Err("API Key is required for export".to_string());
    }
    let share_data =
        ProviderShareData { name: provider.name, api_type: provider.api_type, endpoint, api_key };
    if !crate::vault::config_ref::contains(&share_data.api_key) {
        return encrypt_provider_data(&share_data, &password).map(Some).map_err(|e| e.to_string());
    }
    if cfg!(desktop) && window.label() != "config" {
        return Err("请在设置的模型提供商页面分享保险箱凭据".into());
    }
    if password.encode_utf16().count() < 6 {
        return Err("分享密码至少需要 6 个字符".into());
    }
    tokio::task::spawn_blocking(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        crate::vault::config_ref::export_provider_share(&app_handle, share_data, &password, |message| {
            let dialog = app_handle.dialog().message(message).title("确认分享保险箱凭据")
                .buttons(MessageDialogButtons::OkCancelCustom("包含 API Key 并生成分享码".into(), "取消".into()));
            #[cfg(desktop)]
            let dialog = dialog.parent(&window);
            dialog.blocking_show()
        })
    }).await.map_err(|e| format!("生成保险箱凭据分享码失败：{e}"))?
}

#[derive(Serialize)]
pub struct ImportedProvider {
    #[serde(flatten)]
    provider: LlmProvider,
    already_exists: bool,
}

fn import_provider_record(db: &LLMDatabase, data: ProviderShareData, new_name: Option<String>, draft: bool) -> Result<ImportedProvider, String> {
    let tx = rusqlite::Transaction::new_unchecked(&db.conn, rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
    let incoming_endpoint = data.endpoint.as_deref().unwrap_or("").trim().trim_end_matches('/');
    let incoming_key = data.api_key.trim();
    for (id, name, api_type, description, is_official, is_enabled) in db.get_llm_providers().map_err(|e| e.to_string())? {
        if api_type != data.api_type { continue; }
        let configs = db.get_llm_provider_config(id).map_err(|e| e.to_string())?;
        let endpoint = provider_setting(&configs, "endpoint").or_else(|| provider_setting(&configs, "base_url")).unwrap_or("").trim().trim_end_matches('/');
        if endpoint == incoming_endpoint
            && provider_setting(&configs, "api_key").map(str::trim) == Some(incoming_key) {
            let mut enabled = is_enabled;
            if !draft && !enabled {
                db.update_llm_provider(id, &name, &api_type, &description, true).map_err(|e| e.to_string())?;
                db.release_onboarding_draft(id).map_err(|e| e.to_string())?;
                tx.commit().map_err(|e| e.to_string())?;
                enabled = true;
            }
            return Ok(ImportedProvider { provider: LlmProvider { id, name, api_type, description, is_official, is_enabled: enabled }, already_exists: true });
        }
    }
    let name = new_name.unwrap_or_else(|| format!("{} (导入)", data.name));
    let description = "Imported provider";
    let id = db.add_llm_provider(&name, &data.api_type, description, false, !draft).map_err(|e| e.to_string())?;
    if let Some(endpoint) = data.endpoint {
        db.add_llm_provider_config(id, "endpoint", endpoint.trim(), "header", false).map_err(|e| e.to_string())?;
    }
    db.add_llm_provider_config(id, "api_key", incoming_key, "header", false).map_err(|e| e.to_string())?;
    if draft { db.mark_onboarding_draft(id).map_err(|e| e.to_string())?; }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ImportedProvider { provider: LlmProvider { id, name, api_type: data.api_type, description: description.into(), is_official: false, is_enabled: !draft }, already_exists: false })
}

#[tauri::command]
pub async fn import_llm_provider(
    app_handle: tauri::AppHandle,
    share_code: String,
    password: String,
    new_name: Option<String>,
    as_draft: Option<bool>,
) -> Result<ImportedProvider, String> {
    let data = decrypt_provider_data(&share_code, &password).map_err(|e| e.to_string())?;
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let draft = as_draft.unwrap_or(false);
    let result = import_provider_record(&db, data, new_name, draft)?;
    if !draft && (!result.already_exists || result.provider.is_enabled) {
        crate::sync::schedule_sync_after_local_change(&app_handle);
        emit_llm_provider_changed(&app_handle);
    } else if !result.already_exists {
        emit_llm_provider_changed(&app_handle);
    }
    Ok(result)
}

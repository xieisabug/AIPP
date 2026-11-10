//! AIPP's Magpie configuration and persistence; provider transport lives in genai.
use super::llm_api::LlmModel;
use crate::db::llm_db::LLMDatabase;
use genai::providers::magpie::{self as provider, ChatModel, Gateway};
pub use genai::providers::magpie::{mode, TEXT_PROTOCOLS};
use rusqlite::params;
use std::collections::HashMap;

pub const PROVIDER: &str = "magpie";
pub const BUILTIN_SYNC_ID: &str = "official:llm.provider:magpie";
pub const APP_KEY: &str = "magpie-aipp";
pub const USER_AGENT: &str = concat!("AIPP/", env!("CARGO_PKG_VERSION"));

pub fn selected_mode(modes: &[String], saved: Option<&String>) -> String {
    saved.filter(|s| modes.contains(s)).or_else(|| modes.first())
        .cloned().unwrap_or_else(|| "chat_completions".into())
}

pub fn api_key_or_default(raw: Option<&str>) -> String {
    let trimmed = raw.unwrap_or("").trim();
    if trimmed.is_empty() { APP_KEY.to_string() } else { trimmed.to_string() }
}

fn into_models(models: Vec<ChatModel>, provider_id: i64, saved: &HashMap<String, String>) -> Vec<LlmModel> {
    models.into_iter().map(|model| LlmModel {
        id: 0, llm_provider_id: provider_id,
        request_mode: selected_mode(&model.available_modes, saved.get(&model.id)),
        code: model.id, name: model.name, description: model.owner,
        vision_support: model.vision_support, audio_support: model.audio_support,
        video_support: model.video_support, available_modes: Some(model.available_modes),
    }).collect()
}

#[cfg(test)]
fn parse_catalog(body: &str, provider_id: i64, saved: &HashMap<String, String>) -> Result<Vec<LlmModel>, String> {
    Ok(into_models(provider::parse_catalog(body)?, provider_id, saved))
}

pub fn available_modes(db: &LLMDatabase, provider_id: i64, code: &str, _saved: Option<&str>) -> rusqlite::Result<Vec<String>> {
    let mut stmt = db.conn.prepare(
        "SELECT mode FROM magpie_protocols WHERE provider_id=?1 AND model=?2 ORDER BY position")?;
    let rows = stmt.query_map(params![provider_id, code], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.is_empty() {
        return Ok(TEXT_PROTOCOLS.iter().map(|mode| (*mode).to_string()).collect());
    }
    Ok(rows.into_iter().filter(|mode| self::mode(Some(mode)).is_some()).collect())
}

pub async fn catalog(app: &tauri::AppHandle, provider_id: i64, endpoint: Option<&str>, api_key: Option<&str>, proxy: Option<&str>) -> Result<Vec<LlmModel>, String> {
    let resolved = crate::api::genai_client::resolve_provider_endpoint(endpoint).map_err(|e| e.to_string())?;
    let endpoint = match resolved {
        crate::api::genai_client::ProviderEndpointResolution::Unset => None,
        crate::api::genai_client::ProviderEndpointResolution::Custom(url) => Some(url),
    };
    let key = api_key_or_default(api_key);
    // Local settings discovery performs filesystem IO outside the async worker.
    let gateway = tokio::task::spawn_blocking(move || Gateway::new(endpoint.as_deref(), &key)).await
        .map_err(|e| format!("Magpie 网关配置加载失败: {e}"))??;
    let mut config = genai::WebConfig::default().with_timeout(std::time::Duration::from_secs(30));
    if !provider::origin_is_loopback(gateway.origin()) {
        if let Some(proxy) = proxy.filter(|value| !value.is_empty()) {
            config = config.with_all_proxy_url(proxy).map_err(|e| format!("Magpie 代理 {proxy} 无效: {e}"))?;
        }
    }
    let catalog = gateway.catalog(config, USER_AGENT).await?;
    let db = LLMDatabase::new(app).map_err(|e| e.to_string())?;
    let saved: HashMap<_, _> = db.list_model_request_modes(provider_id).map_err(|e| e.to_string())?.into_iter().collect();
    let models = into_models(catalog, provider_id, &saved);
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM magpie_protocols WHERE provider_id=?1", [provider_id]).map_err(|e| e.to_string())?;
    for model in &models {
        let modes = model.available_modes.as_deref().unwrap_or_default();
        for (position, mode) in modes.iter().enumerate() {
            tx.execute("INSERT INTO magpie_protocols (provider_id,model,mode,position) VALUES (?1,?2,?3,?4)",
                params![provider_id, model.code, mode, position as i64]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(models)
}

#[cfg(test)]
#[path = "magpie_tests.rs"]
mod tests;

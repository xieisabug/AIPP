//! Automatic review output preferences and local endpoint capability memory.
use crate::db::llm_db::{LLMDatabase, ModelDetail};
use crate::db::system_db::SystemDatabase;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OutputCapability {
    pub generation: u64,
    pub prompt: bool,
    pub reason: String,
}

pub fn capability_key(detail: &ModelDetail) -> String {
    let endpoint = detail.configs.iter().find(|c| c.name == "endpoint")
        .map(|c| c.value.trim().trim_end_matches('/')).unwrap_or("");
    let identity = serde_json::json!([
        detail.provider.id, detail.provider.api_type, detail.model.code,
        detail.model.request_mode, endpoint
    ]);
    format!("tool_review_output_{:x}", Sha256::digest(identity.to_string().as_bytes()))
}

pub fn load_capability(db: &SystemDatabase, key: &str) -> Result<OutputCapability, String> {
    let value = db.get_config(key).map_err(|e| format!("读取审核输出检测记录失败: {e}"))?;
    if value.is_empty() { return Ok(OutputCapability::default()); }
    serde_json::from_str(&value).map_err(|e| format!("解析审核输出检测记录失败: {e}"))
}

// Compare-and-swap prevents an in-flight request from undoing a user's reset.
pub fn remember_prompt(db: &SystemDatabase, key: &str, generation: u64, reason: &str) -> Result<(), String> {
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let current = load_capability(db, key)?;
    if current.generation == generation {
        let value = OutputCapability { generation, prompt: true, reason: reason.to_string() };
        write_capability(db, key, &value)?;
    }
    tx.commit().map_err(|e| format!("保存审核输出检测记录失败: {e}"))
}

fn write_capability(db: &SystemDatabase, key: &str, value: &OutputCapability) -> Result<(), String> {
    let value = serde_json::to_string(value).map_err(|e| e.to_string())?;
    db.conn.execute(
        "INSERT INTO system_config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![key, value],
    ).map_err(|e| format!("保存审核输出检测记录失败: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_tool_review_output_capability(app_handle: tauri::AppHandle, provider_id: i64, model_code: String) -> Result<OutputCapability, String> {
    let detail = LLMDatabase::new(&app_handle).and_then(|db| db.get_llm_model_detail(&provider_id, &model_code))
        .map_err(|e| format!("读取审核模型配置失败: {e}"))?;
    load_capability(&SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?, &capability_key(&detail))
}

#[tauri::command]
pub fn reset_tool_review_output_capability(app_handle: tauri::AppHandle, provider_id: i64, model_code: String) -> Result<(), String> {
    let detail = LLMDatabase::new(&app_handle).and_then(|db| db.get_llm_model_detail(&provider_id, &model_code))
        .map_err(|e| format!("读取审核模型配置失败: {e}"))?;
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let key = capability_key(&detail);
    let current = load_capability(&db, &key)?;
    write_capability(&db, &key, &OutputCapability { generation: current.generation + 1, ..Default::default() })?;
    tx.commit().map_err(|e| e.to_string())
}

/// Only explicit capability errors qualify; generic 400s and operational failures do not.
pub fn unsupported_format_error(error: &genai::Error) -> bool {
    let genai::Error::WebModelCall { webc_error: genai::webc::Error::ResponseFailedStatus { status, body, .. }, .. } = error else { return false; };
    if status.as_u16() != 400 && status.as_u16() != 422 { return false; }
    let Ok(body) = serde_json::from_str::<serde_json::Value>(body) else { return false; };
    let Some(error) = body.get("error") else { return false; };
    let message = error.get("message").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    let param = error.get("param").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
    // An invalid schema must be fixed, rather than interpreted as lack of Format support.
    if ["invalid schema", "invalid json schema", "unsupported keyword", "schema validation", "schema must"]
        .iter().any(|s| message.contains(s)) { return false; }
    let format = ["response_format", "json_schema", "text.format"].iter().any(|s| message.contains(s) || param.contains(s));
    let unsupported = ["unavailable", "not supported", "unsupported", "does not support", "not support", "unknown parameter", "unrecognized"].iter().any(|s| message.contains(s));
    format && unsupported
}

#[cfg(test)]
#[path = "tool_review_output_tests.rs"]
mod tests;

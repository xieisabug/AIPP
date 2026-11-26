use rusqlite::OptionalExtension;
use crate::{api::ai::config::get_network_proxy_from_config, db::system_db::SystemDatabase, FeatureConfigState};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const URL: &str = "https://aipp-helper.xiejingyang.com/api/onboarding?lang=zh-CN";
const CACHE_KEY: &str = "onboarding_preset_cache";
const PACKAGED: &str = include_str!("preset.json");

const INCOMPATIBLE: &str = "引导预设协议版本不兼容，请升级 AIPP";

fn validate(data: &str) -> Result<Value, String> {
    let mut preset: Value = serde_json::from_str(data).map_err(|e| format!("引导预设 JSON 无效：{e}"))?;
    match preset["schema_version"].as_u64() {
        Some(1) => {}
        Some(_) => return Err(INCOMPATIBLE.into()),
        None => return Err("引导预设缺少协议版本".into()),
    }
    let provider = &preset["recommended_provider"];
    for field in ["name", "api_type", "endpoint"] {
        if provider[field].as_str().is_none_or(|v| v.trim().is_empty()) {
            return Err(format!("引导预设缺少提供商字段 {field}"));
        }
    }
    let http_url = |v: &Value| v.as_str().and_then(|s| reqwest::Url::parse(s).ok())
        .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some());
    if !http_url(&provider["endpoint"]) { return Err("引导预设的服务商地址无效".into()); }
    for role in ["main", "light"] {
        if provider["models"][role].as_str().is_none() || !preset["model_ranking"][role].is_array() {
            return Err(format!("引导预设缺少 {role} 模型或排行"));
        }
    }
    for role in ["main", "light", "review"] {
        if let Some(entries) = preset["model_ranking"].get_mut(role) {
            let rows = entries.as_array_mut().ok_or("推荐模型排行格式无效")?;
            rows.retain(|row| row["base"].is_string() && row.get("aliases").is_none_or(|v|
                v.as_array().is_some_and(|aliases| aliases.iter().all(Value::is_string))));
        }
    }
    for key in ["strip_suffixes", "light_keywords"] {
        preset[key].as_array_mut().ok_or_else(|| format!("引导预设缺少 {key}"))?.retain(Value::is_string);
    }
    preset["recommended_provider"]["key_guide"].as_array_mut().ok_or("引导预设缺少获取 Key 的步骤")?
        .retain(|step| step["title"].is_string() && http_url(&step["url"]));
    if !preset["recommendation_catalog"]["providers"].is_object() || !preset["recommendation_catalog"]["benchmarks"].is_object() {
        preset["recommendation_catalog"] = serde_json::from_str(include_str!("recommendation-catalog.json"))
            .map_err(|e| format!("内置模型评分目录无效：{e}"))?;
    }
    Ok(preset)
}

// The old public helper response predates Flash-first selection and model limits.
fn official_preset(raw: &str) -> Result<Value, String> {
    let preset = validate(raw)?;
    if matches!(preset["revision"].as_str(), Some("2026-11-25.1" | "2026-11-26.1")) { validate(PACKAGED) } else { Ok(preset) }
}

pub(crate) struct ModelBudget {
    pub output: u64,
    pub context: u64,
}

fn model_budget(limits: &Value) -> ModelBudget {
    let capacity = limits["context"].as_u64().filter(|n| *n >= 2);
    let context = match capacity {
        Some(1_048_576) => 512_000,
        Some(n) => n / 2,
        None => 128_000,
    };
    let output = limits["output"].as_u64().filter(|n| *n > 0).unwrap_or(32_000)
        .min(32_000).min(context);
    ModelBudget { output, context }
}

/// Read the exact provider/model that passed the probe, including when replaying an apply.
pub(crate) fn selected_budget(db: &SystemDatabase, api_type: &str, code: &str) -> Result<ModelBudget, String> {
    let provider = match api_type {
        "openai_api" => "openai",
        "gemini" => "google",
        "github_copilot" => "github-copilot",
        "opencode_go" => "opencode-go",
        other => other,
    };
    let key = format!("{provider}/{code}");
    let saved: Option<String> = db.conn.query_row(
        "SELECT entry.value FROM system_config s, json_each(s.value,'$.preset.model_limits') entry
         WHERE s.key=?1 AND json_valid(s.value) AND entry.key=?2 LIMIT 1",
        rusqlite::params![CACHE_KEY,key], |row| row.get(0),
    ).optional().map_err(|e| e.to_string())?;
    if let Some(raw) = saved {
        return Ok(model_budget(&serde_json::from_str(&raw).map_err(|e| format!("读取模型容量失败：{e}"))?));
    }
    let packaged: Value = serde_json::from_str(PACKAGED).map_err(|e| e.to_string())?;
    Ok(model_budget(&packaged["model_limits"][&key]))
}

/// Only expose connection locations for local price matching; no keys or query parameters.
#[tauri::command]
pub fn onboarding_provider_endpoints(app: AppHandle) -> Result<std::collections::HashMap<i64, String>, String> {
    use crate::api::genai_client::provider_destination;
    let db = crate::db::llm_db::LLMDatabase::new(&app).map_err(super::err)?;
    let mut stmt = db.conn.prepare("SELECT p.id,p.api_type,(SELECT c.value FROM llm_provider_config c
        WHERE c.llm_provider_id=p.id AND c.name='endpoint' ORDER BY c.id DESC LIMIT 1) FROM llm_provider p").map_err(super::err)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(super::err)?;
    let mut endpoints = std::collections::HashMap::new();
    for row in rows {
        let (id, kind, configured) = row.map_err(super::err)?;
        if crate::db::llm_db::is_local_agent_api_type(&kind) { continue; }
        if let Ok(endpoint) = provider_destination(&kind, configured.as_deref()) {
            if let Ok(url) = reqwest::Url::parse(&endpoint) {
                if matches!(url.scheme(), "https" | "http") && url.username().is_empty()
                    && url.password().is_none() && url.query().is_none() && url.fragment().is_none() {
                    endpoints.insert(id, endpoint);
                }
            }
        }
    }
    Ok(endpoints)
}

async fn download(app: &AppHandle, url: &str) -> Result<String, String> {
    let state = app.state::<FeatureConfigState>();
    let proxy = get_network_proxy_from_config(&*state.config_feature_map.lock().await);
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(6));
    if let Some(proxy) = proxy { builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?); }
    let response = builder.build().map_err(|e| e.to_string())?.get(url)
        .header("X-AIPP-Version", app.package_info().version.to_string())
        .send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::UPGRADE_REQUIRED { return Err(INCOMPATIBLE.into()); }
    response.error_for_status().map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_onboarding_preset(app: AppHandle) -> Result<Value, String> {
    load_preset(app, true).await
}

pub(super) async fn load_preset(app: AppHandle, save_cache: bool) -> Result<Value, String> {
    let result = if let Ok(source) = std::env::var("AIPP_ONBOARDING_PRESET_URL") {
        let data = if source.starts_with("https://") || source.starts_with("http://") {
            download(&app, &source).await?
        } else {
            tokio::fs::read_to_string(&source).await.map_err(|e| format!("读取引导预设 {source} 失败：{e}"))?
        };
        Ok(validate(&data)?)
    } else {
        download(&app, URL).await.and_then(|raw| official_preset(&raw))
    };
    match result {
        Ok(preset) if !save_cache => Ok(preset),
        Ok(preset) => {
            let db = SystemDatabase::new(&app).map_err(|e| e.to_string())?;
            let entry = serde_json::json!({"fetched_at":chrono::Utc::now().to_rfc3339(), "preset":preset});
            db.conn.execute("INSERT INTO system_config(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                rusqlite::params![CACHE_KEY, entry.to_string()]).map_err(|e| e.to_string())?;
            Ok(preset)
        }
        Err(error) if error == INCOMPATIBLE => Err(error),
        Err(error) => {
            tracing::warn!(%error, "Unable to fetch onboarding preset");
            let db = SystemDatabase::new(&app).map_err(|e| e.to_string())?;
            let cached = db.get_config(CACHE_KEY).map_err(|e| e.to_string())?;
            if let Ok(entry) = serde_json::from_str::<Value>(&cached) {
                if let Ok(preset) = official_preset(&entry["preset"].to_string()) { return Ok(preset); }
            }
            validate(PACKAGED)
        }
    }
}

#[cfg(test)]
#[path = "preset_tests.rs"]
mod tests;

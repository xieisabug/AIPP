use super::llm_api::LlmModel;
use crate::db::llm_db::LLMDatabase;
use genai::adapter::AdapterKind;
use rusqlite::params;
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

const TEXT_PROTOCOLS: [&str; 4] = ["chat_completions", "responses", "anthropic_messages", "google_gemini"];

pub const PROVIDER: &str = "opencode_go";
pub const BUILTIN_SYNC_ID: &str = "official:llm.provider:opencode_go";
pub const USER_AGENT: &str = concat!("AIPP/", env!("CARGO_PKG_VERSION"));
const CATALOG: &str = "https://opencode.ai/inference/go/v1/models";

pub fn mode(value: Option<&str>) -> Option<&'static str> {
    let value = value?;
    TEXT_PROTOCOLS.into_iter().find(|mode| *mode == value)
}

pub fn selected_mode(modes: &[String], saved: Option<&String>) -> String {
    saved.filter(|s| modes.contains(s)).or_else(|| modes.first())
        .cloned().unwrap_or_else(|| "unsupported".into())
}

pub fn route(value: Option<&str>) -> Option<(AdapterKind, &'static str)> {
    let openai = "https://opencode.ai/inference/go/openai/v1/";
    match value? {
        "chat_completions" => Some((AdapterKind::OpenAI, openai)),
        "responses" => Some((AdapterKind::OpenAIResp, openai)),
        "anthropic_messages" => Some((AdapterKind::Anthropic, "https://opencode.ai/inference/go/anthropic/v1/")),
        "google_gemini" => Some((AdapterKind::Gemini, "https://opencode.ai/inference/go/google/v1beta/")),
        _ => None,
    }
}

/// Only persist accounting that the gateway actually returned. A zero cost
/// can coexist with a positive subscription allowance.
pub fn billing_metadata(usage: &genai::chat::Usage) -> Option<serde_json::Value> {
    let raw = usage.billing.opencode.as_ref()?.as_object()?;
    let mut billing = serde_json::Map::new();
    if let Some(source) = raw.get("billing_source").and_then(|v| v.as_str()) {
        billing.insert("billing_source".into(), source.into());
    }
    for field in ["allowance", "list_cost"] {
        if let Some(value) = raw.get(field).filter(|v| v.is_number()) {
            billing.insert(field.into(), value.clone());
        }
    }
    if let Some(value) = usage.billing.cost.as_ref().filter(|v| v.is_number()) {
        billing.insert("cost".into(), value.clone());
    }
    if let Some(estimated) = raw.get("estimated").and_then(|v| v.as_bool()) {
        billing.insert("estimated".into(), estimated.into());
    }
    Some(serde_json::Value::Object(billing))
}

/// The gateway also reports billed tokens across all four wire formats.
pub fn normalize_usage(mut usage: genai::chat::Usage) -> genai::chat::Usage {
    let Some(tokens) = usage.billing.opencode.as_ref().and_then(|v| v.get("tokens")) else {
        return usage;
    };
    let count = |key: &str| tokens.get(key)?.as_u64()?.try_into().ok();
    let cache_read: Option<i32> = count("cache_read");
    let cache_written: Option<i32> = match (count("cache_write_5m"), count("cache_write_1h")) {
        (None, None) => None,
        (short, long) => Some(i32::saturating_add(short.unwrap_or(0), long.unwrap_or(0))),
    };
    if let Some(input) = count("input") {
        // Go's billed input excludes cache reads and writes. AIPP's input total includes both.
        let details = usage.prompt_tokens_details.as_ref();
        let read = cache_read.or_else(|| details.and_then(|d| d.cached_tokens)).unwrap_or(0);
        let written = cache_written.or_else(|| details.and_then(|d| d.cache_creation_tokens)).unwrap_or(0);
        usage.prompt_tokens = Some(i32::saturating_add(input, read).saturating_add(written));
    }
    if let Some(output) = count("output") {
        // The gateway reports the final turn total, including reasoning.
        usage.completion_tokens = Some(output);
    }
    if let (Some(input), Some(output)) = (usage.prompt_tokens, usage.completion_tokens) {
        usage.total_tokens = Some(input.saturating_add(output));
    }
    if let Some(reasoning) = count("reasoning") {
        usage.completion_tokens_details.get_or_insert_with(Default::default).reasoning_tokens = Some(reasoning);
    }
    if let Some(n) = cache_read {
        usage.prompt_tokens_details.get_or_insert_with(Default::default).cached_tokens = Some(n);
    }
    if let Some(n) = cache_written {
        usage.prompt_tokens_details.get_or_insert_with(Default::default).cache_creation_tokens = Some(n);
    }
    usage
}

pub fn session_id(sync_id: &str) -> Uuid {
    const NAMESPACE: Uuid = Uuid::from_u128(0x7b50d084_5ee3_4bd1_9895_86e04e77b49f);
    Uuid::new_v5(&NAMESPACE, sync_id.as_bytes())
}

#[derive(Deserialize)]
struct Catalog { data: Vec<CatalogModel> }

#[derive(Deserialize)]
struct CatalogModel {
    id: String,
    #[serde(rename = "type")]
    model_type: Option<String>,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    api: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    modalities: Modalities,
}

#[derive(Default, Deserialize)]
struct Modalities {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

impl CatalogModel {
    fn into_model(self, provider_id: i64, saved: Option<&String>) -> LlmModel {
        let mut modes: Vec<String> = self.api.keys().filter_map(|key| match key.as_str() {
            "chat-completions" => Some("chat_completions"),
            "responses" => Some("responses"),
            "messages" => Some("anthropic_messages"),
            "generate-content" => Some("google_gemini"),
            _ => None,
        }).map(str::to_owned).collect();
        // Console translates between these four APIs for Go text models.
        // Keep the native protocol first; unrecognized native APIs stay unsupported.
        if !modes.is_empty() && self.model_type.as_deref() == Some("chat")
            && self.modalities.output.iter().any(|kind| kind == "text") {
            for protocol in TEXT_PROTOCOLS {
                if !modes.iter().any(|m| m == protocol) { modes.push(protocol.into()); }
            }
        }
        let request_mode = selected_mode(&modes, saved);
        let supports = |kind: &str| self.modalities.input.iter()
            .any(|value| value.split('/').next() == Some(kind));
        LlmModel {
            id: 0, llm_provider_id: provider_id, vision_support: supports("image"),
            audio_support: supports("audio"), video_support: supports("video"),
            name: self.name.unwrap_or_else(|| self.id.clone()), code: self.id,
            description: self.description.unwrap_or_default(), request_mode,
            available_modes: Some(modes),
        }
    }
}

pub async fn catalog(app: &tauri::AppHandle, provider_id: i64, proxy: Option<&str>) -> Result<Vec<LlmModel>, String> {
    let mut builder = reqwest::Client::builder().user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30));
    if let Some(proxy) = proxy.filter(|p| !p.is_empty()) {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?);
    }
    let fetch = async {
        builder.build()?.get(CATALOG).send().await?.error_for_status()?.json::<Catalog>().await
    };
    let data = fetch.await.map_err(|e| format!("OpenCode Go 模型目录 {CATALOG}: {e}"))?.data;
    let db = LLMDatabase::new(app).map_err(|e| e.to_string())?;
    let saved: std::collections::HashMap<_, _> = db.list_model_request_modes(provider_id)
        .map_err(|e| e.to_string())?.into_iter().collect();
    let models: Vec<_> = data.into_iter().map(|m| {
        let pref = saved.get(&m.id);
        m.into_model(provider_id, pref)
    }).collect();
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    // An empty-code marker distinguishes a refreshed catalog from a fresh device.
    tx.execute("DELETE FROM opencode_go_protocols", []).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO opencode_go_protocols (model,mode) VALUES ('','catalog_loaded')", []).map_err(|e| e.to_string())?;
    for model in &models {
        let modes = model.available_modes.as_deref().unwrap_or_default();
        for (position, mode) in modes.iter().enumerate() {
            tx.execute("INSERT INTO opencode_go_protocols (model,mode,position) VALUES (?1,?2,?3)", params![model.code, mode, position as i64]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(models)
}

pub fn available_modes(db: &LLMDatabase, code: &str, saved: Option<&str>) -> rusqlite::Result<Vec<String>> {
    let mut stmt = db.conn.prepare("SELECT mode FROM opencode_go_protocols WHERE model=?1 ORDER BY position")?;
    let rows = stmt.query_map([code], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.is_empty() && !db.conn.query_row("SELECT EXISTS(SELECT 1 FROM opencode_go_protocols WHERE model='')", [], |r| r.get::<_, bool>(0))? {
        // A synchronized selection can arrive before the first local catalog refresh.
        return Ok(mode(saved).into_iter().map(str::to_owned).collect());
    }
    Ok(rows.into_iter().filter(|s| mode(Some(s)).is_some()).collect())
}

#[cfg(test)]
#[path = "opencode_go_tests.rs"]
mod tests;

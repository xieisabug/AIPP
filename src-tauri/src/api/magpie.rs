use super::llm_api::LlmModel;
use crate::db::llm_db::LLMDatabase;
use genai::adapter::AdapterKind;
use rusqlite::params;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const TEXT_PROTOCOLS: [&str; 4] = ["chat_completions", "responses", "anthropic_messages", "google_gemini"];

pub const PROVIDER: &str = "magpie";
pub const BUILTIN_SYNC_ID: &str = "official:llm.provider:magpie";
/// Local gateway keys are app names. `magpie-aipp` is counted as `aipp` in Magpie Usage.
pub const APP_KEY: &str = "magpie-aipp";
pub const USER_AGENT: &str = concat!("AIPP/", env!("CARGO_PKG_VERSION"));
pub const DEFAULT_PORT: u16 = 3425;

pub fn mode(value: Option<&str>) -> Option<&'static str> {
    let value = value?;
    TEXT_PROTOCOLS.into_iter().find(|mode| *mode == value)
}

pub fn selected_mode(modes: &[String], saved: Option<&String>) -> String {
    saved.filter(|s| modes.contains(s)).or_else(|| modes.first())
        .cloned().unwrap_or_else(|| "chat_completions".into())
}

pub fn api_key_or_default(raw: Option<&str>) -> String {
    let trimmed = raw.unwrap_or("").trim();
    if trimmed.is_empty() { APP_KEY.to_string() } else { trimmed.to_string() }
}

/// `$XDG_CONFIG_HOME/magpie/settings.json`, otherwise `~/.config/magpie/settings.json`.
pub fn settings_path(xdg: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(xdg) = xdg.filter(|path| !path.as_os_str().is_empty()) {
        return Some(xdg.join("magpie").join("settings.json"));
    }
    Some(home?.join(".config").join("magpie").join("settings.json"))
}

pub fn port_from_settings(text: &str) -> Option<u16> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let port = value.get("port")?.as_u64()?;
    u16::try_from(port).ok().filter(|port| *port > 0)
}

pub fn discovered_port() -> Option<u16> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let home = dirs::home_dir();
    let path = settings_path(xdg.as_deref(), home.as_deref())?;
    port_from_settings(&std::fs::read_to_string(path).ok()?)
}

pub fn default_origin() -> String {
    format!("http://127.0.0.1:{}", discovered_port().unwrap_or(DEFAULT_PORT))
}

/// The gateway origin. An OpenAI `/v1` or Gemini `/v1beta` suffix is the client base, not a path prefix.
pub fn origin_from_endpoint(endpoint: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(endpoint)
        .map_err(|e| format!("Magpie 接口地址 {endpoint} 无法解析: {e}"))?;
    let mut path = url.path().trim_end_matches('/').to_string();
    for suffix in ["/v1beta", "/v1"] {
        if let Some(stripped) = path.strip_suffix(suffix) {
            path = stripped.to_string();
            break;
        }
    }
    if path.is_empty() {
        path = "/".into();
    }
    url.set_path(&path);
    let mut origin = url.to_string();
    if origin.ends_with('/') {
        origin.pop();
    }
    Ok(origin)
}

pub fn origin_is_loopback(origin: &str) -> bool {
    reqwest::Url::parse(origin).ok().and_then(|url| url.host_str().map(|host| {
        host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
    })).unwrap_or(false)
}

pub fn route(value: Option<&str>, origin: &str) -> Result<(AdapterKind, String), String> {
    let mode = match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => "chat_completions",
        Some(raw) => mode(Some(raw)).ok_or_else(|| format!(
            "Magpie 协议 {raw} 未受支持。可用协议：Chat Completions、Responses、Anthropic Messages、Google Gemini"))?,
    };
    let origin = origin.trim_end_matches('/');
    let (adapter, suffix) = match mode {
        "responses" => (AdapterKind::OpenAIResp, "/v1/"),
        "anthropic_messages" => (AdapterKind::Anthropic, "/v1/"),
        "google_gemini" => (AdapterKind::Gemini, "/v1beta/"),
        _ => (AdapterKind::OpenAI, "/v1/"),
    };
    Ok((adapter, format!("{origin}{suffix}")))
}

pub fn adapter_kind(value: Option<&str>) -> AdapterKind {
    route(value, "http://127.0.0.1").map(|(kind, _)| kind).unwrap_or(AdapterKind::OpenAI)
}

fn protocol_of(path: &str) -> Option<&'static str> {
    match path {
        "/v1/chat/completions" => Some("chat_completions"),
        "/v1/responses" => Some("responses"),
        "/v1/messages" => Some("anthropic_messages"),
        path if path.starts_with("/v1beta/") => Some("google_gemini"),
        _ => None,
    }
}

fn modes_for(native: &[String]) -> Vec<String> {
    let mut modes = Vec::new();
    for path in native {
        if let Some(mode) = protocol_of(path) {
            if !modes.iter().any(|existing| existing == mode) {
                modes.push(mode.to_string());
            }
        }
    }
    for protocol in TEXT_PROTOCOLS {
        if !modes.iter().any(|existing| existing == protocol) {
            modes.push((*protocol).to_string());
        }
    }
    modes
}

#[derive(Deserialize)]
struct Catalog { data: Vec<CatalogModel> }

#[derive(Deserialize)]
struct CatalogModel {
    id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    owned_by: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    native_endpoints: Vec<String>,
    #[serde(default)]
    modalities: Modalities,
}

#[derive(Default, Deserialize)]
struct Modalities {
    #[serde(default)]
    input: Vec<String>,
}

impl CatalogModel {
    fn is_chat(&self) -> bool {
        let kind = self.kind.trim();
        kind.is_empty() || kind == "chat" || kind == "model"
    }

    fn into_model(self, provider_id: i64, saved: Option<&String>) -> LlmModel {
        let modes = modes_for(&self.native_endpoints);
        let supports = |kind: &str| self.modalities.input.iter().any(|value| value.split('/').next() == Some(kind));
        let name = if self.display_name.trim().is_empty() { self.id.clone() } else { self.display_name };
        LlmModel {
            id: 0, llm_provider_id: provider_id,
            vision_support: supports("image"), audio_support: supports("audio"), video_support: supports("video"),
            name, code: self.id, description: self.owned_by,
            request_mode: selected_mode(&modes, saved), available_modes: Some(modes),
        }
    }
}

pub(crate) fn parse_catalog(body: &str, provider_id: i64, saved: &HashMap<String, String>) -> Result<Vec<LlmModel>, String> {
    let catalog: Catalog = serde_json::from_str(body)
        .map_err(|e| format!("Magpie 模型目录无法解析: {e}"))?;
    Ok(catalog.data.into_iter().filter(|model| model.is_chat() && !model.id.trim().is_empty()).map(|model| {
        let pref = saved.get(&model.id);
        model.into_model(provider_id, pref)
    }).collect())
}

pub(crate) fn hello_name(body: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("不是 JSON: {e}"))?;
    value.get("name").and_then(|name| name.as_str()).filter(|name| !name.is_empty())
        .map(str::to_string).ok_or_else(|| "缺少 name".to_string())
}

async fn confirm_gateway(client: &reqwest::Client, origin: &str) -> Result<(), String> {
    let url = format!("{}/api/hello", origin.trim_end_matches('/'));
    let response = client.get(&url).send().await.map_err(|e| format!(
        "Magpie 网关 {url} 请求失败: {e}。请确认 Magpie 已启动（本机默认 http://127.0.0.1:{DEFAULT_PORT}）"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| format!("Magpie 网关 {url} 读取响应失败: {e}"))?;
    let detail: String = body.chars().take(300).collect();
    if !status.is_success() {
        return Err(format!("Magpie 网关 {url} 返回 {status}: {detail}"));
    }
    let name = hello_name(&body).map_err(|e| format!("地址 {origin} 的 /api/hello 不是 Magpie：{e}。响应：{detail}"))?;
    if name != "magpie" {
        return Err(format!("地址 {origin} 的 /api/hello 返回 name={name}，不是 magpie"));
    }
    Ok(())
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
    let origin = match endpoint.map(str::trim).filter(|value| !value.is_empty()) {
        Some(endpoint) => {
            let resolved = crate::api::genai_client::resolve_provider_endpoint(Some(endpoint))
                .map_err(|e| e.to_string())?;
            match resolved {
                crate::api::genai_client::ProviderEndpointResolution::Unset => default_origin(),
                crate::api::genai_client::ProviderEndpointResolution::Custom(url) => origin_from_endpoint(&url)?,
            }
        }
        None => default_origin(),
    };
    let url = format!("{}/v1/models", origin.trim_end_matches('/'));
    let mut builder = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30));
    let proxy = proxy.filter(|value| !value.is_empty() && !origin_is_loopback(&origin));
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| format!("Magpie 代理 {proxy} 无效: {e}"))?);
    }
    let key = api_key_or_default(api_key);
    let client = builder.build().map_err(|e| format!("Magpie 模型目录 {url} 创建客户端失败: {e}"))?;
    confirm_gateway(&client, &origin).await?;
    let response = client.get(&url).bearer_auth(&key).send().await
        .map_err(|e| format!("Magpie 模型目录 {url} 请求失败: {e}"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| format!("Magpie 模型目录 {url} 读取响应失败: {e}"))?;
    if !status.is_success() {
        let detail: String = body.chars().take(500).collect();
        return Err(format!("Magpie 模型目录 {url} 返回 {status}: {detail}"));
    }
    let db = LLMDatabase::new(app).map_err(|e| e.to_string())?;
    let saved: HashMap<_, _> = db.list_model_request_modes(provider_id).map_err(|e| e.to_string())?.into_iter().collect();
    let models = parse_catalog(&body, provider_id, &saved).map_err(|e| format!("{e}（{url}）"))?;
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

//! DeepSeek 账户相关的小接口（需求 335：一键接入向导与服务商详情页显示余额）。
//!
//! 请求走与模型调用相同的凭据链路：API Key 先经 `vault::config_ref::prepare_provider_configs`
//! 解析，再用 `check_provider_destination` 核对已确认的网站，未确认的保险箱引用 fail-closed。
use crate::api::ai::config::get_network_proxy_from_config;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::llm_db::{LLMDatabase, LLMProviderConfig};
use crate::FeatureConfigState;
use reqwest::Url;
use serde::Serialize;
use serde_json::Value;
use tauri::Manager;

const BALANCE_TIMEOUT_SECS: u64 = 15;
pub(crate) const SHOW_BALANCE_IN_CHAT_CONFIG: &str = "deepseek_show_balance_in_chat";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeepSeekBalance {
    /// 账户是否还能调用（余额为 0 或欠费时为 false）。
    pub available: bool,
    pub currency: String,
    /// 总余额（充值 + 赠送），保留接口返回的字符串精度。
    pub total: String,
}

/// 余额接口与对话接口同源：`<服务商地址的源站>/user/balance`。
pub(crate) fn balance_url(endpoint: &str) -> Result<Url, String> {
    let url = Url::parse(endpoint).map_err(|_| "服务商地址无效".to_string())?;
    let origin = url.origin().ascii_serialization();
    Url::parse(&format!("{origin}/user/balance")).map_err(|_| "服务商地址无效".to_string())
}

/// 取人民币余额（没有时取第一条）。字段缺失视为协议错误，不当作 0。
pub(crate) fn parse_balance(value: &Value) -> Result<DeepSeekBalance, String> {
    let available = value.get("is_available").and_then(Value::as_bool).ok_or("余额响应缺少 is_available")?;
    let infos = value.get("balance_infos").and_then(Value::as_array).ok_or("余额响应缺少 balance_infos")?;
    let info = infos.iter().find(|i| i.get("currency").and_then(Value::as_str) == Some("CNY")).or_else(|| infos.first());
    let Some(info) = info else {
        return Ok(DeepSeekBalance { available, currency: "CNY".into(), total: "0.00".into() });
    };
    let currency = info.get("currency").and_then(Value::as_str).ok_or("余额响应缺少 currency")?;
    let total = info.get("total_balance").and_then(Value::as_str).ok_or("余额响应缺少 total_balance")?;
    Ok(DeepSeekBalance { available, currency: currency.into(), total: total.into() })
}

/// 只返回状态码对应的说明，不回传响应正文：DeepSeek 的 401 正文带 Key 的掩码尾号（红线①）。
pub(crate) fn status_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 => "API Key 无效或已吊销".into(),
        403 => "此 Key 无权查询余额".into(),
        404 => "该地址不提供余额查询（中转服务通常不支持）".into(),
        429 => "请求过于频繁，请稍后再试".into(),
        code => format!("余额查询失败（HTTP {code}）"),
    }
}

fn setting<'a>(configs: &'a [LLMProviderConfig], name: &str) -> Option<&'a str> {
    configs.iter().find(|c| c.name == name).map(|c| c.value.as_str()).filter(|v| !v.trim().is_empty())
}

/// 仅在 DeepSeek 服务商开启「在聊天窗口展示余额」且已填写 API Key 时展示。
pub(crate) fn show_balance_in_chat_enabled(api_type: &str, configs: &[LLMProviderConfig]) -> bool {
    api_type == "deepseek"
        && setting(configs, SHOW_BALANCE_IN_CHAT_CONFIG) == Some("true")
        && setting(configs, "api_key").is_some()
}

/// 查询某个对话或助手是否应在聊天标题栏展示 DeepSeek 余额。
/// 满足条件时返回对应的 DeepSeek `llm_provider_id`，否则返回 `None`。
#[tauri::command]
pub async fn get_deepseek_chat_balance_provider(
    app_handle: tauri::AppHandle,
    conversation_id: Option<i64>,
    assistant_id: Option<i64>,
) -> Result<Option<i64>, String> {
    let mut resolved_assistant_id = assistant_id.filter(|id| *id > 0);
    let mut resolved_provider_id: Option<i64> = None;

    if let Some(cid) = conversation_id.filter(|id| *id > 0) {
        let conversation = {
            let conv_db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
            let repo = conv_db.conversation_repo().map_err(|e| e.to_string())?;
            repo.read(cid).map_err(|e| e.to_string())?
        };
        if let Some(conversation) = conversation {
            if conversation.conversation_kind == "butler_main" {
                resolved_provider_id = crate::api::butler_api::get_butler_model_selection(&app_handle)
                    .await
                    .ok()
                    .map(|selection| selection.provider_id)
                    .filter(|id| *id > 0);
            }
            if resolved_assistant_id.is_none() {
                resolved_assistant_id = conversation.assistant_id.filter(|id| *id > 0);
            }
        }
    }

    if resolved_provider_id.is_none() {
        if let Some(aid) = resolved_assistant_id {
            let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
            let models = assistant_db.get_assistant_model(aid).map_err(|e| e.to_string())?;
            resolved_provider_id = models.first().map(|m| m.provider_id).filter(|id| *id > 0);
        }
    }

    let Some(provider_id) = resolved_provider_id else {
        return Ok(None);
    };

    let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let Ok(provider) = llm_db.get_llm_provider(provider_id) else {
        return Ok(None);
    };
    if provider.api_type != "deepseek" {
        return Ok(None);
    }
    let configs = llm_db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;
    Ok(show_balance_in_chat_enabled(&provider.api_type, &configs).then_some(provider_id))
}

#[tauri::command]
pub async fn get_deepseek_balance(app_handle: tauri::AppHandle, llm_provider_id: i64) -> Result<DeepSeekBalance, String> {
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let provider = db.get_llm_provider(llm_provider_id).map_err(|e| e.to_string())?;
    if provider.api_type != "deepseek" {
        return Err("只有 DeepSeek 服务商可以查询余额".into());
    }
    let configs = db.get_llm_provider_config(llm_provider_id).map_err(|e| e.to_string())?;
    drop(db);

    let proxy_enabled = setting(&configs, "proxy_enabled").and_then(|v| v.parse::<bool>().ok()).unwrap_or(false);
    let network_proxy = if proxy_enabled {
        let state = app_handle.state::<FeatureConfigState>();
        let map = state.config_feature_map.lock().await;
        get_network_proxy_from_config(&map)
    } else {
        None
    };

    let resolved = crate::vault::config_ref::prepare_provider_configs(&app_handle, configs).await?;
    let api_key = setting(&resolved, "api_key").ok_or("服务商还没有填写 API Key")?.trim().to_string();
    let endpoint = crate::api::genai_client::provider_destination(&provider.api_type, setting(&resolved, "endpoint"))?;
    let url = balance_url(&endpoint)?;
    crate::vault::config_ref::check_provider_destination(&resolved, &api_key, url.as_str())?;

    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(BALANCE_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(proxy) = network_proxy.as_deref() {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| format!("代理地址无效：{e}"))?);
    }
    let client = builder.build().map_err(|e| format!("创建请求失败：{}", e.without_url()))?;
    let mut auth = reqwest::header::HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_| "API Key 包含非法字符".to_string())?;
    auth.set_sensitive(true);
    let response = client.get(url).header(reqwest::header::AUTHORIZATION, auth).header("Accept", "application/json")
        .send().await
        .map_err(|e| format!("余额查询失败：{:#}", anyhow::Error::new(e.without_url())))?;
    let status = response.status();
    if !status.is_success() {
        return Err(status_error(status));
    }
    let value: Value = response.json().await.map_err(|_| "余额响应不是有效的 JSON".to_string())?;
    parse_balance(&value)
}

#[cfg(test)]
#[path = "deepseek_api_tests.rs"]
mod tests;

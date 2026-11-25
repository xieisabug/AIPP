use crate::api::ai::{acp::apply_network_proxy_to_env_vars, agent_driver::native_agents::auth,
    codex_app_server, config::agent_network_proxy};
use crate::db::llm_db::LLMDatabase;
use crate::vault::config_ref;
use chrono::{DateTime, Utc};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::Manager;

mod antigravity;
mod cache;
use cache::{QuotaCache, QuotaReading};

const GO_USAGE: &str = "https://opencode.ai/inference/go/v1/usage";
const CURSOR_KEYS: &str = "https://api2.cursor.sh/auth/exchange_user_api_key";
const CURSOR_USAGE: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaWindow {
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<String>,
    label: String,
    remaining_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_amount: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
}

impl QuotaWindow {
    fn new(label: impl Into<String>, used: f64, resets_at: Option<DateTime<Utc>>) -> Self {
        Self { group: None, label: label.into(), remaining_percent: Some((100.0 - used).clamp(0.0, 100.0)), remaining_amount: None, resets_at }
    }
}

fn codex_windows(body: &Value) -> Vec<QuotaWindow> {
    let mut windows = Vec::new();
    let mut read = |id: &str, bucket: &Value| {
        let name = bucket["limitName"].as_str().unwrap_or(id);
        for (key, fallback) in [("primary", "主窗口"), ("secondary", "次窗口")] {
            let window = &bucket[key];
            let Some(used) = window["usedPercent"].as_f64() else { continue };
            let duration = match window["windowDurationMins"].as_i64() {
                Some(10080) => "每周".into(),
                Some(n) if n > 0 && n % 1440 == 0 => format!("{} 天", n / 1440),
                Some(n) if n > 0 && n % 60 == 0 => format!("{} 小时", n / 60),
                Some(n) if n > 0 => format!("{n} 分钟"),
                _ => fallback.into(),
            };
            let reset = codex_reset(window);
            windows.push(QuotaWindow::new(format!("{name} · {duration}"), used, reset));
        }
        let individual = &bucket["individualLimit"];
        if let Some(remaining) = individual["remainingPercent"].as_f64() {
            windows.push(QuotaWindow {
                group: None,
                label: format!("{name} · 个人月度额度"),
                remaining_percent: Some(remaining.clamp(0.0, 100.0)),
                remaining_amount: None,
                resets_at: codex_reset(individual),
            });
        }
    };
    if let Some(buckets) = body["rateLimitsByLimitId"].as_object().filter(|m| !m.is_empty()) {
        for (id, bucket) in buckets { read(id, bucket); }
    } else {
        let bucket = &body["rateLimits"];
        read(bucket["limitId"].as_str().unwrap_or("Codex"), bucket);
    }
    windows
}

fn codex_reset(window: &Value) -> Option<DateTime<Utc>> {
    window["resetsAt"].as_i64().filter(|s| *s > 0).and_then(DateTime::from_timestamp_secs)
}

fn go_windows(body: &Value) -> Result<Vec<QuotaWindow>, String> {
    [("rolling", "5 小时"), ("weekly", "每周"), ("monthly", "每月")].into_iter().map(|(key, label)| {
        let meter = &body["usage"][key];
        let used = meter["percent"].as_f64().ok_or_else(|| format!("OpenCode Go 的 {key} 窗口缺少有效的 percent"))?;
        let reset = meter["resetsAt"].as_str().map(str::parse::<DateTime<Utc>>).transpose()
            .map_err(|_| format!("OpenCode Go 的 {key} 窗口重置时间无效"))?;
        Ok(QuotaWindow::new(label, used, reset))
    }).collect()
}

fn cursor_windows(body: &Value) -> Vec<QuotaWindow> {
    let end = &body["billingCycleEnd"];
    let reset = end.as_i64().or_else(|| end.as_str()?.parse().ok())
        .filter(|ms| *ms > 0).and_then(DateTime::from_timestamp_millis);
    [("totalPercentUsed", "本账期 · 总额度"), ("autoPercentUsed", "本账期 · Cursor 模型（Auto）"),
     ("apiPercentUsed", "本账期 · 其他模型（API）")].into_iter().filter_map(|(key, label)| {
        Some(QuotaWindow::new(label, body["planUsage"][key].as_f64()?, reset))
    }).collect()
}

fn http_client(proxy: Option<&str>) -> Result<Client, String> {
    let mut builder = Client::builder().user_agent(super::opencode_go::USER_AGENT)
        .redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20));
    if let Some(proxy) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| "额度查询的代理地址无效")?);
    }
    builder.build().map_err(|e| format!("创建额度查询客户端失败：{e}"))
}

async fn fetch(request: RequestBuilder, operation: &str) -> Result<Value, String> {
    let response = request.send().await.map_err(|e| format!("{operation} 连接失败：{e}"))?;
    let status = response.status();
    if !status.is_success() {
        let reason = match status {
            StatusCode::UNAUTHORIZED => "凭据无效或登录已过期",
            StatusCode::FORBIDDEN => "当前账号或密钥无此查询权限，请检查订阅权益",
            StatusCode::TOO_MANY_REQUESTS => "查询过于频繁，请稍后刷新",
            _ => "服务未接受请求",
        };
        return Err(format!("{operation} 返回 HTTP {status}：{reason}"));
    }
    response.json().await.map_err(|e| format!("{operation} 响应解析失败：{e}"))
}

#[tauri::command]
pub(crate) async fn get_provider_quota(app: tauri::AppHandle, provider_id: i64, force: Option<bool>) -> Result<QuotaReading, String> {
    let db = LLMDatabase::new(&app).map_err(|e| e.to_string())?;
    let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
    let configs = db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;
    let kind = provider.api_type.as_str();
    let mut cache = QuotaCache { conn: db.conn, owner: if kind == "antigravity" { 0 } else { provider_id }, force: force.unwrap_or(false) };
    let state = app.state::<crate::FeatureConfigState>();
    let proxy = agent_network_proxy(&configs, &*state.config_feature_map.lock().await);
    match kind {
        "antigravity" => cache.read(kind, "", antigravity::local_quota(proxy.as_deref())).await,
        "codex_app_server" => {
            let mut config = codex_app_server::extract_codex_app_server_config(&[], &configs, None)
                .map_err(|e| e.to_string())?;
            if let Some(proxy) = proxy.as_deref() { apply_network_proxy_to_env_vars(&mut config.env_vars, proxy); }
            let env = config.env_vars.iter().collect::<std::collections::BTreeMap<_, _>>();
            let account = json!([config.cli_command, config.working_directory, config.additional_args, env]).to_string();
            cache.read(kind, &account, async {
                Ok(codex_windows(&codex_app_server::probe_account_limits(&app, &config).await?))
            }).await
        }
        "opencode_go" => {
            let configs = config_ref::prepare_provider_configs(&app, configs).await?;
            let key = configs.iter().find(|c| c.name == "api_key").map(|c| c.value.trim())
                .filter(|s| !s.is_empty()).ok_or("请先填写 OpenCode Go API Key")?;
            config_ref::check_provider_destination(&configs, key, GO_USAGE)?;
            cache.read(kind, key, async {
                let client = http_client(proxy.as_deref())?;
                go_windows(&fetch(client.get(GO_USAGE).bearer_auth(key), "OpenCode Go 用量查询").await?)
            }).await
        }
        "cursor" => {
            let key = auth::cursor_quota_key(&app, provider_id, &configs)?;
            config_ref::check_provider_destination(&configs, &key, CURSOR_KEYS)?;
            cache.read(kind, &key, async {
                let client = http_client(proxy.as_deref())?;
                let exchange = fetch(client.post(CURSOR_KEYS).bearer_auth(&key).json(&json!({})), "Cursor 账号认证").await?;
                let token = exchange["accessToken"].as_str().filter(|s| !s.is_empty()).ok_or("Cursor 认证响应缺少 accessToken")?;
                let result = fetch(client.post(CURSOR_USAGE).bearer_auth(token)
                    .header("connect-protocol-version", "1").json(&json!({})), "Cursor 账期查询").await?;
                Ok(cursor_windows(&result))
            }).await
        }
        _ => Err("该提供商未接入订阅额度查询".into()),
    }
}

#[cfg(test)]
#[path = "provider_quota_tests.rs"]
mod tests;

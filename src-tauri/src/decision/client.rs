use std::collections::HashMap;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::Value;

use crate::api::ai::config::get_custom_headers_from_config;
use crate::api::genai_client::{provider_destination, resolve_provider_endpoint, ProviderEndpointResolution};
use crate::db::llm_db::LLMProviderConfig;
use crate::db::system_db::FeatureConfig;
use crate::vault::config_ref::{provider_redactor_with_secrets, ProviderErrorRedactor};

use super::protocol::{
    build_endpoint_url, decode_response, encode_request, DecisionProtocol, DecisionQuestion,
    DecisionRequest, DecisionResponse,
};

const DEFAULT_DECISION_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone)]
pub struct DecisionClientConfig {
    pub protocol: DecisionProtocol,
    pub endpoint_url: String,
    pub api_key: String,
    pub network_proxy: Option<String>,
    pub timeout_secs: u64,
    pub custom_headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecisionProbeResult {
    pub ok: bool,
    pub protocol: String,
    pub protocol_label: String,
    pub endpoint_url: String,
    pub latency_ms: i64,
    pub probability: Option<f64>,
    pub input_tokens: Option<i64>,
    pub message: String,
}

/// 纯传输层执行函数：不依赖 Tauri/DB，便于独立抽离与测试。
pub async fn execute_decision(
    cfg: &DecisionClientConfig,
    req: &DecisionRequest,
) -> Result<DecisionResponse, String> {
    let redactor = decision_redactor(&[], &cfg.api_key, &cfg.custom_headers)?;
    send_decision(cfg, req, &redactor).await.map_err(|err| redactor.clean(&err))
}

fn decision_redactor(
    configs: &[LLMProviderConfig],
    api_key: &str,
    headers: &HashMap<String, String>,
) -> Result<ProviderErrorRedactor, String> {
    let auth_values = headers.iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("x-goog-api-key"))
        .map(|(_, value)| value.as_str());
    provider_redactor_with_secrets(configs, auth_values.chain(std::iter::once(api_key.trim())))
}

async fn send_decision(
    cfg: &DecisionClientConfig,
    req: &DecisionRequest,
    redactor: &ProviderErrorRedactor,
) -> Result<DecisionResponse, String> {
    let payload = encode_request(cfg.protocol, req)?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let mut has_custom_auth = false;
    for (k, v) in &cfg.custom_headers {
        let lower = k.to_ascii_lowercase();
        if lower == "authorization" || lower == "x-goog-api-key" {
            has_custom_auth = true;
        }
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            headers.insert(name, val);
        }
    }

    let api_key = cfg.api_key.trim();
    if !api_key.is_empty() && !has_custom_auth {
        match cfg.protocol {
            DecisionProtocol::TypesafeSystemOne | DecisionProtocol::OpenAiDecisions => {
                let val = HeaderValue::from_str(&format!("Bearer {api_key}"))
                    .map_err(|e| format!("API Key 格式无效，无法写入 Authorization 头: {e}"))?;
                headers.insert(AUTHORIZATION, val);
            }
            DecisionProtocol::GeminiDecisions => {
                let val = HeaderValue::from_str(api_key)
                    .map_err(|e| format!("API Key 格式无效，无法写入 x-goog-api-key 头: {e}"))?;
                headers.insert(HeaderName::from_static("x-goog-api-key"), val);
            }
        }
    }

    let timeout = Duration::from_secs(cfg.timeout_secs.max(1));
    let mut builder = reqwest::Client::builder()
        .default_headers(headers)
        .timeout(timeout);

    if let Some(proxy_url) = cfg
        .network_proxy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| format!("决策接口代理配置无效（{proxy_url}）：{e}"))?;
        builder = builder.proxy(proxy);
    }

    let client = builder
        .build()
        .map_err(|e| format!("构建决策 HTTP 客户端失败: {e}"))?;

    let started = Instant::now();
    let response = client
        .post(&cfg.endpoint_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("决策接口请求超时（{}s）", cfg.timeout_secs.max(1))
            } else {
                format!("决策接口网络请求失败: {e}")
            }
        })?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|e| format!("读取决策接口响应失败: {e}"))?;
    let latency_ms = started.elapsed().as_millis() as i64;

    if !status.is_success() {
        let snippet: String = redactor.clean(&body_text).trim().chars().take(400).collect();
        return Err(format!(
            "{} 返回 HTTP {}：{}",
            cfg.protocol.display_name(),
            status.as_u16(),
            if snippet.is_empty() {
                "空响应体"
            } else {
                &snippet
            }
        ));
    }

    let body: Value = serde_json::from_str(&body_text).map_err(|e| {
        let snippet: String = redactor.clean(&body_text).trim().chars().take(200).collect();
        format!("决策接口返回的不是有效 JSON（{e}）：{snippet}")
    })?;

    decode_response(cfg.protocol, req, body, latency_ms)
}

pub(super) fn decision_endpoint_url(
    api_type: &str, endpoint: Option<&str>, protocol: DecisionProtocol,
) -> Result<String, String> {
    let mode = protocol.as_request_mode();
    if !crate::db::llm_db::http_provider_modes(api_type).is_some_and(|modes| modes.contains(&mode)) {
        return Err(format!("提供商 {api_type} 不支持决策协议 {mode}，请检查提供商和模型协议"));
    }
    let base = match resolve_provider_endpoint(endpoint).map_err(|e| e.to_string())? {
        ProviderEndpointResolution::Custom(url) => url,
        ProviderEndpointResolution::Unset => provider_destination(api_type, None)?,
    };
    build_endpoint_url(protocol, &base)
}

/// 通过 AIPP 提供商配置执行决策请求：强制经过保险箱去向校验与错误脱敏。
pub async fn execute_for_provider(
    api_type: &str,
    prepared_configs: &[LLMProviderConfig],
    request_mode: &str,
    request: &DecisionRequest,
    network_proxy: Option<&str>,
    proxy_enabled: bool,
    timeout_secs: Option<u64>,
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Result<(DecisionResponse, String), String> {
    let protocol = DecisionProtocol::from_request_mode(request_mode).ok_or_else(|| {
        format!("模型请求协议 `{request_mode}` 不是决策协议，无法发起决策请求")
    })?;

    let api_key = prepared_configs.iter().rev().find(|cfg| cfg.name == "api_key")
        .map(|cfg| cfg.value.as_str()).unwrap_or_default();
    let custom_headers = get_custom_headers_from_config(config_feature_map);
    let redactor = decision_redactor(prepared_configs, api_key, &custom_headers)?;

    let result = async {
        let mut endpoint_raw: Option<String> = None;
        let mut provider_proxy_enabled = proxy_enabled;

        for cfg in prepared_configs {
            match cfg.name.as_str() {
                "endpoint" => endpoint_raw = Some(cfg.value.clone()),
                "proxy_enabled" => {
                    if let Ok(enabled) = cfg.value.parse::<bool>() {
                        provider_proxy_enabled = enabled;
                    }
                }
                _ => {}
            }
        }

        let endpoint_url = decision_endpoint_url(api_type, endpoint_raw.as_deref(), protocol)?;

        // 凭据保险箱红线：检查解析后的 API Key 是否获准发往目标站点
        crate::vault::config_ref::check_provider_destination(
            prepared_configs,
            api_key,
            &endpoint_url,
        )?;

        let effective_proxy = if provider_proxy_enabled {
            network_proxy.map(str::to_string)
        } else {
            None
        };

        let client_cfg = DecisionClientConfig {
            protocol,
            endpoint_url: endpoint_url.clone(),
            api_key: api_key.to_owned(),
            network_proxy: effective_proxy,
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_DECISION_TIMEOUT_SECS),
            custom_headers,
        };

        let response = send_decision(&client_cfg, request, &redactor).await?;
        Ok::<_, String>((response, endpoint_url))
    }
    .await;

    result.map_err(|err| redactor.clean(&err))
}

/// 轻量连通性探针：发一题最简 Bool 判定，验证端点、鉴权、协议解析与延迟。
pub async fn probe_decision_model(
    api_type: &str,
    prepared_configs: &[LLMProviderConfig],
    request_mode: &str,
    model_code: &str,
    network_proxy: Option<&str>,
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Result<DecisionProbeResult, String> {
    let protocol = DecisionProtocol::from_request_mode(request_mode).ok_or_else(|| {
        format!("模型 `{model_code}` 当前协议为 `{request_mode}`，请先在提供商模型配置中切换为决策协议")
    })?;

    let req = DecisionRequest::new(
        model_code,
        "AIPP decision endpoint connectivity check ping.",
        vec![DecisionQuestion::bool(
            "is_connectivity_ping",
            "Is this text a short connectivity check ping?",
        )],
    );

    let (resp, endpoint_url) = execute_for_provider(
        api_type,
        prepared_configs,
        request_mode,
        &req,
        network_proxy,
        false,
        Some(15),
        config_feature_map,
    )
    .await?;

    let prob = resp.bool_probability("is_connectivity_ping");
    let prob_text = prob
        .map(|p| format!("{:.1}%", p * 100.0))
        .unwrap_or_else(|| "未知".to_string());

    Ok(DecisionProbeResult {
        ok: true,
        protocol: protocol.as_request_mode().to_string(),
        protocol_label: protocol.display_name().to_string(),
        endpoint_url,
        latency_ms: resp.latency_ms,
        probability: prob,
        input_tokens: resp.usage.input_tokens,
        message: format!(
            "连接成功（{}，{}ms，探针概率 {}）",
            protocol.display_name(),
            resp.latency_ms,
            prob_text
        ),
    })
}

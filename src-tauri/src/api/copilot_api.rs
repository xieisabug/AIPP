use crate::api::ai::config::get_network_proxy_from_config;
use crate::db::llm_db::LLMDatabase;
use crate::FeatureConfigState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CopilotDeviceFlowStartResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

// GitHub Device Flow API 响应结构
#[derive(Debug, Deserialize)]
struct GitHubDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i64,
    interval: i64,
}

#[derive(Debug, Deserialize)]
struct GitHubTokenResponse {
    access_token: String,
    token_type: String,
    scope: String,
}

#[derive(Debug, Deserialize)]
struct GitHubTokenError {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CopilotAuthResult {
    pub llm_provider_id: i64,
    pub access_token: String,
    pub token_type: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub scope: Option<String>,
}

/// 启动 GitHub Copilot Device Flow 授权
/// 调用 GitHub API 获取真实的 device code，并自动打开浏览器让用户授权
#[tauri::command]
pub async fn start_github_copilot_device_flow(
    app_handle: AppHandle,
    llm_provider_id: i64,
) -> Result<CopilotDeviceFlowStartResponse, String> {
    info!(llm_provider_id, "[Copilot] start_github_copilot_device_flow called");

    // GitHub Copilot 的 OAuth App credentials
    // 这是 GitHub Copilot 官方的 client_id
    let client_id = "Iv1.b507a08c87ecfe98";

    info!("[Copilot] Requesting device code from GitHub...");

    // 获取网络代理配置
    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    drop(config_feature_map);

    // 1. 请求 device code
    let client = if let Some(proxy_url) = &network_proxy {
        info!(proxy_url = %proxy_url, "[Copilot] Using network proxy");
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| format!("代理配置失败: {}", e))?;
        reqwest::Client::builder()
            .proxy(proxy)
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?
    } else {
        reqwest::Client::new()
    };
    let response = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .header("User-Agent", "AIPP-Copilot-Client")
        .json(&serde_json::json!({
            "client_id": client_id,
            "scope": "user:email"
        }))
        .send()
        .await
        .map_err(|e| {
            error!(error = ?e, "[Copilot] Failed to request device code");
            format!("请求 GitHub Device Code 失败: {}", e)
        })?;

    let status = response.status();
    let body = response.text().await.map_err(|e| {
        error!(error = ?e, "[Copilot] Failed to read response body");
        format!("读取响应失败: {}", e)
    })?;

    debug!(status = ?status, body = %body, "[Copilot] GitHub API response");

    if !status.is_success() {
        error!(status = ?status, body = %body, "[Copilot] GitHub API returned error");
        return Err(format!("GitHub API 返回错误: {} - {}", status, body));
    }

    let device_response: GitHubDeviceCodeResponse = serde_json::from_str(&body).map_err(|e| {
        error!(error = ?e, body = %body, "[Copilot] Failed to parse device code response");
        format!("解析 Device Code 响应失败: {}", e)
    })?;

    info!(
        user_code = %device_response.user_code,
        verification_uri = %device_response.verification_uri,
        expires_in = device_response.expires_in,
        "[Copilot] Device code obtained successfully"
    );

    // 2. 自动打开浏览器
    let verification_url = device_response.verification_uri.clone();
    info!(url = %verification_url, "[Copilot] Opening browser for user authorization...");

    if let Err(e) = open::that(&verification_url) {
        warn!(error = ?e, "[Copilot] Failed to open browser automatically, user needs to open manually");
    } else {
        info!("[Copilot] Browser opened successfully");
    }

    let resp = CopilotDeviceFlowStartResponse {
        device_code: device_response.device_code,
        user_code: device_response.user_code,
        verification_uri: device_response.verification_uri,
        expires_in: device_response.expires_in,
        interval: device_response.interval,
    };

    debug!(?resp, "[Copilot] Device flow started successfully");

    Ok(resp)
}

/// 轮询 GitHub Copilot 授权结果，并在成功后将 access_token 存入 llm_provider_config 表的 `api_key` 字段。
/// 真实调用 GitHub API 进行轮询，直到用户完成授权或超时
#[tauri::command]
pub async fn poll_github_copilot_token(
    app_handle: AppHandle,
    llm_provider_id: i64,
    device_code: String,
    interval: i64,
) -> Result<CopilotAuthResult, String> {
    info!(
        llm_provider_id,
        device_code = %device_code,
        interval,
        "[Copilot] Start polling GitHub for access token"
    );

    let client_id = "Iv1.b507a08c87ecfe98";

    // 获取网络代理配置
    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    drop(config_feature_map);

    let client = if let Some(proxy_url) = &network_proxy {
        info!(proxy_url = %proxy_url, "[Copilot] Using network proxy for polling");
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| format!("代理配置失败: {}", e))?;
        reqwest::Client::builder()
            .proxy(proxy)
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?
    } else {
        reqwest::Client::new()
    };

    // 最多轮询 15 分钟 (900秒 / interval)
    let max_attempts = 900 / interval.max(5);
    let mut attempt = 0;

    loop {
        attempt += 1;

        if attempt > max_attempts {
            error!(max_attempts, "[Copilot] Polling timeout, user did not authorize in time");
            return Err("授权超时，用户未在规定时间内完成授权".to_string());
        }

        debug!(attempt, max_attempts, "[Copilot] Polling for access token...");

        // 等待指定的间隔时间
        sleep(Duration::from_secs(interval as u64)).await;

        // 请求 access token
        let response = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .header("User-Agent", "AIPP-Copilot-Client")
            .json(&serde_json::json!({
                "client_id": client_id,
                "device_code": device_code,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
            }))
            .send()
            .await
            .map_err(|e| {
                error!(error = ?e, attempt, "[Copilot] Failed to poll for token");
                format!("轮询 token 失败: {}", e)
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            error!(error = ?e, "[Copilot] Failed to read poll response");
            format!("读取轮询响应失败: {}", e)
        })?;

        debug!(attempt, status = ?status, body = %body, "[Copilot] Poll response");

        // 尝试解析为成功响应
        if let Ok(token_response) = serde_json::from_str::<GitHubTokenResponse>(&body) {
            info!(attempt, "[Copilot] Authorization successful! Access token obtained.");

            let access_token = token_response.access_token;
            let token_type = token_response.token_type;
            let scope = token_response.scope;

            // 保存 token 到数据库
            let db = LLMDatabase::new(&app_handle)
                .map_err(|e| format!("创建 LLM 数据库连接失败: {}", e))?;

            if let Err(e) = db.update_llm_provider_config(llm_provider_id, "api_key", &access_token)
            {
                error!(llm_provider_id, error = ?e, "[Copilot] Failed to save api_key");
                return Err(format!("保存 Copilot 授权信息失败: {}", e));
            }

            info!(llm_provider_id, "[Copilot] Access token saved to database successfully");

            let result = CopilotAuthResult {
                llm_provider_id,
                access_token,
                token_type,
                expires_at: None, // GitHub token 一般不会过期
                scope: Some(scope),
            };

            debug!(?result, "[Copilot] Authorization completed");
            return Ok(result);
        }

        // 尝试解析为错误响应
        if let Ok(error_response) = serde_json::from_str::<GitHubTokenError>(&body) {
            match error_response.error.as_str() {
                "authorization_pending" => {
                    debug!(
                        attempt,
                        "[Copilot] Authorization pending, user has not completed authorization yet"
                    );
                    // 继续轮询
                    continue;
                }
                "slow_down" => {
                    warn!(attempt, "[Copilot] Rate limited, slowing down polling");
                    // GitHub 要求减慢轮询速度，增加等待时间
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }
                "expired_token" => {
                    error!(attempt, "[Copilot] Device code expired");
                    return Err("授权码已过期，请重新开始授权流程".to_string());
                }
                "access_denied" => {
                    error!(attempt, "[Copilot] User denied authorization");
                    return Err("用户拒绝了授权请求".to_string());
                }
                _ => {
                    error!(attempt, error = %error_response.error, description = ?error_response.error_description, "[Copilot] Unknown error from GitHub");
                    return Err(format!("GitHub 返回错误: {}", error_response.error));
                }
            }
        }

        // 无法解析响应
        warn!(attempt, body = %body, "[Copilot] Unexpected response format, continuing to poll...");
    }
}

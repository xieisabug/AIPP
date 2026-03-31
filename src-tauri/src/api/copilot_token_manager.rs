use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, error, info, warn};

const COPILOT_TOKEN_EXCHANGE_URL: &str = "https://api.github.com/copilot_internal/v2/token";

// 与 cc-switch / goose 等开源项目保持一致的 Header 常量
const COPILOT_EDITOR_VERSION: &str = "vscode/1.96.0";
const COPILOT_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.26.7";

/// Token 刷新提前量（秒）— 在 session token 过期前提前刷新
const TOKEN_REFRESH_BUFFER_SECONDS: i64 = 60;

/// Token Exchange 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotSessionInfo {
    /// 短期 Copilot session token
    pub token: String,
    /// Unix timestamp，session token 过期时间
    pub expires_at: i64,
    /// 建议的刷新间隔（秒）
    #[serde(default)]
    pub refresh_in: i64,
    /// API endpoints
    pub endpoints: CopilotEndpoints,
    /// 其他字段
    #[serde(flatten)]
    pub _extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotEndpoints {
    /// 主 API 端点（如 https://api.individual.githubcopilot.com）
    pub api: String,
    /// 其他端点
    #[serde(flatten)]
    pub _extra: HashMap<String, serde_json::Value>,
}

/// 缓存的 session 状态
#[derive(Debug, Clone)]
struct CachedSession {
    /// 用于生成此 session 的 OAuth token（用于检测 token 变更）
    oauth_token_hash: u64,
    /// Session 信息
    info: CopilotSessionInfo,
    /// 过期时间（基于 expires_at 减去 buffer）
    effective_expiry: DateTime<Utc>,
}

/// Copilot Token Manager — 管理 OAuth → Session Token 的交换和缓存
#[derive(Clone)]
pub struct CopilotTokenManagerState {
    inner: Arc<TokioMutex<CopilotTokenManagerInner>>,
}

struct CopilotTokenManagerInner {
    cached: Option<CachedSession>,
    http_client: Option<Client>,
}

impl CopilotTokenManagerState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokioMutex::new(CopilotTokenManagerInner {
                cached: None,
                http_client: None,
            })),
        }
    }

    /// 获取有效的 session token 和 API 端点。
    /// 如果缓存有效则直接返回，否则自动执行 token exchange。
    pub async fn get_session(
        &self,
        oauth_token: &str,
        network_proxy: Option<&str>,
    ) -> Result<(String, String), String> {
        let mut inner = self.inner.lock().await;

        let token_hash = hash_token(oauth_token);

        // 检查缓存是否有效
        if let Some(cached) = &inner.cached {
            if cached.oauth_token_hash == token_hash && cached.effective_expiry > Utc::now() {
                debug!("[CopilotTokenManager] Using cached session token");
                return Ok((cached.info.token.clone(), cached.info.endpoints.api.clone()));
            }
            debug!("[CopilotTokenManager] Cached session expired or token changed, refreshing");
        }

        // 执行 token exchange
        let info = exchange_token(&mut inner, oauth_token, network_proxy).await?;

        let effective_expiry =
            DateTime::from_timestamp(info.expires_at - TOKEN_REFRESH_BUFFER_SECONDS, 0)
                .unwrap_or_else(Utc::now);

        let result = (info.token.clone(), info.endpoints.api.clone());

        inner.cached = Some(CachedSession { oauth_token_hash: token_hash, info, effective_expiry });

        Ok(result)
    }

    /// 使缓存失效（如 OAuth token 更新后）
    pub async fn invalidate(&self) {
        let mut inner = self.inner.lock().await;
        inner.cached = None;
        info!("[CopilotTokenManager] Cache invalidated");
    }
}

fn hash_token(token: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    hasher.finish()
}

fn get_or_create_client(
    inner: &mut CopilotTokenManagerInner,
    network_proxy: Option<&str>,
) -> Result<Client, String> {
    // 每次根据 proxy 配置重建 client（proxy 可能变化）
    let client = if let Some(proxy_url) = network_proxy {
        if !proxy_url.trim().is_empty() {
            let proxy =
                reqwest::Proxy::all(proxy_url).map_err(|e| format!("代理配置失败: {}", e))?;
            reqwest::Client::builder()
                .proxy(proxy)
                .build()
                .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?
        } else {
            Client::new()
        }
    } else {
        if let Some(ref existing) = inner.http_client {
            return Ok(existing.clone());
        }
        Client::new()
    };

    inner.http_client = Some(client.clone());
    Ok(client)
}

/// 执行 Token Exchange: 用 OAuth token 换取短期 Copilot session token
async fn exchange_token(
    inner: &mut CopilotTokenManagerInner,
    oauth_token: &str,
    network_proxy: Option<&str>,
) -> Result<CopilotSessionInfo, String> {
    info!("[CopilotTokenManager] Exchanging OAuth token for Copilot session token...");

    let client = get_or_create_client(inner, network_proxy)?;

    let response = client
        .get(COPILOT_TOKEN_EXCHANGE_URL)
        .header("Authorization", format!("token {}", oauth_token))
        .header("User-Agent", COPILOT_USER_AGENT)
        .header("Editor-Version", COPILOT_EDITOR_VERSION)
        .header("Editor-Plugin-Version", COPILOT_PLUGIN_VERSION)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| {
            error!(error = ?e, "[CopilotTokenManager] Token exchange request failed");
            format!("Copilot Token Exchange 请求失败: {}", e)
        })?;

    let status = response.status();

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("Copilot Token Exchange 端点返回 404。\
            此 Token 可能不支持完整模型访问，请通过 OAuth Device Flow 重新授权。"
            .to_string());
    }

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("GitHub Token 无效或已过期，请重新授权。".to_string());
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        return Err("无 Copilot 订阅或 Token 权限不足，请检查账号的 Copilot 订阅状态。".to_string());
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!(status = ?status, body = %body, "[CopilotTokenManager] Token exchange failed");
        return Err(format!("Copilot Token Exchange 失败: {} - {}", status, body));
    }

    let body = response.text().await.map_err(|e| {
        error!(error = ?e, "[CopilotTokenManager] Failed to read exchange response");
        format!("读取 Token Exchange 响应失败: {}", e)
    })?;

    debug!("[CopilotTokenManager] Token exchange response received");

    let info: CopilotSessionInfo = serde_json::from_str(&body).map_err(|e| {
        error!(error = ?e, body = %body, "[CopilotTokenManager] Failed to parse exchange response");
        format!("解析 Token Exchange 响应失败: {}", e)
    })?;

    info!(
        expires_at = info.expires_at,
        api_endpoint = %info.endpoints.api,
        "[CopilotTokenManager] Token exchange successful"
    );

    Ok(info)
}

/// Tauri command: 测试 token exchange 是否可用（用于前端验证 token 质量）
#[tauri::command]
pub async fn test_copilot_token_exchange(
    app_handle: tauri::AppHandle,
    oauth_token: String,
) -> Result<CopilotSessionInfo, String> {
    use crate::api::ai::config::get_network_proxy_from_config;
    use crate::FeatureConfigState;
    use tauri::Manager;

    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    drop(config_feature_map);

    let manager = app_handle.state::<CopilotTokenManagerState>();
    let (_, _) = manager.get_session(&oauth_token, network_proxy.as_deref()).await?;

    // 返回完整的 session info 供前端检查
    let inner = manager.inner.lock().await;
    inner
        .cached
        .as_ref()
        .map(|c| c.info.clone())
        .ok_or_else(|| "Token exchange 成功但缓存为空".to_string())
}

/// 用于模型获取和聊天流程：对 github_copilot 类型的 provider 进行 token exchange，
/// 返回替换后的 (session_token, api_endpoint)。
/// 非 Copilot 类型直接返回 None。
pub async fn resolve_copilot_session_if_needed(
    app_handle: &tauri::AppHandle,
    api_type: &str,
    configs: &[crate::db::llm_db::LLMProviderConfig],
    network_proxy: Option<&str>,
) -> Result<Option<(String, String)>, String> {
    use tauri::Manager;

    if api_type.to_lowercase() != "github_copilot" {
        return Ok(None);
    }

    // 从 configs 中提取 OAuth token
    let oauth_token =
        configs.iter().find(|c| c.name == "api_key").map(|c| c.value.clone()).unwrap_or_default();

    if oauth_token.is_empty() {
        return Err("Copilot provider 未配置 API Key (OAuth Token)".to_string());
    }

    let manager = app_handle.state::<CopilotTokenManagerState>();
    let (session_token, api_endpoint) = manager.get_session(&oauth_token, network_proxy).await?;

    info!(
        api_endpoint = %api_endpoint,
        "[resolve_copilot_session] Token exchange successful for github_copilot provider"
    );

    Ok(Some((session_token, api_endpoint)))
}

/// 准备 provider configs。对 Copilot 类型进行 token exchange 并替换 api_key/endpoint，
/// 对其他类型原样返回。所有需要创建 genai client 的地方都应通过此函数处理 configs。
pub async fn prepare_provider_configs(
    app_handle: &tauri::AppHandle,
    api_type: &str,
    configs: &[crate::db::llm_db::LLMProviderConfig],
    network_proxy: Option<&str>,
) -> Result<Vec<crate::db::llm_db::LLMProviderConfig>, String> {
    use crate::db::llm_db::LLMProviderConfig;

    let session =
        resolve_copilot_session_if_needed(app_handle, api_type, configs, network_proxy).await?;

    match session {
        Some((session_token, api_endpoint)) => {
            info!(
                api_endpoint = %api_endpoint,
                token_prefix = %&session_token[..20.min(session_token.len())],
                "[prepare_provider_configs] Replacing configs for Copilot"
            );
            let mut new_configs: Vec<LLMProviderConfig> = configs
                .iter()
                .filter(|c| c.name != "api_key" && c.name != "endpoint")
                .cloned()
                .collect();

            new_configs.push(LLMProviderConfig {
                id: 0,
                name: "api_key".to_string(),
                llm_provider_id: 0,
                value: session_token,
                append_location: String::new(),
                is_addition: false,
            });

            new_configs.push(LLMProviderConfig {
                id: 0,
                name: "endpoint".to_string(),
                llm_provider_id: 0,
                value: api_endpoint,
                append_location: String::new(),
                is_addition: false,
            });

            Ok(new_configs)
        }
        None => Ok(configs.to_vec()),
    }
}

use crate::utils::bun_utils::BunUtils;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// LSP 请求 ID 计数器
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Copilot LSP 服务器状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotLspStatus {
    pub is_running: bool,
    pub is_authorized: bool,
    pub user: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotOauthTokenCandidate {
    pub id: String,
    pub token: String,
    pub masked_token: String,
    pub source: String,
    pub location: String,
}

/// SignInInitiate 结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum SignInInitiateResult {
    #[serde(rename = "AlreadySignedIn")]
    AlreadySignedIn { user: String },
    #[serde(rename = "PromptUserDeviceFlow")]
    PromptUserDeviceFlow(DeviceFlowPrompt),
}

/// Device Flow 提示信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFlowPrompt {
    pub user_code: String,
    pub verification_uri: String,
}

/// SignInConfirm 结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum SignInStatus {
    #[serde(rename = "OK")]
    Ok { user: Option<String> },
    #[serde(rename = "AlreadySignedIn")]
    AlreadySignedIn { user: String },
    #[serde(rename = "MaybeOk")]
    MaybeOk { user: Option<String> },
    #[serde(rename = "NotAuthorized")]
    NotAuthorized { user: String },
    #[serde(rename = "NotSignedIn")]
    NotSignedIn,
}

/// CheckStatus 结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CheckStatusResult {
    #[serde(rename = "OK")]
    Ok { user: Option<String> },
    #[serde(rename = "AlreadySignedIn")]
    AlreadySignedIn { user: String },
    #[serde(rename = "MaybeOk")]
    MaybeOk { user: Option<String> },
    #[serde(rename = "NotAuthorized")]
    NotAuthorized { user: String },
    #[serde(rename = "NotSignedIn")]
    NotSignedIn,
}

/// Copilot LSP 服务器实例
pub struct CopilotLspServer {
    process: Child,
    stdin: std::process::ChildStdin,
    stdout_reader: BufReader<std::process::ChildStdout>,
    is_initialized: bool,
    pending_requests: HashMap<u64, tokio::sync::oneshot::Sender<serde_json::Value>>,
}

/// Copilot LSP 管理器状态
pub struct CopilotLspState {
    pub server: Arc<Mutex<Option<CopilotLspServer>>>,
}

impl Default for CopilotLspState {
    fn default() -> Self {
        Self { server: Arc::new(Mutex::new(None)) }
    }
}

/// 停止 Copilot LSP 服务器
#[tauri::command]
pub async fn stop_copilot_lsp(app_handle: AppHandle) -> Result<(), String> {
    info!("[CopilotLSP] Stopping Copilot Language Server...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    if let Some(mut server) = server_guard.take() {
        // 发送 shutdown 请求
        let _ = send_lsp_request(&mut server, "shutdown", serde_json::Value::Null);

        // 发送 exit 通知
        let _ = send_lsp_notification(&mut server, "exit", serde_json::Value::Null);

        // 强制终止进程
        let _ = server.process.kill();

        info!("[CopilotLSP] Server stopped");
    }

    Ok(())
}

/// 检查 Copilot 登录状态
#[tauri::command]
pub async fn check_copilot_status(app_handle: AppHandle) -> Result<CheckStatusResult, String> {
    info!("[CopilotLSP] Checking Copilot status...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    let server = server_guard.as_mut().ok_or("Copilot LSP 未启动，请先启动服务")?;

    let result = send_lsp_request(
        server,
        "checkStatus",
        serde_json::json!({
            "localChecksOnly": false
        }),
    )?;

    debug!(result = ?result, "[CopilotLSP] checkStatus response");

    let status: CheckStatusResult =
        serde_json::from_value(result).map_err(|e| format!("解析状态响应失败: {}", e))?;

    info!(status = ?status, "[CopilotLSP] Status checked");

    Ok(status)
}

/// 启动 Copilot 登录流程
#[tauri::command]
pub async fn sign_in_initiate(app_handle: AppHandle) -> Result<SignInInitiateResult, String> {
    info!("[CopilotLSP] Initiating sign in...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    let server = server_guard.as_mut().ok_or("Copilot LSP 未启动，请先启动服务")?;

    let result = send_lsp_request(server, "signInInitiate", serde_json::json!({}))?;

    debug!(result = ?result, "[CopilotLSP] signInInitiate response");

    let sign_in_result: SignInInitiateResult =
        serde_json::from_value(result).map_err(|e| format!("解析登录响应失败: {}", e))?;

    // 如果是 device flow，自动打开浏览器
    if let SignInInitiateResult::PromptUserDeviceFlow(ref prompt) = sign_in_result {
        info!(
            user_code = %prompt.user_code,
            verification_uri = %prompt.verification_uri,
            "[CopilotLSP] Device flow started"
        );

        let uri = prompt.verification_uri.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = open::that(&uri) {
                warn!(error = ?e, "[CopilotLSP] Failed to open browser");
            }
        });
    }

    Ok(sign_in_result)
}

/// 确认 Copilot 登录
#[tauri::command]
pub async fn sign_in_confirm(
    app_handle: AppHandle,
    user_code: String,
) -> Result<SignInStatus, String> {
    info!(user_code = %user_code, "[CopilotLSP] Confirming sign in...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    let server = server_guard.as_mut().ok_or("Copilot LSP 未启动，请先启动服务")?;

    let result = send_lsp_request(
        server,
        "signInConfirm",
        serde_json::json!({
            "userCode": user_code
        }),
    )?;

    debug!(result = ?result, "[CopilotLSP] signInConfirm response");

    let status: SignInStatus =
        serde_json::from_value(result).map_err(|e| format!("解析确认响应失败: {}", e))?;

    info!(status = ?status, "[CopilotLSP] Sign in confirmed");

    Ok(status)
}

/// 登出 Copilot
#[tauri::command]
pub async fn sign_out_copilot(app_handle: AppHandle) -> Result<(), String> {
    info!("[CopilotLSP] Signing out...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    let server = server_guard.as_mut().ok_or("Copilot LSP 未启动，请先启动服务")?;

    let _ = send_lsp_request(server, "signOut", serde_json::json!({}))?;

    info!("[CopilotLSP] Signed out");

    Ok(())
}

/// 获取 Copilot LSP 状态
#[tauri::command]
pub async fn get_copilot_lsp_status(app_handle: AppHandle) -> Result<CopilotLspStatus, String> {
    let state = app_handle.state::<CopilotLspState>();
    let server_guard = state.server.lock().await;

    if server_guard.is_none() {
        return Ok(CopilotLspStatus {
            is_running: false,
            is_authorized: false,
            user: None,
            error: None,
        });
    }

    // 如果服务器正在运行，检查状态
    drop(server_guard);

    match check_copilot_status(app_handle).await {
        Ok(status) => {
            let (is_authorized, user) = match status {
                CheckStatusResult::Ok { user } => (true, user),
                CheckStatusResult::AlreadySignedIn { user } => (true, Some(user)),
                CheckStatusResult::MaybeOk { user } => (true, user),
                CheckStatusResult::NotAuthorized { user } => (false, Some(user)),
                CheckStatusResult::NotSignedIn => (false, None),
            };

            Ok(CopilotLspStatus { is_running: true, is_authorized, user, error: None })
        }
        Err(e) => Ok(CopilotLspStatus {
            is_running: true,
            is_authorized: false,
            user: None,
            error: Some(e),
        }),
    }
}

fn is_supported_copilot_token(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed.starts_with("gho_") || trimmed.starts_with("ghu_") || trimmed.starts_with("github_pat_")
}

fn mask_copilot_token(token: &str) -> String {
    let trimmed = token.trim();
    let prefix: String = trimmed.chars().take(5).collect();
    if trimmed.chars().count() <= 5 {
        prefix
    } else {
        format!("{}{}", prefix, "********")
    }
}

fn build_token_candidate(
    token: String,
    source: impl Into<String>,
    location: impl Into<String>,
) -> CopilotOauthTokenCandidate {
    let source = source.into();
    let location = location.into();
    CopilotOauthTokenCandidate {
        id: format!("{}::{}", source, location),
        masked_token: mask_copilot_token(&token),
        token,
        source,
        location,
    }
}

fn scan_json_for_copilot_token(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(token) if is_supported_copilot_token(token) => {
            Some(token.trim().to_string())
        }
        serde_json::Value::Array(items) => items.iter().find_map(scan_json_for_copilot_token),
        serde_json::Value::Object(map) => {
            for value in map.values() {
                if let Some(token) = scan_json_for_copilot_token(value) {
                    return Some(token);
                }
            }
            None
        }
        _ => None,
    }
}

fn try_read_token_from_env() -> Vec<CopilotOauthTokenCandidate> {
    let mut candidates = Vec::new();

    for env_name in ["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(env_name) {
            let trimmed = value.trim();
            if is_supported_copilot_token(trimmed) {
                candidates.push(build_token_candidate(
                    trimmed.to_string(),
                    "环境变量",
                    env_name.to_string(),
                ));
            }
        }
    }

    candidates
}


#[cfg(target_os = "macos")]
fn try_read_token_from_macos_keychain() -> Result<Vec<CopilotOauthTokenCandidate>, String> {
    let mut candidates = Vec::new();

    // 扫描 VSCode 的 GitHub 认证 keychain 条目
    let output = Command::new("security")
        .args(["find-generic-password", "-s", "github.vscode-github-authentication", "-w"])
        .output()
        .map_err(|e| format!("调用 macOS Keychain 失败: {}", e))?;

    if output.status.success() {
        let raw = String::from_utf8(output.stdout)
            .map_err(|e| format!("解析 macOS Keychain 输出失败: {}", e))?;
        let trimmed = raw.trim();

        // VSCode 存储的值可能是 JSON 数组，需要解析
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
            // 格式: [{"id": "github", "scopes": [...], "accessToken": "ghu_..."}]
            if let Some(array) = parsed.as_array() {
                for entry in array {
                    if let Some(token) = entry.get("accessToken").and_then(|v| v.as_str()) {
                        if is_supported_copilot_token(token) {
                            let account = entry
                                .get("account")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            candidates.push(build_token_candidate(
                                token.to_string(),
                                "VSCode Keychain",
                                format!("service=github.vscode-github-authentication, account={}", account),
                            ));
                        }
                    }
                }
            }
        } else if is_supported_copilot_token(trimmed) {
            // 可能是直接的 token 字符串
            candidates.push(build_token_candidate(
                trimmed.to_string(),
                "VSCode Keychain",
                "service=github.vscode-github-authentication".to_string(),
            ));
        }
    } else {
        let error_message = String::from_utf8_lossy(&output.stderr);
        if !error_message.trim().is_empty() {
            warn!(error = %error_message.trim(), "[CopilotLSP] VSCode keychain entry not found");
        }
    }

    Ok(candidates)
}

#[cfg(not(target_os = "macos"))]
fn try_read_token_from_macos_keychain() -> Result<Vec<CopilotOauthTokenCandidate>, String> {
    Ok(Vec::new())
}

fn get_legacy_apps_json_candidates() -> Result<Vec<PathBuf>, String> {
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        candidates.push(
            dirs::data_local_dir()
                .ok_or("无法获取 LocalAppData 目录")?
                .join("github-copilot")
                .join("apps.json"),
        );
    } else {
        if let Some(config_dir) = dirs::config_dir() {
            candidates.push(config_dir.join("github-copilot").join("apps.json"));
        }

        let home_dir = dirs::home_dir().ok_or("无法获取用户主目录")?;
        candidates.push(home_dir.join(".config").join("github-copilot").join("apps.json"));
        candidates.push(
            home_dir
                .join("Library")
                .join("Application Support")
                .join("github-copilot")
                .join("apps.json"),
        );
        candidates.push(
            home_dir
                .join("Library")
                .join("Application Support")
                .join("GitHub Copilot")
                .join("apps.json"),
        );
    }

    candidates.sort();
    candidates.dedup();
    Ok(candidates)
}

fn try_read_token_from_legacy_apps_json() -> Result<Vec<CopilotOauthTokenCandidate>, String> {
    for config_path in get_legacy_apps_json_candidates()? {
        if !config_path.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("读取 apps.json 失败 ({}): {}", config_path.display(), e))?;
        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("解析 apps.json 失败 ({}): {}", config_path.display(), e))?;

        if let Some(token) = scan_json_for_copilot_token(&value) {
            return Ok(vec![build_token_candidate(
                token,
                "旧版 Copilot apps.json",
                config_path.display().to_string(),
            )]);
        }
    }

    Ok(Vec::new())
}

fn try_read_token_from_github_cli() -> Result<Vec<CopilotOauthTokenCandidate>, String> {
    let output = match Command::new("gh").args(["auth", "token"]).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("调用 gh auth token 失败: {}", e)),
    };

    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr);
        if !error_message.trim().is_empty() {
            warn!(error = %error_message.trim(), "[CopilotLSP] gh auth token did not return a usable token");
        }
        return Ok(Vec::new());
    }

    let token = String::from_utf8(output.stdout)
        .map_err(|e| format!("解析 gh auth token 输出失败: {}", e))?;
    let trimmed = token.trim();

    if !is_supported_copilot_token(trimmed) {
        return Ok(Vec::new());
    }

    Ok(vec![build_token_candidate(trimmed.to_string(), "GitHub CLI", "gh auth token".to_string())])
}

/// 从当前机器的 Copilot / GitHub 认证来源扫描可复用的 OAuth token 候选
#[tauri::command]
pub async fn get_copilot_oauth_token_from_config() -> Result<Vec<CopilotOauthTokenCandidate>, String>
{
    info!("[CopilotLSP] Scanning existing Copilot authorization...");
    let mut candidates = Vec::new();

    let env_candidates = try_read_token_from_env();
    if !env_candidates.is_empty() {
        info!(
            count = env_candidates.len(),
            "[CopilotLSP] Found Copilot token candidates from environment"
        );
        candidates.extend(env_candidates);
    }

    match try_read_token_from_macos_keychain() {
        Ok(found_candidates) => {
            if !found_candidates.is_empty() {
                info!(
                    count = found_candidates.len(),
                    "[CopilotLSP] Found Copilot token candidates from system keychain"
                );
                candidates.extend(found_candidates);
            }
        }
        Err(error_message) => {
            warn!(error = %error_message, "[CopilotLSP] Failed to read token from system keychain");
        }
    }

    match try_read_token_from_legacy_apps_json() {
        Ok(found_candidates) => {
            if !found_candidates.is_empty() {
                info!(
                    count = found_candidates.len(),
                    "[CopilotLSP] Found Copilot token candidates from legacy apps.json"
                );
                candidates.extend(found_candidates);
            }
        }
        Err(error_message) => {
            warn!(error = %error_message, "[CopilotLSP] Failed to read legacy Copilot apps.json");
        }
    }

    match try_read_token_from_github_cli() {
        Ok(found_candidates) => {
            if !found_candidates.is_empty() {
                info!(
                    count = found_candidates.len(),
                    "[CopilotLSP] Found Copilot token candidates from GitHub CLI"
                );
                candidates.extend(found_candidates);
            }
        }
        Err(error_message) => {
            warn!(error = %error_message, "[CopilotLSP] Failed to read token from GitHub CLI");
        }
    }

    if candidates.is_empty() {
        info!("[CopilotLSP] No reusable Copilot authorization found");
    }

    Ok(candidates)
}

/// 发送 LSP 请求
fn send_lsp_request(
    server: &mut CopilotLspServer,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    });

    let request_str =
        serde_json::to_string(&request).map_err(|e| format!("序列化请求失败: {}", e))?;

    let message = format!("Content-Length: {}\r\n\r\n{}", request_str.len(), request_str);

    debug!(method = method, id = id, "[CopilotLSP] Sending request");

    server.stdin.write_all(message.as_bytes()).map_err(|e| format!("发送请求失败: {}", e))?;

    server.stdin.flush().map_err(|e| format!("刷新缓冲区失败: {}", e))?;

    // 读取响应
    read_lsp_response(server, id)
}

/// 发送 LSP 通知（无需响应）
fn send_lsp_notification(
    server: &mut CopilotLspServer,
    method: &str,
    params: serde_json::Value,
) -> Result<(), String> {
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });

    let notification_str =
        serde_json::to_string(&notification).map_err(|e| format!("序列化通知失败: {}", e))?;

    let message = format!("Content-Length: {}\r\n\r\n{}", notification_str.len(), notification_str);

    debug!(method = method, "[CopilotLSP] Sending notification");

    server.stdin.write_all(message.as_bytes()).map_err(|e| format!("发送通知失败: {}", e))?;

    server.stdin.flush().map_err(|e| format!("刷新缓冲区失败: {}", e))?;

    Ok(())
}

/// 读取 LSP 响应
fn read_lsp_response(
    server: &mut CopilotLspServer,
    expected_id: u64,
) -> Result<serde_json::Value, String> {
    loop {
        // 读取 Content-Length 头
        let mut header_line = String::new();
        server
            .stdout_reader
            .read_line(&mut header_line)
            .map_err(|e| format!("读取响应头失败: {}", e))?;

        if !header_line.starts_with("Content-Length:") {
            continue;
        }

        let content_length: usize = header_line
            .trim()
            .strip_prefix("Content-Length:")
            .ok_or("无效的 Content-Length 头")?
            .trim()
            .parse()
            .map_err(|e| format!("解析 Content-Length 失败: {}", e))?;

        // 读取空行
        let mut empty_line = String::new();
        server
            .stdout_reader
            .read_line(&mut empty_line)
            .map_err(|e| format!("读取空行失败: {}", e))?;

        // 读取内容
        let mut content = vec![0u8; content_length];
        std::io::Read::read_exact(&mut server.stdout_reader, &mut content)
            .map_err(|e| format!("读取响应内容失败: {}", e))?;

        let content_str =
            String::from_utf8(content).map_err(|e| format!("响应内容不是有效的 UTF-8: {}", e))?;

        debug!(content = %content_str, "[CopilotLSP] Received message");

        let message: serde_json::Value =
            serde_json::from_str(&content_str).map_err(|e| format!("解析响应 JSON 失败: {}", e))?;

        // 检查是否是我们期望的响应
        if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
            if id == expected_id {
                // 检查是否有错误
                if let Some(error) = message.get("error") {
                    let error_message =
                        error.get("message").and_then(|v| v.as_str()).unwrap_or("未知错误");
                    return Err(format!("LSP 错误: {}", error_message));
                }

                return message.get("result").cloned().ok_or("响应中没有 result 字段".to_string());
            }
        }

        // 不是我们期望的响应，继续读取
        // 可能是通知或其他请求的响应
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_json_for_copilot_token_finds_nested_oauth_token() {
        let value = serde_json::json!({
            "github.com:Iv1.test": {
                "oauth_token": "gho_nested_token"
            }
        });

        assert_eq!(scan_json_for_copilot_token(&value), Some("gho_nested_token".to_string()));
    }

    #[test]
    fn scan_json_for_copilot_token_finds_plaintext_cli_token() {
        let value = serde_json::json!({
            "token": "github_pat_plaintext",
            "trusted_folders": ["/tmp/demo"]
        });

        assert_eq!(scan_json_for_copilot_token(&value), Some("github_pat_plaintext".to_string()));
    }

    #[test]
    fn scan_json_for_copilot_token_ignores_unsupported_values() {
        let value = serde_json::json!({
            "token": "ghp_classic_pat",
            "nested": {
                "oauth_token": "not-a-token"
            }
        });

        assert_eq!(scan_json_for_copilot_token(&value), None);
    }
}

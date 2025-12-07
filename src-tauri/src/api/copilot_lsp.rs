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
        Self {
            server: Arc::new(Mutex::new(None)),
        }
    }
}

/// 安装 Copilot Language Server
async fn install_copilot_lsp(app_handle: &AppHandle) -> Result<PathBuf, String> {
    info!("[CopilotLSP] Installing @github/copilot-language-server...");

    // 获取 Bun 可执行文件路径
    let bun_path = BunUtils::get_bun_executable(app_handle)
        .map_err(|e| format!("获取 Bun 路径失败: {}。请先在「预览配置」中安装 Bun", e))?;

    info!(bun_path = ?bun_path, "[CopilotLSP] Using Bun");

    // 获取安装目录
    let install_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?
        .join("copilot-lsp");

    // 创建目录
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("创建安装目录失败: {}", e))?;

    // 检查是否已安装
    let server_path = install_dir.join("node_modules/@github/copilot-language-server/dist/language-server.js");
    if server_path.exists() {
        info!(server_path = ?server_path, "[CopilotLSP] Server already installed");
        return Ok(server_path);
    }

    // 创建 package.json
    let package_json = serde_json::json!({
        "name": "aipp-copilot-lsp",
        "private": true,
        "dependencies": {
            "@github/copilot-language-server": "latest"
        }
    });

    let package_json_path = install_dir.join("package.json");
    std::fs::write(&package_json_path, serde_json::to_string_pretty(&package_json).unwrap())
        .map_err(|e| format!("写入 package.json 失败: {}", e))?;

    info!("[CopilotLSP] Running bun install...");

    // 使用 Bun 安装
    let output = Command::new(&bun_path)
        .arg("install")
        .current_dir(&install_dir)
        .output()
        .map_err(|e| format!("执行 bun install 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "[CopilotLSP] bun install failed");
        return Err(format!("安装 Copilot LSP 失败: {}", stderr));
    }

    info!("[CopilotLSP] Installation completed");

    if !server_path.exists() {
        return Err("安装完成但服务器文件不存在".to_string());
    }

    Ok(server_path)
}

/// 启动 Copilot LSP 服务器
#[tauri::command]
pub async fn start_copilot_lsp(app_handle: AppHandle) -> Result<CopilotLspStatus, String> {
    info!("[CopilotLSP] Starting Copilot Language Server...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    // 如果已经运行，返回当前状态
    if server_guard.is_some() {
        info!("[CopilotLSP] Server already running");
        return Ok(CopilotLspStatus {
            is_running: true,
            is_authorized: false,
            user: None,
            error: None,
        });
    }

    // 安装 LSP
    let server_path = install_copilot_lsp(&app_handle).await?;

    // 获取 Bun 路径
    let bun_path = BunUtils::get_bun_executable(&app_handle)
        .map_err(|e| format!("获取 Bun 路径失败: {}", e))?;

    info!(
        bun_path = ?bun_path,
        server_path = ?server_path,
        "[CopilotLSP] Starting server process"
    );

    // 启动进程
    let mut process = Command::new(&bun_path)
        .arg(&server_path)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 Copilot LSP 进程失败: {}", e))?;

    let stdin = process.stdin.take().ok_or("无法获取 stdin")?;
    let stdout = process.stdout.take().ok_or("无法获取 stdout")?;
    let stdout_reader = BufReader::new(stdout);

    let mut server = CopilotLspServer {
        process,
        stdin,
        stdout_reader,
        is_initialized: false,
        pending_requests: HashMap::new(),
    };

    // 发送 initialize 请求
    let init_result = send_lsp_request(&mut server, "initialize", serde_json::json!({
        "processId": std::process::id(),
        "clientInfo": {
            "name": "AIPP",
            "version": env!("CARGO_PKG_VERSION")
        },
        "capabilities": {},
        "rootUri": null
    }))?;

    debug!(init_result = ?init_result, "[CopilotLSP] Initialize response");

    // 发送 initialized 通知
    send_lsp_notification(&mut server, "initialized", serde_json::json!({}))?;

    // 设置编辑器信息
    let editor_info = serde_json::json!({
        "editorInfo": {
            "name": "AIPP",
            "version": env!("CARGO_PKG_VERSION")
        },
        "editorPluginInfo": {
            "name": "aipp-copilot",
            "version": "1.0.0"
        }
    });

    let _ = send_lsp_request(&mut server, "setEditorInfo", editor_info);

    server.is_initialized = true;

    info!("[CopilotLSP] Server initialized successfully");

    *server_guard = Some(server);

    Ok(CopilotLspStatus {
        is_running: true,
        is_authorized: false,
        user: None,
        error: None,
    })
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

    let server = server_guard
        .as_mut()
        .ok_or("Copilot LSP 未启动，请先启动服务")?;

    let result = send_lsp_request(server, "checkStatus", serde_json::json!({
        "localChecksOnly": false
    }))?;

    debug!(result = ?result, "[CopilotLSP] checkStatus response");

    let status: CheckStatusResult = serde_json::from_value(result)
        .map_err(|e| format!("解析状态响应失败: {}", e))?;

    info!(status = ?status, "[CopilotLSP] Status checked");

    Ok(status)
}

/// 启动 Copilot 登录流程
#[tauri::command]
pub async fn sign_in_initiate(app_handle: AppHandle) -> Result<SignInInitiateResult, String> {
    info!("[CopilotLSP] Initiating sign in...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    let server = server_guard
        .as_mut()
        .ok_or("Copilot LSP 未启动，请先启动服务")?;

    let result = send_lsp_request(server, "signInInitiate", serde_json::json!({}))?;

    debug!(result = ?result, "[CopilotLSP] signInInitiate response");

    let sign_in_result: SignInInitiateResult = serde_json::from_value(result)
        .map_err(|e| format!("解析登录响应失败: {}", e))?;

    // 如果是 device flow，自动打开浏览器
    if let SignInInitiateResult::PromptUserDeviceFlow(ref prompt) = sign_in_result {
        info!(
            user_code = %prompt.user_code,
            verification_uri = %prompt.verification_uri,
            "[CopilotLSP] Device flow started"
        );

        if let Err(e) = open::that(&prompt.verification_uri) {
            warn!(error = ?e, "[CopilotLSP] Failed to open browser");
        }
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

    let server = server_guard
        .as_mut()
        .ok_or("Copilot LSP 未启动，请先启动服务")?;

    let result = send_lsp_request(server, "signInConfirm", serde_json::json!({
        "userCode": user_code
    }))?;

    debug!(result = ?result, "[CopilotLSP] signInConfirm response");

    let status: SignInStatus = serde_json::from_value(result)
        .map_err(|e| format!("解析确认响应失败: {}", e))?;

    info!(status = ?status, "[CopilotLSP] Sign in confirmed");

    Ok(status)
}

/// 登出 Copilot
#[tauri::command]
pub async fn sign_out_copilot(app_handle: AppHandle) -> Result<(), String> {
    info!("[CopilotLSP] Signing out...");

    let state = app_handle.state::<CopilotLspState>();
    let mut server_guard = state.server.lock().await;

    let server = server_guard
        .as_mut()
        .ok_or("Copilot LSP 未启动，请先启动服务")?;

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

            Ok(CopilotLspStatus {
                is_running: true,
                is_authorized,
                user,
                error: None,
            })
        }
        Err(e) => Ok(CopilotLspStatus {
            is_running: true,
            is_authorized: false,
            user: None,
            error: Some(e),
        }),
    }
}

/// 从 apps.json 读取已存在的 OAuth token
#[tauri::command]
pub async fn get_copilot_oauth_token_from_config() -> Result<Option<String>, String> {
    info!("[CopilotLSP] Reading OAuth token from config...");

    // Copilot 官方 client_id
    let client_id = "Iv1.b507a08c87ecfe98";
    let key = format!("github.com:{}", client_id);

    // 获取配置文件路径
    // GitHub Copilot 使用 ~/.config/github-copilot/apps.json
    let config_path = dirs::home_dir()
        .ok_or("无法获取用户主目录")?
        .join(".config")
        .join("github-copilot")
        .join("apps.json");

    info!(config_path = ?config_path, "[CopilotLSP] Looking for apps.json");

    if !config_path.exists() {
        info!("[CopilotLSP] apps.json not found at {:?}", config_path);
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("读取 apps.json 失败: {}", e))?;

    let apps: HashMap<String, serde_json::Value> = serde_json::from_str(&content)
        .map_err(|e| format!("解析 apps.json 失败: {}", e))?;

    if let Some(app) = apps.get(&key) {
        if let Some(oauth_token) = app.get("oauth_token").and_then(|v| v.as_str()) {
            info!("[CopilotLSP] Found OAuth token in apps.json");
            return Ok(Some(oauth_token.to_string()));
        }
    }

    info!("[CopilotLSP] No OAuth token found for client_id {}", client_id);
    Ok(None)
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

    let request_str = serde_json::to_string(&request)
        .map_err(|e| format!("序列化请求失败: {}", e))?;

    let message = format!("Content-Length: {}\r\n\r\n{}", request_str.len(), request_str);

    debug!(method = method, id = id, "[CopilotLSP] Sending request");

    server
        .stdin
        .write_all(message.as_bytes())
        .map_err(|e| format!("发送请求失败: {}", e))?;

    server
        .stdin
        .flush()
        .map_err(|e| format!("刷新缓冲区失败: {}", e))?;

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

    let notification_str = serde_json::to_string(&notification)
        .map_err(|e| format!("序列化通知失败: {}", e))?;

    let message = format!(
        "Content-Length: {}\r\n\r\n{}",
        notification_str.len(),
        notification_str
    );

    debug!(method = method, "[CopilotLSP] Sending notification");

    server
        .stdin
        .write_all(message.as_bytes())
        .map_err(|e| format!("发送通知失败: {}", e))?;

    server
        .stdin
        .flush()
        .map_err(|e| format!("刷新缓冲区失败: {}", e))?;

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

        let content_str = String::from_utf8(content)
            .map_err(|e| format!("响应内容不是有效的 UTF-8: {}", e))?;

        debug!(content = %content_str, "[CopilotLSP] Received message");

        let message: serde_json::Value = serde_json::from_str(&content_str)
            .map_err(|e| format!("解析响应 JSON 失败: {}", e))?;

        // 检查是否是我们期望的响应
        if let Some(id) = message.get("id").and_then(|v| v.as_u64()) {
            if id == expected_id {
                // 检查是否有错误
                if let Some(error) = message.get("error") {
                    let error_message = error
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未知错误");
                    return Err(format!("LSP 错误: {}", error_message));
                }

                return message
                    .get("result")
                    .cloned()
                    .ok_or("响应中没有 result 字段".to_string());
            }
        }

        // 不是我们期望的响应，继续读取
        // 可能是通知或其他请求的响应
    }
}

use crate::db::mcp_db::MCPDatabase;
use std::path::Path;
use tauri::{AppHandle, Emitter};
use tracing::{debug, info, warn};

use super::state::OperationState;
use super::types::{PermissionDecision, PermissionRequestEvent};

/// 权限管理器
pub struct PermissionManager {
    app_handle: AppHandle,
}

impl PermissionManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    /// 加载白名单目录列表
    pub fn load_whitelist(&self) -> Vec<String> {
        match MCPDatabase::new(&self.app_handle) {
            Ok(db) => {
                let env_text: Option<String> = db
                    .conn
                    .prepare(
                        "SELECT environment_variables FROM mcp_server WHERE command = ? AND is_builtin = 1 LIMIT 1",
                    )
                    .and_then(|mut stmt| {
                        stmt.query_row(["aipp:operation"], |row| row.get::<_, Option<String>>(0))
                    })
                    .unwrap_or(None);

                if let Some(text) = env_text {
                    for line in text.lines() {
                        let line = line.trim();
                        if line.starts_with("ALLOWED_DIRECTORIES=") {
                            let dirs = line.trim_start_matches("ALLOWED_DIRECTORIES=");
                            return dirs
                                .lines()
                                .flat_map(|l| l.split('\n'))
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                        }
                    }
                }
                Vec::new()
            }
            Err(e) => {
                warn!(error = %e, "Failed to load whitelist from database");
                Vec::new()
            }
        }
    }

    /// 检查路径是否在白名单内
    pub fn is_path_allowed(&self, path: &str) -> bool {
        let whitelist = self.load_whitelist();
        if whitelist.is_empty() {
            debug!(path = %path, "Whitelist is empty, path not auto-allowed");
            return false;
        }

        let path = Path::new(path);
        let path = if path.is_relative() {
            path.canonicalize().ok()
        } else {
            Some(path.to_path_buf())
        };

        if let Some(abs_path) = path {
            for allowed_dir in &whitelist {
                let allowed = Path::new(allowed_dir);
                let allowed = if allowed.is_relative() {
                    allowed.canonicalize().ok()
                } else {
                    Some(allowed.to_path_buf())
                };

                if let Some(allowed_abs) = allowed {
                    if abs_path.starts_with(&allowed_abs) {
                        debug!(path = %abs_path.display(), allowed = %allowed_abs.display(), "Path is within whitelist");
                        return true;
                    }
                }
            }
        }

        false
    }

    /// 请求权限确认（异步等待用户响应）
    pub async fn request_permission(
        &self,
        operation_state: &OperationState,
        operation: &str,
        path: &str,
        conversation_id: Option<i64>,
    ) -> Result<PermissionDecision, String> {
        let request_id = uuid::Uuid::new_v4().to_string();

        // 创建 oneshot 通道等待用户响应
        let (tx, rx) = tokio::sync::oneshot::channel();

        // 存储待处理请求
        operation_state.store_permission_request(request_id.clone(), tx).await;

        // 发送事件到前端
        let event = PermissionRequestEvent {
            request_id: request_id.clone(),
            operation: operation.to_string(),
            path: path.to_string(),
            conversation_id,
        };

        info!(request_id = %request_id, operation = %operation, path = %path, "Requesting permission from user");

        if let Err(e) = self.app_handle.emit("operation-permission-request", &event) {
            warn!(error = %e, "Failed to emit permission request event");
            return Err("Failed to request permission".to_string());
        }

        // 等待用户响应（无超时，一直等待）
        match rx.await {
            Ok(decision) => {
                info!(request_id = %request_id, decision = ?decision, "Permission decision received");
                Ok(decision)
            }
            Err(_) => {
                warn!(request_id = %request_id, "Permission request channel closed unexpectedly");
                Err("Permission request was cancelled".to_string())
            }
        }
    }

    /// 将目录添加到白名单
    pub fn add_to_whitelist(&self, path: &str) -> Result<(), String> {
        let db = MCPDatabase::new(&self.app_handle).map_err(|e| e.to_string())?;

        // 获取当前的环境变量
        let env_text: Option<String> = db
            .conn
            .prepare(
                "SELECT environment_variables FROM mcp_server WHERE command = ? AND is_builtin = 1 LIMIT 1",
            )
            .and_then(|mut stmt| {
                stmt.query_row(["aipp:operation"], |row| row.get::<_, Option<String>>(0))
            })
            .unwrap_or(None);

        // 解析并更新白名单
        let mut env_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(text) = &env_text {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    env_map.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }

        // 获取父目录作为白名单项
        let parent_path = Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        // 更新白名单
        let mut current_dirs: Vec<String> = env_map
            .get("ALLOWED_DIRECTORIES")
            .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();

        if !current_dirs.contains(&parent_path) {
            current_dirs.push(parent_path);
            env_map.insert("ALLOWED_DIRECTORIES".to_string(), current_dirs.join("\n"));
        }

        // 重建环境变量字符串
        let new_env_text = env_map
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        // 更新数据库
        db.conn
            .execute(
                "UPDATE mcp_server SET environment_variables = ? WHERE command = ? AND is_builtin = 1",
                [&new_env_text, "aipp:operation"],
            )
            .map_err(|e| format!("Failed to update whitelist: {}", e))?;

        info!(path = %path, "Added to whitelist");
        Ok(())
    }

    /// 检查路径并在需要时请求权限
    pub async fn check_and_request_permission(
        &self,
        operation_state: &OperationState,
        operation: &str,
        path: &str,
        conversation_id: Option<i64>,
    ) -> Result<bool, String> {
        // 首先检查白名单
        if self.is_path_allowed(path) {
            return Ok(true);
        }

        // 请求用户权限
        let decision = self
            .request_permission(operation_state, operation, path, conversation_id)
            .await?;

        match decision {
            PermissionDecision::Allow => Ok(true),
            PermissionDecision::AllowAndSave => {
                // 添加到白名单
                if let Err(e) = self.add_to_whitelist(path) {
                    warn!(error = %e, "Failed to add path to whitelist, but allowing operation");
                }
                Ok(true)
            }
            PermissionDecision::Deny => Ok(false),
        }
    }
}

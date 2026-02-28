use crate::db::mcp_db::MCPDatabase;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};
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

    fn parse_allowed_directories(env_text: &str) -> Vec<String> {
        let mut dirs = Vec::new();
        let mut collecting = false;

        for raw_line in env_text.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(value) = line.strip_prefix("ALLOWED_DIRECTORIES=") {
                collecting = true;
                let value = value.trim();
                if !value.is_empty() {
                    dirs.push(value.to_string());
                }
                continue;
            }

            if collecting {
                // 下一条 KEY=VALUE 说明白名单段结束
                if line.contains('=') {
                    break;
                }
                dirs.push(line.to_string());
            }
        }

        dirs
    }

    fn normalize_absolute_path(path: &Path) -> Option<PathBuf> {
        if !path.is_absolute() {
            return None;
        }
        if path.exists() {
            return path.canonicalize().ok();
        }

        // 非存在路径也做词法归一化，避免 `..` 绕过判断
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
                Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
                Component::CurDir => {}
                Component::Normal(part) => normalized.push(part),
                Component::ParentDir => {
                    if !normalized.pop() {
                        return None;
                    }
                }
            }
        }
        Some(normalized)
    }

    fn is_conversation_artifact_workspace_path(
        &self,
        path: &str,
        conversation_id: Option<i64>,
    ) -> bool {
        let Some(conversation_id) = conversation_id else {
            return false;
        };
        let Ok(app_data_dir) = self.app_handle.path().app_data_dir() else {
            return false;
        };

        let workspace_root = app_data_dir
            .join("artifact_workspaces")
            .join(format!("conversation_{}", conversation_id));
        let Some(normalized_workspace_root) = Self::normalize_absolute_path(&workspace_root) else {
            return false;
        };
        let Some(normalized_target_path) = Self::normalize_absolute_path(Path::new(path)) else {
            return false;
        };

        normalized_target_path.starts_with(&normalized_workspace_root)
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
                    return Self::parse_allowed_directories(&text)
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
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
            Self::normalize_absolute_path(path)
        };

        if let Some(abs_path) = path {
            for allowed_dir in &whitelist {
                let allowed = Path::new(allowed_dir);
                let allowed = if allowed.is_relative() {
                    allowed.canonicalize().ok()
                } else {
                    Self::normalize_absolute_path(allowed)
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
            operation_state.remove_permission_request(&request_id).await;
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
        let mut env_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut current_dirs: Vec<String> =
            env_text.as_deref().map(Self::parse_allowed_directories).unwrap_or_default();
        if let Some(text) = &env_text {
            let mut in_allowed_directories = false;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim();
                    if key == "ALLOWED_DIRECTORIES" {
                        in_allowed_directories = true;
                        continue;
                    }
                    in_allowed_directories = false;
                    env_map.insert(key.to_string(), v.trim().to_string());
                } else if in_allowed_directories {
                    // ALLOWED_DIRECTORIES 的续行，由 parse_allowed_directories 统一处理
                    continue;
                }
            }
        }

        // 获取父目录作为白名单项
        let parent_path = Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

        // 更新白名单
        if !current_dirs.contains(&parent_path) {
            current_dirs.push(parent_path);
        }
        env_map.insert("ALLOWED_DIRECTORIES".to_string(), current_dirs.join("\n"));

        // 重建环境变量字符串
        let new_env_text =
            env_map.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join("\n");

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
        // 对会话专属 Artifact 工作区自动放行，避免每次弹窗确认
        if self.is_conversation_artifact_workspace_path(path, conversation_id) {
            debug!(
                path = %path,
                conversation_id = ?conversation_id,
                "Path auto-allowed for conversation artifact workspace"
            );
            return Ok(true);
        }

        // 首先检查白名单
        if self.is_path_allowed(path) {
            return Ok(true);
        }

        // 请求用户权限
        let decision =
            self.request_permission(operation_state, operation, path, conversation_id).await?;

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

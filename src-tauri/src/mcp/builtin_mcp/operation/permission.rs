use crate::api::operation_api::{
    emit_permission_request_event, OPERATION_PERMISSION_REQUEST_EVENT,
};
use crate::db::conversation_db::Repository;
use crate::db::{
    assistant_db::AssistantDatabase, conversation_db::ConversationDatabase, mcp_db::MCPDatabase,
};
use crate::utils::path_utils::is_path_under_trusted;
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
            let canonical = path.canonicalize().ok()?;
            // On Windows, canonicalize returns \\?\ extended-length prefix which
            // breaks starts_with comparisons against non-canonical paths.
            #[cfg(windows)]
            {
                let s = canonical.to_string_lossy();
                if let Some(stripped) = s.strip_prefix(r"\\?\") {
                    return Some(PathBuf::from(stripped));
                }
            }
            return Some(canonical);
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
        let target_path = Path::new(path);

        is_path_under_trusted(target_path, &workspace_root)
    }

    /// Get assistant_id from conversation
    fn get_assistant_id_from_conversation(&self, conversation_id: i64) -> Option<i64> {
        let conversation_db = ConversationDatabase::new(&self.app_handle).ok()?;
        conversation_db.conversation_repo().ok()?.read(conversation_id).ok()??.assistant_id
    }

    /// Check if path is in conversation trust list
    async fn is_path_in_conversation_trust_list(
        &self,
        operation_state: &OperationState,
        conversation_id: i64,
        path: &str,
    ) -> bool {
        operation_state.is_path_trusted_for_conversation(conversation_id, path).await
    }

    /// Check if path is in assistant workspace
    fn is_path_in_assistant_workspace(&self, assistant_id: i64, path: &str) -> bool {
        let assistant_db = match AssistantDatabase::new(&self.app_handle) {
            Ok(db) => db,
            Err(e) => {
                warn!(error = %e, "Failed to open assistant database");
                return false;
            }
        };

        match assistant_db.is_path_in_assistant_workspace(assistant_id, path) {
            Ok(is_trusted) => {
                if is_trusted {
                    debug!(assistant_id, path = %path, "Path is in assistant workspace");
                }
                is_trusted
            }
            Err(e) => {
                warn!(error = %e, "Failed to check assistant workspace");
                false
            }
        }
    }

    /// Add path to assistant workspace
    fn add_path_to_assistant_workspace(&self, assistant_id: i64, path: &str) -> Result<(), String> {
        let assistant_db = AssistantDatabase::new(&self.app_handle).map_err(|e| e.to_string())?;

        // 如果路径是目录，直接添加；如果是文件（或不存在），添加其父目录
        let path_to_add = {
            let p = Path::new(path);
            if p.is_dir() {
                path.to_string()
            } else {
                p.parent()
                    .map(|parent| parent.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string())
            }
        };

        assistant_db
            .add_assistant_workspace(assistant_id, &path_to_add)
            .map_err(|e| e.to_string())?;

        info!(assistant_id, path = %path_to_add, "Added path to assistant workspace");
        Ok(())
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

    /// 检查路径是否在总管家可信工作区内（从 experimental_config 读取）
    async fn is_butler_trusted_workspace(&self, path: &str) -> bool {
        let feature_state = match self.app_handle.try_state::<crate::FeatureConfigState>() {
            Some(s) => s,
            None => return false,
        };
        let config_map = feature_state.config_feature_map.lock().await;
        let experimental = match config_map.get("experimental") {
            Some(m) => m,
            None => return false,
        };

        // 若开启了信任所有工作区则直接放行
        if let Some(cfg) = experimental.get("butler_trust_all_workspaces") {
            if cfg.value == "true" {
                info!(path = %path, "Path auto-allowed: butler_trust_all_workspaces is enabled");
                return true;
            }
        }

        // 检查可信工作区列表
        let trusted_paths_raw = match experimental.get("butler_trusted_workspaces") {
            Some(cfg) => cfg.value.clone(),
            None => return false,
        };
        drop(config_map);

        Self::is_path_in_trusted_dirs(path, &trusted_paths_raw)
    }

    /// Pure path matching: check if `path` is under any trusted dir.
    /// Supports JSON array `[{"path":"...","description":"..."}]` and legacy newline-separated paths.
    fn is_path_in_trusted_dirs(path: &str, trusted_paths_raw: &str) -> bool {
        let trusted_dirs = Self::extract_trusted_paths(trusted_paths_raw);
        if trusted_dirs.is_empty() {
            return false;
        }

        let target = Path::new(path);
        let target_normalized = if target.is_relative() {
            target.canonicalize().ok()
        } else {
            Self::normalize_absolute_path(target)
        };
        let target_abs = match target_normalized {
            Some(p) => p,
            None => return false,
        };

        for dir in &trusted_dirs {
            let trusted = Path::new(dir.as_str());
            let trusted_normalized = if trusted.is_relative() {
                trusted.canonicalize().ok()
            } else {
                Self::normalize_absolute_path(trusted)
            };
            if let Some(trusted_abs) = trusted_normalized {
                if target_abs.starts_with(&trusted_abs) {
                    debug!(
                        path = %target_abs.display(),
                        trusted = %trusted_abs.display(),
                        "Path is within butler trusted workspace"
                    );
                    return true;
                }
            }
        }

        false
    }

    /// Extract path strings from trusted workspaces config value.
    /// Supports JSON array `[{"path":"..."}]` and legacy newline-separated plain paths.
    fn extract_trusted_paths(raw: &str) -> Vec<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        if trimmed.starts_with('[') {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
                let paths: Vec<String> = arr
                    .iter()
                    .filter_map(|v| {
                        let p = v.get("path")?.as_str()?.trim().to_string();
                        if p.is_empty() { None } else { Some(p) }
                    })
                    .collect();
                if !paths.is_empty() {
                    return paths;
                }
            }
        }

        trimmed
            .split('\n')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
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
        let event = PermissionRequestEvent {
            request_id: request_id.clone(),
            operation: operation.to_string(),
            path: path.to_string(),
            conversation_id,
        };

        // 存储待处理请求
        operation_state.store_permission_request(event.clone(), tx).await;

        info!(request_id = %request_id, operation = %operation, path = %path, "Requesting permission from user");

        let delivered_to_feishu = if let Some(conversation_id) = conversation_id {
            let snapshot = operation_state.get_permission_request(&request_id).await;
            if let Some(snapshot) = snapshot {
                match crate::feishu::try_deliver_operation_permission_to_feishu(
                    &self.app_handle,
                    conversation_id,
                    &snapshot,
                )
                .await
                {
                    Ok(delivered) => delivered,
                    Err(error) => {
                        warn!(
                            request_id = %request_id,
                            conversation_id,
                            error = %error,
                            "Failed to deliver operation permission to Feishu"
                        );
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        };

        if let Err(e) = emit_permission_request_event(
            &self.app_handle,
            OPERATION_PERMISSION_REQUEST_EVENT,
            conversation_id,
            &event,
        ) {
            if delivered_to_feishu {
                warn!(
                    request_id = %request_id,
                    error = %e,
                    "Operation permission frontend emit failed, but Feishu delivery is active"
                );
            } else {
                operation_state.remove_permission_request(&request_id).await;
                warn!(error = %e, "Failed to emit permission request event");
                return Err("Failed to request permission".to_string());
            }
        }

        if let Some(conversation_id) = conversation_id {
            if let Err(error) = crate::api::butler_api::emit_butler_task_permission_state_changed(
                &self.app_handle,
                conversation_id,
                "operation",
                true,
            )
            .await
            {
                warn!(
                    conversation_id,
                    error = %error,
                    "Failed to refresh Butler operation permission state"
                );
            }
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

        // 如果路径是目录，直接添加；如果是文件（或不存在），添加其父目录
        let path_to_add = {
            let p = Path::new(path);
            if p.is_dir() {
                path.to_string()
            } else {
                p.parent()
                    .map(|parent| parent.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string())
            }
        };

        info!(path = %path_to_add, "Adding to whitelist");

        // 更新白名单
        if !current_dirs.contains(&path_to_add) {
            current_dirs.push(path_to_add);
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
        // 1. 对会话专属 Artifact 工作区自动放行，避免每次弹窗确认
        if self.is_conversation_artifact_workspace_path(path, conversation_id) {
            debug!(
                path = %path,
                conversation_id = ?conversation_id,
                "Path auto-allowed for conversation artifact workspace"
            );
            return Ok(true);
        }

        // 2. 总管家可信工作区自动放行
        if self.is_butler_trusted_workspace(path).await {
            debug!(
                path = %path,
                "Path auto-allowed for butler trusted workspace"
            );
            return Ok(true);
        }

        // 3. 检查会话信任路径列表
        if let Some(conv_id) = conversation_id {
            if self.is_path_in_conversation_trust_list(operation_state, conv_id, path).await {
                debug!(
                    path = %path,
                    conversation_id = conv_id,
                    "Path auto-allowed for conversation trusted list"
                );
                return Ok(true);
            }
        }

        // 4. 检查助手工作区信任列表
        if let Some(conv_id) = conversation_id {
            if let Some(assistant_id) = self.get_assistant_id_from_conversation(conv_id) {
                if self.is_path_in_assistant_workspace(assistant_id, path) {
                    debug!(
                        path = %path,
                        assistant_id = assistant_id,
                        "Path auto-allowed for assistant workspace"
                    );
                    return Ok(true);
                }
            }
        }

        // 5. 检查全局白名单
        if self.is_path_allowed(path) {
            return Ok(true);
        }

        // 6. 请求用户权限
        let decision =
            self.request_permission(operation_state, operation, path, conversation_id).await?;

        match decision {
            PermissionDecision::Allow => Ok(true),
            PermissionDecision::AllowForConversation => {
                // 添加到会话信任列表
                if let Some(conv_id) = conversation_id {
                    let parent_path = Path::new(path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.to_string());
                    operation_state.add_conversation_trusted_path(conv_id, parent_path).await;
                }
                Ok(true)
            }
            PermissionDecision::AllowForAssistant => {
                // 添加到助手工作区
                if let Some(conv_id) = conversation_id {
                    if let Some(assistant_id) = self.get_assistant_id_from_conversation(conv_id) {
                        if let Err(e) = self.add_path_to_assistant_workspace(assistant_id, path) {
                            warn!(error = %e, "Failed to add path to assistant workspace, but allowing operation");
                        }
                    }
                }
                Ok(true)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_trusted_paths tests ---

    #[test]
    fn test_extract_json_format() {
        let json = r#"[{"path":"C:\\proj1","description":"frontend"},{"path":"C:\\proj2","description":""}]"#;
        let paths = PermissionManager::extract_trusted_paths(json);
        assert_eq!(paths, vec!["C:\\proj1", "C:\\proj2"]);
    }

    #[test]
    fn test_extract_legacy_newline_format() {
        let raw = "C:\\proj1\nC:\\proj2\n";
        let paths = PermissionManager::extract_trusted_paths(raw);
        assert_eq!(paths, vec!["C:\\proj1", "C:\\proj2"]);
    }

    #[test]
    fn test_extract_empty() {
        assert!(PermissionManager::extract_trusted_paths("").is_empty());
        assert!(PermissionManager::extract_trusted_paths("  ").is_empty());
        assert!(PermissionManager::extract_trusted_paths("[]").is_empty());
    }

    #[test]
    fn test_extract_json_skips_empty_paths() {
        let json = r#"[{"path":"","description":"empty"},{"path":"C:\\ok","description":"valid"}]"#;
        let paths = PermissionManager::extract_trusted_paths(json);
        assert_eq!(paths, vec!["C:\\ok"]);
    }

    // --- is_path_in_trusted_dirs tests ---

    #[test]
    fn test_trusted_dirs_json_match() {
        #[cfg(windows)]
        {
            let json = r#"[{"path":"C:\\Users\\admin\\projects","description":"my code"}]"#;
            assert!(PermissionManager::is_path_in_trusted_dirs(
                "C:\\Users\\admin\\projects\\myapp\\src\\main.rs",
                json
            ));
        }
        #[cfg(not(windows))]
        {
            let json = r#"[{"path":"/home/user/projects","description":"my code"}]"#;
            assert!(PermissionManager::is_path_in_trusted_dirs(
                "/home/user/projects/myapp/src/main.rs",
                json
            ));
        }
    }

    #[test]
    fn test_trusted_dirs_no_match_outside() {
        #[cfg(windows)]
        {
            let json = r#"[{"path":"C:\\Users\\admin\\projects","description":""}]"#;
            assert!(!PermissionManager::is_path_in_trusted_dirs(
                "C:\\Users\\admin\\documents\\secret.txt",
                json
            ));
        }
        #[cfg(not(windows))]
        {
            let json = r#"[{"path":"/home/user/projects","description":""}]"#;
            assert!(!PermissionManager::is_path_in_trusted_dirs(
                "/home/user/documents/secret.txt",
                json
            ));
        }
    }

    #[test]
    fn test_trusted_dirs_multiple_json() {
        #[cfg(windows)]
        {
            let json = r#"[{"path":"C:\\Users\\admin\\proj1","description":"fe"},{"path":"C:\\Users\\admin\\proj2","description":"be"}]"#;
            assert!(PermissionManager::is_path_in_trusted_dirs(
                "C:\\Users\\admin\\proj2\\file.rs", json
            ));
            assert!(!PermissionManager::is_path_in_trusted_dirs(
                "C:\\Users\\admin\\proj3\\file.rs", json
            ));
        }
        #[cfg(not(windows))]
        {
            let json = r#"[{"path":"/home/user/proj1","description":"fe"},{"path":"/home/user/proj2","description":"be"}]"#;
            assert!(PermissionManager::is_path_in_trusted_dirs(
                "/home/user/proj2/file.rs", json
            ));
            assert!(!PermissionManager::is_path_in_trusted_dirs(
                "/home/user/proj3/file.rs", json
            ));
        }
    }

    #[test]
    fn test_trusted_dirs_empty_list() {
        assert!(!PermissionManager::is_path_in_trusted_dirs("C:\\file.txt", ""));
        assert!(!PermissionManager::is_path_in_trusted_dirs("C:\\file.txt", "[]"));
        assert!(!PermissionManager::is_path_in_trusted_dirs("C:\\file.txt", "\n\n  \n"));
    }

    #[test]
    fn test_trusted_dirs_prefix_attack_prevented() {
        #[cfg(windows)]
        {
            let json = r#"[{"path":"C:\\Users\\admin\\projects","description":""}]"#;
            assert!(!PermissionManager::is_path_in_trusted_dirs(
                "C:\\Users\\admin\\projects-evil\\malware.exe", json
            ));
        }
        #[cfg(not(windows))]
        {
            let json = r#"[{"path":"/home/user/projects","description":""}]"#;
            assert!(!PermissionManager::is_path_in_trusted_dirs(
                "/home/user/projects-evil/malware", json
            ));
        }
    }

    #[test]
    fn test_trusted_dirs_legacy_format_still_works() {
        #[cfg(windows)]
        {
            let legacy = "C:\\Users\\admin\\proj1\nC:\\Users\\admin\\proj2";
            assert!(PermissionManager::is_path_in_trusted_dirs(
                "C:\\Users\\admin\\proj1\\file.txt", legacy
            ));
        }
        #[cfg(not(windows))]
        {
            let legacy = "/home/user/proj1\n/home/user/proj2";
            assert!(PermissionManager::is_path_in_trusted_dirs(
                "/home/user/proj1/file.txt", legacy
            ));
        }
    }

    #[test]
    fn test_normalize_absolute_path_removes_dotdot() {
        #[cfg(windows)]
        {
            let path = Path::new("C:\\Users\\admin\\projects\\..\\documents");
            let normalized = PermissionManager::normalize_absolute_path(path).unwrap();
            assert_eq!(normalized, PathBuf::from("C:\\Users\\admin\\documents"));
        }
        #[cfg(not(windows))]
        {
            let path = Path::new("/home/user/projects/../documents");
            let normalized = PermissionManager::normalize_absolute_path(path).unwrap();
            assert_eq!(normalized, PathBuf::from("/home/user/documents"));
        }
    }
}

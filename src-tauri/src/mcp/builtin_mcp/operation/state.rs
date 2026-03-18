use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::types::{PermissionDecision, PermissionRequestEvent};
use crate::utils::path_utils::is_path_under_trusted;

/// 文件读取记录
#[derive(Debug, Clone)]
pub struct FileReadRecord {
    /// 文件路径
    pub path: String,
    /// 读取时间（Unix 时间戳）
    pub read_time: u64,
}

/// 后台 Bash 进程信息
pub struct BashProcessInfo {
    /// 进程句柄
    pub child: Option<Child>,
    /// 输出缓冲区
    pub output_buffer: String,
    /// 是否已完成
    pub completed: bool,
    /// 退出码
    pub exit_code: Option<i32>,
    /// 上次读取位置
    pub last_read_pos: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PermissionRequestSnapshot {
    pub conversation_id: Option<i64>,
    pub event: PermissionRequestEvent,
    pub review_code: String,
    pub feishu_message_id: Option<String>,
    pub allowed_open_id: Option<String>,
    pub allowed_chat_id: Option<String>,
}

pub(crate) struct PendingPermissionRequest {
    sender: tokio::sync::oneshot::Sender<PermissionDecision>,
    conversation_id: Option<i64>,
    event: PermissionRequestEvent,
    review_code: String,
    feishu_message_id: Option<String>,
    allowed_open_id: Option<String>,
    allowed_chat_id: Option<String>,
}

pub struct PermissionRequestResolution {
    pub conversation_id: Option<i64>,
    pub delivered: bool,
}

/// 操作工具状态管理器
pub struct OperationState {
    /// 已读文件记录（路径 -> 读取记录）
    pub(crate) read_files: Arc<Mutex<HashMap<String, FileReadRecord>>>,
    /// 当前会话写入过的文件（路径 -> 写入时间戳）
    pub(crate) written_files: Arc<Mutex<HashMap<String, u64>>>,
    /// 后台 Bash 进程（bash_id -> 进程信息）
    pub(crate) bash_processes: Arc<Mutex<HashMap<String, BashProcessInfo>>>,
    /// 待处理的权限请求（request_id -> 发送通道）
    pub(crate) pending_permissions: Arc<Mutex<HashMap<String, PendingPermissionRequest>>>,
    /// 会话信任路径（conversation_id -> 信任路径列表）
    pub(crate) conversation_trusted_paths: Arc<Mutex<HashMap<i64, Vec<String>>>>,
}

impl OperationState {
    pub fn new() -> Self {
        Self {
            read_files: Arc::new(Mutex::new(HashMap::new())),
            written_files: Arc::new(Mutex::new(HashMap::new())),
            bash_processes: Arc::new(Mutex::new(HashMap::new())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            conversation_trusted_paths: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn build_permission_review_code(request_id: &str) -> String {
        let compact: String = request_id
            .chars()
            .filter(|value| value.is_ascii_alphanumeric())
            .take(6)
            .collect::<String>()
            .to_ascii_uppercase();
        if compact.is_empty() {
            "OP-UNKNOWN".to_string()
        } else {
            format!("OP-{compact}")
        }
    }

    /// 记录文件已被读取
    pub async fn record_file_read(&self, path: &str) {
        let mut files = self.read_files.lock().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        files.insert(path.to_string(), FileReadRecord { path: path.to_string(), read_time: now });
        debug!(path = %path, "Recorded file read");
    }

    /// 检查文件是否已被读取过
    pub async fn has_file_been_read(&self, path: &str) -> bool {
        let files = self.read_files.lock().await;
        files.contains_key(path)
    }

    /// 清除文件读取记录
    pub async fn clear_file_read(&self, path: &str) {
        let mut files = self.read_files.lock().await;
        files.remove(path);
    }

    /// 清除所有文件读取记录
    pub async fn clear_all_file_reads(&self) {
        let mut files = self.read_files.lock().await;
        files.clear();
    }

    /// 记录文件已被写入
    pub async fn record_file_write(&self, path: &str) {
        let mut files = self.written_files.lock().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        files.insert(path.to_string(), now);
        debug!(path = %path, "Recorded file write");
    }

    /// 检查文件是否在当前会话中已被写入过
    pub async fn has_file_been_written(&self, path: &str) -> bool {
        let files = self.written_files.lock().await;
        files.contains_key(path)
    }

    /// 清除文件写入记录
    pub async fn clear_file_write(&self, path: &str) {
        let mut files = self.written_files.lock().await;
        files.remove(path);
    }

    /// 清除所有文件写入记录
    pub async fn clear_all_file_writes(&self) {
        let mut files = self.written_files.lock().await;
        files.clear();
    }

    /// 存储后台 Bash 进程
    pub async fn store_bash_process(&self, bash_id: String, child: Child) {
        let mut processes = self.bash_processes.lock().await;
        processes.insert(
            bash_id,
            BashProcessInfo {
                child: Some(child),
                output_buffer: String::new(),
                completed: false,
                exit_code: None,
                last_read_pos: 0,
            },
        );
    }

    /// 获取后台 Bash 进程的增量输出
    pub async fn get_bash_incremental_output(
        &self,
        bash_id: &str,
    ) -> Option<(String, bool, Option<i32>)> {
        let mut processes = self.bash_processes.lock().await;
        if let Some(info) = processes.get_mut(bash_id) {
            let new_output = if info.last_read_pos < info.output_buffer.len() {
                let output = info.output_buffer[info.last_read_pos..].to_string();
                info.last_read_pos = info.output_buffer.len();
                output
            } else {
                String::new()
            };
            Some((new_output, info.completed, info.exit_code))
        } else {
            None
        }
    }

    /// 追加 Bash 进程输出
    pub async fn append_bash_output(&self, bash_id: &str, output: &str) {
        let mut processes = self.bash_processes.lock().await;
        if let Some(info) = processes.get_mut(bash_id) {
            info.output_buffer.push_str(output);
        }
    }

    /// 标记 Bash 进程已完成
    pub async fn mark_bash_completed(&self, bash_id: &str, exit_code: Option<i32>) {
        let mut processes = self.bash_processes.lock().await;
        if let Some(info) = processes.get_mut(bash_id) {
            info.completed = true;
            info.exit_code = exit_code;
            info.child = None;
        }
    }

    /// 获取 Bash 进程退出码（尝试等待进程完成）
    pub async fn get_bash_exit_code(&self, bash_id: &str) -> Option<i32> {
        let mut processes = self.bash_processes.lock().await;
        if let Some(info) = processes.get_mut(bash_id) {
            // 如果已有退出码，直接返回
            if info.exit_code.is_some() {
                return info.exit_code;
            }
            // 尝试等待进程完成获取退出码
            if let Some(ref mut child) = info.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let code = status.code();
                        info.exit_code = code;
                        return code;
                    }
                    Ok(None) => {
                        // 进程还在运行
                        return None;
                    }
                    Err(_) => {
                        return None;
                    }
                }
            }
        }
        None
    }

    /// 移除 Bash 进程记录
    pub async fn remove_bash_process(&self, bash_id: &str) {
        let mut processes = self.bash_processes.lock().await;
        processes.remove(bash_id);
    }

    /// 存储待处理的权限请求
    pub async fn store_permission_request(
        &self,
        event: PermissionRequestEvent,
        sender: tokio::sync::oneshot::Sender<PermissionDecision>,
    ) {
        let request_id = event.request_id.clone();
        let conversation_id = event.conversation_id;
        let mut pending = self.pending_permissions.lock().await;
        pending.insert(
            request_id.clone(),
            PendingPermissionRequest {
                sender,
                conversation_id,
                event,
                review_code: Self::build_permission_review_code(&request_id),
                feishu_message_id: None,
                allowed_open_id: None,
                allowed_chat_id: None,
            },
        );
    }

    /// 移除待处理权限请求（用于事件发送失败等场景）
    pub async fn remove_permission_request(&self, request_id: &str) {
        let mut pending = self.pending_permissions.lock().await;
        pending.remove(request_id);
    }

    pub async fn get_permission_request(&self, request_id: &str) -> Option<PermissionRequestSnapshot> {
        let pending = self.pending_permissions.lock().await;
        pending.get(request_id).map(|request| PermissionRequestSnapshot {
            conversation_id: request.conversation_id,
            event: request.event.clone(),
            review_code: request.review_code.clone(),
            feishu_message_id: request.feishu_message_id.clone(),
            allowed_open_id: request.allowed_open_id.clone(),
            allowed_chat_id: request.allowed_chat_id.clone(),
        })
    }

    pub async fn find_permission_request_by_review_code(
        &self,
        review_code: &str,
    ) -> Option<PermissionRequestSnapshot> {
        let normalized = review_code.trim().to_ascii_uppercase();
        let pending = self.pending_permissions.lock().await;
        pending.values().find_map(|request| {
            if request.review_code == normalized {
                Some(PermissionRequestSnapshot {
                    conversation_id: request.conversation_id,
                    event: request.event.clone(),
                    review_code: request.review_code.clone(),
                    feishu_message_id: request.feishu_message_id.clone(),
                    allowed_open_id: request.allowed_open_id.clone(),
                    allowed_chat_id: request.allowed_chat_id.clone(),
                })
            } else {
                None
            }
        })
    }

    pub async fn set_permission_feishu_delivery(
        &self,
        request_id: &str,
        feishu_message_id: Option<String>,
        allowed_open_id: Option<String>,
        allowed_chat_id: Option<String>,
    ) {
        let mut pending = self.pending_permissions.lock().await;
        if let Some(request) = pending.get_mut(request_id) {
            request.feishu_message_id = feishu_message_id;
            request.allowed_open_id = allowed_open_id;
            request.allowed_chat_id = allowed_chat_id;
        }
    }

    pub async fn list_permission_requests_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Vec<PermissionRequestSnapshot> {
        let pending = self.pending_permissions.lock().await;
        pending
            .values()
            .filter(|request| request.conversation_id == Some(conversation_id))
            .map(|request| PermissionRequestSnapshot {
                conversation_id: request.conversation_id,
                event: request.event.clone(),
                review_code: request.review_code.clone(),
                feishu_message_id: request.feishu_message_id.clone(),
                allowed_open_id: request.allowed_open_id.clone(),
                allowed_chat_id: request.allowed_chat_id.clone(),
            })
            .collect()
    }

    /// 处理权限确认
    pub async fn resolve_permission_request(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Option<PermissionRequestResolution> {
        let mut pending = self.pending_permissions.lock().await;
        pending.remove(request_id).map(|request| PermissionRequestResolution {
            conversation_id: request.conversation_id,
            delivered: request.sender.send(decision).is_ok(),
        })
    }

    pub async fn has_pending_permission_for_conversation(&self, conversation_id: i64) -> bool {
        let pending = self.pending_permissions.lock().await;
        pending.values().any(|request| request.conversation_id == Some(conversation_id))
    }

    /// 添加会话信任路径
    pub async fn add_conversation_trusted_path(&self, conversation_id: i64, path: String) {
        let mut trusted = self.conversation_trusted_paths.lock().await;
        trusted.entry(conversation_id).or_insert_with(Vec::new).push(path.clone());
        info!(conversation_id, path = %path, "Added conversation trusted path");
    }

    /// 检查路径是否在会话信任列表中（前缀匹配）
    pub async fn is_path_trusted_for_conversation(&self, conversation_id: i64, path: &str) -> bool {
        let trusted = self.conversation_trusted_paths.lock().await;
        if let Some(paths) = trusted.get(&conversation_id) {
            let target_path = Path::new(path);
            for trusted_path in paths {
                let trusted = Path::new(trusted_path);
                // 使用规范化路径比较（Windows 兼容）
                if is_path_under_trusted(target_path, trusted) {
                    debug!(path = %path, trusted_path = %trusted_path, "Path matched conversation trusted path");
                    return true;
                }
            }
        }
        false
    }

    /// 清除会话信任路径（对话结束时调用）
    pub async fn clear_conversation_trusted_paths(&self, conversation_id: i64) {
        let mut trusted = self.conversation_trusted_paths.lock().await;
        trusted.remove(&conversation_id);
        debug!(conversation_id, "Cleared conversation trusted paths");
    }

    /// 检查 Bash 进程是否存在
    pub async fn bash_process_exists(&self, bash_id: &str) -> bool {
        let processes = self.bash_processes.lock().await;
        processes.contains_key(bash_id)
    }
}

impl Default for OperationState {
    fn default() -> Self {
        Self::new()
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::Mutex;
use tracing::debug;

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

/// 操作工具状态管理器
pub struct OperationState {
    /// 已读文件记录（路径 -> 读取记录）
    pub(crate) read_files: Arc<Mutex<HashMap<String, FileReadRecord>>>,
    /// 后台 Bash 进程（bash_id -> 进程信息）
    pub(crate) bash_processes: Arc<Mutex<HashMap<String, BashProcessInfo>>>,
    /// 待处理的权限请求（request_id -> 发送通道）
    pub(crate) pending_permissions:
        Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<super::types::PermissionDecision>>>>,
}

impl OperationState {
    pub fn new() -> Self {
        Self {
            read_files: Arc::new(Mutex::new(HashMap::new())),
            bash_processes: Arc::new(Mutex::new(HashMap::new())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
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
        request_id: String,
        sender: tokio::sync::oneshot::Sender<super::types::PermissionDecision>,
    ) {
        let mut pending = self.pending_permissions.lock().await;
        pending.insert(request_id, sender);
    }

    /// 处理权限确认
    pub async fn resolve_permission_request(
        &self,
        request_id: &str,
        decision: super::types::PermissionDecision,
    ) -> bool {
        let mut pending = self.pending_permissions.lock().await;
        if let Some(sender) = pending.remove(request_id) {
            sender.send(decision).is_ok()
        } else {
            false
        }
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

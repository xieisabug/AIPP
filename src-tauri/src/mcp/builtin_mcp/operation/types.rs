use serde::{Deserialize, Serialize};

/// 文件读取请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileRequest {
    /// 文件的绝对路径
    pub file_path: String,
    /// 开始读取的行号（1-indexed，可选）
    pub offset: Option<usize>,
    /// 读取的行数限制（可选）
    pub limit: Option<usize>,
}

/// 文件读取响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileResponse {
    /// 文件路径
    pub file_path: String,
    /// 文件内容（带行号格式）
    pub content: String,
    /// 实际读取的起始行号
    pub start_line: usize,
    /// 实际读取的结束行号
    pub end_line: usize,
    /// 文件总行数
    pub total_lines: usize,
    /// 是否有更多内容
    pub has_more: bool,
}

/// 文件写入请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileRequest {
    /// 文件的绝对路径
    pub file_path: String,
    /// 要写入的完整内容
    pub content: String,
}

/// 文件写入响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileResponse {
    /// 文件路径
    pub file_path: String,
    /// 写入的字节数
    pub bytes_written: usize,
    /// 成功消息
    pub message: String,
}

/// 文件编辑请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileRequest {
    /// 文件的绝对路径
    pub file_path: String,
    /// 要查找替换的旧文本
    pub old_string: String,
    /// 替换后的新文本
    pub new_string: String,
    /// 是否替换所有匹配项（默认false，只替换第一个）
    pub replace_all: Option<bool>,
}

/// 文件编辑响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileResponse {
    /// 文件路径
    pub file_path: String,
    /// 替换的次数
    pub replacements_made: usize,
    /// 成功消息
    pub message: String,
}

/// 目录列表请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirectoryRequest {
    /// 目录路径
    pub path: String,
    /// 可选的 glob 模式过滤
    pub pattern: Option<String>,
    /// 是否递归列出子目录
    pub recursive: Option<bool>,
}

/// 目录项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// 文件/目录名
    pub name: String,
    /// 完整路径
    pub path: String,
    /// 是否为目录
    pub is_directory: bool,
    /// 文件大小（字节），目录为 None
    pub size: Option<u64>,
    /// 修改时间（Unix 时间戳）
    pub modified: Option<u64>,
}

/// 目录列表响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirectoryResponse {
    /// 目录路径
    pub path: String,
    /// 目录项列表
    pub entries: Vec<DirectoryEntry>,
    /// 总数量
    pub total_count: usize,
}

/// Bash 执行请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteBashRequest {
    /// 要执行的命令
    pub command: String,
    /// 命令描述（可选）
    pub description: Option<String>,
    /// 超时时间（毫秒，可选，默认 120000）
    pub timeout: Option<u64>,
    /// 是否后台运行（可选，默认 false）
    pub run_in_background: Option<bool>,
}

/// Bash 执行响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteBashResponse {
    /// 如果是后台运行，返回 bash_id
    pub bash_id: Option<String>,
    /// 命令输出（非后台运行时返回）
    pub output: Option<String>,
    /// 退出码（非后台运行时返回）
    pub exit_code: Option<i32>,
    /// 状态消息
    pub message: String,
}

/// 获取 Bash 输出请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetBashOutputRequest {
    /// Bash 任务 ID
    pub bash_id: String,
    /// 可选的正则过滤器
    pub filter: Option<String>,
}

/// Bash 进程状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BashProcessStatus {
    /// 运行中
    Running,
    /// 已完成
    Completed,
    /// 出错
    Error,
}

/// 获取 Bash 输出响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetBashOutputResponse {
    /// Bash 任务 ID
    pub bash_id: String,
    /// 进程状态
    pub status: BashProcessStatus,
    /// 新增输出内容（自上次读取后）
    pub output: String,
    /// 退出码（如果已完成）
    pub exit_code: Option<i32>,
}

/// 权限请求事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestEvent {
    /// 请求 ID
    pub request_id: String,
    /// 操作类型
    pub operation: String,
    /// 请求的路径
    pub path: String,
    /// 会话 ID（用于关联到特定的会话/窗口）
    pub conversation_id: Option<i64>,
}

/// 权限决策
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// 仅本次允许
    Allow,
    /// 允许并加入白名单
    AllowAndSave,
    /// 拒绝
    Deny,
}

/// 权限确认请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionConfirmRequest {
    /// 请求 ID
    pub request_id: String,
    /// 用户决策
    pub decision: PermissionDecision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_decision_serialize() {
        let allow = PermissionDecision::Allow;
        let json = serde_json::to_string(&allow).unwrap();
        assert_eq!(json, "\"allow\"");

        let allow_save = PermissionDecision::AllowAndSave;
        let json = serde_json::to_string(&allow_save).unwrap();
        assert_eq!(json, "\"allow_and_save\"");

        let deny = PermissionDecision::Deny;
        let json = serde_json::to_string(&deny).unwrap();
        assert_eq!(json, "\"deny\"");
    }

    #[test]
    fn test_permission_decision_deserialize() {
        let allow: PermissionDecision = serde_json::from_str("\"allow\"").unwrap();
        assert_eq!(allow, PermissionDecision::Allow);

        let allow_save: PermissionDecision = serde_json::from_str("\"allow_and_save\"").unwrap();
        assert_eq!(allow_save, PermissionDecision::AllowAndSave);
    }

    #[test]
    fn test_bash_process_status_serialize() {
        let running = BashProcessStatus::Running;
        let json = serde_json::to_string(&running).unwrap();
        assert_eq!(json, "\"running\"");
    }
}

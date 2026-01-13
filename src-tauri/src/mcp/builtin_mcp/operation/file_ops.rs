use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use tracing::{debug, info};

use super::permission::PermissionManager;
use super::state::OperationState;
use super::types::*;

/// 文件操作实现
pub struct FileOperations;

impl FileOperations {
    /// 默认读取行数限制
    const DEFAULT_LINE_LIMIT: usize = 2000;
    /// 每行最大字符数
    const MAX_LINE_LENGTH: usize = 2000;

    /// 读取文件内容
    pub async fn read_file(
        state: &OperationState,
        permission_manager: &PermissionManager,
        request: ReadFileRequest,
        conversation_id: Option<i64>,
    ) -> Result<ReadFileResponse, String> {
        let path = &request.file_path;

        // 检查路径是否为绝对路径
        if !Path::new(path).is_absolute() {
            return Err("File path must be absolute".to_string());
        }

        // 检查文件是否存在
        if !Path::new(path).exists() {
            return Err(format!("File not found: {}", path));
        }

        // 检查是否为目录
        if Path::new(path).is_dir() {
            return Err("Cannot read a directory. Use list_directory instead.".to_string());
        }

        // 检查权限
        let allowed = permission_manager
            .check_and_request_permission(state, "read_file", path, conversation_id)
            .await?;
        if !allowed {
            return Err("Permission denied by user".to_string());
        }

        // 读取文件
        let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap_or_default()).collect();

        let total_lines = lines.len();
        let offset = request.offset.unwrap_or(1).max(1);
        let limit = request.limit.unwrap_or(Self::DEFAULT_LINE_LIMIT);

        // 计算实际范围（1-indexed）
        let start_idx = (offset - 1).min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);

        // 构建带行号的输出（cat -n 格式）
        let mut content = String::new();
        for (idx, line) in lines[start_idx..end_idx].iter().enumerate() {
            let line_num = start_idx + idx + 1;
            // 截断过长的行
            let truncated_line = if line.len() > Self::MAX_LINE_LENGTH {
                format!("{}...[truncated]", &line[..Self::MAX_LINE_LENGTH])
            } else {
                line.clone()
            };
            content.push_str(&format!("{:>6}\t{}\n", line_num, truncated_line));
        }

        // 记录文件已读取
        state.record_file_read(path).await;

        info!(path = %path, start_line = start_idx + 1, end_line = end_idx, total_lines = total_lines, "File read successfully");

        Ok(ReadFileResponse {
            file_path: path.to_string(),
            content,
            start_line: start_idx + 1,
            end_line: end_idx,
            total_lines,
            has_more: end_idx < total_lines,
        })
    }

    /// 写入文件内容
    pub async fn write_file(
        state: &OperationState,
        permission_manager: &PermissionManager,
        request: WriteFileRequest,
        conversation_id: Option<i64>,
    ) -> Result<WriteFileResponse, String> {
        let path = &request.file_path;

        // 检查路径是否为绝对路径
        if !Path::new(path).is_absolute() {
            return Err("File path must be absolute".to_string());
        }

        // 检查权限
        let allowed = permission_manager
            .check_and_request_permission(state, "write_file", path, conversation_id)
            .await?;
        if !allowed {
            return Err("Permission denied by user".to_string());
        }

        // 如果文件存在，检查是否已读取过（read-before-write 安全机制）
        if Path::new(path).exists() {
            if !state.has_file_been_read(path).await {
                return Err(format!(
                    "Safety check failed: You must read the file before overwriting it. \
                    Use read_file to read '{}' first, or use write_file only for new files.",
                    path
                ));
            }
        }

        // 确保父目录存在
        if let Some(parent) = Path::new(path).parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent directory: {}", e))?;
            }
        }

        // 写入文件
        let mut file = File::create(path).map_err(|e| format!("Failed to create file: {}", e))?;
        let bytes = request.content.as_bytes();
        file.write_all(bytes).map_err(|e| format!("Failed to write file: {}", e))?;

        info!(path = %path, bytes = bytes.len(), "File written successfully");

        Ok(WriteFileResponse {
            file_path: path.to_string(),
            bytes_written: bytes.len(),
            message: format!("Successfully wrote {} bytes to {}", bytes.len(), path),
        })
    }

    /// 编辑文件（精确字符串替换）
    pub async fn edit_file(
        state: &OperationState,
        permission_manager: &PermissionManager,
        request: EditFileRequest,
        conversation_id: Option<i64>,
    ) -> Result<EditFileResponse, String> {
        let path = &request.file_path;

        // 检查路径是否为绝对路径
        if !Path::new(path).is_absolute() {
            return Err("File path must be absolute".to_string());
        }

        // 检查文件是否存在
        if !Path::new(path).exists() {
            return Err(format!("File not found: {}", path));
        }

        // 检查权限
        let allowed = permission_manager
            .check_and_request_permission(state, "edit_file", path, conversation_id)
            .await?;
        if !allowed {
            return Err("Permission denied by user".to_string());
        }

        // 检查是否已读取过（read-before-edit 安全机制）
        if !state.has_file_been_read(path).await {
            return Err(format!(
                "Safety check failed: You must read the file before editing it. \
                Use read_file to read '{}' first.",
                path
            ));
        }

        // 检查 old_string 和 new_string 是否相同
        if request.old_string == request.new_string {
            return Err("old_string and new_string must be different".to_string());
        }

        // 读取当前内容
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        // 计算匹配次数
        let match_count = content.matches(&request.old_string).count();

        if match_count == 0 {
            return Err(format!(
                "old_string not found in file. Make sure you're using the exact text including whitespace."
            ));
        }

        let replace_all = request.replace_all.unwrap_or(false);

        // 如果不是 replace_all 模式，且匹配次数大于1，返回错误
        if !replace_all && match_count > 1 {
            return Err(format!(
                "old_string matches {} occurrences in the file. \
                Please provide more context to make it unique, or set replace_all=true to replace all occurrences.",
                match_count
            ));
        }

        // 执行替换
        let new_content = if replace_all {
            content.replace(&request.old_string, &request.new_string)
        } else {
            content.replacen(&request.old_string, &request.new_string, 1)
        };

        // 写入文件
        fs::write(path, &new_content).map_err(|e| format!("Failed to write file: {}", e))?;

        let replacements_made = if replace_all { match_count } else { 1 };

        info!(path = %path, replacements = replacements_made, "File edited successfully");

        Ok(EditFileResponse {
            file_path: path.to_string(),
            replacements_made,
            message: format!("Successfully made {} replacement(s) in {}", replacements_made, path),
        })
    }

    /// 列出目录内容
    pub async fn list_directory(
        state: &OperationState,
        permission_manager: &PermissionManager,
        request: ListDirectoryRequest,
        conversation_id: Option<i64>,
    ) -> Result<ListDirectoryResponse, String> {
        let path = &request.path;

        // 检查路径是否为绝对路径
        if !Path::new(path).is_absolute() {
            return Err("Path must be absolute".to_string());
        }

        // 检查目录是否存在
        if !Path::new(path).exists() {
            return Err(format!("Directory not found: {}", path));
        }

        // 检查是否为目录
        if !Path::new(path).is_dir() {
            return Err(format!("Path is not a directory: {}", path));
        }

        // 检查权限
        let allowed = permission_manager
            .check_and_request_permission(state, "list_directory", path, conversation_id)
            .await?;
        if !allowed {
            return Err("Permission denied by user".to_string());
        }

        let recursive = request.recursive.unwrap_or(false);
        let pattern = request.pattern.as_deref();

        let entries = if let Some(glob_pattern) = pattern {
            // 使用 glob 模式匹配
            Self::list_with_glob(path, glob_pattern, recursive)?
        } else {
            // 普通目录列表
            Self::list_entries(path, recursive)?
        };

        let total_count = entries.len();

        info!(path = %path, count = total_count, recursive = recursive, "Directory listed successfully");

        Ok(ListDirectoryResponse { path: path.to_string(), entries, total_count })
    }

    /// 普通目录列表
    fn list_entries(path: &str, recursive: bool) -> Result<Vec<DirectoryEntry>, String> {
        let mut entries = Vec::new();

        let read_dir =
            fs::read_dir(path).map_err(|e| format!("Failed to read directory: {}", e))?;

        for entry in read_dir {
            let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
            let entry_path = entry.path();
            let metadata = entry.metadata().ok();

            let dir_entry = DirectoryEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry_path.to_string_lossy().to_string(),
                is_directory: entry_path.is_dir(),
                size: metadata
                    .as_ref()
                    .and_then(|m| if m.is_file() { Some(m.len()) } else { None }),
                modified: metadata.as_ref().and_then(|m| {
                    m.modified().ok().and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
                    })
                }),
            };

            entries.push(dir_entry);

            // 递归处理子目录
            if recursive && entry_path.is_dir() {
                if let Ok(sub_entries) =
                    Self::list_entries(&entry_path.to_string_lossy(), recursive)
                {
                    entries.extend(sub_entries);
                }
            }
        }

        // 按修改时间排序（最新的在前）
        entries.sort_by(|a, b| b.modified.cmp(&a.modified));

        Ok(entries)
    }

    /// 使用 glob 模式列出文件
    fn list_with_glob(
        base_path: &str,
        pattern: &str,
        _recursive: bool,
    ) -> Result<Vec<DirectoryEntry>, String> {
        let full_pattern = if pattern.starts_with('/') || pattern.contains(':') {
            pattern.to_string()
        } else {
            format!("{}/{}", base_path, pattern)
        };

        debug!(pattern = %full_pattern, "Searching with glob pattern");

        let mut entries = Vec::new();

        match glob::glob(&full_pattern) {
            Ok(paths) => {
                for path_result in paths {
                    match path_result {
                        Ok(path) => {
                            let metadata = fs::metadata(&path).ok();
                            let entry = DirectoryEntry {
                                name: path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                                path: path.to_string_lossy().to_string(),
                                is_directory: path.is_dir(),
                                size: metadata.as_ref().and_then(|m| {
                                    if m.is_file() {
                                        Some(m.len())
                                    } else {
                                        None
                                    }
                                }),
                                modified: metadata.as_ref().and_then(|m| {
                                    m.modified().ok().and_then(|t| {
                                        t.duration_since(std::time::UNIX_EPOCH)
                                            .ok()
                                            .map(|d| d.as_secs())
                                    })
                                }),
                            };
                            entries.push(entry);
                        }
                        Err(e) => {
                            debug!(error = %e, "Failed to resolve glob path");
                        }
                    }
                }
            }
            Err(e) => {
                return Err(format!("Invalid glob pattern: {}", e));
            }
        }

        // 按修改时间排序（最新的在前）
        entries.sort_by(|a, b| b.modified.cmp(&a.modified));

        Ok(entries)
    }
}

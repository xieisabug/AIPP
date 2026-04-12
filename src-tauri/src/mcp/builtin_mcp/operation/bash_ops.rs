use std::sync::Arc;
use std::process::Stdio;
use tauri::AppHandle;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::state::OperationState;
use super::types::*;
use crate::db::mcp_db::MCPDatabase;
use crate::utils::shell_utils::{decode_process_output, decode_process_output_line, resolve_shell, ShellCommand};

/// Bash 操作实现
pub struct BashOperations;

impl BashOperations {
    /// 默认超时时间（毫秒）
    const DEFAULT_TIMEOUT_MS: u64 = 120000;
    /// 最大超时时间（毫秒）
    const MAX_TIMEOUT_MS: u64 = 600000;
    /// 最大输出长度（字符）
    const MAX_OUTPUT_LENGTH: usize = 30000;

    fn get_configured_default_shell(app_handle: Option<&AppHandle>) -> Option<String> {
        let app_handle = app_handle?;
        let db = MCPDatabase::new(app_handle).ok()?;
        let env_text = db
            .conn
            .prepare("SELECT environment_variables FROM mcp_server WHERE command = ? AND is_builtin = 1 LIMIT 1")
            .and_then(|mut stmt| stmt.query_row(["aipp:operation"], |row| row.get::<_, Option<String>>(0)))
            .ok()
            .flatten()?;

        for raw_line in env_text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            if key.trim() == "DEFAULT_SHELL" {
                return Some(value.trim().to_string());
            }
        }

        None
    }

    fn get_shell_command(app_handle: Option<&AppHandle>, command: &str) -> Result<ShellCommand, String> {
        let preferred_shell = Self::get_configured_default_shell(app_handle);
        let shell = resolve_shell(preferred_shell.as_deref())?;
        Ok(shell.into_command(command))
    }

    /// 执行 Bash 命令
    pub async fn execute_bash(
        app_handle: Option<&AppHandle>,
        state: &OperationState,
        request: ExecuteBashRequest,
    ) -> Result<ExecuteBashResponse, String> {
        let command = &request.command;
        let run_in_background = request.run_in_background.unwrap_or(false);
        let timeout_ms =
            request.timeout.unwrap_or(Self::DEFAULT_TIMEOUT_MS).min(Self::MAX_TIMEOUT_MS);
        let shell_command = Self::get_shell_command(app_handle, command)?;

        info!(
            shell = %shell_command.program,
            command = %command,
            background = run_in_background,
            timeout_ms = timeout_ms,
            "Executing bash command"
        );

        if run_in_background {
            // 后台执行
            Self::execute_background(state, &shell_command, command).await
        } else {
            // 前台执行（等待完成）
            Self::execute_foreground(&shell_command, timeout_ms).await
        }
    }

    /// 前台执行命令
    async fn execute_foreground(
        shell_command: &ShellCommand,
        timeout_ms: u64,
    ) -> Result<ExecuteBashResponse, String> {
        let mut cmd = Command::new(&shell_command.program);
        cmd.args(&shell_command.args).stdout(Stdio::piped()).stderr(Stdio::piped());

        let child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {}", e))?;

        // 等待命令完成，带超时
        let result = timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                let mut combined_output = String::new();
                combined_output.push_str(&decode_process_output(&output.stdout));
                if !output.stderr.is_empty() {
                    combined_output.push_str("\n[stderr]\n");
                    combined_output.push_str(&decode_process_output(&output.stderr));
                }

                // 截断过长输出
                let truncated = if combined_output.len() > Self::MAX_OUTPUT_LENGTH {
                    format!(
                        "{}...\n[Output truncated at {} characters]",
                        &combined_output[..Self::MAX_OUTPUT_LENGTH],
                        Self::MAX_OUTPUT_LENGTH
                    )
                } else {
                    combined_output
                };

                let exit_code = output.status.code();

                Ok(ExecuteBashResponse {
                    bash_id: None,
                    output: Some(truncated),
                    exit_code,
                    message: format!("Command completed with exit code {:?}", exit_code),
                })
            }
            Ok(Err(e)) => Err(format!("Command execution failed: {}", e)),
            Err(_) => Err(format!(
                "Command timed out after {} ms. Consider using run_in_background=true for long-running commands.",
                timeout_ms
            )),
        }
    }

    /// 后台执行命令
    async fn execute_background(
        state: &OperationState,
        shell_command: &ShellCommand,
        command: &str,
    ) -> Result<ExecuteBashResponse, String> {
        let bash_id = Uuid::new_v4().to_string();

        info!(
            bash_id = %bash_id,
            shell = %shell_command.program,
            shell_args = ?shell_command.args,
            command = %command,
            "Spawning background command"
        );

        let mut cmd = Command::new(&shell_command.program);
        cmd.args(&shell_command.args).stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            error!(error = %e, command = %command, "Failed to spawn background command");
            format!("Failed to spawn command: {}", e)
        })?;

        info!(bash_id = %bash_id, "Background process spawned successfully");

        // 获取输出流
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // 存储进程
        state.store_bash_process(bash_id.clone(), child).await;

        // 启动后台任务读取输出
        let state_clone = state.clone();
        let bash_id_clone = bash_id.clone();
        let command_clone = command.to_string();

        tokio::spawn(async move {
            Self::read_output_streams(&state_clone, &bash_id_clone, &command_clone, stdout, stderr)
                .await;
        });

        info!(bash_id = %bash_id, "Command started in background");

        Ok(ExecuteBashResponse {
            bash_id: Some(bash_id.clone()),
            output: None,
            exit_code: None,
            message: format!(
                "Command started in background. Use get_bash_output with bash_id='{}' to check output.",
                bash_id
            ),
        })
    }

    /// 读取输出流
    async fn read_output_streams(
        state: &OperationState,
        bash_id: &str,
        command: &str,
        stdout: Option<tokio::process::ChildStdout>,
        stderr: Option<tokio::process::ChildStderr>,
    ) {
        info!(bash_id = %bash_id, command = %command, "Starting to read output streams");

        if stdout.is_none() && stderr.is_none() {
            warn!(bash_id = %bash_id, "Both stdout and stderr are None, nothing to read");
            state.mark_bash_completed(bash_id, Some(0)).await;
            return;
        }

        let state = Arc::new(state.clone());
        let mut tasks = Vec::new();

        if let Some(stdout) = stdout {
            let state = Arc::clone(&state);
            let bash_id = bash_id.to_string();
            tasks.push(tokio::spawn(async move {
                Self::read_single_stream(state, bash_id, stdout, false).await;
            }));
        }

        if let Some(stderr) = stderr {
            let state = Arc::clone(&state);
            let bash_id = bash_id.to_string();
            tasks.push(tokio::spawn(async move {
                Self::read_single_stream(state, bash_id, stderr, true).await;
            }));
        }

        for task in tasks {
            if let Err(e) = task.await {
                error!(bash_id = %bash_id, error = %e, "Output stream task failed");
            }
        }

        // 尝试获取退出码
        let exit_code = state.get_bash_exit_code(bash_id).await;
        info!(bash_id = %bash_id, exit_code = ?exit_code, command = %command, "Background process completed");

        // 标记进程已完成
        state.mark_bash_completed(bash_id, exit_code).await;
    }

    async fn read_single_stream<R>(
        state: Arc<OperationState>,
        bash_id: String,
        mut reader: R,
        is_stderr: bool,
    ) where
        R: AsyncRead + Unpin,
    {
        let mut buffer = [0u8; 4096];
        let mut pending = Vec::new();

        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => {
                    if !pending.is_empty() {
                        Self::append_decoded_line(&state, &bash_id, &pending, is_stderr).await;
                    }
                    break;
                }
                Ok(read) => {
                    pending.extend_from_slice(&buffer[..read]);
                    while let Some(newline_index) = pending.iter().position(|byte| *byte == b'\n') {
                        let line = pending.drain(..=newline_index).collect::<Vec<_>>();
                        Self::append_decoded_line(&state, &bash_id, &line, is_stderr).await;
                    }
                }
                Err(e) => {
                    let label = if is_stderr { "stderr" } else { "stdout" };
                    error!(bash_id = %bash_id, error = %e, stream = %label, "Error reading process output");
                    state
                        .append_bash_output(&bash_id, &format!("[error reading {}: {}]\n", label, e))
                        .await;
                    break;
                }
            }
        }
    }

    async fn append_decoded_line(
        state: &OperationState,
        bash_id: &str,
        bytes: &[u8],
        is_stderr: bool,
    ) {
        let line = decode_process_output_line(bytes);
        debug!(bash_id = %bash_id, line = %line, is_stderr, "Process output line");

        let formatted = if is_stderr {
            format!("[stderr] {}\n", line)
        } else {
            format!("{}\n", line)
        };
        state.append_bash_output(bash_id, &formatted).await;
    }

    /// 获取 Bash 输出
    pub async fn get_bash_output(
        state: &OperationState,
        request: GetBashOutputRequest,
    ) -> Result<GetBashOutputResponse, String> {
        let bash_id = &request.bash_id;

        // 检查进程是否存在
        if !state.bash_process_exists(bash_id).await {
            return Err(format!("Bash process not found: {}", bash_id));
        }

        // 获取增量输出
        let (output, completed, exit_code) = state
            .get_bash_incremental_output(bash_id)
            .await
            .ok_or_else(|| format!("Failed to get output for bash_id: {}", bash_id))?;

        // 可选的正则过滤
        let filtered_output = if let Some(filter) = &request.filter {
            match regex::Regex::new(filter) {
                Ok(re) => {
                    output.lines().filter(|line| re.is_match(line)).collect::<Vec<_>>().join("\n")
                }
                Err(e) => {
                    warn!(error = %e, filter = %filter, "Invalid regex filter, returning unfiltered output");
                    output
                }
            }
        } else {
            output
        };

        let status = if completed {
            if exit_code == Some(0) {
                BashProcessStatus::Completed
            } else {
                BashProcessStatus::Error
            }
        } else {
            BashProcessStatus::Running
        };

        Ok(GetBashOutputResponse {
            bash_id: bash_id.clone(),
            status,
            output: filtered_output,
            exit_code,
        })
    }
}

impl Clone for OperationState {
    fn clone(&self) -> Self {
        Self {
            read_files: self.read_files.clone(),
            written_files: self.written_files.clone(),
            bash_processes: self.bash_processes.clone(),
            pending_permissions: self.pending_permissions.clone(),
            conversation_trusted_paths: self.conversation_trusted_paths.clone(),
        }
    }
}

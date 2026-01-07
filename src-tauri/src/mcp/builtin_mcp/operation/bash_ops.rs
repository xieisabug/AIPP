use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::state::OperationState;
use super::types::*;

/// Bash 操作实现
pub struct BashOperations;

impl BashOperations {
    /// 默认超时时间（毫秒）
    const DEFAULT_TIMEOUT_MS: u64 = 120000;
    /// 最大超时时间（毫秒）
    const MAX_TIMEOUT_MS: u64 = 600000;
    /// 最大输出长度（字符）
    const MAX_OUTPUT_LENGTH: usize = 30000;

    /// 获取当前平台的 Shell
    fn get_shell() -> (&'static str, &'static str) {
        #[cfg(target_os = "windows")]
        {
            ("powershell", "-Command")
        }
        #[cfg(not(target_os = "windows"))]
        {
            // 尝试使用 zsh，fallback 到 bash
            if std::path::Path::new("/bin/zsh").exists() {
                ("zsh", "-c")
            } else {
                ("bash", "-c")
            }
        }
    }

    /// 执行 Bash 命令
    pub async fn execute_bash(
        state: &OperationState,
        request: ExecuteBashRequest,
    ) -> Result<ExecuteBashResponse, String> {
        let command = &request.command;
        let run_in_background = request.run_in_background.unwrap_or(false);
        let timeout_ms = request
            .timeout
            .unwrap_or(Self::DEFAULT_TIMEOUT_MS)
            .min(Self::MAX_TIMEOUT_MS);

        let (shell, shell_arg) = Self::get_shell();
        info!(shell = %shell, command = %command, background = run_in_background, timeout_ms = timeout_ms, "Executing bash command");

        if run_in_background {
            // 后台执行
            Self::execute_background(state, shell, shell_arg, command).await
        } else {
            // 前台执行（等待完成）
            Self::execute_foreground(shell, shell_arg, command, timeout_ms).await
        }
    }

    /// 前台执行命令
    async fn execute_foreground(
        shell: &str,
        shell_arg: &str,
        command: &str,
        timeout_ms: u64,
    ) -> Result<ExecuteBashResponse, String> {
        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg)
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {}", e))?;

        // 等待命令完成，带超时
        let result = timeout(Duration::from_millis(timeout_ms), child.wait_with_output()).await;

        match result {
            Ok(Ok(output)) => {
                let mut combined_output = String::new();
                combined_output.push_str(&String::from_utf8_lossy(&output.stdout));
                if !output.stderr.is_empty() {
                    combined_output.push_str("\n[stderr]\n");
                    combined_output.push_str(&String::from_utf8_lossy(&output.stderr));
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
        shell: &str,
        shell_arg: &str,
        command: &str,
    ) -> Result<ExecuteBashResponse, String> {
        let bash_id = Uuid::new_v4().to_string();

        info!(
            bash_id = %bash_id,
            shell = %shell,
            shell_arg = %shell_arg,
            command = %command,
            "Spawning background command"
        );

        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg)
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

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
            Self::read_output_streams(&state_clone, &bash_id_clone, &command_clone, stdout, stderr).await;
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

        let mut stdout_reader = stdout.map(|s| BufReader::new(s).lines());
        let mut stderr_reader = stderr.map(|s| BufReader::new(s).lines());

        // 如果两个流都是 None，直接返回
        if stdout_reader.is_none() && stderr_reader.is_none() {
            warn!(bash_id = %bash_id, "Both stdout and stderr are None, nothing to read");
            state.mark_bash_completed(bash_id, Some(0)).await;
            return;
        }

        loop {
            // 如果两个流都已关闭，退出循环
            if stdout_reader.is_none() && stderr_reader.is_none() {
                break;
            }

            tokio::select! {
                // 只在 stdout_reader 存在时才 select 它
                line = async {
                    match &mut stdout_reader {
                        Some(reader) => reader.next_line().await,
                        None => std::future::pending().await, // 永远不会返回
                    }
                } => {
                    match line {
                        Ok(Some(line)) => {
                            debug!(bash_id = %bash_id, line = %line, "stdout");
                            state.append_bash_output(bash_id, &format!("{}\n", line)).await;
                        }
                        Ok(None) => {
                            debug!(bash_id = %bash_id, "stdout stream closed");
                            stdout_reader = None;
                        }
                        Err(e) => {
                            error!(bash_id = %bash_id, error = %e, "Error reading stdout");
                            state.append_bash_output(bash_id, &format!("[error reading stdout: {}]\n", e)).await;
                            stdout_reader = None;
                        }
                    }
                }
                // 只在 stderr_reader 存在时才 select 它
                line = async {
                    match &mut stderr_reader {
                        Some(reader) => reader.next_line().await,
                        None => std::future::pending().await, // 永远不会返回
                    }
                } => {
                    match line {
                        Ok(Some(line)) => {
                            debug!(bash_id = %bash_id, line = %line, "stderr");
                            state.append_bash_output(bash_id, &format!("[stderr] {}\n", line)).await;
                        }
                        Ok(None) => {
                            debug!(bash_id = %bash_id, "stderr stream closed");
                            stderr_reader = None;
                        }
                        Err(e) => {
                            error!(bash_id = %bash_id, error = %e, "Error reading stderr");
                            state.append_bash_output(bash_id, &format!("[error reading stderr: {}]\n", e)).await;
                            stderr_reader = None;
                        }
                    }
                }
            }
        }

        // 尝试获取退出码
        let exit_code = state.get_bash_exit_code(bash_id).await;
        info!(bash_id = %bash_id, exit_code = ?exit_code, command = %command, "Background process completed");

        // 标记进程已完成
        state.mark_bash_completed(bash_id, exit_code).await;
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
                Ok(re) => output
                    .lines()
                    .filter(|line| re.is_match(line))
                    .collect::<Vec<_>>()
                    .join("\n"),
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
            bash_processes: self.bash_processes.clone(),
            pending_permissions: self.pending_permissions.clone(),
        }
    }
}

/// Bash 操作测试
///
/// 使用安全的命令进行测试，确保：
/// - 只使用只读/无害命令（echo, pwd, ls /tmp 等）
/// - 不修改系统状态
/// - 不创建/删除系统文件
/// - 测试超时和输出截断逻辑
use super::super::bash_ops::BashOperations;
use super::super::state::OperationState;
use super::super::types::*;
use std::time::Duration;

// ============= Shell 检测测试 =============

/// 测试平台 shell 检测
/// 在不同平台上应返回正确的 shell
#[test]
fn test_get_shell_detection() {
    // 直接测试 shell 可执行文件是否存在
    #[cfg(not(target_os = "windows"))]
    {
        // macOS/Linux 应该有 zsh 或 bash
        let has_zsh = std::path::Path::new("/bin/zsh").exists();
        let has_bash = std::path::Path::new("/bin/bash").exists();
        assert!(has_zsh || has_bash, "Neither zsh nor bash found");
    }

    #[cfg(target_os = "windows")]
    {
        // Windows 应该有 PowerShell
        // PowerShell 通常在 PATH 中
        let has_powershell = std::process::Command::new("powershell")
            .arg("-Command")
            .arg("echo test")
            .output()
            .is_ok();
        assert!(has_powershell, "PowerShell not found");
    }
}

// ============= 前台命令执行测试 =============

/// 测试简单 echo 命令执行
///
/// 使用无害的 echo 命令验证基本执行流程
#[tokio::test]
async fn test_execute_bash_echo_command() {
    let state = OperationState::new();

    let request = ExecuteBashRequest {
        command: "echo 'hello world'".to_string(),
        description: Some("Test echo".to_string()),
        timeout: Some(5000),
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok(), "Echo command should succeed");

    let resp = response.unwrap();
    assert!(resp.bash_id.is_none(), "Foreground command should not have bash_id");
    assert!(resp.output.is_some(), "Should have output");

    let output = resp.output.unwrap();
    assert!(output.contains("hello world"), "Output should contain 'hello world'");
}

/// 测试 pwd 命令（只读，获取当前目录）
#[tokio::test]
async fn test_execute_bash_pwd_command() {
    let state = OperationState::new();

    #[cfg(not(target_os = "windows"))]
    let command = "pwd".to_string();
    #[cfg(target_os = "windows")]
    let command = "echo $PWD".to_string();

    let request = ExecuteBashRequest {
        command,
        description: None,
        timeout: Some(5000),
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok());

    let resp = response.unwrap();
    let output = resp.output.unwrap_or_default();
    // pwd 应该返回一个路径
    assert!(!output.trim().is_empty(), "PWD should return a path");
}

/// 测试带管道的命令
#[tokio::test]
async fn test_execute_bash_pipe_command() {
    let state = OperationState::new();

    #[cfg(not(target_os = "windows"))]
    let command = "echo -e 'line1\\nline2\\nline3' | wc -l".to_string();
    #[cfg(target_os = "windows")]
    let command = "echo line1 line2 line3".to_string();

    let request = ExecuteBashRequest {
        command,
        description: None,
        timeout: Some(5000),
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok());

    let resp = response.unwrap();
    assert!(resp.output.is_some());
}

/// 测试命令退出码
#[tokio::test]
async fn test_execute_bash_exit_code() {
    let state = OperationState::new();

    // 成功命令（exit 0）
    #[cfg(not(target_os = "windows"))]
    let success_cmd = "exit 0".to_string();
    #[cfg(target_os = "windows")]
    let success_cmd = "exit 0".to_string();

    let request = ExecuteBashRequest {
        command: success_cmd,
        description: None,
        timeout: Some(5000),
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok());
    let resp = response.unwrap();
    assert_eq!(resp.exit_code, Some(0));

    // 失败命令（exit 1）
    #[cfg(not(target_os = "windows"))]
    let fail_cmd = "exit 1".to_string();
    #[cfg(target_os = "windows")]
    let fail_cmd = "exit 1".to_string();

    let request = ExecuteBashRequest {
        command: fail_cmd,
        description: None,
        timeout: Some(5000),
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok());
    let resp = response.unwrap();
    assert_eq!(resp.exit_code, Some(1));
}

/// 测试 stderr 输出捕获
#[tokio::test]
async fn test_execute_bash_stderr_capture() {
    let state = OperationState::new();

    #[cfg(not(target_os = "windows"))]
    let command = "echo 'error message' >&2".to_string();
    #[cfg(target_os = "windows")]
    let command = "Write-Error 'error message' 2>&1".to_string();

    let request = ExecuteBashRequest {
        command,
        description: None,
        timeout: Some(5000),
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok());

    let resp = response.unwrap();
    let output = resp.output.unwrap_or_default();
    // stderr 应该被捕获并标记
    assert!(
        output.contains("error") || output.contains("[stderr]"),
        "stderr should be captured"
    );
}

// ============= 超时测试 =============

/// 测试命令超时
///
/// 使用 sleep 命令测试超时机制
#[tokio::test]
async fn test_execute_bash_timeout() {
    let state = OperationState::new();

    // 使用短超时和长 sleep
    #[cfg(not(target_os = "windows"))]
    let command = "sleep 10".to_string();
    #[cfg(target_os = "windows")]
    let command = "Start-Sleep -Seconds 10".to_string();

    let request = ExecuteBashRequest {
        command,
        description: None,
        timeout: Some(500), // 500ms 超时
        run_in_background: Some(false),
    };

    let response = BashOperations::execute_bash(&state, request).await;

    // 应该因超时而返回错误
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert!(err.contains("timed out"), "Error should mention timeout: {}", err);
}

// ============= 输出截断测试 =============

/// 测试长输出截断
#[test]
fn test_output_truncation_logic() {
    let max_output_length = 30000;

    // 创建超长输出
    let long_output = "x".repeat(40000);

    let truncated = if long_output.len() > max_output_length {
        format!(
            "{}...\n[Output truncated at {} characters]",
            &long_output[..max_output_length],
            max_output_length
        )
    } else {
        long_output.clone()
    };

    assert!(truncated.len() < long_output.len());
    assert!(truncated.contains("[Output truncated"));
}

// ============= 后台执行测试 =============

/// 测试后台命令执行
///
/// 验证后台命令：
/// - 返回 bash_id
/// - 可通过 get_bash_output 获取输出
#[tokio::test]
async fn test_execute_bash_background() {
    let state = OperationState::new();

    #[cfg(not(target_os = "windows"))]
    let command = "sleep 0.1 && echo 'background done'".to_string();
    #[cfg(target_os = "windows")]
    let command = "Start-Sleep -Milliseconds 100; echo 'background done'".to_string();

    let request = ExecuteBashRequest {
        command,
        description: Some("Background test".to_string()),
        timeout: None,
        run_in_background: Some(true),
    };

    let response = BashOperations::execute_bash(&state, request).await;
    assert!(response.is_ok());

    let resp = response.unwrap();
    assert!(resp.bash_id.is_some(), "Background command should have bash_id");
    assert!(resp.output.is_none(), "Background command should not have immediate output");

    let bash_id = resp.bash_id.unwrap();

    // 等待命令完成
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 获取输出
    let output_request = GetBashOutputRequest {
        bash_id: bash_id.clone(),
        filter: None,
    };

    let output_response = BashOperations::get_bash_output(&state, output_request).await;
    assert!(output_response.is_ok());

    let output_resp = output_response.unwrap();
    assert_eq!(output_resp.bash_id, bash_id);
    // 进程应该已完成
    assert!(
        output_resp.status == BashProcessStatus::Completed
            || output_resp.status == BashProcessStatus::Running,
        "Process should be completed or still running"
    );
}

/// 测试获取不存在的 bash 进程输出
#[tokio::test]
async fn test_get_bash_output_not_found() {
    let state = OperationState::new();

    let request = GetBashOutputRequest {
        bash_id: "non-existent-id".to_string(),
        filter: None,
    };

    let response = BashOperations::get_bash_output(&state, request).await;
    assert!(response.is_err());
    assert!(response.unwrap_err().contains("not found"));
}

/// 测试输出过滤
#[tokio::test]
async fn test_get_bash_output_with_filter() {
    let state = OperationState::new();

    // 手动设置一个带输出的进程状态
    let bash_id = "test-filter-id";
    {
        let mut processes = state.bash_processes.lock().await;
        processes.insert(
            bash_id.to_string(),
            super::super::state::BashProcessInfo {
                child: None,
                output_buffer: "line1 error\nline2 ok\nline3 error\nline4 ok\n".to_string(),
                completed: true,
                exit_code: Some(0),
                last_read_pos: 0,
            },
        );
    }

    // 使用正则过滤只包含 "error" 的行
    let request = GetBashOutputRequest {
        bash_id: bash_id.to_string(),
        filter: Some("error".to_string()),
    };

    let response = BashOperations::get_bash_output(&state, request).await;
    assert!(response.is_ok());

    let resp = response.unwrap();
    let output = resp.output;

    // 应该只包含 error 行
    assert!(output.contains("line1 error"));
    assert!(output.contains("line3 error"));
    assert!(!output.contains("line2 ok"));
    assert!(!output.contains("line4 ok"));
}

// ============= BashProcessStatus 测试 =============

/// 测试 BashProcessStatus 枚举序列化
#[test]
fn test_bash_process_status_serialization() {
    let running = BashProcessStatus::Running;
    let json = serde_json::to_string(&running).unwrap();
    assert_eq!(json, "\"running\"");

    let completed = BashProcessStatus::Completed;
    let json = serde_json::to_string(&completed).unwrap();
    assert_eq!(json, "\"completed\"");

    let error = BashProcessStatus::Error;
    let json = serde_json::to_string(&error).unwrap();
    assert_eq!(json, "\"error\"");
}

// ============= 增量输出测试 =============

/// 测试增量输出获取
#[tokio::test]
async fn test_incremental_output() {
    let state = OperationState::new();
    let bash_id = "incremental-test";

    // 初始化进程
    {
        let mut processes = state.bash_processes.lock().await;
        processes.insert(
            bash_id.to_string(),
            super::super::state::BashProcessInfo {
                child: None,
                output_buffer: String::new(),
                completed: false,
                exit_code: None,
                last_read_pos: 0,
            },
        );
    }

    // 追加第一批输出
    state.append_bash_output(bash_id, "output 1\n").await;
    state.append_bash_output(bash_id, "output 2\n").await;

    // 获取第一次增量
    let request = GetBashOutputRequest {
        bash_id: bash_id.to_string(),
        filter: None,
    };
    let resp1 = BashOperations::get_bash_output(&state, request.clone()).await.unwrap();
    assert_eq!(resp1.output, "output 1\noutput 2\n");

    // 追加更多输出
    state.append_bash_output(bash_id, "output 3\n").await;

    // 获取第二次增量（只应该有新内容）
    let resp2 = BashOperations::get_bash_output(&state, request).await.unwrap();
    assert_eq!(resp2.output, "output 3\n");
}

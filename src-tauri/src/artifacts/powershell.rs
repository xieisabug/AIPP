use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;

use crate::utils::background_process::background_command;
use crate::utils::shell_utils::{decode_process_output_line, resolve_powershell_executable, wrap_powershell_command};

pub fn run_powershell(script: &str) -> Result<String, String> {
    let shell = resolve_powershell_executable()?;
    let mut child = background_command(&shell)
        .args(["-NonInteractive", "-Command"])
        .arg(wrap_powershell_command(script))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 {} 失败：{e}", shell.display()))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let (tx, rx) = mpsc::channel();

    // 读取标准输出
    let tx_stdout = tx.clone();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let decoded = decode_process_output_line(&line);
                    let _ = tx_stdout.send(format!("stdout: {}", decoded));
                }
                Err(err) => {
                    let _ = tx_stdout.send(format!("stderr: [error reading stdout: {}]", err));
                    break;
                }
            }
        }
    });

    // 读取标准错误
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let decoded = decode_process_output_line(&line);
                    let _ = tx.send(format!("stderr: {}", decoded));
                }
                Err(err) => {
                    let _ = tx.send(format!("stderr: [error reading stderr: {}]", err));
                    break;
                }
            }
        }
    });

    let mut output = String::new();

    for line in rx {
        output.push_str(&line);
        output.push('\n');
    }
    let status = child.wait().map_err(|e| format!("等待 {} 退出失败：{e}", shell.display()))?;
    if status.success() {
        Ok(output)
    } else {
        Err(format!("{} 退出状态 {status}\n{output}", shell.display()))
    }
}

/// The code block toolbar opens a regular shell for user input. No script is passed to it.
#[cfg(windows)]
#[tauri::command]
pub fn open_interactive_powershell() -> Result<(), String> {
    let path = resolve_powershell_executable()?;
    open::that(&path).map_err(|e| format!("打开 {} 失败：{e}", path.display()))
}

#[cfg(all(test, windows))]
#[path = "powershell_tests.rs"]
mod tests;

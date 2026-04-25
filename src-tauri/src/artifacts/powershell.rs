use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::utils::shell_utils::{
    decode_process_output_line, resolve_powershell_shell, wrap_powershell_command, ShellCommand,
};

pub fn run_powershell(script: &str) -> Result<String, Box<dyn std::error::Error>> {
    let shell = resolve_powershell_shell()
        .map(|shell| shell.into_command(script))
        .unwrap_or_else(|_| ShellCommand {
            program: "powershell".to_string(),
            args: vec!["-Command".to_string(), wrap_powershell_command(script)],
        });

    let mut command_args = Vec::with_capacity(shell.args.len() + 1);
    command_args.push("-NoExit".to_string());
    command_args.extend(shell.args);

    let mut child = Command::new(shell.program)
        .args(command_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

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

    // 持续读取输出，直到进程结束
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(line) => {
                println!("{}", line); // 实时打印输出
                output.push_str(&line);
                output.push('\n');
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 检查进程是否仍在运行
                match child.try_wait() {
                    Ok(Some(status)) => {
                        output.push_str(&format!("PowerShell 执行完成，退出状态: {}", status));
                        break;
                    }
                    Ok(None) => continue, // 进程仍在运行
                    Err(e) => return Err(Box::new(e)),
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // 确保子进程已经完全退出
    let _ = child.wait_with_output()?;

    Ok(output)
}

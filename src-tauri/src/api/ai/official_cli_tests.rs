use super::official_cli::resolve;
#[cfg(windows)]
use super::official_cli::find_in_dirs;

#[tokio::test]
async fn test_explicit_cli_path_is_strict() {
    let file = std::env::current_exe().unwrap();
    assert_eq!(resolve(file.to_str().unwrap()).await.unwrap(), file);
    let missing = file.with_file_name("missing-configured-cli");
    assert!(resolve(missing.to_str().unwrap()).await.unwrap_err().contains(missing.to_str().unwrap()));
}

#[cfg(windows)]
#[test]
fn test_windows_cli_shims_and_directory_order() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first location");
    let second = root.path().join("second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(first.join("claude"), "#!/bin/sh").unwrap();
    std::fs::write(first.join("claude.cmd"), "@echo test-cli 1.2.3").unwrap();
    std::fs::write(second.join("claude.exe"), "").unwrap();
    let dirs = [first.clone(), second.clone()];
    let path = find_in_dirs("claude", &dirs).unwrap();
    assert_eq!(path, first.join("claude.cmd"));
    let output = super::official_cli::command(&path).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "test-cli 1.2.3");
    std::fs::remove_file(path).unwrap();
    assert_eq!(find_in_dirs("claude", &dirs).unwrap(), second.join("claude.exe"));
    let error = find_in_dirs("codex", &dirs).unwrap_err();
    assert!(error.contains(first.to_str().unwrap()) && error.contains(second.to_str().unwrap()));
}

/// Explicit local verification; no model prompt is sent.
#[tokio::test]
#[ignore = "requires local Codex and Claude Code installations"]
async fn test_installed_official_clis_initialize() {
    use serde_json::{json, Value};
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    for name in ["codex", "claude"] {
        let path = resolve(name).await.unwrap();
        let mut command = tokio::process::Command::from(super::official_cli::command(&path));
        let (request, response_id) = if name == "codex" {
            command.args(["app-server", "--listen", "stdio://"]);
            (json!({"id":1, "method":"initialize", "params":{"clientInfo":{"name":"aipp","version":"1"}}}), "/id")
        } else {
            command.args(["--print", "--verbose", "--input-format", "stream-json", "--output-format",
                "stream-json", "--no-session-persistence", "--strict-mcp-config", "--setting-sources", ""]);
            (json!({"type":"control_request","request_id":"aipp-cli-test","request":{"subtype":"initialize"}}), "/response/request_id")
        };
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::null()).kill_on_drop(true).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
        stdin.write_all(format!("{request}\n").as_bytes()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while let Some(line) = stdout.next_line().await.unwrap() {
                let Ok(frame) = serde_json::from_str::<Value>(&line) else { continue };
                if frame.pointer(response_id) == request.get(if name == "codex" { "id" } else { "request_id" }) {
                    assert!(frame.get("result").is_some() || frame.pointer("/response/subtype") == Some(&json!("success")),
                        "{name} initialize did not succeed");
                    return;
                }
            }
            panic!("{name} exited before initialize");
        }).await.unwrap_or_else(|_| panic!("{name} initialize timed out"));
        drop(stdin);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
        println!("{name} initialize OK ({})", path.display());
    }
}

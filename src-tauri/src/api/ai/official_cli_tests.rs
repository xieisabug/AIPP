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

#[cfg(windows)]
#[test]
fn test_codex_uses_platform_package_pinned_by_active_pnpm_shim() {
    use super::official_cli::codex_windows::discover;
    use std::fs;
    let root = tempfile::tempdir().unwrap();
    let bin = root.path();
    let store = bin.join("global/5/.pnpm");
    let package = store.join("@openai+codex@1.2.3/node_modules/@openai/codex");
    fs::create_dir_all(package.join("bin")).unwrap();
    fs::write(bin.join("codex.cmd"), r#"node "%~dp0\global\5\.pnpm\@openai+codex@1.2.3\node_modules\@openai\codex\bin\codex.js" %*"#).unwrap();
    let (arch, triple) = match std::env::consts::ARCH {
        "x86_64" => ("x64", "x86_64-pc-windows-msvc"),
        "aarch64" => ("arm64", "aarch64-pc-windows-msvc"),
        _ => return,
    };
    let alias = format!("@openai/codex-win32-{arch}");
    let version = format!("1.2.3-win32-{arch}");
    fs::write(package.join("package.json"), serde_json::json!({
        "name": "@openai/codex", "version": "1.2.3",
        "optionalDependencies": { (alias): format!("npm:@openai/codex@{version}") }
    }).to_string()).unwrap();
    // No optional-dependency junction. Only the platform package in the virtual store survives.
    let platform = store.join(format!("@openai+codex@{version}/node_modules/@openai/codex"));
    let native = platform.join("vendor").join(triple).join("bin/codex.exe");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(&native, "").unwrap();
    let manifest = platform.join("package.json");
    fs::write(&manifest, r#"{"name":"@openai/codex","version":"0.9.0"}"#).unwrap();
    let dirs = [bin.to_owned()];
    assert_eq!(discover(&dirs).unwrap()[0].path, bin.join("codex.cmd"));
    fs::write(&manifest, serde_json::json!({"name":"@openai/codex", "version":version}).to_string()).unwrap();
    let programs = discover(&dirs).unwrap();
    assert_eq!(fs::canonicalize(&programs[0].path).unwrap(), fs::canonicalize(native).unwrap());
    let cmd = programs[0].command();
    assert!(cmd.get_envs().any(|(key,value)| key == "CODEX_MANAGED_BY_PNPM" && value == Some("1".as_ref())));
}

#[cfg(windows)]
#[tokio::test]
async fn test_codex_automatic_lookup_skips_broken_install_and_reports_all_failures() {
    use super::official_cli::codex_windows::{discover, first_working};
    let root = tempfile::tempdir().unwrap();
    let dirs = [root.path().join("first"), root.path().join("second")];
    for dir in &dirs { std::fs::create_dir(dir).unwrap(); }
    let broken = "@echo Missing optional dependency @openai/codex-win32-x64 1>&2\r\n@exit /b 1";
    std::fs::write(dirs[0].join("codex.cmd"), broken).unwrap();
    let second = dirs[1].join("codex.cmd");
    std::fs::write(&second, "@echo codex-cli 1.2.3").unwrap();
    let (program, version) = first_working(discover(&dirs).unwrap()).await.unwrap();
    assert_eq!(program.path, second);
    assert_eq!(version, "1.2.3");
    // A configured path selects only that launcher, even if another installation exists.
    let first = dirs[0].join("codex.cmd");
    let command = first.to_str().unwrap();
    let explicit = super::official_cli::detect("codex", command).await.err().unwrap();
    assert_eq!(explicit.path.as_deref(), Some(first.as_path()));
    std::fs::write(&second, broken).unwrap();
    let error = first_working(discover(&dirs).unwrap()).await.err().unwrap();
    assert!(error.message.contains("Missing optional dependency"));
    for dir in dirs { assert!(error.message.contains(dir.to_str().unwrap())); }

}

/// Explicit local verification; no model prompt is sent.
#[tokio::test]
#[ignore = "requires local Codex and Claude Code installations"]
async fn test_installed_official_clis_initialize() {
    use serde_json::{json, Value};
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    for name in ["codex", "claude"] {
        let (detected, version) = super::official_cli::detect(name, name).await.unwrap();
        let program = if name == "codex" {
            let session = super::official_cli::codex_for_session(name).await.unwrap();
            assert_eq!(session.path, detected.path);
            session
        } else {
            detected
        };
        let mut command = tokio::process::Command::from(program.command());
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
        println!("{name} {version} initialize OK ({})", program.path.display());
    }
}

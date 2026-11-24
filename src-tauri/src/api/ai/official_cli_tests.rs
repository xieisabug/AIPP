use super::official_cli::resolve;
#[cfg(windows)]
use super::official_cli::find_in_dirs;

#[test]
fn test_kimi_discovery_includes_the_user_installer_directory() {
    let home = dirs::home_dir().unwrap();
    let installer_dir = home.join(".kimi-code").join("bin");
    assert!(super::agent_driver::native_agents::env::tool_dirs("kimi").contains(&installer_dir));
}

#[tokio::test]
#[ignore = "requires a local Kimi Code installation"]
async fn test_installed_kimi_acp_initialize() {
    use serde_json::{json, Value};
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (detected, version) = super::official_cli::detect("kimi", "kimi", &Default::default()).await.unwrap();
    let path = super::acp::resolve_acp_cli_path("kimi", &Default::default()).await.unwrap();
    assert_eq!(detected.path, path);
    let plan = super::acp::build_acp_launch_plan("kimi", &path, &[], &Default::default());
    let mut child = tokio::process::Command::from(super::official_cli::command(&plan.program, &Default::default()))
        .args(plan.args).envs(plan.extra_env).stdin(Stdio::piped()).stdout(Stdio::piped())
        .kill_on_drop(true).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let request = json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
        "protocolVersion":1, "clientCapabilities":{}, "clientInfo":{"name":"aipp", "version":"1"}
    }});
    stdin.write_all(format!("{request}\n").as_bytes()).await.unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            let frame: Value = serde_json::from_str(&line).unwrap();
            if frame.get("id") == Some(&json!(1)) { return frame; }
        }
        panic!("Kimi exited before replying to initialize");
    }).await.expect("Kimi initialize timed out");
    assert_eq!(response.pointer("/result/protocolVersion"), Some(&json!(1)), "{response}");
    println!("Kimi {version} ACP initialize OK ({})", path.display());
    drop(stdin);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
}

#[tokio::test]
async fn test_explicit_cli_path_is_strict() {
    let file = std::env::current_exe().unwrap();
    assert_eq!(resolve(file.to_str().unwrap(), &Default::default()).await.unwrap(), file);
    let missing = file.with_file_name("missing-configured-cli");
    assert!(resolve(missing.to_str().unwrap(), &Default::default()).await.unwrap_err().contains(missing.to_str().unwrap()));
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
    let output = super::official_cli::command(&path, &Default::default()).arg("--version").output().unwrap();
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
    let cmd = programs[0].command(&Default::default());
    assert!(cmd.get_envs().any(|(key,value)| key == "CODEX_MANAGED_BY_PNPM" && value == Some("1".as_ref())));
}

#[cfg(windows)]
#[tokio::test]
async fn test_codex_automatic_lookup_skips_broken_install_and_reports_all_failures() {
    use super::official_cli::{codex_windows::discover, first_working};
    let root = tempfile::tempdir().unwrap();
    let dirs = [root.path().join("first"), root.path().join("second")];
    for dir in &dirs { std::fs::create_dir(dir).unwrap(); }
    let broken = "@echo Missing optional dependency @openai/codex-win32-x64 1>&2\r\n@exit /b 1";
    std::fs::write(dirs[0].join("codex.cmd"), broken).unwrap();
    let second = dirs[1].join("codex.cmd");
    std::fs::write(&second, "@echo codex-cli 1.2.3").unwrap();
    let (program, version) = first_working("codex", discover(&dirs).unwrap(), &Default::default()).await.unwrap();
    assert_eq!(program.path, second);
    assert_eq!(version, "1.2.3");
    // A configured path selects only that launcher, even if another installation exists.
    let first = dirs[0].join("codex.cmd");
    let command = first.to_str().unwrap();
    let explicit = super::official_cli::detect("codex", command, &Default::default()).await.err().unwrap();
    assert_eq!(explicit.path.as_deref(), Some(first.as_path()));
    std::fs::write(&second, broken).unwrap();
    let error = first_working("codex", discover(&dirs).unwrap(), &Default::default()).await.err().unwrap();
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
        let (detected, version) = super::official_cli::detect(name, name, &Default::default()).await.unwrap();
        let program = super::official_cli::for_session(name, name, &Default::default()).await.unwrap();
        assert_eq!(program.path, detected.path);
        let mut command = tokio::process::Command::from(program.command(&Default::default()));
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

#[cfg(windows)]
#[tokio::test]
async fn test_windows_provider_path_and_env_match_settings_and_session() {
    use super::official_cli::{detect, for_session};
    use std::{collections::HashMap, fs};

    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("older install");
    let second = root.path().join("selected install");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let path = std::env::join_paths([&first, &second]).unwrap();
    let env = HashMap::from([
        ("Path".to_owned(), path.to_str().unwrap().to_owned()),
        ("REQUIRED_FOR_LAUNCH".to_owned(), "enabled".to_owned()),
        ("TEST_API_TOKEN".to_owned(), "private-fixture-token".to_owned()),
    ]);
    for kind in ["claude", "kimi", "codex", "gemini", "custom-acp"] {
        fs::write(first.join(kind), "#!/bin/sh\nexit 1").unwrap();
        let broken = first.join(format!("{kind}.cmd"));
        if matches!(kind, "claude" | "kimi" | "codex" | "gemini") {
            fs::write(&broken, "@echo %TEST_API_TOKEN% broken launcher 1>&2\r\n@exit /b 7").unwrap();
            let failure = detect(kind, broken.to_str().unwrap(), &env).await.err().unwrap();
            assert_eq!(failure.path.as_deref(), Some(broken.as_path()));
            assert!(failure.message.contains("broken launcher"));
            assert!(!failure.message.contains("private-fixture-token"));
            assert_eq!(for_session(kind, broken.to_str().unwrap(), &env).await.unwrap().path, broken);
        }
        let expected = second.join(format!("{kind}.cmd"));
        fs::write(&expected, "@if not \"%REQUIRED_FOR_LAUNCH%\"==\"enabled\" exit /b 17\r\n@echo agent 1.2.3").unwrap();
        let (detected, version) = detect(kind, kind, &env).await.unwrap();
        assert_eq!(detected.path, expected);
        assert_eq!(version, "1.2.3");
        let selected = if matches!(kind, "gemini" | "custom-acp" | "kimi") {
            super::acp::resolve_acp_cli_path(kind, &env).await.unwrap()
        } else {
            for_session(kind, kind, &env).await.unwrap().path
        };
        assert_eq!(selected, detected.path);
        let output = super::official_cli::command(&selected, &env).arg("session").output().unwrap();
        assert!(output.status.success());
    }
}

#[cfg(windows)]
#[test]
fn test_windows_ps1_host_uses_effective_path_then_builtin_windows_powershell() {
    use std::{collections::HashMap, fs};
    let dir = tempfile::tempdir().unwrap();
    let env = HashMap::from([("PATH".to_owned(), dir.path().to_str().unwrap().to_owned())]);
    let shim = dir.path().join("cli with spaces.ps1");
    let command = || super::official_cli::command(&shim, &env);
    let builtin = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    assert_eq!(command().get_program(), builtin.as_os_str());
    for host in ["powershell.exe", "pwsh.exe"] {
        fs::write(dir.path().join(host), "").unwrap();
        let cmd = command();
        assert_eq!(cmd.get_program(), dir.path().join(host).as_os_str());
        assert_eq!(cmd.get_args().last(), Some(shim.as_os_str()));
    }
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires a local Gemini CLI installation"]
async fn test_installed_gemini_with_minimal_path() {
    probe_installed_acp_with_minimal_path("gemini").await;
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires a local claude-code-acp installation"]
async fn test_installed_claude_code_acp_with_minimal_path() {
    probe_installed_acp_with_minimal_path("claude-code-acp").await;
}

#[cfg(windows)]
async fn probe_installed_acp_with_minimal_path(kind: &str) {
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    let env = std::collections::HashMap::from([("PATH".to_string(), system.display().to_string())]);
    let (program, version) = super::official_cli::detect(kind, kind, &env).await.unwrap();
    let session = super::acp::resolve_acp_cli_path(kind, &env).await.unwrap();
    assert_eq!(program.path, session);
    println!("{kind} {version} ({})", program.path.display());
}

#[test]
fn test_claude_provider_routes_official_cli_and_acp_adapter_by_entrypoint() {
    use super::agent_driver::native_agents::env::provider_settings;
    for (command, native) in [
        (r"C:\tools with spaces\Claude.CMD", true),
        ("claude", true),
        ("claude-code-acp", false),
        (r"C:\claude\bin\claude-code-acp.exe", false),
        ("/home/claude/bin/custom-acp", false),
    ] {
        let provider = provider_settings(&[("acp_cli_command".to_owned(), command.to_owned())].into());
        assert_eq!(super::claude_sdk::is_claude_code_provider("acp", &provider), native, "{command}");
    }
}

#[cfg(windows)]
#[test]
fn test_provider_and_assistant_cli_env_use_the_same_windows_path_override() {
    use super::{acp::extract_acp_config, claude_sdk::extract_claude_sdk_config,
        codex_app_server::extract_codex_app_server_config, agent_driver::native_agents::env::provider_settings};
    use crate::db::assistant_db::AssistantModelConfig;
    let providers = provider_settings(&[
        ("acp_cli_command".into(), "gemini".into()),
        ("acp_env_vars".into(), r#"{"PATH":"C:/provider path","SHARED":"provider"}"#.into()),
    ].into());
    let model = AssistantModelConfig {
        id: 1, assistant_id: 1, assistant_model_id: 1, name: "acp_env_vars".into(), value_type: "string".into(),
        value: Some("Path=C:/assistant override\nSHARED=model".into()),
    };
    for env in [
        extract_acp_config(std::slice::from_ref(&model), &providers).unwrap().env_vars,
        extract_claude_sdk_config(std::slice::from_ref(&model), &providers, None, vec![]).unwrap().env_vars,
        extract_codex_app_server_config(&[model], &providers, None).unwrap().env_vars,
    ] {
        assert_eq!(env.get("Path").map(String::as_str), Some("C:/assistant override"));
        assert!(!env.contains_key("PATH"));
        assert_eq!(env["SHARED"], "model");
    }
}

#[cfg(windows)]
#[tokio::test]
async fn test_acp_adapter_selects_second_installation_after_a_failed_initialize() {
    let root = tempfile::tempdir().unwrap();
    let bad = root.path().join("broken Bun");
    let good = root.path().join("working npm");
    std::fs::create_dir(&bad).unwrap();
    std::fs::create_dir(&good).unwrap();
    let name = "claude-code-acp";
    let broken = bad.join("claude-code-acp.cmd");
    let working = good.join("claude-code-acp.cmd");
    std::fs::write(&broken, "@set /p frame=\r\n@echo broken adapter 1>&2\r\n@exit /b 1").unwrap();
    std::fs::write(&working, concat!("@set /p frame=\r\n",
        r#"@echo {"jsonrpc":"2.0","id":1,"result":{"agentInfo":{"version":"0.13.0"}}}"#)).unwrap();
    let env = std::collections::HashMap::from([
        ("PATH".into(), std::env::join_paths([&bad, &good]).unwrap().to_str().unwrap().to_owned())
    ]);
    let strict = super::official_cli::detect(name, broken.to_str().unwrap(), &env).await.err().unwrap();
    assert_eq!(strict.path.as_deref(), Some(broken.as_path()));
    assert!(strict.message.contains("broken adapter"));
    let (detected, version) = super::official_cli::detect(name, name, &env).await.unwrap();
    assert_eq!(detected.path, working);
    assert_eq!(version, "0.13.0");
    assert_eq!(super::acp::resolve_acp_cli_path(name, &env).await.unwrap(), working);
}

#[cfg(windows)]
#[tokio::test]
async fn test_windows_package_manager_in_managed_node_and_pnpm_home() {
    let dir = tempfile::tempdir().unwrap();
    let node = dir.path().join("node manager");
    let pnpm = dir.path().join("pnpm home");
    std::fs::create_dir(&node).unwrap();
    std::fs::create_dir(&pnpm).unwrap();
    for name in ["npm.cmd", "pnpm.cmd"] { std::fs::write(node.join(name), "").unwrap(); }
    std::fs::write(pnpm.join("pnpm.cmd"), "").unwrap();
    let env = [
        ("PATH".into(), String::new()),
        ("NVM_SYMLINK".into(), node.to_str().unwrap().to_owned()),
        ("PNPM_HOME".into(), pnpm.to_str().unwrap().to_owned()),
    ].into();
    assert_eq!(resolve("pnpm", &env).await.unwrap(), pnpm.join("pnpm.cmd"));
    assert_eq!(resolve("npm", &env).await.unwrap(), node.join("npm.cmd"));
    let candidates = super::official_cli::manager_candidates("pnpm", &node, &env);
    assert_eq!(candidates[..2], [node.join("pnpm.cmd"), pnpm.join("pnpm.cmd")]);
    assert_eq!(candidates.iter().filter(|p| **p == node.join("pnpm.cmd")).count(), 1);
}

#[cfg(windows)]
#[tokio::test]
async fn test_windows_bun_shim_can_find_separate_bun_and_node_without_overriding_path() {
    use std::{collections::HashMap, fs};
    let root = tempfile::tempdir().unwrap();
    let shim = root.path().join("global adapter.exe");
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
    // A real executable observes its launch environment; no Bun/Node installation is required.
    fs::copy(system.join("where.exe"), &shim).unwrap();
    fs::write(shim.with_extension("bunx"), []).unwrap();
    let node = root.path().join("managed node");
    let bun = root.path().join("runtime/bin");
    let preferred = root.path().join("provider path");
    for path in [&node, &bun, &preferred] { fs::create_dir_all(path).unwrap(); }
    fs::write(node.join("node.exe"), []).unwrap();
    fs::write(bun.join("bun.exe"), []).unwrap();
    let path = std::env::join_paths([&preferred, &system]).unwrap();
    let env = HashMap::from([
        ("Path".into(), path.to_str().unwrap().into()),
        ("NVM_SYMLINK".into(), node.to_str().unwrap().into()),
        ("BUN_INSTALL".into(), bun.parent().unwrap().to_str().unwrap().into()),
        ("BUN_INSTALL_BIN".into(), root.path().to_str().unwrap().into()),
    ]);
    assert_eq!(resolve("global adapter", &env).await.unwrap(), shim);
    for (exe, expanded) in [("node.exe", &node), ("bun.exe", &bun)] {
        let output = super::acp::build_acp_command("custom-acp", &shim, &[exe.into()], &env)
            .current_dir(root.path()).output().unwrap();
        assert!(output.status.success());
        let reported = String::from_utf8_lossy(&output.stdout);
        assert!(reported.lines().any(|path| std::path::Path::new(path.trim()) == expanded.join(exe)));
        fs::write(preferred.join(exe), []).unwrap();
        let cmd = super::official_cli::command(&shim, &env);
        let effective = cmd.get_envs().find(|(key, _)| key.eq_ignore_ascii_case("PATH")).unwrap().1.unwrap();
        let dirs: Vec<_> = std::env::split_paths(effective).collect();
        assert_eq!(dirs[0], preferred);
        assert!(!dirs.contains(expanded));
    }
}

use super::*;

#[test]
fn test_native_agent_env_version_token_reads_common_cli_outputs() {
    assert_eq!(version_token("kiro-cli 3.2.0"), Some("3.2.0".into()));
    assert_eq!(version_token("v25.9.0\n"), Some("25.9.0".into()));
    assert_eq!(version_token("1.2.12"), Some("1.2.12".into()));
    assert_eq!(version_token("opencode 1.0.0-beta.3"), Some("1.0.0-beta.3".into()));
    assert_eq!(version_token("version: 1.5.1."), Some("1.5.1".into()));
    assert_eq!(version_token("kiro-cli"), None);
    assert_eq!(version_token("1.2"), None);
}

#[test]
fn test_native_agent_env_newer_version_only_for_valid_higher_releases() {
    assert!(is_newer("1.5.1", "1.5.2"));
    assert!(is_newer("1.2.12", "1.5.5"));
    assert!(is_newer("v22.14.0", "22.19.0"));
    assert!(!is_newer("1.5.2", "1.5.2"));
    assert!(!is_newer("2.0.0", "1.9.9"));
    assert!(!is_newer("unknown", "1.0.0"));
}

#[test]
fn test_native_agent_env_latest_sources_parse_published_versions() {
    assert_eq!(changelog_latest("# Antigravity CLI Changelog\n\n## 1.5.5\n- fix\n## 1.5.4\n"), Some("1.5.5".into()));
    assert_eq!(changelog_latest("# Changelog\n"), None);
    assert_eq!(parse_latest(LatestSource::Npm("opencode-ai"), r#"{"name":"opencode-ai","version":"1.19.5"}"#).unwrap(), "1.19.5");
    assert_eq!(parse_latest(LatestSource::KiroManifest, r#"{"version":"3.2.0","packages":[]}"#).unwrap(), "3.2.0");
    assert!(parse_latest(LatestSource::KiroManifest, r#"{"packages":[]}"#).is_err());
    assert!(parse_latest(LatestSource::AntigravityChangelog, "no headings").is_err());
    assert!(latest_source_url(LatestSource::Npm("@cursor/sdk")).ends_with("%40cursor%2Fsdk/latest"));
}

/// Candidate files must be the same file the bridge's launch() would pick in that directory.
#[test]
fn test_native_agent_env_command_in_follows_bridge_launch_order() {
    let dir = Path::new("tools");
    let files = [dir.join("pi.cmd"), dir.join("pi.ps1"), dir.join("pi")];
    let exists = |path: &Path| files.iter().any(|file| file == path);
    assert_eq!(command_in(dir, "pi", true, exists), Some(dir.join("pi.ps1")));
    assert_eq!(command_in(dir, "pi", false, exists), Some(dir.join("pi")));
    assert_eq!(command_in(dir, "pi.cmd", true, exists), Some(dir.join("pi.cmd")));
    assert_eq!(command_in(dir, "agy", true, exists), None);
}

#[test]
fn test_native_agent_env_auto_node_chooses_highest_supported_version_across_sources() {
    let node = |source: &str, version: &str| node_program(Path::new(source), source.into(), Ok(version.into()));
    let old = node("PATH", "22.18.0");
    let broken = node_program(Path::new("broken"), "fnm".into(), Err("无法运行".into()));
    let mut nodes = vec![old.clone(), node("PATH", "24.9.0"), broken.clone(), node("nvm", "24.10.0")];
    assert_eq!(rank_nodes(&mut nodes).unwrap().source, "nvm");
    assert_eq!(rank_nodes(&mut [node("fnm", "22.19.0")]).unwrap().source, "fnm");
    assert_eq!(rank_nodes(&mut [node("PATH", "24.0.0"), node("nvm", "24.0.0")]).unwrap().source, "PATH");
    assert!(rank_nodes(&mut [old, broken]).is_none());
}

#[test]
fn test_native_agent_env_selected_node_directory_is_prepended_once() {
    let (node, node_dir, other) = if cfg!(windows) { (r"C:\nvm\v22.19.0\node.exe", r"C:\nvm\v22.19.0", r"C:\Windows") } else { ("/opt/nvm/v22.19.0/bin/node", "/opt/nvm/v22.19.0/bin", "/usr/bin") };
    let joined = |parts: &[&str]| std::env::join_paths(parts).unwrap().to_string_lossy().into_owned();

    let mut env = HashMap::new();
    with_node_on_path(node, &mut env, Some(other.into()), false);
    assert_eq!(env.get("PATH"), Some(&joined(&[node_dir, other])));

    let mut already = HashMap::new();
    with_node_on_path(node, &mut already, Some(std::env::join_paths([other, node_dir]).unwrap()), false);
    assert!(already.is_empty(), "a directory already on PATH keeps its position");
    with_node_on_path(node, &mut already, Some(std::env::join_paths([other, node_dir]).unwrap()), true);
    assert_eq!(already.get("PATH"), Some(&joined(&[node_dir, other])));

    let mut bare = HashMap::new();
    with_node_on_path("node", &mut bare, Some(other.into()), false);
    assert!(bare.is_empty(), "PATH lookup keeps the inherited PATH untouched");

    let mut configured = HashMap::from([("PATH".to_string(), "custom".to_string())]);
    with_node_on_path(node, &mut configured, Some(other.into()), false);
    assert_eq!(configured.get("PATH"), Some(&joined(&[node_dir, "custom"])), "configured PATH is extended, not replaced");
}

#[tokio::test]
#[ignore = "requires a supported Node installed in a discovered location outside PATH"]
async fn test_native_agent_env_launch_with_empty_path_uses_discovered_node_for_children() {
    let mut env = HashMap::from([("PATH".to_string(), String::new())]);
    let cwd = std::env::current_dir().unwrap();
    let status = detect_node("node", &effective_path(&env), &cwd).await;
    let selected = status.selected.expect("install a supported Node in a version manager or installer directory");
    let command = resolve_launch_node("node", &mut env, &cwd).await.unwrap();
    assert_eq!(command, selected.path);
    assert_eq!(Path::new(&env["PATH"]), Path::new(&command).parent().unwrap());
    let output = tokio::process::Command::new(command).envs(env)
        .args(["-p", "require('node:child_process').execFileSync('node',['--version'],{encoding:'utf8'}).trim()"])
        .output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), format!("v{}", selected.version.unwrap()));
}

fn provider(name: &str, value: &str) -> LLMProviderConfig {
    LLMProviderConfig { id: 0, name: name.into(), value: value.into(), llm_provider_id: 1, append_location: String::new(), is_addition: false }
}

#[test]
fn test_native_agent_env_cursor_api_key_field_feeds_environment_and_explicit_variable_wins() {
    let env = agent_environment(&[], &[provider("cursor_api_key", " key-one ")], "node").unwrap();
    assert_eq!(env.get("CURSOR_API_KEY").map(String::as_str), Some("key-one"));
    let env = agent_environment(&[], &[provider("cursor_api_key", "key-one"), provider("acp_env_vars", "CURSOR_API_KEY=key-two")], "node").unwrap();
    assert_eq!(env.get("CURSOR_API_KEY").map(String::as_str), Some("key-two"));
    let env = agent_environment(&[], &[provider("cursor_api_key", "  ")], "node").unwrap();
    assert!(!env.contains_key("CURSOR_API_KEY"));
}

#[test]
fn test_native_agent_env_cursor_sdk_lookup_walks_parent_directories_like_require() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("node_modules").join("@cursor").join("sdk");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("package.json"), r#"{"name":"@cursor/sdk","version":"1.2.0"}"#).unwrap();
    let nested = root.path().join("project").join("src");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(find_cursor_sdk(&nested), Some((root.path().to_path_buf(), Some("1.2.0".into()))));
    let empty = tempfile::tempdir().unwrap();
    assert!(find_cursor_sdk(empty.path()).is_none_or(|(dir, _)| !dir.starts_with(empty.path())), "an empty directory has no SDK of its own");
}

#[tokio::test]
async fn test_native_agent_env_launch_cli_uses_the_installer_candidate_and_preserves_explicit_errors() {
    let root = tempfile::tempdir().unwrap();
    let global = root.path().join("package manager");
    let first = root.path().join("path");
    std::fs::create_dir_all(&global).unwrap();
    std::fs::create_dir_all(&first).unwrap();
    let env = HashMap::from([("PATH".into(), String::new()), ("PNPM_HOME".into(), global.display().to_string())]);
    for driver in ["pi", "opencode", "kiro", "antigravity"] {
        let spec = ToolSpec { command: "aipp-test-cli", ..spec(driver).unwrap() };
        let filename = if cfg!(windows) { format!("{}.exe", spec.command) } else { spec.command.into() };
        let installed = global.join(&filename);
        std::fs::write(&installed, "").unwrap();
        let mut config = NativeConfig {
            node: root.path().join("fixture-node").display().to_string(), cwd: root.path().to_owned(), env: env.clone(), wire: json!({}), review: false,
        };
        let files = cli_candidates(driver, false, spec.command, &env, root.path());
        assert_eq!(files.first().unwrap().0, installed);
        let selected = select_cli(driver, &spec, false, None, &env, root.path(), false).await.unwrap();
        assert_eq!(Path::new(&selected.command), installed);

        let preferred = first.join(&filename);
        std::fs::write(&preferred, "").unwrap();
        config.env.insert("PATH".into(), first.display().to_string());
        let selected = select_cli(driver, &spec, false, Some(spec.command), &config.env, root.path(), false).await.unwrap();
        assert_eq!(Path::new(&selected.command), preferred);
        let missing = root.path().join("missing").join(&filename);
        config.wire["command"] = json!(missing);
        prepare_bridge_launch(driver, &mut config).await.unwrap();
        assert_eq!(config.wire["command"], json!(missing));
    }
}

#[tokio::test]
async fn test_native_agent_env_bad_cli_candidates_before_thirteenth_working_installation() {
    let root = tempfile::tempdir().unwrap();
    let node = which::which("node").expect("Node.js is required for native bridge tests").display().to_string();
    let spec = ToolSpec { command: "aipp-test-cli.mjs", ..spec("antigravity").unwrap() };
    let mut paths = Vec::new();
    for index in 0..13 {
        let dir = root.path().join(index.to_string());
        std::fs::create_dir(&dir).unwrap();
        let script = if index == 12 { "console.log('1.2.3')" } else { "process.exit(2)" };
        std::fs::write(dir.join(spec.command), script).unwrap();
        paths.push(dir);
    }
    let expected = paths[12].join(spec.command).display().to_string();
    let env = HashMap::from([("PATH".into(), std::env::join_paths(&paths).unwrap().to_string_lossy().into_owned())]);
    let lookup = select_cli("antigravity", &spec, false, None, &env, root.path(), true).await.unwrap();
    let (selected, alternatives) = lookup.probe(&node, root.path(), &env, &spec, false).await.unwrap();
    assert_eq!(alternatives.len(), 13);
    assert!(alternatives[..12].iter().all(|entry| !entry.ok));
    assert_eq!(selected.path, expected);
    assert_eq!(lookup.launch_command(&node, root.path(), &env, &spec).await.unwrap(), expected);

    let broken = paths[0].join(spec.command).display().to_string();
    let explicit = select_cli("antigravity", &spec, false, Some(&broken), &env, root.path(), true).await.unwrap();
    let (selected, _) = explicit.probe(&node, root.path(), &env, &spec, false).await.unwrap();
    assert!(!selected.ok);
    assert_eq!(selected.path, broken);
    assert_eq!(explicit.launch_command(&node, root.path(), &env, &spec).await.unwrap(), broken);
}

#[tokio::test]
#[ignore = "requires a supported Node and the official Antigravity CLI installed outside PATH"]
async fn test_native_agent_env_antigravity_detect_and_launch_with_empty_path() {
    let settings = HashMap::from([("acp_env_vars".into(), "PATH=".into())]);
    let mut context = Context::new(&settings).unwrap();
    context.env.insert("PATH".into(), String::new());
    let mut config = configuration(&[], &context.providers, None).unwrap();
    config.env = context.env.clone();
    let node = detect_node("node", &effective_path(&context.env), &context.run_dir).await;
    context.node = node.auto_command(&mut context.env).unwrap();
    let spec = spec("antigravity").unwrap();
    let tool = detect_cli("antigravity", &context, &spec, &node).await.unwrap();
    let selected = tool.selected.unwrap();
    assert!(selected.ok, "{selected:?}");
    assert!(Path::new(&selected.path).is_absolute());
    let launch_node = prepare_bridge_launch("antigravity", &mut config).await.unwrap();
    assert_eq!(launch_node, context.node);
    assert_eq!(config.wire["command"], json!(selected.path));
    let output = bridge_call(&launch_node, &config.cwd, &config.env,
        json!({"type":"exec","id":"version","command":config.wire["command"],"args":["--version"]}),
        Duration::from_secs(30), |_| {}).await.unwrap();
    assert_eq!(output["code"], 0, "{output}");
    assert_eq!(version_token(output["output"].as_str().unwrap()), selected.version);
    eprintln!("Node: {launch_node}, Antigravity: {} ({})", selected.path, output["output"].as_str().unwrap().trim());
}

#[test]
fn test_native_agent_env_cursor_prefers_workspace_then_managed_and_keeps_explicit_directory() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let cwd = workspace.join("project");
    std::fs::create_dir_all(&cwd).unwrap();
    let managed = root.path().join("managed");
    let install = |dir: &Path| {
        let package = dir.join("node_modules/@cursor/sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();
    };
    install(&managed);
    assert_eq!(cursor_sdk_directory(&cwd, None, Some(&managed)), (managed.clone(), "AIPP 管理"));
    assert_eq!(cursor_sdk_directory(&cwd, Some("missing"), Some(&managed)), (cwd.join("missing"), "所选目录"));
    install(&workspace);
    let (start, source) = cursor_sdk_directory(&cwd, None, Some(&managed));
    assert_eq!(source, "工作目录");
    assert_eq!(find_cursor_sdk(&start).unwrap().0, workspace);
    assert!(find_cursor_sdk(&cwd.join("missing")).is_none());
}

#[test]
fn test_native_agent_env_detect_results_keep_missing_programs_and_old_versions_explicit() {
    let old = cli_program(&json!({"command":"agy","resolved":"C:/agy/bin/agy.exe","ok":true,"code":0,"output":"1.2.12"}), "Antigravity 安装器".into(), Some("1.5.1"));
    assert!(old.ok);
    assert_eq!(old.version.as_deref(), Some("1.2.12"));
    assert_eq!(old.warning.as_deref(), Some("建议使用 1.5.1 或更新版本"));
    let missing = cli_program(&json!({"command":"kiro-cli","resolved":null,"ok":false,"error":"spawn kiro-cli ENOENT"}), "PATH".into(), None);
    assert!(!missing.ok);
    assert_eq!(missing.error.as_deref(), Some("未找到 kiro-cli"));
    let shim = cli_program(&json!({"command":"C:/npm/pi.cmd","resolved":"C:/npm/pi.cmd","ok":false,"error":"Use the executable or JavaScript CLI entry point instead of a shell shim: C:/npm/pi.cmd"}), "npm 全局".into(), None);
    assert!(shim.error.unwrap().contains("shell shim"));
    let failed = cli_program(&json!({"command":"pi","resolved":"/bin/pi","ok":false,"code":1,"output":"boom"}), "PATH".into(), None);
    assert_eq!(failed.error.as_deref(), Some("--version 退出码 1：boom"));
}

#[test]
fn test_native_agent_env_source_labels_name_installers_and_package_managers() {
    assert_eq!(source_label(Path::new(r"C:\Users\u\AppData\Local\pnpm\opencode.ps1"), &[]), "pnpm 全局");
    assert_eq!(source_label(Path::new("/home/u/.pi/agent/bin/pi"), &[]), "Pi 安装器");
    assert_eq!(source_label(Path::new(r"C:\Users\u\AppData\Local\agy\bin\agy.exe"), &[]), "Antigravity 安装器");
    assert_eq!(source_label(Path::new("/srv/tools/pi"), &[]), "其他位置");
}

#[test]
fn test_native_agent_env_every_driver_has_update_metadata() {
    for driver in DRIVER_IDS {
        let spec = spec(driver).expect(driver);
        assert!(!spec.display_name.is_empty());
        if driver != "cursor" { assert!(!spec.command.is_empty() && !spec.update_args.is_empty(), "{driver} needs a CLI updater"); }
    }
    assert!(spec("codex").is_none());
}

#[test]
fn test_native_agent_env_cursor_account_reports_sign_in_without_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    assert_eq!(auth::read_cursor_account(&path, 1_000).unwrap(), auth::CursorAccount { signed_in: false, email: None, expires_at_ms: None, expired: false });

    std::fs::write(&path, r#"{"version":1,"backendUrl":"https://api2.cursor.sh","apiKey":"secret-key","apiKeyExpiresAtMs":5000,"email":"u@example.com","createdAtMs":1}"#).unwrap();
    let account = auth::read_cursor_account(&path, 1_000).unwrap();
    assert!(account.signed_in && !account.expired);
    assert_eq!(account.email.as_deref(), Some("u@example.com"));
    assert!(!serde_json::to_string(&account).unwrap().contains("secret-key"), "the minted key never reaches the UI");

    let expired = auth::read_cursor_account(&path, 6_000).unwrap();
    assert!(!expired.signed_in && expired.expired);

    std::fs::write(&path, "{broken").unwrap();
    assert!(auth::read_cursor_account(&path, 1_000).unwrap_err().contains("已损坏"));
    std::fs::write(&path, r#"{"version":1,"apiKey":""}"#).unwrap();
    assert!(auth::read_cursor_account(&path, 1_000).unwrap_err().contains("缺少密钥"));
}

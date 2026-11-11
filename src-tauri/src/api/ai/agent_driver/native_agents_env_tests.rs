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
fn test_native_agent_env_selected_node_directory_is_prepended_once() {
    let (node, node_dir, other) = if cfg!(windows) { (r"C:\nvm\v22.19.0\node.exe", r"C:\nvm\v22.19.0", r"C:\Windows") } else { ("/opt/nvm/v22.19.0/bin/node", "/opt/nvm/v22.19.0/bin", "/usr/bin") };
    let joined = |parts: &[&str]| std::env::join_paths(parts).unwrap().to_string_lossy().into_owned();

    let mut env = HashMap::new();
    with_node_on_path(node, &mut env, Some(other.into()));
    assert_eq!(env.get("PATH"), Some(&joined(&[node_dir, other])));

    let mut already = HashMap::new();
    with_node_on_path(node, &mut already, Some(std::env::join_paths([other, node_dir]).unwrap()));
    assert!(already.is_empty(), "a directory already on PATH keeps its position");

    let mut bare = HashMap::new();
    with_node_on_path("node", &mut bare, Some(other.into()));
    assert!(bare.is_empty(), "PATH lookup keeps the inherited PATH untouched");

    let mut configured = HashMap::from([("PATH".to_string(), "custom".to_string())]);
    with_node_on_path(node, &mut configured, Some(other.into()));
    assert_eq!(configured.get("PATH"), Some(&joined(&[node_dir, "custom"])), "configured PATH is extended, not replaced");
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

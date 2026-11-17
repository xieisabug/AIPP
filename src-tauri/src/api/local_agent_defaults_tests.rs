use super::*;

fn info_of(defaults: &LocalAgentDefaults) -> Vec<(&str, &str)> {
    defaults.info.iter().map(|item| (item.label.as_str(), item.value.as_str())).collect()
}

#[test]
fn test_codex_defaults_profile_overrides_top_level() {
    let content = r#"
model = "gpt-5"
approval_policy = "never"
sandbox_mode = "read-only"
profile = "work"

[profiles.work]
model = "gpt-5-codex"
sandbox_mode = "workspace-write"
"#;
    let defaults = parse_defaults("codex_app_server", "~/.codex/config.toml", Some(content));
    assert!(defaults.found);
    assert_eq!(defaults.settings.get("codex_approval_policy").map(String::as_str), Some("never"));
    assert_eq!(defaults.settings.get("codex_sandbox").map(String::as_str), Some("workspace-write"));
    assert!(info_of(&defaults).contains(&("默认模型", "gpt-5-codex")));
    assert!(info_of(&defaults).contains(&("使用的 profile", "work")));
    assert!(defaults.notes.is_empty());
}

#[test]
fn test_codex_defaults_unsupported_value_becomes_note() {
    let defaults = parse_defaults("codex_app_server", "~/.codex/config.toml", Some("approval_policy = \"on-failure\"\n"));
    assert!(defaults.settings.is_empty());
    assert_eq!(defaults.notes.len(), 1);
    assert!(defaults.notes[0].contains("on-failure"));
    assert!(defaults.notes[0].contains("on-request"));
}

#[test]
fn test_codex_defaults_missing_file_has_no_values() {
    let defaults = parse_defaults("codex_app_server", "~/.codex/config.toml", None);
    assert!(!defaults.found);
    assert_eq!(defaults.source.as_deref(), Some("~/.codex/config.toml"));
    assert!(defaults.settings.is_empty() && defaults.info.is_empty() && defaults.notes.is_empty());
}

#[test]
fn test_codex_defaults_invalid_toml_reports_parse_error() {
    let defaults = parse_defaults("codex_app_server", "~/.codex/config.toml", Some("approval_policy = "));
    assert!(defaults.found);
    assert!(defaults.settings.is_empty());
    assert!(defaults.notes[0].starts_with("解析 ~/.codex/config.toml 失败"));
}

#[test]
fn test_claude_defaults_are_info_only() {
    let content = r#"{"permissions":{"defaultMode":"acceptEdits","allow":["Read"]},"effortLevel":"xhigh","model":"opus"}"#;
    let defaults = parse_defaults("claude_sdk", "~/.claude/settings.json", Some(content));
    // AIPP 不设置时 Claude Code 自己读取这些值，因此不写入提供商
    assert!(defaults.settings.is_empty());
    assert_eq!(info_of(&defaults), vec![("权限模式", "acceptEdits"), ("思考强度", "xhigh"), ("默认模型", "opus")]);
}

#[test]
fn test_kiro_defaults_read_flat_and_nested_keys() {
    let flat = parse_defaults("kiro", "~/.kiro/settings/cli.json", Some(r#"{"chat.defaultAgent":"reviewer"}"#));
    assert_eq!(info_of(&flat), vec![("默认 Agent", "reviewer")]);
    let nested = parse_defaults("kiro", "~/.kiro/settings/cli.json", Some(r#"{"chat":{"defaultModel":"claude-sonnet-4.6"}}"#));
    assert_eq!(info_of(&nested), vec![("默认模型", "claude-sonnet-4.6")]);
}

#[test]
fn test_pi_defaults_read_provider_model_and_thinking() {
    let content = r#"{"defaultProvider":"anthropic","defaultModel":"claude-opus-4-5","defaultThinkingLevel":"high","apiKeys":{"zai":"secret"}}"#;
    let defaults = parse_defaults("pi", "~/.pi/agent/settings.json", Some(content));
    assert_eq!(
        info_of(&defaults),
        vec![("默认 Provider", "anthropic"), ("默认模型", "claude-opus-4-5"), ("思考强度", "high")]
    );
    // 不回显无关字段（例如密钥）
    assert!(!defaults.info.iter().any(|item| item.value.contains("secret")));
}

#[test]
fn test_opencode_defaults_accept_jsonc() {
    let content = r#"{
  // 全局默认模型
  "$schema": "https://opencode.ai/config.json",
  "model": "anthropic/claude-sonnet-4-5", /* trailing */
  "small_model": "a//b",
}"#;
    let defaults = parse_defaults("opencode", "~/.config/opencode/opencode.jsonc", Some(content));
    assert!(defaults.notes.is_empty(), "{:?}", defaults.notes);
    assert_eq!(info_of(&defaults), vec![("默认模型", "anthropic/claude-sonnet-4-5"), ("小模型", "a//b")]);
}

#[test]
fn test_strip_jsonc_keeps_string_content() {
    let stripped = strip_jsonc(r#"{"url":"http://x/*y*/", "list":[1,2,],}"#);
    let value: serde_json::Value = serde_json::from_str(&stripped).unwrap();
    assert_eq!(value["url"], "http://x/*y*/");
    assert_eq!(value["list"], serde_json::json!([1, 2]));
}

#[test]
fn test_read_defaults_from_home_directory() {
    let home = tempfile::tempdir().unwrap();
    let codex = home.path().join(".codex");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(codex.join("config.toml"), "sandbox_mode = \"danger-full-access\"\n").unwrap();
    let locations = Locations { home: home.path().to_path_buf(), claude_config_dir: None };

    let defaults = read_defaults("codex_app_server", &locations);
    assert_eq!(defaults.source.as_deref(), Some("~/.codex/config.toml"));
    assert_eq!(defaults.settings.get("codex_sandbox").map(String::as_str), Some("danger-full-access"));

    let missing = read_defaults("claude_sdk", &locations);
    assert!(!missing.found);
    assert_eq!(missing.source.as_deref(), Some("~/.claude/settings.json"));

    assert_eq!(read_defaults("cursor", &locations), LocalAgentDefaults::default());
}

#[test]
fn test_read_defaults_honours_claude_config_dir() {
    let home = tempfile::tempdir().unwrap();
    let custom = home.path().join("claude-alt");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(custom.join("settings.json"), r#"{"permissions":{"defaultMode":"plan"}}"#).unwrap();
    let locations = Locations { home: home.path().to_path_buf(), claude_config_dir: Some(custom) };

    let defaults = read_defaults("claude_sdk", &locations);
    assert_eq!(defaults.source.as_deref(), Some("~/claude-alt/settings.json"));
    assert_eq!(info_of(&defaults), vec![("权限模式", "plan")]);
}

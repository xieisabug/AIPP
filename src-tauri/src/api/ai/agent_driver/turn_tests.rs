use super::*;
use crate::db::assistant_db::{Assistant, AssistantModel, AssistantModelConfig};
use serde_json::json;
use std::cell::Cell;

// These fixtures call the same selection, native config extraction and MCP
// signature finalization as prepare_config. Only provider reads/tool payload
// inputs are supplied; no Wry runtime, file database or native CLI is started.
struct Fixture {
    assistant: AssistantDetail,
    override_model_id: Option<String>,
    overrides: Option<HashMap<String, serde_json::Value>>,
    features: HashMap<String, HashMap<String, FeatureConfig>>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            assistant: AssistantDetail {
                assistant: Assistant {
                    id: 7, name: "test agent".into(), description: None,
                    assistant_type: Some(4), is_addition: false, created_time: String::new(),
                },
                model: vec![AssistantModel {
                    id: 11, assistant_id: 7, provider_id: 1,
                    model_code: "configured-model".into(), alias: String::new(),
                }],
                model_configs: Vec::new(), prompts: Vec::new(), prompt_params: Vec::new(),
                mcp_configs: Vec::new(), mcp_tool_configs: Vec::new(),
            },
            override_model_id: None, overrides: None, features: HashMap::new(),
        }
    }

    fn request(&self) -> AgentTurnRequest<'_> {
        AgentTurnRequest {
            assistant: &self.assistant, conversation_id: 42,
            prompt: "runtime".into(), display_prompt: "display".into(),
            override_model_id: self.override_model_id.clone(), overrides: &self.overrides,
            feature_configs: &self.features, messages: &[],
        }
    }

    fn overrides(&mut self, values: &[(&str, &str)]) {
        self.overrides = Some(values.iter().map(|(key, value)| ((*key).into(), json!(value))).collect());
    }

    fn prepare(&self, api_type: &str, configs: Vec<LLMProviderConfig>) -> Result<PreparedAgentSession, AppError> {
        let selected = load_agent_provider(&self.assistant, self.override_model_id.as_deref(), |_| {
            Ok((api_type.into(), configs))
        })?;
        prepare_selected_config(&self.request(), &selected, Vec::new())
    }
}

fn provider_config(name: &str, value: &str) -> LLMProviderConfig {
    LLMProviderConfig {
        id: 1, llm_provider_id: 2, name: name.into(), value: value.into(),
        append_location: String::new(), is_addition: false,
    }
}

fn model_config(name: &str, value: &str) -> AssistantModelConfig {
    AssistantModelConfig {
        id: 1, assistant_id: 7, assistant_model_id: 11,
        name: name.into(), value: Some(value.into()), value_type: "string".into(),
    }
}

fn provider_configs() -> Vec<LLMProviderConfig> {
    vec![provider_config("acp_cli_command", "test-acp")]
}

fn signature(config: &PreparedAgentSession) -> &str {
    match config {
        PreparedAgentSession::Acp(config) => &config.session_signature,
        PreparedAgentSession::Codex(config) => &config.session_signature,
        PreparedAgentSession::Claude(config) => &config.session_signature,
    }
}

fn model(config: &PreparedAgentSession) -> Option<&str> {
    match config {
        PreparedAgentSession::Acp(config) => config.model.as_deref(),
        PreparedAgentSession::Codex(config) => config.model.as_deref(),
        PreparedAgentSession::Claude(config) => config.model.as_deref(),
    }
}

#[test]
fn test_prepare_override_selects_provider_and_model_for_each_driver() {
    for (api_type, expected) in [
        ("acp", AgentSessionKind::Acp),
        ("codex_app_server", AgentSessionKind::Codex),
        ("anthropic", AgentSessionKind::Claude),
    ] {
        let mut fixture = Fixture::new();
        fixture.override_model_id = Some(" override-model %%2".into());
        let calls = Cell::new(0);
        let selected = load_agent_provider(&fixture.assistant, fixture.override_model_id.as_deref(), |id| {
            calls.set(calls.get() + 1);
            assert_eq!(id, 2, "must not read the configured default provider 1");
            Ok((api_type.into(), provider_configs()))
        }).unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(selected.provider_id, 2);
        assert_eq!(selected.kind, expected);
        let prepared = prepare_selected_config(&fixture.request(), &selected, Vec::new()).unwrap();
        assert_eq!(prepared.kind(), expected);
        assert_eq!(model(&prepared), Some("override-model"));
    }
}

#[test]
fn test_prepare_invalid_override_fails_before_provider_lookup() {
    let fixture = Fixture::new();
    for invalid in ["bad", "model%%0", "model%%-1", "%%2", "  %%2", "model%%x", "model%%2%%3"] {
        let calls = Cell::new(0);
        let error = load_agent_provider(&fixture.assistant, Some(invalid), |_| {
            calls.set(calls.get() + 1);
            Ok(("codex_app_server".into(), Vec::new()))
        }).err().unwrap().to_string();
        assert!(error.contains(invalid));
        assert!(error.contains("格式无效"));
        assert!(error.contains("未回退"));
        assert_eq!(calls.get(), 0, "invalid selection must fail before database/MCP/actor work");
    }
}

#[test]
fn test_prepare_absent_or_blank_override_preserves_default_selection() {
    let fixture = Fixture::new();
    for override_id in [None, Some(""), Some("  ")] {
        let selected = load_agent_provider(&fixture.assistant, override_id, |id| {
            assert_eq!(id, 1);
            Ok(("codex_app_server".into(), Vec::new()))
        }).unwrap();
        assert_eq!(selected.model_override, None);
        let prepared = prepare_selected_config(&fixture.request(), &selected, Vec::new()).unwrap();
        assert_eq!(model(&prepared), Some("configured-model"));
    }
}

#[test]
fn test_prepare_provider_selection_preserves_legacy_config_precedence() {
    let mut fixture = Fixture::new();
    fixture.assistant.model_configs.push(model_config("acp_provider", "3"));
    for (model_provider_id, expected) in [(1, 1), (0, 3)] {
        fixture.assistant.model[0].provider_id = model_provider_id;
        let selected = load_agent_provider(&fixture.assistant, None, |id| {
            assert_eq!(id, expected);
            Ok(("acp".into(), provider_configs()))
        }).unwrap();
        assert_eq!(selected.provider_id, expected);
    }
}

#[test]
fn test_prepare_missing_provider_fails_without_lookup() {
    let mut fixture = Fixture::new();
    fixture.assistant.model.clear();
    let error = load_agent_provider(&fixture.assistant, None, |_| {
        panic!("an unconfigured assistant must not look up another provider")
    }).err().unwrap().to_string();
    assert!(error.contains("尚未配置模型提供商"));
}

#[test]
fn test_prepare_provider_read_failure_preserves_error_without_default_retry() {
    let fixture = Fixture::new();
    let calls = Cell::new(0);
    let error = load_agent_provider(&fixture.assistant, Some("override-model%%99"), |id| {
        calls.set(calls.get() + 1);
        assert_eq!(id, 99);
        Err(AppError::ProviderError("provider 99 config read failed: SQLite test failure".into()))
    }).err().unwrap();
    assert!(matches!(error, AppError::ProviderError(ref reason) if reason == "provider 99 config read failed: SQLite test failure"));
    assert_eq!(calls.get(), 1);
}

#[test]
fn test_prepare_unsupported_provider_never_uses_default_driver() {
    let fixture = Fixture::new();
    let error = load_agent_provider(&fixture.assistant, Some("override-model%%2"), |id| {
        assert_eq!(id, 2);
        Ok(("openai".into(), provider_configs()))
    }).err().unwrap().to_string();
    assert!(error.contains("unsupported provider API type 'openai'"));
    assert!(error.contains("no fallback"));
}

#[test]
fn test_prepare_codex_applies_supported_turn_overrides() {
    for (key, supported) in [
        ("approval_policy", CODEX_APPROVAL_POLICIES),
        ("sandbox", CODEX_SANDBOX_MODES),
        ("collaboration_mode", &["default", "plan"][..]),
    ] {
        for value in supported {
            let mut fixture = Fixture::new();
            fixture.overrides(&[(key, value), ("reasoning_effort", "high")]);
            let PreparedAgentSession::Codex(config) = fixture.prepare("codex_app_server", Vec::new()).unwrap() else {
                panic!("expected Codex");
            };
            let actual = match key {
                "approval_policy" => config.approval_policy.as_deref(),
                "sandbox" => config.sandbox.as_deref(),
                _ => config.collaboration_mode.as_deref(),
            };
            assert_eq!(actual, Some(*value));
            assert_eq!(config.reasoning_effort.as_deref(), Some("high"));
        }
    }
}

#[test]
fn test_prepare_codex_rejects_invalid_turn_modes_and_permissions() {
    for (key, reason) in [
        ("approval_policy", "审批策略"), ("sandbox", "沙箱模式"), ("collaboration_mode", "协作模式"),
    ] {
        let mut fixture = Fixture::new();
        fixture.overrides(&[(key, "unsupported")]);
        let error = fixture.prepare("codex_app_server", Vec::new()).err().unwrap().to_string();
        assert!(error.contains(reason));
        assert!(error.contains("unsupported"));
    }
}

#[test]
fn test_prepare_claude_validates_turn_permission_modes() {
    let mut fixture = Fixture::new();
    for mode in ["default", "plan"] {
        fixture.overrides(&[("permission_mode", mode), ("claude_effort", "high")]);
        let PreparedAgentSession::Claude(config) = fixture.prepare("claude_sdk", Vec::new()).unwrap() else {
            panic!("expected Claude");
        };
        assert_eq!(config.permission_mode.as_deref(), Some(mode));
        assert_eq!(config.effort.as_deref(), Some("high"));
    }
    fixture.overrides(&[("permission_mode", "bypassPermissions")]);
    let error = fixture.prepare("claude_sdk", Vec::new()).err().unwrap().to_string();
    assert!(error.contains("Claude Code 不支持工作模式：bypassPermissions"));
}

#[test]
fn test_prepare_claude_default_sentinels_clear_model_and_effort() {
    for from_override in [false, true] {
        let mut fixture = Fixture::new();
        fixture.assistant.model_configs.push(model_config("claude_effort", "high"));
        if from_override {
            fixture.override_model_id = Some(format!("{CLAUDE_CODE_DEFAULT_MODEL}%%2"));
        } else {
            fixture.assistant.model[0].model_code = CLAUDE_CODE_DEFAULT_MODEL.into();
        }
        fixture.overrides(&[("claude_effort", "default")]);
        let selected = load_agent_provider(&fixture.assistant, fixture.override_model_id.as_deref(), |_| {
            Ok(("claude_sdk".into(), Vec::new()))
        }).unwrap();
        let choices = vec![AcpSessionConfigChoicePayload {
            value: "listed-model".into(), name: "Listed model".into(),
            description: Some("provider choice".into()), group_name: None,
        }];
        let PreparedAgentSession::Claude(config) = prepare_selected_config(&fixture.request(), &selected, choices).unwrap() else {
            panic!("expected Claude");
        };
        assert_eq!(config.model, None);
        assert_eq!(config.effort, None);
        assert_eq!(config.model_choices.len(), 1);
        assert_eq!(config.model_choices[0].value, "listed-model");
        assert_eq!(config.model_choices[0].description.as_deref(), Some("provider choice"));
    }
}

#[test]
fn test_prepare_provider_config_and_assistant_environment_are_preserved() {
    for api_type in ["acp", "codex_app_server", "claude_sdk"] {
        let mut fixture = Fixture::new();
        fixture.assistant.model_configs = vec![
            model_config("acp_env_vars", "SHARED=assistant\nASSISTANT_ONLY=kept"),
            model_config("acp_working_directory", "/test/assistant"),
        ];
        let mut configs = provider_configs();
        configs.extend([
            provider_config("acp_env_vars", "SHARED=provider\nPROVIDER_ONLY=kept"),
            provider_config("acp_working_directory", "/test/provider"),
        ]);
        let prepared = fixture.prepare(api_type, configs).unwrap();
        let (env, cwd) = match &prepared {
            PreparedAgentSession::Acp(config) => (&config.env_vars, &config.working_directory),
            PreparedAgentSession::Codex(config) => (&config.env_vars, &config.working_directory),
            PreparedAgentSession::Claude(config) => (&config.env_vars, &config.working_directory),
        };
        assert_eq!(env.get("SHARED").map(String::as_str), Some("assistant"));
        assert_eq!(env.get("PROVIDER_ONLY").map(String::as_str), Some("kept"));
        assert_eq!(env.get("ASSISTANT_ONLY").map(String::as_str), Some("kept"));
        assert_eq!(cwd, &std::path::PathBuf::from("/test/assistant"));
    }
}

#[test]
fn test_prepare_acp_applies_effort_and_only_enabled_network_proxy() {
    let mut fixture = Fixture::new();
    fixture.overrides(&[("reasoning_effort", " high ")]);
    fixture.features.insert("network_config".into(), HashMap::from([("network_proxy".into(), FeatureConfig {
        id: None, feature_code: "network_config".into(), key: "network_proxy".into(),
        value: "http://proxy.test:8080".into(), data_type: "string".into(), description: None,
    })]));
    for enabled in [false, true] {
        let mut configs = provider_configs();
        configs.push(provider_config("proxy_enabled", if enabled { "true" } else { "false" }));
        let PreparedAgentSession::Acp(config) = fixture.prepare("acp", configs).unwrap() else {
            panic!("expected ACP");
        };
        assert_eq!(config.thought_level.as_deref(), Some("high"));
        assert_eq!(config.env_vars.get("HTTP_PROXY").map(String::as_str), enabled.then_some("http://proxy.test:8080"));
    }
}

#[test]
fn test_prepare_missing_acp_cli_is_an_error_without_driver_fallback() {
    let error = Fixture::new().prepare("acp", Vec::new()).err().unwrap().to_string();
    assert!(error.contains("没有拿到有效的 provider CLI 配置"));
}

#[test]
fn test_prepare_mcp_finalization_refreshes_each_native_signature_after_overrides() {
    let payload = r#"[{"server_id":7,"tools":[{"tool_name":"read"}]}]"#;
    for api_type in ["acp", "codex_app_server", "claude_sdk"] {
        let mut fixture = Fixture::new();
        fixture.override_model_id = Some("override-model%%2".into());
        fixture.overrides(&[
            ("reasoning_effort", "high"), ("claude_effort", "high"),
            ("approval_policy", "never"), ("sandbox", "read-only"),
            ("collaboration_mode", "plan"), ("permission_mode", "plan"),
        ]);
        let mut prepared = fixture.prepare(api_type, provider_configs()).unwrap();
        let initial = signature(&prepared).to_string();
        prepared.finalize_mcp_tools(payload.into());
        let finalized = signature(&prepared).to_string();
        assert_ne!(finalized, initial);
        assert!(finalized.contains("override-model"));
        assert!(finalized.contains("high"));
        match &prepared {
            PreparedAgentSession::Codex(config) => {
                assert_eq!(config.selected_mcp_tools_payload, payload);
                let signed: serde_json::Value = serde_json::from_str(&finalized).unwrap();
                assert_eq!(signed["selected_mcp"], payload);
                assert_eq!(signed["approval"], "never");
                assert_eq!(signed["sandbox"], "read-only");
                assert_eq!(signed["collaboration_mode"], "plan");
            }
            PreparedAgentSession::Claude(config) => {
                assert_eq!(config.selected_mcp_tools_payload, payload);
                let signed: serde_json::Value = serde_json::from_str(&finalized).unwrap();
                assert_eq!(signed["selected_mcp"], payload);
                assert_eq!(signed["permission_mode"], "plan");
            }
            PreparedAgentSession::Acp(config) => {
                assert_eq!(config.selected_mcp_tools_payload, payload);
                assert!(finalized.contains(payload));
            }
        }
        prepared.finalize_mcp_tools(payload.into());
        assert_eq!(signature(&prepared), finalized);
        prepared.finalize_mcp_tools("[]".into());
        assert_ne!(signature(&prepared), finalized);
    }
}

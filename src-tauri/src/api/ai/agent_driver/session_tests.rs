use super::*;

fn cli_config(name: &str, value: &str) -> LLMProviderConfig {
    LLMProviderConfig {
        id: 1,
        name: name.into(),
        llm_provider_id: 1,
        value: value.into(),
        append_location: String::new(),
        is_addition: false,
    }
}

#[test]
fn test_driver_routing_known_native_providers() {
    for (api_type, expected) in [
        ("acp", AgentSessionKind::Acp),
        ("codex_app_server", AgentSessionKind::Codex),
        ("claude_sdk", AgentSessionKind::Claude),
        ("anthropic", AgentSessionKind::Claude),
        ("CLAUDE_SDK", AgentSessionKind::Claude),
    ] {
        assert_eq!(AgentSessionKind::from_provider(api_type, &[]).unwrap(), expected);
    }
}

#[test]
fn test_driver_routing_legacy_claude_uses_native_transport() {
    for name in ["acp_cli_command", "claude_cli_command"] {
        let config = cli_config(name, r"C:\Users\user\.local\bin\CLAUDE.exe");
        assert_eq!(
            AgentSessionKind::from_provider("acp", &[config]).unwrap(),
            AgentSessionKind::Claude
        );
    }
    let config = cli_config("acp_cli_command", "my-acp-agent");
    assert_eq!(
        AgentSessionKind::from_provider("acp", &[config]).unwrap(),
        AgentSessionKind::Acp
    );
}

#[test]
fn test_driver_routing_unknown_provider_never_falls_back() {
    for api_type in ["", "openai", "codex", "unknown", "CODEX_APP_SERVER", "ACP"] {
        let error = AgentSessionKind::from_provider(api_type, &[]).unwrap_err().to_string();
        assert!(error.contains("routing failed"));
        assert!(error.contains(&format!("'{api_type}'")));
        assert!(error.contains("no fallback"));
    }
}

#[test]
fn test_driver_capabilities_match_native_handle_contracts() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let capabilities = kind.capabilities();
        assert!(capabilities.cancel_prompt);
        assert!(capabilities.set_config_option);
        assert_eq!(capabilities.structured_attachments, kind == AgentSessionKind::Acp);
        assert_eq!(AgentSessionKind::from_provider(kind.api_type(), &[]).unwrap(), kind);
    }
}

#[test]
fn test_driver_attachments_reject_unsupported_input_without_silent_loss() {
    assert!(AgentSessionKind::Acp.validate_attachments(2, 42).is_ok());
    for kind in [AgentSessionKind::Codex, AgentSessionKind::Claude] {
        assert!(kind.validate_attachments(0, 42).is_ok());
        let error = kind.validate_attachments(2, 42).unwrap_err().to_string();
        assert!(error.contains(kind.api_type()));
        assert!(error.contains("message_id=42"));
        assert!(error.contains("received 2"));
        assert!(error.contains("does not accept structured attachments"));
    }
}

fn control_fixture(
    kind: AgentSessionKind,
) -> (AgentSessionHandle, tokio::sync::mpsc::UnboundedReceiver<TestSessionCommand>) {
    match kind {
        AgentSessionKind::Acp => {
            let (handle, receiver) = AcpSessionHandle::driver_test_channel();
            (AgentSessionHandle::new(handle, "driver-test-run".into()), receiver)
        }
        AgentSessionKind::Codex => {
            let (handle, receiver) = CodexSessionHandle::driver_test_channel();
            (AgentSessionHandle::new(handle, "driver-test-run".into()), receiver)
        }
        AgentSessionKind::Claude => {
            let (handle, receiver) = ClaudeSessionHandle::driver_test_channel();
            (AgentSessionHandle::new(handle, "driver-test-run".into()), receiver)
        }
    }
}

#[tokio::test]
async fn test_driver_cancel_delegates_and_preserves_native_error() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let (handle, mut receiver) = control_fixture(kind);
        assert_eq!(handle.metadata().id, kind.api_type());
        assert_eq!(handle.capabilities(), kind.capabilities());
        let native_error = format!("{} interrupt rejected [session=test-session]", kind.api_type());
        let expected_error = native_error.clone();
        let operation = async {
            let (result, ()) = tokio::join!(handle.cancel_current_prompt(), async {
                let Some(TestSessionCommand::Cancel { response }) = receiver.recv().await else {
                    panic!("expected cancel command");
                };
                response.send(Err(native_error)).unwrap();
            });
            match result {
                Err(AppError::UnknownError(reason)) => assert_eq!(reason, expected_error),
                other => panic!("expected original native error, received {other:?}"),
            }
            assert!(receiver.try_recv().is_err());
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), operation).await.unwrap();
    }
}

#[tokio::test]
async fn test_driver_config_forwards_exact_values_and_preserves_result() {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        let (handle, mut receiver) = control_fixture(kind);
        for (config_id, value) in [("model", "configured-model"), ("vendor/opaque-option", ""), ("future-config", " exact value ")] {
        for native_result in [Ok(()), Err("unsupported native model selection".to_string())] {
            let expected_result = native_result.clone();
            let operation = async {
                let (result, ()) = tokio::join!(
                    handle.set_config_option(config_id.into(), value.into()),
                    async {
                        let Some(TestSessionCommand::SetConfig { config_id: actual_id, value: actual_value, response }) =
                            receiver.recv().await
                        else {
                            panic!("expected config command");
                        };
                        assert_eq!(actual_id, config_id);
                        assert_eq!(actual_value, value);
                        response.send(native_result).unwrap();
                    }
                );
                match (result, expected_result) {
                    (Ok(()), Ok(())) => {}
                    (Err(AppError::UnknownError(actual)), Err(expected)) => assert_eq!(actual, expected),
                    other => panic!("unexpected config result: {other:?}"),
                }
                assert!(receiver.try_recv().is_err());
            };
            tokio::time::timeout(std::time::Duration::from_secs(2), operation).await.unwrap();
        }
        }
    }
}

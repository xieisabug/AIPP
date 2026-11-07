// Native transport control tests: real production handles, mpsc and oneshot channels.
// No installed/authenticated agent CLI, Wry window, sleeps or database is required.
// Unix process tests invoke the system false executable directly, without a shell.
use super::*;

fn handle_channel(run_id: &str) -> (CodexSessionHandle, mpsc::UnboundedReceiver<CodexSessionCommand>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (CodexSessionHandle { sender, run_id: run_id.into(), failure_context: std::sync::Arc::new(std::sync::Mutex::new(None)) }, receiver)
}

fn entry(id: i64, run_id: &str) -> CodexSessionEntry {
    let (handle, _receiver) = handle_channel(run_id);
    CodexSessionEntry::new(handle, id, "signature".into())
}

#[tokio::test]
async fn test_codex_closed_control_channel_rejects_cancel_and_config() {
    let (handle, receiver) = handle_channel("closed-run");
    drop(receiver);
    let cancel = handle.cancel_current_prompt().await.unwrap_err().to_string();
    let config = handle.set_config_option("model".into(), "chosen".into()).await.unwrap_err().to_string();
    assert!(cancel.contains("控制通道已关闭") && cancel.contains("closed-run"));
    assert!(config.contains("控制通道已关闭") && config.contains("closed-run"));
}

#[tokio::test]
async fn test_codex_cancel_dropped_reply() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let CodexSessionCommand::CancelCurrentPrompt { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        drop(response);
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.cancel_current_prompt(), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("未返回结果"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_codex_cancel_native_failure() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let CodexSessionCommand::CancelCurrentPrompt { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        response.send(Err("native protocol refused control: request 17".into())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.cancel_current_prompt(), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("native protocol refused control: request 17"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_codex_cancel_success() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let CodexSessionCommand::CancelCurrentPrompt { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        response.send(Ok(())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.cancel_current_prompt(), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.is_ok());
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_codex_config_dropped_reply() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let CodexSessionCommand::SetConfigOption { response, config_id, value } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        assert_eq!(config_id, "model");
        assert_eq!(value, "chosen");
        drop(response);
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.set_config_option("model".into(), "chosen".into()), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("未返回结果"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_codex_config_native_failure() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let CodexSessionCommand::SetConfigOption { response, config_id, value } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        assert_eq!(config_id, "model");
        assert_eq!(value, "chosen");
        response.send(Err("native protocol refused control: request 17".into())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.set_config_option("model".into(), "chosen".into()), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("native protocol refused control: request 17"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_codex_config_success() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let CodexSessionCommand::SetConfigOption { response, config_id, value } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        assert_eq!(config_id, "model");
        assert_eq!(value, "chosen");
        response.send(Ok(())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.set_config_option("model".into(), "chosen".into()), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.is_ok());
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[test]
fn test_codex_stale_cleanup_preserves_replacement_and_other_conversation() {
    let mut sessions = HashMap::from([(1, entry(1, "replacement")), (2, entry(2, "other"))]);
    assert!(!remove_codex_session_for_run(&mut sessions, 1, "stale-run"));
    assert_eq!(sessions[&1].run_id, "replacement");
    assert_eq!(sessions[&1].handle.run_id, "replacement");
    assert_eq!(sessions[&1].snapshot.conversation_id, 1);
    assert!(remove_codex_session_for_run(&mut sessions, 1, "replacement"));
    assert!(!sessions.contains_key(&1));
    assert_eq!(sessions[&2].run_id, "other");
    assert!(!remove_codex_session_for_run(&mut sessions, 1, "replacement"));
}

#[test]
fn test_codex_active_prompt_prevents_idle_release_and_touch_refreshes_entry() {
    let mut session = entry(1, "run");
    session.last_activity = Instant::now() - Duration::from_secs(120);
    session.snapshot.has_active_prompt = true;
    assert!(!session.is_idle_for(Duration::from_secs(60)));
    session.snapshot.has_active_prompt = false;
    assert!(session.is_idle_for(Duration::from_secs(60)));
    let previous = session.last_activity;
    session.touch();
    assert!(session.last_activity > previous);
    assert!(!session.is_idle_for(Duration::from_secs(60)));
}

#[test]
fn test_codex_protocol_failure_retains_reason_runtime_and_bounded_stderr() {
    let buffer: CodexStderrBuffer = Default::default();
    for index in 0..(CODEX_STDERR_MAX_LINES + 3) {
        assert!(record_codex_stderr(&buffer, format!("native diagnostic {index}")));
    }
    assert!(!record_codex_stderr(&buffer, "2026-01-01T00:00:00Z INFO tracing noise".into()));
    let error = codex_error_with_diagnostics("invalid JSON from app-server; exit status: 17", 42, "run-failed", &buffer);
    assert!(error.contains("invalid JSON from app-server; exit status: 17"));
    assert!(error.contains("conversation_id=42"));
    assert!(error.contains("run_id=run-failed"));
    assert!(!error.contains("native diagnostic 0\n"));
    assert!(error.contains("native diagnostic 3\n"));
    assert!(error.contains(&format!("native diagnostic {}", CODEX_STDERR_MAX_LINES + 2)));
    assert!(!error.contains("tracing noise"));
    assert_eq!(buffer.lock().unwrap().len(), CODEX_STDERR_MAX_LINES);
}

#[test]
fn test_codex_protocol_failure_without_stderr_keeps_original_cause() {
    let buffer: CodexStderrBuffer = Default::default();
    let error = codex_error_with_diagnostics("stdout closed before response id=19", 42, "run-empty", &buffer);
    assert_eq!(error, "stdout closed before response id=19 [conversation_id=42, run_id=run-empty]");
}

#[test]
fn test_codex_native_thread_identity_requires_a_string_id() {
    assert_eq!(thread_id_from_result(&json!({"thread": {"id": "native-thread"}})).as_deref(), Some("native-thread"));
    for invalid in [json!({}), json!({"thread": {}}), json!({"thread": {"id": 42}})] {
        assert_eq!(thread_id_from_result(&invalid), None);
    }
}

// The fake executable ignores protocol arguments and exits nonzero immediately.
// This tests the production pipe/child setup, not the Wry-bound session loop.
#[cfg(unix)]
#[tokio::test]
async fn test_codex_native_process_nonzero_exit_closes_stdout() {
    let mut config = extract_codex_app_server_config(&[], &[], None).unwrap();
    config.cli_command = which::which("false")
        .expect("Unix process fixture requires the system false executable on PATH")
        .to_string_lossy().into_owned();
    config.working_directory = std::env::current_dir().unwrap();
    let buffer: CodexStderrBuffer = Default::default();
    let (mut child, _stdin, mut lines) = spawn_process(&config, buffer).await.unwrap();
    let (status, output) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(child.wait(), lines.next_line())
    }).await.expect("fake process must terminate");
    assert_eq!(status.unwrap().code(), Some(1));
    assert_eq!(output.unwrap(), None);
}

#[tokio::test]
async fn test_codex_reuse_matching_signature_preserves_native_handle_and_refreshes_idle() {
    let (handle, mut receiver) = handle_channel("existing-run");
    let mut session = entry(1, "existing-run");
    session.handle = handle;
    session.last_activity = Instant::now() - Duration::from_secs(120);
    let previous = session.last_activity;
    let snapshot = serde_json::to_value(&session.snapshot).unwrap();
    let reused = session.reuse_handle("signature").expect("matching configuration must reuse");
    assert_eq!(reused.run_id, "existing-run");
    assert_eq!(session.run_id, "existing-run");
    assert!(session.last_activity > previous);
    assert!(!session.is_idle_for(Duration::from_secs(60)));
    assert_eq!(serde_json::to_value(&session.snapshot).unwrap(), snapshot);
    // The returned handle must still address the existing native receiver.
    let respond = async {
        let CodexSessionCommand::CancelCurrentPrompt { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        response.send(Ok(())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(reused.cancel_current_prompt(), respond)
    }).await.expect("reused handle must address the existing receiver");
    assert!(result.is_ok());
}

#[test]
fn test_codex_reuse_mismatch_leaves_entry_unchanged_and_entries_are_isolated() {
    let mut first = entry(1, "first-run");
    let mut second = entry(2, "second-run");
    first.last_activity = Instant::now() - Duration::from_secs(120);
    second.last_activity = first.last_activity;
    let first_activity = first.last_activity;
    let second_activity = second.last_activity;
    let first_snapshot = serde_json::to_value(&first.snapshot).unwrap();
    let second_snapshot = serde_json::to_value(&second.snapshot).unwrap();

    assert!(first.reuse_handle("different-signature").is_none());
    assert_eq!(first.last_activity, first_activity);
    assert_eq!(first.run_id, "first-run");
    assert_eq!(first.handle.run_id, "first-run");
    assert_eq!(first.config_signature, "signature");
    assert_eq!(serde_json::to_value(&first.snapshot).unwrap(), first_snapshot);

    assert_eq!(second.reuse_handle("signature").unwrap().run_id, "second-run");
    assert!(second.last_activity > second_activity);
    assert_eq!(second.run_id, "second-run");
    assert_eq!(serde_json::to_value(&second.snapshot).unwrap(), second_snapshot);
    assert_eq!(first.last_activity, first_activity);
    assert_eq!(first.handle.run_id, "first-run");
    assert_eq!(serde_json::to_value(&first.snapshot).unwrap(), first_snapshot);
}

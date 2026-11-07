//! Unix subprocess tests of the production active-turn transport boundary.
//! The fake CLI is only a shell peer: Rust production code reads its stdout and
//! writes its stdin. No authentication, desktop runtime, or database is used.
//! These do not claim coverage of the Tauri outer actor or result persistence.
use super::*;
use crate::api::ai::agent_driver::session::AgentSessionHandle;
use std::os::unix::fs::PermissionsExt;

struct FakeClaude {
    _directory: tempfile::TempDir,
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

impl FakeClaude {
    async fn start(body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("claude-fixture");
        std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap();
        config.cli_command = path.to_string_lossy().into_owned();
        config.working_directory = directory.path().to_owned();
        let (child, stdin, lines) = spawn_claude_process(&config, Some("native-fixture")).await.unwrap();
        Self { _directory: directory, child, stdin, lines }
    }

    async fn echo() -> Self {
        Self::start(r#"
printf '%s\n' '{"type":"system","session_id":"native-fixture"}'
while IFS= read -r input; do
    printf '{"type":"fixture_input","input":%s}\n' "$input"
done
"#).await
    }

    async fn read(&mut self, commands: &mut mpsc::UnboundedReceiver<SessionCommand>) -> Result<ClaudePromptInput, String> {
        read_claude_prompt_input(&mut self.stdin, &mut self.lines, commands).await
    }

    async fn ready(&mut self, commands: &mut mpsc::UnboundedReceiver<SessionCommand>) {
        let ClaudePromptInput::Frame(frame) = self.read(commands).await.unwrap() else {
            panic!("expected startup frame")
        };
        assert_eq!(frame["type"], "system");
        assert_eq!(frame["session_id"], "native-fixture");
    }

    async fn stop(mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }
}

fn session() -> (AgentSessionHandle, mpsc::UnboundedReceiver<SessionCommand>, watch::Receiver<u64>) {
    let (sender, commands) = mpsc::unbounded_channel();
    let (cancel_signal, cancellation) = watch::channel(0);
    (AgentSessionHandle::Claude(ClaudeSessionHandle {
        sender, cancel_signal, run_id: "fixture-run".into(),
    }), commands, cancellation)
}

async fn bounded(future: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(5), future).await.expect("Claude protocol fixture timed out");
}

#[tokio::test]
async fn test_claude_unified_cancel_writes_native_interrupt_before_success() {
    bounded(async {
        let (handle, mut commands, _cancellation) = session();
        let mut peer = FakeClaude::echo().await;
        peer.ready(&mut commands).await;
        let (result, observed) = tokio::join!(handle.cancel_current_prompt(), peer.read(&mut commands));
        result.unwrap();
        assert!(matches!(observed, Ok(ClaudePromptInput::Interrupted)));
        let ClaudePromptInput::Frame(frame) = peer.read(&mut commands).await.unwrap() else { panic!("missing wire frame") };
        assert_eq!(frame["input"]["type"], "control_request");
        assert_eq!(frame["input"]["request"]["subtype"], "interrupt");
        uuid::Uuid::parse_str(frame["input"]["request_id"].as_str().unwrap()).unwrap();
        assert!(commands.try_recv().is_err(), "one cancel must not enqueue a retry");
        peer.stop().await;
    }).await;
}

#[tokio::test]
async fn test_claude_unified_active_config_rejection_preserves_cancel_and_native_pipe() {
    bounded(async {
        let (handle, mut commands, _cancellation) = session();
        let mut peer = FakeClaude::echo().await;
        peer.ready(&mut commands).await;
        let control = async {
            let error = handle.set_config_option("model".into(), "next-model".into()).await.unwrap_err();
            assert!(error.to_string().contains("配置将在下一次会话启动时生效"));
            handle.cancel_current_prompt().await.unwrap();
        };
        let ((), observed) = tokio::join!(control, peer.read(&mut commands));
        assert!(matches!(observed, Ok(ClaudePromptInput::Interrupted)));
        let ClaudePromptInput::Frame(frame) = peer.read(&mut commands).await.unwrap() else { panic!("missing wire frame") };
        assert_eq!(frame["input"]["request"]["subtype"], "interrupt", "rejected config must not be sent to CLI");
        peer.stop().await;
    }).await;
}

#[tokio::test]
async fn test_claude_native_reader_preserves_frames_and_ignores_non_json_diagnostics() {
    bounded(async {
        let (_handle, mut commands, _cancellation) = session();
        let mut peer = FakeClaude::start(r#"
printf '%s\n' 'not-json diagnostic' '' '{"type":"system","session_id":"native-fixture"}'
printf '%s\n' '{"type":"stream_event","event":{"delta":{"text":"first"}}}' '{"type":"result","subtype":"success","result":"done"}'
"#).await;
        peer.ready(&mut commands).await;
        let ClaudePromptInput::Frame(delta) = peer.read(&mut commands).await.unwrap() else { panic!("missing delta") };
        let ClaudePromptInput::Frame(result) = peer.read(&mut commands).await.unwrap() else { panic!("missing result") };
        assert_eq!(delta.pointer("/event/delta/text").and_then(Value::as_str), Some("first"));
        assert_eq!(result["result"], "done");
        assert!(peer.child.wait().await.unwrap().success());
    }).await;
}

#[tokio::test]
async fn test_claude_native_reader_eof_and_crash_return_without_retry() {
    bounded(async {
        for exit in [0, 17] {
            let (_handle, mut commands, _cancellation) = session();
            let mut peer = FakeClaude::start(&format!("printf '%s\\n' 'truncated non-json'\nexit {exit}")).await;
            let error = match peer.read(&mut commands).await {
                Err(error) => error,
                _ => panic!("EOF must fail the active stream"),
            };
            assert_eq!(error, "Claude Code 进程提前退出");
            assert_eq!(peer.child.wait().await.unwrap().code(), Some(exit));
            assert!(commands.try_recv().is_err());
        }
    }).await;
}

#[tokio::test]
async fn test_claude_unified_closed_owner_terminates_active_reader() {
    bounded(async {
        let (handle, mut commands, cancellation) = session();
        let mut peer = FakeClaude::echo().await;
        peer.ready(&mut commands).await;
        drop(handle);
        drop(cancellation);
        let error = match peer.read(&mut commands).await {
            Err(error) => error,
            _ => panic!("closed owner must stop active reader"),
        };
        assert!(error.contains("会话在完成当前请求前已关闭"));
        peer.stop().await;
    }).await;
}

#[tokio::test]
async fn test_claude_unified_concurrent_sessions_keep_controls_and_signals_isolated() {
    bounded(async {
        let (first, mut first_commands, first_cancellation) = session();
        let (second, mut second_commands, second_cancellation) = session();
        let mut first_peer = FakeClaude::echo().await;
        let mut second_peer = FakeClaude::echo().await;
        first_peer.ready(&mut first_commands).await;
        second_peer.ready(&mut second_commands).await;
        let (result, observed) = tokio::join!(first.cancel_current_prompt(), first_peer.read(&mut first_commands));
        result.unwrap();
        assert!(matches!(observed, Ok(ClaudePromptInput::Interrupted)));
        assert_eq!(*first_cancellation.borrow(), 1);
        assert_eq!(*second_cancellation.borrow(), 0);
        assert!(second_commands.try_recv().is_err());
        let (result, observed) = tokio::join!(second.cancel_current_prompt(), second_peer.read(&mut second_commands));
        result.unwrap();
        assert!(matches!(observed, Ok(ClaudePromptInput::Interrupted)));
        assert_eq!(*second_cancellation.borrow(), 1);
        first_peer.stop().await;
        second_peer.stop().await;
    }).await;
}

fn approval(request_id: &str, conversation_id: i64) -> AcpPermissionRequestEvent {
    AcpPermissionRequestEvent {
        request_id: request_id.into(), conversation_id: Some(conversation_id),
        agent_kind: Some(CLAUDE_SDK_API_TYPE.into()), tool_call_id: "tool-1".into(),
        title: None, kind: None, parameters: None,
        options: vec![AcpPermissionOptionPayload {
            option_id: "allow".into(), name: "Allow".into(), kind: "allow_once".into(),
        }],
    }
}

#[tokio::test]
async fn test_claude_unified_cancel_unblocks_real_approval_wait_and_rejects_late_reply() {
    bounded(async {
        let (handle, mut commands, mut cancellation) = session();
        let mut peer = FakeClaude::echo().await;
        peer.ready(&mut commands).await;
        let state = AcpPermissionState::new();
        let (pending_tx, pending_rx) = oneshot::channel();
        let (other_tx, mut other_rx) = oneshot::channel();
        let (replacement_tx, mut replacement_rx) = oneshot::channel();
        let request_id = "claude:1:old-run:request-1";
        state.store_request(approval(request_id, 1), pending_tx).await;
        state.store_request(approval("claude:2:peer:request-1", 2), other_tx).await;
        state.store_request(approval("claude:1:replacement-run:request-1", 1), replacement_tx).await;
        let actor = async {
            let (decision, resolution) = wait_claude_permission_decision(
                &state, request_id, pending_rx, &mut cancellation, 0,
            ).await;
            assert!(matches!(decision, AcpPermissionDecision::Cancelled));
            assert_eq!(resolution.unwrap().conversation_id, Some(1));
            assert!(matches!(peer.read(&mut commands).await, Ok(ClaudePromptInput::Interrupted)));
        };
        let ((), result) = tokio::join!(actor, handle.cancel_current_prompt());
        result.unwrap();
        assert!(state.resolve_request(request_id, AcpPermissionDecision::Selected("allow".into())).await.unwrap().is_none());
        assert_eq!(state.list_requests_for_conversation(1).await.len(), 1);
        assert_eq!(state.list_requests_for_conversation(2).await.len(), 1);
        assert!(matches!(other_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
        assert!(matches!(replacement_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
        peer.stop().await;
    }).await;
}

#[tokio::test]
async fn test_claude_decision_generation_handles_pre_cancel_late_answer_and_next_prompt() {
    bounded(async {
        let (signal, mut cancellation) = watch::channel(4_u64);
        // A queued old prompt must not invoke its question UI after cancellation.
        let polled = std::sync::atomic::AtomicBool::new(false);
        let result = wait_claude_decision(async {
            polled.store(true, std::sync::atomic::Ordering::SeqCst);
            "allow"
        }, &mut cancellation, 3).await;
        assert_eq!(result, None);
        assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
        // The next prompt captures the new generation rather than inheriting it.
        assert_eq!(wait_claude_decision(async { "next" }, &mut cancellation, 4).await, Some("next"));
        let (tx, rx) = oneshot::channel::<String>();
        let pending = wait_claude_decision(rx, &mut cancellation, 4);
        let cancel = async {
            signal.send_modify(|generation| *generation += 1);
        };
        let (result, ()) = tokio::join!(pending, cancel);
        assert!(result.is_none());
        assert!(tx.send("late allow".into()).is_err());
        // Both ready at entry: cancellation wins over a stale successful answer.
        let (tx, rx) = oneshot::channel::<String>();
        tx.send("already queued allow".into()).unwrap();
        signal.send_modify(|generation| *generation += 1);
        assert!(wait_claude_decision(rx, &mut cancellation, 5).await.is_none());
    }).await;
}

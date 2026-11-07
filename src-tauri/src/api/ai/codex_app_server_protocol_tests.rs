//! Scripted peer on real pipes, using production spawn/write/read/initialize/
//! thread-open functions. The fake is a wire transcript, not a second actor.
//! No Codex installation, login, desktop window, network or database is used.
//! Unix only: the executable fixture uses a Python shebang and chmod. Every
//! interaction has a timeout; kill_on_drop handles panic/timeout and finish reaps.
use super::*;
use std::os::unix::fs::PermissionsExt;

struct NoDesktopRequests;

#[async_trait::async_trait]
impl CodexServerRequests for NoDesktopRequests {
    async fn handle(&self, _stdin: &mut tokio::process::ChildStdin, _frame: &Value) -> Result<bool, String> {
        Ok(false)
    }
}

struct WirePeer {
    child: Child,
    stdin: tokio::process::ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    stderr: CodexStderrBuffer,
    _directory: tempfile::TempDir,
}

impl WirePeer {
    async fn start(steps: Value) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("codex-wire-peer");
        let python = which::which("python3").expect("protocol fixture requires python3 on PATH");
        let script = format!(r#"#!{}
import json, sys
steps = json.loads({})
for step in steps:
    if 'expect' in step:
        line = sys.stdin.readline()
        assert line, 'client closed stdin before expected frame: ' + repr(step)
        actual = json.loads(line)
        assert actual == step['expect'], 'unexpected outbound frame: ' + repr(actual) + ' expected ' + repr(step['expect'])
    elif 'send' in step:
        print(json.dumps(step['send']), flush=True)
    elif 'raw' in step:
        print(step['raw'], flush=True)
    elif 'stderr' in step:
        print(step['stderr'], file=sys.stderr, flush=True)
    elif 'exit' in step:
        sys.exit(step['exit'])
    elif 'eof' in step:
        assert sys.stdin.read() == '', 'unexpected retry or fallback after terminal response'
"#, python.display(), serde_json::to_string(&steps.to_string()).unwrap());
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut config = wire_config();
        config.cli_command = executable.to_string_lossy().into_owned();
        let stderr = CodexStderrBuffer::default();
        let (child, stdin, lines) = tokio::time::timeout(Duration::from_secs(5), spawn_process(&config, stderr.clone()))
            .await.expect("spawn timed out").unwrap();
        Self { child, stdin, lines, stderr, _directory: directory }
    }

    async fn read(&mut self, id: u64, notifications: &mut VecDeque<Value>) -> Result<Value, String> {
        tokio::time::timeout(Duration::from_secs(5), read_codex_rpc_response(
            &NoDesktopRequests, 41, &mut self.stdin, &mut self.lines, id, notifications,
        )).await.expect("RPC fixture timed out")
    }

    async fn initialize(&mut self, notifications: &mut VecDeque<Value>) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(5), initialize_codex_transport(
            &NoDesktopRequests, 41, "aipp", &mut self.stdin, &mut self.lines, notifications,
        )).await.expect("initialize fixture timed out")
    }

    async fn open(&mut self, stored_thread: Option<&str>, notifications: &mut VecDeque<Value>) -> Result<Value, String> {
        tokio::time::timeout(Duration::from_secs(5), open_codex_thread(
            &NoDesktopRequests, 41, &wire_config(), stored_thread, None, 2,
            &mut self.stdin, &mut self.lines, notifications,
        )).await.expect("thread open fixture timed out")
    }

    async fn finish(self, expected_code: i32) {
        let Self { mut child, stdin, stderr, _directory, .. } = self;
        drop(stdin);
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await
            .expect("fixture must exit or be killed on drop").unwrap();
        assert_eq!(status.code(), Some(expected_code), "peer transcript failed: {:?}", stderr.lock().unwrap());
    }
}

fn wire_config() -> CodexAppServerConfig {
    let mut config = extract_codex_app_server_config(&[], &[], None).unwrap();
    config.working_directory = std::env::current_dir().unwrap();
    config.model = Some("configured-model".into());
    config.reasoning_effort = Some("high".into());
    config.approval_policy = Some("on-request".into());
    config.sandbox = Some("workspace-write".into());
    config.approvals_reviewer = Some("user".into());
    config
}

fn handshake_steps() -> Vec<Value> {
    vec![
        json!({"expect":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "clientInfo":{"name":"aipp","title":"AIPP","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"experimentalApi":true}
        }}}),
        json!({"send":{"id":1,"result":{"userAgent":"fake-codex"}}}),
        json!({"expect":{"jsonrpc":"2.0","method":"initialized"}}),
    ]
}

fn thread_request(stored_thread: Option<&str>) -> Value {
    // Explicit protocol expectations, independent of the production builder.
    let (method, params) = match stored_thread {
        Some(id) => ("thread/resume", json!({"threadId":id,"approvalPolicy":"on-request", "sandbox":"workspace-write","approvalsReviewer":"user","config":null})),
        None => ("thread/start", json!({"model":"configured-model","reasoningEffort":"high", "cwd":std::env::current_dir().unwrap(),"approvalPolicy":"on-request", "sandbox":"workspace-write","approvalsReviewer":"user","config":null})),
    };
    json!({"jsonrpc":"2.0","id":2,"method":method,"params":params})
}

#[tokio::test]
async fn test_codex_transport_initializes_then_starts_or_resumes_without_history_rpc() {
    for stored_thread in [None, Some("saved-thread")] {
        let expected_native_id = stored_thread.unwrap_or("native-thread");
        let mut steps = handshake_steps();
        steps.extend([
            json!({"expect":thread_request(stored_thread)}),
            json!({"send":{"method":"thread/started","params":{"thread":{"id":expected_native_id}}}}),
            json!({"send":{"id":2,"result":{"thread":{"id":expected_native_id},"model":"configured-model"}}}),
            json!({"eof":true}),
        ]);
        let mut peer = WirePeer::start(json!(steps)).await;
        let mut notifications = VecDeque::new();
        peer.initialize(&mut notifications).await.unwrap();
        let result = peer.open(stored_thread, &mut notifications).await.unwrap();
        assert_eq!(thread_id_from_result(&result).as_deref(), Some(expected_native_id));
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0]["method"], "thread/started");
        peer.finish(0).await;
    }
}

#[tokio::test]
async fn test_codex_transport_resume_error_preserves_reason_without_start_fallback() {
    let mut steps = handshake_steps();
    steps.extend([
        json!({"expect":thread_request(Some("missing-thread"))}),
        json!({"send":{"id":2,"error":{"code":-32001,"message":"thread not found: missing-thread"}}}),
        json!({"eof":true}),
    ]);
    let mut peer = WirePeer::start(json!(steps)).await;
    let mut notifications = VecDeque::new();
    peer.initialize(&mut notifications).await.unwrap();
    let error = peer.open(Some("missing-thread"), &mut notifications).await.unwrap_err();
    assert!(error.contains("恢复失败") && error.contains("thread not found: missing-thread") && error.contains("-32001"));
    peer.finish(0).await;
}

#[tokio::test]
async fn test_codex_transport_initialize_error_does_not_notify_initialized_or_retry() {
    let mut steps = handshake_steps();
    steps.truncate(1);
    steps.extend([
        json!({"send":{"id":1,"error":{"code":-32002,"message":"initialize rejected"}}}),
        json!({"eof":true}),
    ]);
    let mut peer = WirePeer::start(json!(steps)).await;
    let error = peer.initialize(&mut VecDeque::new()).await.unwrap_err();
    assert!(error.contains("initialize rejected") && error.contains("-32002"));
    peer.finish(0).await;
}

#[tokio::test]
async fn test_codex_transport_correlates_responses_and_rejects_unknown_server_requests() {
    let mut peer = WirePeer::start(json!([
        {"send":{"id":"server-request","method":"future/unsupported","params":{}}},
        {"expect":{"jsonrpc":"2.0","id":"server-request","error":{"code":-32601,"message":"AIPP does not support Codex server request future/unsupported"}}},
        {"send":{"id":99,"result":{"wrong":"request"}}},
        {"send":{"method":"item/agentMessage/delta","params":{"threadId":"native-thread","turnId":"turn-1","delta":"hello"}}},
        {"send":{"id":7,"result":{"matched":true}}},
        {"eof":true}
    ])).await;
    let mut notifications = VecDeque::new();
    assert_eq!(peer.read(7, &mut notifications).await.unwrap(), json!({"matched":true}));
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["params"]["delta"], "hello");
    peer.finish(0).await;
}

#[tokio::test]
async fn test_codex_transport_old_completion_before_or_after_start_cannot_match_new_turn() {
    for before_response in [true, false] {
        let old = json!({"method":"turn/completed","params":{"threadId":"native-thread","turn":{"id":"old-turn","status":"completed"}}});
        let reply = json!({"id":5,"result":{"turn":{"id":"new-turn"}}});
        let mut steps = vec![json!({"expect":{"jsonrpc":"2.0","id":5,"method":"turn/start","params":{"threadId":"native-thread"}}})];
        if before_response { steps.push(json!({"send":old})); }
        steps.push(json!({"send":reply}));
        if !before_response { steps.push(json!({"send":old})); }
        steps.extend([
            json!({"send":{"method":"item/agentMessage/delta","params":{"threadId":"other-thread","turnId":"new-turn","delta":"foreign"}}}),
            json!({"send":{"method":"turn/completed","params":{"threadId":"native-thread","turn":{"id":"new-turn","status":"completed"}}}}),
            json!({"eof":true}),
        ]);
        let mut peer = WirePeer::start(json!(steps)).await;
        write_frame(&mut peer.stdin, &json_rpc_request(5, "turn/start", json!({"threadId":"native-thread"}))).await.unwrap();
        let mut notifications = VecDeque::new();
        let started = peer.read(5, &mut notifications).await.unwrap();
        assert_eq!(started["turn"]["id"], "new-turn");
        let expected_queued = usize::from(before_response);
        assert_eq!(notifications.len(), expected_queued);
        // Feed both the RPC-queued and post-response frames into the exact guard
        // used by run_session. This tests the boundary, not the Wry event loop.
        for _ in expected_queued..3 {
            let line = tokio::time::timeout(Duration::from_secs(5), peer.lines.next_line()).await.unwrap().unwrap().unwrap();
            notifications.push_back(serde_json::from_str(&line).unwrap());
        }
        let accepted = notifications.iter().filter(|frame| codex_notification_matches_turn(frame, "native-thread", Some("new-turn"))).collect::<Vec<_>>();
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0]["params"]["turn"]["id"], "new-turn");
        peer.finish(0).await;
    }
}

#[test]
fn test_codex_notification_identity_checks_only_explicit_thread_and_turn_fields() {
    for method in ["turn/started", "turn/completed"] {
        assert!(!codex_notification_matches_turn(&json!({"method":method,"params":{"turn":{"id":"old"}}}), "thread", Some("new")));
    }
    assert!(!codex_notification_matches_turn(&json!({"method":"item/agentMessage/delta","params":{"turnId":"old"}}), "thread", Some("new")));
    assert!(!codex_notification_matches_turn(&json!({"method":"error","params":{"threadId":"other"}}), "thread", Some("new")));
    for frame in [
        json!({"method":"thread/tokenUsage/updated","params":{"threadId":"thread"}}),
        json!({"method":"mcpServer/startupStatus/updated","params":{"name":"aipp"}}),
        json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}),
        json!({"method":"item/completed","params":{"item":{"id":"not-a-turn"}}}),
    ] {
        assert!(codex_notification_matches_turn(&frame, "thread", Some("new")), "unscoped legal notification must remain supported: {frame}");
    }
}

#[tokio::test]
async fn test_codex_transport_crash_and_malformed_frame_preserve_stage_and_diagnostics() {
    for malformed in [false, true] {
        let mut steps = vec![json!({"stderr":"native fixture failure before RPC 17"})];
        if malformed { steps.push(json!({"raw":"not-json"})); }
        steps.push(json!({"exit":17}));
        let mut peer = WirePeer::start(json!(steps)).await;
        let error = peer.read(17, &mut VecDeque::new()).await.unwrap_err();
        if malformed {
            assert!(error.contains("Invalid Codex app-server JSON") && error.contains("not-json"));
        } else {
            assert!(error.contains("id=17") && error.contains("stdout"));
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while peer.stderr.lock().unwrap().is_empty() { tokio::task::yield_now().await; }
        }).await.expect("stderr reader must finish");
        let error = codex_error_with_diagnostics(&error, 41, "fixture-run", &peer.stderr);
        assert!(error.contains("native fixture failure before RPC 17") && error.contains("conversation_id=41") && error.contains("run_id=fixture-run"));
        peer.finish(17).await;
    }
}

#[tokio::test]
async fn test_codex_transport_concurrent_sessions_keep_responses_and_notifications_isolated() {
    async fn session(name: &str) -> (String, String) {
        let mut steps = handshake_steps();
        steps.extend([
            json!({"expect":thread_request(Some(name))}),
            json!({"send":{"method":"thread/started","params":{"threadId":name}}}),
            json!({"send":{"id":2,"result":{"thread":{"id":name}}}}),
            json!({"eof":true}),
        ]);
        let mut peer = WirePeer::start(json!(steps)).await;
        let mut notifications = VecDeque::new();
        peer.initialize(&mut notifications).await.unwrap();
        let thread = peer.open(Some(name), &mut notifications).await.unwrap();
        let identity = thread_id_from_result(&thread).unwrap();
        let notification_id = notifications.pop_front().unwrap()["params"]["threadId"].as_str().unwrap().to_string();
        assert!(notifications.is_empty());
        peer.finish(0).await;
        (identity, notification_id)
    }
    let (first, second) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(session("first-thread"), session("second-thread"))
    }).await.unwrap();
    assert_eq!(first, ("first-thread".into(), "first-thread".into()));
    assert_eq!(second, ("second-thread".into(), "second-thread".into()));
}

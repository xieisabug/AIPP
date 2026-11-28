// Native transport control tests: real production handles, mpsc and oneshot channels.
// No installed/authenticated agent CLI, Wry window, sleeps or database is required.
// Unix process tests invoke the system false executable directly, without a shell.
use super::*;

#[test]
fn test_claude_effort_uses_reported_model_levels_and_passes_assistant_choice_to_cli() {
    let mut config = extract_claude_sdk_config(&[], &[], Some("opus".into()), vec![]).unwrap();
    config.instance_id = Some(794);
    let data = json!({"models":[
        {"value":"default","supportedEffortLevels":["low","high"]},
        {"value":"opus","resolvedModel":"claude-opus-5-5","supportedEffortLevels":["low","xhigh","max"]},
        {"value":"custom","supportsEffort":false}
    ]});
    let frame = json!({"type":"control_response","response":{"subtype":"success","request_id":MODEL_CATALOG_REQUEST,"response":data}});
    REPORTED_MODELS.lock().unwrap().insert(config.catalog_key(794), reported_claude_models(&frame).unwrap());
    assert_eq!(claude_model_efforts(&config, 794, CLAUDE_CODE_DEFAULT_MODEL), ["default", "low", "high"]);
    assert_eq!(claude_model_efforts(&config, 794, "custom"), ["default"]);
    assert_eq!(claude_model_efforts(&config, 794, "claude-opus-5-5"), ["default", "low", "xhigh", "max"]);
    config.effort = Some("xhigh".into());
    let selector = claude_config_options(&config).into_iter().find(|o|o.id == "effort").unwrap();
    assert_eq!(selector.current_value, "xhigh");
    assert_eq!(selector.options.iter().map(|o|o.value.as_str()).collect::<Vec<_>>(), ["default","low","xhigh","max"]);
    for session in [None, Some("existing")] {
        let command = build_claude_command(&config, session, std::path::Path::new("claude"));
        let args: Vec<_> = command.as_std().get_args().map(|v|v.to_str().unwrap()).collect();
        assert!(args.windows(2).any(|pair|pair == ["--effort", "xhigh"]));
    }
}

#[test]
fn test_claude_permission_mode_for_new_and_resumed_sessions() {
    let provider_mode = crate::db::llm_db::LLMProviderConfig {
        id: 1, llm_provider_id: 1, name: "claude_permission_mode".into(),
        value: "auto".into(), append_location: String::new(), is_addition: false,
    };
    let config = extract_claude_sdk_config(&[], &[provider_mode], None, Vec::new()).unwrap();
    let mode = claude_config_options(&config).into_iter()
        .find(|option| option.id == "permission_mode").unwrap();
    assert_eq!(mode.current_value, "auto");
    let mut changed = config.clone();
    changed.permission_mode = Some("dontAsk".into());
    changed.turn_permission_mode = Some("bypassPermissions".into());
    refresh_claude_session_signature(&mut changed);
    assert_eq!(config.session_signature, changed.session_signature, "权限切换必须复用现有进程");
    assert!(mode.options.iter().any(|choice| choice.value == "auto"));
    for session_id in [None, Some("existing-session")] {
        let command = build_claude_command(&config, session_id, std::path::Path::new("claude"));
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(args.iter().filter(|arg| **arg == "--permission-mode").count(), 1);
        assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "auto"]));
        assert!(args.iter().any(|arg| *arg == "--allow-dangerously-skip-permissions"));
    }
}

#[tokio::test]
async fn test_claude_permission_changes_on_one_stream_only_after_matching_ack() {
    let mut config = extract_claude_sdk_config(&[], &[], None, vec![]).unwrap();
    let (client, peer) = tokio::io::duplex(4096);
    let (read, mut write) = tokio::io::split(client);
    let mut lines = BufReader::new(read).lines();
    let (peer_read, mut peer_write) = tokio::io::split(peer);
    let mut requests = BufReader::new(peer_read).lines();
    let modes = ["acceptEdits", "auto", "bypassPermissions", "dontAsk", "default"];
    let cli = async {
        for (i, mode) in modes.iter().enumerate() {
            let request: Value = serde_json::from_str(&requests.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["request"], json!({"subtype":"set_permission_mode","mode":mode}));
            let reply = json!({"type":"control_response","response":{
                "request_id":request["request_id"], "subtype":if i == 1 {"error"} else {"success"},
                "error":"classifier unavailable"
            }});
            let stale = json!({"type":"control_response","response":{"request_id":"unrelated","subtype":"success"}});
            let notification = json!({"type":"system","subtype":"task_notification","task_id":"prior-task"});
            peer_write.write_all(format!("{stale}\n{notification}\n{reply}\n").as_bytes()).await.unwrap();
        }
    };
    let app = async {
        for mode in modes {
            let mut deferred = VecDeque::new();
            let result = apply_claude_permission_mode(&mut config, mode, &mut write, &mut lines, &mut deferred).await;
            assert_eq!(deferred.len(), 2);
            assert_eq!(deferred[1]["task_id"], "prior-task");
            if mode == "auto" {
                assert!(result.unwrap_err().to_string().contains("classifier unavailable"));
                assert_eq!(config.permission_mode.as_deref(), Some("acceptEdits"));
            } else {
                result.unwrap();
                assert_eq!(config.permission_mode.as_deref(), Some(mode));
            }
        }
    };
    tokio::join!(cli, app);
}

fn handle_channel(run_id: &str) -> (ClaudeSessionHandle, mpsc::UnboundedReceiver<SessionCommand>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (ClaudeSessionHandle { permissions: Default::default(), sender, cancel_signal: watch::channel(0).0, run_id: run_id.into() }, receiver)
}

#[test]
fn test_claude_turn_usage_survives_error_interrupt_and_duplicate_frames() {
    let mut stream = ClaudeStreamUsage::default();
    let start = json!({"event":{"type":"message_start","message":{"id":"a","usage":{"input_tokens":12,"cache_read_input_tokens":6}}}});
    assert_eq!(claude_usage_counts(&stream.observe(&start).unwrap()).input, 18);
    assert!(stream.observe(&start).is_none());
    let delta = json!({"event":{"type":"message_delta","usage":{"output_tokens":4}}});
    assert_eq!(claude_usage_counts(&stream.observe(&delta).unwrap()).total, 22);
    assert!(stream.observe(&delta).is_none());
    let child = json!({"parent_tool_use_id":"child","event":{"type":"message_delta","usage":{"output_tokens":100}}});
    assert!(stream.observe(&child).is_none());
    let next = json!({"event":{"type":"message_start","message":{"id":"b","usage":{"input_tokens":5}}}});
    let mut reply = ClaudeTurnOutput::new(9);
    reply.usage = stream.observe(&next).unwrap();
    assert_eq!(claude_usage_counts(&reply.usage).total, 27);
    let error = json!({"is_error":true,"result":"failure","usage":{"input_tokens":35,"output_tokens":14}});
    assert!(reply.apply_result(&error, None, false).is_err());
    assert_eq!(claude_usage_counts(&reply.usage).total, 49);
    let interrupted = json!({"is_error":true,"result":"interrupted","usage":{"input_tokens":0}});
    assert!(reply.apply_result(&interrupted, None, true).unwrap().is_break());
    assert_eq!(claude_usage_counts(&reply.usage).total, 49);
}

/// Real streaming CLI, one user prompt. No tools other than Write are exposed.
#[tokio::test]
#[ignore = "uses the installed Claude Code account and two temporary files"]
async fn test_claude_live_switch_while_first_write_awaits_approval() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap();
    config.working_directory = dir.path().to_path_buf();
    let program = Path::new(if cfg!(windows) { "claude.exe" } else { "claude" });
    let mut child = build_claude_command(&config, None, program)
        .args(["--tools", "Write", "--strict-mcp-config", "--setting-sources", "user", "--no-session-persistence"])
        .stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    let prompt = format!("Use Write to create {} containing one. Wait for that result. Then use Write to create {} containing two. Make the calls sequentially. Finally say done.", first.display(), second.display());
    let input = claude_user_input(prompt, vec![], None).await.unwrap();
    stdin.write_all(format!("{input}\n").as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();
    let (handle, mut commands) = handle_channel("live-test");
    let mut deferred = VecDeque::new();
    let mut approvals = 0;
    let mut decisions = FuturesUnordered::new();
    let mut session = None;
    let mut steps = ClaudeStreamUsage::default();
    let mut live_tokens = 0;
    tokio::time::timeout(Duration::from_secs(150), async {
        loop {
            let frame = match deferred.pop_front() {
                Some(frame) => frame,
                None => match read_claude_prompt_input(&mut stdin, &mut lines, &mut commands, &mut decisions).await.unwrap() {
                    ClaudePromptInput::Frame(frame) => frame,
                    ClaudePromptInput::PermissionReply(reply) => {
                        write_permission_reply(&mut stdin, reply).await.unwrap();
                        continue;
                    }
                    _ => panic!("unexpected UI command"),
                },
            };
            if let Some(id) = frame["session_id"].as_str() {
                assert_eq!(session.get_or_insert_with(|| id.to_owned()), id);
            }
            if let Some(usage) = steps.observe(&frame) { live_tokens = claude_usage_counts(&usage).total; }
            match frame["type"].as_str() {
                Some("control_request") => {
                    approvals += 1;
                    assert_eq!(approvals, 1, "second Write unexpectedly requested manual approval");
                    assert_eq!(frame["request"]["tool_name"], "Write");
                    assert_eq!(Path::new(frame["request"]["input"]["file_path"].as_str().unwrap()), first);
                    let (answer, decision) = oneshot::channel();
                    decisions.push(async move { decision.await.map_err(|e| e.to_string()) }.boxed());
                    let switch = async {
                        let (mode, response) = loop {
                            match read_claude_prompt_input(&mut stdin, &mut lines, &mut commands, &mut decisions).await.unwrap() {
                                ClaudePromptInput::SetPermissionMode(mode, response) => break (mode, response),
                                ClaudePromptInput::Frame(frame) => deferred.push_back(frame),
                                _ => panic!("unexpected command while switching mode"),
                            }
                        };
                        response.send(apply_claude_permission_mode(&mut config, &mode, &mut stdin, &mut lines, &mut deferred)
                            .await.map_err(|error| error.to_string())).unwrap();
                    };
                    let (result, ()) = tokio::join!(handle.set_config_option("permission_mode".into(), "auto".into()), switch);
                    result.unwrap();
                    assert!(!first.exists());
                    let allow = claude_control_permission_allow(frame["request_id"].as_str().unwrap(), frame["request"]["input"].clone());
                    answer.send(allow).unwrap();
                }
                Some("result") => {
                    assert_ne!(frame["is_error"], true, "CLI error: {}", frame["result"]);
                    assert!(claude_usage_counts(&frame["usage"]).total > 0);
                    break;
                }
                _ => {}
            }
        }
    }).await.expect("Claude Code live switch timed out");
    assert!(session.is_some() && live_tokens > 0);
    assert_eq!(approvals, 1);
    assert_eq!(std::fs::read_to_string(first).unwrap().trim(), "one");
    assert_eq!(std::fs::read_to_string(second).unwrap().trim(), "two");
}

#[tokio::test]
async fn test_queued_permissions_claude_overrides_enqueue_mode_only_after_matching_ack() {
    use crate::api::ai::agent_driver::{next_permissions, queued_permission_fixture::Queue};
    for reject in [false, true] {
        let queue = Queue::new();
        let session = "original-claude-session";
        queue.db.upsert_agent_session_id_for_instance(queue.cid, CLAUDE_SDK_API_TYPE, Some(3), session).unwrap();
        let (client, peer) = tokio::io::duplex(4096);
        let (read, mut write) = tokio::io::split(client);
        let mut lines = BufReader::new(read).lines();
        let (peer_read, mut peer_write) = tokio::io::split(peer);
        let mut requests = BufReader::new(peer_read).lines();
        let (edited, release) = oneshot::channel();
        let cli = async {
            let first: Value = serde_json::from_str(&requests.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(first["session_id"], session);
            assert_eq!(first["message"]["content"][0]["text"], "first");
            peer_write.write_all(b"{\"type\":\"stream_event\"}\n").await.unwrap();
            release.await.unwrap();
            peer_write.write_all(b"{\"type\":\"result\"}\n").await.unwrap();
            let request: Value = serde_json::from_str(&requests.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["request"], json!({"subtype":"set_permission_mode","mode":"dontAsk"}));
            let ack = json!({"type":"control_response","response":{"request_id":request["request_id"],
                "subtype":if reject {"error"} else {"success"}, "error":"fixture rejected dontAsk"}});
            peer_write.write_all(format!("{ack}\n").as_bytes()).await.unwrap();
            if !reject {
                let second: Value = serde_json::from_str(&requests.next_line().await.unwrap().unwrap()).unwrap();
                assert_eq!(second["session_id"], session);
                assert_eq!(second["message"]["content"][0]["text"], "second");
            }
            assert!(requests.next_line().await.unwrap().is_none());
        };
        let app = async move {
            let first = claude_user_input("first".into(), vec![], Some(session.into())).await.unwrap();
            write.write_all(format!("{first}\n").as_bytes()).await.unwrap();
            assert_eq!(lines.next_line().await.unwrap().unwrap(), r#"{"type":"stream_event"}"#);
            let id = queue.enqueue(r#"{"override_model_config":{"permission_mode":"acceptEdits"}}"#);
            for value in ["auto", "dontAsk"] {
                next_permissions::save(&queue.conn, queue.cid, CLAUDE_SDK_API_TYPE, 3, session, "permission_mode", value).unwrap();
            }
            edited.send(()).unwrap();
            assert_eq!(lines.next_line().await.unwrap().unwrap(), r#"{"type":"result"}"#);
            let row = queue.dequeue(id);
            let payload: Value = serde_json::from_str(&row.request_json).unwrap();
            let mut config = extract_claude_sdk_config(&[], &[], None, vec![]).unwrap();
            config.instance_id = Some(3);
            config.turn_permission_mode = Some(payload["override_model_config"]["permission_mode"].as_str().unwrap().into());
            config.apply_saved_turn_permission(&queue.conn, row.conversation_id).unwrap();
            let permissions = ClaudeTurnPermissions { configured: None, selected: config.turn_permission_mode.take() };
            let mode = permissions.desired(&None).unwrap();
            let result = apply_claude_permission_mode(&mut config, mode, &mut write, &mut lines, &mut VecDeque::new()).await;
            match result {
                Ok(()) => {
                    let frame = claude_user_input(row.prompt, vec![], Some(session.into())).await.unwrap();
                    write.write_all(format!("{frame}\n").as_bytes()).await.unwrap();
                }
                Err(error) => assert!(reject && error.to_string().contains("fixture rejected dontAsk"), "{error}"),
            }
        };
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(cli, app) }).await.unwrap();
    }
}

fn entry(id: i64, run_id: &str) -> ClaudeSessionEntry {
    let (handle, _receiver) = handle_channel(run_id);
    ClaudeSessionEntry::new(handle, id, "signature".into(), None, None, None)
}

#[tokio::test]
async fn test_claude_closed_control_channel_rejects_cancel_and_config() {
    let (handle, receiver) = handle_channel("closed-run");
    drop(receiver);
    let cancel = handle.cancel_current_prompt().await.unwrap_err().to_string();
    let config = handle.set_config_option("model".into(), "chosen".into()).await.unwrap_err().to_string();
    assert!(cancel.contains("Claude Code session closed"));
    assert!(config.contains("Claude Code session closed"));
}

#[tokio::test]
async fn test_claude_cancel_dropped_reply() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let SessionCommand::Cancel { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        drop(response);
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.cancel_current_prompt(), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("Claude Code session closed"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_claude_cancel_native_failure() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let SessionCommand::Cancel { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        response.send(Err("native protocol refused control: request 17".into())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.cancel_current_prompt(), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("native protocol refused control: request 17"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_claude_cancel_success() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let SessionCommand::Cancel { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        response.send(Ok(())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.cancel_current_prompt(), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.is_ok());
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_claude_config_dropped_reply() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let SessionCommand::SetConfig { response, config_id, value } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        assert_eq!(config_id, "model");
        assert_eq!(value, "chosen");
        drop(response);
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(handle.set_config_option("model".into(), "chosen".into()), respond)
    }).await.expect("native control must return a response or failure");
    assert!(result.unwrap_err().to_string().contains("Claude Code session closed"));
    assert!(receiver.try_recv().is_err(), "control must not silently retry");
}

#[tokio::test]
async fn test_claude_config_native_failure() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let SessionCommand::SetConfig { response, config_id, value } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
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
async fn test_claude_config_success() {
    let (handle, mut receiver) = handle_channel("control-run");
    let respond = async {
        let SessionCommand::SetConfig { response, config_id, value } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
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
fn test_claude_stale_cleanup_preserves_replacement_and_other_conversation() {
    let mut sessions = HashMap::from([(1, entry(1, "replacement")), (2, entry(2, "other"))]);
    assert!(!remove_claude_session_for_run(&mut sessions, 1, "stale-run"));
    assert_eq!(sessions[&1].run_id, "replacement");
    assert_eq!(sessions[&1].handle.run_id, "replacement");
    assert_eq!(sessions[&1].snapshot.conversation_id, 1);
    assert!(remove_claude_session_for_run(&mut sessions, 1, "replacement"));
    assert!(!sessions.contains_key(&1));
    assert_eq!(sessions[&2].run_id, "other");
    assert!(!remove_claude_session_for_run(&mut sessions, 1, "replacement"));
}

#[test]
fn test_claude_active_prompt_prevents_idle_release_and_touch_refreshes_entry() {
    let mut session = entry(1, "run");
    session.last_activity = Instant::now() - Duration::from_secs(120);
    session.snapshot.has_active_prompt = true;
    assert!(!session.is_idle_for(Duration::from_secs(60)));
    session.snapshot.has_active_prompt = false;
    session.background_pending = true;
    assert!(!session.is_idle_for(Duration::from_secs(60)));
    session.background_pending = false;
    assert!(session.is_idle_for(Duration::from_secs(60)));
    let previous = session.last_activity;
    session.touch();
    assert!(session.last_activity > previous);
    assert!(!session.is_idle_for(Duration::from_secs(60)));
}

#[test]
fn test_claude_result_failure_markers_preserve_native_reason() {
    for frame in [
        json!({"is_error": true, "result": "interrupt acknowledged"}),
        json!({"subtype": "error", "result": "interrupt acknowledged"}),
        json!({"error": "interrupt acknowledged", "result": "partial output"}),
    ] {
        assert_eq!(claude_result_error(&frame).as_deref(), Some("interrupt acknowledged"));
    }
    assert_eq!(
        claude_result_error(&json!({"is_error": true, "error": "permission denied", "result": "partial output"})).as_deref(),
        Some("permission denied"),
    );
}

#[test]
fn test_claude_success_and_null_error_are_not_failures() {
    for frame in [
        json!({"subtype": "success", "result": "done"}),
        json!({"is_error": false, "error": null, "result": "done"}),
    ] {
        assert_eq!(claude_result_error(&frame), None);
    }
    assert!(claude_result_error(&json!({"is_error": true})).is_some());
    assert!(claude_result_error(&json!({"error": {"message": "structured failure"}})).is_some());
}

#[test]
fn test_claude_resume_identity_is_exact_and_event_is_scoped_to_run() {
    assert!(confirm_claude_resume(Some("native-session"), "native-session ", "run-1").is_err());
    let first = confirm_claude_resume(Some("native-session"), "native-session", "run-1").unwrap();
    let next = confirm_claude_resume(Some("native-session"), "native-session", "run-2").unwrap();
    assert_eq!(first, Some(("resume".into(), "run-1:resume".into())));
    assert_eq!(next, Some(("resume".into(), "run-2:resume".into())));
}

#[cfg(unix)]
#[tokio::test]
async fn test_claude_native_process_nonzero_exit_closes_stdout() {
    let mut config = extract_claude_sdk_config(&[], &[], None, Vec::new()).unwrap();
    config.cli_command = which::which("false")
        .expect("Unix process fixture requires the system false executable on PATH")
        .to_string_lossy().into_owned();
    config.working_directory = std::env::current_dir().unwrap();
    let (mut child, _stdin, mut lines) = spawn_claude_process(&config, Some("native-session")).await.unwrap();
    let (status, output) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(child.wait(), lines.next_line())
    }).await.expect("fake process must terminate");
    assert_eq!(status.unwrap().code(), Some(1));
    assert_eq!(output.unwrap(), None);
}

#[tokio::test]
async fn test_claude_reuse_matching_signature_preserves_native_handle_and_refreshes_idle() {
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
        let SessionCommand::Cancel { response } = receiver.recv().await.unwrap() else { panic!("wrong native command") };
        response.send(Ok(())).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(reused.cancel_current_prompt(), respond)
    }).await.expect("reused handle must address the existing receiver");
    assert!(result.is_ok());
}

#[test]
fn test_claude_reuse_mismatch_leaves_entry_unchanged_and_entries_are_isolated() {
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

#[tokio::test]
async fn test_agent_permission_review_claude_native_reply_and_late_decision() {
    use crate::api::ai::agent_driver::native_agents::{review_outcome, ReviewOutcome};
    let options = vec![AcpPermissionOptionPayload { option_id: "allow".into(), name: "本次允许".into(), kind: "allow_once".into() }];
    let request = json!({"tool_name":"Read","input":{"file_path":"README.md"}});
    let ReviewOutcome::Allow(option) = review_outcome(&options, "safe", "读取说明") else { panic!("expected one-time approval") };
    let (tx, rx) = oneshot::channel();
    tx.send(AcpPermissionDecision::Selected(option)).unwrap();
    let (_signal, mut cancellation) = watch::channel(0);
    let state = AcpPermissionState::new();
    let (decision, _) = wait_claude_permission_decision(&state, "claude:42:run:req", rx, &mut cancellation, 0).await;
    let response = claude_permission_response("req", &request, decision);
    assert_eq!(response["response"]["request_id"], "req");
    assert_eq!(response["response"]["response"]["behavior"], "allow");
    assert_eq!(response["response"]["response"]["updatedInput"], request["input"]);
    assert!(response["response"]["response"].get("message").is_none());

    let (tx, rx) = oneshot::channel();
    let (_signal, mut cancelled) = watch::channel(1);
    let (decision, _) = wait_claude_permission_decision(&state, "claude:42:run:req", rx, &mut cancelled, 0).await;
    assert!(tx.send(AcpPermissionDecision::Selected("allow".into())).is_err());
    let denied = claude_permission_response("req", &request, decision);
    assert_eq!(denied["response"]["response"]["behavior"], "deny");
    assert_eq!(denied["response"]["response"]["message"], "Permission denied by user");
    assert!(denied["response"]["response"].get("updatedInput").is_none());
}

#[test]
fn test_model_catalog_merges_native_configured_and_reported_models_for_one_provider() {
    let config = extract_claude_sdk_config(&[], &[], Some("previous".into()), vec![]).unwrap();
    let models = reported_claude_models(&json!({"type":"control_response","response":{
        "request_id":MODEL_CATALOG_REQUEST,"subtype":"success","response":{"claude_code_version":"2.1.326","models":[
            {"value":"default","resolvedModel":"opus[1m]"},{"value":"opus","resolvedModel":"opus"},
            {"value":"sonnet","resolvedModel":"opus"},{"value":"cli-extra","displayName":"CLI extra"}]}}})).unwrap();
    REPORTED_MODELS.lock().unwrap().insert(config.catalog_key(781), models);
    let configured = vec![(1, "Registered".into(), 781, "custom".into(), String::new(), false, false, false)];
    let options = claude_model_choices(&config, 781, &configured, true);
    let ids: Vec<_> = options.iter().map(|o|o.value.as_str()).collect();
    assert_eq!(ids, [CLAUDE_CODE_DEFAULT_MODEL, "claude-opus-5-5", "claude-sonnet-5-5", "opus", "sonnet", "cli-extra", "custom", "previous"]);
    assert_eq!(options[0].name, "使用 CLI 默认模型 → opus[1m]");
    assert_eq!(options.iter().find(|o| o.value == "sonnet").unwrap().name, "CLI 路由 sonnet → opus");
    assert_eq!(options.iter().find(|o| o.value == "opus").unwrap().name, "CLI 别名 · opus");
    for choice in &options[1..] {
        let mut selected = config.clone();
        selected.model = Some(choice.value.clone());
        for session in [None, Some("existing")] {
            let command = build_claude_command(&selected, session, std::path::Path::new("claude"));
            let args: Vec<_> = command.as_std().get_args().map(|arg|arg.to_str().unwrap()).collect();
            assert!(args.windows(2).any(|pair|pair == ["--model", &choice.value]));
        }
    }
    assert!(claude_model_choices(&config, 782, &[], false).iter().all(|o|o.value == "previous"));
}

#[test]
fn test_model_catalog_preserves_backend_routes_and_limits_pins_to_supported_cli() {
    let config = extract_claude_sdk_config(&[], &[], None, vec![]).unwrap();
    for (provider_id, version) in [(784, "2.1.326"), (785, "2.1.279")] {
        let data = json!({"claude_code_version":version, "models":[
            {"value":"opus","resolvedModel":"us.anthropic.claude-opus-5-5-v1:0"},
            {"value":"sonnet","resolvedModel":"sonnet"},
            {"value":"haiku","resolvedModel":"claude-haiku-5-5@20261007"}]});
        let frame = json!({"type":"control_response","response":{"subtype":"success","request_id":MODEL_CATALOG_REQUEST,"response":data}});
        REPORTED_MODELS.lock().unwrap().insert(config.catalog_key(provider_id), reported_claude_models(&frame).unwrap());
        let options = claude_model_choices(&config, provider_id, &[], true);
        assert!(options.iter().any(|o| o.value == "opus" && o.name.contains("us.anthropic.")));
        assert!(!options.iter().any(|o| o.value == "claude-opus-5-5" || o.value == "claude-haiku-5-5"));
        assert_eq!(options.iter().any(|o| o.value == "claude-sonnet-5-5"), provider_id == 784);
    }
}

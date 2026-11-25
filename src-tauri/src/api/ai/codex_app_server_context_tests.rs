use super::*;

#[tokio::test]
async fn test_queued_permissions_codex_uses_final_snapshot_in_second_turn_start() {
    use crate::api::ai::agent_driver::queued_permission_fixture::Queue;
    let queue = Queue::new();
    let (mut writer, reader) = tokio::io::duplex(4096);
    let mut peer = BufReader::new(reader).lines();
    let (mut server, notifications) = tokio::io::duplex(4096);
    let mut notifications = BufReader::new(notifications).lines();
    let thread = "original-thread";
    let mut snapshot = CodexConversationSessionState {
        approval_policy: Some("on-request".into()), sandbox: Some("read-only".into()),
        approvals_reviewer: Some("user".into()), ..Default::default()
    };
    let turn_start = |id, state: &CodexConversationSessionState, prompt: &str| {
        json_rpc_request(id, "turn/start", codex_turn_start_params(thread, state, prompt, &Default::default()))
    };
    write_frame(&mut writer, &turn_start(1, &snapshot, "first")).await.unwrap();
    let first: Value = serde_json::from_str(&peer.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(first["params"]["approvalPolicy"], "on-request");
    assert_eq!(first["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(first["params"]["threadId"], thread);
    write_frame(&mut server, &json!({
        "method": "item/agentMessage/delta",
        "params": { "threadId": thread, "turnId": "turn-1", "delta": "working" }
    })).await.unwrap();
    let delta: Value = serde_json::from_str(&notifications.next_line().await.unwrap().unwrap()).unwrap();
    assert!(codex_notification_matches_turn(&delta, thread, Some("turn-1")));
    assert_eq!(delta["params"]["delta"], "working");
    let id = queue.enqueue("{}");
    for (key, value) in [("approval_policy", "never"), ("sandbox", "danger-full-access"),
                         ("approval_policy", "untrusted"), ("sandbox", "workspace-write"), ("approvals_reviewer", "auto_review")] {
        apply_codex_config_option(&mut snapshot, &[], key, value.into()).unwrap();
    }
    assert!(apply_codex_config_option(&mut snapshot, &[], "approval_policy", "invalid".into()).is_err());
    write_frame(&mut server, &json!({
        "method": "turn/completed",
        "params": { "threadId": thread, "turn": { "id": "turn-1", "status": "completed" } }
    })).await.unwrap();
    let done: Value = serde_json::from_str(&notifications.next_line().await.unwrap().unwrap()).unwrap();
    assert!(codex_notification_matches_turn(&done, thread, Some("turn-1")));
    assert_eq!(done["params"]["turn"]["status"], "completed");
    let row = queue.dequeue(id);
    write_frame(&mut writer, &turn_start(2, &snapshot, &row.prompt)).await.unwrap();
    drop(writer);
    let second: Value = serde_json::from_str(&peer.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(second["method"], "turn/start");
    assert_eq!(second["params"]["threadId"], thread);
    assert_eq!(second["params"]["input"][0]["text"], "second");
    assert_eq!(second["params"]["approvalPolicy"], "untrusted");
    assert_eq!(second["params"]["sandboxPolicy"]["type"], "workspaceWrite");
    assert_eq!(second["params"]["approvalsReviewer"], "auto_review");
    assert!(peer.next_line().await.unwrap().is_none());
}

#[test]
fn test_butler_context_codex_splits_without_truncation_and_keeps_user_input_separate() {
    let role = format!("角色约定\n{}\n工具约定", "文".repeat(4001));
    let context = codex_application_context(&role);
    assert!(context.len() > 2);
    let params = codex_turn_start_params("thread", &Default::default(), "用户原话", &context);
    assert_eq!(params["input"][0]["text"], "用户原话");
    let mut restored = String::new();
    for entry in context.values() {
        assert_eq!(entry["kind"], "application");
        let value = entry["value"].as_str().unwrap();
        assert!(value.len() <= 4000);
        restored.push_str(value);
    }
    assert_eq!(restored, role);
    assert_eq!(params["additionalContext"], json!(context));
    let ordinary = codex_turn_start_params("thread", &Default::default(), "hello", &Default::default());
    assert!(ordinary.get("additionalContext").is_none());
}

#[tokio::test]
async fn test_butler_context_codex_restores_on_compaction_before_turn_completed() {
    let context = codex_application_context("管家角色与工具说明");
    let mut restore = CodexContextRestore::default();
    let mut rpc = 3;
    let (mut writer, reader) = tokio::io::duplex(4096);
    let mut wire = BufReader::new(reader).lines();
    let compact = json!({"item":{"id":"compact-1","type":"contextCompaction"}});
    restore.on_notification("item/completed", &compact, false, "thread", &context, &mut rpc, &mut writer).await.unwrap();

    // The server withholds turn/completed until it receives the restore request.
    let line = tokio::time::timeout(Duration::from_secs(1), wire.next_line()).await.unwrap().unwrap().unwrap();
    let frame: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(frame, json_rpc_request(4, "thread/inject_items", json!({"threadId":"thread","items":[{
        "type":"message","role":"developer","content":[{"type":"input_text","text":"<aipp_butler_00000000>管家角色与工具说明</aipp_butler_00000000>"}]
    }]})));
    restore.acknowledge("thread", &json!({"id":4,"result":{}})).unwrap();
    restore.on_notification("thread/compacted", &json!({}), false, "thread", &context, &mut rpc, &mut writer).await.unwrap();
    restore.on_notification("item/completed", &compact, false, "thread", &context, &mut rpc, &mut writer).await.unwrap();
    restore.on_notification("turn/completed", &json!({"turn":{"status":"completed"}}), false, "thread", &context, &mut rpc, &mut writer).await.unwrap();
    assert_eq!(rpc, 4);
    assert!(restore.pending.is_empty());
}

#[tokio::test]
async fn test_butler_context_codex_tracks_multiple_compactions_and_late_rpc_errors() {
    let context = codex_application_context("role");
    let mut restore = CodexContextRestore::default();
    let mut rpc = 8;
    let mut wire = Vec::new();
    for (method, params) in [
        ("thread/compacted", json!({})),
        ("item/completed", json!({"item":{"id":"first","type":"contextCompaction"}})),
        ("item/completed", json!({"item":{"id":"second","type":"contextCompaction"}})),
        ("turn/completed", json!({"turn":{"status":"completed"}})),
    ] {
        restore.on_notification(method, &params, true, "thread", &context, &mut rpc, &mut wire).await.unwrap();
    }
    assert_eq!(rpc, 10);
    restore.acknowledge("thread", &json!({"id":10,"result":{}})).unwrap();
    assert!(restore.pending.contains(&9));
    let err = restore.acknowledge("thread", &json!({"id":9,"error":{"message":"restore denied"}})).unwrap_err();
    assert!(err.contains("thread") && err.contains("restore denied"));
}

#[tokio::test]
async fn test_butler_context_codex_manual_compact_without_item_only_restores_on_success() {
    let mut restore = CodexContextRestore::default();
    let mut rpc = 0;
    let mut wire = Vec::new();
    for (context, status) in [("", "completed"), ("role", "failed"), ("role", "interrupted"), ("role", "completed")] {
        restore.on_notification("turn/completed", &json!({"turn":{"status":status}}), true, "thread",
            &codex_application_context(context), &mut rpc, &mut wire).await.unwrap();
    }
    assert_eq!(rpc, 1);
}

#[tokio::test]
async fn test_codex_attachment_images_keep_bytes_and_order() {
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jvLkAAAAASUVORK5CYII=";
    let gif = "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
    let inputs = [format!("data:image/png;base64,{png}"), format!("data:image/gif;base64,{gif}")];
    let attachments = inputs.iter().enumerate().map(|(id, url)| MessageAttachment {
        id: id as i64, message_id: 1, attachment_type: crate::db::conversation_db::AttachmentType::Image,
        attachment_url: None, attachment_hash: None, attachment_content: Some(url.clone()), use_vector: false, token_count: None,
    }).collect();
    assert_eq!(codex_image_items(attachments).await.unwrap(), inputs.map(|url| json!({"type":"image","url":url})));
}

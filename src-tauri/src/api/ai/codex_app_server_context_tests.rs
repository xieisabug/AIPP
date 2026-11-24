use super::*;

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

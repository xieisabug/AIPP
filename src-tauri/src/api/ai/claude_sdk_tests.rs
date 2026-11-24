use super::claude_sdk::{claude_usage_counts, confirm_claude_resume, ClaudeUsageCounts};

#[test]
fn test_claude_resume_confirms_matching_session_id() {
    let confirmation = confirm_claude_resume(Some("session-1"), "session-1", "run-1")
        .expect("matching session id should confirm resume");

    assert_eq!(
        confirmation,
        Some(("resume".to_string(), "run-1:resume".to_string()))
    );
}

#[test]
fn test_claude_resume_rejects_mismatched_session_id() {
    let error = confirm_claude_resume(Some("session-1"), "session-2", "run-1")
        .expect_err("mismatched session id must fail");

    assert!(error.contains("session_id=session-1"));
    assert!(error.contains("session_id=session-2"));
}

#[test]
fn test_claude_resume_ignores_fresh_session() {
    assert_eq!(
        confirm_claude_resume(None, "session-1", "run-1")
            .expect("fresh session should not be treated as resume"),
        None
    );
}

#[test]
fn test_claude_usage_counts_include_cache_in_input() {
    let usage = serde_json::json!({
        "input_tokens": 12,
        "cache_creation_input_tokens": 3000,
        "cache_read_input_tokens": 45000,
        "output_tokens": 800,
    });
    assert_eq!(
        claude_usage_counts(&usage),
        ClaudeUsageCounts { input: 48012, output: 800, total: 48812, cache_read: Some(45000), cache_write: Some(3000) }
    );
}

#[test]
fn test_claude_usage_counts_without_cache_fields_reports_unknown_cache() {
    let usage = serde_json::json!({ "input_tokens": 20, "output_tokens": 5 });
    assert_eq!(
        claude_usage_counts(&usage),
        ClaudeUsageCounts { input: 20, output: 5, total: 25, cache_read: None, cache_write: None }
    );
}

fn image_attachment(id: i64, content: &str) -> crate::db::conversation_db::MessageAttachment {
    use crate::db::conversation_db::{AttachmentType, MessageAttachment};
    MessageAttachment {
        id, message_id: 7, attachment_type: AttachmentType::Image,
        attachment_url: None, attachment_content: Some(content.into()), attachment_hash: None,
        use_vector: false, token_count: None,
    }
}

#[tokio::test]
async fn test_claude_images_encode_the_actual_format_and_preserve_order() {
    use super::claude_sdk::claude_user_input;
    use serde_json::json;

    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jvLkAAAAASUVORK5CYII=";
    let gif = "R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";
    let first = image_attachment(4, &format!("data:image/jpeg;base64,{png}"));
    let mut second = image_attachment(2, "");
    second.attachment_url = Some(format!("data:image/gif;base64,{gif}"));
    let frame = claude_user_input("question and notes".into(), vec![first, second], Some("existing".into())).await.unwrap();
    assert_eq!(frame, json!({"type":"user","session_id":"existing","message":{"role":"user","content":[
        {"type":"text","text":"question and notes"},
        {"type":"image","source":{"type":"base64","media_type":"image/png","data":png}},
        {"type":"image","source":{"type":"base64","media_type":"image/gif","data":gif}}
    ]}}));
    let image_only = claude_user_input(String::new(), vec![image_attachment(4, &format!("data:image/png;base64,{png}"))], None).await.unwrap();
    assert_eq!(image_only["message"]["content"].as_array().unwrap().len(), 1);
    assert_eq!(image_only["message"]["content"][0]["type"], "image");
}

#[tokio::test]
async fn test_claude_images_invalid_second_attachment_rejects_entire_message() {
    let valid = image_attachment(1, "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==");
    let invalid = image_attachment(2, "data:image/png;base64,invalid!sensitive");
    let error = super::claude_sdk::claude_user_input("question".into(), vec![valid, invalid], None).await.unwrap_err();
    assert!(error.contains("attachment_id=2") && error.contains("base64"));
    assert!(!error.contains("sensitive"));
}

/// Opt-in smoke test. Uses the local CLI account and sends only generated color swatches.
#[tokio::test]
#[ignore = "requires an authenticated Claude Code CLI and makes a model request"]
async fn test_claude_images_live_cli_reads_the_generated_frame() {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let dir = tempfile::tempdir().unwrap();
    let palette = [("red", [255, 0, 0]), ("blue", [0, 0, 255]), ("green", [0, 180, 0]), ("yellow", [255, 255, 0])];
    let random = uuid::Uuid::new_v4().as_u128();
    let first = (random % 4) as usize;
    let second = (first + 1 + (random / 4 % 3) as usize) % 4;
    let mut attachments = Vec::new();
    for (index, color) in [first, second].into_iter().enumerate() {
        let path = dir.path().join(format!("{index}.png"));
        image::RgbImage::from_pixel(128, 128, image::Rgb(palette[color].1)).save(&path).unwrap();
        let mut attachment = image_attachment(index as i64, "");
        attachment.attachment_url = Some(tauri::Url::from_file_path(&path).unwrap().into());
        attachments.push(attachment);
    }
    let frame = super::claude_sdk::claude_user_input(
        "Name the solid color of each image in order. Answer with exactly two lowercase English color names separated by one space.".into(),
        attachments, None,
    ).await.unwrap();
    let program = if cfg!(windows) { "claude.exe" } else { "claude" };
    let mut child = tokio::process::Command::new(program)
        .current_dir(dir.path())
        .args(["--print", "--verbose", "--input-format", "stream-json", "--output-format", "stream-json",
            "--include-partial-messages", "--permission-prompt-tool", "stdio", "--tools", "", "--strict-mcp-config",
            "--mcp-config", r#"{"mcpServers":{}}"#, "--setting-sources", "user", "--disable-slash-commands",
            "--no-session-persistence", "--max-turns", "1"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
    child.stdin.take().unwrap().write_all(format!("{frame}\n").as_bytes()).await.unwrap();
    let output = tokio::time::timeout(std::time::Duration::from_secs(90), child.wait_with_output()).await.unwrap().unwrap();
    let result = String::from_utf8_lossy(&output.stdout).lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|frame| frame["type"] == "result").expect("missing Claude result frame");
    assert!(output.status.success() && result["is_error"] != true, "Claude CLI returned {}", result["result"]);
    let expected = format!("{} {}", palette[first].0, palette[second].0);
    assert_eq!(result["result"].as_str().unwrap().trim(), expected);
}

#[tokio::test]
async fn test_claude_attachment_pdf_uses_native_document_and_preserves_prompt() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("工作材料.pdf");
    let bytes = b"%PDF-1.7\nexample";
    std::fs::write(&path, bytes).unwrap();
    let mut pdf = image_attachment(3, "");
    pdf.attachment_type = crate::db::conversation_db::AttachmentType::PDF;
    pdf.attachment_url = Some(path.display().to_string());
    let input = super::claude_sdk::claude_user_input("summary".into(), vec![pdf], None).await.unwrap();
    assert_eq!(input["message"]["content"], serde_json::json!([
        {"type":"text","text":"summary"},
        {"type":"document","source":{"media_type":"application/pdf","type":"base64","data":base64::engine::general_purpose::STANDARD.encode(bytes)}}
    ]));
}

use super::*;

#[test]
fn test_mobile_recovery_attachment_matches_inline_and_remote_references() {
    let mut attachment = MessageAttachment { id: 1, message_id: 1, attachment_type: AttachmentType::Text,
        attachment_url: None, attachment_content: Some(String::new()), attachment_hash: None, use_vector: false, token_count: None };
    assert!(!attachment_missing(&attachment));
    attachment.attachment_type = AttachmentType::Image;
    assert!(attachment_missing(&attachment));
    attachment.attachment_url = Some("https://example.com/image.png".to_string());
    assert!(!attachment_missing(&attachment));
    attachment.attachment_url = None;
    attachment.attachment_content = Some("data:image/png;base64,aGVsbG8=".to_string());
    assert!(!attachment_missing(&attachment));
    attachment.attachment_content = Some("invalid inline image".to_string());
    assert!(attachment_missing(&attachment));
}

#[test]
fn test_mobile_recovery_unknown_results_never_become_success() {
    assert!(is_unknown_execution("executing", false));
    assert!(!is_unknown_execution("executing", true));
    assert!(!is_unknown_execution("pending", false));
    assert!(!is_unknown_execution("success", false));
}

#[test]
fn test_mobile_draft_updates_clear_and_preserve_other_conversations() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_drafts(&conn).unwrap();
    write_draft(&conn, "12", "中文草稿", &[]).unwrap();
    write_draft(&conn, "13", "another", &[7]).unwrap();
    write_draft(&conn, "12", "edited", &[3, 5]).unwrap();
    assert_eq!(read_draft_row(&conn, "12").unwrap(), ("edited".to_string(), vec![3, 5]));
    write_draft(&conn, "12", "", &[]).unwrap();
    assert_eq!(read_draft_row(&conn, "12").unwrap(), (String::new(), Vec::new()));
    assert_eq!(read_draft_row(&conn, "13").unwrap(), ("another".to_string(), vec![7]));
}

#[test]
fn test_mobile_draft_keeps_attachment_only_rows() {
    let conn = Connection::open_in_memory().unwrap();
    initialize_drafts(&conn).unwrap();
    write_draft(&conn, "img", "", &[9]).unwrap();
    assert_eq!(read_draft_row(&conn, "img").unwrap(), (String::new(), vec![9]));
}

#[test]
fn test_safe_draft_attachment_requires_recoverable_content() {
    let missing = MessageAttachment {
        id: 1,
        message_id: -1,
        attachment_type: AttachmentType::Image,
        attachment_url: None,
        attachment_content: None,
        attachment_hash: None,
        use_vector: false,
        token_count: None,
    };
    assert!(safe_draft_attachment(missing).is_none());

    let ok = MessageAttachment {
        id: 2,
        message_id: -1,
        attachment_type: AttachmentType::Image,
        attachment_url: Some("photo.png".to_string()),
        attachment_content: Some("data:image/png;base64,aGVsbG8=".to_string()),
        attachment_hash: None,
        use_vector: false,
        token_count: None,
    };
    let restored = safe_draft_attachment(ok).unwrap();
    assert_eq!(restored.id, 2);
    assert_eq!(restored.name, "photo.png");
    assert_eq!(restored.attachment_type, AttachmentType::Image as i64);
    assert_eq!(restored.thumbnail.as_deref(), Some("data:image/png;base64,aGVsbG8="));
}

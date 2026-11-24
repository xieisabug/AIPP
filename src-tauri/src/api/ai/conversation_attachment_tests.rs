use super::*;

#[test]
fn test_attachment_cloud_request_embeds_short_text_once_and_pdf_as_binary() {
    let text = MessageAttachment {
        id: 4, message_id: 8, attachment_type: AttachmentType::Text, attachment_url: Some("a.txt".into()),
        attachment_content: Some("unique line".into()), attachment_hash: None, token_count: None, use_vector: false,
    };
    let pdf = MessageAttachment { attachment_type: AttachmentType::PDF, attachment_url: Some("/local/report.pdf".into()),
        attachment_content: Some("data:application/pdf;base64,JVBERi0=".into()), ..text.clone() };
    let body = r#"Review <fileattachment name="a.txt">unique line</fileattachment>"#;
    let message = build_user_message_with_attachments(body, &[text, pdf]);
    assert_eq!(message.content.parts().len(), 2);
    assert!(matches!(&message.content.parts()[0], ContentPart::Text(value) if value == body));
    assert!(matches!(&message.content.parts()[1], ContentPart::Binary(binary) if binary.content_type == "application/pdf"));
}

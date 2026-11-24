use super::*;
use std::io::{Cursor, Write};

fn file(kind: AttachmentType, path: &Path) -> MessageAttachment {
    MessageAttachment { id: 23, message_id: -1, attachment_type: kind,
        attachment_url: Some(path.to_string_lossy().into_owned()), attachment_content: None,
        attachment_hash: None, use_vector: false, token_count: None }
}

fn package(entries: &[(&str, &str)]) -> Cursor<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, xml) in entries {
        writer.start_file(*path, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
    }
    writer.finish().unwrap()
}

#[test]
fn test_attachment_word_paragraphs_and_slide_order() {
    let word = package(&[("word/document.xml", "<doc><p><t>A</t><t>B</t></p><p><t>C</t></p></doc>")]);
    assert_eq!(office_text::extract(word, AttachmentType::Word).unwrap(), "AB\nC\n");
    let slides = package(&[("ppt/slides/slide10.xml", "<p><t>Z</t></p>"),
        ("ppt/slides/slide2.xml", "<p><t>B</t></p>")]);
    assert_eq!(office_text::extract(slides, AttachmentType::PowerPoint).unwrap(), "[slide2.xml]\nB\n[slide10.xml]\nZ\n");
}

#[test]
fn test_attachment_xlsx_preserves_names_coordinates_and_values() {
    let workbook = package(&[
        ("xl/workbook.xml", r#"<workbook xmlns:r="urn:example"><sheet name="客户" r:id="a"/><sheet name="本月" r:id="b"/></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<rels><Relationship Id="a" Target="worksheets/sheet1.xml"/><Relationship Id="b" Target="/xl/worksheets/sheet2.xml"/></rels>"#),
        ("xl/sharedStrings.xml", "<sst><si><r><t>上</t></r><r><t>海</t></r></si></sst>"),
        ("xl/worksheets/sheet1.xml", r#"<sheet><c r="A1" t="s"><v>0</v></c></sheet>"#),
        ("xl/worksheets/sheet2.xml", r#"<sheet><c r="D17"><v>517</v></c><c r="E17" t="inlineStr"><is><t>已收</t></is></c></sheet>"#),
    ]);
    assert_eq!(office_text::extract(workbook, AttachmentType::Excel).unwrap(), "[客户]\nA1\t上海\n[本月]\nD17\t517\nE17\t已收\n");
}

#[tokio::test]
async fn test_attachment_pdf_native_bytes_or_tool_and_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("报表.pdf");
    let bytes = b"%PDF-1.7\nfixture";
    std::fs::write(&path, bytes).unwrap();
    let pdf = file(AttachmentType::PDF, &path);
    let prepared = prepare_chat_files(vec![pdf.clone()], true, false, false).await.unwrap();
    assert_eq!(prepared[0].attachment_content, Some(format!("data:application/pdf;base64,{}", STANDARD.encode(bytes))));
    let prepared = prepare_chat_files(vec![pdf.clone()], false, false, true).await.unwrap();
    assert!(prepared[0].attachment_content.as_ref().unwrap().contains("execute_bash"));
    let error = prepare_chat_files(vec![pdf], false, true, false).await.unwrap_err();
    assert!(error.to_string().contains("报表.pdf"));
}

#[tokio::test]
async fn test_attachment_long_text_uses_configured_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("server.log");
    std::fs::write(&path, "x".repeat(INLINE_TEXT_BYTES + 1)).unwrap();
    let input = file(AttachmentType::Text, &path);
    let result = prepare_chat_files(vec![input.clone()], false, true, false).await.unwrap();
    assert!(result[0].attachment_content.as_ref().unwrap().contains("read_file"));
    assert!(prepare_chat_files(result, false, false, false).await.is_err());
    assert!(prepare_chat_files(vec![input], false, false, false).await.is_err());
}

#[test]
fn test_attachment_received_bytes_become_local_and_missing_bytes_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = file(AttachmentType::Text, Path::new("aipp-remote:/notes.txt"));
    input.attachment_content = Some("测试内容".into());
    input.attachment_hash = Some(hex::encode(Sha256::digest("测试内容".as_bytes())));
    resolve_received_file(dir.path(), &mut input).unwrap();
    let saved = Path::new(input.attachment_url.as_ref().unwrap());
    assert!(saved.starts_with(dir.path()));
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "测试内容");
    let mut missing = file(AttachmentType::File, Path::new("aipp-remote:/sample.zip"));
    assert!(resolve_received_file(dir.path(), &mut missing).unwrap_err().to_string().contains("sample.zip"));
    missing.attachment_content = Some(format!("data:application/zip;base64,{}", STANDARD.encode([0, 255, 3])));
    resolve_received_file(dir.path(), &mut missing).unwrap();
    assert_eq!(std::fs::read(missing.attachment_url.unwrap()).unwrap(), [0, 255, 3]);
    for name in ["..", "../sample", "C:\\sample", "a/b"] {
        assert!(validate_attachment_name(name).is_err());
    }
}

#[test]
fn test_attachment_registered_path_preserves_binary_and_classifies_extensionless_paste() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("原始材料.zip");
    let bytes = [0x50, 0x4b, 0xff, 0, 0x42];
    std::fs::write(&path, bytes).unwrap();
    let (kind, text, hash) = inspect_attachment(&path, None).unwrap();
    assert_eq!(kind, AttachmentType::File);
    assert_eq!(text, None);
    assert_eq!(hash, hex::encode(Sha256::digest(bytes)));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let path = dir.path().join("clipboard");
    std::fs::write(&path, STANDARD.decode("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==").unwrap()).unwrap();
    let (kind, image, _) = inspect_attachment(&path, Some("image/gif")).unwrap();
    assert_eq!(kind, AttachmentType::Image);
    assert!(image.unwrap().starts_with("data:image/gif;base64,"));
}

#[test]
fn test_attachment_bytes_are_saved_intact_and_failed_registration_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = b"PK\x03\x04\0\xff";
    let saved = store_received_bytes(dir.path(), "资料.zip", bytes, |path| Ok(path.to_owned())).unwrap();
    assert_eq!(std::fs::read(&saved).unwrap(), bytes);
    let failed = store_received_bytes::<()>(dir.path(), "资料.zip", b"different", |_| Err(AppError::Anyhow("registration failed".into())));
    assert!(failed.is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(saved).unwrap(), bytes);
}

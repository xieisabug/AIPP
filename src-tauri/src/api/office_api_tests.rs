use super::*;
use std::io::Write;

fn build_minimal_ooxml_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn test_classify_office_extension_supported_and_legacy() {
    assert_eq!(classify_office_extension("report.DOCX").unwrap(), OfficeFormat::Docx);
    assert_eq!(classify_office_extension("/tmp/data.xlsx").unwrap(), OfficeFormat::Xlsx);
    assert_eq!(classify_office_extension("slides.pptx").unwrap(), OfficeFormat::Pptx);

    let legacy = classify_office_extension("legacy.doc").unwrap_err();
    assert_eq!(legacy.code, "unsupported_legacy_format");

    let unknown = classify_office_extension("notes.txt").unwrap_err();
    assert_eq!(unknown.code, "unsupported_format");
}

#[test]
fn test_validate_office_bytes_accepts_valid_docx_xlsx_pptx() {
    let docx = build_minimal_ooxml_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", b"<w:document/>"),
    ]);
    assert_eq!(
        validate_office_bytes("sample.docx", &docx, DESKTOP_MAX_OFFICE_BYTES).unwrap(),
        OfficeFormat::Docx
    );

    let xlsx = build_minimal_ooxml_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("xl/workbook.xml", b"<workbook/>"),
    ]);
    assert_eq!(
        validate_office_bytes("sheet.xlsx", &xlsx, DESKTOP_MAX_OFFICE_BYTES).unwrap(),
        OfficeFormat::Xlsx
    );

    let pptx = build_minimal_ooxml_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("ppt/presentation.xml", b"<p:presentation/>"),
    ]);
    assert_eq!(
        validate_office_bytes("deck.pptx", &pptx, DESKTOP_MAX_OFFICE_BYTES).unwrap(),
        OfficeFormat::Pptx
    );
}

#[test]
fn test_validate_office_bytes_rejects_encrypted_corrupt_and_oversized() {
    // 1. OLE2 encrypted container header
    let mut ole2 = OLE2_MAGIC.to_vec();
    ole2.extend_from_slice(&[0u8; 64]);
    let err = validate_office_bytes("encrypted.docx", &ole2, DESKTOP_MAX_OFFICE_BYTES).unwrap_err();
    assert_eq!(err.code, "password_encrypted");

    // 2. ZIP containing EncryptedPackage entry
    let enc_zip = build_minimal_ooxml_zip(&[
        ("EncryptionInfo", b"info"),
        ("EncryptedPackage", b"cipher"),
    ]);
    let err = validate_office_bytes("encrypted.xlsx", &enc_zip, DESKTOP_MAX_OFFICE_BYTES).unwrap_err();
    assert_eq!(err.code, "password_encrypted");

    // 3. Corrupt non-ZIP bytes
    let err = validate_office_bytes("corrupt.docx", b"NOT_A_ZIP_FILE", DESKTOP_MAX_OFFICE_BYTES)
        .unwrap_err();
    assert_eq!(err.code, "corrupt_file");

    // 4. Valid ZIP but missing word/document.xml
    let wrong_zip = build_minimal_ooxml_zip(&[("xl/workbook.xml", b"<workbook/>")]);
    let err = validate_office_bytes("mismatch.docx", &wrong_zip, DESKTOP_MAX_OFFICE_BYTES)
        .unwrap_err();
    assert_eq!(err.code, "corrupt_file");

    // 5. File exceeding size limit
    let valid_docx = build_minimal_ooxml_zip(&[("word/document.xml", b"<w:document/>")]);
    let err = validate_office_bytes("huge.docx", &valid_docx, 10).unwrap_err();
    assert_eq!(err.code, "file_too_large");
}

#[test]
fn test_sanitize_relative_viewer_path_blocks_traversal() {
    assert_eq!(
        sanitize_relative_viewer_path("/"),
        Some(PathBuf::from("index.html"))
    );
    assert_eq!(
        sanitize_relative_viewer_path("/assets/docx_edit_bg.wasm"),
        Some(PathBuf::from("assets").join("docx_edit_bg.wasm"))
    );
    assert_eq!(sanitize_relative_viewer_path("/../plugin.json"), None);
    assert_eq!(sanitize_relative_viewer_path("/assets/../../secret"), None);
}

#[test]
fn test_encode_office_file_response_frames_header_and_bytes() {
    let header = OfficeFileHeader {
        file_name: "季度报告.docx".to_string(),
        format: "docx".to_string(),
        mime: OfficeFormat::Docx.mime().to_string(),
        size_bytes: 3,
        sha256: "abc".to_string(),
    };
    let body = encode_office_file_response(&header, &[1, 2, 3]).unwrap();

    let header_len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let parsed: serde_json::Value = serde_json::from_slice(&body[4..4 + header_len]).unwrap();
    assert_eq!(parsed["fileName"], "季度报告.docx");
    assert_eq!(parsed["sizeBytes"], 3);
    assert_eq!(&body[4 + header_len..], &[1, 2, 3]);
}

#[test]
fn test_load_office_file_blocking_rejects_missing_and_reads_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = load_office_file_blocking(&dir.path().join("none.docx"), "none.docx").unwrap_err();
    assert_eq!(missing.code, "file_not_found");

    let docx = build_minimal_ooxml_zip(&[("word/document.xml", b"<w:document/>")]);
    let path = dir.path().join("ok.docx");
    fs::write(&path, &docx).unwrap();
    let before = fs::read(&path).unwrap();
    let body = load_office_file_blocking(&path, "ok.docx").unwrap();
    let header_len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    assert_eq!(&body[4 + header_len..], docx.as_slice());
    // 只读：源文件字节不变
    assert_eq!(fs::read(&path).unwrap(), before);
}

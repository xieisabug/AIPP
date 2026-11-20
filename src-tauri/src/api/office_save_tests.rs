use super::*;
use std::io::{Cursor, Write as _};

fn docx_bytes(marker: &str) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(format!("<w:document>{marker}</w:document>").as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf.into_inner()
}

fn leftover_temp_files(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[test]
fn test_copy_file_name_keeps_extension_and_numbers_conflicts() {
    assert_eq!(copy_file_name("季度报告.docx", COPY_SUFFIX, 1), "季度报告（副本）.docx");
    assert_eq!(copy_file_name("季度报告.docx", COPY_SUFFIX, 3), "季度报告（副本 3）.docx");
    assert_eq!(copy_file_name("data.v2.xlsx", AI_SUFFIX, 1), "data.v2（AI 修改）.xlsx");

    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a（副本）.pptx"), b"x").unwrap();
    fs::write(dir.path().join("a（副本 2）.pptx"), b"x").unwrap();
    assert_eq!(
        unique_copy_path(dir.path(), "a.pptx", COPY_SUFFIX).file_name().unwrap().to_str().unwrap(),
        "a（副本 3）.pptx"
    );
}

#[test]
fn test_sanitize_output_name_strips_paths_and_forces_format() {
    let docx = OfficeFormat::Docx;
    assert_eq!(sanitize_output_name(None, "报告.docx", docx), "报告（AI 修改）.docx");
    assert_eq!(sanitize_output_name(Some("  "), "报告.docx", docx), "报告（AI 修改）.docx");
    assert_eq!(sanitize_output_name(Some("../../etc/改好的.docx"), "报告.docx", docx), "改好的.docx");
    assert_eq!(sanitize_output_name(Some("C:\\x\\新版"), "报告.docx", docx), "新版.docx");
    assert_eq!(sanitize_output_name(Some("a:b?.xlsx"), "报告.docx", docx), "a_b_.xlsx.docx");
}

#[test]
fn test_safe_write_refuses_to_overwrite_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.docx");
    fs::write(&target, b"original").unwrap();
    let e = write_office_file_safely(&target, &docx_bytes("new"), false).unwrap_err();
    assert_eq!(e.code, "target_exists");
    assert_eq!(fs::read(&target).unwrap(), b"original");
    assert!(leftover_temp_files(dir.path()).is_empty());
}

#[test]
fn test_safe_write_creates_new_file_and_overwrites_only_when_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.docx");
    let first = docx_bytes("first");
    write_office_file_safely(&target, &first, false).unwrap();
    assert_eq!(fs::read(&target).unwrap(), first);

    let second = docx_bytes("second");
    write_office_file_safely(&target, &second, true).unwrap();
    assert_eq!(fs::read(&target).unwrap(), second);
    assert!(leftover_temp_files(dir.path()).is_empty());
}

#[test]
fn test_safe_write_reports_missing_directory_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("missing").join("out.docx");
    let e = write_office_file_safely(&target, &docx_bytes("x"), false).unwrap_err();
    assert_eq!(e.code, "target_dir_missing");
    assert!(!target.exists());
}

#[test]
fn test_safe_write_failure_keeps_existing_file_intact() {
    // 目标是只读文件：覆盖失败时原内容不变、不留临时文件（Windows 上 rename 覆盖只读文件会失败）
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("locked.docx");
    let original = docx_bytes("original");
    fs::write(&target, &original).unwrap();
    let mut perms = fs::metadata(&target).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&target, perms.clone()).unwrap();

    let result = write_office_file_safely(&target, &docx_bytes("new"), true);
    if cfg!(windows) {
        assert!(result.is_err(), "overwriting a read-only file must fail on Windows");
        assert_eq!(fs::read(&target).unwrap(), original);
    }
    assert!(leftover_temp_files(dir.path()).is_empty());
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&target, perms).unwrap();
}

#[test]
fn test_save_bytes_rejects_format_mismatch_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.xlsx");
    let e = save_bytes(&target, &docx_bytes("x"), false, OfficeFormat::Docx).unwrap_err();
    assert_eq!(e.code, "invalid_target");
    assert!(!target.exists());
}

#[test]
fn test_decode_framed_request_round_trip_and_bounds() {
    let header = br#"{"targetPath":"C:/a.docx","overwrite":true}"#;
    let mut body = (header.len() as u32).to_le_bytes().to_vec();
    body.extend_from_slice(header);
    body.extend_from_slice(b"PK..");
    let (json, bytes) = decode_framed_request(&body).unwrap();
    assert_eq!(json["targetPath"], "C:/a.docx");
    assert_eq!(bytes, b"PK..");

    assert_eq!(decode_framed_request(&[1, 0]).unwrap_err().code, "invalid_request");
    let mut bad = 999u32.to_le_bytes().to_vec();
    bad.extend_from_slice(b"{}");
    assert_eq!(decode_framed_request(&bad).unwrap_err().code, "invalid_request");
}

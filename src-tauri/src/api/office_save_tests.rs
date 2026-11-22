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
    let e = save_bytes(&target, &docx_bytes("x"), false, OfficeFormat::Docx, None).unwrap_err();
    assert_eq!(e.code, "invalid_target");
    assert!(!target.exists());
}

#[test]
fn test_write_edited_copy_protects_original_and_allows_other_file_after_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("报告.docx");
    let original = docx_bytes("original");
    let edited = docx_bytes("edited");
    fs::write(&source, &original).unwrap();
    let original_hash = sha256_hex(&original);

    for overwrite in [false, true] {
        let result = write_edited_copy(&ProtectedCopySource::from(source.clone()), &source, &edited, overwrite).unwrap_err();
        assert_eq!(result.code, "protected_source");
    }

    let copy = dir.path().join("报告（副本）.docx");
    write_edited_copy(&ProtectedCopySource::from(source.clone()), &copy, &edited, false).unwrap();
    assert_eq!(fs::read(&copy).unwrap(), edited);

    // Distinct inode, identical basename and bytes: this is still another file.
    let other_dir = dir.path().join("other");
    fs::create_dir(&other_dir).unwrap();
    let other = other_dir.join("报告.docx");
    fs::write(&other, &original).unwrap();
    let result = write_edited_copy(&ProtectedCopySource::from(source.clone()), &other, &edited, false).unwrap_err();
    assert_eq!(result.code, "target_exists");
    assert_eq!(fs::read(&other).unwrap(), original);
    write_edited_copy(&ProtectedCopySource::from(source.clone()), &other, &edited, true).unwrap();

    assert_eq!(fs::read(&other).unwrap(), edited);
    assert_eq!(sha256_hex(&fs::read(&source).unwrap()), original_hash);
    assert!(leftover_temp_files(dir.path()).is_empty());
    assert!(leftover_temp_files(&other_dir).is_empty());
}

#[test]
fn test_write_edited_copy_blocks_a_target_that_became_a_source_alias_after_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.docx");
    let target = dir.path().join("target.docx");
    let original = docx_bytes("original");
    let edited = docx_bytes("edited");
    fs::write(&source, &original).unwrap();
    fs::write(&target, docx_bytes("unrelated")).unwrap();

    assert_eq!(write_edited_copy(&ProtectedCopySource::from(source.clone()), &target, &edited, false).unwrap_err().code, "target_exists");
    fs::remove_file(&target).unwrap();
    fs::hard_link(&source, &target).unwrap();
    assert_eq!(write_edited_copy(&ProtectedCopySource::from(source.clone()), &target, &edited, true).unwrap_err().code, "protected_source");
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(fs::read(&target).unwrap(), original);
    assert!(leftover_temp_files(dir.path()).is_empty());
}

#[test]
#[cfg(unix)]
fn test_write_edited_copy_refuses_symlink_alias_of_source_directory() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let alias = dir.path().join("alias");
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let source = real.join("source.docx");
    let original = docx_bytes("original");
    fs::write(&source, &original).unwrap();
    let edited = docx_bytes("edited");
    let target = alias.join("source.docx");
    assert_eq!(write_edited_copy(&ProtectedCopySource::from(source.clone()), &target, &edited, true).unwrap_err().code, "protected_source");
    assert_eq!(fs::read(&source).unwrap(), original);

    // A missing leaf is still the original save location.
    fs::remove_file(&source).unwrap();
    assert_eq!(write_edited_copy(&ProtectedCopySource::from(source.clone()), &target, &edited, false).unwrap_err().code, "protected_source");
    assert!(!source.exists());
    assert!(leftover_temp_files(&real).is_empty());
}

#[test]
fn test_bound_relay_editor_can_save_without_live_relay_and_protects_initial_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.docx");
    let alias = dir.path().join("alias.docx");
    let original = docx_bytes("original");
    let edited = docx_bytes("edited");
    fs::write(&path, &original).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    let origins = OfficeEditCopyOrigins::default();
    let relay = "already-expired-relay";
    let token = origins.remember("chat", relay, path.clone(), fs::File::open(&path).unwrap()).unwrap();

    // Loading an editor earlier is required. The ordinary relay token is not a save handle,
    // and another host window can't reuse the handle.
    assert_eq!(origins.get("chat", relay, None).unwrap_err().code, "file_not_found");
    assert_eq!(origins.get("other", relay, Some(&token)).unwrap_err().code, "file_not_found");
    assert_eq!(origins.get("chat", "unopened-relay", Some(&token)).unwrap_err().code, "file_not_found");

    // Neither the protection lookup nor the save needs the live read relay.
    let source = origins.get("chat", relay, Some(&token)).unwrap();
    fs::remove_file(&path).unwrap();
    let replacement = docx_bytes("external replacement");
    fs::write(&path, &replacement).unwrap();
    assert_eq!(write_edited_copy(&source, &path, &edited, true).unwrap_err().code, "protected_source");
    assert_eq!(write_edited_copy(&source, &alias, &edited, true).unwrap_err().code, "protected_source");
    let copy = dir.path().join("new.docx");
    write_edited_copy(&source, &copy, &edited, false).unwrap();
    assert_eq!(fs::read(&path).unwrap(), replacement);
    assert_eq!(fs::read(&alias).unwrap(), original);
    assert_eq!(fs::read(&copy).unwrap(), edited);
    origins.release("other", &token);
    assert!(origins.get("chat", relay, Some(&token)).is_ok());
    origins.release("chat", &token);
    assert!(origins.get("chat", relay, Some(&token)).is_err());
}

#[test]
fn test_edit_origin_registry_is_bounded_and_clears_destroyed_windows() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.docx");
    fs::write(&source, docx_bytes("original")).unwrap();
    let origins = OfficeEditCopyOrigins::default();
    let evicted = origins.remember("chat", "relay", source.clone(), fs::File::open(&source).unwrap()).unwrap();
    for _ in 0..EDIT_COPY_ORIGIN_LIMIT - 1 {
        origins.remember("chat", "relay", source.clone(), fs::File::open(&source).unwrap()).unwrap();
    }
    let active = origins.remember("other", "relay", source.clone(), fs::File::open(&source).unwrap()).unwrap();
    let same_window = origins.remember("chat", "relay", source.clone(), fs::File::open(&source).unwrap()).unwrap();
    assert!(origins.get("chat", "relay", Some(&evicted)).is_err());
    origins.release_window("chat");
    assert!(origins.get("chat", "relay", Some(&same_window)).is_err());
    assert!(origins.get("other", "relay", Some(&active)).is_ok());
}

#[test]
fn test_edited_copy_requires_source_in_framed_header() {
    let header = serde_json::json!({"targetPath": "copy.docx", "overwrite": true});
    assert!(serde_json::from_value::<WriteCopyHeader>(header.clone()).is_err());
    let mut with_source = header;
    with_source["source"] = serde_json::json!({"attachmentId": 8});
    with_source["copyOriginToken"] = serde_json::json!("save-only");
    let parsed = serde_json::from_value::<WriteCopyHeader>(with_source).unwrap();
    assert_eq!(parsed.source.attachment_id, Some(8));
    assert_eq!(parsed.copy_origin_token.as_deref(), Some("save-only"));
    assert!(parsed.overwrite);
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

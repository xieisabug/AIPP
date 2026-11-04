use super::validate_archive_entry;

#[test]
fn test_archive_paths_reject_traversal_and_platform_prefixes() {
    for name in ["../SKILL.md", "a/../../secret", "/etc/passwd", "C:/secret", "a\\..\\secret", "a\0b"] {
        assert!(validate_archive_entry(name, None, 10).is_err(), "{name:?}");
    }
    assert!(validate_archive_entry("中文技能/references/example.md", Some(0o100644), 50 * 1024 * 1024).is_ok());
}

#[test]
fn test_archive_rejects_symlinks_and_expansion_limit() {
    assert!(validate_archive_entry("link", Some(0o120777), 10).is_err());
    assert!(validate_archive_entry("skill/SKILL.md", None, 50 * 1024 * 1024 + 1).is_err());
}

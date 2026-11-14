use super::*;

#[test]
fn test_manual_compact_detects_bare_command() {
    assert!(is_manual_compact_prompt("/compact"));
    assert!(is_manual_compact_prompt("  /compact\n"));
}

#[test]
fn test_manual_compact_ignores_other_text() {
    assert!(!is_manual_compact_prompt("/compact focus on tests"));
    assert!(!is_manual_compact_prompt("please /compact"));
    assert!(!is_manual_compact_prompt("/compactx"));
    assert!(!is_manual_compact_prompt(""));
}

#[test]
fn test_manual_compact_done_text_with_and_without_tokens() {
    assert_eq!(compact_done_text(Some(12345)), "上下文已压缩（压缩前约 12345 tokens）。");
    assert_eq!(compact_done_text(Some(0)), "上下文已压缩。");
    assert_eq!(compact_done_text(None), "上下文已压缩。");
}

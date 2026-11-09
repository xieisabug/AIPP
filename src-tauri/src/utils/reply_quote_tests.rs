use super::reply_quote::{matches, split, strip};

#[test]
fn test_reply_quote_split_marker_line() {
    assert_eq!(split("[[QUOTE:明天几点开会]]\n下午三点。"), Some(("明天几点开会", "下午三点。")));
    assert_eq!(split("  [[QUOTE: 预算 ]] 先按十万算。"), Some(("预算", "先按十万算。")));
    assert_eq!(split("[[QUOTE:方案 A]]\n\n```rust\n  let x = 1;\n```"), Some(("方案 A", "```rust\n  let x = 1;\n```")));
}

#[test]
fn test_reply_quote_split_rejects_invalid_markers() {
    assert_eq!(split("下午三点。"), None);
    assert_eq!(split("我觉得 [[QUOTE:预算]] 可以"), None, "标记不在开头");
    assert_eq!(split("[[QUOTE:]]\n正文"), None, "片段为空");
    assert_eq!(split("[[QUOTE:预算]]\n  "), None, "没有正文");
    assert_eq!(split("[[QUOTE:预算\n还没结束"), None, "没有结束符");
    assert_eq!(split(&format!("[[QUOTE:{}]]\n正文", "长".repeat(201))), None, "片段过长");
    assert_eq!(strip("[[QUOTE:预算]]\n先按十万算。"), "先按十万算。");
    assert_eq!(strip("[[QUOTE:预算]]"), "[[QUOTE:预算]]", "没有正文时原样保留");
}

#[test]
fn test_reply_quote_matches_ignores_punctuation_and_case() {
    let text = "我们下周要不要把 **API 网关** 换成自建的？预算大概十万。";
    assert!(matches("API网关换成自建的", text));
    assert!(matches("「api 网关」换成自建的?", text));
    assert!(matches("预算大概十万", text));
    assert!(!matches("预算大概二十万", text));
    assert!(!matches("？", text), "去掉标点后为空");
    assert!(!matches("预", text), "片段太短");
}

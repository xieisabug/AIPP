//! 助手的引用回复：回复第一行写 `[[QUOTE:原话片段]]`，表示这条回复是在回应包含这段原话的那条消息。
//!
//! 模型看到的历史消息不带编号，所以按原话片段定位：从近到远找第一条包含该片段的消息。
//! 比较前去掉空白、标点和 Markdown 符号并统一大小写，模型抄写时的细微差别不影响定位。
//! 找不到对应消息时只去掉标记，不显示引用（不展示一段没人说过的“原话”）。
//!
//! 与 `src/components/butler/butlerReplyQuote.ts` 保持一致。

pub const OPEN: &str = "[[QUOTE:";
const CLOSE: &str = "]]";
/// 片段至少要有这么多个文字或数字，太短的片段容易配错。
const MIN_FRAGMENT_CHARS: usize = 2;
const MAX_FRAGMENT_CHARS: usize = 200;

/// 拆出回复开头的引用标记，返回（原话片段，正文）。
/// 标记必须在回复最前面且独占一段；没有标记、片段为空或过长、标记后没有正文时返回 None。
pub fn split(reply: &str) -> Option<(&str, &str)> {
    let rest = reply.trim_start().strip_prefix(OPEN)?;
    let end = rest.find(CLOSE)?;
    let fragment = rest[..end].trim();
    let body = rest[end + CLOSE.len()..].trim_start_matches([' ', '\t']).trim_start_matches(['\r', '\n']);
    if fragment.is_empty() || fragment.contains('\n') || fragment.chars().count() > MAX_FRAGMENT_CHARS || body.trim().is_empty() {
        return None;
    }
    Some((fragment, body))
}

/// 去掉回复开头的引用标记，只留正文（用于不显示引用的场合，如联系人摘要、飞书转发）。
pub fn strip(reply: &str) -> &str {
    split(reply).map(|(_, body)| body).unwrap_or(reply)
}

fn normalize(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// `text` 是否包含引用片段。
pub fn matches(fragment: &str, text: &str) -> bool {
    let needle = normalize(fragment);
    needle.chars().count() >= MIN_FRAGMENT_CHARS && normalize(text).contains(&needle)
}

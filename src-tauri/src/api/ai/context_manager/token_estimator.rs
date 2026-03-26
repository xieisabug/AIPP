/// Estimate token count for a message.
///
/// Uses the DB-reported `input_token_count` when available (from previous API
/// responses), and falls back to a content-based heuristic that is aware of
/// CJK character density.
pub fn estimate_message_tokens(content: &str, db_input_tokens: i32) -> usize {
    if db_input_tokens > 0 {
        return db_input_tokens as usize;
    }
    estimate_by_content(content)
}

/// Heuristic token estimation from raw text.
///
/// - ASCII-heavy text ≈ 4 characters per token
/// - CJK text ≈ 1.5 characters per token
/// - Mixed content uses a weighted average
pub fn estimate_by_content(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }

    let mut cjk_chars: usize = 0;
    let mut ascii_chars: usize = 0;

    for ch in content.chars() {
        if is_cjk(ch) {
            cjk_chars += 1;
        } else {
            ascii_chars += 1;
        }
    }

    // CJK: ~1.5 chars/token → multiply by 2/3
    // ASCII: ~4 chars/token → multiply by 1/4
    let cjk_tokens = (cjk_chars * 2 + 2) / 3; // ceiling division by 1.5
    let ascii_tokens = (ascii_chars + 3) / 4; // ceiling division by 4

    // Add a small overhead per message for role/metadata tokens (~4 tokens)
    cjk_tokens + ascii_tokens + 4
}

/// Check if a character falls into CJK Unicode ranges.
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        | '\u{3000}'..='\u{303F}' // CJK Symbols and Punctuation
        | '\u{FF00}'..='\u{FFEF}' // Halfwidth and Fullwidth Forms
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
    )
}

/// Estimate total tokens for a list of messages.
#[allow(dead_code)]
pub fn estimate_total_tokens(
    messages: &[(String, String, Vec<crate::db::conversation_db::MessageAttachment>)],
    db_token_counts: &[i32],
) -> usize {
    messages
        .iter()
        .zip(db_token_counts.iter().chain(std::iter::repeat(&0)))
        .map(|((_, content, attachments), &db_tokens)| {
            let content_tokens = estimate_message_tokens(content, db_tokens);
            let attachment_tokens: usize = attachments
                .iter()
                .map(|a| {
                    a.attachment_content.as_deref().map(|c| estimate_by_content(c)).unwrap_or(0)
                })
                .sum();
            content_tokens + attachment_tokens
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_empty() {
        assert_eq!(estimate_by_content(""), 0);
    }

    #[test]
    fn estimate_pure_ascii() {
        // 100 ASCII chars → ~25 tokens + 4 overhead = 29
        let text = "a".repeat(100);
        let est = estimate_by_content(&text);
        assert!(est >= 25 && est <= 35, "got {est}");
    }

    #[test]
    fn estimate_pure_cjk() {
        // 100 CJK chars → ~67 tokens + 4 overhead = 71
        let text = "你".repeat(100);
        let est = estimate_by_content(&text);
        assert!(est >= 60 && est <= 80, "got {est}");
    }

    #[test]
    fn estimate_mixed() {
        // 50 ASCII + 50 CJK
        let text = format!("{}{}", "a".repeat(50), "你".repeat(50));
        let est = estimate_by_content(&text);
        // ~13 ASCII tokens + ~34 CJK tokens + 4 = ~51
        assert!(est >= 40 && est <= 60, "got {est}");
    }

    #[test]
    fn db_token_takes_priority() {
        assert_eq!(estimate_message_tokens("hello world", 42), 42);
    }

    #[test]
    fn db_token_zero_falls_back() {
        let est = estimate_message_tokens("hello world", 0);
        assert!(est > 0);
    }
}

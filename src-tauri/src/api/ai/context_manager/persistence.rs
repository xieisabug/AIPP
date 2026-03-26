use crate::db::conversation_db::{ConversationDatabase, Message};
use crate::errors::AppError;
use chrono::Utc;
use tracing::{debug, info};

/// Marker prefix for compaction summary content in the DB.
pub const CONTEXT_SUMMARY_MARKER: &str = "<context_summary>";

/// Insert a compaction summary message into the conversation.
///
/// The summary replaces all messages between `first_compacted_id` and
/// `last_compacted_id` (inclusive). The marker comment embedded in the
/// content allows `is_compaction_summary` to recognize this message on
/// subsequent loads.
pub fn store_compaction_summary(
    db: &ConversationDatabase,
    conversation_id: i64,
    summary_content: &str,
    first_compacted_id: i64,
    last_compacted_id: i64,
) -> Result<i64, AppError> {
    // Embed the compacted range as an HTML comment so it can be parsed later
    let content = format!(
        "<!-- compacted_range: {}..{} -->\n{}",
        first_compacted_id, last_compacted_id, summary_content
    );

    info!(
        conversation_id,
        first_compacted_id,
        last_compacted_id,
        summary_len = content.len(),
        "storing compaction summary"
    );

    let message = Message {
        id: 0, // auto-increment
        parent_id: None,
        conversation_id,
        message_type: "system".to_string(),
        content,
        llm_model_id: None,
        llm_model_name: None,
        created_time: Utc::now(),
        start_time: None,
        finish_time: None,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: None,
        parent_group_id: None,
        tool_calls_json: None,
        first_token_time: None,
        ttft_ms: None,
    };

    let created = db
        .message_repo()
        .map_err(|e| AppError::UnknownError(format!("DB error: {}", e)))?
        .create_without_touch_conversation(&message)
        .map_err(|e| {
            AppError::UnknownError(format!("Failed to store compaction summary: {}", e))
        })?;

    debug!(summary_message_id = created.id, "compaction summary stored");
    Ok(created.id)
}

/// Check if a message is a compaction summary by inspecting its content.
pub fn is_compaction_summary(content: &str) -> bool {
    content.contains(CONTEXT_SUMMARY_MARKER) && content.contains("<!-- compacted_range:")
}

/// Parse the compacted message ID range from a summary message.
/// Returns `Some((first_id, last_id))` if the marker is found.
pub fn parse_compacted_range(content: &str) -> Option<(i64, i64)> {
    let marker = "<!-- compacted_range: ";
    let start = content.find(marker)?;
    let rest = &content[start + marker.len()..];
    let end = rest.find(" -->")?;
    let range_str = &rest[..end];
    let mut parts = range_str.split("..");
    let first: i64 = parts.next()?.parse().ok()?;
    let last: i64 = parts.next()?.parse().ok()?;
    Some((first, last))
}

/// Filter a loaded message list to respect compaction summaries.
///
/// If a compaction summary is found, messages whose IDs fall within the
/// compacted range are excluded (the summary replaces them). Messages
/// outside the range (system prompt, tail, the summary itself) are kept.
///
/// `messages_with_ids` pairs each message with its DB id. Messages with
/// `id == 0` (e.g., dynamically injected) are always kept.
pub fn apply_compaction_filter(
    messages_with_ids: &[(i64, String, String)], // (id, msg_type, content)
) -> Vec<usize> {
    // Find all compaction summaries and collect their ranges
    let mut compacted_ranges: Vec<(i64, i64)> = Vec::new();
    for (_id, _msg_type, content) in messages_with_ids {
        if is_compaction_summary(content) {
            if let Some(range) = parse_compacted_range(content) {
                compacted_ranges.push(range);
            }
        }
    }

    if compacted_ranges.is_empty() {
        return (0..messages_with_ids.len()).collect();
    }

    // Keep messages whose IDs are NOT in any compacted range
    messages_with_ids
        .iter()
        .enumerate()
        .filter(|(_idx, (id, _, _))| {
            if *id == 0 {
                return true; // dynamic messages always kept
            }
            !compacted_ranges.iter().any(|(first, last)| *id >= *first && *id <= *last)
        })
        .map(|(idx, _)| idx)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_compaction_summary_positive() {
        let content = "<!-- compacted_range: 5..20 -->\n<context_summary>\n## 用户目标\ntest\n</context_summary>";
        assert!(is_compaction_summary(content));
    }

    #[test]
    fn is_compaction_summary_negative() {
        assert!(!is_compaction_summary("Hello world"));
        assert!(!is_compaction_summary("<context_summary>without range</context_summary>"));
    }

    #[test]
    fn parse_range_valid() {
        let content = "<!-- compacted_range: 100..500 -->\n<context_summary>...</context_summary>";
        assert_eq!(parse_compacted_range(content), Some((100, 500)));
    }

    #[test]
    fn parse_range_invalid() {
        assert_eq!(parse_compacted_range("no range here"), None);
        assert_eq!(parse_compacted_range("<!-- compacted_range: abc..def -->"), None);
    }

    #[test]
    fn filter_removes_compacted_messages() {
        let summary = "<!-- compacted_range: 2..4 -->\n<context_summary>summary</context_summary>";
        let messages = vec![
            (1, "system".into(), "system prompt".into()),
            (2, "user".into(), "old user msg".into()), // compacted
            (3, "response".into(), "old response".into()), // compacted
            (4, "user".into(), "old user msg 2".into()), // compacted
            (5, "system".into(), summary.to_string()), // summary itself
            (6, "user".into(), "recent msg".into()),
            (7, "response".into(), "recent response".into()),
        ];
        let kept = apply_compaction_filter(&messages);
        assert_eq!(kept, vec![0, 4, 5, 6]); // indices: system(0), summary(4), recent(5,6)
    }

    #[test]
    fn filter_no_summary_keeps_all() {
        let messages =
            vec![(1, "system".into(), "prompt".into()), (2, "user".into(), "hello".into())];
        let kept = apply_compaction_filter(&messages);
        assert_eq!(kept, vec![0, 1]);
    }

    #[test]
    fn filter_dynamic_messages_always_kept() {
        let summary = "<!-- compacted_range: 1..10 -->\n<context_summary>s</context_summary>";
        let messages = vec![
            (0, "system".into(), "dynamic inject".into()), // id=0, always kept
            (5, "user".into(), "compacted".into()),        // in range
            (11, "system".into(), summary.to_string()),
        ];
        let kept = apply_compaction_filter(&messages);
        assert_eq!(kept, vec![0, 2]); // dynamic(0) + summary(2)
    }
}

use crate::db::conversation_db::ConversationDatabase;
use crate::errors::AppError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
struct CheckpointSource {
    ids: Vec<i64>,
    digest: String,
}

fn source_digest<'a>(messages: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut hash = Sha256::new();
    for (kind, text) in messages {
        for value in [kind, text] {
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}

pub fn checkpoint_text(content: &str) -> &str {
    content.split_once(" -->\n").map(|(_, text)| text).unwrap_or(content)
}

pub trait CheckpointStore: Sync {
    fn save(&self, conversation_id: i64, header: &str, content: &str) -> Result<String, AppError>;
}

impl CheckpointStore for ConversationDatabase {
    fn save(&self, conversation_id: i64, header: &str, content: &str) -> Result<String, AppError> {
        Ok(self.message_repo()?.get_or_create_checkpoint(conversation_id, header, content)?)
    }
}

/// Marker prefix for compaction summary content in the DB.
pub const CONTEXT_SUMMARY_MARKER: &str = "<context_summary>";

/// Insert a compaction summary message into the conversation.
///
/// Keep original messages intact. Only the model request substitutes the recorded slice.
pub fn store_compaction_summary(
    store: &dyn CheckpointStore,
    conversation_id: i64,
    summary_content: &str,
    source_ids: &[i64],
    body: &[super::MessageTuple],
) -> Result<String, AppError> {
    let content = checkpoint_content(summary_content, source_ids, body);
    let (header, _) = content.split_once('\n').expect("checkpoint header always ends with a newline");
    let stored = store.save(conversation_id, header, &content)?;
    Ok(checkpoint_text(&stored).to_owned())
}

fn checkpoint_content(summary_content: &str, source_ids: &[i64], body: &[super::MessageTuple]) -> String {
    let source = CheckpointSource {
        ids: source_ids.to_vec(),
        digest: source_digest(body.iter().map(|(kind, text, _)| (kind.as_str(), text.as_str()))),
    };
    format!("<!-- compacted_ids: {} -->\n{}", serde_json::json!(source), summary_content)
}

/// Check if a message is a compaction summary by inspecting its content.
pub fn is_compaction_summary(content: &str) -> bool {
    content.contains(CONTEXT_SUMMARY_MARKER) && (content.starts_with("<!-- compacted_range:") || content.starts_with("<!-- compacted_ids:"))
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
    let mut kept: Vec<usize> = Vec::new();
    for (index, (_, kind, content)) in messages_with_ids.iter().enumerate() {
        if kind != "system" || !is_compaction_summary(content) {
            kept.push(index);
            continue;
        }
        let span = if let Some(encoded) = content.strip_prefix("<!-- compacted_ids: ")
            .and_then(|text| text.split_once(" -->").map(|(ids, _)| ids))
        {
            let Ok(source) = serde_json::from_str::<CheckpointSource>(encoded) else { continue };
            if source.ids.is_empty() { continue; }
            kept.windows(source.ids.len()).position(|slice| {
                slice.iter().zip(&source.ids).all(|(&at, id)| messages_with_ids[at].0 == *id)
                    && source_digest(slice.iter().map(|&at| {
                        let (_, kind, text) = &messages_with_ids[at];
                        let text = if kind == "system" && is_compaction_summary(text) {
                            checkpoint_text(text)
                        } else { text };
                        (kind.as_str(), text)
                    })) == source.digest
            }).map(|start| (start, start + source.ids.len()))
        } else {
            // Older installations stored an inclusive range rather than individual IDs.
            parse_compacted_range(content).and_then(|(first, last)| {
                let covered = |at: &usize| {
                    let id = messages_with_ids[*at].0;
                    id > 0 && (first..=last).contains(&id)
                };
                let start = kept.iter().position(covered)?;
                let count = kept[start..].iter().take_while(|idx| covered(idx)).count();
                Some((start, start + count))
            })
        };
        if let Some((start, end)) = span {
            kept.splice(start..end, [index]);
        }
    }
    kept
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

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod checkpoint_tests;

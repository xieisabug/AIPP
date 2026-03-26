pub mod budget;
#[allow(dead_code)]
pub mod persistence;
pub mod summarizer;
pub mod token_estimator;

use crate::db::conversation_db::{ConversationDatabase, MessageAttachment};
use budget::ContextBudget;
use genai::Client;
use token_estimator::{estimate_by_content, estimate_message_tokens};
use tracing::{debug, info, warn};

/// A message in the context management pipeline.
/// Mirrors the tuple format used by build_message_list_from_db:
/// (message_type, content, attachments)
pub type MessageTuple = (String, String, Vec<MessageAttachment>);

/// Result of fitting messages to a context budget.
pub struct FitResult {
    /// The (possibly compacted) message list, ready for chat request assembly.
    pub messages: Vec<MessageTuple>,
    /// Whether compaction was performed.
    pub compacted: bool,
    /// Estimated total tokens of the output message list.
    pub estimated_tokens: usize,
}

/// Optional context for LLM-based compaction. When provided and the message
/// list exceeds the budget threshold, the body segment will be summarized
/// via an LLM call.
pub struct CompactionContext<'a> {
    pub client: &'a Client,
    pub model_name: &'a str,
    pub conversation_id: i64,
    pub conversation_db: &'a ConversationDatabase,
    pub is_butler: bool,
    /// DB message IDs aligned by index with the message list.
    /// Used for persistence (recording which messages were compacted).
    pub message_ids: Vec<i64>,
}

/// Fit a message list into the given context budget (synchronous, estimation only).
///
/// Estimates tokens and logs warnings if over budget. Returns messages
/// unmodified. Use `fit_to_budget_with_compaction` for active LLM compression.
pub fn fit_to_budget(
    messages: Vec<MessageTuple>,
    budget: &ContextBudget,
    db_token_counts: &[i32],
) -> FitResult {
    if !budget.enabled {
        return FitResult { estimated_tokens: 0, compacted: false, messages };
    }

    let estimated_tokens = estimate_total(messages.as_slice(), db_token_counts);
    let trigger = budget.compaction_trigger();
    let effective_limit = budget.effective_input_limit();

    debug!(
        estimated_tokens,
        trigger,
        effective_limit,
        message_count = messages.len(),
        "context budget check"
    );

    if estimated_tokens > effective_limit {
        warn!(
            estimated_tokens,
            effective_limit,
            message_count = messages.len(),
            "context EXCEEDS effective limit — LLM call may fail or degrade"
        );
    } else if estimated_tokens > trigger {
        info!(
            estimated_tokens,
            trigger,
            message_count = messages.len(),
            "context reached compaction threshold — compaction recommended"
        );
    }

    FitResult { estimated_tokens, compacted: false, messages }
}

/// Fit a message list into the given context budget, performing LLM-based
/// compaction when the threshold is exceeded.
///
/// The algorithm splits messages into three segments:
/// - **Head**: the system prompt (first message)
/// - **Body**: everything between Head and Tail — candidate for summarization
/// - **Tail**: the most recent `tail_preserve_count` messages — kept verbatim
///
/// When over budget, Body is summarized by the LLM and replaced with a single
/// summary message. The summary is also persisted to the DB so subsequent
/// loads can skip the compacted messages.
pub async fn fit_to_budget_with_compaction(
    messages: Vec<MessageTuple>,
    budget: &ContextBudget,
    db_token_counts: &[i32],
    ctx: CompactionContext<'_>,
) -> FitResult {
    if !budget.enabled {
        return FitResult { estimated_tokens: 0, compacted: false, messages };
    }

    let estimated_tokens = estimate_total(messages.as_slice(), db_token_counts);
    let trigger = budget.compaction_trigger();

    debug!(
        estimated_tokens,
        trigger,
        message_count = messages.len(),
        "context budget check (with compaction)"
    );

    // Not over threshold — pass through
    if estimated_tokens <= trigger {
        return FitResult { estimated_tokens, compacted: false, messages };
    }

    // Compute tail by walking backwards with a token budget
    let tail_budget = budget.tail_token_budget();
    let head_count =
        if messages.first().map(|(t, _, _)| t.as_str()) == Some("system") { 1 } else { 0 };

    let tail_count = compute_tail_count(&messages, db_token_counts, head_count, tail_budget);
    let body_end = messages.len().saturating_sub(tail_count);

    if body_end <= head_count {
        info!(
            estimated_tokens,
            trigger,
            message_count = messages.len(),
            "over threshold but not enough messages to compact"
        );
        return FitResult { estimated_tokens, compacted: false, messages };
    }

    info!(
        estimated_tokens,
        trigger,
        head_count,
        body_range = format!("{}..{}", head_count, body_end),
        tail_count,
        "compacting conversation context via LLM summary"
    );

    let body_messages = &messages[head_count..body_end];

    // Generate summary via LLM
    let summary = match summarizer::generate_summary(
        ctx.client,
        ctx.model_name,
        body_messages,
        ctx.is_butler,
    )
    .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "LLM summary generation failed, passing through uncompacted");
            return FitResult { estimated_tokens, compacted: false, messages };
        }
    };

    // Persist the compaction summary to DB
    let first_id = ctx.message_ids.get(head_count).copied().unwrap_or(0);
    let last_id = ctx.message_ids.get(body_end.saturating_sub(1)).copied().unwrap_or(0);
    if first_id > 0 && last_id > 0 {
        if let Err(e) = persistence::store_compaction_summary(
            ctx.conversation_db,
            ctx.conversation_id,
            &summary,
            first_id,
            last_id,
        ) {
            warn!(error = %e, "failed to persist compaction summary, continuing with in-memory compaction");
        }
    }

    // Assemble compacted message list: Head + Summary + Tail
    let mut compacted = Vec::with_capacity(head_count + 1 + tail_count);
    // Head
    for msg in messages.iter().take(head_count) {
        compacted.push(msg.clone());
    }
    // Summary as system message
    compacted.push(("system".to_string(), summary, vec![]));
    // Tail
    for msg in messages.iter().skip(body_end) {
        compacted.push(msg.clone());
    }

    let compacted_tokens = estimate_total(&compacted, &[]);
    info!(
        original_tokens = estimated_tokens,
        compacted_tokens,
        original_messages = messages.len(),
        compacted_messages = compacted.len(),
        "context compaction complete"
    );

    FitResult { estimated_tokens: compacted_tokens, compacted: true, messages: compacted }
}

/// Walk backwards from the end of the message list, accumulating estimated tokens,
/// until `tail_budget` is exhausted. Returns the number of tail messages to keep.
/// Always keeps at least 1 recent message (the last user/assistant turn).
fn compute_tail_count(
    messages: &[MessageTuple],
    db_token_counts: &[i32],
    head_count: usize,
    tail_budget: usize,
) -> usize {
    if messages.len() <= head_count {
        return 0;
    }
    let candidate_range = head_count..messages.len();
    let mut accumulated: usize = 0;
    let mut count: usize = 0;

    for i in (candidate_range.start..candidate_range.end).rev() {
        let (_, content, attachments) = &messages[i];
        let db_tok = db_token_counts.get(i).copied().unwrap_or(0);
        let msg_tokens = estimate_message_tokens(content, db_tok);
        let att_tokens: usize = attachments
            .iter()
            .map(|a| a.attachment_content.as_deref().map(estimate_by_content).unwrap_or(0))
            .sum();
        let total = msg_tokens + att_tokens;

        // Always include at least 1 message
        if count == 0 {
            accumulated += total;
            count += 1;
            continue;
        }

        if accumulated + total > tail_budget {
            break;
        }
        accumulated += total;
        count += 1;
    }
    count
}

/// Estimate total tokens for a message list with optional DB token data.
fn estimate_total(messages: &[MessageTuple], db_token_counts: &[i32]) -> usize {
    messages
        .iter()
        .enumerate()
        .map(|(i, (_, content, attachments))| {
            let db_tokens = db_token_counts.get(i).copied().unwrap_or(0);
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

    fn msg(msg_type: &str, content: &str) -> MessageTuple {
        (msg_type.to_string(), content.to_string(), vec![])
    }

    #[test]
    fn fit_within_budget_passes_through() {
        let messages = vec![
            msg("system", "You are a helpful assistant."),
            msg("user", "Hello"),
            msg("assistant", "Hi there!"),
        ];
        let budget = ContextBudget::default();
        let result = fit_to_budget(messages.clone(), &budget, &[]);
        assert!(!result.compacted);
        assert_eq!(result.messages.len(), 3);
        assert!(result.estimated_tokens > 0);
        assert!(result.estimated_tokens < budget.compaction_trigger());
    }

    #[test]
    fn fit_disabled_skips_estimation() {
        let messages = vec![msg("user", "x".repeat(1_000_000).as_str())];
        let budget = ContextBudget { enabled: false, ..Default::default() };
        let result = fit_to_budget(messages, &budget, &[]);
        assert!(!result.compacted);
        assert_eq!(result.estimated_tokens, 0);
    }

    #[test]
    fn fit_uses_db_tokens_when_available() {
        let messages = vec![msg("system", "short"), msg("user", "also short")];
        let db_tokens = vec![50_000i32, 60_000];
        let budget = ContextBudget::default();
        let result = fit_to_budget(messages, &budget, &db_tokens);
        assert!(result.estimated_tokens >= 110_000);
    }

    #[test]
    fn estimate_total_handles_attachments() {
        let messages = vec![(
            "user".to_string(),
            "short".to_string(),
            vec![MessageAttachment {
                id: 1,
                message_id: 1,
                attachment_type: crate::db::conversation_db::AttachmentType::Text,
                attachment_url: None,
                attachment_content: Some("a]".repeat(1000)),
                attachment_hash: None,
                use_vector: false,
                token_count: None,
            }],
        )];
        let tokens = estimate_total(&messages, &[]);
        // Content tokens + attachment tokens
        assert!(tokens > 100);
    }

    #[test]
    fn compute_tail_always_keeps_at_least_one() {
        let messages = vec![
            msg("system", "sys"),
            msg("user", &"x".repeat(200_000)), // huge single message
        ];
        let count = compute_tail_count(&messages, &[], 1, 100); // tiny budget
        assert_eq!(count, 1, "must keep at least the last message");
    }

    #[test]
    fn compute_tail_fits_multiple_small_messages() {
        // Each short message ≈ 5-10 tokens
        let messages = vec![
            msg("system", "sys"),
            msg("user", "hello"),
            msg("assistant", "hi"),
            msg("user", "how are you"),
            msg("assistant", "fine"),
            msg("user", "bye"),
        ];
        // Budget of 10000 tokens — all 5 non-system messages should fit
        let count = compute_tail_count(&messages, &[], 1, 10_000);
        assert_eq!(count, 5);
    }

    #[test]
    fn compute_tail_respects_budget_limit() {
        // Create messages where DB tokens make them large
        let messages = vec![
            msg("system", "sys"),
            msg("user", "a"),      // will get 20000 from db
            msg("assistant", "b"), // will get 20000 from db
            msg("user", "c"),      // will get 20000 from db
            msg("assistant", "d"), // will get 20000 from db
        ];
        let db_tokens = vec![0i32, 20_000, 20_000, 20_000, 20_000];
        // Budget: 50000 — should fit last 2 messages (40000) but not 3 (60000)
        let count = compute_tail_count(&messages, &db_tokens, 1, 50_000);
        assert_eq!(count, 2, "should keep 2 messages within 50k budget");
    }

    #[test]
    fn compute_tail_with_zero_budget_keeps_one() {
        let messages = vec![msg("system", "sys"), msg("user", "hello"), msg("assistant", "hi")];
        let count = compute_tail_count(&messages, &[], 1, 0);
        assert_eq!(count, 1, "zero budget still keeps 1 message");
    }
}

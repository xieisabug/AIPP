pub mod budget;
pub mod persistence;
pub mod summarizer;
pub mod token_estimator;

use std::collections::HashSet;

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

/// Marker text replacing truncated old tool results (microcompact).
const TOOL_RESULT_CLEARED: &str = "[旧工具结果已清除]";

/// Maximum number of tool results to keep (counted from newest).
const MICROCOMPACT_KEEP_RECENT: usize = 6;

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
/// Strategy (inspired by Claude Code's multi-level approach):
/// 1. **Microcompact** — truncate old tool results (cheap, no LLM call)
/// 2. **Full compact** — summarize body messages via LLM
///
/// The algorithm splits messages into three segments:
/// - **Head**: the system prompt (first message)
/// - **Body**: everything between Head and Tail — candidate for summarization
/// - **Tail**: the most recent messages (by token budget) — kept verbatim
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
        compaction_threshold = budget.compaction_threshold,
        context_window = budget.context_window_size,
        effective_limit = budget.effective_input_limit(),
        message_count = messages.len(),
        "context budget check (with compaction)"
    );

    // Not over threshold — pass through
    if estimated_tokens <= trigger {
        return FitResult { estimated_tokens, compacted: false, messages };
    }

    info!(estimated_tokens, trigger, "context over threshold, attempting compaction");

    // --- Level 1: Microcompact (truncate old tool results) ---
    let (messages, db_token_counts_vec) = microcompact_tool_results(messages, db_token_counts);
    let db_token_counts = &db_token_counts_vec;
    let post_mc_tokens = estimate_total(&messages, db_token_counts);

    if post_mc_tokens <= trigger {
        info!(
            original_tokens = estimated_tokens,
            post_microcompact_tokens = post_mc_tokens,
            "microcompact sufficient, skipping full LLM compaction"
        );
        return FitResult { estimated_tokens: post_mc_tokens, compacted: true, messages };
    }

    info!(
        post_microcompact_tokens = post_mc_tokens,
        trigger, "microcompact not sufficient, proceeding with LLM compaction"
    );

    // --- Level 2: Full LLM compaction ---
    // Compute tail by walking backwards with a token budget, respecting tool pair atomicity
    let tail_budget = budget.tail_token_budget();
    let head_count =
        if messages.first().map(|(t, _, _)| t.as_str()) == Some("system") { 1 } else { 0 };

    let tail_count = compute_tail_count(&messages, db_token_counts, head_count, tail_budget);
    let body_end = messages.len().saturating_sub(tail_count);

    if body_end <= head_count {
        info!(
            estimated_tokens = post_mc_tokens,
            trigger,
            message_count = messages.len(),
            "over threshold but not enough messages to compact"
        );
        return FitResult { estimated_tokens: post_mc_tokens, compacted: false, messages };
    }

    info!(
        estimated_tokens = post_mc_tokens,
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
            return FitResult { estimated_tokens: post_mc_tokens, compacted: false, messages };
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
        } else {
            info!(
                conversation_id = ctx.conversation_id,
                first_id, last_id, "compaction summary persisted to DB"
            );
        }
    } else {
        warn!(
            head_count,
            body_end,
            message_ids_len = ctx.message_ids.len(),
            "no valid message IDs for compaction persistence — summary will be in-memory only"
        );
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
        post_microcompact_tokens = post_mc_tokens,
        compacted_tokens,
        original_messages = messages.len(),
        compacted_messages = compacted.len(),
        "context compaction complete"
    );

    FitResult { estimated_tokens: compacted_tokens, compacted: true, messages: compacted }
}

/// Microcompact: truncate old tool_result contents to save tokens cheaply.
///
/// Keeps the most recent `MICROCOMPACT_KEEP_RECENT` tool results intact and
/// replaces the content of older ones with a placeholder. This is similar to
/// Claude Code's microcompact strategy.
fn microcompact_tool_results(
    messages: Vec<MessageTuple>,
    db_token_counts: &[i32],
) -> (Vec<MessageTuple>, Vec<i32>) {
    // Collect indices of tool_result messages
    let tool_result_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, (msg_type, content, _))| {
            msg_type == "tool_result" && content != TOOL_RESULT_CLEARED
        })
        .map(|(i, _)| i)
        .collect();

    if tool_result_indices.len() <= MICROCOMPACT_KEEP_RECENT {
        return (messages, db_token_counts.to_vec());
    }

    let truncate_count = tool_result_indices.len() - MICROCOMPACT_KEEP_RECENT;
    let indices_to_truncate: HashSet<usize> =
        tool_result_indices[..truncate_count].iter().copied().collect();

    let mut tokens_saved: usize = 0;
    let mut new_db_tokens = db_token_counts.to_vec();
    // Ensure new_db_tokens covers all messages
    new_db_tokens.resize(messages.len(), 0);

    let new_messages: Vec<MessageTuple> = messages
        .into_iter()
        .enumerate()
        .map(|(i, (msg_type, content, attachments))| {
            if indices_to_truncate.contains(&i) {
                let old_tokens =
                    estimate_message_tokens(&content, new_db_tokens.get(i).copied().unwrap_or(0));
                let new_tokens = estimate_message_tokens(TOOL_RESULT_CLEARED, 0);
                tokens_saved += old_tokens.saturating_sub(new_tokens);
                new_db_tokens[i] = 0; // reset DB token count for truncated message
                (msg_type, TOOL_RESULT_CLEARED.to_string(), attachments)
            } else {
                (msg_type, content, attachments)
            }
        })
        .collect();

    if tokens_saved > 0 {
        info!(
            truncated_results = truncate_count,
            kept_results = MICROCOMPACT_KEEP_RECENT,
            estimated_tokens_saved = tokens_saved,
            "microcompact: truncated old tool results"
        );
    }

    (new_messages, new_db_tokens)
}

/// Walk backwards from the end of the message list, accumulating estimated tokens,
/// until `tail_budget` is exhausted. Returns the number of tail messages to keep.
///
/// Ensures tool_use/tool_result pairs are not split: if including a tool_result
/// message, also include the preceding response that contains the tool_use.
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

    // Ensure atomicity at the tail boundary so we do not split:
    // 1. response/assistant <-> following tool_result messages
    // 2. reasoning <-> the response/assistant it belongs to
    let tail_start = messages.len() - count;
    let expanded_tail_start = expand_tail_start_for_atomic_group(messages, head_count, tail_start);
    if expanded_tail_start < tail_start {
        count += tail_start - expanded_tail_start;
    }

    count
}

fn expand_tail_start_for_atomic_group(
    messages: &[MessageTuple],
    head_count: usize,
    mut tail_start: usize,
) -> usize {
    if tail_start <= head_count || tail_start >= messages.len() {
        return tail_start;
    }

    let (msg_type, _, _) = &messages[tail_start];
    if msg_type == "tool_result" {
        // Walk backwards to include the full response/assistant + tool_result block.
        while tail_start > head_count {
            let prev = tail_start - 1;
            tail_start = prev;
            let (prev_type, _, _) = &messages[tail_start];
            if prev_type == "response" || prev_type == "assistant" {
                break;
            }
        }
    }

    let (msg_type, _, _) = &messages[tail_start];
    if msg_type == "response" || msg_type == "assistant" {
        // Include the reasoning message(s) immediately preceding this response.
        while tail_start > head_count {
            let prev = tail_start - 1;
            let (prev_type, _, _) = &messages[prev];
            if prev_type != "reasoning" {
                break;
            }
            tail_start = prev;
        }
    }

    tail_start
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

    #[test]
    fn compute_tail_does_not_split_tool_result_pair() {
        let messages = vec![
            msg("system", "sys"),
            msg("user", "hello"),
            msg("response", "calling tool"),   // contains tool_use
            msg("tool_result", "tool output"), // paired with response above
            msg("user", "next question"),
        ];
        // Budget should include user("next question") + tool_result, but tool_result
        // should pull in the response too for atomicity
        let db_tokens = vec![0, 100, 100, 100, 100];
        let count = compute_tail_count(&messages, &db_tokens, 1, 250);
        // Should keep: user + tool_result + response = 3 (not split at tool_result)
        assert!(count >= 3, "tail should not split tool_result from its response, got {count}");
    }

    #[test]
    fn compute_tail_does_not_split_reasoning_response_pair() {
        let messages = vec![
            msg("system", "sys"),
            msg("user", "hello"),
            msg("reasoning", "let me think"),
            msg("response", "calling tool"),
            msg("tool_result", "tool output"),
            msg("user", "next question"),
        ];
        // Budget fits user + tool_result + response, but not the preceding reasoning
        // unless atomic expansion pulls it in.
        let db_tokens = vec![0, 100, 100, 100, 100, 100];
        let count = compute_tail_count(&messages, &db_tokens, 1, 300);
        assert!(
            count >= 4,
            "tail should keep reasoning + response + tool_result + user together, got {count}"
        );
    }

    #[test]
    fn microcompact_truncates_old_tool_results() {
        let mut messages = vec![msg("system", "sys")];
        // Add 10 tool results
        for i in 0..10 {
            messages.push(msg("tool_result", &format!("tool output {}", "x".repeat(1000))));
        }
        messages.push(msg("user", "latest"));

        let (result, _) = microcompact_tool_results(messages, &[]);
        let cleared_count = result.iter().filter(|(_, c, _)| c == TOOL_RESULT_CLEARED).count();
        let kept_count = result
            .iter()
            .filter(|(t, c, _)| t == "tool_result" && c != TOOL_RESULT_CLEARED)
            .count();
        assert_eq!(kept_count, MICROCOMPACT_KEEP_RECENT);
        assert_eq!(cleared_count, 10 - MICROCOMPACT_KEEP_RECENT);
    }

    #[test]
    fn microcompact_noop_when_few_results() {
        let messages = vec![
            msg("system", "sys"),
            msg("tool_result", "output 1"),
            msg("tool_result", "output 2"),
            msg("user", "hi"),
        ];
        let (result, _) = microcompact_tool_results(messages.clone(), &[]);
        assert_eq!(result.len(), messages.len());
        assert!(result.iter().all(|(_, c, _)| c != TOOL_RESULT_CLEARED));
    }

    #[test]
    fn compute_tail_atomicity_with_multiple_tool_results() {
        // Scenario: response followed by 3 tool_results, then a user message
        let messages = vec![
            msg("system", "sys"),
            msg("user", "hello"),
            msg("response", "calling 3 tools"), // index 2
            msg("tool_result", "result 1"),     // index 3
            msg("tool_result", "result 2"),     // index 4
            msg("tool_result", "result 3"),     // index 5
            msg("user", "next question"),       // index 6
        ];
        // Budget fits user + 1 tool_result but atomicity should pull in all 3 + response
        let db_tokens = vec![0, 100, 100, 100, 100, 100, 100];
        let count = compute_tail_count(&messages, &db_tokens, 1, 250);
        // Must include: user(6) + tool_result(5,4,3) + response(2) = 5
        assert!(
            count >= 5,
            "tail should include response + all 3 tool_results + user, got {count}"
        );
    }
}

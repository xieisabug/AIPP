//! Concurrent same-conversation continue → auto-fork helpers (MVP).
//!
//! Pure detection lives here so unit tests do not need a DB. DB queries and
//! fork orchestration stay in `sync::mod` / `conversation_api`.

/// Whether applying a **new insert** of `conversation.message` into conversation C
/// would create a concurrent divergence (sibling under the same parent, or a
/// root message into a non-empty conversation).
///
/// - Updates to an existing sync_id (`is_new_insert == false`) never fork.
/// - `parent_id = Some(P)` + another local row with `parent_id = P` → diverge.
/// - `parent_id = None` + conversation already has any messages → diverge.
/// - Regenerate reusing the user-message parent while a response exists is a
///   sibling under that parent → fork (product decision: prefer clean threads).
pub fn detect_concurrent_message_divergence(
    is_new_insert: bool,
    parent_id: Option<i64>,
    sibling_exists_under_parent: bool,
    conversation_already_has_messages: bool,
) -> bool {
    if !is_new_insert {
        return false;
    }
    match parent_id {
        Some(_) => sibling_exists_under_parent,
        None => conversation_already_has_messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_under_same_parent_is_divergence() {
        assert!(detect_concurrent_message_divergence(
            true,
            Some(10),
            true,
            false
        ));
    }

    #[test]
    fn no_sibling_under_parent_is_not_divergence() {
        assert!(!detect_concurrent_message_divergence(
            true,
            Some(10),
            false,
            true
        ));
    }

    #[test]
    fn root_insert_into_nonempty_conversation_is_divergence() {
        assert!(detect_concurrent_message_divergence(
            true, None, false, true
        ));
    }

    #[test]
    fn root_insert_into_empty_conversation_is_not_divergence() {
        assert!(!detect_concurrent_message_divergence(
            true, None, false, false
        ));
    }

    #[test]
    fn update_path_never_diverges() {
        assert!(!detect_concurrent_message_divergence(
            false,
            Some(10),
            true,
            true
        ));
        assert!(!detect_concurrent_message_divergence(
            false, None, true, true
        ));
    }
}

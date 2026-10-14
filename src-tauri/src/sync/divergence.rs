//! Concurrent same-conversation continue → auto-fork helpers (MVP).
//!
//! Pure detection lives here so unit tests do not need a DB. DB queries and
//! fork orchestration stay in `sync::mod` / `conversation_api`.
//!
//! Aligned with design §4.3: tip / `generation_group_id` / `parent_group_id`.
//! `parent_id` is no longer written by conversation writers (always None);
//! never treat `parent_id == None` + non-empty conversation as divergence.

/// Facts about an incoming remote message and the local tip, gathered by the
/// sync apply path before calling [`detect_concurrent_message_divergence`].
#[derive(Debug, Clone, Default)]
pub struct DivergenceInput {
    /// False for map-hit updates — never fork on update.
    pub is_new_insert: bool,
    /// Legacy `message.parent_id` on the remote row (writers currently leave None).
    pub remote_parent_id: Option<i64>,
    pub remote_generation_group_id: Option<String>,
    pub remote_parent_group_id: Option<String>,
    /// Local row already exists with the same legacy `parent_id`.
    pub sibling_exists_under_parent_id: bool,
    /// Local already has a *different* `generation_group_id` that shares
    /// `remote_parent_group_id` (sibling versions / concurrent regenerate).
    pub local_has_sibling_under_parent_group: bool,
    /// Remote `generation_group_id` already present locally (same turn / group).
    pub remote_generation_group_already_local: bool,
    /// Latest finalized local tip's `generation_group_id`, if any.
    pub local_tip_generation_group_id: Option<String>,
    /// Local ask_ai / streaming still running (or unfinished response row).
    pub local_runtime_running: bool,
    /// Local tip messages exist that are not yet confirmed in `sync_shadow`
    /// (local continue not acked / not pulled from server).
    pub local_has_unconfirmed_tip: bool,
}

/// Whether applying a **new insert** of `conversation.message` into conversation C
/// would create a concurrent divergence (design §4.3).
///
/// Guarantee: `parent_id == None` on a normal insert into a non-empty conversation
/// does **not** by itself cause a fork.
pub fn detect_concurrent_message_divergence(input: &DivergenceInput) -> bool {
    if !input.is_new_insert {
        return false;
    }

    // Same generation group already local → same turn (e.g. response after user).
    if input.remote_generation_group_already_local {
        return false;
    }

    // Legacy parent_id sibling (old data / tests).
    if input.remote_parent_id.is_some() {
        return input.sibling_exists_under_parent_id;
    }

    // Versioning / regenerate: sibling under the same parent_group_id.
    if let Some(ref remote_pg) = input.remote_parent_group_id {
        // Remote continues *from* the current local tip → linear extension.
        if input.local_tip_generation_group_id.as_ref() == Some(remote_pg) {
            return false;
        }
        if input.local_has_sibling_under_parent_group {
            return true;
        }
        // parent_group set but no local sibling under it → treat as extension.
        return false;
    }

    // parent_id and parent_group_id both None (normal ask_ai writers).
    // Only fork when local already has a concurrent tip or is mid-turn —
    // never merely because the conversation already has messages.
    if input.local_runtime_running || input.local_has_unconfirmed_tip {
        return true;
    }

    false
}

/// Whether `fork_at` for `fork_conversation_internal` should exclude the local tip.
///
/// When legacy `parent_id` is set, the parent row is the fork point (inclusive).
/// Otherwise copy only **common history** (caller supplies that message id), never
/// the entire local tip.
pub fn resolve_fork_at_message_id(
    remote_parent_id: Option<i64>,
    common_history_tip_message_id: Option<i64>,
) -> Option<i64> {
    match remote_parent_id {
        Some(p) => Some(p),
        None => common_history_tip_message_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_insert() -> DivergenceInput {
        DivergenceInput {
            is_new_insert: true,
            ..DivergenceInput::default()
        }
    }

    #[test]
    fn update_path_never_diverges() {
        let mut input = base_insert();
        input.is_new_insert = false;
        input.sibling_exists_under_parent_id = true;
        input.local_has_unconfirmed_tip = true;
        input.local_runtime_running = true;
        assert!(!detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn nonempty_conversation_parent_id_none_normal_insert_does_not_fork() {
        // Regression: writers always leave parent_id None; non-empty alone must not fork.
        let input = base_insert();
        assert!(!detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn same_generation_group_already_local_does_not_fork() {
        let mut input = base_insert();
        input.remote_generation_group_id = Some("g1".into());
        input.remote_generation_group_already_local = true;
        input.local_has_unconfirmed_tip = true;
        assert!(!detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn legacy_parent_id_sibling_is_divergence() {
        let mut input = base_insert();
        input.remote_parent_id = Some(10);
        input.sibling_exists_under_parent_id = true;
        assert!(detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn legacy_parent_id_without_sibling_is_not_divergence() {
        let mut input = base_insert();
        input.remote_parent_id = Some(10);
        input.sibling_exists_under_parent_id = false;
        assert!(!detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn parent_group_sibling_is_divergence() {
        let mut input = base_insert();
        input.remote_parent_group_id = Some("parent-g".into());
        input.remote_generation_group_id = Some("remote-g".into());
        input.local_has_sibling_under_parent_group = true;
        assert!(detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn parent_group_extending_local_tip_is_not_divergence() {
        let mut input = base_insert();
        input.remote_parent_group_id = Some("tip-g".into());
        input.local_tip_generation_group_id = Some("tip-g".into());
        input.local_has_sibling_under_parent_group = false;
        assert!(!detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn unconfirmed_local_tip_with_root_insert_is_divergence() {
        let mut input = base_insert();
        input.local_has_unconfirmed_tip = true;
        assert!(detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn runtime_running_with_root_insert_is_divergence() {
        let mut input = base_insert();
        input.local_runtime_running = true;
        assert!(detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn fork_at_uses_parent_id_when_present() {
        assert_eq!(resolve_fork_at_message_id(Some(42), Some(7)), Some(42));
    }

    #[test]
    fn fork_at_uses_common_history_when_parent_id_none() {
        assert_eq!(resolve_fork_at_message_id(None, Some(7)), Some(7));
        assert_eq!(resolve_fork_at_message_id(None, None), None);
    }
}

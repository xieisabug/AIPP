# Sync: concurrent continue → auto-fork

When a remote `conversation.message` **insert** would land in conversation C as a
**true concurrent tip** (design §4.3), the client **forks** C into `{name} 同步分叉`
and applies the remote message into the fork instead of interleaving AI turns.

## Detection (`src-tauri/src/sync/divergence.rs`)

Uses tip / `generation_group_id` / `parent_group_id` — **not** the obsolete
`parent_id == None && conversation non-empty` heuristic (`parent_id` is no longer
written by conversation APIs).

Fork when (new insert only):

- legacy `parent_id` sibling exists, or
- sibling `generation_group` under the same `parent_group_id`, or
- local tip is still unconfirmed (`sync_shadow` miss) / local response unfinished,
  and the remote root insert is a different tip

**Guarantee:** non-empty conversation + `parent_id` None normal insert → **no fork**.

## Fork point

When `parent_id` is None, copy only **common history** (last shadowed message), not
the entire local tip, into C'. Remote message lands on C'.

## Hooks

- Apply: `maybe_fork_before_message_insert` in `sync/mod.rs`
- Conflict: `try_fork_message_conflict_instead_of_overwrite` (same detector)
- Fork helper: `fork_conversation_internal` (`ForkNameStyle::SyncDivergence`)

See also: [AIPP-mobile-sync-hostbridge-设计方案.md](./AIPP-mobile-sync-hostbridge-设计方案.md) §4.3.

Deferred: execution lease, full Host Bridge WS/relay/E2EE; stamping `parent_group_id`
on every normal `ask_ai` continue (both-sides-already-pushed concurrent tips without
unconfirmed/local-running signals). SSE notify: shipped as `GET /v1/sync/notify`.

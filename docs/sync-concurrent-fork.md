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
- sibling `generation_group` under the same `parent_group_id`

**Guarantee:** non-empty conversation + `parent_id` None normal insert → **no fork**.
Unconfirmed local rows, delayed acknowledgements and unfinished historical responses
are not proof of divergent ancestry. Root inserts without ancestry do not fork.
Responses in an already routed remote generation group and its descendants reuse
that fork; subsequent streaming updates stay with the mapped message's conversation.

## Fork point

When `parent_id` is None, copy only **common history** (last shadowed message), not
the entire local tip, into C'. Remote message lands on C'.

## Hooks

- Apply: `maybe_fork_before_message_insert` in `sync/mod.rs`
- Conflict: `try_fork_message_conflict_instead_of_overwrite` (same detector)
- Fork helper: `fork_conversation_internal` (`ForkNameStyle::SyncDivergence`)

See also: [AIPP-mobile-sync-hostbridge-设计方案.md](./AIPP-mobile-sync-hostbridge-设计方案.md) §4.3.

Deferred: execution lease, full Host Bridge WS/relay/E2EE; stamping `parent_group_id`
on every normal `ask_ai` continue. Without ancestry, concurrent root turns cannot
be reliably distinguished from a normal delayed pull. SSE notify: shipped as
`GET /v1/sync/notify`.

## Identity and replay

Payload comparisons use sync references rather than device-local FK numbers.
Natural/unique-key merges preserve previous identities as reference aliases. Old
remote versions and delayed acknowledgements cannot roll back an applied version.
Across merged aliases, server sequence orders changes because each UUID has its own
independent version counter. Known provider IDs embedded in model-selection settings
also carry sync references; legacy values without identity are rejected explicitly.
Only unsent pending snapshots are coalesced; attempted events retain their event ID,
base version and payload for idempotent retries. One object has at most one outstanding event in
a push window; accepted predecessors rebase unsent successors.

Required ownership references (for example message → conversation) must not be
uploaded as NULL. Invalid historical changes remain in dead letters with their
object ID and failure reason; no conversation is invented to hide missing data.
Existing fork conversations are not automatically deleted or merged.

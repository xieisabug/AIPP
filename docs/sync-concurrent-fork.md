# Sync: concurrent continue → auto-fork

When a remote `conversation.message` **insert** would land in conversation C as a
**true concurrent tip** (design §4.3), the client **forks** C into `{name} 同步分叉`
and applies the remote message into the fork instead of interleaving AI turns.

## Detection (`src-tauri/src/sync/divergence.rs`)

Uses tip / `generation_group_id` / `parent_group_id` — **not** the obsolete
`parent_id == None && conversation non-empty` heuristic (`parent_id` is no longer
written by conversation APIs).

Fork when (new insert only):

- an outstanding local write is a sibling at the same ancestor, and
- legacy `parent_id` siblings or distinct generation groups under the same
  `parent_group_id` establish that ancestry.

Downloaded historical regenerations are ordinary history, including when multiple
versions share a parent. A mapped/shadowed sibling without an outstanding local
upsert must not fork during bootstrap or repeated replay. Pending writes under an
unrelated ancestor do not establish a conflict. This is conservative: an already
acknowledged local branch cannot prove concurrency using these facts alone; both
message versions remain available, but automatic separation is not guaranteed.

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

Snapshots verify that referenced business rows still exist even when an old sync
map remains. Deleted optional historical references are serialized as NULL;
required owners block upload with a diagnostic. Official identity migration keeps
old UUIDs as aliases so already uploaded children retain their reference identity.
No alternative model is selected.

Uploads prioritize dependencies and wait for queued parent upserts. Pull pages
apply parents first, retry dependency failures to a fixed point, then replay
cross-page failures at the end of pull. Ordering never alters the server cursor.
Dead letters retain the latest sequence; a storage failure stops cursor advancement.
Missing roots that were never delivered still require a corrected source snapshot.

## Regression coverage

`src-tauri/src/sync/convergence_tests.rs` exercises production snapshot, outbox,
identity/FK application, fork routing and replay code using independent in-memory
device databases. The opt-in `test_sync_server_multidevice_roundtrip` sends Rust
protocol structs through real FastAPI authentication/push/merge/pull routes via
`sync-server/app/tests/sync_client_bridge.py`. It uses 1,200 messages, three devices,
multiple page sizes, duplicate uploads, full replay and same-device metadata reset.
It does not launch Tauri windows or test network transport. Server API tests also
cover a 10,000-message graph with children arriving before parents.

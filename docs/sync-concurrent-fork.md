# Sync: concurrent continue → auto-fork

When a remote `conversation.message` **insert** would land in conversation C under a parent that already has a local sibling (or a root message into a non-empty C), the client **forks** C into `{name} 同步分叉` and applies the remote message into the fork instead of interleaving AI turns.

- Detection: `src-tauri/src/sync/divergence.rs` (`detect_concurrent_message_divergence`)
- Apply hook: `apply_change` / conflict path in `src-tauri/src/sync/mod.rs`
- Fork: `fork_conversation_internal` in `conversation_api.rs` (`ForkNameStyle::SyncDivergence`)

See also: [AIPP-mobile-sync-hostbridge-设计方案.md](./AIPP-mobile-sync-hostbridge-设计方案.md) §4.3.

Deferred: execution lease, full Host Bridge WS/relay/E2EE. (SSE notify: shipped as GET /v1/sync/notify)

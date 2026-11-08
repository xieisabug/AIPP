# AIPP genai source

This directory contains the library source, manifest, README and licenses from
https://github.com/xieisabug/rust-genai at
`4bdd56e4b308a988300d442ed73359d16f845e23` (the previously pinned dependency).
It is used directly by `src-tauri/Cargo.toml` so local protocol fixes are included
in reproducible application builds. The older `vendor/rust-genai` gitlink is not
used by Cargo.

Local changes:

- Add `ChatOptions.prompt_cache_ttl` and its builder / per-call option resolution.
- Serialize it as `prompt_cache_options: { "ttl": "30m" | "12h" }` in the
  Responses adapter, without sending legacy `prompt_cache_retention` alongside it.
- Reject unsupported TTLs with a specific error before HTTP transport.

Coverage lives in AIPP's `api/tests/ai_config_tests.rs` and captures actual
loopback HTTP request bodies without external API calls.

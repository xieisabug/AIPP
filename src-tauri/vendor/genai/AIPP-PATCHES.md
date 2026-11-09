# AIPP genai source

This directory contains library source, manifest, README and licenses from
https://github.com/xieisabug/rust-genai at
`8a322dcb68eff91f1d24991cb6b4e22e21ae3b5b` (version 0.7.4, matching main).
It is used directly by `src-tauri/Cargo.toml` so the existing AIPP protocol
patch remains part of reproducible builds. The older `vendor/rust-genai`
gitlink is not used by Cargo.

Local changes:

- Retain `ChatOptions.prompt_cache_ttl`, its builder and per-call option resolution.
- Serialize an explicit TTL as `prompt_cache_options.ttl` (`30m` or `12h`),
  retaining the upstream `mode` when its endpoint/model policy adds one.
- Explicit TTL takes precedence over automatic TTL and legacy
  `prompt_cache_retention`; unsupported values fail before HTTP transport.
- Keep upstream cache behavior unchanged when no explicit TTL is supplied.
- Include the explicit TTL error in optional OpenTelemetry classification.

Coverage lives in AIPP's `api/tests/ai_config_tests.rs` and captures actual
loopback HTTP request bodies without external API calls. Upstream unit tests
remain in the vendored source. The manifest's named `c12-otel` example is
included so its target remains valid.

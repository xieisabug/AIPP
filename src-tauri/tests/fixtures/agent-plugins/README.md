# OpenAI plugin compatibility fixtures

Source: https://github.com/openai/plugins/tree/d420d13051bffe4543ed39e30fa46b87148e47dd

Captured 2026-11-11 for parser regression tests. `upstream.json` includes the manifests, MCP/app declarations and skill frontmatter of the 62 top-level plugins at this revision. Fixture-only nested plugins are excluded. Non-Notion skill bodies are omitted. Public client-secret fields are replaced with a fixture placeholder. No account credentials were obtained or used.

These fixtures test format parsing, not availability of hosted connectors, script dependencies, OAuth registrations or model behavior. The original package license declarations remain in each manifest. Tests must not execute these instructions or connect to the declared services.

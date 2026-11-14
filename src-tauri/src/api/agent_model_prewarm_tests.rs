use super::{is_prewarm_safe_agent, prewarm_targets, PrewarmCandidate};

fn candidate(assistant_id: i64, provider_api_type: &str, provider_enabled: bool) -> PrewarmCandidate {
    PrewarmCandidate { assistant_id, provider_api_type: provider_api_type.into(), provider_enabled }
}

#[test]
fn test_agent_model_prewarm_only_probes_agents_without_session_side_effects() {
    for api_type in ["codex_app_server", "pi", "cursor", "opencode"] {
        assert!(is_prewarm_safe_agent(api_type), "{api_type} probes without creating a session");
    }
    // session/new during the probe would show up in the agent's own history
    for api_type in ["acp", "kiro", "antigravity"] {
        assert!(!is_prewarm_safe_agent(api_type), "{api_type} must wait until the user opens it");
    }
    // Static list: nothing to warm
    assert!(!is_prewarm_safe_agent("claude_sdk"));
}

#[test]
fn test_agent_model_prewarm_targets_skip_disabled_providers_and_unsafe_agents() {
    let candidates = [
        candidate(16, "codex_app_server", true),
        candidate(17, "pi", true),
        candidate(18, "pi", false),
        candidate(19, "kiro", true),
        candidate(20, "cursor", true),
    ];

    assert_eq!(prewarm_targets(&candidates), vec![16, 17, 20]);
}

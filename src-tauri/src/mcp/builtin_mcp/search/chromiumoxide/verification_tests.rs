use super::*;

#[test]
fn test_human_verification_finds_closed_shadow_iframe_only_for_known_host() {
    let document = json!({"nodeName":"DIV","shadowRoots":[{"shadowRootType":"closed","children":[
        {"nodeName":"IFRAME","nodeId":42,"attributes":["src","https://challenges.cloudflare.com/cdn-cgi/challenge-platform/widget"]},
        {"nodeName":"IFRAME","nodeId":43,"attributes":["src","https://challenges.cloudflare.com.evil.example/widget"]},
        {"nodeName":"IFRAME","nodeId":44,"attributes":["src","https://example.com/?next=challenges.cloudflare.com"]}
    ]}]});
    assert_eq!(closed_challenge_nodes(&document), vec![42]);
    assert!(closed_challenge_nodes(&json!({"nodeName":"DIV"})).is_empty());
}

#[test]
fn test_human_verification_detects_turnstile_without_exposing_query() {
    let state = PageState { url: "https://kagi.com/turnstile".into(), ready: "complete".into(), has_body: true, ..Default::default() };
    assert_eq!(classify(&state), PageStatus::HumanRequired);
}

#[test]
fn test_human_verification_challenge_takes_priority_over_stale_results() {
    let state = PageState { has_results: true, visible_challenge: true, ..Default::default() };
    assert_eq!(classify(&state), PageStatus::HumanRequired);
}

#[test]
fn test_human_verification_requires_real_results_before_completion() {
    let mut state = PageState { url: "https://kagi.com/".into(), ready: "complete".into(), has_body: true, ..Default::default() };
    assert!(!verification_complete(&state));
    state.has_results = true;
    assert!(verification_complete(&state));
    state.challenge_text = true;
    assert!(!verification_complete(&state));
}

use super::*;

#[test]
fn test_human_verification_search_wait_defaults_to_sixty_seconds() {
    assert_eq!(FetchConfig::default().wait_timeout_ms, 60_000);
}

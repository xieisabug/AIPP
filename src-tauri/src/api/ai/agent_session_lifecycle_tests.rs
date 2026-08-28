use super::agent_session_lifecycle::should_release_idle_session;
use std::time::Duration;

#[test]
fn test_idle_session_releases_after_timeout() {
    assert!(should_release_idle_session(
        false,
        Duration::from_secs(15 * 60),
        Duration::from_secs(15 * 60),
    ));
}

#[test]
fn test_active_session_never_releases_at_timeout() {
    assert!(!should_release_idle_session(
        true,
        Duration::from_secs(60 * 60),
        Duration::from_secs(15 * 60),
    ));
}

#[test]
fn test_recent_idle_session_stays_warm() {
    assert!(!should_release_idle_session(
        false,
        Duration::from_secs(14 * 60),
        Duration::from_secs(15 * 60),
    ));
}

use super::*;

#[test]
fn test_capability_defaults_preserve_existing_installations() {
    assert!(configured_enabled(None));
    assert!(configured_enabled(Some("true")));
    assert!(configured_enabled(Some("1")));
}

#[test]
fn test_capability_disabled_or_invalid_values_fail_closed() {
    for value in ["false", "0", "", "yes", "TRUE", "garbage"] {
        assert!(!configured_enabled(Some(value)), "{value}");
    }
}

#[test]
fn test_capability_keys_are_independent_and_scoped_to_butler() {
    let keys: std::collections::HashSet<_> = [
        ButlerCapability::TaskDispatch, ButlerCapability::PeerCollaboration,
        ButlerCapability::WorkContinuation, ButlerCapability::BackgroundChecks,
    ].into_iter().map(ButlerCapability::key).collect();
    assert_eq!(keys.len(), 4);
    assert!(keys.iter().all(|key| key.starts_with("butler_")));
}

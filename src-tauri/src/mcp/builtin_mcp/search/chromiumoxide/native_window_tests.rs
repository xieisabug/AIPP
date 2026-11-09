use super::*;

#[test]
fn test_native_verification_keeps_normal_search_headless() {
    assert!(super::super::fetcher::FetchConfig::default().headless);
    let params = page_params(true);
    assert_eq!(params.new_window, None);
    assert_eq!(params.background, None);
}

#[test]
fn test_native_verification_opens_independent_foreground_window_only_for_handoff() {
    let params = page_params(false);
    assert_eq!(params.new_window, Some(true));
    assert_eq!(params.background, Some(false));
    assert_eq!(params.window_state, None);
    assert_eq!(params.url, "about:blank");
}

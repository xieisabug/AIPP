use super::*;

fn stored(token: &str) -> (String, String) {
    let docs = DOCUMENTS.lock().unwrap();
    let doc = docs.get(token).expect("document stored");
    (doc.html.clone(), doc.policy.clone())
}

fn script_src(policy: &str) -> &str {
    policy.split(';').map(str::trim).find(|d| d.starts_with("script-src")).expect("script-src")
}

#[test]
fn test_choice_document_static_runs_only_the_nonce_height_script() {
    let token = create_choice_document("<p>hi</p>".into(), vec![], false).unwrap();
    let (html, policy) = stored(&token);
    let src = script_src(&policy);
    let nonce = src.strip_prefix("script-src 'nonce-").and_then(|s| s.strip_suffix('\'')).unwrap();
    assert!(policy.starts_with("sandbox allow-scripts;"));
    assert!(!src.contains("unsafe-inline"));
    assert!(html.starts_with("<p>hi</p>"));
    assert!(html.ends_with(&format!("<script nonce=\"{nonce}\">{FRAME_HEIGHT_SCRIPT}</script>")));
    let other = create_choice_document("<p>hi</p>".into(), vec![], false).unwrap();
    assert_ne!(script_src(&stored(&other).1), src);
    close_choice_document(token);
    close_choice_document(other);
}

#[test]
fn test_choice_document_scripted_keeps_unsafe_inline_without_nonce() {
    let token = create_choice_document("<script>go()</script>".into(), vec![], true).unwrap();
    let (html, policy) = stored(&token);
    let src = script_src(&policy);
    assert!(src.contains("'unsafe-inline'"));
    assert!(!src.contains("nonce-"));
    assert!(html.contains(FRAME_HEIGHT_SCRIPT));
    close_choice_document(token);
}

use super::*;

#[test]
#[cfg(target_os = "windows")]
#[ignore = "Explicit Windows Credential Manager smoke test; writes and removes a unique dummy credential"]
fn test_apps_windows_credential_store_roundtrip() {
    let reference = uuid::Uuid::new_v4().to_string();
    struct Cleanup(String);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = clear(&self.0); } }
    let _cleanup = Cleanup(reference.clone());
    let credential = Credential { server_id: 900001, endpoint: "https://example.com/mcp".into(), headers: HashMap::from([("authorization".into(), "Bearer aipp-dummy-smoke-value".into())]), oauth: None };
    save(&reference, &credential).unwrap();
    let loaded = load(&reference).unwrap();
    assert_eq!(loaded.server_id, credential.server_id);
    assert_eq!(loaded.headers, credential.headers);
    clear(&reference).unwrap();
    assert!(load_optional(&reference).unwrap().is_none());
}

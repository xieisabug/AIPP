use super::*;

#[test]
#[cfg(target_os = "windows")]
#[ignore = "Explicit Windows Credential Manager smoke test; writes and removes a unique dummy credential"]
fn test_apps_windows_credential_store_roundtrip() {
    let reference = uuid::Uuid::new_v4().to_string();
    struct Cleanup(String);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = delete_legacy(&self.0); } }
    let _cleanup = Cleanup(reference.clone());
    let credential = Credential { server_id: 900001, endpoint: "https://example.com/mcp".into(), headers: HashMap::from([("authorization".into(), "Bearer aipp-dummy-smoke-value".into())]), oauth: None };
    let raw = serde_json::to_string(&credential).unwrap();
    entry(&reference).unwrap().set_password(&raw).unwrap();
    let loaded: Credential = serde_json::from_str(&entry(&reference).unwrap().get_password().unwrap()).unwrap();
    assert_eq!(loaded.server_id, credential.server_id);
    assert_eq!(loaded.headers, credential.headers);
    delete_legacy(&reference).unwrap();
    assert!(matches!(entry(&reference).unwrap().get_password(), Err(keyring::Error::NoEntry)));
}

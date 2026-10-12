use super::{cleanup_profile_locks, is_profile_singleton_error};
use std::fs;
use std::path::Path;

#[test]
fn test_is_profile_singleton_error_matches_chrome_abort() {
    let error = "Failed to create SingletonLock: File exists (17)\n\
        Failed to create a ProcessSingleton for your profile directory. Aborting now to avoid profile corruption.";
    assert!(is_profile_singleton_error(error));
    assert!(!is_profile_singleton_error("Timed out waiting for browser page slot"));
}

#[cfg(unix)]
#[test]
fn test_cleanup_profile_locks_removes_dangling_symlink_when_owner_is_dead() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lock = dir.path().join("SingletonLock");
    std::os::unix::fs::symlink("host-name.local-2147483647", &lock).expect("symlink lock");
    std::os::unix::fs::symlink("missing-socket", dir.path().join("SingletonSocket"))
        .expect("symlink socket");
    std::os::unix::fs::symlink("8847379793044060837", dir.path().join("SingletonCookie"))
        .expect("symlink cookie");

    assert!(!lock.exists(), "Path::exists follows the hostname-pid symlink and misses the lock");

    cleanup_profile_locks(dir.path(), "test_dead_owner");

    assert!(metadata_absent(&lock));
    assert!(metadata_absent(&dir.path().join("SingletonSocket")));
    assert!(metadata_absent(&dir.path().join("SingletonCookie")));
}

#[cfg(unix)]
#[test]
fn test_cleanup_profile_locks_keeps_symlink_when_owner_is_alive() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lock = dir.path().join("SingletonLock");
    let target = format!("xiejingyangdeMacBook-Pro.local-{}", std::process::id());
    std::os::unix::fs::symlink(&target, &lock).expect("symlink lock");
    std::os::unix::fs::symlink("cookie", dir.path().join("SingletonCookie"))
        .expect("symlink cookie");

    cleanup_profile_locks(dir.path(), "test_live_owner");

    assert_eq!(fs::read_link(&lock).expect("lock remains").to_string_lossy(), target);
    assert!(fs::symlink_metadata(dir.path().join("SingletonCookie")).is_ok());
}

#[test]
fn test_cleanup_profile_locks_ignores_missing_directory_entries() {
    let dir = tempfile::tempdir().expect("temp dir");
    cleanup_profile_locks(dir.path(), "test_absent");
    assert!(dir.path().exists());
}

fn metadata_absent(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        _ => false,
    }
}

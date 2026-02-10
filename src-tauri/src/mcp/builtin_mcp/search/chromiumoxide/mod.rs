use std::fs;
use std::path::Path;
use tracing::{info, warn};

pub mod browser_pool;
pub mod fetcher;

pub use browser_pool::{BrowserPool, BrowserPoolConfig, PooledPage};
pub use fetcher::{ContentFetcher, FetchConfig};

pub(crate) fn cleanup_profile_locks(user_data_dir: &Path, context: &str) {
    let lock_files = ["SingletonLock", "SingletonSocket", "SingletonCookie"];
    let mut removed = Vec::new();
    let mut failed = Vec::new();
    let mut found = false;

    info!(
        context,
        user_data_dir = %user_data_dir.display(),
        "Checking Chromium profile singleton locks"
    );

    for name in lock_files {
        let path = user_data_dir.join(name);
        if !path.exists() {
            continue;
        }
        found = true;
        match fs::remove_file(&path) {
            Ok(_) => removed.push(name.to_string()),
            Err(e) => {
                warn!(
                    context,
                    path = %path.display(),
                    error = %e,
                    "Failed to remove Chromium profile singleton lock"
                );
                failed.push(name.to_string());
            }
        }
    }

    if !removed.is_empty() {
        info!(
            context,
            user_data_dir = %user_data_dir.display(),
            removed = ?removed,
            "Removed Chromium profile singleton locks"
        );
    }
    if !failed.is_empty() {
        warn!(
            context,
            user_data_dir = %user_data_dir.display(),
            failed = ?failed,
            "Failed to remove Chromium profile singleton locks"
        );
    }
    if !found {
        info!(
            context,
            user_data_dir = %user_data_dir.display(),
            "No Chromium profile singleton locks found"
        );
    }
}

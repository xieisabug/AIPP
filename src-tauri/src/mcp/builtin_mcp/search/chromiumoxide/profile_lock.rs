use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, info, warn};

const LOCK_FILES: [&str; 3] = ["SingletonLock", "SingletonSocket", "SingletonCookie"];
const LOCK_RELEASE_WAIT: Duration = Duration::from_secs(3);
const LOCK_RELEASE_POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockOwner {
    Absent,
    Alive(i32),
    Dead(i32),
    /// 锁存在，但不是可解析的 hostname-pid 符号链接。
    Stale,
    /// 无法判断锁状态，保留文件。
    Unknown,
}

fn launch_lock() -> &'static tokio::sync::Mutex<()> {
    static LAUNCH_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LAUNCH_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) fn is_profile_singleton_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("singletonlock")
        || lower.contains("processsingleton")
        || lower.contains("process singleton")
        || lower.contains("profile directory")
}

/// 删除已经退出的 Chrome 留下的 profile 锁。
///
/// `SingletonLock` 在 macOS/Linux 上是指向 `主机名-pid` 的符号链接，目标并不是真实路径。
/// `Path::exists()` 会跟随链接，因此看不到这把锁。这里用 `symlink_metadata` 判断文件本身。
/// 属主进程仍在运行时保留锁，避免两个 Chrome 同时写同一份 profile。
pub(crate) fn cleanup_profile_locks(user_data_dir: &Path, context: &str) {
    match singleton_lock_owner(user_data_dir) {
        LockOwner::Alive(pid) => {
            warn!(
                context,
                pid,
                user_data_dir = %user_data_dir.display(),
                "Keeping Chromium profile singleton locks because the owner process is still running"
            );
        }
        LockOwner::Unknown => {
            warn!(
                context,
                user_data_dir = %user_data_dir.display(),
                "Skipping Chromium profile lock cleanup because the lock state could not be read"
            );
        }
        LockOwner::Absent => {
            let (removed, failed) = remove_present_lock_files(user_data_dir, context);
            if removed.is_empty() && failed.is_empty() {
                debug!(
                    context,
                    user_data_dir = %user_data_dir.display(),
                    "No Chromium profile singleton locks found"
                );
            }
        }
        LockOwner::Dead(pid) => {
            info!(
                context,
                pid,
                user_data_dir = %user_data_dir.display(),
                "Removing Chromium profile locks left by an exited process"
            );
            remove_present_lock_files(user_data_dir, context);
        }
        LockOwner::Stale => {
            info!(
                context,
                user_data_dir = %user_data_dir.display(),
                "Removing unreadable Chromium profile singleton locks"
            );
            remove_present_lock_files(user_data_dir, context);
        }
    }
}

/// 启动前回收锁。属主还活着时先等它退出，超时后仍不删除活进程的锁。
pub(crate) async fn prepare_profile_for_launch(user_data_dir: &Path, context: &str) {
    let deadline = tokio::time::Instant::now() + LOCK_RELEASE_WAIT;
    let mut logged_wait = false;

    loop {
        match singleton_lock_owner(user_data_dir) {
            LockOwner::Alive(pid) => {
                if tokio::time::Instant::now() >= deadline {
                    warn!(
                        context,
                        pid,
                        user_data_dir = %user_data_dir.display(),
                        "Chromium profile lock is still held by a live process"
                    );
                    return;
                }
                if !logged_wait {
                    info!(
                        context,
                        pid,
                        user_data_dir = %user_data_dir.display(),
                        "Waiting for Chromium profile lock owner to exit"
                    );
                    logged_wait = true;
                }
                tokio::time::sleep(LOCK_RELEASE_POLL).await;
            }
            _ => {
                cleanup_profile_locks(user_data_dir, context);
                return;
            }
        }
    }
}

/// 同一时刻只启动一个使用搜索 profile 的 Chrome。
/// 如果启动失败是 profile 单例锁，回收已退出进程的锁后再试一次。
pub(crate) async fn launch_browser_recovering_stale_locks<T, F, Fut>(
    user_data_dir: &Path,
    context: &str,
    mut launch: F,
) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let _launch_guard = launch_lock().lock().await;
    prepare_profile_for_launch(user_data_dir, context).await;

    match launch().await {
        Ok(value) => Ok(value),
        Err(error) if is_profile_singleton_error(&error) => {
            warn!(
                context,
                error = %error,
                user_data_dir = %user_data_dir.display(),
                "Browser launch hit a profile singleton lock, reclaiming stale locks and retrying once"
            );
            cleanup_profile_locks(user_data_dir, &format!("{context}_retry"));
            match launch().await {
                Ok(value) => Ok(value),
                Err(retry_error) => Err(annotate_live_lock_owner(user_data_dir, retry_error)),
            }
        }
        Err(error) => Err(error),
    }
}

fn annotate_live_lock_owner(user_data_dir: &Path, error: String) -> String {
    match singleton_lock_owner(user_data_dir) {
        LockOwner::Alive(pid) => format!(
            "{error} (profile lock still held by live process pid={pid}, user_data_dir={})",
            user_data_dir.display()
        ),
        _ => error,
    }
}

fn singleton_lock_owner(user_data_dir: &Path) -> LockOwner {
    let path = user_data_dir.join("SingletonLock");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return LockOwner::Absent,
        Err(error) => {
            warn!(
                path = %path.display(),
                error = %error,
                "Failed to inspect Chromium SingletonLock"
            );
            return LockOwner::Unknown;
        }
    };

    if !metadata.file_type().is_symlink() {
        return LockOwner::Stale;
    }

    match read_lock_pid(&path) {
        Some(pid) if process_is_alive(pid) => LockOwner::Alive(pid),
        Some(pid) => LockOwner::Dead(pid),
        None => LockOwner::Stale,
    }
}

fn read_lock_pid(path: &Path) -> Option<i32> {
    let target = fs::read_link(path).ok()?;
    let target = target.to_string_lossy();
    let pid_text = target.rsplit('-').next()?.trim();
    let pid = pid_text.parse::<i32>().ok()?;
    if pid <= 0 {
        None
    } else {
        Some(pid)
    }
}

fn remove_present_lock_files(user_data_dir: &Path, context: &str) -> (Vec<String>, Vec<String>) {
    let mut removed = Vec::new();
    let mut failed = Vec::new();

    for name in LOCK_FILES {
        let path = user_data_dir.join(name);
        if fs::symlink_metadata(&path).is_err() {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed.push(name.to_string()),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                warn!(
                    context,
                    path = %path.display(),
                    error = %error,
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

    (removed, failed)
}

#[cfg(unix)]
fn process_is_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }

    // SAFETY: pid 已经排除 0 和负数，kill(pid, 0) 只探测进程是否存在，不发送信号。
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }

    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn process_is_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const ERROR_ACCESS_DENIED: u32 = 5;

    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
        fn GetLastError() -> u32;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if !handle.is_null() {
            CloseHandle(handle);
            return true;
        }
        GetLastError() == ERROR_ACCESS_DENIED
    }
}

#[cfg(test)]
#[path = "profile_lock_tests.rs"]
mod profile_lock_tests;

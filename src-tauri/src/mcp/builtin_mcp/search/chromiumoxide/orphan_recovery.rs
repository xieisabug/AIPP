#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct BrowserProcess {
    pid: u32,
    parent_alive: bool,
    name: String,
    command_line: String,
    created: String,
}

fn command_args(command: &str) -> Vec<String> {
    #[link(name = "shell32")]
    extern "system" {
        fn CommandLineToArgvW(command: *const u16, count: *mut i32) -> *mut *mut u16;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }
    let wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
    let mut count = 0;
    // SAFETY: input is NUL terminated; argv is a count-sized allocation owned by LocalFree.
    unsafe {
        let argv = CommandLineToArgvW(wide.as_ptr(), &mut count);
        if argv.is_null() { return Vec::new(); }
        let mut result = Vec::new();
        for index in 0..count as usize {
            let value = *argv.add(index);
            let mut length = 0;
            while *value.add(length) != 0 { length += 1; }
            result.push(String::from_utf16_lossy(std::slice::from_raw_parts(value, length)));
        }
        LocalFree(argv.cast());
        result
    }
}

fn normalized_profile(profile: &str) -> String {
    profile.trim_start_matches(r"\\?\").replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn is_orphan_search_browser(process: &BrowserProcess, profile: &str) -> bool {
    if process.parent_alive || process.pid == 0 || !process.name.eq_ignore_ascii_case("chrome.exe") {
        return false;
    }
    let args = command_args(&process.command_line);
    let profiles: Vec<_> = args.iter().filter_map(|arg| arg.strip_prefix("--user-data-dir=")).collect();
    profiles.len() == 1
        && normalized_profile(profiles[0]) == normalized_profile(profile)
        && args.iter().any(|arg| arg == "--enable-automation")
        && args.iter().any(|arg| arg == "--remote-debugging-port=0")
        && !args.iter().any(|arg| arg.starts_with("--type="))
}

async fn inspect_or_stop(expected: Option<&BrowserProcess>) -> Result<Vec<BrowserProcess>, String> {
    let mut command = tokio::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", include_str!("orphan_recovery.ps1")])
        .creation_flags(0x08000000).kill_on_drop(true);
    // Environment data is never interpolated into executable PowerShell source.
    command.env("AIPP_SEARCH_EXPECTED_PROCESS", expected.map(|p| serde_json::to_string(p).unwrap()).unwrap_or_default());
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output()).await
        .map_err(|_| "Windows search browser recovery: process inspection/stop timed out after 10000ms".to_string())?
        .map_err(|e| format!("Windows search browser recovery: cannot start process inspection: {e}"))?;
    if !output.status.success() {
        return Err(format!("Windows search browser recovery failed (status={}): {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Windows search browser recovery: invalid process snapshot: {e}; stderr={}", String::from_utf8_lossy(&output.stderr).trim()))
}

pub(super) async fn recover_orphaned_browser(profile: &std::path::Path) -> Result<(), String> {
    let profile_text = profile.to_string_lossy();
    for process in inspect_or_stop(None).await? {
        if is_orphan_search_browser(&process, &profile_text) {
            tracing::warn!(pid = process.pid, profile = %profile.display(), "Closing orphaned AIPP search Chrome before launch");
            inspect_or_stop(Some(&process)).await.map_err(|e| format!("{e} (pid={}, profile={})", process.pid, profile.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "orphan_recovery_tests.rs"]
mod orphan_recovery_tests;

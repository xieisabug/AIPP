use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use super::background_process::background_command;

pub(super) struct Runtime {
    pub path: PathBuf,
    pub version: String,
}

/// Keep the executable and its successful probe together. A bare filename tests the
/// process working directory, so expand PATH into full candidates before probing.
pub(super) fn find_runtime(
    name: &str,
    preferred: Option<PathBuf>,
    search_path: &OsStr,
    common_dirs: Vec<PathBuf>,
) -> Option<Runtime> {
    let dirs = preferred.into_iter().chain(std::env::split_paths(search_path)).chain(common_dirs);
    let mut seen = HashSet::new();
    for dir in dirs.filter(|dir| dir.is_absolute()) {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let extensions: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
        for extension in extensions {
            let path = dir.join(format!("{name}{extension}"));
            if path.is_file() {
                if let Some(version) = version_at(&path) {
                    return Some(Runtime { path, version });
                }
            }
        }
    }
    None
}

fn version_at(path: &Path) -> Option<String> {
    let output = background_command(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.split_whitespace().find_map(|part| {
        let version = part.trim_start_matches('v');
        semver::Version::parse(version).ok().map(|_| version.to_owned())
    })
}

pub(super) fn common_bin_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("scoop/shims"));
        }
        if let Some(local) = dirs::data_local_dir() {
            dirs.push(local.join("Microsoft/WinGet/Links"));
        }
        dirs
    }
    #[cfg(not(windows))]
    {
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from).to_vec()
    }
}

#[cfg(test)]
#[path = "tool_runtime_tests.rs"]
mod tests;

use std::path::PathBuf;
use tauri::AppHandle;

use super::tool_runtime::{common_bin_dirs, find_runtime, Runtime};

pub struct UvUtils;

impl UvUtils {
    pub(crate) fn detect() -> Option<(PathBuf, String)> {
        let mut common = Vec::new();
        for variable in ["UV_INSTALL_DIR", "UV_UNMANAGED_INSTALL", "XDG_BIN_HOME"] {
            common.extend(std::env::var_os(variable).map(PathBuf::from));
        }
        #[cfg(windows)]
        if let Some(dir) = dirs::data_local_dir() {
            common.push(dir.join("Programs/uv"));
        }
        if let Some(home) = dirs::home_dir() {
            common.extend([home.join(".local/bin"), home.join(".cargo/bin")]);
        }
        common.extend(common_bin_dirs());
        find_runtime("uv", None, &std::env::var_os("PATH").unwrap_or_default(), common)
            .map(|Runtime { path, version }| (path, version))
    }

    pub fn get_uv_version(_app_handle: &AppHandle) -> Result<String, String> {
        Ok(Self::detect().map(|(_, version)| version).unwrap_or_else(|| "Not Installed".into()))
    }

    pub fn find_uv_executable() -> Option<PathBuf> {
        Self::detect().map(|(path, _)| path)
    }
}

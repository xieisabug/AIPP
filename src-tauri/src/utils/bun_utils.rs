use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use super::tool_runtime::{common_bin_dirs, find_runtime, Runtime};

pub struct BunUtils;

// CLI launch plans also run without an AppHandle. Capture this installation's
// data directory at startup, including installations with a custom identifier.
#[cfg(windows)]
static MANAGED_BIN: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

impl BunUtils {
    #[cfg(windows)]
    pub(crate) fn initialize_managed_bin(app_handle: &AppHandle) {
        if let Ok(dir) = app_handle.path().app_data_dir() {
            let _ = MANAGED_BIN.set(dir.join("bun/bin"));
        }
    }

    #[cfg(windows)]
    pub(crate) fn managed_bin() -> Option<&'static std::path::Path> {
        MANAGED_BIN.get().map(PathBuf::as_path)
    }

    fn detect(app_handle: &AppHandle) -> Option<Runtime> {
        let managed = app_handle.path().app_data_dir().ok().map(|dir| dir.join("bun/bin"));
        let mut common = Vec::new();
        if let Some(root) = std::env::var_os("BUN_INSTALL") {
            common.push(PathBuf::from(root).join("bin"));
        }
        if let Some(home) = dirs::home_dir() {
            common.push(home.join(".bun/bin"));
        }
        common.extend(common_bin_dirs());
        find_runtime("bun", managed, &std::env::var_os("PATH").unwrap_or_default(), common)
    }

    pub fn get_bun_executable(
        app_handle: &AppHandle,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        Self::detect(app_handle).map(|runtime| runtime.path)
            .ok_or_else(|| "在应用数据目录、PATH 和常用安装目录中未找到可运行的 Bun".into())
    }

    pub fn get_bun_version(app_handle: &AppHandle) -> Result<String, String> {
        Ok(Self::detect(app_handle).map(|runtime| runtime.version)
            .unwrap_or_else(|| "Not Installed".into()))
    }
}

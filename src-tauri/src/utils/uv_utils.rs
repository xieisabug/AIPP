use dirs;
use std::process::Command;
use tauri::AppHandle;

/// Uv 可执行文件工具函数
pub struct UvUtils;

impl UvUtils {
    /// 获取 Uv 可执行文件名（根据操作系统）
    fn get_uv_executable_name() -> &'static str {
        if cfg!(target_os = "windows") {
            "uv.exe"
        } else {
            "uv"
        }
    }

    /// 获取 Uv 版本信息
    pub fn get_uv_version(_app_handle: &AppHandle) -> Result<String, String> {
        let uv_executable_name = Self::get_uv_executable_name();

        let get_version = |exe: &std::path::Path| -> Option<String> {
            if exe.exists() {
                match Command::new(exe).arg("--version").output() {
                    Ok(output) if output.status.success() => {
                        let version_info =
                            String::from_utf8_lossy(&output.stdout).trim().to_string();
                        // The output is like "uv 0.4.0 (d9bd3bc7a 2024-08-28)"
                        // We need the version number "0.4.0" which is the second word
                        let parts: Vec<&str> = version_info.split_whitespace().collect();
                        if parts.len() >= 2 {
                            // 第二个部分是版本号 (0.4.0)
                            Some(parts[1].to_string())
                        } else {
                            Some(version_info)
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        };

        // 1. 检查 $HOME/.local/bin/uv
        if let Some(home_dir) = dirs::home_dir() {
            let local_bin_path = home_dir.join(".local").join("bin").join(uv_executable_name);
            if let Some(ver) = get_version(&local_bin_path) {
                return Ok(ver);
            }
        }

        // 2. 检查 $HOME/.cargo/bin/uv
        if let Some(home_dir) = dirs::home_dir() {
            let cargo_bin_path = home_dir.join(".cargo").join("bin").join(uv_executable_name);
            if let Some(ver) = get_version(&cargo_bin_path) {
                return Ok(ver);
            }
        }

        // 3. 再试系统 PATH
        if let Some(ver) = get_version(std::path::Path::new(uv_executable_name)) {
            return Ok(ver);
        }

        Ok("Not Installed".to_string())
    }

    /// 获取 uv 可执行文件路径（按常见安装位置优先）
    pub fn find_uv_executable() -> Option<std::path::PathBuf> {
        let uv_executable_name = Self::get_uv_executable_name();

        if cfg!(target_os = "windows") {
            if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                let path = std::path::Path::new(&local_app_data)
                    .join("Programs")
                    .join("uv")
                    .join(uv_executable_name);
                if path.exists() {
                    return Some(path);
                }
            }
        }

        if let Some(home_dir) = dirs::home_dir() {
            let local_bin_path = home_dir.join(".local").join("bin").join(uv_executable_name);
            if local_bin_path.exists() {
                return Some(local_bin_path);
            }

            let cargo_bin_path = home_dir.join(".cargo").join("bin").join(uv_executable_name);
            if cargo_bin_path.exists() {
                return Some(cargo_bin_path);
            }
        }

        let path = std::path::Path::new(uv_executable_name);
        if path.exists() {
            return Some(path.to_path_buf());
        }

        None
    }
}

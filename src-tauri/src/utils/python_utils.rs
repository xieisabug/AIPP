use std::process::Command;
use tauri::AppHandle;
use crate::utils::uv_utils::UvUtils;

/// Python 版本信息
#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct PythonInfo {
    pub python2_version: Option<String>,
    pub python3_version: Option<String>,
    pub installed_pythons: Vec<String>,
    pub need_install_python3: bool,
}

/// Python 可执行文件工具函数
pub struct PythonUtils;

impl PythonUtils {
    /// 获取 Python 可执行文件名（根据操作系统）
    fn get_python_executable_name(version: u8) -> &'static str {
        if cfg!(target_os = "windows") {
            // Windows: python.exe, python3.exe
            match version {
                3 => "python3.exe",
                _ => "python.exe",
            }
        } else {
            // Unix-like: python3, python2
            match version {
                3 => "python3",
                _ => "python2",
            }
        }
    }

    /// 运行命令获取 Python 版本
    fn get_version_from_command(exe: &str, version_flag: &str) -> Option<String> {
        match Command::new(exe).arg(version_flag).output() {
            Ok(output) if output.status.success() => {
                let version_info = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // 解析版本号，如 "Python 3.11.0" -> "3.11.0"
                let parts: Vec<&str> = version_info.split_whitespace().collect();
                if parts.len() >= 2 {
                    Some(parts[1].to_string())
                } else {
                    Some(version_info)
                }
            }
            _ => None,
        }
    }

    /// 检查 Python 2 是否安装
    pub fn check_python2(_app_handle: &AppHandle) -> Option<String> {
        tracing::info!("检查 Python 2 版本");

        // 先尝试从 uv 管理的 Python 中查找
        if let Some(versions) = Self::get_uv_python_list() {
            for version in versions {
                if version.starts_with("2.") {
                    tracing::info!("通过 uv 找到 Python 2: {}", version);
                    return Some(version);
                }
            }
        }

        // 回退到系统 PATH 检查
        let exe_names = if cfg!(target_os = "windows") {
            vec!["python2.exe", "python.exe"]
        } else {
            vec!["python2", "python"]
        };

        for exe in exe_names {
            if let Some(version) = Self::get_version_from_command(exe, "--version") {
                // 检查是否是 Python 2
                if version.starts_with("2.") {
                    tracing::info!("在系统 PATH 中找到 Python 2: {} ({})", version, exe);
                    return Some(version);
                }
            }
        }

        tracing::info!("未找到 Python 2");
        None
    }

    /// 检查 Python 3 是否安装
    pub fn check_python3(_app_handle: &AppHandle) -> Option<String> {
        tracing::info!("检查 Python 3 版本");

        // 先尝试从 uv 管理的 Python 中查找
        if let Some(versions) = Self::get_uv_python_list() {
            for version in versions {
                if version.starts_with("3.") {
                    tracing::info!("通过 uv 找到 Python 3: {}", version);
                    return Some(version);
                }
            }
        }

        // 回退到系统 PATH 检查
        let exe_names = if cfg!(target_os = "windows") {
            vec!["python3.exe", "python.exe"]
        } else {
            vec!["python3", "python"]
        };

        for exe in exe_names {
            if let Some(version) = Self::get_version_from_command(exe, "--version") {
                // 检查是否是 Python 3
                if version.starts_with("3.") {
                    tracing::info!("在系统 PATH 中找到 Python 3: {} ({})", version, exe);
                    return Some(version);
                }
            }
        }

        tracing::info!("未找到 Python 3");
        None
    }

    /// 获取 uv 管理的 Python 版本列表
    fn get_uv_python_list() -> Option<Vec<String>> {
        let uv_exe = UvUtils::find_uv_executable()?;

        match Command::new(&uv_exe).arg("python").arg("list").output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut versions = Vec::new();

                // uv python list 输出格式示例：
                // - CPython 3.11.0
                //   cpython-3.11.0-windows-x86_64-none
                // ✓ 3.11
                for line in stdout.lines() {
                    let line = line.trim();
                    if !line.contains("cpython-") {
                        continue;
                    }

                    if line.contains("<download available>") {
                        continue;
                    }

                    let mut parts = line.split_whitespace();
                    let spec = match parts.next() {
                        Some(value) => value,
                        None => continue,
                    };
                    if parts.next().is_none() {
                        // 没有安装路径，说明未安装
                        continue;
                    }

                    if let Some(start) = spec.find("cpython-") {
                        let rest = &spec[start + 8..];
                        if let Some(end) = rest.find('-') {
                            let version = rest[..end].to_string();
                            versions.push(version);
                        }
                    }
                }

                if !versions.is_empty() {
                    tracing::info!("uv 管理的 Python 版本: {:?}", versions);
                    Some(versions)
                } else {
                    None
                }
            }
            Ok(output) => {
                tracing::warn!(
                    "uv python list 命令失败: {:?}",
                    String::from_utf8_lossy(&output.stderr)
                );
                None
            }
            Err(e) => {
                tracing::warn!("执行 uv python list 失败: {}", e);
                None
            }
        }
    }

    /// 获取完整的 Python 信息
    pub fn get_python_info(app_handle: &AppHandle) -> PythonInfo {
        tracing::info!("开始获取 Python 版本信息");

        let python2_version = Self::check_python2(app_handle);
        let python3_version = Self::check_python3(app_handle);

        let installed_pythons = Self::get_uv_python_list().unwrap_or_default();

        // 如果没有 Python 3，则需要安装
        let need_install_python3 = python3_version.is_none();

        tracing::info!(
            "Python 信息 - Python2: {:?}, Python3: {:?}, 需要安装 Python3: {}",
            python2_version,
            python3_version,
            need_install_python3
        );

        PythonInfo { python2_version, python3_version, installed_pythons, need_install_python3 }
    }
}

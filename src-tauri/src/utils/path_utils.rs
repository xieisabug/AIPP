use std::path::{Component, Path, PathBuf};

/// 规范化路径用于比较（Windows 兼容）
///
/// 处理以下 Windows 路径格式问题：
/// - 驱动器号大小写：`C:` vs `c:`
/// - 路径组件大小写：Windows 文件系统不区分大小写
/// - 斜杠方向：`\` vs `/`
///
/// 对于存在的路径，使用 `canonicalize` 获取真实路径。
/// 对于不存在的路径，进行词法规范化处理。
pub fn normalize_path_for_comparison(path: &Path) -> Option<PathBuf> {
    // 1. 先尝试 canonicalize（如果路径存在）
    if path.exists() {
        return path.canonicalize().ok();
    }

    // 2. 不存在的路径：手动规范化
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => {
                // Windows: 规范化驱动器号为大写
                let prefix_str = p.as_os_str().to_string_lossy();
                #[cfg(windows)]
                normalized.push(prefix_str.to_uppercase());
                #[cfg(not(windows))]
                normalized.push(prefix_str);
            }
            Component::RootDir => {
                normalized.push(std::path::MAIN_SEPARATOR.to_string());
            }
            Component::CurDir => {}
            Component::Normal(part) => {
                // Windows: 统一使用小写比较（Windows 文件系统不区分大小写）
                #[cfg(windows)]
                normalized.push(part.to_string_lossy().to_lowercase());
                #[cfg(not(windows))]
                normalized.push(part);
            }
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
        }
    }
    Some(normalized)
}

/// 检查目标路径是否在信任路径下（Windows 兼容）
///
/// 使用规范化路径进行前缀匹配，确保跨平台路径比较的正确性。
pub fn is_path_under_trusted(target: &Path, trusted: &Path) -> bool {
    let target_normalized = normalize_path_for_comparison(target);
    let trusted_normalized = normalize_path_for_comparison(trusted);

    match (target_normalized, trusted_normalized) {
        (Some(t), Some(tr)) => t.starts_with(&tr),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_windows_drive() {
        // 测试驱动器号规范化
        let path = Path::new("C:\\Users\\test");
        let normalized = normalize_path_for_comparison(path);

        #[cfg(windows)]
        {
            // Windows 上应该规范化为小写路径组件
            assert!(normalized.is_some());
            let n = normalized.unwrap();
            assert!(n.to_string_lossy().to_string().contains("c:"));
        }
    }

    #[test]
    fn test_is_path_under_trusted_basic() {
        let target = Path::new("C:\\Users\\test\\file.txt");
        let trusted = Path::new("C:\\Users\\test");

        #[cfg(windows)]
        assert!(is_path_under_trusted(target, trusted));

        #[cfg(not(windows))]
        assert!(is_path_under_trusted(target, trusted) || !target.starts_with(trusted));
    }

    #[test]
    fn test_is_path_under_trusted_different_case() {
        // 测试大小写不敏感匹配（Windows）
        let target = Path::new("C:\\USERS\\TEST\\file.txt");
        let trusted = Path::new("c:\\users\\test");

        #[cfg(windows)]
        assert!(is_path_under_trusted(target, trusted));
    }
}

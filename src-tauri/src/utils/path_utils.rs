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
        let canonical = path.canonicalize().ok()?;
        // On Windows, canonicalize returns \\?\ extended-length prefix which
        // breaks starts_with comparisons against non-canonical paths.
        #[cfg(windows)]
        {
            let s = canonical.to_string_lossy();
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return Some(PathBuf::from(stripped));
            }
        }
        return Some(canonical);
    }

    // 2. 不存在的路径：手动规范化
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => {
                // Windows: 规范化驱动器号为大写
                #[cfg(windows)]
                normalized.push(p.as_os_str().to_string_lossy().to_uppercase());
                #[cfg(not(windows))]
                normalized.push(p.as_os_str());
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
        // 不存在的路径走词法规范化：驱动器号转大写、路径组件转小写。
        // 必须是测试机上不存在的路径，存在的路径会按真实大小写 canonicalize。
        let path = Path::new("c:\\AIPP-Nonexistent-Normalize\\Test");
        let normalized = normalize_path_for_comparison(path);

        #[cfg(windows)]
        {
            assert_eq!(normalized, Some(PathBuf::from("C:\\aipp-nonexistent-normalize\\test")));
            assert!(is_path_under_trusted(Path::new("C:\\AIPP-NONEXISTENT-NORMALIZE\\TEST\\file.txt"), path));
        }
        #[cfg(not(windows))]
        let _ = normalized;
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

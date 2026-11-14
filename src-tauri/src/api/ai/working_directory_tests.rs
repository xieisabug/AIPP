use super::*;

#[test]
fn test_working_directory_expands_only_home_path_prefix() {
    let home = std::env::temp_dir().join("用户 home");
    for (input, expected) in [
        ("~", home.clone()),
        ("~/project", home.join("project")),
        ("~/a/../project", home.join("a/../project")),
        ("~/~project/$UNEXPANDED", home.join("~project/$UNEXPANDED")),
    ] {
        assert_eq!(resolve(Some(input), true, || Some(home.clone())).unwrap(), expected);
    }
}

#[test]
fn test_working_directory_preserves_literal_path_boundaries() {
    for input in ["~other/project", "project/~/file", "$HOME/project", "%USERPROFILE%/project"] {
        assert_eq!(resolve(Some(input), true, || panic!("literal must not query home")).unwrap(), PathBuf::from(input));
    }
    let absolute = std::env::temp_dir().join("project");
    assert_eq!(resolve(absolute.to_str(), true, || None).unwrap(), absolute);
    #[cfg(windows)]
    {
        let home = PathBuf::from(r"C:\Users\测试");
        assert_eq!(resolve(Some(r"~\project"), true, || Some(home.clone())).unwrap(), home.join("project"));
        for input in [r"\\server\share\project", r"\\?\C:\project"] {
            assert_eq!(resolve(Some(input), true, || None).unwrap(), PathBuf::from(input));
        }
    }
    #[cfg(not(windows))]
    assert_eq!(resolve(Some(r"~\project"), true, || None).unwrap(), PathBuf::from(r"~\project"));
}

#[test]
fn test_working_directory_errors_when_required_home_is_unavailable() {
    for input in [None, Some("~"), Some("~/project")] {
        assert!(resolve(input, true, || None).unwrap_err().to_string().contains("系统未提供用户主目录"));
    }
    let home = std::env::temp_dir();
    assert_eq!(resolve(None, true, || Some(home.clone())).unwrap(), home);
}

#[test]
fn test_working_directory_codex_preserves_literal_tilde() {
    for input in ["~", "~/project", r"~\project"] {
        assert_eq!(resolve(Some(input), false, || panic!("literal Codex path must not query home")).unwrap(), PathBuf::from(input));
    }
}

#[cfg(unix)]
#[test]
fn test_working_directory_preserves_non_unicode_home() {
    use std::os::unix::ffi::OsStringExt;
    let home = PathBuf::from(std::ffi::OsString::from_vec(b"/home/user\xff".to_vec()));
    assert_eq!(resolve(Some("~/project"), true, || Some(home.clone())).unwrap(), home.join("project"));
}

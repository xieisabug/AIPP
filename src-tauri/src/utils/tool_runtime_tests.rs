use super::*;
use std::{fs, path::Path, process::Command};

fn fixture(dir: &Path, name: &str, version: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    #[cfg(windows)]
    {
        let path = dir.join(format!("{name}.cmd"));
        fs::write(&path, format!("@echo off\r\nif \"%1\"==\"--version\" (echo {version}) else (echo %*)\r\n")).unwrap();
        path
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo '{version}'; else echo \"$*\"; fi\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

#[test]
fn test_runtime_path_with_spaces_selects_first_runnable_and_executes_that_program() {
    let root = tempfile::tempdir().unwrap();
    for name in ["uv", "bun"] {
        let bad = root.path().join("broken runtime");
        let good = root.path().join("working runtime");
        let common = root.path().join("installer default");
        fixture(&bad, name, "unusable");
        let selected = fixture(&good, name, &format!("{name} 1.2.3"));
        fixture(&common, name, "3.4.5");
        let path = std::env::join_paths([&bad, &good]).unwrap();
        let runtime = find_runtime(name, None, &path, vec![common.clone()]).unwrap();
        assert_eq!(runtime.path, selected);
        assert_eq!(runtime.version, "1.2.3");
        let out = Command::new(runtime.path).args(["python", "install", "3"]).output().unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "python install 3");
        let managed = find_runtime(name, Some(common.clone()), &path, vec![]).unwrap();
        assert_eq!(managed.version, "3.4.5");
        let standalone = find_runtime(name, None, OsStr::new(""), vec![common]).unwrap();
        assert_eq!(standalone.path, managed.path);
    }
}

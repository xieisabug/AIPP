use super::*;

fn manager_fixture(dir: &Path, manager: Manager) -> PathBuf {
    #[cfg(windows)]
    let (name, script) = ("manager.cmd", "@echo off\r\nif \"%1\"==\"root\" (echo %TEST_MODULES%) else if \"%2\"==\"ls\" (echo %TEST_PROJECT% node_modules) else (echo %TEST_BIN%)\r\n");
    #[cfg(unix)]
    let (name, script) = ("manager", "#!/bin/sh\nif [ \"$1\" = root ]; then echo \"$TEST_MODULES\"; elif [ \"$2\" = ls ]; then echo \"$TEST_PROJECT node_modules\"; else echo \"$TEST_BIN\"; fi\n");
    let folder = dir.join(manager.name());
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(name);
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[tokio::test]
async fn test_package_update_matches_actual_launcher_for_npm_pnpm_and_bun() {
    let root = tempfile::tempdir().unwrap();
    for manager in [Manager::Npm, Manager::Pnpm, Manager::Bun] {
        let executable = manager_fixture(root.path(), manager);
        let prefix = root.path().join(manager.name()).join("global with spaces");
        let bin = if manager == Manager::Npm && cfg!(windows) { prefix.clone() } else { prefix.join("bin") };
        let project = match manager {
            Manager::Npm if !cfg!(windows) => prefix.join("lib"),
            Manager::Npm => prefix.clone(),
            Manager::Pnpm => prefix.join("global/5"),
            Manager::Bun => prefix.join("install/global"),
        };
        let modules = project.join("node_modules");
        let env = HashMap::from([
            ("TEST_BIN".into(), if manager == Manager::Npm { &prefix } else { &bin }.display().to_string()),
            ("TEST_PROJECT".into(), project.display().to_string()),
            ("TEST_MODULES".into(), modules.display().to_string()),
        ]);
        fs::create_dir_all(&bin).unwrap();
        for (cli, package) in [("codex", "@openai/codex"), ("claude-code-acp", "@zed-industries/claude-code-acp")] {
            let pkg = modules.join(package);
            fs::create_dir_all(&pkg).unwrap();
            let manifest = serde_json::json!({"name": package, "bin": {(cli): "main.js"}});
            fs::write(pkg.join("package.json"), manifest.to_string()).unwrap();
            fs::write(pkg.join("main.js"), "// entrypoint").unwrap();
            #[cfg(windows)]
            let launcher = if manager == Manager::Bun {
                let launcher = bin.join(format!("{cli}.exe"));
                fs::write(&launcher, "shim").unwrap();
                let entry = format!("install/global/node_modules/{package}/main.js\"\0node");
                let bytes: Vec<u8> = entry.encode_utf16().flat_map(u16::to_le_bytes).collect();
                fs::write(launcher.with_extension("bunx"), bytes).unwrap();
                launcher
            } else {
                let launcher = bin.join(format!("{cli}.cmd"));
                let rel = if manager == Manager::Npm { "node_modules" } else { "../global/5/node_modules" };
                fs::write(&launcher, format!("@node \"%~dp0/{rel}/{package}/main.js\" %*")).unwrap();
                launcher
            };
            #[cfg(unix)]
            let launcher = {
                let launcher = bin.join(cli);
                std::os::unix::fs::symlink(pkg.join("main.js"), &launcher).unwrap();
                launcher
            };
            let updater = identify(manager, executable.clone(), &launcher, cli, package, &env).await.unwrap();
            let (command, _) = updater.command(package, &env);
            assert_eq!(command.get_program(), executable.as_os_str());
            let args = command.get_args().collect::<Vec<_>>();
            let action = if manager == Manager::Npm { "install" } else { "add" };
            assert_eq!(args[0], action);
            assert_eq!(args[1], "-g");
            assert_eq!(args[2], std::ffi::OsStr::new(&format!("{package}@latest")));

            let first = root.path().join(executable.file_name().unwrap());
            #[cfg(windows)]
            fs::write(&first, format!("@echo {}\r\n", root.path().display())).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::write(&first, format!("#!/bin/sh\necho '{}'\n", root.path().display())).unwrap();
                fs::set_permissions(&first, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let candidates = vec![first.clone(), first, executable.clone()];
            let matched = identify_candidates(manager, candidates, &launcher, cli, package, &env).await.unwrap();
            assert_eq!(matched.executable, executable);

            // The package is registered but this selected copy has a different origin.
            let other = root.path().join(launcher.file_name().unwrap());
            fs::write(&other, "unmanaged").unwrap();
            assert!(identify(manager, executable.clone(), &other, cli, package, &env).await.is_none());
            // Even a same-name command in the manager's bin directory must point at this package.
            fs::remove_file(&launcher).unwrap();
            fs::write(&launcher, "standalone").unwrap();
            if launcher.with_extension("bunx").exists() { fs::remove_file(launcher.with_extension("bunx")).unwrap(); }
            assert!(identify(manager, executable.clone(), &launcher, cli, package, &env).await.is_none());
        }
    }
}

#[test]
fn test_package_manager_environment_excludes_provider_credentials() {
    let original = HashMap::from([
        ("PATH".into(), "node-bin".into()), ("PNPM_HOME".into(), "pnpm-bin".into()),
        ("NPM_CONFIG_PREFIX".into(), "global".into()), ("OPENAI_API_KEY".into(), "provider-only".into()),
        ("ANTHROPIC_AUTH_TOKEN".into(), "provider-only".into()), ("NODE_OPTIONS".into(), "--require ./provider.cjs".into()),
    ]);
    let filtered = manager_environment(&original);
    assert_eq!(filtered.len(), 3);
    assert!(filtered.contains_key("PATH") && filtered.contains_key("PNPM_HOME") && filtered.contains_key("NPM_CONFIG_PREFIX"));
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires a local pnpm installation of Codex and a writable pnpm home for pnpm bin -g"]
async fn test_installed_pnpm_codex_owner_with_minimal_path() {
    let env = HashMap::from([("PATH".into(), String::new())]);
    let (program, version) = official_cli::detect("codex", "codex", &env).await.unwrap();
    let launcher = program.launcher_path();
    let paths = official_cli::manager_candidates("pnpm", launcher.parent().unwrap(), &env);
    let owner = identify_candidates(Manager::Pnpm, paths, launcher, "codex", "@openai/codex", &env).await.unwrap();
    println!("Codex {version}, launcher {}, verified pnpm {}", launcher.display(), owner.executable.display());
}

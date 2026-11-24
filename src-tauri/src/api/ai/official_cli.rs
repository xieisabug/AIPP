use super::agent_driver::native_agents::env::tool_dirs;
use crate::utils::background_process::background_command;
use std::{env, path::{Path, PathBuf}, process::Command};

pub(crate) async fn resolve(name: &str) -> Result<PathBuf, String> {
    // PATH may include network drives. Keep filesystem discovery off Tokio's I/O workers.
    let command = name.to_owned();
    tokio::task::spawn_blocking(move || resolve_path(&command)).await
        .map_err(|error| format!("查找 CLI {name} 失败：{error}"))?
}

/// An explicit path selects exactly that file. For bare names, respect PATH order,
/// then check installer directories that a desktop process may not have inherited.
fn resolve_path(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    if path.is_absolute() || path.components().count() > 1 {
        let path = std::path::absolute(path).map_err(|e| format!("解析 {name} 失败：{e}"))?;
        return path.is_file().then_some(path).ok_or_else(|| format!("找不到配置的 CLI 文件：{name}"));
    }

    let mut dirs: Vec<PathBuf> = env::var_os("PATH").map(|paths| env::split_paths(&paths)
        .filter(|dir| dir.is_absolute()).collect()).unwrap_or_default();
    let mut installed_dirs = tool_dirs(name);
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        installed_dirs.push(local.join("Microsoft/WinGet/Links"));
    }
    if let Some(pnpm) = env::var_os("PNPM_HOME") {
        installed_dirs.insert(0, PathBuf::from(pnpm));
    }
    for dir in installed_dirs {
        if dir.is_absolute() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    find_in_dirs(name, &dirs)
}

pub(super) fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    #[cfg(windows)]
    let extensions: &[&str] = if Path::new(name).extension().is_none() {
        &[".exe", ".com", ".cmd", ".bat", ".ps1"]
    } else {
        &[""]
    };
    #[cfg(not(windows))]
    let extensions = [""];

    for dir in dirs {
        for extension in extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if candidate.is_file() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if !candidate.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
                        continue;
                    }
                }
                return Ok(candidate);
            }
        }
    }
    let searched: Vec<_> = dirs.iter().map(|dir| dir.display().to_string()).collect();
    Err(format!("找不到命令 {name}。已检查目录：{}", searched.join("；")))
}

pub(crate) fn command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        let ext = path.extension().unwrap_or_default();
        if ext.eq_ignore_ascii_case("ps1") {
            let mut command = background_command("pwsh.exe");
            command.args(["-NoProfile", "-File"]).arg(path);
            supply_node_path(&mut command);
            return command;
        }
        // std::process handles batch launchers, including paths with spaces.
        let mut command = background_command(path);
        if ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat") {
            supply_node_path(&mut command);
        }
        command
    }
    #[cfg(not(windows))]
    background_command(path)
}

// npm/pnpm launchers may be visible to a desktop process while node.exe is not.
#[cfg(windows)]
fn supply_node_path(command: &mut Command) {
    use super::agent_driver::native_agents::env::node_locations;
    let inherited = env::var_os("PATH").unwrap_or_default();
    if env::split_paths(&inherited).any(|dir| dir.is_absolute() && dir.join("node.exe").is_file()) {
        return;
    }
    let managed = ["NVM_SYMLINK", "FNM_MULTISHELL_PATH"].iter()
        .filter_map(env::var_os).map(PathBuf::from)
        .find(|dir| dir.is_absolute() && dir.join("node.exe").is_file());
    let node_dir = managed.or_else(|| node_locations(&inherited).into_iter()
        .map(|(path, _)| path).find(|path| path.is_absolute() && path.is_file())
        .and_then(|path| path.parent().map(Path::to_owned)));
    if let Some(dir) = node_dir {
        if let Ok(path) = env::join_paths(std::iter::once(dir).chain(env::split_paths(&inherited))) {
            command.env("PATH", path);
        }
    }
}

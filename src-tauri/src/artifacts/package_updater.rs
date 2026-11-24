use super::AcpLibraryInfo;
use crate::api::ai::official_cli;
use std::{collections::HashMap, fs, path::{Path, PathBuf}, process::Command, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Manager { Npm, Pnpm, Bun }

impl Manager {
    fn name(self) -> &'static str {
        match self { Self::Npm => "npm", Self::Pnpm => "pnpm", Self::Bun => "bun" }
    }
}

#[derive(Clone)]
pub(super) struct PackageUpdater {
    manager: Manager,
    executable: PathBuf,
    pub(super) launcher: PathBuf,
}

/// Package managers only need the provider's executable search paths and global-install
/// settings. Proxy variables for the optional update request are applied by the caller.
fn manager_environment(environment: &HashMap<String, String>) -> HashMap<String, String> {
    const KEYS: &[&str] = &["PATH", "PNPM_HOME", "BUN_INSTALL", "BUN_INSTALL_BIN",
        "BUN_INSTALL_GLOBAL_DIR", "NVM_HOME", "NVM_SYMLINK", "FNM_DIR", "FNM_MULTISHELL_PATH",
        "VOLTA_HOME", "NPM_CONFIG_PREFIX", "NPM_CONFIG_GLOBAL_DIR", "NPM_CONFIG_GLOBAL_BIN_DIR",
        "NPM_CONFIG_USERCONFIG", "XDG_CONFIG_HOME"];
    environment.iter().filter(|(key, _)| KEYS.iter().any(|name| key.eq_ignore_ascii_case(name)))
        .map(|(key, value)| (key.clone(), value.clone())).collect()
}

fn include_pnpm_bin(manager: Manager, launcher: &Path, env: &mut HashMap<String, String>) {
    if manager != Manager::Pnpm { return; }
    let Some(bin) = launcher.parent() else { return };
    // pnpm rejects even "bin -g" when its global bin directory isn't on PATH.
    // A desktop process often discovered this launcher through PNPM_HOME instead.
    let key = env.keys().find(|key| if cfg!(windows) { key.eq_ignore_ascii_case("PATH") } else { *key == "PATH" })
        .cloned().unwrap_or_else(|| "PATH".into());
    let original = env.get(&key).map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    let mut dirs: Vec<_> = std::env::split_paths(&original).filter(|dir| !dir.as_os_str().is_empty()).collect();
    if !dirs.iter().any(|dir| dir == bin) {
        dirs.push(bin.to_owned());
        if let Ok(path) = std::env::join_paths(dirs) { env.insert(key, path.to_string_lossy().into_owned()); }
    }
}

impl PackageUpdater {
    pub(super) fn command(&self, package: &str, env: &HashMap<String, String>) -> (Command, String) {
        let mut env = manager_environment(env);
        include_pnpm_bin(self.manager, &self.launcher, &mut env);
        let mut command = official_cli::command(&self.executable, &env);
        let action = if self.manager == Manager::Npm { "install" } else { "add" };
        let spec = format!("{package}@latest");
        command.args([action, "-g", &spec]);
        let label = format!("{} {action} -g {spec}（{}）", self.executable.display(), self.launcher.display());
        (command, label)
    }
}

async fn manager_output(program: &Path, args: &[&str], env: &HashMap<String, String>) -> Option<String> {
    let mut command = tokio::process::Command::from(official_cli::command(program, env));
    let output = tokio::time::timeout(Duration::from_secs(5), command.args(args).kill_on_drop(true).output())
        .await.ok()?.ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Match the package manifest's bin entry against the *selected* launcher, not another CLI
/// with the same name on PATH. npm creates symlinks on Unix. pnpm and Windows npm use shims.
fn launcher_matches(launcher: &Path, modules: &Path, cli: &str, package: &str, manager: Manager) -> bool {
    let Some(dir) = launcher.parent() else { return false };
    let root = modules.join(package);
    let Some(manifest) = fs::read(root.join("package.json")).ok()
        .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok()) else { return false };
    if manifest["name"].as_str() != Some(package) { return false; }
    let Some(bin) = manifest["bin"][cli].as_str().or_else(|| manifest["bin"].as_str()) else { return false };
    let target = root.join(bin);
    if same_file(launcher, &target) { return true; }

    if cfg!(windows) && launcher.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("exe")) {
        if manager != Manager::Bun { return false; }
        // Bun's .bunx companion starts with the UTF-16LE entrypoint relative to the
        // bin directory's parent, followed by a quote, NUL and interpreter metadata.
        let Some(bytes) = fs::read(launcher.with_extension("bunx")).ok() else { return false };
        let path = bytes.chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]]))
            .take_while(|w| *w != 0).collect::<Vec<_>>();
        let Ok(path) = String::from_utf16(&path) else { return false };
        return dir.parent().is_some_and(|parent| same_file(&parent.join(path.trim_end_matches('"')), &target));
    }
    // Only inspect the final quoted entrypoint argument before the forwarded arguments.
    if fs::metadata(launcher).map_or(true, |m| m.len() > 64 * 1024) { return false; }
    let Ok(text) = fs::read_to_string(launcher) else { return false };
    text.lines().any(|line| {
        let Some(invocation) = [" %*", " $args", " \"$@\""].iter()
            .find_map(|suffix| line.trim_end().strip_suffix(suffix)) else { return false };
        let Some((_, arg)) = invocation.trim_end().strip_suffix('"').and_then(|s| s.rsplit_once('"')) else { return false };
        let Some(rel) = ["%dp0%", "%~dp0", "$basedir"].iter()
            .find_map(|prefix| arg.strip_prefix(prefix)) else { return false };
        same_file(&dir.join(rel.trim_start_matches(['/', '\\'])), &target)
    })
}

async fn identify(
    manager: Manager, program: PathBuf, launcher: &Path, cli: &str, package: &str, env: &HashMap<String, String>,
) -> Option<PackageUpdater> {
    let mut env = env.clone();
    include_pnpm_bin(manager, launcher, &mut env);
    let bin_args = match manager {
        Manager::Npm => &["prefix", "-g"][..],
        Manager::Pnpm => &["bin", "-g"],
        Manager::Bun => &["pm", "bin", "-g"],
    };
    let prefix = PathBuf::from(manager_output(&program, bin_args, &env).await?);
    if !prefix.is_absolute() { return None; }
    let bin = if manager == Manager::Npm && !cfg!(windows) { prefix.join("bin") } else { prefix.clone() };
    if !same_file(launcher.parent()?, &bin) { return None; }

    let modules = match manager {
        Manager::Npm if cfg!(windows) => prefix.join("node_modules"),
        Manager::Npm => prefix.join("lib/node_modules"),
        Manager::Pnpm => PathBuf::from(manager_output(&program, &["root", "-g"], &env).await?),
        Manager::Bun => {
            let list = manager_output(&program, &["pm", "ls", "-g"], &env).await?;
            let (project, _) = list.lines().find_map(|line| line.rsplit_once(" node_modules"))?;
            Path::new(project).join("node_modules")
        }
    };
    if !modules.is_absolute() || !launcher_matches(launcher, &modules, cli, package, manager) { return None; }
    Some(PackageUpdater { manager, executable: program, launcher: launcher.to_owned() })
}

pub(super) async fn attach_package_updater(
    app: &tauri::AppHandle,
    info: &mut AcpLibraryInfo,
    launcher: &Path,
    env: &HashMap<String, String>,
) {
    let cli = info.cli_command.as_str();
    let package = info.package_name.as_str();
    let manager_env = manager_environment(env);
    let env = &manager_env;
    let owners = futures::future::join_all([Manager::Npm, Manager::Pnpm, Manager::Bun].into_iter().map(|manager| async move {
        let (app, dir, env_copy) = (app.clone(), launcher.parent()?.to_owned(), env.clone());
        let programs = tokio::task::spawn_blocking(move || {
            let mut found = official_cli::manager_candidates(manager.name(), &dir, &env_copy);
            if manager == Manager::Bun {
                if let Ok(managed) = crate::utils::bun_utils::BunUtils::get_bun_executable(&app) {
                    if !found.contains(&managed) { found.push(managed); }
                }
            }
            found
        }).await.ok()?;
        identify_candidates(manager, programs, launcher, cli, package, env).await
    })).await;
    let mut verified = owners.into_iter().flatten();
    let first = verified.next();
    info.package_updater = first.filter(|_| verified.next().is_none());
    info.can_update = info.package_updater.is_some();
    info.update_hint = (!info.can_update).then(||
        "未能确认当前 CLI 的安装来源，请使用原安装方式手动更新。".into()
    );
}

async fn identify_candidates(
    manager: Manager, programs: Vec<PathBuf>, launcher: &Path, cli: &str, package: &str, env: &HashMap<String, String>,
) -> Option<PackageUpdater> {
    let mut seen = std::collections::HashSet::new();
    for program in programs {
        let identity = fs::canonicalize(&program).unwrap_or_else(|_| program.clone());
        if !seen.insert(identity) { continue; }
        if let Some(owner) = identify(manager, program, launcher, cli, package, env).await {
            return Some(owner);
        };
    }
    None
}

#[cfg(test)]
#[path = "package_updater_tests.rs"]
mod tests;

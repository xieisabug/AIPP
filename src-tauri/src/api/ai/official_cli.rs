use super::agent_driver::native_agents::env::{tool_dirs, version_token};
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

    find_in_dirs(name, &search_dirs(name))
}

fn search_dirs(name: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env::var_os("PATH").map(|paths| env::split_paths(&paths)
        .filter(|dir| dir.is_absolute()).collect()).unwrap_or_default();
    let mut installed_dirs = tool_dirs(name);
    #[cfg(windows)]
    {
        if let Some(local) = dirs::data_local_dir() {
            installed_dirs.extend(["Microsoft/WinGet/Links", "Programs/nodejs", "Volta/bin"].map(|dir| local.join(dir)));
        }
        if let Some(home) = dirs::home_dir() {
            installed_dirs.push(home.join("scoop/shims"));
        }
    }
    if let Some(pnpm) = env::var_os("PNPM_HOME") {
        installed_dirs.insert(0, PathBuf::from(pnpm));
    }
    for dir in installed_dirs {
        if dir.is_absolute() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

pub(super) fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    if let Some(path) = candidates(name, dirs).next() {
        return Ok(path);
    }
    let searched: Vec<_> = dirs.iter().map(|dir| dir.display().to_string()).collect();
    Err(format!("找不到命令 {name}。已检查目录：{}", searched.join("；")))
}

fn candidates<'a>(name: &'a str, dirs: &'a [PathBuf]) -> impl Iterator<Item = PathBuf> + 'a {
    #[cfg(windows)]
    let extensions: &[&str] = if Path::new(name).extension().is_none() {
        &[".exe", ".com", ".cmd", ".bat", ".ps1"]
    } else {
        &[""]
    };
    #[cfg(not(windows))]
    let extensions: &[&str] = &[""];

    dirs.iter().flat_map(move |dir| extensions.iter().map(move |ext| dir.join(format!("{name}{ext}"))))
        .filter(|path| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            path.is_file()
        })
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

pub(crate) struct Program {
    pub path: PathBuf,
    #[cfg(windows)]
    package: Option<codex_windows::Package>,
}

impl Program {
    fn from_path(path: PathBuf) -> Self {
        Self {
            path,
            #[cfg(windows)]
            package: None,
        }
    }

    pub(crate) fn command(&self) -> Command {
        #[cfg(windows)]
        if let Some(package) = &self.package {
            let mut cmd = command(&self.path);
            package.apply_environment(&mut cmd);
            return cmd;
        }
        command(&self.path)
    }
}

#[derive(Debug)]
pub(crate) struct ProbeError {
    pub path: Option<PathBuf>,
    pub message: String,
}

pub(crate) async fn detect(kind: &str, name: &str) -> Result<(Program, String), ProbeError> {
    #[cfg(windows)]
    if kind == "codex" && name.eq_ignore_ascii_case("codex") {
        return codex_windows::detect().await;
    }
    let _ = kind;
    let path = resolve(name).await.map_err(|message| ProbeError { path: None, message })?;
    let program = Program::from_path(path);
    match probe_version(&program).await {
        Ok(version) => Ok((program, version)),
        Err(message) => Err(ProbeError { path: Some(program.path), message }),
    }
}

pub(crate) async fn codex_for_session(name: &str) -> Result<Program, String> {
    #[cfg(windows)]
    if name.eq_ignore_ascii_case("codex") {
        return codex_windows::detect().await.map(|(program, _)| program).map_err(|e| e.message);
    }
    resolve(name).await.map(Program::from_path)
}

#[cfg(windows)]
#[path = "official_cli_codex_windows.rs"]
pub(super) mod codex_windows;

async fn probe_version(program: &Program) -> Result<String, String> {
    let path = &program.path;
    let mut command = tokio::process::Command::from(program.command());
    command.arg("--version").kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output()).await
        .map_err(|_| format!("{} --version 超过 10 秒未返回", path.display()))?
        .map_err(|error| format!("无法运行 {} --version：{error}", path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = [stderr.trim(), stdout.trim()].into_iter().find(|s| !s.is_empty())
            .unwrap_or("没有诊断输出").chars().take(1024).collect::<String>();
        return Err(format!("{} --version 失败（{}）：{detail}", path.display(), output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    version_token(&stdout).or_else(|| {
        let trimmed = stdout.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }).ok_or_else(|| format!("{} --version 没有返回版本信息", path.display()))
}

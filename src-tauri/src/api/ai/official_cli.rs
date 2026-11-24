use super::agent_driver::native_agents::env::{tool_dirs, version_token};
use crate::utils::background_process::background_command;
use std::{collections::HashMap, env, ffi::OsString, path::{Path, PathBuf}, process::Command};

pub(crate) async fn resolve(name: &str, environment: &HashMap<String, String>) -> Result<PathBuf, String> {
    // PATH may include network drives. Keep filesystem discovery off Tokio's I/O workers.
    let command = name.to_owned();
    let environment = environment.clone();
    tokio::task::spawn_blocking(move || resolve_path(&command, &environment)).await
        .map_err(|error| format!("查找 CLI {name} 失败：{error}"))?
}

/// An explicit path selects exactly that file. For bare names, respect PATH order,
/// then check installer directories that a desktop process may not have inherited.
fn resolve_path(name: &str, environment: &HashMap<String, String>) -> Result<PathBuf, String> {
    let path = Path::new(name);
    if path.is_absolute() || path.components().count() > 1 {
        let path = std::path::absolute(path).map_err(|e| format!("解析 {name} 失败：{e}"))?;
        return path.is_file().then_some(path).ok_or_else(|| format!("找不到配置的 CLI 文件：{name}"));
    }

    find_in_dirs(name, &search_dirs(name, environment))
}

pub(super) fn env_value(environment: &HashMap<String, String>, key: &str) -> Option<OsString> {
    let configured = environment.get(key);
    #[cfg(windows)]
    let configured = configured.or_else(|| environment.iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key)).map(|(_, value)| value));
    configured.map(OsString::from).or_else(|| env::var_os(key))
}

fn search_dirs(name: &str, environment: &HashMap<String, String>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env_value(environment, "PATH").map(|paths| env::split_paths(&paths)
        .filter(|dir| dir.is_absolute()).collect()).unwrap_or_default();
    let mut installed_dirs = tool_dirs(name);
    #[cfg(windows)]
    {
        if matches!(name, "npm" | "pnpm") {
            // npm.cmd (and Corepack's pnpm.cmd) commonly live beside the selected node.exe.
            let mut node_dirs: Vec<_> = ["NVM_SYMLINK", "FNM_MULTISHELL_PATH"].into_iter()
                .filter_map(|key| env_value(environment, key)).map(PathBuf::from).collect();
            let path = env_value(environment, "PATH").unwrap_or_default();
            node_dirs.extend(super::agent_driver::native_agents::env::node_locations(&path).into_iter()
                .filter_map(|(file, _)| file.parent().map(Path::to_owned)));
            node_dirs.append(&mut installed_dirs);
            installed_dirs = node_dirs;
        }
        if let Some(local) = dirs::data_local_dir() {
            installed_dirs.extend(["Microsoft/WinGet/Links", "Programs/nodejs", "Volta/bin"].map(|dir| local.join(dir)));
        }
        if let Some(home) = dirs::home_dir() {
            installed_dirs.push(home.join("scoop/shims"));
        }
    }
    // Bun permits the global launcher directory and runtime directory to be separate.
    if let Some(root) = env_value(environment, "BUN_INSTALL") {
        installed_dirs.insert(0, PathBuf::from(root).join("bin"));
    }
    if let Some(dir) = env_value(environment, "BUN_INSTALL_BIN") {
        installed_dirs.insert(0, PathBuf::from(dir));
    }
    if let Some(pnpm) = env_value(environment, "PNPM_HOME") {
        installed_dirs.insert(0, PathBuf::from(pnpm));
    }
    for dir in installed_dirs {
        if dir.is_absolute() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// A package manager beside the selected launcher may own it even when another
/// installation is first on PATH. Return each candidate for ownership verification.
pub(crate) fn manager_candidates(name: &str, launcher_dir: &Path, environment: &HashMap<String, String>) -> Vec<PathBuf> {
    let mut dirs = vec![launcher_dir.to_owned()];
    dirs.extend(search_dirs(name, environment));
    let mut seen = std::collections::HashSet::new();
    candidates(name, &dirs).filter(|path| {
        #[cfg(windows)]
        let key = path.as_os_str().to_ascii_lowercase();
        #[cfg(not(windows))]
        let key = path.as_os_str().to_owned();
        seen.insert(key)
    }).collect()
}

pub(super) fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    if let Some(path) = candidates(name, dirs).next() {
        return Ok(path);
    }
    Err(missing_command(name, dirs))
}

fn missing_command(name: &str, dirs: &[PathBuf]) -> String {
    let searched: Vec<_> = dirs.iter().map(|dir| dir.display().to_string()).collect();
    format!("找不到命令 {name}。已检查目录：{}", searched.join("；"))
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

pub(crate) fn command(path: &Path, environment: &HashMap<String, String>) -> Command {
    #[cfg(windows)]
    {
        let ext = path.extension().unwrap_or_default();
        let mut command = if ext.eq_ignore_ascii_case("ps1") {
            let mut shell = background_command(powershell(environment));
            shell.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"]).arg(path);
            shell
        } else {
            // std::process handles batch launchers, including paths with spaces.
            background_command(path)
        };
        command.envs(environment);
        // Bun generates a native .exe dispatcher and a matching .bunx file that
        // specifies its interpreter. A Node shebang also needs node.exe on PATH.
        let bun_shim = ext.eq_ignore_ascii_case("exe") && path.with_extension("bunx").is_file();
        if bun_shim || ["cmd", "bat", "ps1"].iter().any(|item| ext.eq_ignore_ascii_case(item)) {
            supply_launcher_runtimes(&mut command, path, environment, bun_shim);
        }
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = background_command(path);
        command.envs(environment);
        command
    }
}

#[cfg(windows)]
fn powershell(environment: &HashMap<String, String>) -> PathBuf {
    let path = env_value(environment, "PATH").unwrap_or_default();
    let dirs: Vec<_> = env::split_paths(&path).filter(|dir| dir.is_absolute()).collect();
    for shell in ["pwsh.exe", "powershell.exe"] {
        if let Ok(file) = find_in_dirs(shell, &dirs) {
            return file;
        }
    }
    env::var_os("SystemRoot").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe")
}

#[cfg(windows)]
fn supply_launcher_runtimes(command: &mut Command, launcher: &Path, environment: &HashMap<String, String>, bun_shim: bool) {
    use super::agent_driver::native_agents::env::node_locations;
    let inherited = env_value(environment, "PATH").unwrap_or_default();
    let mut dirs: Vec<_> = env::split_paths(&inherited).collect();
    let initial_count = dirs.len();
    if !dirs.iter().any(|dir| dir.is_absolute() && dir.join("node.exe").is_file()) {
        let managed = ["NVM_SYMLINK", "FNM_MULTISHELL_PATH"].iter()
            .filter_map(|key| env_value(environment, key)).map(PathBuf::from)
            .find(|dir| dir.is_absolute() && dir.join("node.exe").is_file());
        let node = managed.or_else(|| node_locations(&inherited).into_iter()
            .map(|(path, _)| path).find(|path| path.is_absolute() && path.is_file())
            .and_then(|path| path.parent().map(Path::to_owned)));
        dirs.extend(node);
    }
    if bun_shim && !dirs.iter().any(|dir| dir.is_absolute() && dir.join("bun.exe").is_file()) {
        let bin = launcher.parent().map(Path::to_owned).into_iter().chain(search_dirs("bun", environment))
            .chain(crate::utils::bun_utils::BunUtils::managed_bin().map(Path::to_owned))
            .find(|dir| dir.is_absolute() && dir.join("bun.exe").is_file());
        dirs.extend(bin);
    }
    if dirs.len() != initial_count {
        if let Ok(path) = env::join_paths(dirs) {
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
    pub(crate) fn launcher_path(&self) -> &Path {
        #[cfg(windows)]
        if let Some(package) = &self.package {
            return &package.launcher;
        }
        &self.path
    }

    fn from_path(path: PathBuf) -> Self {
        Self {
            path,
            #[cfg(windows)]
            package: None,
        }
    }

    pub(crate) fn command(&self, environment: &HashMap<String, String>) -> Command {
        #[cfg(windows)]
        if let Some(package) = &self.package {
            let mut cmd = command(&self.path, environment);
            package.apply_environment(&mut cmd);
            return cmd;
        }
        command(&self.path, environment)
    }
}

#[derive(Debug)]
pub(crate) struct ProbeError {
    pub path: Option<PathBuf>,
    pub message: String,
}

#[cfg(windows)]
fn automatic_preset(kind: &str, name: &str) -> bool {
    matches!(kind, "codex" | "claude" | "kimi" | "gemini" | "claude-code-acp") && name.eq_ignore_ascii_case(kind)
}

#[cfg(windows)]
async fn preset_candidates(name: &str, environment: &HashMap<String, String>) -> Result<Vec<Program>, ProbeError> {
    let kind = name.to_owned();
    let env = environment.clone();
    tokio::task::spawn_blocking(move || {
        let dirs = search_dirs(&kind, &env);
        if kind == "codex" {
            return codex_windows::discover(&dirs);
        }
        let programs: Vec<_> = candidates(&kind, &dirs).map(Program::from_path).collect();
        if programs.is_empty() {
            return Err(ProbeError { path: None, message: missing_command(&kind, &dirs) });
        }
        Ok(programs)
    }).await.map_err(|error| ProbeError { path: None, message: format!("查找 {name} 失败：{error}") })?
}

pub(crate) async fn detect(kind: &str, name: &str, environment: &HashMap<String, String>) -> Result<(Program, String), ProbeError> {
    #[cfg(windows)]
    if automatic_preset(kind, name) {
        let programs = preset_candidates(kind, environment).await?;
        return first_working(kind, programs, environment).await;
    }
    let path = resolve(name, environment).await.map_err(|message| ProbeError { path: None, message })?;
    first_working(kind, vec![Program::from_path(path)], environment).await
}

pub(crate) async fn for_session(kind: &str, name: &str, environment: &HashMap<String, String>) -> Result<Program, String> {
    #[cfg(windows)]
    if automatic_preset(kind, name) {
        let mut programs = preset_candidates(kind, environment).await.map_err(|e| e.message)?;
        // With a single installation there is no ambiguity. Let the session itself run its handshake.
        if programs.len() == 1 {
            return Ok(programs.remove(0));
        }
        return first_working(kind, programs, environment).await.map(|(program, _)| program).map_err(|e| e.message);
    }
    // Custom ACP servers can speak the protocol without implementing a --version option.
    let _ = kind;
    resolve(name, environment).await.map(Program::from_path)
}

pub(super) async fn first_working(kind: &str, programs: Vec<Program>, environment: &HashMap<String, String>) -> Result<(Program, String), ProbeError> {
    let first = programs.first().map(|p| p.path.clone());
    let mut errors = Vec::new();
    for program in programs {
        let result = if kind == "claude-code-acp" {
            super::acp_adapter_probe::probe_version(&program.path, environment).await
        } else {
            probe_version(kind, &program, environment).await
        };
        match result {
            Ok(version) if !matches!(kind, "codex" | "claude" | "kimi" | "gemini" | "claude-code-acp") || semver::Version::parse(&version).is_ok() => return Ok((program, version)),
            Ok(_) => errors.push(format!("{} --version 未返回 {} 版本号", program.path.display(), kind)),
            Err(error) => errors.push(error),
        }
    }
    Err(ProbeError { path: first, message: errors.join("\n") })
}

#[cfg(windows)]
#[path = "official_cli_codex_windows.rs"]
pub(super) mod codex_windows;

async fn probe_version(kind: &str, program: &Program, environment: &HashMap<String, String>) -> Result<String, String> {
    let path = &program.path;
    let command = if matches!(kind, "codex" | "claude" | "kimi") {
        program.command(environment)
    } else {
        super::acp::build_acp_command("", path, &[], environment)
    };
    let mut command = tokio::process::Command::from(command);
    command.arg("--version").kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output()).await
        .map_err(|_| format!("{} --version 超过 10 秒未返回", path.display()))?
        .map_err(|error| format!("无法运行 {} --version：{error}", path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let diagnostics = super::agent_driver::failure::AgentDiagnosticBuffer::for_environment(environment);
        let detail = [stderr.trim(), stdout.trim()].into_iter().find(|s| !s.is_empty())
            .unwrap_or("没有诊断输出");
        let detail: String = diagnostics.sanitize(detail).chars().take(1024).collect();
        return Err(format!("{} --version 失败（{}）：{detail}", path.display(), output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let diagnostics = super::agent_driver::failure::AgentDiagnosticBuffer::for_environment(environment);
    let stdout = diagnostics.sanitize(&stdout);
    version_token(&stdout).or_else(|| {
        let trimmed = stdout.trim();
        (!trimmed.is_empty()).then(|| trimmed.chars().take(1024).collect())
    }).ok_or_else(|| format!("{} --version 没有返回版本信息", path.display()))
}

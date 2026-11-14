//! Provider settings support for native agents: find the Node.js and agent programs a
//! chat session would launch, compare them with the latest published release, and run
//! the agent's own updater. CLI detection goes through the bridge's launch() resolution,
//! so the reported path and version are exactly what sessions start.
use super::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;

pub(crate) const NODE_MINIMUM: &str = "22.19.0";
const CURSOR_SDK_PACKAGE: &str = "@cursor/sdk";
#[cfg(windows)]
const NODE_EXE: &str = "node.exe";
#[cfg(not(windows))]
const NODE_EXE: &str = "node";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LatestSource { Npm(&'static str), KiroManifest, AntigravityChangelog }

#[derive(Clone, Copy)]
pub(crate) struct ToolSpec {
    pub display_name: &'static str,
    pub command: &'static str,
    pub latest: LatestSource,
    pub update_args: &'static [&'static str],
    /// (label, command, platform: "windows" | "unix" | "")
    install: &'static [(&'static str, &'static str, &'static str)],
    docs_url: &'static str,
    recommended: Option<&'static str>,
}

pub(crate) fn spec(driver: &str) -> Option<ToolSpec> {
    Some(match driver {
        "pi" => ToolSpec { display_name: "Pi", command: "pi", latest: LatestSource::Npm("@earendil-works/pi-coding-agent"),
            update_args: &["update", "self"], docs_url: "https://pi.dev", recommended: None,
            install: &[("npm", "npm install -g @earendil-works/pi-coding-agent", "")] },
        "opencode" => ToolSpec { display_name: "OpenCode", command: "opencode", latest: LatestSource::Npm("opencode-ai"),
            update_args: &["upgrade"], docs_url: "https://opencode.ai/docs", recommended: None,
            install: &[("npm", "npm install -g opencode-ai", ""), ("安装脚本", "curl -fsSL https://opencode.ai/install | bash", "unix")] },
        "kiro" => ToolSpec { display_name: "Kiro CLI", command: "kiro-cli", latest: LatestSource::KiroManifest,
            update_args: &["update"], docs_url: "https://kiro.dev/docs/cli/", recommended: None,
            install: &[("PowerShell", "irm https://cli.kiro.dev/install.ps1 | iex", "windows"), ("安装脚本", "curl -fsSL https://cli.kiro.dev/install | bash", "unix")] },
        "antigravity" => ToolSpec { display_name: "Antigravity CLI", command: "agy", latest: LatestSource::AntigravityChangelog,
            update_args: &["update"], docs_url: "https://antigravity.google/docs/getting-started", recommended: Some("1.5.1"),
            install: &[("PowerShell", "irm https://antigravity.google/cli/install.ps1 | iex", "windows"), ("安装脚本", "curl -fsSL https://antigravity.google/cli/install.sh | bash", "unix")] },
        "cursor" => ToolSpec { display_name: "Cursor SDK", command: "", latest: LatestSource::Npm(CURSOR_SDK_PACKAGE),
            update_args: &[], docs_url: "https://www.npmjs.com/package/@cursor/sdk", recommended: None, install: &[] },
        _ => return None,
    })
}

#[derive(serde::Serialize, Clone, Debug, Default, PartialEq)]
pub struct DetectedProgram {
    pub path: String,
    pub source: String,
    pub version: Option<String>,
    /// Ran successfully (`--version` exited 0, or the SDK package was found).
    pub ok: bool,
    pub warning: Option<String>,
    pub error: Option<String>,
}
#[derive(serde::Serialize, Clone, Debug)]
pub struct InstallCommand { pub label: String, pub command: String }
#[derive(serde::Serialize, Debug)]
pub struct NodeStatus { pub configured: String, pub required: String, pub selected: Option<DetectedProgram>, pub candidates: Vec<DetectedProgram> }
#[derive(serde::Serialize, Debug)]
pub struct ToolStatus {
    /// "cli" or "sdk"
    pub kind: &'static str,
    pub display_name: String,
    pub default_command: String,
    pub configured: String,
    pub selected: Option<DetectedProgram>,
    pub candidates: Vec<DetectedProgram>,
    pub install_commands: Vec<InstallCommand>,
    pub docs_url: String,
    pub can_update: bool,
    pub managed_directory: Option<String>,
    pub warnings: Vec<String>,
    /// Cursor: this provider's browser sign-in (None for other agents or when no provider id was given).
    pub account: Option<auth::CursorAccount>,
    /// Cursor: an API key (field or CURSOR_API_KEY) is configured and takes precedence over sign-in.
    pub uses_api_key: bool,
}
#[derive(serde::Serialize, Debug)]
pub struct WorkingDirectoryStatus { pub configured: String, pub resolved: String, pub exists: bool }
#[derive(serde::Serialize, Debug)]
pub struct NativeAgentEnvironment { pub driver: String, pub node: NodeStatus, pub tool: ToolStatus, pub working_directory: WorkingDirectoryStatus }
#[derive(serde::Serialize, Debug)]
pub struct NativeAgentUpdateInfo { pub latest: String, pub update_available: bool, pub source: String }

// ---------------------------------------------------------------- versions

/// First `x.y.z` token (optional `v` prefix and pre-release suffix), e.g. "kiro-cli 3.2.0" -> "3.2.0".
pub(crate) fn version_token(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')' | '[' | ']' | '"' | '\''))
        .map(|token| token.trim_start_matches(['v', 'V']).trim_end_matches(['.', ':', ';']))
        .find(|token| semver::Version::parse(token).is_ok())
        .map(str::to_string)
}
fn parse_version(version: &str) -> Option<semver::Version> { semver::Version::parse(version.trim().trim_start_matches(['v', 'V'])).ok() }
pub(crate) fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parse_version(current), parse_version(latest)), (Some(current), Some(latest)) if latest > current)
}
fn below(version: &str, minimum: &str) -> bool { is_newer(version, minimum) }

/// Latest release from the Antigravity CLI changelog the CLI itself reads.
pub(crate) fn changelog_latest(markdown: &str) -> Option<String> {
    markdown.lines().filter_map(|line| line.trim().strip_prefix("## ")).find_map(version_token)
}

// ---------------------------------------------------------------- discovery

fn env_dir(name: &str) -> Option<PathBuf> { std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from) }
fn children(dir: &Path, tail: &[&str]) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new(); };
    let mut found = entries.filter_map(Result::ok)
        .map(|entry| tail.iter().fold(entry.path(), |path, part| path.join(part)))
        .filter(|path| path.is_file()).collect::<Vec<_>>();
    found.sort();
    found
}
fn identity(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    if cfg!(windows) { text.to_lowercase() } else { text }
}
fn dedupe(paths: Vec<(PathBuf, String)>) -> Vec<(PathBuf, String)> {
    let mut seen = HashSet::new();
    paths.into_iter().filter(|(path, _)| path.is_file() && seen.insert(identity(path))).collect()
}

/// PATH a bridge child would see: a configured PATH override, else the app's own.
fn effective_path(env: &HashMap<String, String>) -> OsString {
    env.iter().find(|(key, _)| if cfg!(windows) { key.eq_ignore_ascii_case("PATH") } else { key.as_str() == "PATH" })
        .map(|(_, value)| OsString::from(value)).or_else(|| std::env::var_os("PATH")).unwrap_or_default()
}

/// Every Node.js installation we know how to find: PATH first, then version managers.
fn node_locations(path: &OsString) -> Vec<(PathBuf, String)> {
    let mut found: Vec<(PathBuf, String)> = std::env::split_paths(path).map(|dir| (dir.join(NODE_EXE), "PATH".to_string())).collect();
    let mut add = |paths: Vec<PathBuf>, label: &str| found.extend(paths.into_iter().map(|path| (path, label.to_string())));
    let home = dirs::home_dir();
    #[cfg(windows)]
    {
        add(env_dir("ProgramFiles").map(|dir| vec![dir.join("nodejs").join(NODE_EXE)]).unwrap_or_default(), "Node.js 安装包");
        for nvm in [env_dir("NVM_HOME"), env_dir("APPDATA").map(|dir| dir.join("nvm"))].into_iter().flatten() { add(children(&nvm, &[NODE_EXE]), "nvm"); }
        for fnm in [env_dir("FNM_DIR"), env_dir("APPDATA").map(|dir| dir.join("fnm")), env_dir("LOCALAPPDATA").map(|dir| dir.join("fnm"))].into_iter().flatten() {
            add(children(&fnm.join("node-versions"), &["installation", NODE_EXE]), "fnm");
        }
        if let Some(local) = env_dir("LOCALAPPDATA") {
            add(children(&local.join("Volta").join("tools").join("image").join("node"), &[NODE_EXE]), "Volta");
            add(children(&local.join("mise").join("installs").join("node"), &[NODE_EXE]), "mise");
        }
        if let Some(home) = &home {
            add(["nodejs", "nodejs-lts"].iter().map(|app| home.join("scoop").join("apps").join(app).join("current").join(NODE_EXE)).collect(), "Scoop");
        }
    }
    #[cfg(not(windows))]
    {
        add(vec![PathBuf::from("/opt/homebrew/bin/node")], "Homebrew");
        add(vec![PathBuf::from("/usr/local/bin/node")], "/usr/local");
        add(vec![PathBuf::from("/usr/bin/node")], "系统");
        for opt in ["/opt/homebrew/opt", "/usr/local/opt"] {
            let versions = std::fs::read_dir(opt).map(|entries| entries.filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("node@"))
                .map(|entry| entry.path().join("bin").join("node")).collect()).unwrap_or_default();
            add(versions, "Homebrew");
        }
        if let Some(home) = &home {
            add(children(&env_dir("NVM_DIR").unwrap_or_else(|| home.join(".nvm")).join("versions").join("node"), &["bin", "node"]), "nvm");
            for fnm in [env_dir("FNM_DIR"), Some(home.join(".local/share/fnm")), Some(home.join("Library/Application Support/fnm")), Some(home.join(".fnm"))].into_iter().flatten() {
                add(children(&fnm.join("node-versions"), &["installation", "bin", "node"]), "fnm");
            }
            add(children(&home.join(".volta/tools/image/node"), &["bin", "node"]), "Volta");
            add(children(&home.join(".asdf/installs/nodejs"), &["bin", "node"]), "asdf");
            add(children(&home.join(".local/share/mise/installs/node"), &["bin", "node"]), "mise");
        }
        add(children(Path::new("/usr/local/n/versions/node"), &["bin", "node"]), "n");
    }
    let _ = home;
    found
}

/// The file the bridge's launch() picks inside one directory (Windows: .exe, .ps1, .cmd, bare).
pub(crate) fn command_in(dir: &Path, command: &str, windows: bool, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let extensions: &[&str] = if windows && Path::new(command).extension().is_none() { &[".exe", ".ps1", ".cmd", ""] } else { &[""] };
    extensions.iter().map(|extension| dir.join(format!("{command}{extension}"))).find(|path| exists(path))
}

/// Install locations used by each agent's official installer and common package managers.
fn tool_dirs(driver: &str) -> Vec<PathBuf> {
    let home = dirs::home_dir();
    let mut dirs = Vec::new();
    if let Some(home) = &home {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".bun").join("bin"));
        match driver {
            "pi" => dirs.push(home.join(".pi").join("agent").join("bin")),
            "opencode" => dirs.push(home.join(".opencode").join("bin")),
            _ => {}
        }
    }
    #[cfg(windows)]
    {
        dirs.extend(env_dir("APPDATA").map(|dir| dir.join("npm")));
        dirs.extend(env_dir("LOCALAPPDATA").map(|dir| dir.join("pnpm")));
        match driver {
            "kiro" => if let Some(programs) = env_dir("ProgramFiles") { dirs.push(programs.join("Kiro-Cli")); dirs.push(programs.join("Kiro-Cli").join("bin")); },
            "antigravity" => dirs.extend(env_dir("LOCALAPPDATA").map(|dir| dir.join("agy").join("bin"))),
            _ => {}
        }
    }
    #[cfg(not(windows))]
    {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        if let Some(home) = &home {
            dirs.extend([".npm-global/bin", "Library/pnpm", ".local/share/pnpm"].map(|dir| home.join(dir)));
        }
    }
    dirs
}

pub(crate) fn source_label(path: &Path, path_dirs: &[PathBuf]) -> String {
    let text = path.to_string_lossy().replace('\\', "/").to_lowercase();
    for (needle, label) in [(".pi/agent/bin", "Pi 安装器"), ("/pnpm", "pnpm 全局"), ("/.bun/", "Bun 全局"), ("/.opencode/bin", "OpenCode 安装器"),
        ("/agy/bin", "Antigravity 安装器"), ("/kiro-cli", "Kiro 安装器"), ("/fnm", "fnm"), ("/volta", "Volta"), ("/nvm", "nvm"),
        ("/appdata/roaming/npm", "npm 全局"), ("/.npm-global/", "npm 全局"), ("/homebrew/", "Homebrew"), ("/.local/bin", "用户目录")] {
        if text.contains(needle) { return label.to_string(); }
    }
    let parent = path.parent().map(identity);
    if path_dirs.iter().any(|dir| Some(identity(dir)) == parent) { "PATH".into() } else { "其他位置".into() }
}

async fn program_version(path: &Path) -> Result<String, String> {
    let mut command = tokio::process::Command::new(path);
    command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)] { command.creation_flags(0x08000000); }
    let output = timeout(Duration::from_secs(10), command.output()).await
        .map_err(|_| "--version 10 秒内没有返回".to_string())?.map_err(|e| format!("无法运行：{e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!("--version 退出状态 {}：{}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }
    version_token(&stdout).ok_or_else(|| format!("无法从 --version 输出识别版本：{}", stdout.trim()))
}

fn node_program(path: &Path, source: String, result: Result<String, String>) -> DetectedProgram {
    let (version, error) = match result { Ok(version) => (Some(version), None), Err(error) => (None, Some(error)) };
    let warning = version.as_deref().filter(|version| below(version, NODE_MINIMUM)).map(|_| format!("低于要求的 Node.js {NODE_MINIMUM}"));
    DetectedProgram { path: path.display().to_string(), source, ok: version.is_some(), version, warning, error }
}

async fn detect_node(command: &str, search_path: &OsString) -> NodeStatus {
    let locations = dedupe(node_locations(search_path));
    let mut candidates = futures::future::join_all(locations.into_iter().map(|(path, source)| async move {
        let result = program_version(&path).await;
        node_program(&path, source, result)
    })).await;
    // PATH entries keep PATH order; the rest newest first.
    candidates.sort_by(|a, b| (b.source == "PATH").cmp(&(a.source == "PATH"))
        .then_with(|| if a.source == "PATH" { std::cmp::Ordering::Equal } else {
            parse_version(b.version.as_deref().unwrap_or_default()).cmp(&parse_version(a.version.as_deref().unwrap_or_default()))
        }));
    let configured = Path::new(command);
    let selected = if configured.is_absolute() {
        let found = candidates.iter().find(|candidate| identity(Path::new(&candidate.path)) == identity(configured)).cloned();
        Some(match found { Some(found) => found, None => node_program(configured, "自定义".into(), if configured.is_file() { program_version(configured).await } else { Err("文件不存在".into()) }) })
    } else {
        match which::which_in(command, Some(search_path), std::env::current_dir().unwrap_or_default()) {
            Ok(path) => {
                let found = candidates.iter().find(|candidate| identity(Path::new(&candidate.path)) == identity(&path)).cloned();
                Some(match found { Some(found) => found, None => node_program(&path, "PATH".into(), program_version(&path).await) })
            }
            Err(_) => None,
        }
    };
    NodeStatus { configured: if command == "node" { String::new() } else { command.to_string() }, required: NODE_MINIMUM.into(), selected, candidates }
}

fn cli_program(result: &Value, source: String, recommended: Option<&str>) -> DetectedProgram {
    let command = result["command"].as_str().unwrap_or_default();
    let path = result["resolved"].as_str().unwrap_or(command).to_string();
    let output = result["output"].as_str().unwrap_or_default().trim();
    let ok = result["ok"] == true;
    let version = ok.then(|| version_token(output)).flatten();
    let error = (!ok).then(|| match result["error"].as_str() {
        Some(error) if result["resolved"].is_null() && error.contains("ENOENT") => format!("未找到 {command}"),
        Some(error) => error.to_string(),
        None => format!("--version 退出码 {}：{}", result["code"], output.chars().take(400).collect::<String>()),
    });
    let warning = match (&version, recommended) {
        (Some(version), Some(minimum)) if below(version, minimum) => Some(format!("建议使用 {minimum} 或更新版本")),
        _ => None,
    };
    DetectedProgram { path, source, version, ok, warning, error }
}

/// One-shot bridge call (detect/exec). `log` frames are forwarded to `on_log`.
async fn bridge_call(node: &str, cwd: &Path, env: &HashMap<String, String>, request: Value, limit: Duration, mut on_log: impl FnMut(&str)) -> Result<Value, String> {
    let files = bridge_files().await?;
    let mut command = tokio::process::Command::new(node);
    command.arg(files.0.join("bridge.mjs")).current_dir(cwd).envs(env)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)] { command.creation_flags(0x08000000); }
    #[cfg(unix)] { command.process_group(0); }
    let mut child = command.spawn().map_err(|e| format!("无法用 {node} 启动 Node 桥接进程：{e}"))?;
    let diagnostics = AgentDiagnosticBuffer::for_environment(env);
    let buffer = diagnostics.clone();
    let mut stderr = BufReader::new(child.stderr.take().ok_or("Bridge stderr missing")?).lines();
    let stderr_task = tokio::spawn(async move { while let Ok(Some(line)) = stderr.next_line().await { buffer.push(line); } });
    let id = request["id"].clone();
    let result = timeout(limit, async {
        let mut stdin = child.stdin.take().ok_or("Bridge stdin missing")?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("Bridge stdout missing")?).lines();
        write_frame(&mut stdin, &request).await?;
        loop {
            let line = lines.next_line().await.map_err(|e| e.to_string())?.ok_or("Node 桥接进程提前退出")?;
            let frame: Value = serde_json::from_str(&line).map_err(|e| format!("Invalid bridge JSON: {e}"))?;
            if frame["type"] == "fatal" { return Err(frame["error"].as_str().unwrap_or("bridge fatal frame omitted detail").to_string()); }
            if frame["type"] == "log" && frame["id"] == id { on_log(frame["text"].as_str().unwrap_or_default()); continue; }
            if frame["type"] != "response" || frame["id"] != id { continue; }
            if frame["ok"] != true { return Err(frame["error"].as_str().unwrap_or("bridge response omitted error detail").to_string()); }
            return Ok(frame["data"].clone());
        }
    }).await.unwrap_or_else(|_| Err(format!("Node 桥接进程 {} 秒内没有完成", limit.as_secs())));
    if result.is_ok() {
        // 结果已拿到；Windows 上结束进程树本身要 ~0.5s，放到后台，解包的桥接文件等进程结束后再删
        tokio::spawn(async move {
            terminate_tree(&mut child).await;
            let _ = timeout(Duration::from_secs(2), stderr_task).await;
            drop(files);
        });
        return result;
    }
    terminate_tree(&mut child).await;
    let _ = timeout(Duration::from_secs(2), stderr_task).await;
    result.map_err(|error| {
        let error = diagnostics.sanitize(&error);
        match diagnostics.diagnostics(None, None).stderr.filter(|stderr| !stderr.trim().is_empty()) {
            Some(stderr) => format!("{error}\nstderr: {}", tail(&stderr, 2000)),
            None => error,
        }
    })
}

fn tail(text: &str, chars: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(chars)).collect()
}

fn provider_settings(settings: &HashMap<String, String>) -> Vec<LLMProviderConfig> {
    settings.iter().map(|(name, value)| LLMProviderConfig {
        id: 0, name: name.clone(), value: value.clone(), llm_provider_id: 0, append_location: String::new(), is_addition: false,
    }).collect()
}

struct Context {
    providers: Vec<LLMProviderConfig>,
    node: String,
    env: HashMap<String, String>,
    run_dir: PathBuf,
    working_directory: WorkingDirectoryStatus,
}
impl Context {
    fn get(&self, key: &str) -> Option<String> { setting(&[], &self.providers, key) }
    fn new(settings: &HashMap<String, String>) -> Result<Self, String> {
        let providers = provider_settings(settings);
        let configured = setting(&[], &providers, "acp_working_directory").unwrap_or_default();
        let resolved = if configured.is_empty() { dirs::home_dir() } else { Some(expand_home(configured.trim())) };
        let exists = resolved.as_ref().is_some_and(|path| path.is_dir());
        let run_dir = resolved.clone().filter(|_| exists).or_else(dirs::home_dir).or_else(|| std::env::current_dir().ok())
            .ok_or("无法确定检测使用的目录")?;
        let node = setting(&[], &providers, "agent_node_command").map(|value| value.trim().to_string()).unwrap_or_else(|| "node".into());
        let env = agent_environment(&[], &providers, &node).map_err(|e| e.to_string())?;
        Ok(Self { working_directory: WorkingDirectoryStatus { configured, resolved: resolved.map(|path| path.display().to_string()).unwrap_or_default(), exists },
            providers, node, env, run_dir })
    }
    fn standalone_antigravity(&self, driver: &str) -> bool { driver == "antigravity" && self.get("antigravity_runtime").as_deref() == Some("standalone") }
}

fn managed_sdk_directory(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("agent-runtimes").join("cursor-sdk"))
}

/// Node's require() walks up from the start directory through every `node_modules`.
pub(crate) fn find_cursor_sdk(start: &Path) -> Option<(PathBuf, Option<String>)> {
    start.ancestors().find_map(|dir| {
        let manifest = dir.join("node_modules").join("@cursor").join("sdk").join("package.json");
        manifest.is_file().then(|| {
            let version = std::fs::read_to_string(&manifest).ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|json| json["version"].as_str().map(str::to_string));
            (dir.to_path_buf(), version)
        })
    })
}

fn install_commands(spec: &ToolSpec) -> Vec<InstallCommand> {
    let platform = if cfg!(windows) { "windows" } else { "unix" };
    spec.install.iter().filter(|(_, _, only)| only.is_empty() || *only == platform)
        .map(|(label, command, _)| InstallCommand { label: label.to_string(), command: command.to_string() }).collect()
}

fn detect_cursor(app: &tauri::AppHandle, context: &Context, spec: &ToolSpec, node: &NodeStatus, provider_id: Option<i64>) -> ToolStatus {
    let configured = context.get("agent_sdk_directory").map(|value| value.trim().to_string()).unwrap_or_default();
    let managed = managed_sdk_directory(app).ok();
    let start = if configured.is_empty() { context.run_dir.clone() } else { expand_home(&configured) };
    let program = |dir: PathBuf, version: Option<String>, source: &str| DetectedProgram {
        path: dir.display().to_string(), source: source.into(), version, ok: true, warning: None, error: None };
    let selected = match find_cursor_sdk(&start) {
        Some((dir, version)) => Some(program(dir, version, if configured.is_empty() { "工作目录" } else { "所选目录" })),
        None => Some(DetectedProgram { path: start.display().to_string(), source: if configured.is_empty() { "工作目录".into() } else { "所选目录".into() },
            error: Some(format!("{} 中没有安装 {CURSOR_SDK_PACKAGE}", start.display())), ..Default::default() }),
    };
    let mut candidates = Vec::new();
    if let Some((dir, version)) = managed.as_deref().and_then(find_cursor_sdk) { candidates.push(program(dir, version, "AIPP 管理")); }
    let mut warnings = Vec::new();
    let uses_api_key = context.env.keys().any(|key| key == "CURSOR_API_KEY") || std::env::var_os("CURSOR_API_KEY").is_some();
    let now = chrono::Utc::now().timestamp_millis();
    let account = provider_id.map(|id| auth::cursor_auth_store(app, id).map_err(|e| e.to_string()).and_then(|path| auth::read_cursor_account(&path, now)));
    let account = match account {
        Some(Ok(account)) => Some(account),
        Some(Err(error)) => { warnings.push(error); None }
        None => None,
    };
    if !uses_api_key {
        match &account {
            Some(account) if account.expired => warnings.push("Cursor 登录已过期，请重新登录".into()),
            Some(account) if account.signed_in => {}
            _ => warnings.push("尚未登录 Cursor：点击「登录 Cursor」用浏览器登录，或填写 Cursor API Key".into()),
        }
    }
    ToolStatus { kind: "sdk", display_name: spec.display_name.into(), default_command: String::new(), configured,
        can_update: node.selected.as_ref().is_some_and(|node| node.ok), selected, candidates,
        install_commands: vec![InstallCommand { label: "npm".into(), command: format!("npm install --prefix <目录> {CURSOR_SDK_PACKAGE}") }],
        docs_url: spec.docs_url.into(), managed_directory: managed.map(|dir| dir.display().to_string()), warnings, account, uses_api_key }
}

async fn detect_cli(driver: &str, context: &Context, spec: &ToolSpec, node: &NodeStatus) -> ToolStatus {
    let standalone = context.standalone_antigravity(driver);
    let default_command = if standalone { "antigravity-acp" } else { spec.command };
    let configured = context.get("agent_cli_command").map(|value| value.trim().to_string()).unwrap_or_default();
    let command = if configured.is_empty() { default_command.to_string() } else { configured.clone() };
    let path_var = effective_path(&context.env);
    let path_dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    // Candidate files: the selected Node's directory, every Node install (nvm-style
    // globals live beside node), PATH, then installer/package-manager locations.
    let mut scan = Vec::new();
    if !standalone {
        scan.extend(node.selected.iter().chain(node.candidates.iter()).filter_map(|node| Path::new(&node.path).parent().map(Path::to_path_buf)));
        scan.extend(path_dirs.iter().cloned());
        scan.extend(tool_dirs(driver));
    }
    let files = dedupe(scan.into_iter().filter_map(|dir| command_in(&dir, default_command, cfg!(windows), Path::is_file))
        .map(|path| { let label = source_label(&path, &path_dirs); (path, label) }).collect());
    let mut commands = vec![json!({"key": "selected", "command": command})];
    commands.extend(files.iter().take(12).enumerate().map(|(index, (path, _))| json!({"key": index.to_string(), "command": path})));
    let node_ok = node.selected.as_ref().is_some_and(|node| node.ok);
    let detected = if node_ok {
        bridge_call(&context.node, &context.run_dir, &context.env, json!({"type": "detect", "id": "detect", "commands": commands}), Duration::from_secs(60), |_| {}).await
            .and_then(|data| data["commands"].as_array().cloned().ok_or_else(|| "detect response omitted commands".to_string()))
    } else { Err(format!("需要先选择可运行的 Node.js（≥ {NODE_MINIMUM}）才能检测 {}", spec.display_name)) };
    let (selected, candidates) = match detected {
        Ok(results) => {
            let find = |key: &str| results.iter().find(|result| result["key"] == key);
            let selected = find("selected").map(|result| {
                let path = result["resolved"].as_str().map(PathBuf::from);
                let source = if configured.is_empty() { path.as_deref().map(|path| source_label(path, &path_dirs)).unwrap_or_else(|| "PATH".into()) } else { "自定义".into() };
                cli_program(result, source, spec.recommended)
            });
            let candidates = files.iter().take(12).enumerate().filter_map(|(index, (_, source))| find(&index.to_string()).map(|result| cli_program(result, source.clone(), spec.recommended))).collect();
            (selected, candidates)
        }
        Err(error) => (Some(DetectedProgram { path: command.clone(), source: "未检测".into(), error: Some(error), ..Default::default() }),
            files.iter().map(|(path, source)| DetectedProgram { path: path.display().to_string(), source: source.clone(), ..Default::default() }).collect()),
    };
    let mut warnings = Vec::new();
    match driver {
        "kiro" => warnings.push("首次使用前请在终端运行 kiro-cli login 完成登录；AIPP 以 V3 引擎启动，每次会话都会关闭 Autopilot".into()),
        "pi" | "opencode" => warnings.push(format!("复用本机 {} 的登录与配置；未登录时先在终端运行 {}", spec.display_name, spec.command)),
        _ => {}
    }
    ToolStatus { kind: "cli", display_name: if standalone { "Antigravity 独立 ACP 程序".into() } else { spec.display_name.into() },
        default_command: default_command.into(), configured, can_update: !standalone && node_ok && selected.as_ref().is_some_and(|program| program.ok),
        selected, candidates, install_commands: if standalone { Vec::new() } else { install_commands(spec) },
        docs_url: spec.docs_url.into(), managed_directory: None, warnings, account: None, uses_api_key: false }
}

pub(crate) async fn detect_environment(app: &tauri::AppHandle, driver: &str, settings: &HashMap<String, String>, provider_id: Option<i64>) -> Result<NativeAgentEnvironment, String> {
    let spec = spec(driver).ok_or_else(|| format!("Unknown native agent driver: {driver}"))?;
    let context = Context::new(settings)?;
    let node = detect_node(&context.node, &effective_path(&context.env)).await;
    let tool = if driver == "cursor" { detect_cursor(app, &context, &spec, &node, provider_id) } else { detect_cli(driver, &context, &spec, &node).await };
    Ok(NativeAgentEnvironment { driver: driver.into(), node, tool, working_directory: context.working_directory })
}

// ---------------------------------------------------------------- latest version

async fn fetch_text(url: &str, proxy: Option<&str>) -> Result<String, String> {
    let mut builder = reqwest::Client::builder().user_agent("AIPP-App").timeout(Duration::from_secs(20));
    if let Some(proxy) = proxy { builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| format!("代理地址无效：{e}"))?); }
    let response = builder.build().map_err(|e| e.to_string())?.get(url).send().await.map_err(|e| format!("请求 {url} 失败：{e}"))?;
    if !response.status().is_success() { return Err(format!("{url} 返回 HTTP {}", response.status())); }
    response.text().await.map_err(|e| format!("读取 {url} 失败：{e}"))
}

pub(crate) fn latest_source_url(source: LatestSource) -> String {
    match source {
        LatestSource::Npm(package) => format!("https://registry.npmjs.org/{}/latest", urlencoding::encode(package)),
        LatestSource::KiroManifest => "https://prod.download.cli.kiro.dev/stable/latest/manifest.json".into(),
        LatestSource::AntigravityChangelog => "https://raw.githubusercontent.com/google-antigravity/antigravity-cli/refs/heads/main/CHANGELOG.md".into(),
    }
}

pub(crate) fn parse_latest(source: LatestSource, body: &str) -> Result<String, String> {
    match source {
        LatestSource::Npm(_) | LatestSource::KiroManifest => serde_json::from_str::<Value>(body).ok()
            .and_then(|json| json["version"].as_str().map(str::to_string)).filter(|version| parse_version(version).is_some())
            .ok_or_else(|| "发布信息中没有有效的 version 字段".to_string()),
        LatestSource::AntigravityChangelog => changelog_latest(body).ok_or_else(|| "CHANGELOG 中没有版本标题".to_string()),
    }
}

async fn network_proxy(state: &crate::FeatureConfigState) -> Option<String> {
    let map = state.config_feature_map.lock().await;
    crate::api::ai::config::get_network_proxy_from_config(&map)
}

#[tauri::command]
pub async fn check_native_agent_update(feature_config_state: tauri::State<'_, crate::FeatureConfigState>, driver: String, current_version: Option<String>, use_proxy: Option<bool>) -> Result<NativeAgentUpdateInfo, String> {
    let spec = spec(&driver).ok_or_else(|| format!("Unknown native agent driver: {driver}"))?;
    let proxy = if use_proxy.unwrap_or(false) {
        Some(network_proxy(&feature_config_state).await.ok_or("未配置全局网络代理")?)
    } else { None };
    let url = latest_source_url(spec.latest);
    let latest = parse_latest(spec.latest, &fetch_text(&url, proxy.as_deref()).await?)?;
    let update_available = current_version.as_deref().is_some_and(|current| is_newer(current, &latest));
    Ok(NativeAgentUpdateInfo { latest, update_available, source: url })
}

#[tauri::command]
pub async fn detect_native_agent_environment(app: tauri::AppHandle, driver: String, settings: HashMap<String, String>, provider_id: Option<i64>) -> Result<NativeAgentEnvironment, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return Err("MOBILE_UNSUPPORTED: native agents require a local desktop runtime".into());
    detect_environment(&app, &driver, &settings, provider_id).await
}

// ---------------------------------------------------------------- update

fn updating() -> &'static Mutex<HashSet<String>> {
    static UPDATING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    UPDATING.get_or_init(Default::default)
}
struct UpdateGuard(String);
impl Drop for UpdateGuard { fn drop(&mut self) { if let Ok(mut set) = updating().lock() { set.remove(&self.0); } } }

/// Running agents keep the binary/SDK files open. Busy sessions block the update; idle
/// ones are closed (their native session ids stay persisted and resume on next prompt).
pub(crate) async fn release_idle_sessions(driver: &str) -> Result<usize, String> {
    let released: Vec<Entry> = {
        let mut entries = sessions().lock().await;
        let busy = entries.iter().filter(|((kind, _), entry)| *kind == driver && !entry.control.tx.is_closed() && entry.control.busy.load(Ordering::SeqCst)).count();
        if busy > 0 { return Err(format!("{busy} 个 {driver} 会话正在回复，请等待完成或停止后再更新")); }
        let keys: Vec<_> = entries.keys().filter(|(kind, _)| *kind == driver).cloned().collect();
        keys.into_iter().filter_map(|key| entries.remove(&key)).collect()
    };
    for entry in &released { let _ = entry.control.tx.send(Command::Shutdown); }
    for entry in &released { let _ = timeout(Duration::from_secs(10), entry.control.tx.closed()).await; }
    Ok(released.len())
}

/// Returns the SDK directory for Cursor (so the form can pin it), otherwise None.
#[tauri::command]
pub async fn run_native_agent_update(app: tauri::AppHandle, feature_config_state: tauri::State<'_, crate::FeatureConfigState>, driver: String,
    settings: HashMap<String, String>, use_proxy: Option<bool>, on_log: tauri::ipc::Channel<String>) -> Result<Option<String>, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return Err("MOBILE_UNSUPPORTED: native agents require a local desktop runtime".into());

    let spec = spec(&driver).ok_or_else(|| format!("Unknown native agent driver: {driver}"))?;
    let context = Context::new(&settings)?;
    if context.standalone_antigravity(&driver) { return Err("独立 ACP 程序由你自行安装和管理，AIPP 不提供一键更新".into()); }
    {
        let mut set = updating().lock().map_err(|_| "update state lock poisoned")?;
        if !set.insert(driver.clone()) { return Err(format!("{} 正在更新中", spec.display_name)); }
    }
    let _guard = UpdateGuard(driver.clone());
    let released = release_idle_sessions(&driver).await?;
    if released > 0 { let _ = on_log.send(format!("已关闭 {released} 个空闲的 {} 会话，下次发送消息时会自动恢复\n", spec.display_name)); }

    let mut env = context.env.clone();
    if use_proxy.unwrap_or(false) {
        let proxy = network_proxy(&feature_config_state).await.ok_or("未配置全局网络代理")?;
        env.extend(crate::api::ai::config::build_proxy_env_vars(&proxy));
        let _ = on_log.send(format!("使用全局网络代理 {proxy}\n"));
    }
    let (request, sdk_directory) = if driver == "cursor" {
        let directory = match context.get("agent_sdk_directory") { Some(dir) => expand_home(dir.trim()), None => managed_sdk_directory(&app)? };
        tokio::fs::create_dir_all(&directory).await.map_err(|e| format!("无法创建 SDK 目录 {}：{e}", directory.display()))?;
        let args = json!(["install", "--prefix", directory, format!("{CURSOR_SDK_PACKAGE}@latest"), "--no-audit", "--no-fund"]);
        (json!({"type": "exec", "id": "exec", "tool": "npm", "args": args}), Some(directory))
    } else {
        let command = context.get("agent_cli_command").map(|value| value.trim().to_string()).unwrap_or_else(|| spec.command.into());
        (json!({"type": "exec", "id": "exec", "command": command, "args": spec.update_args}), None)
    };
    let description = match &sdk_directory {
        Some(dir) => format!("npm install --prefix {} {CURSOR_SDK_PACKAGE}@latest", dir.display()),
        None => format!("{} {}", request["command"].as_str().unwrap_or_default(), spec.update_args.join(" ")),
    };
    let _ = on_log.send(format!("$ {description}\n"));
    let data = bridge_call(&context.node, &context.run_dir, &env, request, Duration::from_secs(660), |text| { let _ = on_log.send(text.to_string()); }).await
        .map_err(|error| format!("{} 更新失败（{description}）：{error}", spec.display_name))?;
    if data["code"] != 0 {
        return Err(format!("{} 更新失败（{description}，退出码 {}，信号 {}）：{}", spec.display_name, data["code"], data["signal"],
            tail(data["output"].as_str().unwrap_or_default(), 2000).trim()));
    }
    Ok(sdk_directory.map(|dir| dir.display().to_string()))
}

#[cfg(test)]
#[path = "native_agents_env_tests.rs"]
mod tests;

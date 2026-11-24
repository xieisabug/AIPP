use super::{candidates, probe_version, search_dirs, ProbeError, Program};
use serde::Deserialize;
use std::{collections::{HashMap, HashSet}, fs, path::{Component, Path, PathBuf}, process::Command};

pub(super) struct Package {
    root: PathBuf,
    pnpm: bool,
}

impl Package {
    pub(super) fn apply_environment(&self, cmd: &mut Command) {
        // These are set by the official JS launcher for the native program.
        for key in ["CODEX_MANAGED_BY_NPM", "CODEX_MANAGED_BY_PNPM", "CODEX_MANAGED_BY_BUN", "CODEX_MANAGED_BY_VITE_PLUS"] {
            cmd.env_remove(key);
        }
        cmd.env("CODEX_MANAGED_PACKAGE_ROOT", &self.root);
        cmd.env(if self.pnpm { "CODEX_MANAGED_BY_PNPM" } else { "CODEX_MANAGED_BY_NPM" }, "1");
    }
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    version: String,
    #[serde(rename = "optionalDependencies", default)]
    optional: HashMap<String, String>,
}

fn manifest(dir: &Path) -> Option<Manifest> {
    serde_json::from_slice(&fs::read(dir.join("package.json")).ok()?).ok()
}

/// Read the entrypoint referenced by a generated npm/pnpm batch launcher. Never
/// run its text or guess which of several versions in a pnpm store is current.
fn shim_package(shim: &Path) -> Option<Package> {
    if !shim.file_name()?.eq_ignore_ascii_case("codex.cmd") { return None; }
    let text = fs::read_to_string(shim).ok()?;
    for line in text.lines() {
        let Some(invocation) = line.trim_end().strip_suffix("\" %*") else { continue };
        let Some((prefix, arg)) = invocation.rsplit_once('"') else { continue };
        let prefix = prefix.trim_end();
        if !(prefix.ends_with(" node") || prefix.trim() == "node" || prefix.ends_with("node.exe\"")
            || prefix.ends_with("\"%_prog%\"")) { continue; }
        let Some(rel) = arg.strip_prefix("%~dp0").or_else(|| arg.strip_prefix("%dp0%")) else { continue };
        let js = Path::new(rel.trim_start_matches(['\\', '/']));
        if !js.ends_with("node_modules/@openai/codex/bin/codex.js")
            || !js.components().all(|part| matches!(part, Component::Normal(_))) { continue; }
        let entry = shim.parent()?.join(js);
        let root = fs::canonicalize(entry.parent()?.parent()?).ok()?;
        let pnpm = root.ancestors().any(|dir| dir.file_name().is_some_and(|s| s == ".pnpm"))
            || js.starts_with("global");
        return Some(Package { root, pnpm });
    }
    None
}

fn native_program(shim: &Path) -> Option<Program> {
    let package = shim_package(shim)?;
    let wrapper = manifest(&package.root)?;
    if wrapper.name != "@openai/codex" { return None; }
    let (arch, triple) = match std::env::consts::ARCH {
        "x86_64" => ("x64", "x86_64-pc-windows-msvc"),
        "aarch64" => ("arm64", "aarch64-pc-windows-msvc"),
        _ => return None,
    };
    let alias = format!("@openai/codex-win32-{arch}");
    let dependency = wrapper.optional.get(&alias)?;
    let expected = dependency.strip_prefix("npm:@openai/codex@").unwrap_or(dependency);
    semver::Version::parse(expected).ok()?;
    let modules = package.root.parent()?.parent()?;
    let mut roots = vec![package.root.join("node_modules").join(&alias), modules.join(&alias)];
    // pnpm can retain a valid platform package even if the optional dependency junction is broken.
    if package.pnpm {
        let virtual_store = modules.parent()?.parent()?;
        roots.push(virtual_store.join(format!("@openai+codex@{expected}")).join("node_modules/@openai/codex"));
    }
    for root in roots {
        let Some(platform) = manifest(&root) else { continue };
        if platform.version != expected || (platform.name != "@openai/codex" && platform.name != alias) { continue; }
        let path = root.join("vendor").join(triple).join("bin/codex.exe");
        if path.is_file() {
            return Some(Program { path, package: Some(package) });
        }
    }
    None
}

pub(super) async fn detect() -> Result<(Program, String), ProbeError> {
    let dirs = search_dirs("codex");
    let programs = tokio::task::spawn_blocking(move || discover(&dirs)).await
        .map_err(|e| ProbeError { path: None, message: format!("查找 Codex 失败：{e}") })??;
    first_working(programs).await
}

pub(in crate::api::ai) fn discover(dirs: &[PathBuf]) -> Result<Vec<Program>, ProbeError> {
    let mut programs = Vec::new();
    let mut seen = HashSet::new();
    for path in candidates("codex", dirs) {
        if !seen.insert(path.clone()) { continue; }
        if let Some(native) = native_program(&path) {
            programs.push(native);
        }
        programs.push(Program::from_path(path));
    }
    if programs.is_empty() {
        let searched: Vec<_> = dirs.iter().map(|d| d.display().to_string()).collect();
        return Err(ProbeError { path: None, message: format!("找不到命令 codex。已检查目录：{}", searched.join("；")) });
    }
    Ok(programs)
}

pub(in crate::api::ai) async fn first_working(programs: Vec<Program>) -> Result<(Program, String), ProbeError> {
    let first = programs.first().map(|p| p.path.clone());
    let mut errors = Vec::new();
    for program in programs {
        match probe_version(&program).await {
            Ok(version) if semver::Version::parse(&version).is_ok() => return Ok((program, version)),
            Ok(_) => errors.push(format!("{} --version 未返回 Codex 版本号", program.path.display())),
            Err(error) => errors.push(error),
        }
    }
    Err(ProbeError { path: first, message: errors.join("\n") })
}

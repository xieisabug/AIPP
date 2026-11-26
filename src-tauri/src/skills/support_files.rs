//! Supporting files of folder-based skills (`<skill>/SKILL.md` + `references/`, `scripts/`, `assets/`).
//!
//! `load_skill` only returns SKILL.md, so models without native file access need a bounded way
//! to read the files a skill points to. Reads stay inside the skill folder; single-file skills
//! (e.g. a Codex instructions file) have no folder of their own and expose nothing.

use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_LISTED_FILES: usize = 60;
const MAX_DEPTH: usize = 3;

/// The folder owning `skill_file`, only when the skill is a folder-based `SKILL.md`.
pub fn skill_folder(skill_file: &Path) -> Option<PathBuf> {
    let is_skill_md = skill_file.file_name()?.to_str()?.eq_ignore_ascii_case("SKILL.md");
    if !is_skill_md {
        return None;
    }
    skill_file.parent().map(Path::to_path_buf)
}

/// Read a UTF-8 text file addressed relative to the skill folder.
pub fn read_support_file(folder: &Path, relative: &str) -> Result<String, String> {
    let relative = relative.trim();
    if relative.is_empty() || relative.contains(['\\', ':', '\0']) {
        return Err("配套文件路径必须是相对于技能目录的 / 分隔路径".into());
    }
    let candidate = Path::new(relative);
    if candidate.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err("配套文件路径不能是绝对路径或包含 ..".into());
    }
    let root = folder.canonicalize().map_err(|e| format!("读取技能目录失败：{e}"))?;
    let resolved = root
        .join(candidate)
        .canonicalize()
        .map_err(|e| format!("读取配套文件 {relative} 失败：{e}"))?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err("配套文件越出技能目录或不是文件".into());
    }
    let size = resolved.metadata().map_err(|e| e.to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err("配套文件超过 1 MiB，不能直接放入模型上下文".into());
    }
    fs::read_to_string(&resolved).map_err(|e| format!("配套文件必须为 UTF-8 文本：{e}"))
}

/// Relative paths of files in the skill folder (excluding SKILL.md and hidden entries), sorted.
pub fn list_support_files(folder: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect(folder, "", 0, &mut files);
    files.sort();
    files.truncate(MAX_LISTED_FILES);
    files
}

fn collect(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH || out.len() >= MAX_LISTED_FILES * 2 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let relative = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            collect(&entry.path(), &relative, depth + 1, out);
        } else if kind.is_file() && !(prefix.is_empty() && name.eq_ignore_ascii_case("SKILL.md")) {
            out.push(relative);
        }
    }
}

#[cfg(test)]
#[path = "support_files_tests.rs"]
mod tests;

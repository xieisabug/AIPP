//! Configuration readback and process launch must resolve the same directory.
use std::path::{Path, PathBuf};
use crate::db::{assistant_db::AssistantModelConfig, llm_db::LLMProviderConfig};
use crate::errors::AppError;

pub(crate) fn from_configs(
    models: &[AssistantModelConfig],
    providers: &[LLMProviderConfig],
    expand_tilde: bool,
) -> Result<PathBuf, AppError> {
    resolve(configured_path(models, providers), expand_tilde, dirs::home_dir)
}

fn configured_path<'a>(
    models: &'a [AssistantModelConfig],
    providers: &'a [LLMProviderConfig],
) -> Option<&'a str> {
    models.iter().find(|c| c.name == "acp_working_directory")
        .and_then(|c| c.value.as_deref()).map(str::trim).filter(|s| !s.is_empty())
        .or_else(|| providers.iter().find(|c| c.name == "acp_working_directory")
            .map(|c| c.value.trim()).filter(|s| !s.is_empty()))
}

/// Only a configured directory grants file access. The launch fallback to home does not.
pub(crate) fn configured_workspace(
    models: &[AssistantModelConfig],
    providers: &[LLMProviderConfig],
    expand_tilde: bool,
) -> Result<Option<PathBuf>, AppError> {
    configured_path(models, providers)
        .map(|path| resolve(Some(path), expand_tilde, dirs::home_dir))
        .transpose()
}

pub(crate) fn contains_file_path(workspace: &Path, target: &Path) -> bool {
    if !workspace.is_absolute() || !workspace.is_dir() || !target.is_absolute() {
        return false;
    }
    let Ok(root) = workspace.canonicalize() else { return false };
    // For a new file, resolve its closest existing ancestor, including junctions.
    // A dangling link or an unresolved ".." must not inherit trust from its parent.
    for ancestor in target.ancestors() {
        match ancestor.symlink_metadata() {
            Ok(_) => return ancestor.canonicalize().is_ok_and(|path| path.starts_with(&root)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && ancestor.file_name().is_some() => {}
            Err(_) => return false,
        }
    }
    false
}

fn resolve(
    configured: Option<&str>,
    expand_tilde: bool,
    home_dir: impl FnOnce() -> Option<PathBuf>,
) -> Result<PathBuf, AppError> {
    let missing_home = || AppError::ProviderError("无法解析 Agent 工作目录：系统未提供用户主目录，请配置明确的工作目录".into());
    let Some(path) = configured else { return home_dir().ok_or_else(missing_home); };
    // Codex keeps its existing literal-path semantics. No shell or environment
    // expansion is performed; the library handles path boundaries and encoding.
    if !expand_tilde { return Ok(PathBuf::from(path)); }
    let mut unavailable = false;
    let expanded = shellexpand::path::tilde_with_context(Path::new(path), || {
        let home = home_dir();
        unavailable = home.is_none();
        home
    });
    if unavailable { return Err(missing_home()); }
    Ok(expanded.into_owned())
}

#[cfg(test)]
#[path = "working_directory_tests.rs"]
mod tests;

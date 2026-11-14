//! Configuration readback and process launch must resolve the same directory.
use std::path::{Path, PathBuf};
use crate::db::{assistant_db::AssistantModelConfig, llm_db::LLMProviderConfig};
use crate::errors::AppError;

pub(crate) fn from_configs(
    models: &[AssistantModelConfig],
    providers: &[LLMProviderConfig],
    expand_tilde: bool,
) -> Result<PathBuf, AppError> {
    let configured = models.iter().find(|c| c.name == "acp_working_directory")
        .and_then(|c| c.value.as_deref()).map(str::trim).filter(|s| !s.is_empty())
        .or_else(|| providers.iter().find(|c| c.name == "acp_working_directory")
            .map(|c| c.value.trim()).filter(|s| !s.is_empty()));
    resolve(configured, expand_tilde, dirs::home_dir)
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

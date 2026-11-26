//! Agent handler - implements agent tool operations

use super::types::*;
use crate::api::skill_api::get_skill_content_internal;
use crate::slash::find_skill_by_source_and_command;
use tauri::{AppHandle, Manager};
use tracing::{debug, error, info, instrument};

/// Handler for Agent tools
pub struct AgentHandler {
    app_handle: AppHandle,
}

impl AgentHandler {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    /// Load a skill's content by name and source type
    #[instrument(skip(self), fields(command = %request.command, source_type = %request.source_type))]
    pub async fn load_skill(&self, request: LoadSkillRequest) -> Result<LoadSkillResponse, String> {
        info!("Loading skill: command={}, source_type={}", request.command, request.source_type);

        let matching_skill = find_skill_by_source_and_command(
            &self.app_handle,
            &request.source_type,
            &request.command,
        )
        .await
        .map_err(|e| e.to_string())?;

        match matching_skill {
            Some(skill) => {
                debug!("Found skill: {}", skill.identifier);

                let folder = crate::api::skill_api::create_scanner(&self.app_handle)
                    .get_skill(&skill.identifier)
                    .and_then(|scanned| crate::skills::support_files::skill_folder(std::path::Path::new(&scanned.file_path)));
                let skill_dir = folder.as_ref().map(|dir| dir.to_string_lossy().to_string());

                if let Some(file) = request.file {
                    let identifier = skill.identifier.clone();
                    let content = if identifier.starts_with("agent_plugin:") {
                        let data = self.app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
                        tokio::task::spawn_blocking(move || crate::mcp::apps::bundles::read_skill_file(&data, &identifier, &file))
                            .await.map_err(|e| format!("读取插件技能文件：{e}"))??
                    } else {
                        let folder = folder.ok_or_else(|| format!("技能 {identifier} 不是目录形式的 SKILL.md，没有配套文件"))?;
                        tokio::task::spawn_blocking(move || crate::skills::support_files::read_support_file(&folder, &file))
                            .await.map_err(|e| format!("读取技能配套文件：{e}"))??
                    };
                    return Ok(LoadSkillResponse { identifier: skill.identifier, content, found: true, skill_dir, ..Default::default() });
                }
                let files = match folder {
                    Some(folder) => tokio::task::spawn_blocking(move || crate::skills::support_files::list_support_files(&folder))
                        .await.unwrap_or_default(),
                    None => Vec::new(),
                };

                // Load the full content
                match get_skill_content_internal(&self.app_handle, &skill.identifier).await {
                    Ok(content) => {
                        let additional_files = content
                            .additional_files
                            .into_iter()
                            .map(|f| SkillFileContent { path: f.path, content: f.content })
                            .collect();

                        Ok(LoadSkillResponse {
                            identifier: content.identifier,
                            content: content.content,
                            additional_files,
                            found: true,
                            error: None,
                            skill_dir,
                            files,
                        })
                    }
                    Err(e) => {
                        error!("Failed to load skill content: {}", e);
                        Ok(LoadSkillResponse {
                            identifier: skill.identifier,
                            content: String::new(),
                            found: false,
                            error: Some(format!("Failed to load skill content: {}", e)),
                            ..Default::default()
                        })
                    }
                }
            }
            None => {
                // Skill not found
                let error_msg = format!(
                    "Skill not found: command='{}', source_type='{}'",
                    request.command, request.source_type
                );
                debug!("{}", error_msg);

                Ok(LoadSkillResponse {
                    identifier: format!("{}:{}", request.source_type, request.command),
                    content: String::new(),
                    found: false,
                    error: Some(error_msg),
                    ..Default::default()
                })
            }
        }
    }
}

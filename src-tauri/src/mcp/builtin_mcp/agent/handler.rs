//! Agent handler - implements agent tool operations

use super::types::*;
use crate::api::skill_api::get_skill_content_internal;
use crate::slash::find_skill_by_source_and_command;
use tauri::AppHandle;
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
                        })
                    }
                    Err(e) => {
                        error!("Failed to load skill content: {}", e);
                        Ok(LoadSkillResponse {
                            identifier: skill.identifier,
                            content: String::new(),
                            additional_files: vec![],
                            found: false,
                            error: Some(format!("Failed to load skill content: {}", e)),
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
                    additional_files: vec![],
                    found: false,
                    error: Some(error_msg),
                })
            }
        }
    }
}

//! Agent handler - implements agent tool operations

use super::types::*;
use crate::api::skill_api::get_skill_content_internal;
use crate::skills::scanner::SkillScanner;
use std::path::PathBuf;
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

    /// Get home directory path
    fn get_home_dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
    }

    /// Get app data directory path
    fn get_app_data_dir(&self) -> PathBuf {
        self.app_handle.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    /// Create a skill scanner
    fn create_scanner(&self) -> SkillScanner {
        SkillScanner::new(Self::get_home_dir(), self.get_app_data_dir())
    }

    /// Load a skill's content by name and source type
    #[instrument(skip(self), fields(command = %request.command, source_type = %request.source_type))]
    pub async fn load_skill(&self, request: LoadSkillRequest) -> Result<LoadSkillResponse, String> {
        info!("Loading skill: command={}, source_type={}", request.command, request.source_type);

        // Create the identifier from source_type and command
        // The identifier format is "{source_type}:{relative_path}"
        // We need to find the skill that matches the command (name) and source_type
        let scanner = self.create_scanner();
        let all_skills = scanner.scan_all();

        // Find the skill that matches both source_type and command (display_name or relative_path)
        let matching_skill = all_skills.into_iter().find(|skill| {
            let source_matches = skill.source_type.as_str() == request.source_type;
            let name_matches = skill.display_name.eq_ignore_ascii_case(&request.command)
                || skill.relative_path.eq_ignore_ascii_case(&request.command)
                || skill.relative_path.to_lowercase().contains(&request.command.to_lowercase())
                || skill
                    .metadata
                    .name
                    .as_ref()
                    .map(|n| n.eq_ignore_ascii_case(&request.command))
                    .unwrap_or(false);
            source_matches && name_matches
        });

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

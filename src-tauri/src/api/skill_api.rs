//! Skill API - Tauri commands for skill management

use crate::db::skill_db::SkillDatabase;
use crate::skills::parser::SkillParser;
use crate::skills::scanner::SkillScanner;
use crate::skills::types::{
    ScannedSkill, SkillContent, SkillSourceConfig, SkillWithConfig,
};
use std::path::PathBuf;
use tauri::Manager;
use tracing::{debug, info, warn};

/// Get the home directory path
fn get_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Get the app data directory path
fn get_app_data_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Create a skill scanner with proper paths
fn create_scanner(app_handle: &tauri::AppHandle) -> SkillScanner {
    SkillScanner::new(get_home_dir(), get_app_data_dir(app_handle))
}

/// Scan all skills from all configured sources
#[tauri::command]
pub async fn scan_skills(app_handle: tauri::AppHandle) -> Result<Vec<ScannedSkill>, String> {
    let scanner = create_scanner(&app_handle);
    let skills = scanner.scan_all();
    info!("Scanned {} skills", skills.len());
    Ok(skills)
}

/// Get all configured skill sources
#[tauri::command]
pub async fn get_skill_sources(
    app_handle: tauri::AppHandle,
) -> Result<Vec<SkillSourceConfig>, String> {
    let scanner = create_scanner(&app_handle);
    Ok(scanner.get_sources().to_vec())
}

/// Get full content of a skill by identifier
#[tauri::command]
pub async fn get_skill_content(
    app_handle: tauri::AppHandle,
    identifier: String,
) -> Result<SkillContent, String> {
    get_skill_content_internal(&app_handle, &identifier)
        .await
        .map_err(|e| e.to_string())
}

/// Internal function to get skill content (for use by other modules)
pub async fn get_skill_content_internal(
    app_handle: &tauri::AppHandle,
    identifier: &str,
) -> Result<SkillContent, crate::errors::AppError> {
    let scanner = create_scanner(app_handle);

    // Find the skill
    let skill = scanner
        .get_skill(identifier)
        .ok_or_else(|| crate::errors::AppError::InternalError(format!("Skill not found: {}", identifier)))?;

    // Parse full content
    let file_path = PathBuf::from(&skill.file_path);
    let (_metadata, content) = SkillParser::parse_full(&file_path, identifier)
        .map_err(|e| crate::errors::AppError::UnknownError(format!("Failed to parse skill: {}", e)))?;

    Ok(content)
}

/// Get a single skill by identifier
#[tauri::command]
pub async fn get_skill(
    app_handle: tauri::AppHandle,
    identifier: String,
) -> Result<Option<ScannedSkill>, String> {
    let scanner = create_scanner(&app_handle);
    Ok(scanner.get_skill(&identifier))
}

/// Check if a skill exists
#[tauri::command]
pub async fn skill_exists(app_handle: tauri::AppHandle, identifier: String) -> Result<bool, String> {
    let scanner = create_scanner(&app_handle);
    Ok(scanner.skill_exists(&identifier))
}

/// Get skill configs for an assistant (with existence validation)
#[tauri::command]
pub async fn get_assistant_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<Vec<SkillWithConfig>, String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let configs = db
        .get_assistant_skill_configs(assistant_id)
        .map_err(|e| e.to_string())?;

    // Scan all skills to check existence
    let scanner = create_scanner(&app_handle);
    let existing_skills = scanner.scan_all_as_map();

    // Build result with existence check
    let result: Vec<SkillWithConfig> = configs
        .into_iter()
        .map(|config| {
            let skill = existing_skills.get(&config.skill_identifier).cloned();
            let exists = skill.is_some();
            SkillWithConfig {
                skill,
                config,
                exists,
            }
        })
        .collect();

    Ok(result)
}

/// Get enabled skills for an assistant (with existence validation, filters out missing)
#[tauri::command]
pub async fn get_enabled_assistant_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<Vec<ScannedSkill>, String> {
    get_enabled_assistant_skills_internal(&app_handle, assistant_id)
        .await
        .map_err(|e| e.to_string())
}

/// Internal function to get enabled skills (for use by other modules)
pub async fn get_enabled_assistant_skills_internal(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> Result<Vec<ScannedSkill>, crate::errors::AppError> {
    let db = SkillDatabase::new(app_handle).map_err(crate::errors::AppError::from)?;
    let configs = db
        .get_enabled_skill_configs(assistant_id)
        .map_err(crate::errors::AppError::from)?;

    if configs.is_empty() {
        return Ok(Vec::new());
    }

    // Scan all skills to check existence
    let scanner = create_scanner(app_handle);
    let existing_skills = scanner.scan_all_as_map();

    // Filter to only existing skills, maintaining priority order
    let result: Vec<ScannedSkill> = configs
        .into_iter()
        .filter_map(|config| existing_skills.get(&config.skill_identifier).cloned())
        .collect();

    Ok(result)
}

/// 检查操作 MCP 是否已启用（内部函数）
fn check_operation_mcp_enabled(app_handle: &tauri::AppHandle, assistant_id: i64) -> Result<bool, String> {
    use crate::db::mcp_db::MCPDatabase;
    use crate::db::assistant_db::AssistantDatabase;
    use crate::mcp::registry_api::OPERATION_MCP_COMMAND;
    
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let servers = mcp_db.get_mcp_servers().map_err(|e| e.to_string())?;
    
    // 查找操作 MCP
    let operation_mcp = servers.into_iter().find(|s| s.command.as_deref() == Some(OPERATION_MCP_COMMAND));
    
    if let Some(server) = operation_mcp {
        // 检查全局是否启用
        if !server.is_enabled {
            return Ok(false);
        }
        
        // 检查助手级是否启用
        let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let mcp_configs = assistant_db.get_assistant_mcp_configs(assistant_id).map_err(|e| e.to_string())?;
        let assistant_enabled = mcp_configs.iter()
            .find(|c| c.mcp_server_id == server.id)
            .map(|c| c.is_enabled)
            .unwrap_or(false);
        
        Ok(assistant_enabled)
    } else {
        Ok(false)
    }
}

/// Update skill config for an assistant
#[tauri::command]
pub async fn update_assistant_skill_config(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    skill_identifier: String,
    is_enabled: bool,
    priority: i32,
) -> Result<i64, String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    
    // 后端校验：如果要启用 skill，检查当前是否有启用的 skills
    if is_enabled {
        let current_enabled = db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;
        
        // 如果当前没有启用的 skills（即从 0 到 1），需要检查操作 MCP
        if current_enabled.is_empty() {
            let operation_mcp_enabled = check_operation_mcp_enabled(&app_handle, assistant_id)?;
            if !operation_mcp_enabled {
                return Err("OPERATION_MCP_NOT_ENABLED".to_string());
            }
        }
    }
    
    let id = db
        .upsert_assistant_skill_config(assistant_id, &skill_identifier, is_enabled, priority)
        .map_err(|e| e.to_string())?;

    info!(
        "Updated skill config: assistant={}, skill={}, enabled={}",
        assistant_id, skill_identifier, is_enabled
    );
    Ok(id)
}

/// Toggle skill enabled status for an assistant
#[tauri::command]
pub async fn toggle_assistant_skill(
    app_handle: tauri::AppHandle,
    config_id: i64,
    is_enabled: bool,
) -> Result<(), String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.update_skill_config_enabled(config_id, is_enabled)
        .map_err(|e| e.to_string())?;

    debug!("Toggled skill config {} to enabled={}", config_id, is_enabled);
    Ok(())
}

/// Remove a skill config from an assistant
#[tauri::command]
pub async fn remove_assistant_skill(
    app_handle: tauri::AppHandle,
    config_id: i64,
) -> Result<(), String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.delete_skill_config(config_id).map_err(|e| e.to_string())?;

    debug!("Removed skill config {}", config_id);
    Ok(())
}

/// Bulk update skill configs for an assistant
/// Replaces all existing configs with the provided list
#[tauri::command]
pub async fn bulk_update_assistant_skills(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    configs: Vec<(String, bool, i32)>, // (skill_identifier, is_enabled, priority)
) -> Result<(), String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    
    // 后端校验：如果新配置中有启用的 skills，检查当前是否有启用的 skills
    let has_enabled_in_new = configs.iter().any(|(_, enabled, _)| *enabled);
    if has_enabled_in_new {
        let current_enabled = db.get_enabled_skill_configs(assistant_id).map_err(|e| e.to_string())?;
        
        // 如果当前没有启用的 skills（即从 0 到 n），需要检查操作 MCP
        if current_enabled.is_empty() {
            let operation_mcp_enabled = check_operation_mcp_enabled(&app_handle, assistant_id)?;
            if !operation_mcp_enabled {
                return Err("OPERATION_MCP_NOT_ENABLED".to_string());
            }
        }
    }
    
    db.bulk_update_assistant_skills(assistant_id, &configs)
        .map_err(|e| e.to_string())?;

    info!(
        "Bulk updated {} skill configs for assistant {}",
        configs.len(),
        assistant_id
    );
    Ok(())
}

/// Clean up orphaned skill configs (skills that no longer exist)
#[tauri::command]
pub async fn cleanup_orphaned_skill_configs(app_handle: tauri::AppHandle) -> Result<usize, String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let scanner = create_scanner(&app_handle);

    // Get all configured identifiers
    let configured = db
        .get_all_configured_skill_identifiers()
        .map_err(|e| e.to_string())?;

    let mut deleted_count = 0;

    // Check each and delete if not found
    for identifier in configured {
        if !scanner.skill_exists(&identifier) {
            let deleted = db
                .delete_skill_configs_by_identifier(&identifier)
                .map_err(|e| e.to_string())?;
            deleted_count += deleted;
            warn!("Cleaned up orphaned skill config: {}", identifier);
        }
    }

    if deleted_count > 0 {
        info!("Cleaned up {} orphaned skill configs", deleted_count);
    }

    Ok(deleted_count)
}

/// Open the skills folder in the system file manager
#[tauri::command]
pub async fn open_skills_folder(app_handle: tauri::AppHandle) -> Result<(), String> {
    let skills_dir = get_app_data_dir(&app_handle).join("skills");

    // Create if not exists
    std::fs::create_dir_all(&skills_dir).map_err(|e| e.to_string())?;

    // Open with system file manager
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&skills_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&skills_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&skills_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Open the parent folder of a skill file in the system file manager
#[tauri::command]
pub async fn open_skill_parent_folder(file_path: String) -> Result<(), String> {
    let path = PathBuf::from(&file_path);
    let parent = path.parent().ok_or_else(|| "Invalid file path".to_string())?;

    if !parent.exists() {
        return Err("Folder does not exist".to_string());
    }

    // Open with system file manager
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Get skills directory path
#[tauri::command]
pub async fn get_skills_directory(app_handle: tauri::AppHandle) -> Result<String, String> {
    let skills_dir = get_app_data_dir(&app_handle).join("skills");
    Ok(skills_dir.to_string_lossy().to_string())
}

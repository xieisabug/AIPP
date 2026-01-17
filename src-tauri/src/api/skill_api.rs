//! Skill API - Tauri commands for skill management

use crate::db::skill_db::SkillDatabase;
use crate::skills::parser::SkillParser;
use crate::skills::scanner::SkillScanner;
use crate::skills::types::{ScannedSkill, SkillContent, SkillSourceConfig, SkillWithConfig};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;
use tracing::{debug, info, warn};

/// Official skill from the skills store API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub download_url: String,
    pub source_url: String,
}

/// Official skills API endpoint
const OFFICIAL_SKILLS_API: &str = "https://aipp-helper.xieisabug.workers.dev/api/skills";

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
    get_skill_content_internal(&app_handle, &identifier).await.map_err(|e| e.to_string())
}

/// Internal function to get skill content (for use by other modules)
pub async fn get_skill_content_internal(
    app_handle: &tauri::AppHandle,
    identifier: &str,
) -> Result<SkillContent, crate::errors::AppError> {
    let scanner = create_scanner(app_handle);

    // Find the skill
    let skill = scanner.get_skill(identifier).ok_or_else(|| {
        crate::errors::AppError::InternalError(format!("Skill not found: {}", identifier))
    })?;

    // Parse full content
    let file_path = PathBuf::from(&skill.file_path);
    let (_metadata, content) = SkillParser::parse_full(&file_path, identifier).map_err(|e| {
        crate::errors::AppError::UnknownError(format!("Failed to parse skill: {}", e))
    })?;

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
pub async fn skill_exists(
    app_handle: tauri::AppHandle,
    identifier: String,
) -> Result<bool, String> {
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
    let configs = db.get_assistant_skill_configs(assistant_id).map_err(|e| e.to_string())?;

    // Scan all skills to check existence
    let scanner = create_scanner(&app_handle);
    let existing_skills = scanner.scan_all_as_map();

    // Build result with existence check
    let result: Vec<SkillWithConfig> = configs
        .into_iter()
        .map(|config| {
            let skill = existing_skills.get(&config.skill_identifier).cloned();
            let exists = skill.is_some();
            SkillWithConfig { skill, config, exists }
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
    let configs =
        db.get_enabled_skill_configs(assistant_id).map_err(crate::errors::AppError::from)?;

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

/// 检查 Agent load_skill 是否已就绪（全局 + 助手级）
fn check_agent_load_skill_ready(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
) -> Result<bool, String> {
    crate::mcp::registry_api::is_agent_load_skill_ready(app_handle, assistant_id)
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

    // 后端校验：启用 skill 时需要 Agent load_skill 可用
    if is_enabled {
        let agent_ready = check_agent_load_skill_ready(&app_handle, assistant_id)?;
        if !agent_ready {
            return Err("AGENT_LOAD_SKILL_REQUIRED".to_string());
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
    db.update_skill_config_enabled(config_id, is_enabled).map_err(|e| e.to_string())?;

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
        let agent_ready = check_agent_load_skill_ready(&app_handle, assistant_id)?;
        if !agent_ready {
            return Err("AGENT_LOAD_SKILL_REQUIRED".to_string());
        }
    }

    db.bulk_update_assistant_skills(assistant_id, &configs).map_err(|e| e.to_string())?;

    info!("Bulk updated {} skill configs for assistant {}", configs.len(), assistant_id);
    Ok(())
}

/// Clean up orphaned skill configs (skills that no longer exist)
#[tauri::command]
pub async fn cleanup_orphaned_skill_configs(app_handle: tauri::AppHandle) -> Result<usize, String> {
    let db = SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let scanner = create_scanner(&app_handle);

    // Get all configured identifiers
    let configured = db.get_all_configured_skill_identifiers().map_err(|e| e.to_string())?;

    let mut deleted_count = 0;

    // Check each and delete if not found
    for identifier in configured {
        if !scanner.skill_exists(&identifier) {
            let deleted =
                db.delete_skill_configs_by_identifier(&identifier).map_err(|e| e.to_string())?;
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
        std::process::Command::new("open").arg(&skills_dir).spawn().map_err(|e| e.to_string())?;
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
        std::process::Command::new("open").arg(parent).spawn().map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(parent).spawn().map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(parent).spawn().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Get skills directory path
#[tauri::command]
pub async fn get_skills_directory(app_handle: tauri::AppHandle) -> Result<String, String> {
    let skills_dir = get_app_data_dir(&app_handle).join("skills");
    Ok(skills_dir.to_string_lossy().to_string())
}

/// Fetch official skills from the skills store API
#[tauri::command]
pub async fn fetch_official_skills(
    app_handle: tauri::AppHandle,
    use_proxy: bool,
) -> Result<Vec<OfficialSkill>, String> {
    use crate::api::ai::config::get_network_proxy_from_config;
    use std::time::Duration;

    // 5 second timeout
    const TIMEOUT_SECS: u64 = 5;

    // Get proxy configuration if requested
    let proxy_url = if use_proxy {
        let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
        let config_feature_map = feature_config_state.config_feature_map.lock().await;
        get_network_proxy_from_config(&config_feature_map)
    } else {
        None
    };

    // Build client with optional proxy
    let client = if let Some(ref proxy) = proxy_url {
        info!(proxy_url = %proxy, "Using proxy for fetching official skills");
        let proxy = reqwest::Proxy::all(proxy)
            .map_err(|e| format!("代理配置失败: {}", e))?;
        reqwest::Client::builder()
            .proxy(proxy)
            .build()
            .map_err(|e| format!("Failed to build client: {}", e))?
    } else {
        reqwest::Client::new()
    };

    // Fetch with timeout
    let fetch_future = async {
        let response = client
            .get(OFFICIAL_SKILLS_API)
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    format!("请求超时（超过{}秒），请尝试使用代理访问", TIMEOUT_SECS)
                } else if e.is_connect() {
                    format!("网络连接失败，请检查网络或尝试使用代理")
                } else {
                    format!("获取官方技能列表失败: {}", e)
                }
            })?;

        if !response.status().is_success() {
            return Err(format!(
                "Official skills API returned error: {}",
                response.status()
            ));
        }

        let skills = response
            .json::<Vec<OfficialSkill>>()
            .await
            .map_err(|e| format!("Failed to parse skills response: {}", e))?;

        info!("Fetched {} official skills", skills.len());
        Ok::<Vec<OfficialSkill>, String>(skills)
    };

    fetch_future.await
}

/// Install an official skill by downloading and extracting the zip file
#[tauri::command]
pub async fn install_official_skill(
    app_handle: tauri::AppHandle,
    download_url: String,
) -> Result<(), String> {
    info!("Downloading skill from: {}", download_url);

    // Create skills directory if it doesn't exist
    let skills_dir = get_app_data_dir(&app_handle).join("skills");
    std::fs::create_dir_all(&skills_dir)
        .map_err(|e| format!("Failed to create skills directory: {}", e))?;

    // Download zip file to a temporary location
    let client = reqwest::Client::new();
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download skill: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;

    // Create a unique temporary extraction directory
    let temp_extract_dir = std::env::temp_dir().join(format!("skill_extract_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_extract_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    info!("Extracting zip file to: {}", temp_extract_dir.display());

    // Use zip crate to extract and preserve directory structure
    {
        use std::io::Cursor;

        let cursor = Cursor::new(bytes.as_ref());
        let mut zip = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("Failed to open zip archive: {}", e))?;

        for i in 0..zip.len() {
            let mut file = zip.by_index(i)
                .map_err(|e| format!("Failed to get file from zip: {}", e))?;
            let file_path = temp_extract_dir.join(file.mangled_name());

            // Create parent directories if needed
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create directory: {}", e))?;
            }

            // Skip directories (they will be created when extracting files)
            if file.name().ends_with('/') {
                continue;
            }

            let mut output = std::fs::File::create(&file_path)
                .map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut file, &mut output)
                .map_err(|e| format!("Failed to write file: {}", e))?;

            // Set file permissions if available
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    let mut perms = std::fs::Permissions::from_mode(0o644);
                    perms.set_mode(mode);
                    output.set_permissions(perms)
                        .map_err(|e| format!("Failed to set permissions: {}", e))?;
                }
            }
        }
    }

    // Now move the extracted content to skills directory
    // Check what's in the temp_extract_dir
    let entries = std::fs::read_dir(&temp_extract_dir)
        .map_err(|e| format!("Failed to read temp directory: {}", e))?;

    let entries: Vec<_> = entries.collect::<Result<_, _>>()
        .map_err(|e| format!("Failed to collect directory entries: {}", e))?;

    info!("Found {} entries in extracted zip", entries.len());

    // Move each entry to the skills directory
    for entry in entries {
        let entry_path = entry.path();
        let dest_path = skills_dir.join(entry.file_name());

        // Use rename for efficiency (works if on same filesystem)
        if std::fs::rename(&entry_path, &dest_path).is_err() {
            // If rename fails (cross-device), fall back to copy
            if entry_path.is_dir() {
                copy_dir_recursive(&entry_path, &dest_path)?;
            } else {
                std::fs::copy(&entry_path, &dest_path)
                    .map_err(|e| format!("Failed to copy file: {}", e))?;
            }
        }
    }

    // Clean up the temporary extraction directory
    let _ = std::fs::remove_dir_all(&temp_extract_dir);

    info!("Skill installed successfully to {}", skills_dir.display());
    Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("Failed to read directory: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy file: {}", e))?;
        }
    }
    Ok(())
}

/// Open a URL in the default browser
#[tauri::command]
pub async fn open_source_url(url: String) -> Result<(), String> {
    info!("Opening URL in browser: {}", url);
    if let Err(e) = open::that(&url) {
        warn!(error = ?e, "Failed to open browser automatically");
        // Still return Ok - the user can open manually
    }
    Ok(())
}

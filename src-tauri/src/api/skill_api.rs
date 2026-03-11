//! Skill API - Tauri commands for skill management

use crate::api::ai::config::get_network_proxy_from_config;
use crate::db::skill_db::SkillDatabase;
use crate::skills::installer::{
    copy_dir_recursive, inspect_skill_archive as inspect_archive_internal,
    inspect_skill_install_recipe as inspect_recipe_internal,
    install_skill_archive as install_archive_internal,
    install_skill_install_recipe as install_recipe_internal, load_skill_install_recipe_from_file,
    validate_skill_install_dirs, SkillArchiveInspection, SkillArchiveInstallResult,
    SkillInstallPlan, SkillInstallRecipe, SkillInstallRecipeDir, SkillInstallRecipeSource,
    SkillInstallRecipeSourceType, SkillInstallResult,
};
use crate::skills::parser::SkillParser;
use crate::skills::scanner::SkillScanner;
use crate::skills::types::{ScannedSkill, SkillContent, SkillSourceConfig, SkillWithConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;
use tracing::{debug, error, info, warn};

/// Official skill from the skills store API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialSkill {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub source: Option<SkillInstallRecipeSource>,
    #[serde(default)]
    pub dirs: Vec<SkillInstallRecipeDir>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
}

/// Official skills API endpoint
const OFFICIAL_SKILLS_API: &str = "https://aipp-helper.xiejingyang.com/api/skills";

const OFFICIAL_SKILLS_TIMEOUT_SECS: u64 = 5;

impl OfficialSkill {
    fn normalize(mut self) -> Result<Self, String> {
        let resolved_source = self.resolve_source()?;
        resolved_source.validate()?;
        validate_skill_install_dirs(&self.dirs, false)?;
        if self.source_url.is_none() {
            self.source_url = Some(official_skill_source_url(&resolved_source));
        }
        self.source = Some(resolved_source);
        Ok(self)
    }

    pub fn resolve_source(&self) -> Result<SkillInstallRecipeSource, String> {
        if let Some(source) = &self.source {
            source.validate()?;
            return Ok(source.clone());
        }

        if let Some(download_url) = &self.download_url {
            let source = SkillInstallRecipeSource {
                source_type: SkillInstallRecipeSourceType::Zip,
                repo: None,
                git_ref: "main".to_string(),
                url: Some(download_url.clone()),
            };
            source.validate()?;
            return Ok(source);
        }

        Err(format!("Official skill {} is missing source information and download_url", self.id))
    }
}

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

async fn resolve_proxy_url(
    app_handle: &tauri::AppHandle,
    use_proxy: bool,
) -> Result<Option<String>, String> {
    if !use_proxy {
        return Ok(None);
    }

    let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let proxy_url = get_network_proxy_from_config(&config_feature_map);

    if proxy_url.is_none() {
        return Err("当前未配置网络代理，请先在网络设置中填写代理地址".to_string());
    }

    Ok(proxy_url)
}

fn official_skill_source_url(source: &SkillInstallRecipeSource) -> String {
    match source.source_type {
        SkillInstallRecipeSourceType::GitHub => format!(
            "https://github.com/{}/tree/{}",
            source.repo.as_deref().unwrap_or_default(),
            source.git_ref
        ),
        SkillInstallRecipeSourceType::Zip => source.url.clone().unwrap_or_default(),
    }
}

fn inspection_to_selections(inspection: &SkillArchiveInspection) -> Vec<SkillInstallRecipeDir> {
    inspection
        .skills
        .iter()
        .map(|skill| SkillInstallRecipeDir { from: skill.from.clone(), to: skill.to.clone() })
        .collect()
}

/// Migrate skills from old {app_data}/skills to new ~/.agents/skills
pub fn migrate_skills_to_agents_dir(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let home_dir = get_home_dir();
    let app_data_dir = get_app_data_dir(app_handle);

    let old_skills_dir = app_data_dir.join("skills");
    let new_skills_dir = home_dir.join(".agents/skills");

    debug!("Starting skill migration");
    debug!("  Old dir: {:?}", old_skills_dir);
    debug!("  New dir: {:?}", new_skills_dir);

    // 检查旧目录是否存在
    if !old_skills_dir.exists() {
        debug!("Old skills directory does not exist, skipping migration");
        return Ok(());
    }

    // 检查旧目录是否为目录
    if !old_skills_dir.is_dir() {
        warn!("Old skills path exists but is not a directory, skipping migration");
        return Ok(());
    }

    // 检查旧目录是否为空
    let entries = match std::fs::read_dir(&old_skills_dir) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to read old skills directory: {}", e);
            return Err(format!("Failed to read old skills directory: {}", e));
        }
    };

    let entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();

    if entries.is_empty() {
        debug!("Old skills directory is empty, skipping migration");
        // 删除空目录
        if let Err(e) = std::fs::remove_dir(&old_skills_dir) {
            debug!("Failed to remove empty old skills directory: {}", e);
        }
        return Ok(());
    }

    info!("Found {} items in old skills directory to migrate", entries.len());

    // 创建新目录
    if let Err(e) = std::fs::create_dir_all(&new_skills_dir) {
        error!("Failed to create new skills directory: {}", e);
        return Err(format!("Failed to create new skills directory: {}", e));
    }
    debug!("New skills directory ready");

    // 合并迁移：如果新目录已有同名技能，跳过（新目录优先）
    let mut migrated_count = 0u32;
    let mut skipped_count = 0u32;
    let mut error_count = 0u32;

    for entry in entries {
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dest_path = new_skills_dir.join(&file_name);

        debug!("Processing: {:?}", file_name);

        // 如果目标已存在，跳过
        if dest_path.exists() {
            debug!("  Skipping (already exists in new dir)");
            skipped_count += 1;
            continue;
        }

        // 尝试移动（效率高），失败则复制
        if std::fs::rename(&src_path, &dest_path).is_ok() {
            debug!("  Moved successfully");
            migrated_count += 1;
        } else {
            debug!("  Rename failed, falling back to copy");
            if src_path.is_dir() {
                if let Err(e) = copy_dir_recursive(&src_path, &dest_path) {
                    error!("  Failed to copy directory: {}", e);
                    error_count += 1;
                    continue;
                }
                // 复制成功后删除源目录
                if let Err(e) = std::fs::remove_dir_all(&src_path) {
                    debug!("  Failed to remove source directory after copy: {}", e);
                }
            } else {
                if let Err(e) = std::fs::copy(&src_path, &dest_path) {
                    error!("  Failed to copy file: {}", e);
                    error_count += 1;
                    continue;
                }
                // 复制成功后删除源文件
                if let Err(e) = std::fs::remove_file(&src_path) {
                    debug!("  Failed to remove source file after copy: {}", e);
                }
            }
            debug!("  Copied successfully");
            migrated_count += 1;
        }
    }

    info!(
        "Migration complete: migrated {} skills, skipped {} (already exist), errors {}",
        migrated_count, skipped_count, error_count
    );

    // 检查旧目录是否为空
    if let Ok(mut remaining) = std::fs::read_dir(&old_skills_dir) {
        if remaining.next().is_none() {
            debug!("Old directory is empty, removing it");
            if let Err(e) = std::fs::remove_dir(&old_skills_dir) {
                debug!("Failed to remove old skills directory: {}", e);
            }
        } else {
            debug!("Old directory still has items, keeping it");
        }
    }

    if error_count > 0 {
        return Err(format!("Migration completed with {} errors", error_count));
    }

    Ok(())
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
    let skills_dir = get_home_dir().join(".agents/skills");

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
    let skills_dir = get_home_dir().join(".agents/skills");
    Ok(skills_dir.to_string_lossy().to_string())
}

/// Fetch official skills from the skills store API
#[tauri::command]
pub async fn fetch_official_skills(
    app_handle: tauri::AppHandle,
    use_proxy: bool,
) -> Result<Vec<OfficialSkill>, String> {
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;

    // Build client with optional proxy
    let client = if let Some(ref proxy) = proxy_url {
        info!(proxy_url = %proxy, "Using proxy for fetching official skills");
        let proxy = reqwest::Proxy::all(proxy).map_err(|e| format!("代理配置失败: {}", e))?;
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
            .timeout(Duration::from_secs(OFFICIAL_SKILLS_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    format!(
                        "请求超时（超过{}秒），请尝试使用代理访问",
                        OFFICIAL_SKILLS_TIMEOUT_SECS
                    )
                } else if e.is_connect() {
                    format!("网络连接失败，请检查网络或尝试使用代理")
                } else {
                    format!("获取官方技能列表失败: {}", e)
                }
            })?;

        if !response.status().is_success() {
            return Err(format!("Official skills API returned error: {}", response.status()));
        }

        let skills = response
            .json::<Vec<OfficialSkill>>()
            .await
            .map_err(|e| format!("Failed to parse skills response: {}", e))?;
        let normalized_skills =
            skills.into_iter().map(OfficialSkill::normalize).collect::<Result<Vec<_>, _>>()?;

        info!("Fetched {} official skills", normalized_skills.len());
        Ok::<Vec<OfficialSkill>, String>(normalized_skills)
    };

    fetch_future.await
}

/// Load a JSON skill install recipe from disk.
#[tauri::command]
pub async fn load_skill_install_recipe_file(
    recipe_path: String,
) -> Result<SkillInstallRecipe, String> {
    load_skill_install_recipe_from_file(Path::new(&recipe_path))
}

/// Inspect a skill install recipe and return the resolved installation plan.
#[tauri::command]
pub async fn inspect_skill_install_recipe(
    app_handle: tauri::AppHandle,
    recipe: SkillInstallRecipe,
    use_proxy: bool,
) -> Result<SkillInstallPlan, String> {
    let skills_dir = get_home_dir().join(".agents/skills");
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;
    inspect_recipe_internal(&recipe, &skills_dir, proxy_url.as_deref()).await
}

/// Load a JSON skill install recipe from disk and inspect it.
#[tauri::command]
pub async fn inspect_skill_install_recipe_file(
    app_handle: tauri::AppHandle,
    recipe_path: String,
    use_proxy: bool,
) -> Result<SkillInstallPlan, String> {
    let recipe = load_skill_install_recipe_from_file(Path::new(&recipe_path))?;
    let skills_dir = get_home_dir().join(".agents/skills");
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;
    inspect_recipe_internal(&recipe, &skills_dir, proxy_url.as_deref()).await
}

/// Install skills from a structured recipe and refresh the scanned skills list.
#[tauri::command]
pub async fn install_skill_install_recipe(
    app_handle: tauri::AppHandle,
    recipe: SkillInstallRecipe,
    use_proxy: bool,
) -> Result<SkillInstallResult, String> {
    let skills_dir = get_home_dir().join(".agents/skills");
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;
    let result = install_recipe_internal(&recipe, &skills_dir, proxy_url.as_deref()).await?;
    let _ = scan_skills(app_handle).await?;
    Ok(result)
}

/// Load a JSON skill install recipe from disk, install it, and refresh the scanned skills list.
#[tauri::command]
pub async fn install_skill_install_recipe_file(
    app_handle: tauri::AppHandle,
    recipe_path: String,
    use_proxy: bool,
) -> Result<SkillInstallResult, String> {
    let recipe = load_skill_install_recipe_from_file(Path::new(&recipe_path))?;
    let skills_dir = get_home_dir().join(".agents/skills");
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;
    let result = install_recipe_internal(&recipe, &skills_dir, proxy_url.as_deref()).await?;
    let _ = scan_skills(app_handle).await?;
    Ok(result)
}

/// Inspect a GitHub repository or zip source and return installable skills.
#[tauri::command]
pub async fn inspect_skill_archive_source(
    app_handle: tauri::AppHandle,
    source: SkillInstallRecipeSource,
    dirs: Option<Vec<SkillInstallRecipeDir>>,
    use_proxy: bool,
) -> Result<SkillArchiveInspection, String> {
    let skills_dir = get_home_dir().join(".agents/skills");
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;
    inspect_archive_internal(&source, dirs.as_deref(), &skills_dir, proxy_url.as_deref()).await
}

/// Install selected skills from a GitHub repository or zip source.
#[tauri::command]
pub async fn install_skill_archive_source(
    app_handle: tauri::AppHandle,
    source: SkillInstallRecipeSource,
    selections: Vec<SkillInstallRecipeDir>,
    use_proxy: bool,
) -> Result<SkillArchiveInstallResult, String> {
    let skills_dir = get_home_dir().join(".agents/skills");
    let proxy_url = resolve_proxy_url(&app_handle, use_proxy).await?;
    let result =
        install_archive_internal(&source, &selections, &skills_dir, proxy_url.as_deref()).await?;
    let _ = scan_skills(app_handle).await?;
    Ok(result)
}

/// Backward-compatible wrapper that installs every discovered skill from a zip URL.
#[tauri::command]
pub async fn install_official_skill(
    app_handle: tauri::AppHandle,
    download_url: String,
) -> Result<(), String> {
    let source = SkillInstallRecipeSource {
        source_type: SkillInstallRecipeSourceType::Zip,
        repo: None,
        git_ref: "main".to_string(),
        url: Some(download_url),
    };
    let skills_dir = get_home_dir().join(".agents/skills");
    let inspection = inspect_archive_internal(&source, None, &skills_dir, None).await?;
    let selections = inspection_to_selections(&inspection);
    let _ = install_archive_internal(&source, &selections, &skills_dir, None).await?;
    let _ = scan_skills(app_handle).await?;
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

/// Delete a skill folder from the filesystem
/// Only AIPP-sourced skills can be deleted (not system directories like Claude Code, Codex, etc.)
#[tauri::command]
pub async fn delete_skill(app_handle: tauri::AppHandle, identifier: String) -> Result<(), String> {
    let scanner = create_scanner(&app_handle);

    // Find the skill
    let skill =
        scanner.get_skill(&identifier).ok_or_else(|| format!("Skill not found: {}", identifier))?;

    // Allow deleting Agents-sourced skills
    // (Previously AIPP-sourced, now unified with Agents)

    // Get the skill folder path (parent of the skill file)
    let skill_path = PathBuf::from(&skill.file_path);
    let skill_folder = skill_path.parent().ok_or_else(|| "Invalid skill file path".to_string())?;

    // Check if folder exists
    if !skill_folder.exists() {
        return Err("Skill folder does not exist".to_string());
    }

    // Delete the folder recursively
    std::fs::remove_dir_all(skill_folder)
        .map_err(|e| format!("Failed to delete skill folder: {}", e))?;

    info!("Deleted skill folder: {}", skill_folder.display());
    Ok(())
}

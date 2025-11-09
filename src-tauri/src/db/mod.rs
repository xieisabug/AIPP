use std::path::PathBuf;

use assistant_db::AssistantDatabase;
use conversation_db::ConversationDatabase;
use llm_db::LLMDatabase;
use semver::Version;
use sub_task_db::SubTaskDatabase;
use system_db::SystemDatabase;
use tauri::Manager;
use tracing::{debug, error, info, instrument, warn};

pub mod artifacts_db;
pub mod assistant_db;
pub mod conn_helper;
pub mod conversation_db;
pub mod llm_db;
pub mod mcp_db;
pub mod plugin_db;
pub mod sub_task_db;
pub mod system_db;

#[cfg(test)]
mod tests;

const CURRENT_VERSION: &str = "0.0.5";

pub(crate) fn get_db_path(app_handle: &tauri::AppHandle, db_name: &str) -> Result<PathBuf, String> {
    let app_dir = app_handle.path().app_data_dir().unwrap();
    let db_path = app_dir.join("db");
    std::fs::create_dir_all(&db_path).map_err(|e| e.to_string())?;
    Ok(db_path.join(db_name))
}

#[instrument(level = "info", skip(app_handle))]
pub fn database_upgrade(app_handle: &tauri::AppHandle) -> Result<(), String> {
    info!(target_version = CURRENT_VERSION, "Starting database upgrade check");
    
    // 创建数据库实例（它们会复用全局连接）
    let system_db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let llm_db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    
    let system_version = system_db.get_config(app_handle, "system_version");
    match system_version {
        Ok(version) => {
            if version.is_empty() {
                let _ = system_db.add_system_config(app_handle, "system_version", CURRENT_VERSION);
                info!("Initialized system_version to current version");
                if let Err(err) = system_db.init_feature_config(app_handle) {
                    error!(error = ?err, "init_feature_config failed");
                } else {
                    info!("Feature configs initialized");
                }
            } else {
                // 临时逻辑
                let now_version;
                if version == "0.1" {
                    let _ = system_db.delete_system_config(app_handle, "system_version");
                    let _ = system_db.add_system_config(app_handle, "system_version", "0.0.1");
                    now_version = "0.0.1";
                } else {
                    now_version = version.as_str();
                }
                info!(current_version = now_version, "Detected existing system version");

                let current_version = Version::parse(now_version).unwrap();

                // 定义需要执行特殊逻辑的版本
                let special_versions: Vec<(
                    &str,
                    fn(
                        &SystemDatabase,
                        &LLMDatabase,
                        &AssistantDatabase,
                        &ConversationDatabase,
                        &tauri::AppHandle,
                    ) -> Result<(), String>,
                )> = vec![
                    ("0.0.2", special_logic_0_0_2),
                    ("0.0.3", special_logic_0_0_3),
                    ("0.0.4", special_logic_0_0_4),
                    ("0.0.5", special_logic_0_0_5),
                ];

                for (version_str, logic) in special_versions.iter() {
                    let version = Version::parse(version_str).unwrap();
                    if current_version < version {
                        info!(target = version_str, "Executing special logic for version");
                        let start = std::time::Instant::now();
                        match logic(&system_db, &llm_db, &assistant_db, &conversation_db, app_handle) {
                            Ok(_) => {
                                info!(
                                    target = version_str,
                                    elapsed_ms = start.elapsed().as_millis() as u64,
                                    "Special logic completed"
                                );
                            }
                            Err(err) => {
                                error!(target = version_str, error = ?err, "Special logic failed; exiting");
                                app_handle.exit(-1);
                            }
                        }
                    } else {
                        debug!(
                            target = version_str,
                            "Skipping special logic; already at or past version"
                        );
                    }
                }

                let _ = system_db.update_system_config(app_handle, "system_version", CURRENT_VERSION);
                info!("System version updated to current");
            }
        }
        Err(err) => {
            error!(error = ?err, "Failed to read system_version");
        }
    }

    Ok(())
}

fn special_logic_0_0_2(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    // 已迁移到 SeaORM，旧版 rusqlite 逻辑不再需要
    info!("special_logic_0_0_2 skipped: legacy rusqlite migration removed");
    Ok(())
}

fn special_logic_0_0_4(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_4: 清理废弃的assistant消息类型");

    // 旧版清理逻辑仅针对历史数据，SeaORM 版本中不再需要；保持幂等
    info!("special_logic_0_0_4 skipped: legacy rusqlite cleanup removed");
    Ok(())
}

fn special_logic_0_0_3(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    conversation_db: &ConversationDatabase,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_3: 添加 generation_group_id 字段并更新现有数据");

    info!("special_logic_0_0_3 skipped: generation_group_id already managed by new schema");
    Ok(())
}

fn special_logic_0_0_5(
    _system_db: &SystemDatabase,
    _llm_db: &LLMDatabase,
    _assistant_db: &AssistantDatabase,
    _conversation_db: &ConversationDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    info!("special_logic_0_0_5: 创建 sub task 相关表");

    // 创建 sub task 相关表（现在是静态方法）
    SubTaskDatabase::create_tables(app_handle).map_err(|e| format!("创建 sub task 表失败: {}", e.to_string()))?;

    info!("special_logic_0_0_5 done: sub task 表创建完成");
    Ok(())
}

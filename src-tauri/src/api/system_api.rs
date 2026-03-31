use base64::Engine;
use chrono::Utc;
use serde::Serialize;
use std::cmp::Ord;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager, State};
use tracing::warn;

use crate::scheduler::SchedulerState;
use crate::template_engine::{build_template_engine, BangType};
use crate::AppState;
use crate::FeatureConfigState;
use crate::SyncState;

use crate::db::connection::{
    database_namespace, open_remote_database_connection, params, sync_metadata_path,
    sync_url_for_database, Connection, DatabaseManager, DatabaseMode, ManagedDatabaseState,
    OptionalExtension,
};
use crate::db::sync_manager::{SyncConfig, SyncManager};
use crate::db::system_db::{FeatureConfig, SystemDatabase};

#[derive(Serialize)]
pub struct ExperimentalSummaryTaskStatus {
    pub mcp_running: bool,
    pub assistant_running: bool,
    pub conversation_running: bool,
    pub conversation_running_count: usize,
}

#[derive(Serialize)]
pub struct SummaryTriggerResult {
    pub started: bool,
    pub already_running: bool,
    pub message: String,
}

#[tauri::command]
pub async fn get_all_feature_config(
    state: State<'_, FeatureConfigState>,
) -> Result<Vec<FeatureConfig>, String> {
    let configs = state.configs.lock().await;
    Ok(configs.clone())
}

#[tauri::command]
pub async fn save_feature_config(
    app_handle: tauri::AppHandle,
    state: State<'_, FeatureConfigState>,
    feature_code: String,
    config: HashMap<String, String>,
) -> Result<(), String> {
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let _ = db.delete_feature_config_by_feature_code(feature_code.as_str());
    for (key, value) in config.iter() {
        db.add_feature_config(&FeatureConfig {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        })
        .map_err(|e| e.to_string())?;
    }

    // 更新内存状态
    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;

    // 删除旧的配置
    configs.retain(|c| c.feature_code != feature_code);
    config_feature_map.remove(&feature_code);

    // 添加新的配置
    for (key, value) in config.iter() {
        let new_config = FeatureConfig {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(new_config.clone());
        config_feature_map
            .entry(feature_code.clone())
            .or_insert(HashMap::new())
            .insert(key.clone(), new_config);
    }
    // 如果更新的是快捷键配置，则尝试重新注册全局快捷键（异步，避免阻塞 runtime）
    #[cfg(desktop)]
    if feature_code == "shortcuts" {
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            crate::reconfigure_global_shortcuts_async(&app).await;
        });
    }

    // 发出配置变更事件，通知所有窗口重新加载配置
    let _ = app_handle.emit("feature_config_changed", ());

    if feature_code == "experimental" {
        crate::feishu::refresh_runtime_async(&app_handle);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_sync_config(state: State<'_, SyncState>) -> Result<SyncConfig, String> {
    Ok(state.config.lock().await.clone())
}

#[tauri::command]
pub async fn save_sync_config(
    app_handle: tauri::AppHandle,
    state: State<'_, SyncState>,
    mut config: SyncConfig,
) -> Result<(), String> {
    if config.enabled
        && !config.initial_sync_completed
        && matches!(
            config.first_sync_strategy,
            crate::db::sync_manager::FirstSyncStrategy::AppendLocal
        )
    {
        return Err(
            "当前 sqld 直连同步暂不支持 AppendLocal 合并，请改用“使用云端数据”或“上传本设备数据”。"
                .to_string(),
        );
    }

    let previous = state.config.lock().await.clone();
    if previous.server_url != config.server_url || previous.auth_token != config.auth_token {
        config.initial_sync_completed = false;
        config.last_sync_status = crate::db::sync_manager::SyncStatus::Never;
        config.last_sync_message = Some("同步目标已变更，请重新完成首次同步".to_string());
        config.last_sync_started_at = None;
        config.last_sync_finished_at = None;
    }

    if !config.enabled {
        config.last_sync_message = Some("当前为纯本地模式，不会连接远端。".to_string());
    } else if !config.initial_sync_completed {
        config.last_sync_message = Some(
            "请选择首次同步策略，然后点击“立即同步”；后台不会自动代你执行首次同步。".to_string(),
        );
    }

    SyncManager::save_for_app(&app_handle, &config)?;

    {
        let mut current = state.config.lock().await;
        *current = config.clone();
    }
    state.set_runtime_force_local(false).await;

    let manager = DatabaseManager::new(
        (*state.db_dir).clone(),
        SyncManager::new(config.clone()).to_database_mode(),
    );
    let managed_state =
        ManagedDatabaseState::global().ok_or_else(|| "数据库管理器尚未初始化".to_string())?;
    managed_state.replace(manager);

    let _ = app_handle.emit("sync_config_changed", &config);
    Ok(())
}

#[tauri::command]
pub async fn run_sync_now(
    app_handle: tauri::AppHandle,
    state: State<'_, SyncState>,
) -> Result<String, String> {
    {
        let mut current = state.config.lock().await;
        if !current.enabled {
            return Err("请先启用多端同步".to_string());
        }
        current.last_sync_started_at = Some(crate::current_sync_timestamp());
        current.last_sync_message = Some("正在执行手动同步…".to_string());
        let snapshot = current.clone();
        drop(current);
        SyncManager::save_for_app(&app_handle, &snapshot)?;
        let _ = app_handle.emit("sync_status_changed", &snapshot);
    }

    let config = state.config.lock().await.clone();
    if !config.enabled {
        return Err("请先启用多端同步".to_string());
    }
    if !config.initial_sync_completed
        && matches!(
            config.first_sync_strategy,
            crate::db::sync_manager::FirstSyncStrategy::AppendLocal
        )
    {
        return Err(
            "当前 sqld 直连同步暂不支持 AppendLocal 合并，请改用“使用云端数据”或“上传本设备数据”。"
                .to_string(),
        );
    }

    let managed_state =
        ManagedDatabaseState::global().ok_or_else(|| "数据库管理器尚未初始化".to_string())?;
    let desired_mode = SyncManager::new(config.clone()).to_database_mode();
    managed_state.replace(DatabaseManager::new((*state.db_dir).clone(), desired_mode.clone()));
    state.set_runtime_force_local(false).await;
    let result = run_sync_now_internal(
        &app_handle,
        &managed_state,
        state.db_dir.as_ref().as_path(),
        &config,
    );

    let mut current = state.config.lock().await;
    current.last_sync_finished_at = Some(crate::current_sync_timestamp());
    let message = match result {
        Ok(message) => {
            current.last_sync_status = crate::db::sync_manager::SyncStatus::Success;
            if !current.initial_sync_completed {
                current.initial_sync_completed = true;
            }
            message
        }
        Err(err) => {
            current.last_sync_status = crate::db::sync_manager::SyncStatus::Error;
            if matches!(desired_mode, DatabaseMode::Synced { .. }) {
                managed_state.replace(DatabaseManager::new((*state.db_dir).clone(), DatabaseMode::Local));
                state.set_runtime_force_local(true).await;
            }
            format!("同步失败：{err}")
        }
    };
    current.last_sync_message = Some(message.clone());
    let snapshot = current.clone();
    drop(current);
    SyncManager::save_for_app(&app_handle, &snapshot)?;
    let _ = app_handle.emit("sync_status_changed", &snapshot);
    let _ = app_handle.emit("sync_run_completed", &message);

    if matches!(snapshot.last_sync_status, crate::db::sync_manager::SyncStatus::Error) {
        return Err(message);
    }

    Ok(message)
}

#[tauri::command]
pub async fn reset_sync_onboarding(
    app_handle: tauri::AppHandle,
    state: State<'_, SyncState>,
) -> Result<(), String> {
    let mut current = state.config.lock().await;
    current.initial_sync_completed = false;
    current.last_sync_status = crate::db::sync_manager::SyncStatus::Never;
    current.last_sync_message = Some("已重置首次同步向导，请重新选择首次同步策略。".to_string());
    current.last_sync_started_at = None;
    current.last_sync_finished_at = None;
    let snapshot = current.clone();
    drop(current);
    state.set_runtime_force_local(false).await;
    SyncManager::save_for_app(&app_handle, &snapshot)?;
    let _ = app_handle.emit("sync_status_changed", &snapshot);
    Ok(())
}

#[derive(Debug)]
struct SchemaObject {
    object_type: String,
    name: String,
    sql: String,
}

fn run_sync_now_internal(
    app_handle: &tauri::AppHandle,
    managed_state: &ManagedDatabaseState,
    db_dir: &Path,
    config: &SyncConfig,
) -> Result<String, String> {
    crate::artifacts::artifact_data_db::ArtifactDataDatabase::migrate_all_legacy_databases(
        app_handle,
    )?;

    if !config.initial_sync_completed {
        return run_first_sync(app_handle, managed_state, db_dir, config);
    }

    managed_state.sync_all().map_err(|err| err.to_string())?;
    Ok("手动同步已完成".to_string())
}

fn run_first_sync(
    app_handle: &tauri::AppHandle,
    managed_state: &ManagedDatabaseState,
    db_dir: &Path,
    config: &SyncConfig,
) -> Result<String, String> {
    let synced_mode = synced_database_mode(config)?;
    let db_names =
        DatabaseManager::discover_database_names(db_dir).map_err(|err| err.to_string())?;

    match config.first_sync_strategy {
        crate::db::sync_manager::FirstSyncStrategy::UseRemote => run_use_remote_first_sync(
            app_handle,
            managed_state,
            db_dir,
            &synced_mode,
            &db_names,
            false,
        ),
        crate::db::sync_manager::FirstSyncStrategy::BackupThenUseRemote => {
            run_use_remote_first_sync(
                app_handle,
                managed_state,
                db_dir,
                &synced_mode,
                &db_names,
                true,
            )
        }
        crate::db::sync_manager::FirstSyncStrategy::UseLocal => {
            run_use_local_first_sync(app_handle, managed_state, db_dir, &synced_mode, &db_names)
        }
        crate::db::sync_manager::FirstSyncStrategy::AppendLocal => Err(
            "当前 sqld 直连同步暂不支持 AppendLocal 合并，请改用“使用云端数据”或“上传本设备数据”。"
                .to_string(),
        ),
    }
}

fn synced_database_mode(config: &SyncConfig) -> Result<DatabaseMode, String> {
    match (config.server_url.as_deref(), config.auth_token.as_deref()) {
        (Some(url), Some(auth_token))
            if !url.trim().is_empty() && !auth_token.trim().is_empty() =>
        {
            Ok(DatabaseMode::Synced {
                url: url.trim().to_string(),
                auth_token: auth_token.trim().to_string(),
            })
        }
        _ => Err("同步配置不完整，请先填写有效的服务器地址与访问令牌".to_string()),
    }
}

fn run_use_remote_first_sync(
    app_handle: &tauri::AppHandle,
    managed_state: &ManagedDatabaseState,
    db_dir: &Path,
    synced_mode: &DatabaseMode,
    db_names: &[String],
    keep_backup: bool,
) -> Result<String, String> {
    preflight_remote_namespaces(synced_mode, db_names)?;

    let backup_dir =
        create_sync_backup_dir(app_handle, if keep_backup { "use-remote" } else { "rollback" })?;
    let desired_mode = synced_mode.clone();

    managed_state.replace(DatabaseManager::new(db_dir.to_path_buf(), DatabaseMode::Local));

    let sync_result = (|| -> Result<String, String> {
        backup_database_files(db_dir, db_names, &backup_dir)?;
        for db_name in db_names {
            remove_database_files(db_dir, db_name)?;
        }

        let synced_manager = DatabaseManager::new(db_dir.to_path_buf(), desired_mode.clone());
        for db_name in db_names {
            synced_manager.sync(db_name).map_err(|err| {
                format_sync_namespace_error(&desired_mode, db_name, err.to_string())
            })?;
        }

        managed_state.replace(synced_manager);

        if keep_backup {
            Ok(format!(
                "{}；本地备份已保存在 `{}`",
                crate::describe_first_sync_strategy(
                    &crate::db::sync_manager::FirstSyncStrategy::BackupThenUseRemote
                ),
                backup_dir.display()
            ))
        } else {
            let mut message = format!(
                "{}；首次同步已完成",
                crate::describe_first_sync_strategy(
                    &crate::db::sync_manager::FirstSyncStrategy::UseRemote
                )
            );
            if let Err(err) = std::fs::remove_dir_all(&backup_dir) {
                warn!(
                    error = %err,
                    path = %backup_dir.display(),
                    "Failed to remove temporary first-sync rollback backup"
                );
                message
                    .push_str(&format!("；临时回滚备份未能自动删除：`{}`", backup_dir.display()));
            }
            Ok(message)
        }
    })();

    match sync_result {
        Ok(message) => Ok(message),
        Err(err) => {
            if let Err(restore_err) = restore_database_files(db_dir, db_names, &backup_dir) {
                return Err(format!("{err}；回滚本地数据库失败：{restore_err}"));
            }
            managed_state.replace(DatabaseManager::new(db_dir.to_path_buf(), desired_mode));
            Err(err)
        }
    }
}

fn run_use_local_first_sync(
    app_handle: &tauri::AppHandle,
    managed_state: &ManagedDatabaseState,
    db_dir: &Path,
    synced_mode: &DatabaseMode,
    db_names: &[String],
) -> Result<String, String> {
    let backup_dir = create_sync_backup_dir(app_handle, "use-local")?;
    let remote_backup_dir = backup_dir.join("remote-snapshot");
    std::fs::create_dir_all(&remote_backup_dir)
        .map_err(|err| format!("创建远端快照目录 `{}` 失败：{err}", remote_backup_dir.display()))?;
    let desired_mode = synced_mode.clone();
    snapshot_remote_namespaces(&desired_mode, db_names, &remote_backup_dir)?;

    managed_state.replace(DatabaseManager::new(db_dir.to_path_buf(), DatabaseMode::Local));

    let sync_result = (|| -> Result<String, String> {
        backup_database_files(db_dir, db_names, &backup_dir)?;

        let synced_manager = DatabaseManager::new(db_dir.to_path_buf(), desired_mode.clone());
        let DatabaseMode::Synced { url, auth_token } = &desired_mode else {
            return Err("当前未启用远端同步".to_string());
        };

        for db_name in db_names {
            wipe_remote_namespace(url, auth_token, db_name)?;
            remove_database_files(db_dir, db_name)?;
            synced_manager.sync(db_name).map_err(|err| {
                format_sync_namespace_error(&desired_mode, db_name, err.to_string())
            })?;

            let backup_db_path = backup_dir.join(db_name);
            if backup_db_path.exists() {
                let target_conn = synced_manager
                    .connect(db_name)
                    .map_err(|err| format!("打开目标数据库 `{db_name}` 失败：{err}"))?;
                copy_database_contents_from_backup(&backup_db_path, &target_conn)?;
                drop(target_conn);
            }

            synced_manager.sync(db_name).map_err(|err| {
                format_sync_namespace_error(&desired_mode, db_name, err.to_string())
            })?;
        }

        managed_state.replace(synced_manager);
        let mut message = format!(
            "{}；首次同步已完成",
            crate::describe_first_sync_strategy(
                &crate::db::sync_manager::FirstSyncStrategy::UseLocal
            )
        );
        if let Err(err) = std::fs::remove_dir_all(&backup_dir) {
            warn!(
                error = %err,
                path = %backup_dir.display(),
                "Failed to remove temporary first-sync rollback backup"
            );
            message.push_str(&format!("；临时回滚备份未能自动删除：`{}`", backup_dir.display()));
        }
        Ok(message)
    })();

    match sync_result {
        Ok(message) => Ok(message),
        Err(err) => {
            if let Err(restore_err) = restore_database_files(db_dir, db_names, &backup_dir) {
                return Err(format!("{err}；回滚本地数据库失败：{restore_err}"));
            }
            if let Err(remote_restore_err) =
                restore_remote_namespaces(&desired_mode, &remote_backup_dir, db_names)
            {
                return Err(format!("{err}；回滚远端 namespace 失败：{remote_restore_err}"));
            }
            managed_state.replace(DatabaseManager::new(db_dir.to_path_buf(), desired_mode));
            Err(err)
        }
    }
}

fn create_sync_backup_dir(app_handle: &tauri::AppHandle, suffix: &str) -> Result<PathBuf, String> {
    let app_dir = app_handle.path().app_data_dir().map_err(|err| err.to_string())?;
    let backup_dir = app_dir.join("sync-backups").join(format!(
        "{}-{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        suffix
    ));
    std::fs::create_dir_all(&backup_dir)
        .map_err(|err| format!("创建同步备份目录 `{}` 失败：{err}", backup_dir.display()))?;
    Ok(backup_dir)
}

fn backup_database_files(
    db_dir: &Path,
    db_names: &[String],
    backup_dir: &Path,
) -> Result<(), String> {
    for db_name in db_names {
        for source_path in database_related_paths(db_dir, db_name) {
            if !source_path.exists() {
                continue;
            }

            let target_path = backup_dir.join(
                source_path
                    .file_name()
                    .ok_or_else(|| format!("无法解析数据库文件名：`{}`", source_path.display()))?,
            );
            std::fs::copy(&source_path, &target_path).map_err(|err| {
                format!(
                    "备份数据库文件 `{}` 到 `{}` 失败：{err}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn restore_database_files(
    db_dir: &Path,
    db_names: &[String],
    backup_dir: &Path,
) -> Result<(), String> {
    for db_name in db_names {
        remove_database_files(db_dir, db_name)?;
        for backup_path in database_related_paths(backup_dir, db_name) {
            if !backup_path.exists() {
                continue;
            }

            let target_path = db_dir.join(
                backup_path
                    .file_name()
                    .ok_or_else(|| format!("无法解析备份文件名：`{}`", backup_path.display()))?,
            );
            std::fs::copy(&backup_path, &target_path).map_err(|err| {
                format!(
                    "恢复数据库文件 `{}` 到 `{}` 失败：{err}",
                    backup_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn remove_database_files(dir: &Path, db_name: &str) -> Result<(), String> {
    for path in database_related_paths(dir, db_name) {
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|err| format!("删除数据库文件 `{}` 失败：{err}", path.display()))?;
        }
    }
    Ok(())
}

fn database_related_paths(dir: &Path, db_name: &str) -> Vec<PathBuf> {
    let db_path = dir.join(db_name);
    vec![
        db_path.clone(),
        PathBuf::from(format!("{}-wal", db_path.to_string_lossy())),
        PathBuf::from(format!("{}-shm", db_path.to_string_lossy())),
        sync_metadata_path(&db_path),
    ]
}

fn preflight_remote_namespaces(
    synced_mode: &DatabaseMode,
    db_names: &[String],
) -> Result<(), String> {
    let DatabaseMode::Synced { url, auth_token } = synced_mode else {
        return Ok(());
    };

    for db_name in db_names {
        let conn = open_remote_database_connection(url, auth_token, db_name)
            .map_err(|err| format_sync_namespace_error(synced_mode, db_name, err.to_string()))?;
        conn.query_row("SELECT 1", (), |row| row.get::<_, i64>(0))
            .map_err(|err| format_sync_namespace_error(synced_mode, db_name, err.to_string()))?;
    }

    Ok(())
}

fn snapshot_remote_namespaces(
    synced_mode: &DatabaseMode,
    db_names: &[String],
    snapshot_dir: &Path,
) -> Result<(), String> {
    let snapshot_manager = DatabaseManager::new(snapshot_dir.to_path_buf(), synced_mode.clone());

    for db_name in db_names {
        match snapshot_manager.sync(db_name) {
            Ok(()) => {}
            Err(err) if is_namespace_missing_error(&err.to_string()) => {
                warn!(
                    error = %err,
                    db_name,
                    "Remote namespace missing while creating rollback snapshot; treating it as empty"
                );
            }
            Err(err) => {
                return Err(format_sync_namespace_error(synced_mode, db_name, err.to_string()));
            }
        }
    }

    Ok(())
}

fn restore_remote_namespaces(
    synced_mode: &DatabaseMode,
    snapshot_dir: &Path,
    db_names: &[String],
) -> Result<(), String> {
    let restore_dir = snapshot_dir.join("restore-work");
    std::fs::create_dir_all(&restore_dir)
        .map_err(|err| format!("创建远端回滚工作目录 `{}` 失败：{err}", restore_dir.display()))?;
    let restore_manager = DatabaseManager::new(restore_dir.clone(), synced_mode.clone());
    let DatabaseMode::Synced { url, auth_token } = synced_mode else {
        return Ok(());
    };

    for db_name in db_names {
        let source_db_path = snapshot_dir.join(db_name);
        if !source_db_path.exists() {
            continue;
        }

        wipe_remote_namespace(url, auth_token, db_name)?;
        remove_database_files(&restore_dir, db_name)?;
        restore_manager
            .sync(db_name)
            .map_err(|err| format_sync_namespace_error(synced_mode, db_name, err.to_string()))?;
        let target_conn = restore_manager
            .connect(db_name)
            .map_err(|err| format!("打开远端回滚数据库 `{db_name}` 失败：{err}"))?;
        copy_database_contents_from_backup(&source_db_path, &target_conn)?;
        drop(target_conn);
        restore_manager
            .sync(db_name)
            .map_err(|err| format_sync_namespace_error(synced_mode, db_name, err.to_string()))?;
    }

    Ok(())
}

fn wipe_remote_namespace(base_url: &str, auth_token: &str, db_name: &str) -> Result<(), String> {
    let conn = open_remote_database_connection(base_url, auth_token, db_name).map_err(|err| {
        format_sync_namespace_error(
            &DatabaseMode::Synced { url: base_url.to_string(), auth_token: auth_token.to_string() },
            db_name,
            err.to_string(),
        )
    })?;

    conn.execute_batch("PRAGMA foreign_keys=OFF;")
        .map_err(|err| format!("禁用远端外键检查失败：{err}"))?;

    let mut stmt = conn
        .prepare(
            "SELECT type, name
             FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%'
               AND type IN ('trigger', 'view', 'table')
             ORDER BY CASE type
                 WHEN 'trigger' THEN 0
                 WHEN 'view' THEN 1
                 WHEN 'table' THEN 2
                 ELSE 3
             END, name",
        )
        .map_err(|err| format!("读取远端对象列表失败：{err}"))?;
    let rows = stmt
        .query_map((), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|err| format!("查询远端对象列表失败：{err}"))?;

    let mut objects = Vec::new();
    for row in rows {
        objects.push(row.map_err(|err| format!("读取远端对象失败：{err}"))?);
    }

    for (object_type, object_name) in objects {
        let drop_sql = format!(
            "DROP {} IF EXISTS {};",
            object_type.to_uppercase(),
            quote_identifier(&object_name)
        );
        conn.execute_batch(&drop_sql)
            .map_err(|err| format!("清空远端对象 `{object_name}` 失败：{err}"))?;
    }

    conn.execute_batch("PRAGMA foreign_keys=ON;")
        .map_err(|err| format!("恢复远端外键检查失败：{err}"))?;
    Ok(())
}

fn copy_database_contents_from_backup(
    source_path: &Path,
    target_conn: &Connection,
) -> Result<(), String> {
    let source = source_path
        .to_str()
        .ok_or_else(|| format!("数据库路径不是有效 UTF-8：`{}`", source_path.display()))?;
    let attach_sql = format!("ATTACH DATABASE '{}' AS legacy;", quote_sql_literal(source));

    target_conn
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .map_err(|err| format!("禁用目标数据库外键检查失败：{err}"))?;
    target_conn
        .execute_batch(&attach_sql)
        .map_err(|err| format!("挂载备份数据库 `{}` 失败：{err}", source_path.display()))?;

    let copy_result = (|| -> Result<(), String> {
        let mut stmt = target_conn
            .prepare(
                "SELECT type, name, sql
                 FROM legacy.sqlite_master
                 WHERE sql IS NOT NULL
                   AND name NOT LIKE 'sqlite_%'
                   AND type IN ('table', 'view', 'index', 'trigger')
                 ORDER BY CASE type
                     WHEN 'table' THEN 0
                     WHEN 'view' THEN 1
                     WHEN 'index' THEN 2
                     WHEN 'trigger' THEN 3
                     ELSE 4
                 END, name",
            )
            .map_err(|err| format!("读取本地备份 schema 失败：{err}"))?;

        let rows = stmt
            .query_map((), |row| {
                Ok(SchemaObject { object_type: row.get(0)?, name: row.get(1)?, sql: row.get(2)? })
            })
            .map_err(|err| format!("查询本地备份 schema 失败：{err}"))?;

        let mut objects = Vec::new();
        for row in rows {
            objects.push(row.map_err(|err| format!("读取 schema 对象失败：{err}"))?);
        }

        for object in objects.iter().filter(|object| object.object_type == "table") {
            target_conn
                .execute_batch(&object.sql)
                .map_err(|err| format!("创建表 `{}` 失败：{err}", object.name))?;
        }

        for object in objects.iter().filter(|object| object.object_type == "table") {
            let quoted_name = quote_identifier(&object.name);
            let insert_sql =
                format!("INSERT INTO main.{quoted_name} SELECT * FROM legacy.{quoted_name};");
            target_conn
                .execute_batch(&insert_sql)
                .map_err(|err| format!("导入表 `{}` 数据失败：{err}", object.name))?;
        }

        copy_sqlite_sequence(target_conn)?;

        for object_type in ["view", "index", "trigger"] {
            for object in objects.iter().filter(|object| object.object_type == object_type) {
                target_conn
                    .execute_batch(&object.sql)
                    .map_err(|err| format!("恢复 {} `{}` 失败：{err}", object_type, object.name))?;
            }
        }

        Ok(())
    })();

    if let Err(err) = target_conn.execute_batch("DETACH DATABASE legacy;") {
        warn!(error = %err, "Failed to detach legacy database after first-sync import");
    }
    if let Err(err) = target_conn.execute_batch("PRAGMA foreign_keys=ON;") {
        warn!(error = %err, "Failed to re-enable foreign key checks after first-sync import");
    }

    copy_result
}

fn copy_sqlite_sequence(target_conn: &Connection) -> Result<(), String> {
    let has_legacy_sequence = target_conn
        .query_row(
            "SELECT name FROM legacy.sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'",
            (),
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| format!("检查 legacy.sqlite_sequence 失败：{err}"))?
        .is_some();

    if !has_legacy_sequence {
        return Ok(());
    }

    let has_main_sequence = target_conn
        .query_row(
            "SELECT name FROM main.sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'",
            (),
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| format!("检查 main.sqlite_sequence 失败：{err}"))?
        .is_some();

    if !has_main_sequence {
        return Ok(());
    }

    target_conn
        .execute_batch("DELETE FROM main.sqlite_sequence;")
        .map_err(|err| format!("清理 main.sqlite_sequence 失败：{err}"))?;

    let mut stmt = target_conn
        .prepare("SELECT name, seq FROM legacy.sqlite_sequence ORDER BY name")
        .map_err(|err| format!("读取 legacy.sqlite_sequence 失败：{err}"))?;
    let rows = stmt
        .query_map((), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|err| format!("查询 legacy.sqlite_sequence 失败：{err}"))?;

    for row in rows {
        let (name, seq) = row.map_err(|err| format!("读取 sqlite_sequence 行失败：{err}"))?;
        target_conn
            .execute(
                "INSERT INTO main.sqlite_sequence(name, seq) VALUES (?1, ?2)",
                params![name, seq],
            )
            .map_err(|err| format!("恢复 sqlite_sequence 失败：{err}"))?;
    }

    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn format_sync_namespace_error(mode: &DatabaseMode, db_name: &str, error: String) -> String {
    let DatabaseMode::Synced { url, .. } = mode else {
        return error;
    };

    let sync_url = sync_url_for_database(url, db_name).unwrap_or_else(|_| url.clone());
    let namespace = database_namespace(db_name);
    let mut message =
        format!("数据库 `{db_name}` 无法同步到远端 namespace `{namespace}`（{sync_url}）：{error}");

    if error.contains("404") || error.contains("Not Found") || error.contains("not found") {
        message.push_str("。请确认 sqld 已开启 namespaces，并已为每个数据库创建对应的 namespace。");
    }

    message
}

fn is_namespace_missing_error(error: &str) -> bool {
    error.contains("404") || error.contains("Not Found") || error.contains("not found")
}

#[tauri::command]
pub async fn save_butler_feishu_secret(
    app_handle: tauri::AppHandle,
    app_secret: String,
) -> Result<(), String> {
    crate::feishu::save_feishu_secret(&app_handle, &app_secret)?;
    crate::feishu::refresh_runtime_async(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn clear_butler_feishu_secret(app_handle: tauri::AppHandle) -> Result<(), String> {
    crate::feishu::clear_feishu_secret(&app_handle)?;
    crate::feishu::refresh_runtime_async(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn get_butler_feishu_runtime_status(
    app_handle: tauri::AppHandle,
) -> Result<crate::feishu::FeishuRuntimeStatus, String> {
    crate::feishu::get_runtime_status(&app_handle).await
}

#[tauri::command]
pub async fn refresh_butler_feishu_runtime_command(
    app_handle: tauri::AppHandle,
) -> Result<crate::feishu::FeishuRuntimeStatus, String> {
    crate::feishu::refresh_runtime(&app_handle).await
}

#[tauri::command]
pub async fn get_experimental_summary_task_status(
    scheduler_state: State<'_, SchedulerState>,
) -> Result<ExperimentalSummaryTaskStatus, String> {
    let conversation_running_count =
        crate::scheduler::get_conversation_summary_running_count(&scheduler_state).await;
    Ok(ExperimentalSummaryTaskStatus {
        mcp_running: crate::mcp::summarizer::is_mcp_summary_running(),
        assistant_running: crate::api::assistant_summary_api::is_assistant_summary_running(),
        conversation_running: conversation_running_count > 0,
        conversation_running_count,
    })
}

#[tauri::command]
pub async fn trigger_mcp_summary_generation(
    app_handle: tauri::AppHandle,
) -> Result<SummaryTriggerResult, String> {
    let started = crate::mcp::summarizer::start_mcp_summary_generation(app_handle).await?;
    Ok(if started {
        SummaryTriggerResult {
            started: true,
            already_running: false,
            message: "MCP 总结任务已在后台启动".to_string(),
        }
    } else {
        SummaryTriggerResult {
            started: false,
            already_running: true,
            message: "MCP 总结任务正在进行中".to_string(),
        }
    })
}

#[tauri::command]
pub async fn trigger_assistant_summary_generation(
    app_handle: tauri::AppHandle,
) -> Result<SummaryTriggerResult, String> {
    let started =
        crate::api::assistant_summary_api::start_assistant_summary_generation(app_handle).await?;
    Ok(if started {
        SummaryTriggerResult {
            started: true,
            already_running: false,
            message: "助手画像生成任务已在后台启动".to_string(),
        }
    } else {
        SummaryTriggerResult {
            started: false,
            already_running: true,
            message: "助手画像生成任务正在进行中".to_string(),
        }
    })
}

#[tauri::command]
pub async fn trigger_conversation_summary_generation(
    app_handle: tauri::AppHandle,
    scheduler_state: State<'_, SchedulerState>,
) -> Result<SummaryTriggerResult, String> {
    let running_count =
        crate::scheduler::get_conversation_summary_running_count(&scheduler_state).await;
    if running_count > 0 {
        return Ok(SummaryTriggerResult {
            started: false,
            already_running: true,
            message: format!("已有 {} 个对话总结任务正在进行中", running_count),
        });
    }

    crate::scheduler::run_conversation_summary_now(&app_handle, &scheduler_state)
        .await
        .map_err(|e| e.to_string())?;

    let started_count =
        crate::scheduler::get_conversation_summary_running_count(&scheduler_state).await;
    Ok(if started_count > 0 {
        SummaryTriggerResult {
            started: true,
            already_running: false,
            message: format!("已在后台启动 {} 个对话总结任务", started_count),
        }
    } else {
        SummaryTriggerResult {
            started: false,
            already_running: false,
            message: "当前没有需要立即处理的对话总结任务".to_string(),
        }
    })
}

#[tauri::command]
pub async fn debug_resend_message_to_feishu(
    app_handle: tauri::AppHandle,
    message_id: i64,
) -> Result<crate::feishu::FeishuDebugSendResult, String> {
    crate::feishu::resend_message_to_feishu_for_debug(&app_handle, message_id).await
}

#[tauri::command]
pub async fn conversation_has_feishu_target(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<bool, String> {
    crate::feishu::conversation_has_feishu_target(&app_handle, conversation_id)
}

#[tauri::command]
pub async fn open_data_folder(app: tauri::AppHandle) -> Result<(), String> {
    let app_dir = app.path().app_data_dir().unwrap();
    let db_path = app_dir.join("db");
    if let Err(e) = open::that(db_path) {
        return Err(format!("无法打开数据文件夹: {}", e));
    }
    Ok(())
}

#[tauri::command]
pub async fn get_bang_list(
    app_handle: tauri::AppHandle,
) -> Result<Vec<(String, String, String, BangType)>, String> {
    let engine = build_template_engine(&app_handle)?;
    let mut list = vec![];
    for bang in engine.get_commands().iter() {
        list.push((
            bang.name.clone(),
            bang.complete.clone(),
            bang.description.clone(),
            bang.bang_type.clone(),
        ));
    }
    list.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(list)
}
#[tauri::command]
pub async fn get_selected_text_api(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let selected_text = state.selected_text.lock().await;
    Ok(selected_text.clone())
}

#[tauri::command]
pub async fn set_shortcut_recording(
    state: tauri::State<'_, AppState>,
    active: bool,
) -> Result<(), String> {
    let mut flag = state.recording_shortcut.lock().await;
    *flag = active;
    Ok(())
}

#[tauri::command]
pub async fn suspend_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        if let Err(e) = app.global_shortcut().unregister_all() {
            return Err(format!("无法暂停全局快捷键: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_global_shortcut(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        crate::reconfigure_global_shortcuts_async(&app).await;
    }
    Ok(())
}

/// 复制图片到剪贴板
/// image_data: base64 编码的图片数据（可以包含或不包含 data:image/xxx;base64, 前缀）
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
pub async fn copy_image_to_clipboard(image_data: String) -> Result<(), String> {
    // 移除 data URL 前缀（如果存在）
    let base64_data = if image_data.contains(",") {
        image_data.split(",").last().unwrap_or(&image_data)
    } else {
        &image_data
    };

    // 解码 base64
    let image_bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    // 使用 image crate 解码图片
    let img = image::load_from_memory(&image_bytes)
        .map_err(|e| format!("Failed to load image: {}", e))?;

    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    // 使用 arboard 复制到剪贴板
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;

    let img_data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(rgba.into_raw()),
    };

    clipboard
        .set_image(img_data)
        .map_err(|e| format!("Failed to copy image to clipboard: {}", e))?;

    Ok(())
}

/// 复制图片到剪贴板 - 移动端不支持
#[cfg(any(target_os = "android", target_os = "ios"))]
#[tauri::command]
pub async fn copy_image_to_clipboard(_image_data: String) -> Result<(), String> {
    Err("Clipboard image copy is not supported on mobile platforms".to_string())
}

/// 获取开机自启动状态
#[tauri::command]
pub async fn get_autostart_state(app: tauri::AppHandle) -> Result<bool, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let autostart_manager = app.autolaunch();
        let enabled = autostart_manager.is_enabled().map_err(|e| e.to_string())?;
        tracing::info!("get_autostart_state: enabled={}, bundle_id=com.aipp.app", enabled);
        Ok(enabled)
    }
    #[cfg(mobile)]
    {
        Ok(false)
    }
}

/// 设置开机自启动
#[tauri::command]
pub async fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let autostart_manager = app.autolaunch();
        tracing::info!(
            "set_autostart: enabled={}, bundle_id=com.aipp.app, action={}",
            enabled,
            if enabled { "enable" } else { "disable" }
        );
        if enabled {
            autostart_manager.enable().map_err(|e| {
                tracing::error!("set_autostart: enable failed, error={}", e);
                e.to_string()
            })?;
            tracing::info!("set_autostart: enable succeeded");
        } else {
            autostart_manager.disable().map_err(|e| {
                tracing::error!("set_autostart: disable failed, error={}", e);
                e.to_string()
            })?;
            tracing::info!("set_autostart: disable succeeded");
        }
        Ok(())
    }
    #[cfg(mobile)]
    {
        Err("Autostart is not supported on mobile platforms".to_string())
    }
}

/// 打开图片（支持 base64 和 URL）
/// 对于 base64 图片，会保存到临时文件后用系统默认应用打开
/// 对于 URL，直接用系统默认应用打开
/// conversation_id 和 message_id 用于生成固定的文件名，避免重复创建临时文件
#[tauri::command]
pub async fn open_image(
    image_data: String,
    conversation_id: Option<String>,
    message_id: Option<String>,
) -> Result<(), String> {
    // 如果是 base64 图片，保存到临时文件
    if image_data.starts_with("data:") {
        // 解析 MIME 类型
        let mime_type = image_data
            .strip_prefix("data:")
            .and_then(|s| s.split(';').next())
            .unwrap_or("image/png");

        // 确定文件扩展名
        let ext = match mime_type {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            "image/bmp" => "bmp",
            _ => "png",
        };

        // 移除 data URL 前缀
        let base64_data = image_data.split(',').last().ok_or("Invalid data URL format")?;

        // 解码 base64
        let image_bytes = base64::engine::general_purpose::STANDARD
            .decode(base64_data)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        // 创建临时文件，使用 conversationId 和 messageId 生成固定文件名
        let temp_dir = std::env::temp_dir();
        let filename = match (&conversation_id, &message_id) {
            (Some(conv_id), Some(msg_id)) if !conv_id.is_empty() && !msg_id.is_empty() => {
                format!("aipp_image_{}_{}.{}", conv_id, msg_id, ext)
            }
            _ => {
                // 如果没有 id，使用图片内容的哈希值作为文件名
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&image_bytes);
                let hash = hex::encode(&hasher.finalize()[..8]);
                format!("aipp_image_{}.{}", hash, ext)
            }
        };
        let temp_path = temp_dir.join(filename);

        // 写入文件
        std::fs::write(&temp_path, &image_bytes)
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        // 用系统默认应用打开
        open::that(&temp_path).map_err(|e| format!("Failed to open image: {}", e))?;
    } else {
        // 直接打开 URL
        open::that(&image_data).map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    Ok(())
}

use std::cmp::Ord;
use std::collections::HashMap;
use tauri::{Manager, State};

use crate::template_engine::{BangType, TemplateEngine};
use crate::AppState;
use crate::FeatureConfigState;

use crate::db::system_db::{FeatureConfig, SystemDatabase};
use reqwest::StatusCode;

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

    Ok(())
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

/// Save data storage configuration into feature_config under feature_code = "data_storage"
#[tauri::command]
pub async fn save_data_storage_config(
    app_handle: tauri::AppHandle,
    state: State<'_, FeatureConfigState>,
    storage_mode: String,                 // "local" | "remote"
    remote_type: Option<String>,          // Some("supabase"|"postgresql"|"mysql") when remote
    payload: std::collections::HashMap<String, String>,
)
-> Result<(), String> {
    let db = SystemDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Build a flattened map to store
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("storage_mode".to_string(), storage_mode.clone());
    if let Some(rt) = remote_type.clone() { config.insert("remote_type".to_string(), rt); }
    for (k,v) in payload.iter() { config.insert(k.clone(), v.clone()); }

    // Persist by replacing feature_code = data_storage
    let feature_code = "data_storage".to_string();
    let _ = db.delete_feature_config_by_feature_code(&feature_code);
    for (key, value) in config.iter() {
        db.add_feature_config(&FeatureConfig {
            id: None,
            feature_code: feature_code.clone(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        }).map_err(|e| e.to_string())?;
    }

    // Update in-memory state
    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;
    configs.retain(|c| c.feature_code != feature_code);
    config_feature_map.remove("data_storage");
    for (key, value) in config.iter() {
        let new_config = FeatureConfig {
            id: None,
            feature_code: "data_storage".to_string(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(new_config.clone());
        config_feature_map
            .entry("data_storage".to_string())
            .or_insert(Default::default())
            .insert(key.clone(), new_config);
    }

    Ok(())
}

/// Test connectivity to a remote storage based on type and payload
#[tauri::command]
pub async fn test_remote_storage_connection(
    remote_type: String,
    payload: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    match remote_type.as_str() {
        "supabase" => {
            let url = payload.get("supabase_url").cloned().ok_or("缺少 supabase_url")?;
            let key = payload.get("supabase_key").cloned().ok_or("缺少 supabase_key")?;
            // Try a lightweight request to the REST endpoint root; any non-5xx status counts as reachable
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .map_err(|e| e.to_string())?;
            let rest_url = format!("{}/rest/v1/", url.trim_end_matches('/'));
            let resp = client
                .get(&rest_url)
                .header("apikey", key.clone())
                .header("Authorization", format!("Bearer {}", key))
                .send()
                .await
                .map_err(|e| format!("请求失败: {}", e))?;
            if resp.status().is_server_error() {
                return Err(format!("Supabase 服务返回错误状态: {}", resp.status()));
            }
            Ok(())
        }
        "postgresql" => {
            let host = payload.get("pg_host").cloned().ok_or("缺少 pg_host")?;
            let port = payload.get("pg_port").cloned().unwrap_or_else(|| "5432".to_string());
            let db = payload.get("pg_database").cloned().ok_or("缺少 pg_database")?;
            let user = payload.get("pg_username").cloned().ok_or("缺少 pg_username")?;
            let pass = payload.get("pg_password").cloned().ok_or("缺少 pg_password")?;
            let url = format!("postgres://{}:{}@{}:{}/{}", urlencoding::encode(&user), urlencoding::encode(&pass), host, port, db);
            let conn = sea_orm::Database::connect(&url).await.map_err(|e| format!("连接失败: {}", e))?;
            // Simple ping by acquiring connection
            conn.ping().await.map_err(|e| format!("Ping 失败: {}", e))?;
            Ok(())
        }
        "mysql" => {
            let host = payload.get("mysql_host").cloned().ok_or("缺少 mysql_host")?;
            let port = payload.get("mysql_port").cloned().unwrap_or_else(|| "3306".to_string());
            let db = payload.get("mysql_database").cloned().ok_or("缺少 mysql_database")?;
            let user = payload.get("mysql_username").cloned().ok_or("缺少 mysql_username")?;
            let pass = payload.get("mysql_password").cloned().ok_or("缺少 mysql_password")?;
            let url = format!("mysql://{}:{}@{}:{}/{}", urlencoding::encode(&user), urlencoding::encode(&pass), host, port, db);
            let conn = sea_orm::Database::connect(&url).await.map_err(|e| format!("连接失败: {}", e))?;
            conn.ping().await.map_err(|e| format!("Ping 失败: {}", e))?;
            Ok(())
        }
        _ => Err("不支持的远程类型".to_string()),
    }
}

/// Placeholder: upload local data directory to remote. Actual migration logic will be added later.
#[tauri::command]
pub async fn upload_local_data(
    app: tauri::AppHandle,
    remote_type: String,
    _payload: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    // For now, just verify local data folder exists
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let db_path = app_dir.join("db");
    if !db_path.exists() {
        return Err("本地数据目录不存在".to_string());
    }
    // In the future: zip + stream upload or direct DB migration
    tracing::info!(?remote_type, path=?db_path, "Upload local data requested");
    Ok(())
}

#[tauri::command]
pub async fn get_bang_list() -> Result<Vec<(String, String, String, BangType)>, String> {
    let engine = TemplateEngine::new();
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
pub async fn set_shortcut_recording(state: tauri::State<'_, AppState>, active: bool) -> Result<(), String> {
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

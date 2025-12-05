use std::cmp::Ord;
use std::collections::HashMap;
use tauri::{Manager, State};
use base64::Engine;

use crate::template_engine::{BangType, TemplateEngine};
use crate::AppState;
use crate::FeatureConfigState;

use crate::db::system_db::{FeatureConfig, SystemDatabase};

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

/// 复制图片到剪贴板
/// image_data: base64 编码的图片数据（可以包含或不包含 data:image/xxx;base64, 前缀）
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
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("Failed to access clipboard: {}", e))?;
    
    let img_data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(rgba.into_raw()),
    };
    
    clipboard.set_image(img_data)
        .map_err(|e| format!("Failed to copy image to clipboard: {}", e))?;
    
    Ok(())
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
        let base64_data = image_data
            .split(',')
            .last()
            .ok_or("Invalid data URL format")?;
        
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
                use sha2::{Sha256, Digest};
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
        open::that(&temp_path)
            .map_err(|e| format!("Failed to open image: {}", e))?;
    } else {
        // 直接打开 URL
        open::that(&image_data)
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }
    
    Ok(())
}

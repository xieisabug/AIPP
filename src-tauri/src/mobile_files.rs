//! Android document grants stay in the native picker. No arbitrary URI/path API is exposed.
use serde_json::Value;
#[cfg(target_os = "android")]
use serde_json::json;
use tauri::{Manager, Runtime};

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("mobile-files")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            app.manage(api.register_android_plugin("com.xieisabug.aipp", "MobileFilesPlugin")?);
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

pub fn require_attachment_path(app: &tauri::AppHandle, path: &std::path::Path) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("files/mobile-files/imports");
    let root = root.canonicalize().map_err(|e| format!("MOBILE_FILE authorization: {e}"))?;
    let resolved = path.canonicalize().map_err(|e| format!("MOBILE_FILE authorization: {e}"))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err("MOBILE_FILE authorization: path is not an imported document".into());
    }
    if resolved.metadata().map_err(|e| e.to_string())?.len() > 20 * 1024 * 1024 {
        return Err("MOBILE_FILE size: single file exceeds 20 MiB".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn mobile_file_action(app: tauri::AppHandle, action: String, payload: Value) -> Result<Value, String> {
    if !matches!(action.as_str(), "pick" | "save" | "share" | "open" | "print" | "copyImage") {
        return Err(format!("MOBILE_FILE dispatch: unknown action {action}"));
    }
    #[cfg(target_os = "android")]
    {
        let handle = app.state::<tauri::plugin::PluginHandle<tauri::Wry>>().inner().clone();
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("files/mobile-files");
        let args = json!({"root": root, "payload": payload});
        tauri::async_runtime::spawn_blocking(move || handle.run_mobile_plugin::<Value>(&action, args)
            .map_err(|e| format!("MOBILE_FILE {action}: {e}")))
            .await.map_err(|e| format!("MOBILE_FILE worker: {e}"))?
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, payload);
        Err("MOBILE_UNSUPPORTED mobile_files: Android native service required".into())
    }
}

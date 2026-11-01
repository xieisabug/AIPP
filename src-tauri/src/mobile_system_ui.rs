//! Sync Android status/navigation bar icon appearance with the app theme (LeaderAI #151).
#[cfg(target_os = "android")]
use serde_json::{json, Value};
use tauri::Runtime;

#[cfg(target_os = "android")]
struct SystemUiPluginHandle(tauri::plugin::PluginHandle<tauri::Wry>);

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("mobile-system-ui")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = api.register_android_plugin("com.xieisabug.aipp", "SystemUiPlugin")?;
                app.manage(SystemUiPluginHandle(handle));
            }
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

/// `theme` is the resolved appearance: `"light"` or `"dark"`.
#[tauri::command]
pub async fn set_android_status_bar_appearance(
    app: tauri::AppHandle,
    theme: String,
) -> Result<(), String> {
    let normalized = if theme.eq_ignore_ascii_case("dark") {
        "dark"
    } else {
        "light"
    };

    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        let plugin = app
            .try_state::<SystemUiPluginHandle>()
            .ok_or_else(|| "MOBILE_SYSTEM_UI: plugin not registered".to_string())?
            .0
            .clone();
        let args = json!({ "theme": normalized });
        tauri::async_runtime::spawn_blocking(move || {
            plugin
                .run_mobile_plugin::<Value>("setStatusBarAppearance", args)
                .map_err(|e| format!("MOBILE_SYSTEM_UI setStatusBarAppearance: {e}"))
        })
        .await
        .map_err(|e| format!("MOBILE_SYSTEM_UI worker: {e}"))?
        .map(|_| ())
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, normalized);
        Ok(())
    }
}

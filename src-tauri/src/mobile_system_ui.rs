//! Sync Android status/navigation bar icon appearance with the app theme (LeaderAI #151).
#[cfg(not(target_os = "android"))]
use tauri::Runtime;

#[cfg(target_os = "android")]
struct SystemUiPluginHandle(tauri::plugin::PluginHandle<tauri::Wry>);

/// Desktop / non-Android: keep a generic no-op plugin so call sites stay uniform.
#[cfg(not(target_os = "android"))]
pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("mobile-system-ui")
        .setup(|app, api| {
            let _ = (app, api);
            Ok(())
        })
        .build()
}

/// Android: bind the plugin to `tauri::Wry` (the app Runtime on this target).
/// A generic `init<R>` previously forced `Builder` to `Wry` while the signature still
/// returned `TauriPlugin<R>`, which broke `cargo build --target *-linux-android`.
#[cfg(target_os = "android")]
pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("mobile-system-ui")
        .setup(|app, api| {
            use tauri::Manager;
            let handle = api.register_android_plugin("com.xieisabug.aipp", "SystemUiPlugin")?;
            app.manage(SystemUiPluginHandle(handle));
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
        use serde_json::{json, Value};
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

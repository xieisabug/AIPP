use tauri::AppHandle;
use serde::{Deserialize, Serialize};

/// 更新状态信息
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub body: Option<String>,
    pub date: Option<String>,
}

/// 检查更新
#[tauri::command]
pub async fn check_update(app_handle: AppHandle) -> Result<UpdateInfo, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;

        let updater = app_handle.updater()
            .map_err(|e| format!("获取 updater 失败: {}", e))?;

        match updater.check().await {
            Ok(Some(update)) => {
                Ok(UpdateInfo {
                    available: true,
                    current_version: env!("CARGO_PKG_VERSION").to_string(),
                    latest_version: Some(update.version.clone()),
                    body: update.body,
                    date: update.date.map(|d| d.to_string()),
                })
            }
            Ok(None) => {
                Ok(UpdateInfo {
                    available: false,
                    current_version: env!("CARGO_PKG_VERSION").to_string(),
                    latest_version: None,
                    body: None,
                    date: None,
                })
            }
            Err(e) => Err(format!("检查更新失败: {}", e))
        }
    }
    #[cfg(mobile)]
    {
        Err("自动更新不支持移动平台".to_string())
    }
}

/// 下载并安装更新
#[tauri::command]
pub async fn download_and_install_update(app_handle: AppHandle) -> Result<String, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;

        let updater = app_handle.updater()
            .map_err(|e| format!("获取 updater 失败: {}", e))?;

        // 检查更新
        let update = updater.check().await
            .map_err(|e| format!("检查更新失败: {}", e))?
            .ok_or("没有可用的更新")?;

        // 下载更新
        // download 方法需要两个回调：on_chunk 和 on_download_finish
        update.download(
            |chunk_length, content_length| {
                // on_chunk 回调 - 可以在这里更新下载进度
                tracing::info!("下载进度: {} / {}", chunk_length, content_length.unwrap_or(0));
            },
            || {
                // on_download_finish 回调
                tracing::info!("下载完成");
            }
        ).await
            .map_err(|e| format!("下载更新失败: {}", e))?;

        Ok("更新下载完成，即将安装...".to_string())
    }
    #[cfg(mobile)]
    {
        Err("自动更新不支持移动平台".to_string())
    }
}

/// 获取当前应用版本
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

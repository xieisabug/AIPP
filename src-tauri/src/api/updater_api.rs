use tauri::{AppHandle, State};
use serde::{Deserialize, Serialize};

use crate::FeatureConfigState;

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
    check_update_impl(app_handle, None).await
}

/// 使用代理检查更新
#[tauri::command]
pub async fn check_update_with_proxy(
    app_handle: AppHandle,
    state: State<'_, FeatureConfigState>,
) -> Result<UpdateInfo, String> {
    let proxy = get_network_proxy(&state).await?;
    check_update_impl(app_handle, Some(proxy)).await
}

async fn get_network_proxy(state: &State<'_, FeatureConfigState>) -> Result<String, String> {
    // 从配置中获取代理
    let config_feature_map = state.config_feature_map.lock().await;
    let proxy = if let Some(network_config) = config_feature_map.get("network_config") {
        if let Some(proxy_config) = network_config.get("network_proxy") {
            let proxy_url = proxy_config.value.trim();
            if !proxy_url.is_empty() {
                Some(proxy_url.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    drop(config_feature_map);

    proxy.ok_or_else(|| "未配置代理，请在网络配置中设置代理".to_string())
}

/// 检查更新的内部实现
async fn check_update_impl(app_handle: AppHandle, proxy: Option<String>) -> Result<UpdateInfo, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;

        let updater = app_handle.updater()
            .map_err(|e| format!("获取 updater 失败: {}", e))?;

        // 设置代理环境变量
        if let Some(ref proxy_url) = proxy {
            std::env::set_var("HTTP_PROXY", proxy_url);
            std::env::set_var("HTTPS_PROXY", proxy_url);
        }

        let result = match updater.check().await {
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
            Err(e) => {
                let error_msg = e.to_string();
                // 提供更友好的错误提示
                if error_msg.contains("Could not fetch") || error_msg.contains("valid release") {
                    Err("检查更新失败: GitHub Releases 中未找到 latest.json 文件。请确保已发布新版本并在 Assets 中包含 latest.json".to_string())
                } else if error_msg.contains("signature") {
                    Err(format!("检查更新失败: 签名验证错误 - {}", e))
                } else {
                    Err(format!("检查更新失败: {}", e))
                }
            }
        };

        // 清除代理环境变量
        if proxy.is_some() {
            std::env::remove_var("HTTP_PROXY");
            std::env::remove_var("HTTPS_PROXY");
        }

        result
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

        let mut downloaded: u64 = 0;
        update
            .download_and_install(
                |chunk_length, content_length| {
                    downloaded = downloaded.saturating_add(chunk_length as u64);
                    tracing::info!(
                        "下载进度: {} / {}",
                        downloaded,
                        content_length.unwrap_or(0)
                    );
                },
                || {
                    tracing::info!("下载完成，开始安装");
                },
            )
            .await
            .map_err(|e| format!("下载更新失败: {}", e))?;

        Ok("更新已开始安装".to_string())
    }
    #[cfg(mobile)]
    {
        Err("自动更新不支持移动平台".to_string())
    }
}

/// 使用代理下载并安装更新
#[tauri::command]
pub async fn download_and_install_update_with_proxy(
    app_handle: AppHandle,
    state: State<'_, FeatureConfigState>,
) -> Result<String, String> {
    let proxy = get_network_proxy(&state).await?;
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;

        let updater = app_handle
            .updater()
            .map_err(|e| format!("获取 updater 失败: {}", e))?;

        // 设置代理环境变量
        std::env::set_var("HTTP_PROXY", &proxy);
        std::env::set_var("HTTPS_PROXY", &proxy);

        let result = (async {
            let update = updater
                .check()
                .await
                .map_err(|e| format!("检查更新失败: {}", e))?
                .ok_or("没有可用的更新")?;

            let mut downloaded: u64 = 0;
            update
                .download_and_install(
                    |chunk_length, content_length| {
                        downloaded = downloaded.saturating_add(chunk_length as u64);
                        tracing::info!(
                            "下载进度: {} / {}",
                            downloaded,
                            content_length.unwrap_or(0)
                        );
                    },
                    || {
                        tracing::info!("下载完成，开始安装");
                    },
                )
                .await
                .map_err(|e| format!("下载更新失败: {}", e))?;

            Ok::<_, String>("更新已开始安装".to_string())
        })
        .await;

        // 清除代理环境变量
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("HTTPS_PROXY");

        result
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

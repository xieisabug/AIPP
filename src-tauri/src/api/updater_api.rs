#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(desktop)]
pub mod desktop {
    use serde::Serialize;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Emitter, Manager, State};
    use tauri_plugin_updater::{Update, UpdaterExt};

    use crate::FeatureConfigState;

    #[derive(Clone, Copy, Default, PartialEq, Serialize)]
    #[serde(rename_all = "snake_case")]
    enum Phase {
        #[default]
        Idle,
        Checking,
        UpToDate,
        Downloading,
        Ready,
        Installing,
        Installed,
        Failed,
    }

    #[derive(Clone, Default, Serialize)]
    pub struct UpdateStatus {
        revision: u64,
        phase: Phase,
        version: Option<String>,
        body: Option<String>,
        downloaded: u64,
        total: Option<u64>,
        error: Option<String>,
    }

    #[derive(Default)]
    struct Download {
        status: UpdateStatus,
        // Only Update::download's signature-verified bytes can enter this slot.
        package: Option<Arc<(Update, Vec<u8>)>>,
    }

    impl Download {
        fn publish(&mut self, app: &AppHandle) {
            self.status.revision += 1;
            if let Err(error) = app.emit("app-update-status", &self.status) {
                tracing::warn!(%error, "Failed to broadcast updater status");
            }
        }

        fn installation_started(&self) -> bool {
            matches!(self.status.phase, Phase::Installing | Phase::Installed)
        }
    }

    #[derive(Default)]
    pub struct AppUpdater {
        download: Mutex<Download>,
        check_lock: tokio::sync::Mutex<()>,
    }

    #[tauri::command]
    pub fn get_app_update_status(state: State<'_, AppUpdater>) -> UpdateStatus {
        state.download.lock().unwrap().status.clone()
    }

    #[tauri::command]
    pub async fn check_app_update(app: AppHandle, use_proxy: bool) -> Result<(), String> {
        check_and_download(&app, Some(use_proxy)).await
    }

    // None uses the configured proxy when available for the scheduled check.
    async fn find_update(app: &AppHandle, use_proxy: Option<bool>) -> Result<Option<Update>, String> {
        let mut builder = app.updater_builder().configure_client(|client| {
            client.connect_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(60))
        });
        if use_proxy != Some(false) {
            let config = app.state::<FeatureConfigState>();
            let map = config.config_feature_map.lock().await;
            let proxy = map.get("network_config").and_then(|items| items.get("network_proxy"))
                .map(|entry| entry.value.trim()).filter(|url| !url.is_empty());
            if let Some(url) = proxy {
                builder = builder.proxy(url.parse().map_err(|e| format!("更新代理地址无效：{e}"))?);
            } else if use_proxy == Some(true) {
                return Err("未配置代理，请在网络配置中设置代理".into());
            }
        }
        builder.build().map_err(|e| format!("初始化更新服务失败：{e}"))?
            .check().await.map_err(|e| format!("检查更新失败：{e}"))
    }

    async fn check_and_download(app: &AppHandle, use_proxy: Option<bool>) -> Result<(), String> {
        let state = app.state::<AppUpdater>();
        let Ok(_check) = state.check_lock.try_lock() else { return Ok(()) };
        {
            let mut download = state.download.lock().unwrap();
            if download.installation_started() { return Ok(()) }
            if download.package.is_none() {
                download.status.phase = Phase::Checking;
                download.status.error = None;
                download.publish(app);
            }
        }

        let result = find_update(app, use_proxy).await;
        let update = {
            let mut download = state.download.lock().unwrap();
            // A ready package can be installed even while the hourly check is in flight.
            if download.installation_started() { return Ok(()) }
            match result {
                Err(error) => {
                    if download.package.is_none() {
                        download.status.phase = Phase::Failed;
                    }
                    download.status.error = Some(error.clone());
                    download.publish(app);
                    return Err(error);
                }
                Ok(None) => {
                    download.package = None;
                    download.status = UpdateStatus {
                        revision: download.status.revision, phase: Phase::UpToDate,
                        ..Default::default()
                    };
                    download.publish(app);
                    return Ok(());
                }
                Ok(Some(update)) => {
                    if download.package.as_ref().is_some_and(|p|
                        p.0.version == update.version && p.0.signature == update.signature
                    ) {
                        if download.status.error.take().is_some() { download.publish(app); }
                        return Ok(());
                    }
                    download.package = None;
                    download.status = UpdateStatus {
                        revision: download.status.revision,
                        phase: Phase::Downloading,
                        version: Some(update.version.clone()),
                        body: update.body.clone(),
                        ..Default::default()
                    };
                    download.publish(app);
                    update
                }
            }
        };

        let mut last_event = Instant::now();
        let result = update.download(|length, total| {
            let mut download = state.download.lock().unwrap();
            let first_chunk = download.status.downloaded == 0;
            download.status.downloaded += length as u64;
            download.status.total = total;
            if first_chunk || last_event.elapsed() >= Duration::from_millis(200) {
                download.publish(app);
                last_event = Instant::now();
            }
        }, || {}).await;

        let mut download = state.download.lock().unwrap();
        match result {
            Ok(bytes) => {
                download.status.downloaded = bytes.len() as u64;
                download.package = Some(Arc::new((update, bytes)));
                download.status.phase = Phase::Ready;
                download.publish(app);
                Ok(())
            }
            Err(e) => {
                let error = format!("下载或验证更新包失败：{e}");
                download.status.error = Some(error.clone());
                download.status.phase = Phase::Failed;
                download.publish(app);
                Err(error)
            }
        }
    }

    #[tauri::command]
    pub async fn install_app_update(app: AppHandle, version: String) -> Result<(), String> {
        let state = app.state::<AppUpdater>();
        let package = {
            let mut download = state.download.lock().unwrap();
            if download.installation_started() && download.status.version.as_ref() == Some(&version) {
                return Ok(());
            }
            if download.status.phase != Phase::Ready || download.status.version.as_ref() != Some(&version) {
                return Err(format!("版本 {version} 尚未就绪或更新状态已改变，请查看当前进度"));
            }
            let package = download.package.clone().ok_or("缺少已校验的更新包")?;
            download.status.phase = Phase::Installing;
            download.status.error = None;
            download.publish(&app);
            package
        };
        let result = tokio::task::spawn_blocking(move || package.0.install(&package.1))
            .await.map_err(|e| format!("安装任务失败：{e}"))
            .and_then(|result| result.map_err(|e| format!("安装更新失败：{e}")));
        let mut download = state.download.lock().unwrap();
        if let Err(error) = &result {
            download.status.phase = Phase::Ready;
            download.status.error = Some(error.clone());
        } else {
            download.status.phase = Phase::Installed;
            download.package = None;
        }
        download.publish(&app);
        result
    }

    #[cfg(target_os = "macos")]
    #[tauri::command]
    pub fn restart_after_app_update(app: AppHandle, state: State<'_, AppUpdater>) -> Result<(), String> {
        if state.download.lock().unwrap().status.phase != Phase::Installed {
            return Err("更新尚未安装完成".into());
        }
        crate::request_app_exit(&app, crate::EXIT_STATE_RESTART_REQUESTED);
        Ok(())
    }

    pub fn start(app: &AppHandle) {
        app.manage(AppUpdater::default());
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(3600));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                if let Err(error) = check_and_download(&app, None).await {
                    tracing::warn!(%error, "Background update check or download failed");
                }
            }
        });
    }
}

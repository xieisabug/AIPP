use std::sync::Arc;

use futures::FutureExt;
use openlark_client::ws_client::{EventDispatcherHandler, LarkWsClient};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, warn};

use super::types::*;
use super::config::load_runtime_config;
use super::events::handle_payload;

pub(super) fn build_status(config: &FeishuRuntimeConfig) -> FeishuRuntimeStatus {
    FeishuRuntimeStatus {
        butler_enabled: config.butler_enabled,
        enabled: config.enabled,
        configured: !config.app_id.trim().is_empty() && !config.app_secret.trim().is_empty(),
        secret_configured: !config.app_secret.trim().is_empty(),
        running: false,
        connected: false,
        app_id: (!config.app_id.trim().is_empty()).then(|| config.app_id.clone()),
        base_url: Some(config.base_url.clone()),
        allow_p2p: config.allow_p2p,
        allow_group: config.allow_group,
        group_require_mention: config.group_require_mention,
        last_error: None,
        last_event_at: None,
        last_status_at: Some(now_string()),
        status_detail: None,
        status_text: "飞书机器人未启动".to_string(),
    }
}

pub(super) async fn replace_status(app_handle: &AppHandle, status: FeishuRuntimeStatus) {
    let state = app_handle.state::<FeishuButlerState>();
    let mut snapshot = status;
    snapshot.last_status_at = Some(now_string());
    *state.status.lock().await = snapshot.clone();
    let _ = app_handle.emit("butler_feishu_status_changed", snapshot);
}

pub(super) async fn mutate_status<F>(app_handle: &AppHandle, apply: F)
where
    F: FnOnce(&mut FeishuRuntimeStatus),
{
    let state = app_handle.state::<FeishuButlerState>();
    let mut status = state.status.lock().await;
    apply(&mut status);
    status.last_status_at = Some(now_string());
    let snapshot = status.clone();
    drop(status);
    let _ = app_handle.emit("butler_feishu_status_changed", snapshot);
}

pub(super) async fn set_feishu_runtime_ready_status(app_handle: &AppHandle, detail: impl Into<String>) {
    let detail = detail.into();
    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "飞书机器人已连接，等待消息".to_string();
        status.status_detail = Some(detail);
    })
    .await;
}

pub(super) fn format_panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

pub(crate) async fn get_runtime_status(
    app_handle: &AppHandle,
) -> Result<FeishuRuntimeStatus, String> {
    let config = load_runtime_config(app_handle).await?;
    let state = app_handle.state::<FeishuButlerState>();
    let mut status = state.status.lock().await.clone();
    status.butler_enabled = config.butler_enabled;
    status.enabled = config.enabled;
    status.configured = !config.app_id.trim().is_empty() && !config.app_secret.trim().is_empty();
    status.secret_configured = !config.app_secret.trim().is_empty();
    status.app_id = (!config.app_id.trim().is_empty()).then(|| config.app_id);
    status.base_url = Some(config.base_url);
    status.allow_p2p = config.allow_p2p;
    status.allow_group = config.allow_group;
    status.group_require_mention = config.group_require_mention;
    Ok(status)
}

pub(crate) fn refresh_runtime_async(app_handle: &AppHandle) {
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = refresh_runtime(&app_handle).await {
            warn!(error = %error, "failed to refresh feishu runtime");
        }
    });
}

pub(crate) async fn refresh_runtime(app_handle: &AppHandle) -> Result<FeishuRuntimeStatus, String> {
    crate::ensure_rustls_crypto_provider();
    let config = load_runtime_config(app_handle).await?;
    let state = app_handle.state::<FeishuButlerState>();

    let previous_handle = {
        let mut runtime_task = state.runtime_task.lock().await;
        runtime_task.take()
    };
    if let Some(handle) = previous_handle {
        handle.abort();
    }

    let mut status = build_status(&config);
    if !config.butler_enabled {
        status.status_text = "总管家模式未启用，飞书机器人不会启动".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }
    if !config.enabled {
        status.status_text = "飞书机器人未启用".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        status.status_text = "请先配置飞书 App ID 和 App Secret".to_string();
        status.status_detail = Some("缺少飞书应用凭据，无法启动连接".to_string());
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }

    status.running = true;
    status.status_text = "正在连接飞书长连接".to_string();
    status.status_detail = Some("已创建后台任务，准备初始化飞书 WebSocket 客户端".to_string());
    replace_status(app_handle, status.clone()).await;

    let app_handle_clone = app_handle.clone();
    let config_clone = config.clone();
    let task = tauri::async_runtime::spawn(async move {
        let panic_guard =
            std::panic::AssertUnwindSafe(run_runtime_loop(app_handle_clone.clone(), config_clone))
                .catch_unwind()
                .await;
        if let Err(payload) = panic_guard {
            let panic_message = format_panic_payload(payload);
            mutate_status(&app_handle_clone, |status| {
                status.running = false;
                status.connected = false;
                status.last_error = Some(format!("飞书运行时 panic: {}", panic_message));
                status.status_text = "飞书运行时异常退出".to_string();
                status.status_detail =
                    Some("后台运行任务发生未捕获异常，请检查配置和连接环境".to_string());
            })
            .await;
            warn!(error = %panic_message, "feishu runtime loop panicked");
        }
    });
    {
        let mut runtime_task = state.runtime_task.lock().await;
        *runtime_task = Some(task);
    }
    Ok(status)
}

async fn run_runtime_loop(app_handle: AppHandle, config: FeishuRuntimeConfig) {
    loop {
        mutate_status(&app_handle, |status| {
            status.running = true;
            status.connected = false;
            status.status_text = "正在连接飞书长连接".to_string();
            status.status_detail = Some("正在构建飞书连接配置".to_string());
        })
        .await;

        let ws_config = match openlark_client::Config::builder()
            .app_id(config.app_id.clone())
            .app_secret(config.app_secret.clone())
            .base_url(config.base_url.clone())
            .timeout(FEISHU_HTTP_TIMEOUT)
            .build()
        {
            Ok(config_value) => config_value,
            Err(error) => {
                mutate_status(&app_handle, |status| {
                    status.running = false;
                    status.connected = false;
                    status.last_error = Some(error.to_string());
                    status.status_text = "飞书配置无效".to_string();
                    status.status_detail =
                        Some("连接配置构建失败，请检查 App ID、Secret 和域名".to_string());
                })
                .await;
                return;
            }
        };

        let (payload_tx, payload_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let event_handler = EventDispatcherHandler::builder().payload_sender(payload_tx).build();

        mutate_status(&app_handle, |status| {
            status.running = true;
            status.connected = false;
            status.status_text = "正在连接飞书长连接".to_string();
            status.status_detail = Some("连接配置已生成，正在启动 WebSocket 客户端".to_string());
        })
        .await;

        let processor_app = app_handle.clone();
        let processor_task = tauri::async_runtime::spawn(async move {
            process_payload_loop(processor_app, payload_rx).await;
        });

        set_feishu_runtime_ready_status(
            &app_handle,
            format!("WebSocket 已建立，正在等待飞书事件；{FEISHU_STATUS_READY_DETAIL}"),
        )
        .await;

        let result = LarkWsClient::open(Arc::new(ws_config), event_handler).await;
        processor_task.abort();

        match result {
            Ok(_) => {
                mutate_status(&app_handle, |status| {
                    status.connected = false;
                    status.status_text = "飞书长连接已断开，准备重连".to_string();
                    status.status_detail = Some(
                        "长连接会话正常退出，飞书 SDK 会先内部重连，AIPP 也会在 5 秒后兜底重试"
                            .to_string(),
                    );
                })
                .await;
            }
            Err(error) => {
                mutate_status(&app_handle, |status| {
                    status.connected = false;
                    status.last_error = Some(error.to_string());
                    status.status_text = "飞书连接失败，准备重连".to_string();
                    status.status_detail = Some(
                        "握手或长连接建立失败，飞书 SDK 会先内部重连，AIPP 也会在 5 秒后兜底重试"
                            .to_string(),
                    );
                })
                .await;
            }
        }

        sleep(FEISHU_RUNTIME_RETRY_INTERVAL).await;
    }
}

async fn process_payload_loop(
    app_handle: AppHandle,
    mut payload_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(payload) = payload_rx.recv().await {
        mutate_status(&app_handle, |status| {
            status.last_event_at = Some(now_string());
        })
        .await;

        let config = match load_runtime_config(&app_handle).await {
            Ok(config) => config,
            Err(error) => {
                warn!(error = %error, "failed to reload Feishu runtime config before handling payload");
                mutate_status(&app_handle, |status| {
                    status.last_error = Some(error);
                    status.status_text = "飞书事件到达，但配置读取失败".to_string();
                })
                .await;
                continue;
            }
        };

        if !config.butler_enabled || !config.enabled {
            debug!("ignore Feishu payload because runtime is disabled by latest config");
            continue;
        }

        if let Err(error) = handle_payload(&app_handle, &config, &payload).await {
            warn!(error = %error, "failed to handle feishu payload");
        }
    }
}

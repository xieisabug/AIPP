use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, EventId, Listener, Manager};
use tokio::sync::Mutex;

use crate::artifacts::code_utils::{
    extract_component_name, extract_vue_component_name, is_react_component, is_vue_component,
};
use crate::artifacts::react_preview::{create_react_preview, create_react_preview_for_artifact};
use crate::artifacts::vue_preview::create_vue_preview_for_artifact;
use crate::artifacts::{applescript::run_applescript, powershell::run_powershell};
use crate::errors::AppError;
use crate::utils::bun_utils::BunUtils;

// 缓存最后一次预览的 artifact 信息，用于刷新恢复
struct LastArtifactCache {
    lang: String,
    input_str: String,
}

// 使用 LazyLock 延迟初始化全局状态
use std::sync::LazyLock;
static LAST_ARTIFACT_CACHE: LazyLock<Arc<Mutex<Option<LastArtifactCache>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Wait for the ArtifactPreview window to register its event listeners before sending data.
///
/// The frontend now continuously sends ready signals every 200ms until it receives data,
/// so we just need to wait for any ready signal to arrive.
///
/// Improved mechanism:
/// 1. Extended timeout from 2s to 10s for slow machines or first-time loads
/// 2. The frontend sends ready signals repeatedly, so we're more likely to catch one
/// 3. Returns true if ready signal received, false on timeout
async fn wait_for_artifact_preview_ready(app_handle: &tauri::AppHandle) -> bool {
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<()>();
    let sender = Arc::new(Mutex::new(Some(tx)));
    let app_handle_clone = app_handle.clone();

    let listener_id: EventId = app_handle.listen("artifact-preview-ready", move |_event| {
        let sender_clone = sender.clone();
        // Use spawn_blocking to handle the async lock in sync context
        tokio::spawn(async move {
            if let Some(tx) = sender_clone.lock().await.take() {
                let _ = tx.send(());
            }
        });
    });

    // Extended timeout to 10 seconds for reliability
    // The frontend sends ready signals every 200ms, so we should receive one quickly
    let ready = tokio::time::timeout(Duration::from_secs(10), rx).await.is_ok();

    app_handle_clone.unlisten(listener_id);

    if ready {
        tracing::debug!("artifact-preview-ready signal received");
    } else {
        tracing::warn!("Timeout waiting for artifact-preview-ready signal (10s)");
    }

    ready
}

/// Wait for the Artifact window (not preview) to register its event listeners.
/// Uses the same mechanism as wait_for_artifact_preview_ready but listens for "artifact-ready".
pub async fn wait_for_artifact_ready(app_handle: &tauri::AppHandle) -> bool {
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<()>();
    let sender = Arc::new(Mutex::new(Some(tx)));
    let app_handle_clone = app_handle.clone();

    let listener_id: EventId = app_handle.listen("artifact-ready", move |_event| {
        let sender_clone = sender.clone();
        tokio::spawn(async move {
            if let Some(tx) = sender_clone.lock().await.take() {
                let _ = tx.send(());
            }
        });
    });

    // Extended timeout to 10 seconds for reliability
    let ready = tokio::time::timeout(Duration::from_secs(10), rx).await.is_ok();

    app_handle_clone.unlisten(listener_id);

    if ready {
        tracing::debug!("artifact-ready signal received");
    } else {
        tracing::warn!("Timeout waiting for artifact-ready signal (10s)");
    }

    ready
}

#[tauri::command]
pub async fn run_artifacts(
    app_handle: tauri::AppHandle,
    lang: &str,
    input_str: &str,
) -> Result<String, AppError> {
    let _ = crate::window::open_artifact_preview_window(app_handle.clone()).await;

    // Ensure the preview window is visible and focused so it can attach listeners quickly
    #[cfg(desktop)]
    if let Some(window) = app_handle.get_webview_window("artifact_preview") {
        let _ = window.show();
        let _ = window.set_focus();
    }

    // 发送 reset 事件，通知前端清除旧状态（处理切换 artifact 时的状态清理）
    if let Some(window) = app_handle.get_webview_window("artifact_preview") {
        let _ = window.emit("artifact-preview-reset", ());
        tracing::debug!("artifact-preview-reset event emitted");
    }

    // Avoid first-open race by waiting for the front-end ready event
    let _ = wait_for_artifact_preview_ready(&app_handle).await;

    // 缓存当前 artifact 信息，用于刷新恢复
    {
        let mut cache = LAST_ARTIFACT_CACHE.lock().await;
        *cache = Some(LastArtifactCache {
            lang: lang.to_string(),
            input_str: input_str.to_string(),
        });
        tracing::debug!("Cached artifact: lang={}, input_len={}", lang, input_str.len());
    }

    match lang {
        "powershell" => {
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit("artifact-preview-log", "执行 PowerShell 脚本...");
            }
            return Ok(run_powershell(input_str).map_err(|e| {
                let error_msg = "PowerShell 脚本执行失败:".to_owned() + &e.to_string();
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit("artifact-preview-error", &error_msg);
                }
                AppError::RunCodeError(error_msg)
            })?);
        }
        "applescript" => {
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit("artifact-preview-log", "执行 AppleScript 脚本...");
            }
            return Ok(run_applescript(input_str).map_err(|e| {
                let error_msg = "AppleScript 脚本执行失败:".to_owned() + &e.to_string();
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit("artifact-preview-error", &error_msg);
                }
                AppError::RunCodeError(error_msg)
            })?);
        }
        "mermaid" => {
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit("artifact-preview-log", "准备预览 Mermaid 图表...");
            }
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit(
                    "artifact-preview-data",
                    serde_json::json!({ "type": "mermaid", "original_code": input_str }),
                );
                let _ =
                    window.emit("artifact-preview-log", format!("mermaid content: {}", input_str));
                let _ = window.emit("artifact-preview-success", "Mermaid 图表预览已准备完成");
            }
        }
        "xml" | "svg" | "html" | "markdown" | "md" => {
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit("artifact-preview-log", format!("准备预览 {} 内容...", lang));
                let _ = window.emit(
                    "artifact-preview-data",
                    serde_json::json!({ "type": lang, "original_code": input_str }),
                );
                let _ =
                    window.emit("artifact-preview-log", format!("{} content: {}", lang, input_str));
                let _ = window.emit(
                    "artifact-preview-success",
                    format!("{} 预览已准备完成", lang.to_uppercase()),
                );
            }
        }
        // 支持 "drawio" 和 "drawio:xml" 两种格式
        lang if lang == "drawio" || lang.starts_with("drawio:") => {
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit("artifact-preview-log", "准备预览 Draw.io 图表...");
                let _ = window.emit(
                    "artifact-preview-data",
                    serde_json::json!({ "type": "drawio", "original_code": input_str }),
                );
                let _ = window.emit("artifact-preview-success", "Draw.io 图表预览已准备完成");
            }
        }
        "react" | "jsx" => {
            let bun_version = BunUtils::get_bun_version(&app_handle);
            if bun_version.is_err()
                || bun_version.as_ref().unwrap_or(&String::new()).contains("Not Installed")
            {
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit("environment-check", serde_json::json!({
                        "tool": "bun",
                        "message": "React 预览需要 bun 环境，但系统中未安装 bun。是否要自动安装？",
                        "lang": lang,
                        "input_str": input_str
                    }));
                }
                return Ok("等待用户确认安装环境".to_string());
            }

            if is_react_component(input_str) {
                let component_name = extract_component_name(input_str)
                    .unwrap_or_else(|| "UserComponent".to_string());
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit(
                        "artifact-preview-data",
                        serde_json::json!({ "type": "react", "original_code": input_str }),
                    );
                }
                let preview_id = create_react_preview_for_artifact(
                    app_handle.clone(),
                    input_str.to_string(),
                    component_name,
                )
                .await
                .map_err(|e| {
                    let error_msg = format!("React 组件预览失败: {}", e);
                    if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                        let _ = window.emit("artifact-preview-error", &error_msg);
                    }
                    AppError::RunCodeError(error_msg)
                })?;
                return Ok(format!("React 组件预览已启动，预览 ID: {}", preview_id));
            } else {
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit(
                        "artifact-preview-error",
                        "React 代码片段预览暂不支持，请提供完整的 React 组件代码。",
                    );
                }
            }
        }
        "vue" => {
            let bun_version = BunUtils::get_bun_version(&app_handle);
            if bun_version.is_err()
                || bun_version.as_ref().unwrap_or(&String::new()).contains("Not Installed")
            {
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit("environment-check", serde_json::json!({
                        "tool": "bun",
                        "message": "Vue 预览需要 bun 环境，但系统中未安装 bun。是否要自动安装？",
                        "lang": lang,
                        "input_str": input_str
                    }));
                }
                return Ok("等待用户确认安装环境".to_string());
            }

            if is_vue_component(input_str) {
                let component_name = extract_vue_component_name(input_str)
                    .unwrap_or_else(|| "UserComponent".to_string());
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit(
                        "artifact-preview-data",
                        serde_json::json!({ "type": "vue", "original_code": input_str }),
                    );
                }
                let preview_id = create_vue_preview_for_artifact(
                    app_handle.clone(),
                    input_str.to_string(),
                    component_name,
                )
                .await
                .map_err(|e| {
                    let error_msg = format!("Vue 组件预览失败: {}", e);
                    if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                        let _ = window.emit("artifact-preview-error", &error_msg);
                    }
                    AppError::RunCodeError(error_msg)
                })?;
                return Ok(format!("Vue 组件预览已启动，预览 ID: {}", preview_id));
            } else {
                if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                    let _ = window.emit(
                        "artifact-preview-error",
                        "Vue 代码片段预览暂不支持，请提供完整的 Vue 组件代码。",
                    );
                }
            }
        }
        _ => {
            let error_msg = "暂不支持该语言的代码执行".to_owned();
            if let Some(window) = app_handle.get_webview_window("artifact_preview") {
                let _ = window.emit("artifact-preview-error", &error_msg);
            }
            return Err(AppError::RunCodeError(error_msg));
        }
    }
    Ok(String::new())
}

#[tauri::command]
pub async fn preview_react_component(
    app_handle: tauri::AppHandle,
    component_code: String,
    component_name: Option<String>,
) -> Result<String, String> {
    let name = component_name.unwrap_or_else(|| {
        extract_component_name(&component_code).unwrap_or_else(|| "UserComponent".to_string())
    });
    create_react_preview(app_handle, component_code, name).await
}

#[tauri::command]
pub async fn confirm_environment_install(
    app_handle: tauri::AppHandle,
    tool: String,
    confirmed: bool,
    lang: String,
    input_str: String,
) -> Result<String, String> {
    if !confirmed {
        if let Some(window) = app_handle.get_webview_window("artifact_preview") {
            let _ = window.emit("artifact-preview-error", "用户取消了环境安装，预览已停止");
        }
        return Ok("用户取消安装".to_string());
    }

    if let Some(window) = app_handle.get_webview_window("artifact_preview") {
        let _ = window.emit("artifact-preview-log", format!("开始安装{} 环境...", tool));
        if tool == "bun" {
            let _ = crate::artifacts::env_installer::install_bun(
                app_handle.clone(),
                Some("artifact_preview".to_string()),
            );
        } else if tool == "uv" {
            let _ = crate::artifacts::env_installer::install_uv(
                app_handle.clone(),
                Some("artifact_preview".to_string()),
            );
        }
        if let Some(window) = app_handle.get_webview_window("artifact_preview") {
            let _ = window.emit(
                "environment-install-started",
                serde_json::json!({ "tool": tool, "lang": lang, "input_str": input_str }),
            );
        }
    }
    Ok("开始安装环境".to_string())
}

#[tauri::command]
pub async fn retry_preview_after_install(
    app_handle: tauri::AppHandle,
    lang: String,
    input_str: String,
) -> Result<String, String> {
    match run_artifacts(app_handle.clone(), &lang, &input_str).await {
        Ok(result) => Ok(result),
        Err(e) => Err(e.to_string()),
    }
}

/// 恢复上一次的 artifact 预览（用于窗口刷新后恢复）
///
/// 从后端缓存中读取最后一次预览的 artifact 信息，重新执行预览流程
#[tauri::command]
pub async fn restore_artifact_preview(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let cache = LAST_ARTIFACT_CACHE.lock().await;

    if let Some(artifact) = cache.as_ref() {
        let lang = artifact.lang.clone();
        let input_str = artifact.input_str.clone();

        // 释放锁，因为 run_artifacts 可能需要时间
        drop(cache);

        tracing::info!("Restoring artifact preview: lang={}, input_len={}", lang, input_str.len());

        // 重新处理 artifact
        match run_artifacts(app_handle, &lang, &input_str).await {
            Ok(_) => Ok(Some(format!("Restored {} preview", lang))),
            Err(e) => Err(e.to_string()),
        }
    } else {
        tracing::debug!("No cached artifact to restore");
        Ok(None) // 没有缓存的 artifact
    }
}

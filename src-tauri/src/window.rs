use crate::artifacts::artifacts_db::ArtifactCollection;
use serde::{Deserialize, Serialize};
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri::{LogicalPosition, LogicalSize};
use tauri_plugin_notification::NotificationExt;
use tracing::{debug, error, info, warn};

/// 当按照显示器大小调整窗口尺寸时保留的屏幕占比（90%）
#[cfg(desktop)]
const SCREEN_MARGIN_RATIO: f64 = 0.9;

// 获取合适的窗口大小和位置
#[cfg(desktop)]
fn get_window_size_and_position(
    app: &AppHandle,
    default_width: f64,
    default_height: f64,
    reference_window_labels: &[&str],
) -> (LogicalSize<f64>, Option<LogicalPosition<f64>>) {
    // 预设窗口尺寸
    let mut window_size = LogicalSize::new(default_width, default_height);

    // 优先寻找参考窗口所在的显示器
    let mut target_monitor = None;

    // 为提升效率，提前获取一次显示器列表（若失败留空）
    let monitors_cache = app.available_monitors().unwrap_or_default();

    // 先收集窗口并按“可见优先”排序
    let mut visible_windows = Vec::new();
    let mut hidden_windows = Vec::new();

    for label in reference_window_labels {
        if let Some(w) = app.get_webview_window(label) {
            if w.is_visible().unwrap_or(false) {
                visible_windows.push(w);
            } else {
                hidden_windows.push(w);
            }
        }
    }

    // 可见窗口 → 隐藏窗口 两轮查找
    let search_lists = [visible_windows, hidden_windows];

    'search: for list in &search_lists {
        for w in list {
            // 1. 尝试使用 current_monitor()
            if let Ok(Some(m)) = w.current_monitor() {
                target_monitor = Some(m.clone());
                break 'search;
            }

            // 2. 如果失败，再根据窗口坐标匹配显示器
            if let Ok(pos) = w.outer_position() {
                for m in &monitors_cache {
                    let mp = m.position();
                    let ms = m.size();
                    // 判断窗口左上角是否位于该显示器范围内
                    if pos.x >= mp.x
                        && pos.x < mp.x + ms.width as i32
                        && pos.y >= mp.y
                        && pos.y < mp.y + ms.height as i32
                    {
                        target_monitor = Some(m.clone());
                        break 'search;
                    }
                }
            }
        }
    }

    // 如果仍未找到，则采用 primary_monitor() 兜底
    if target_monitor.is_none() {
        if let Ok(Some(m)) = app.primary_monitor() {
            target_monitor = Some(m.clone());
        }
    }

    // 计算合适的窗口位置
    if let Some(monitor) = target_monitor {
        // 将物理尺寸转换为逻辑尺寸，避免 HiDPI 误差
        let scale = monitor.scale_factor() as f64;

        let screen_width = monitor.size().width as f64 / scale;
        let screen_height = monitor.size().height as f64 / scale;

        // 留出边距
        let max_width = screen_width * SCREEN_MARGIN_RATIO;
        let max_height = screen_height * SCREEN_MARGIN_RATIO;

        window_size.width = window_size.width.min(max_width);
        window_size.height = window_size.height.min(max_height);

        // 居中到目标显示器（逻辑坐标）
        let monitor_pos_x = monitor.position().x as f64 / scale;
        let monitor_pos_y = monitor.position().y as f64 / scale;

        // 取整避免亚像素导致系统自动纠偏
        let center_x = (monitor_pos_x + (screen_width - window_size.width) / 2.0).round();
        let center_y = (monitor_pos_y + (screen_height - window_size.height) / 2.0).round();

        return (window_size, Some(LogicalPosition::new(center_x, center_y)));
    }

    // 若所有方案均失败，交给窗口构建器自行居中
    (window_size, None)
}

pub fn create_ask_window(app: &AppHandle) {
    create_ask_window_with_visibility(app, true);
}

pub fn create_ask_window_hidden(app: &AppHandle) {
    create_ask_window_with_visibility(app, false);
}

fn create_ask_window_with_visibility(app: &AppHandle, visible: bool) {
    let t_build_total = std::time::Instant::now();

    #[cfg(desktop)]
    let window_builder =
        WebviewWindowBuilder::new(app, "ask", WebviewUrl::App("index.html".into()))
            .title("Aipp")
            .inner_size(800.0, 450.0)
            .fullscreen(false)
            .resizable(false)
            .decorations(false)
            .visible(visible)
            .center();

    #[cfg(desktop)]
    #[cfg(not(target_os = "macos"))]
    let window_builder = window_builder.transparent(true);

    #[cfg(mobile)]
    let window_builder =
        WebviewWindowBuilder::new(app, "ask", WebviewUrl::App("index.html".into()));

    match window_builder.build() {
        Ok(window) => {
            let dt = t_build_total.elapsed().as_millis();
            info!(elapsed_ms=%dt, visible=%visible, "Ask window built");
            #[cfg(desktop)]
            {
                let window_clone = window.clone();
                let app_handle = app.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        window_clone.hide().unwrap();
                        // 发送窗口隐藏事件，让前端重置状态
                        let _ = app_handle.emit_to("ask", "ask-window-hidden", ());
                    }
                });
            }
        }
        Err(e) => error!(error=%e, "Failed to build window"),
    }
}

pub fn create_config_window(app: &AppHandle) {
    create_config_window_with_visibility(app, true);
}

pub fn create_config_window_hidden(app: &AppHandle) {
    create_config_window_with_visibility(app, false);
}

fn create_config_window_with_visibility(app: &AppHandle, visible: bool) {
    #[cfg(desktop)]
    {
        let (window_size, window_position) =
            get_window_size_and_position(app, 1300.0, 1000.0, &["ask", "chat_ui"]);

        let mut window_builder =
            WebviewWindowBuilder::new(app, "config", WebviewUrl::App("index.html".into()))
                .title("Aipp")
                .inner_size(window_size.width, window_size.height)
                .fullscreen(false)
                .resizable(true)
                .visible(visible)
                .decorations(true);

        // macOS 若仍有偏差可考虑额外使用 parent(&window) 方案

        if let Some(position) = window_position {
            window_builder = window_builder.position(position.x, position.y);
        } else {
            window_builder = window_builder.center();
        }

        #[cfg(not(target_os = "macos"))]
        let window_builder = window_builder.transparent(false);

        match window_builder.build() {
            Ok(window) => {
                info!(visible=%visible, "Config window built");
                let window_clone = window.clone();
                let app_handle = app.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        window_clone.hide().unwrap();
                        // 发送窗口隐藏事件，让前端重置状态
                        let _ = app_handle.emit_to("config", "config-window-hidden", ());
                    }
                });
            }
            Err(e) => error!(error=%e, "Failed to build window"),
        }
    }
    #[cfg(mobile)]
    {
        let window_builder =
            WebviewWindowBuilder::new(app, "config", WebviewUrl::App("index.html".into()));
        if let Err(e) = window_builder.build() {
            error!(error=%e, "Failed to build window");
        }
    }
}

pub fn create_chat_ui_window(app: &AppHandle) {
    create_chat_ui_window_with_visibility(app, true);
}

pub fn create_chat_ui_window_hidden(app: &AppHandle) {
    create_chat_ui_window_with_visibility(app, false);
}

fn create_chat_ui_window_with_visibility(app: &AppHandle, visible: bool) {
    #[cfg(desktop)]
    {
        let (window_size, window_position) =
            get_window_size_and_position(app, 1000.0, 800.0, &["ask"]);

        let mut window_builder =
            WebviewWindowBuilder::new(app, "chat_ui", WebviewUrl::App("index.html".into()))
                .title("Aipp")
                .inner_size(window_size.width, window_size.height)
                .fullscreen(false)
                .resizable(true)
                .visible(visible)
                .decorations(true)
                .on_download(|webview, event| {
                    let download_path =
                        webview.app_handle().path().download_dir().unwrap_or_default();

                    match event {
                        DownloadEvent::Requested { url, destination } => {
                            debug!(
                                "downloading {} to {}",
                                url,
                                download_path.clone().to_string_lossy()
                            );
                            *destination = download_path.join(&mut *destination);
                        }
                        DownloadEvent::Finished { url, path, success } => {
                            debug!("downloaded {} to {:?}, success: {}", url, path, success);
                            if success {
                                let title = "下载完成";
                                let body = format!("文件已保存到：{:?}", download_path);
                                if let Err(e) = webview
                                    .app_handle()
                                    .notification()
                                    .builder()
                                    .title(title)
                                    .body(&body)
                                    .show()
                                {
                                    warn!(error = %e, "failed to show download notification");
                                }
                            }
                        }
                        _ => {}
                    }
                    true
                })
                .disable_drag_drop_handler();

        // macOS 若仍有偏差可考虑额外使用 parent(&window) 方案

        if let Some(position) = window_position {
            window_builder = window_builder.position(position.x, position.y);
        } else {
            window_builder = window_builder.center();
        }

        #[cfg(not(target_os = "macos"))]
        let window_builder = window_builder.transparent(false);

        match window_builder.build() {
            Ok(window) => {
                info!(visible=%visible, "Chat UI window built");
                let window_clone = window.clone();
                let app_handle = app.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        window_clone.hide().unwrap();
                        // 发送窗口隐藏事件，让前端重置状态
                        let _ = app_handle.emit_to("chat_ui", "chat-ui-window-hidden", ());
                    }
                });
                // 只有在可见时才最大化
                if visible {
                    let _ = window.maximize();
                }
            }
            Err(e) => error!(error=%e, "Failed to build window"),
        }
    }
    #[cfg(mobile)]
    {
        let window_builder =
            WebviewWindowBuilder::new(app, "chat_ui", WebviewUrl::App("index.html".into()));
        if let Err(e) = window_builder.build() {
            error!(error=%e, "Failed to build window");
        }
    }
}

pub fn create_plugin_window(app: &AppHandle) {
    #[cfg(desktop)]
    {
        let window_builder =
            WebviewWindowBuilder::new(app, "plugin", WebviewUrl::App("index.html".into()))
                .title("Aipp")
                .inner_size(1000.0, 800.0)
                .fullscreen(false)
                .resizable(true)
                .decorations(true)
                .center();

        #[cfg(not(target_os = "macos"))]
        let window_builder = window_builder.transparent(false);

        match window_builder.build() {
            Ok(window) => {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { .. } = event {
                        window_clone.hide().unwrap();
                    }
                });
            }
            Err(e) => error!(error=%e, "Failed to build window"),
        }
    }
    #[cfg(mobile)]
    {
        let window_builder =
            WebviewWindowBuilder::new(app, "plugin", WebviewUrl::App("index.html".into()));
        if let Err(e) = window_builder.build() {
            error!(error=%e, "Failed to build window");
        }
    }
}

pub fn create_artifact_preview_window(app: &AppHandle) {
    #[cfg(desktop)]
    {
        let (window_size, window_position) =
            get_window_size_and_position(app, 1000.0, 800.0, &["ask", "chat_ui"]);

        let mut window_builder = WebviewWindowBuilder::new(
            app,
            "artifact_preview",
            WebviewUrl::App("index.html".into()),
        )
        .title("Artifact Preview - Aipp")
        .inner_size(window_size.width, window_size.height)
        .fullscreen(false)
        .resizable(true)
        .decorations(true);

        if let Some(position) = window_position {
            window_builder = window_builder.position(position.x, position.y);
        } else {
            window_builder = window_builder.center();
        }

        #[cfg(not(target_os = "macos"))]
        let window_builder = window_builder.transparent(false);

        match window_builder.build() {
            Ok(window) => {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { .. } = event {
                        window_clone.hide().unwrap();
                    }
                });
            }
            Err(e) => error!(error=%e, "Failed to build window"),
        }
    }
    #[cfg(mobile)]
    {
        let window_builder = WebviewWindowBuilder::new(
            app,
            "artifact_preview",
            WebviewUrl::App("index.html".into()),
        );
        if let Err(e) = window_builder.build() {
            error!(error=%e, "Failed to build window");
        }
    }
}

#[tauri::command]
pub async fn open_artifact_preview_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("artifact_preview").is_none() {
        debug!("Creating artifact preview window");

        create_artifact_preview_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("artifact_preview") {
        debug!("Showing artifact preview window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn open_config_window(app_handle: AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window("config") {
        debug!("Showing config window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    } else {
        // 窗口不存在时创建（正常情况下不应该发生，因为启动时已预创建）
        debug!("Creating config window (fallback)");
        create_config_window(&app_handle);
    }
    Ok(())
}

#[tauri::command]
pub async fn open_chat_ui_window(app_handle: AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window("chat_ui") {
        debug!("Showing chat_ui window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            // 首次显示时最大化
            if !window.is_visible().unwrap_or(false) {
                let _ = window.maximize();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
            if let Some(ask_window) = app_handle.get_webview_window("ask") {
                let _ = ask_window.hide();
            }
        }
    } else {
        // 窗口不存在时创建（正常情况下不应该发生，因为启动时已预创建）
        debug!("Creating chat_ui window (fallback)");
        create_chat_ui_window(&app_handle);
        #[cfg(desktop)]
        if let Some(ask_window) = app_handle.get_webview_window("ask") {
            let _ = ask_window.hide();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn open_plugin_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("plugin").is_none() {
        debug!("Creating window");

        create_plugin_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("plugin") {
        debug!("Showing window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ReactComponentPayload {
    code: String,
    css: String,
}

pub fn handle_open_ask_window(app_handle: &AppHandle) {
    use chrono::Local;
    let t_handle = std::time::Instant::now();

    if let Some(window) = app_handle.get_webview_window("ask") {
        debug!(ts=%Local::now().to_string(), "Showing ask window");
        #[cfg(desktop)]
        {
            let t_focus = std::time::Instant::now();
            if window.is_minimized().unwrap_or(false) {
                let t_unmin = std::time::Instant::now();
                window.unminimize().unwrap();
                info!(elapsed_ms=%t_unmin.elapsed().as_millis(), "ask window unminimize");
            }
            let t_show = std::time::Instant::now();
            window.show().unwrap();
            let show_ms = t_show.elapsed().as_millis();
            let t_setf = std::time::Instant::now();
            window.set_focus().unwrap();
            let focus_ms = t_setf.elapsed().as_millis();
            info!(show_ms=%show_ms, focus_ms=%focus_ms, total_ms=%t_focus.elapsed().as_millis(), "ask window show+focus timings");
        }
    } else {
        // 窗口不存在时创建（正常情况下不应该发生，因为启动时已预创建）
        info!(ts=%Local::now().to_string(), "Creating ask window (fallback)");
        let t_create = std::time::Instant::now();
        create_ask_window(app_handle);
        let dt = t_create.elapsed().as_millis();
        info!(elapsed_ms=%dt, "create_ask_window returned");
    }
    info!(elapsed_ms=%t_handle.elapsed().as_millis(), "handle_open_ask_window done");
}

pub fn awaken_aipp(app_handle: &AppHandle) {
    use chrono::Local;

    let chat_ui_window = app_handle.get_webview_window("chat_ui");
    let ask_window = app_handle.get_webview_window("ask");

    // 优先检查 chat_ui 窗口是否可见
    if let Some(window) = &chat_ui_window {
        #[cfg(desktop)]
        if window.is_visible().unwrap_or(false) {
            debug!(ts=%Local::now().to_string(), "Focusing visible chat_ui window");
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.set_focus().unwrap();
            return;
        }
    }

    // 其次检查 ask 窗口是否可见
    if let Some(window) = &ask_window {
        #[cfg(desktop)]
        if window.is_visible().unwrap_or(false) {
            debug!(ts=%Local::now().to_string(), "Focusing visible ask window");
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.set_focus().unwrap();
            return;
        }
    }

    // 都不可见时，显示 ask 窗口
    if let Some(window) = ask_window {
        debug!(ts=%Local::now().to_string(), "Showing hidden ask window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    } else {
        // 窗口不存在时创建（正常情况下不应该发生）
        info!(ts=%Local::now().to_string(), "Creating ask window (fallback)");
        create_ask_window(app_handle);
    }
}

/// 内部函数：显示配置窗口（用于托盘菜单）
pub fn open_config_window_inner(_app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(desktop)]
    {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 内部函数：显示聊天窗口（用于托盘菜单）
pub fn open_chat_ui_window_inner(app: &AppHandle, window: &tauri::WebviewWindow) {
    #[cfg(desktop)]
    {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        // 首次显示时最大化
        if !window.is_visible().unwrap_or(false) {
            let _ = window.maximize();
        }
        let _ = window.show();
        let _ = window.set_focus();
        // 显示聊天窗口时隐藏 Ask 窗口
        if let Some(ask_window) = app.get_webview_window("ask") {
            let _ = ask_window.hide();
        }
    }
}

// Create artifact collections window to manage saved artifacts
fn create_artifact_collections_window(app_handle: &AppHandle) {
    #[cfg(desktop)]
    {
        let (window_size, window_position) =
            get_window_size_and_position(app_handle, 1200.0, 800.0, &["chat_ui", "ask", "config"]);

        let builder = WebviewWindowBuilder::new(
            app_handle,
            "artifact_collections",
            WebviewUrl::App("artifacts_collections.html".into()),
        )
        .title("Artifacts 合集管理")
        .inner_size(window_size.width, window_size.height)
        .resizable(true)
        .minimizable(true)
        .maximizable(true)
        .center();

        let builder = if let Some(position) = window_position {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

        match builder.build() {
            Ok(_window) => {
                info!("Artifact collections window created successfully");
            }
            Err(e) => {
                error!(error=%e, "Failed to create artifact collections window");
            }
        }
    }
    #[cfg(mobile)]
    {
        let builder = WebviewWindowBuilder::new(
            app_handle,
            "artifact_collections",
            WebviewUrl::App("artifacts_collections.html".into()),
        );
        if let Err(e) = builder.build() {
            error!(error=%e, "Failed to create artifact collections window");
        }
    }
}

// Create artifact window to display a single artifact
fn create_artifact_window(app_handle: &AppHandle, artifact: &ArtifactCollection) {
    let window_label = "artifact";

    #[cfg(desktop)]
    {
        let (window_size, window_position) = get_window_size_and_position(
            app_handle,
            1000.0,
            700.0,
            &["artifact_collections", "ask", "chat_ui"],
        );

        let builder = WebviewWindowBuilder::new(
            app_handle,
            window_label,
            WebviewUrl::App("index.html".into()),
        )
        .title(artifact.name.clone())
        .inner_size(window_size.width, window_size.height)
        .resizable(true)
        .minimizable(true)
        .maximizable(true)
        .center();

        let builder = if let Some(position) = window_position {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

        match builder.build() {
            Ok(_window) => {
                info!(label=%window_label, "Artifact window created successfully");
                // 窗口会根据自己的 label 自动加载对应的 artifact 数据
            }
            Err(e) => {
                error!(error=%e, "Failed to create artifact window");
            }
        }
    }
    #[cfg(mobile)]
    {
        let builder = WebviewWindowBuilder::new(
            app_handle,
            window_label,
            WebviewUrl::App("index.html".into()),
        );
        if let Err(e) = builder.build() {
            error!(error=%e, "Failed to create artifact window");
        }
    }
}

/// Open artifact collections management window
#[tauri::command]
pub async fn open_artifact_collections_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("artifact_collections").is_none() {
        debug!("Creating artifact collections window");
        create_artifact_collections_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("artifact_collections") {
        debug!("Showing artifact collections window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    }
    Ok(())
}

/// Open artifact window to display a single artifact
pub async fn open_artifact_window(
    app_handle: AppHandle,
    artifact: ArtifactCollection,
) -> Result<(), String> {
    let window_label = "artifact";
    if app_handle.get_webview_window(&window_label).is_none() {
        debug!(label=%window_label, "Creating artifact window");
        create_artifact_window(&app_handle, &artifact);
    } else if let Some(window) = app_handle.get_webview_window(&window_label) {
        debug!(label=%window_label, "Showing artifact window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    }
    Ok(())
}

// Create schedule window
fn create_schedule_window(app_handle: &AppHandle) {
    #[cfg(desktop)]
    {
        let (window_size, window_position) =
            get_window_size_and_position(app_handle, 1200.0, 800.0, &["chat_ui", "ask", "config"]);

        let builder = WebviewWindowBuilder::new(app_handle, "schedule", WebviewUrl::App("index.html".into()))
        .title("定时任务")
        .inner_size(window_size.width, window_size.height)
        .resizable(true)
        .minimizable(true)
        .maximizable(true)
        .center();

        let builder = if let Some(position) = window_position {
            builder.position(position.x, position.y)
        } else {
            builder.center()
        };

        match builder.build() {
            Ok(_window) => {
                info!("Schedule window created successfully");
            }
            Err(e) => {
                error!(error=%e, "Failed to create schedule window");
            }
        }
    }
    #[cfg(mobile)]
    {
        let builder = WebviewWindowBuilder::new(app_handle, "schedule", WebviewUrl::App("index.html".into()));
        if let Err(e) = builder.build() {
            error!(error=%e, "Failed to create schedule window");
        }
    }
}

/// Open schedule window
#[tauri::command]
pub async fn open_schedule_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("schedule").is_none() {
        debug!("Creating schedule window");
        create_schedule_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("schedule") {
        debug!("Showing schedule window");
        #[cfg(desktop)]
        {
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    }
    Ok(())
}

// Create hidden search window for builtin MCP tools
fn create_hidden_search_window(app_handle: &AppHandle) {
    #[cfg(desktop)]
    {
        let builder = WebviewWindowBuilder::new(
            app_handle,
            "hidden_search",
            WebviewUrl::External("about:blank".parse().unwrap()),
        )
        .title("Search Window")
        .inner_size(800.0, 600.0)
        .resizable(false)
        .minimizable(true)
        .maximizable(false)
        .closable(false)
        .visible(true) // 改为可见，但会立即最小化
        .skip_taskbar(false) // 允许在任务栏显示
        .decorations(true);

        match builder.build() {
            Ok(window) => {
                info!("Search window created successfully");

                // 立即最小化窗口，让用户看不到但JavaScript仍可执行
                if let Err(e) = window.minimize() {
                    warn!(error=%e, "Failed to minimize search window");
                }

                // 测试JavaScript执行能力
                if let Err(e) =
                    window.eval("console.log('JavaScript execution test in search window');")
                {
                    warn!(error=%e, "JavaScript execution failed in search window");
                } else {
                    debug!("JavaScript execution test successful in search window");
                }
            }
            Err(e) => {
                error!(error=%e, "Failed to create search window");
            }
        }
    }
    #[cfg(mobile)]
    {
        // Mobile doesn't support hidden windows or multiple windows easily in the same way
        // But we can try creating a window
        let builder = WebviewWindowBuilder::new(
            app_handle,
            "hidden_search",
            WebviewUrl::External("about:blank".parse().unwrap()),
        );
        if let Err(e) = builder.build() {
            error!(error=%e, "Failed to create search window");
        }
    }
}

/// Create or get search window for builtin MCP tools
#[tauri::command]
pub async fn ensure_hidden_search_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("hidden_search").is_none() {
        debug!("Creating search window for content extraction");
        create_hidden_search_window(&app_handle);
    }
    Ok(())
}

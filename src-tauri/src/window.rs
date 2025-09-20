use crate::artifacts::artifacts_db::ArtifactCollection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri::{LogicalPosition, LogicalSize};
use tracing::{debug, error, info, warn};

/// 当按照显示器大小调整窗口尺寸时保留的屏幕占比（90%）
const SCREEN_MARGIN_RATIO: f64 = 0.9;

// 获取合适的窗口大小和位置
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
    let window_builder =
        WebviewWindowBuilder::new(app, "ask", WebviewUrl::App("index.html".into()))
            .title("Aipp")
            .inner_size(800.0, 450.0)
            .fullscreen(false)
            .resizable(false)
            .decorations(false)
            .center();

    #[cfg(not(target_os = "macos"))]
    let window_builder = window_builder.transparent(true);

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

pub fn create_config_window(app: &AppHandle) {
    let (window_size, window_position) =
        get_window_size_and_position(app, 1300.0, 1000.0, &["ask", "chat_ui"]);

    let mut window_builder =
        WebviewWindowBuilder::new(app, "config", WebviewUrl::App("index.html".into()))
            .title("Aipp")
            .inner_size(window_size.width, window_size.height)
            .fullscreen(false)
            .resizable(true)
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

pub fn create_chat_ui_window(app: &AppHandle) {
    let (window_size, window_position) = get_window_size_and_position(app, 1000.0, 800.0, &["ask"]);

    let mut window_builder =
        WebviewWindowBuilder::new(app, "chat_ui", WebviewUrl::App("index.html".into()))
            .title("Aipp")
            .inner_size(window_size.width, window_size.height)
            .fullscreen(false)
            .resizable(true)
            .decorations(true)
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
            let window_clone = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { .. } = event {
                    window_clone.hide().unwrap();
                }
            });
            let _ = window.maximize();
        }
        Err(e) => error!(error=%e, "Failed to build window"),
    }
}

pub fn create_plugin_window(app: &AppHandle) {
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

pub fn create_artifact_preview_window(app: &AppHandle) {
    let (window_size, window_position) =
        get_window_size_and_position(app, 1000.0, 800.0, &["ask", "chat_ui"]);

    let mut window_builder =
        WebviewWindowBuilder::new(app, "artifact_preview", WebviewUrl::App("index.html".into()))
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

#[tauri::command]
pub async fn open_artifact_preview_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("artifact_preview").is_none() {
        debug!("Creating artifact preview window");

        create_artifact_preview_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("artifact_preview") {
        debug!("Showing artifact preview window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
    }
    Ok(())
}

#[tauri::command]
pub async fn open_config_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("config").is_none() {
        debug!("Creating window");

        create_config_window(&app_handle)
    } else if let Some(window) = app_handle.get_webview_window("config") {
        debug!("Showing window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
    }
    Ok(())
}

#[tauri::command]
pub async fn open_chat_ui_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("chat_ui").is_none() {
        debug!("Creating window");

        create_chat_ui_window(&app_handle);
        app_handle.get_webview_window("ask").unwrap().hide().unwrap();
    } else if let Some(window) = app_handle.get_webview_window("chat_ui") {
        debug!("Showing window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
        app_handle.get_webview_window("ask").unwrap().hide().unwrap();
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
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
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

    let ask_window = app_handle.get_webview_window("ask");

    match ask_window {
        None => {
            info!(ts=%Local::now().to_string(), "Creating ask window");
            create_ask_window(app_handle);
        }
        Some(window) => {
            debug!(ts=%Local::now().to_string(), "Focusing ask window");
            if window.is_minimized().unwrap_or(false) {
                window.unminimize().unwrap();
            }
            window.show().unwrap();
            window.set_focus().unwrap();
        }
    }
}

pub fn awaken_aipp(app_handle: &AppHandle) {
    use chrono::Local;

    let ask_window = app_handle.get_webview_window("ask");
    let chat_ui_window = app_handle.get_webview_window("chat_ui");

    // 优先检查 chat_ui 窗口
    if let Some(window) = chat_ui_window {
        debug!(ts=%Local::now().to_string(), "Focusing chat_ui window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
        return;
    }

    // 其次检查 ask 窗口
    if let Some(window) = ask_window {
        debug!(ts=%Local::now().to_string(), "Focusing ask window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
        return;
    }

    // 最后创建 ask 窗口
    info!(ts=%Local::now().to_string(), "Creating ask window");
    create_ask_window(app_handle);
}

// Create artifact collections window to manage saved artifacts
fn create_artifact_collections_window(app_handle: &AppHandle) {
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

// Create artifact window to display a single artifact
fn create_artifact_window(app_handle: &AppHandle, artifact: &ArtifactCollection) {
    let window_label = "artifact";

    let (window_size, window_position) = get_window_size_and_position(
        app_handle,
        1000.0,
        700.0,
        &["artifact_collections", "ask", "chat_ui"],
    );

    let builder =
        WebviewWindowBuilder::new(app_handle, window_label, WebviewUrl::App("index.html".into()))
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

/// Open artifact collections management window
#[tauri::command]
pub async fn open_artifact_collections_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("artifact_collections").is_none() {
        debug!("Creating artifact collections window");
        create_artifact_collections_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("artifact_collections") {
        debug!("Showing artifact collections window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
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
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
    }
    Ok(())
}

// Create plugin store window
fn create_plugin_store_window(app_handle: &AppHandle) {
    let (window_size, window_position) =
        get_window_size_and_position(app_handle, 1200.0, 800.0, &["chat_ui", "ask", "config"]);

    let builder =
        WebviewWindowBuilder::new(app_handle, "plugin_store", WebviewUrl::App("index.html".into()))
            .title("插件商店")
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
            info!("Plugin store window created successfully");
        }
        Err(e) => {
            error!(error=%e, "Failed to create plugin store window");
        }
    }
}

/// Open plugin store window
#[tauri::command]
pub async fn open_plugin_store_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("plugin_store").is_none() {
        debug!("Creating plugin store window");
        create_plugin_store_window(&app_handle);
    } else if let Some(window) = app_handle.get_webview_window("plugin_store") {
        debug!("Showing plugin store window");
        if window.is_minimized().unwrap_or(false) {
            window.unminimize().unwrap();
        }
        window.show().unwrap();
        window.set_focus().unwrap();
    }
    Ok(())
}

// Create hidden search window for builtin MCP tools
fn create_hidden_search_window(app_handle: &AppHandle) {
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

/// Create or get search window for builtin MCP tools
#[tauri::command]
pub async fn ensure_hidden_search_window(app_handle: AppHandle) -> Result<(), String> {
    if app_handle.get_webview_window("hidden_search").is_none() {
        debug!("Creating search window for content extraction");
        create_hidden_search_window(&app_handle);
    }
    Ok(())
}

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tauri::{Emitter, Manager, State};

#[tauri::command]
pub async fn check_bun_version(app: tauri::AppHandle) -> Result<String, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || crate::utils::bun_utils::BunUtils::get_bun_version(&app))
        .await
        .map_err(|e| format!("任务执行失败: {}", e))?
}

#[tauri::command]
pub async fn check_uv_version(app: tauri::AppHandle) -> Result<String, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || crate::utils::uv_utils::UvUtils::get_uv_version(&app))
        .await
        .map_err(|e| format!("任务执行失败: {}", e))?
}

/// GitHub Release 信息
#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

/// 从 GitHub Releases API 获取最新版本
async fn fetch_latest_version(
    repo: &str,
    use_proxy: bool,
    proxy_url: Option<&str>,
) -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    tracing::info!("检查更新 - 仓库: {}, 使用代理: {}, 代理地址: {:?}", repo, use_proxy, proxy_url);

    let mut client_builder = reqwest::Client::builder();
    if use_proxy {
        if let Some(proxy) = proxy_url {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                client_builder = client_builder.proxy(p);
                tracing::info!("已配置代理: {}", proxy);
            }
        }
    }

    let client = client_builder
        .user_agent("AIPP-App")
        .build()
        .map_err(|e| format!("创建客户端失败: {}", e))?;

    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API 返回错误: {}", response.status()));
    }

    let release: GitHubRelease =
        response.json().await.map_err(|e| format!("解析响应失败: {}", e))?;

    // 移除版本前缀（Bun 使用 bun-v 前缀，其他项目使用 v 前缀）
    let version = release
        .tag_name
        .strip_prefix("bun-v")
        .or_else(|| release.tag_name.strip_prefix('v'))
        .unwrap_or(&release.tag_name)
        .to_string();
    tracing::info!(
        "获取到最新版本 - 仓库: {}, 原始标签: {}, 版本: {}",
        repo,
        release.tag_name,
        version
    );
    Ok(version)
}

/// 比较版本号，返回 true 如果 latest > current
fn compare_versions(current: &str, latest: &str) -> bool {
    let current_parts: Vec<&str> = current.split('.').collect();
    let latest_parts: Vec<&str> = latest.split('.').collect();

    for i in 0..3 {
        let c = current_parts.get(i).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        let l = latest_parts.get(i).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }
    false
}

#[tauri::command]
pub async fn check_bun_update(
    app: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
) -> Result<Option<String>, String> {
    check_bun_update_impl(app, feature_config_state, false).await
}

#[tauri::command]
pub async fn check_bun_update_with_proxy(
    app: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
) -> Result<Option<String>, String> {
    check_bun_update_impl(app, feature_config_state, true).await
}

async fn check_bun_update_impl(
    app: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    use_proxy: bool,
) -> Result<Option<String>, String> {
    tracing::info!("开始检查 Bun 更新，使用代理: {}", use_proxy);

    // 获取当前版本
    let current_version = check_bun_version(app).await?;
    tracing::info!("Bun 当前版本: {}", current_version);

    if current_version == "Not Installed" {
        tracing::info!("Bun 未安装，跳过更新检查");
        return Ok(None);
    }

    // 获取代理配置
    let proxy_url = if use_proxy {
        let config_map = feature_config_state.config_feature_map.lock().await;
        crate::api::ai::config::get_network_proxy_from_config(&config_map)
    } else {
        None
    };

    // 获取最新版本
    let latest_version =
        fetch_latest_version("oven-sh/bun", use_proxy, proxy_url.as_deref()).await?;

    // 比较版本
    let has_update = compare_versions(&current_version, &latest_version);
    tracing::info!(
        "Bun 版本比较 - 当前: {}, 最新: {}, 需要更新: {}",
        current_version,
        latest_version,
        has_update
    );

    if has_update {
        Ok(Some(latest_version))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn check_uv_update(
    app: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
) -> Result<Option<String>, String> {
    check_uv_update_impl(app, feature_config_state, false).await
}

#[tauri::command]
pub async fn check_uv_update_with_proxy(
    app: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
) -> Result<Option<String>, String> {
    check_uv_update_impl(app, feature_config_state, true).await
}

async fn check_uv_update_impl(
    app: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    use_proxy: bool,
) -> Result<Option<String>, String> {
    tracing::info!("开始检查 uv 更新，使用代理: {}", use_proxy);

    // 获取当前版本
    let current_version = check_uv_version(app).await?;
    tracing::info!("uv 当前版本: {}", current_version);

    if current_version == "Not Installed" {
        tracing::info!("uv 未安装，跳过更新检查");
        return Ok(None);
    }

    // 获取代理配置
    let proxy_url = if use_proxy {
        let config_map = feature_config_state.config_feature_map.lock().await;
        crate::api::ai::config::get_network_proxy_from_config(&config_map)
    } else {
        None
    };

    // 获取最新版本
    let latest_version =
        fetch_latest_version("astral-sh/uv", use_proxy, proxy_url.as_deref()).await?;

    // 比较版本
    let has_update = compare_versions(&current_version, &latest_version);
    tracing::info!(
        "uv 版本比较 - 当前: {}, 最新: {}, 需要更新: {}",
        current_version,
        latest_version,
        has_update
    );

    if has_update {
        Ok(Some(latest_version))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub fn update_bun(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    target_window: Option<String>,
) -> Result<(), String> {
    update_bun_impl(app_handle, feature_config_state, target_window, false)
}

#[tauri::command]
pub fn update_bun_with_proxy(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    target_window: Option<String>,
) -> Result<(), String> {
    update_bun_impl(app_handle, feature_config_state, target_window, true)
}

fn update_bun_impl(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    target_window: Option<String>,
    use_proxy: bool,
) -> Result<(), String> {
    tracing::info!("开始更新 Bun，使用代理: {}", use_proxy);

    let (event_prefix, emit_to_window) = if let Some(ref window) = target_window {
        if window == "artifact_preview" {
            ("artifact", true)
        } else {
            ("bun-install", true)
        }
    } else {
        ("bun-install", false)
    };

    // 获取代理配置（需要在 spawn 前获取）
    let proxy_url = if use_proxy {
        let config_map = feature_config_state.config_feature_map.blocking_lock();
        crate::api::ai::config::get_network_proxy_from_config(&config_map)
    } else {
        None
    };

    tracing::info!("Bun 更新 - 代理配置: {:?}", proxy_url);

    std::thread::spawn(move || {
        let (os, arch) = if cfg!(target_os = "windows") {
            ("windows", "x64")
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                ("darwin", "aarch64")
            } else {
                ("darwin", "x64")
            }
        } else {
            ("linux", "x64")
        };

        tracing::info!("Bun 更新 - 平台: {}-{}", os, arch);

        let emit_log = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-log", event_prefix), msg);
                    }
                }
            } else {
                let _ = app_handle.emit("bun-install-log", msg);
            }
        };

        let emit_error = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-error", event_prefix), msg);
                        let _ = window.emit("bun-install-finished", false);
                    }
                }
            } else {
                let _ = app_handle.emit("bun-install-log", msg);
                let _ = app_handle.emit("bun-install-finished", false);
            }
        };

        let emit_success = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-success", event_prefix), msg);
                        let _ = window.emit("bun-install-finished", true);
                    }
                }
            } else {
                let _ = app_handle.emit("bun-install-log", msg);
                let _ = app_handle.emit("bun-install-finished", true);
            }
        };

        emit_log("获取 Bun 最新版本...");

        // 获取最新版本
        let rt = tokio::runtime::Runtime::new();
        let rt = match rt {
            Ok(r) => r,
            Err(e) => {
                emit_error(&format!("创建运行时失败: {}", e));
                return;
            }
        };

        let latest_version = rt.block_on(async {
            fetch_latest_version("oven-sh/bun", use_proxy, proxy_url.as_deref()).await
        });

        let bun_version = match latest_version {
            Ok(v) => {
                tracing::info!("Bun 更新 - 将要安装版本: {}", v);
                emit_log(&format!("最新版本: {}", v));
                v
            }
            Err(e) => {
                tracing::error!("Bun 更新 - 获取版本失败: {}", e);
                emit_error(&format!("获取版本失败: {}", e));
                return;
            }
        };

        let url = format!(
            "https://registry.npmmirror.com/-/binary/bun/bun-v{}/bun-{}-{}.zip",
            bun_version, os, arch
        );

        tracing::info!("Bun 更新 - 下载地址: {}", url);

        emit_log("开始下载 Bun");

        // 使用代理下载
        if use_proxy && proxy_url.is_some() {
            let rt = tokio::runtime::Runtime::new();
            let rt = match rt {
                Ok(r) => r,
                Err(e) => {
                    emit_error(&format!("创建运行时失败: {}", e));
                    return;
                }
            };

            let download_result = rt.block_on(async {
                let mut client_builder = reqwest::Client::builder();
                if let Some(ref proxy) = proxy_url {
                    if let Ok(p) = reqwest::Proxy::all(proxy) {
                        client_builder = client_builder.proxy(p);
                    }
                }

                let client =
                    client_builder.build().map_err(|e| format!("创建客户端失败: {}", e))?;
                let response =
                    client.get(&url).send().await.map_err(|e| format!("下载失败: {}", e))?;
                let bytes = response.bytes().await.map_err(|e| format!("读取响应失败: {}", e))?;

                let app_data_dir = app_handle.path().app_data_dir().expect("无法获取应用数据目录");
                let bun_install_dir = app_data_dir.join("bun");
                let bun_bin_dir = bun_install_dir.join("bin");

                std::fs::create_dir_all(&bun_bin_dir)
                    .map_err(|e| format!("创建目录失败: {}", e))?;
                let zip_path = bun_install_dir.join("bun.zip");

                std::fs::write(&zip_path, bytes).map_err(|e| format!("写入文件失败: {}", e))?;
                Ok::<(), String>(())
            });

            if download_result.is_err() {
                return;
            }
        } else {
            // 使用原有的同步下载
            let app_data_dir = app_handle.path().app_data_dir().expect("无法获取应用数据目录");
            let bun_install_dir = app_data_dir.join("bun");
            let bun_bin_dir = bun_install_dir.join("bin");

            if let Err(e) = std::fs::create_dir_all(&bun_bin_dir) {
                emit_error(&format!("创建目录失败: {}", e));
                return;
            }
            let zip_path = bun_install_dir.join("bun.zip");

            match reqwest::blocking::get(&url) {
                Ok(mut response) => match std::fs::File::create(&zip_path) {
                    Ok(mut file) => {
                        if let Err(e) = std::io::copy(&mut response, &mut file) {
                            emit_error(&format!("下载失败: {}", e));
                            return;
                        }
                        emit_log("下载完成");
                    }
                    Err(e) => {
                        emit_error(&format!("创建文件失败: {}", e));
                        return;
                    }
                },
                Err(e) => {
                    emit_error(&format!("下载失败: {}", e));
                    return;
                }
            }
        }

        emit_log("开始解压...");

        let app_data_dir = app_handle.path().app_data_dir().expect("无法获取应用数据目录");
        let bun_install_dir = app_data_dir.join("bun");
        let zip_path = bun_install_dir.join("bun.zip");

        match std::fs::File::open(&zip_path) {
            Ok(zip_file) => {
                if let Err(e) = zip_extract::extract(zip_file, &bun_install_dir, true) {
                    emit_error(&format!("解压失败: {}", e));
                    return;
                }
                emit_log("解压成功");
            }
            Err(e) => {
                emit_error(&format!("打开压缩文件失败: {}", e));
                return;
            }
        }

        let bun_executable_name = if cfg!(target_os = "windows") { "bun.exe" } else { "bun" };
        let candidate_paths = [
            bun_install_dir.join(&bun_executable_name),
            bun_install_dir.join(format!("bun-{}-{}", os, arch)).join(&bun_executable_name),
        ];
        let bun_executable_path = match candidate_paths.iter().find(|p| p.exists()) {
            Some(p) => p.to_path_buf(),
            None => {
                emit_error("未找到 bun 可执行文件");
                return;
            }
        };

        let bun_bin_dir = bun_install_dir.join("bin");
        let dest_path = bun_bin_dir.join(&bun_executable_name);
        if dest_path.exists() {
            if let Err(e) = std::fs::remove_file(&dest_path) {
                emit_error(&format!("删除旧文件失败: {}", e));
                return;
            }
        }
        if let Err(e) = std::fs::rename(bun_executable_path, &dest_path) {
            emit_error(&format!("移动文件失败: {}", e));
            return;
        }

        emit_success("Bun 更新成功");
    });

    Ok(())
}

#[tauri::command]
pub fn update_uv(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    target_window: Option<String>,
) -> Result<(), String> {
    update_uv_impl(app_handle, feature_config_state, target_window, false)
}

#[tauri::command]
pub fn update_uv_with_proxy(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    target_window: Option<String>,
) -> Result<(), String> {
    update_uv_impl(app_handle, feature_config_state, target_window, true)
}

fn update_uv_impl(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    target_window: Option<String>,
    use_proxy: bool,
) -> Result<(), String> {
    tracing::info!("开始更新 uv，使用代理: {}", use_proxy);

    let (event_prefix, emit_to_window) = if let Some(ref window) = target_window {
        if window == "artifact_preview" {
            ("artifact", true)
        } else {
            ("uv-install", true)
        }
    } else {
        ("uv-install", false)
    };

    // 获取代理配置
    let proxy_url = if use_proxy {
        let config_map = feature_config_state.config_feature_map.blocking_lock();
        crate::api::ai::config::get_network_proxy_from_config(&config_map)
    } else {
        None
    };

    tracing::info!("uv 更新 - 代理配置: {:?}", proxy_url);

    std::thread::spawn(move || {
        let emit_log = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-log", event_prefix), msg);
                    }
                }
            } else {
                let _ = app_handle.emit("uv-install-log", msg);
            }
        };

        let emit_error = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-error", event_prefix), msg);
                        let _ = window.emit("uv-install-finished", false);
                    }
                }
            } else {
                let _ = app_handle.emit("uv-install-log", msg);
                let _ = app_handle.emit("uv-install-finished", false);
            }
        };

        let emit_success = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-success", event_prefix), msg);
                        let _ = window.emit("uv-install-finished", true);
                    }
                }
            } else {
                let _ = app_handle.emit("uv-install-log", msg);
                let _ = app_handle.emit("uv-install-finished", true);
            }
        };

        emit_log("开始更新 uv...");

        let (command, args) = if cfg!(target_os = "windows") {
            ("powershell", vec!["-c", "irm https://astral.sh/uv/install.ps1 | iex"])
        } else {
            (
                "sh",
                vec![
                    "-c",
                    "curl -LsSf --retry 3 --retry-delay 2 https://astral.sh/uv/install.sh | sh",
                ],
            )
        };

        let mut cmd = Command::new(command);
        cmd.args(args);

        // 设置代理环境变量
        if let Some(ref proxy) = proxy_url {
            cmd.env("HTTP_PROXY", proxy);
            cmd.env("HTTPS_PROXY", proxy);
            emit_log(&format!("使用代理: {}", proxy));
        }

        cmd.env("UV_INSTALLER_GHE_BASE_URL", "https://ghfast.top/https://github.com");

        let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(e) => {
                emit_error(&format!("启动安装命令失败: {}", e));
                return;
            }
        };

        let mut has_critical_error = false;
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                emit_log(&line);
            }
        }
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                if line.contains("curl:")
                    && (line.contains("Error in the HTTP2 framing layer")
                        || line.contains("Recv failure: Connection reset by peer")
                        || line.contains("Failed to connect")
                        || line.contains("Could not resolve host"))
                {
                    has_critical_error = true;
                }
                emit_log(&line);
            }
        }

        match child.wait() {
            Ok(status) => {
                if status.success() && !has_critical_error {
                    emit_success("uv 更新成功！");
                } else {
                    emit_error(&if has_critical_error {
                        "更新失败：检测到网络错误".to_string()
                    } else {
                        format!("更新失败，退出码: {}", status.code().unwrap_or(-1))
                    });
                }
            }
            Err(e) => {
                emit_error(&format!("等待进程失败: {}", e));
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn install_bun(
    app_handle: tauri::AppHandle,
    target_window: Option<String>,
) -> Result<(), String> {
    let (event_prefix, emit_to_window) = if let Some(ref window) = target_window {
        if window == "artifact_preview" {
            ("artifact", true)
        } else {
            ("bun-install", true)
        }
    } else {
        ("bun-install", false)
    };

    std::thread::spawn(move || {
        let bun_version = "1.2.18";
        let (os, arch) = if cfg!(target_os = "windows") {
            ("windows", "x64")
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                ("darwin", "aarch64")
            } else {
                ("darwin", "x64")
            }
        } else {
            ("linux", "x64")
        };

        let url = format!(
            "https://registry.npmmirror.com/-/binary/bun/bun-v{}/bun-{}-{}.zip",
            bun_version, os, arch
        );

        let emit_log = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-log", event_prefix), msg);
                    }
                }
            } else {
                let _ = app_handle.emit("bun-install-log", msg);
            }
        };

        let emit_error = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-error", event_prefix), msg);
                        let _ = window.emit("bun-install-finished", false);
                    }
                }
            } else {
                let _ = app_handle.emit("bun-install-log", msg);
                let _ = app_handle.emit("bun-install-finished", false);
            }
        };

        let emit_success = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-success", event_prefix), msg);
                        let _ = window.emit("bun-install-finished", true);
                    }
                }
            } else {
                let _ = app_handle.emit("bun-install-log", msg);
                let _ = app_handle.emit("bun-install-finished", true);
            }
        };

        emit_log("开始下载 Bun");

        let app_data_dir = app_handle.path().app_data_dir().expect("无法获取应用数据目录");
        let bun_install_dir = app_data_dir.join("bun");
        let bun_bin_dir = bun_install_dir.join("bin");

        if let Err(e) = std::fs::create_dir_all(&bun_bin_dir) {
            emit_error(&format!("创建目录失败: {}", e));
            return;
        }
        let zip_path = bun_install_dir.join("bun.zip");

        match reqwest::blocking::get(&url) {
            Ok(mut response) => match std::fs::File::create(&zip_path) {
                Ok(mut file) => {
                    if let Err(e) = std::io::copy(&mut response, &mut file) {
                        emit_error(&format!("下载失败: {}", e));
                        return;
                    }
                    emit_log("下载完成");
                }
                Err(e) => {
                    emit_error(&format!("创建文件失败: {}", e));
                    return;
                }
            },
            Err(e) => {
                emit_error(&format!("下载失败: {}", e));
                return;
            }
        }

        emit_log("开始解压...");
        match std::fs::File::open(&zip_path) {
            Ok(zip_file) => {
                if let Err(e) = zip_extract::extract(zip_file, &bun_install_dir, true) {
                    emit_error(&format!("解压失败: {}", e));
                    return;
                }
                emit_log("解压成功");
            }
            Err(e) => {
                emit_error(&format!("打开压缩文件失败: {}", e));
                return;
            }
        }

        let bun_executable_name = if cfg!(target_os = "windows") { "bun.exe" } else { "bun" };
        let candidate_paths = [
            bun_install_dir.join(&bun_executable_name),
            bun_install_dir.join(format!("bun-{}-{}", os, arch)).join(&bun_executable_name),
        ];
        let bun_executable_path = match candidate_paths.iter().find(|p| p.exists()) {
            Some(p) => p.to_path_buf(),
            None => {
                emit_error("未找到 bun 可执行文件");
                return;
            }
        };

        let dest_path = bun_bin_dir.join(&bun_executable_name);
        if dest_path.exists() {
            if let Err(e) = std::fs::remove_file(&dest_path) {
                emit_error(&format!("删除旧文件失败: {}", e));
                return;
            }
        }
        if let Err(e) = std::fs::rename(bun_executable_path, &dest_path) {
            emit_error(&format!("移动文件失败: {}", e));
            return;
        }

        emit_success("Bun 安装成功");
    });

    Ok(())
}

#[tauri::command]
pub fn install_uv(
    app_handle: tauri::AppHandle,
    target_window: Option<String>,
) -> Result<(), String> {
    let (event_prefix, emit_to_window) = if let Some(ref window) = target_window {
        if window == "artifact_preview" {
            ("artifact", true)
        } else {
            ("uv-install", true)
        }
    } else {
        ("uv-install", false)
    };

    std::thread::spawn(move || {
        let max_retries = 3;
        let mut success = false;

        let emit_log = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-log", event_prefix), msg);
                    }
                }
            } else {
                let _ = app_handle.emit("uv-install-log", msg);
            }
        };
        let emit_error = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-error", event_prefix), msg);
                        let _ = window.emit("uv-install-finished", false);
                    }
                }
            } else {
                let _ = app_handle.emit("uv-install-log", msg);
                let _ = app_handle.emit("uv-install-finished", false);
            }
        };
        let emit_success = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-success", event_prefix), msg);
                        let _ = window.emit("uv-install-finished", true);
                    }
                }
            } else {
                let _ = app_handle.emit("uv-install-log", msg);
                let _ = app_handle.emit("uv-install-finished", true);
            }
        };

        for attempt in 1..=max_retries {
            emit_log(&format!("正在尝试安装 uv (第 {} 次尝试)...", attempt));

            let (command, args) = if cfg!(target_os = "windows") {
                ("powershell", vec!["-c", "irm https://astral.sh/uv/install.ps1 | iex"])
            } else {
                (
                    "sh",
                    vec![
                        "-c",
                        "curl -LsSf --retry 3 --retry-delay 2 https://astral.sh/uv/install.sh | sh",
                    ],
                )
            };

            let mut child = match Command::new(command)
                .args(args)
                .env("UV_INSTALLER_GHE_BASE_URL", "https://ghfast.top/https://github.com")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    emit_error(&format!("启动安装命令失败: {}", e));
                    continue;
                }
            };

            let mut has_critical_error = false;
            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        emit_log(&line);
                    }
                }
            }
            if let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        if line.contains("curl:")
                            && (line.contains("Error in the HTTP2 framing layer")
                                || line.contains("Recv failure: Connection reset by peer")
                                || line.contains("Failed to connect")
                                || line.contains("Could not resolve host"))
                        {
                            has_critical_error = true;
                        }
                        emit_log(&line);
                    }
                }
            }

            match child.wait() {
                Ok(status) => {
                    if status.success() && !has_critical_error {
                        success = true;
                        emit_success("uv 安装成功！");
                        break;
                    } else {
                        emit_error(&if has_critical_error {
                            format!("第 {} 次尝试失败：检测到网络错误", attempt)
                        } else {
                            format!(
                                "第 {} 次尝试失败，退出码: {}",
                                attempt,
                                status.code().unwrap_or(-1)
                            )
                        });
                        if attempt < max_retries {
                            emit_log("等待 2 秒后重试...");
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                }
                Err(e) => {
                    emit_error(&format!("等待进程失败: {}", e));
                }
            }
        }

        if !success {
            emit_error(&format!("经过 {} 次尝试后，uv 安装失败", max_retries));
        }

        if emit_to_window {
            if let Some(ref window_name) = target_window {
                if let Some(window) = app_handle.get_webview_window(window_name) {
                    let _ = window.emit("uv-install-finished", success);
                }
            }
        } else {
            let _ = app_handle.emit("uv-install-finished", success);
        }
    });

    Ok(())
}

/// 获取 Python 版本信息
#[tauri::command]
pub async fn get_python_info(app: tauri::AppHandle) -> crate::utils::python_utils::PythonInfo {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        crate::utils::python_utils::PythonUtils::get_python_info(&app)
    })
    .await
    .unwrap_or_else(|_| crate::utils::python_utils::PythonInfo::default())
}

/// 安装 Python 3
#[tauri::command]
pub fn install_python3(
    app_handle: tauri::AppHandle,
    target_window: Option<String>,
) -> Result<(), String> {
    let (event_prefix, emit_to_window) = if let Some(ref window) = target_window {
        if window == "artifact_preview" {
            ("artifact", true)
        } else {
            ("python-install", true)
        }
    } else {
        ("python-install", false)
    };

    std::thread::spawn(move || {
        let emit_log = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-log", event_prefix), msg);
                    }
                }
            } else {
                let _ = app_handle.emit("python-install-log", msg);
            }
        };

        let emit_error = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-error", event_prefix), msg);
                        let _ = window.emit("python-install-finished", false);
                    }
                }
            } else {
                let _ = app_handle.emit("python-install-log", msg);
                let _ = app_handle.emit("python-install-finished", false);
            }
        };

        let emit_success = |msg: &str| {
            if emit_to_window {
                if let Some(ref window_name) = target_window {
                    if let Some(window) = app_handle.get_webview_window(window_name) {
                        let _ = window.emit(&format!("{}-success", event_prefix), msg);
                        let _ = window.emit("python-install-finished", true);
                    }
                }
            } else {
                let _ = app_handle.emit("python-install-log", msg);
                let _ = app_handle.emit("python-install-finished", true);
            }
        };

        tracing::info!("开始安装 Python 3");

        // 检查 uv 是否可用
        let uv_exe = if cfg!(target_os = "windows") { "uv.exe" } else { "uv" };

        // 检查 uv 是否安装
        match Command::new(uv_exe).arg("--version").output() {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                tracing::info!("检测到 uv 版本: {}", version);
                emit_log(&format!("使用 uv {} 安装 Python 3", version));
            }
            Ok(_) => {
                let msg = "uv 未正确安装，无法安装 Python".to_string();
                tracing::error!("{}", msg);
                emit_error(&msg);
                return;
            }
            Err(e) => {
                let msg = format!("未找到 uv，无法安装 Python: {}", e);
                tracing::error!("{}", msg);
                emit_error(&msg);
                return;
            }
        }

        emit_log("正在下载并安装最新的 Python 3...");

        let (command, args) = if cfg!(target_os = "windows") {
            ("cmd", vec!["/c", uv_exe, "python", "install", "3"])
        } else {
            (uv_exe, vec!["python", "install", "3"])
        };

        tracing::info!("执行命令: {} {:?}", command, args);

        let mut cmd = Command::new(command);
        cmd.args(args);

        // 设置加速镜像环境变量
        cmd.env("UV_INSTALLER_GHE_BASE_URL", "https://ghfast.top/https://github.com");

        let mut child = match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => child,
            Err(e) => {
                let msg = format!("启动安装命令失败: {}", e);
                tracing::error!("{}", msg);
                emit_error(&msg);
                return;
            }
        };

        let mut has_critical_error = false;
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                emit_log(&line);
            }
        }
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                // 检测网络错误
                if line.contains("error:")
                    || (line.contains("Failed") && line.contains("download"))
                    || line.contains("Connection reset")
                    || line.contains("Could not connect")
                {
                    has_critical_error = true;
                }
                emit_log(&line);
            }
        }

        match child.wait() {
            Ok(status) => {
                if status.success() && !has_critical_error {
                    tracing::info!("Python 3 安装成功");
                    emit_success("Python 3 安装成功！");
                } else {
                    let msg = if has_critical_error {
                        "安装失败：检测到网络错误".to_string()
                    } else {
                        format!("安装失败，退出码: {}", status.code().unwrap_or(-1))
                    };
                    tracing::error!("{}", msg);
                    emit_error(&msg);
                }
            }
            Err(e) => {
                let msg = format!("等待进程失败: {}", e);
                tracing::error!("{}", msg);
                emit_error(&msg);
            }
        }
    });

    Ok(())
}

// ============================================================================
// ACP 环境检测和安装
// ============================================================================

/// ACP 库信息
#[derive(serde::Serialize, Clone)]
pub struct AcpLibraryInfo {
    /// CLI 命令名称
    pub cli_command: String,
    /// 对应的 npm 包名
    pub package_name: String,
    /// 是否已安装
    pub installed: bool,
    /// 安装的版本（如果已安装）
    pub version: Option<String>,
    /// 是否需要外部安装（如 gemini 需要用户自行安装）
    pub requires_external_install: bool,
    /// 安装说明
    pub install_hint: String,
}

/// 获取 ACP 库的配置信息
fn get_acp_library_config(cli_command: &str) -> (String, bool, String) {
    match cli_command {
        "claude-code-acp" => (
            "@zed-industries/claude-code-acp".to_string(),
            false,
            "需要设置 ANTHROPIC_API_KEY 环境变量".to_string(),
        ),
        "codex-acp" => (
            "@zed-industries/codex-acp".to_string(),
            false,
            "需要设置 OPENAI_API_KEY 环境变量".to_string(),
        ),
        "gemini" => (
            "gemini".to_string(),
            true,
            "请参考 Google Gemini CLI 官方文档安装".to_string(),
        ),
        _ => (
            cli_command.to_string(),
            true,
            "请手动安装该 CLI 工具".to_string(),
        ),
    }
}

/// 检查 ACP CLI 工具是否已安装
#[tauri::command]
pub async fn check_acp_library(
    app: tauri::AppHandle,
    cli_command: String,
) -> Result<AcpLibraryInfo, String> {
    let app_clone = app.clone();
    let cli_command_clone = cli_command.clone();

    tokio::task::spawn_blocking(move || {
        let (package_name, requires_external, install_hint) =
            get_acp_library_config(&cli_command_clone);

        // 对于需要外部安装的工具，直接检查系统 PATH
        if requires_external {
            let output = std::process::Command::new(&cli_command_clone)
                .arg("--version")
                .output();

            return match output {
                Ok(out) if out.status.success() => {
                    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    AcpLibraryInfo {
                        cli_command: cli_command_clone,
                        package_name,
                        installed: true,
                        version: Some(version),
                        requires_external_install: true,
                        install_hint,
                    }
                }
                _ => AcpLibraryInfo {
                    cli_command: cli_command_clone,
                    package_name,
                    installed: false,
                    version: None,
                    requires_external_install: true,
                    install_hint,
                },
            };
        }

        // 对于可以通过 bun 安装的包，检查全局安装目录
        let bun_path = match crate::utils::bun_utils::BunUtils::get_bun_executable(&app_clone) {
            Ok(path) => path,
            Err(_) => {
                return AcpLibraryInfo {
                    cli_command: cli_command_clone,
                    package_name,
                    installed: false,
                    version: None,
                    requires_external_install: false,
                    install_hint: "需要先安装 Bun 运行时".to_string(),
                };
            }
        };

        // 使用 bun pm ls -g 检查全局包
        let output = std::process::Command::new(&bun_path)
            .args(["pm", "ls", "-g"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let is_installed = stdout.contains(&package_name);

                // 如果已安装，尝试获取版本
                let version = if is_installed {
                    // 尝试运行命令获取版本
                    std::process::Command::new(&cli_command_clone)
                        .arg("--version")
                        .output()
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                };

                AcpLibraryInfo {
                    cli_command: cli_command_clone,
                    package_name,
                    installed: is_installed,
                    version,
                    requires_external_install: false,
                    install_hint,
                }
            }
            _ => AcpLibraryInfo {
                cli_command: cli_command_clone,
                package_name,
                installed: false,
                version: None,
                requires_external_install: false,
                install_hint,
            },
        }
    })
    .await
    .map_err(|e| format!("任务执行失败: {}", e))
}

/// 安装 ACP 库
#[tauri::command]
pub async fn install_acp_library(
    app: tauri::AppHandle,
    cli_command: String,
) -> Result<(), String> {
    let (package_name, requires_external, _) = get_acp_library_config(&cli_command);

    if requires_external {
        return Err(format!(
            "{} 需要手动安装，无法自动安装",
            cli_command
        ));
    }

    let bun_path = crate::utils::bun_utils::BunUtils::get_bun_executable(&app)
        .map_err(|e| format!("获取 Bun 路径失败: {}", e))?;

    tracing::info!("开始安装 ACP 库: {}", package_name);

    // 发送安装开始事件
    let _ = app.emit("acp-install-log", format!("开始安装 {}...", package_name));

    // 启动后台安装任务
    let app_clone = app.clone();
    let package_name_clone = package_name.clone();
    let cli_command_clone = cli_command.clone();

    tokio::task::spawn_blocking(move || {
        let emit_log = |msg: &str| {
            tracing::info!("ACP Install: {}", msg);
            let _ = app_clone.emit("acp-install-log", msg.to_string());
        };

        let emit_finished = |success: bool| {
            let _ = app_clone.emit("acp-install-finished", serde_json::json!({
                "success": success,
                "cli_command": cli_command_clone,
                "package_name": package_name_clone,
            }));
        };

        emit_log(&format!("执行: bun add -g {}", package_name_clone));

        let output = std::process::Command::new(&bun_path)
            .args(["add", "-g", &package_name_clone])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);

                if !stdout.is_empty() {
                    emit_log(&stdout);
                }
                if !stderr.is_empty() {
                    emit_log(&stderr);
                }

                if out.status.success() {
                    emit_log(&format!("{} 安装成功!", package_name_clone));
                    emit_finished(true);
                } else {
                    emit_log(&format!(
                        "安装失败，退出码: {}",
                        out.status.code().unwrap_or(-1)
                    ));
                    emit_finished(false);
                }
            }
            Err(e) => {
                emit_log(&format!("执行安装命令失败: {}", e));
                emit_finished(false);
            }
        }
    });

    Ok(())
}

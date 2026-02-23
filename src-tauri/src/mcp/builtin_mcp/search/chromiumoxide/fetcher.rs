use super::super::browser::BrowserManager;
use super::super::engine_manager::SearchEngine;
use super::super::fingerprint::{FingerprintConfig, FingerprintManager, TimingConfig};
use super::browser_pool::BrowserPool;
use chromiumoxide_cdp::cdp::browser_protocol::{emulation, network, page as cdp_page};
use futures::StreamExt;
use rand::Rng;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::process::Command as TokioCommand;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, trace, warn};

/// ========== 调试开关 ==========
/// 设置为 true 时会保存获取到的HTML到 /tmp 目录
/// 调试完成后请设置为 false
const DEBUG_SAVE_HTML: bool = false;
/// 调试HTML保存目录
const DEBUG_HTML_DIR: &str = "~/tmp";

#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub user_data_dir: Option<String>,
    pub proxy_server: Option<String>,
    pub headless: bool,
    pub user_agent: Option<String>,
    pub bypass_csp: bool,
    pub wait_selectors: Vec<String>,
    pub wait_timeout_ms: u64,
    pub wait_poll_ms: u64,
    /// Kagi 会话链接，仅在使用 Kagi 搜索引擎时生效
    /// 格式如：https://kagi.com/search?token=xxxxx
    pub kagi_session_url: Option<String>,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            user_data_dir: None,
            proxy_server: None,
            headless: true,
            user_agent: None,
            bypass_csp: false,
            wait_selectors: vec![],
            wait_timeout_ms: 15000,
            wait_poll_ms: 250,
            kagi_session_url: None,
        }
    }
}

pub struct ContentFetcher {
    app_handle: AppHandle,
    config: FetchConfig,
    fingerprint_manager: FingerprintManager,
    timing_config: TimingConfig,
}

impl ContentFetcher {
    fn is_timeout_like(error: &str) -> bool {
        let lower = error.to_lowercase();
        lower.contains("timeout") || lower.contains("timed out") || error.contains("超时")
    }

    pub fn new(app_handle: AppHandle, config: FetchConfig) -> Self {
        let app_data_dir = app_handle
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join("data"));

        let fingerprint_manager = FingerprintManager::new(&app_data_dir);
        let timing_config = FingerprintManager::get_timing_config();

        Self { app_handle, config, fingerprint_manager, timing_config }
    }

    /// 保存调试HTML到文件（仅在 DEBUG_SAVE_HTML 为 true 时生效）
    fn save_debug_html(html: &str, prefix: &str) {
        if !DEBUG_SAVE_HTML {
            return;
        }

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S%.3f");
        let filename = format!("{}_{}.html", prefix, timestamp);
        let filepath = PathBuf::from(DEBUG_HTML_DIR).join(&filename);

        // 确保目录存在
        if let Err(e) = fs::create_dir_all(DEBUG_HTML_DIR) {
            warn!(error = %e, dir = DEBUG_HTML_DIR, "Failed to create debug HTML directory");
            return;
        }

        match fs::write(&filepath, html) {
            Ok(_) => {
                info!(
                    path = %filepath.display(),
                    bytes = html.len(),
                    "🔍 Debug HTML saved"
                );
            }
            Err(e) => {
                warn!(error = %e, path = %filepath.display(), "Failed to save debug HTML");
            }
        }
    }

    /// 导航到URL并等待（带超时和元素等待）
    async fn goto_with_timeout(
        &self,
        page: &chromiumoxide::page::Page,
        url: &str,
        stage: &str,
    ) -> Result<(), String> {
        let timeout_ms = self.config.wait_timeout_ms.max(30_000);
        let timeout_duration = Duration::from_millis(timeout_ms);
        info!(%url, stage, timeout_ms, "Navigating with Chromium");

        match timeout(timeout_duration, page.goto(url)).await {
            Ok(Ok(_)) => {
                info!(%url, stage, "Navigation completed");
                Ok(())
            }
            Ok(Err(e)) => {
                let err = e.to_string();
                let lower = err.to_lowercase();
                if lower.contains("timeout") {
                    warn!(%url, stage, timeout_ms, error = %err, "Chromium navigation timeout");
                } else {
                    warn!(%url, stage, timeout_ms, error = %err, "Chromium navigation failed");
                }
                Err(format!("Chromium goto error ({}): {}", stage, err))
            }
            Err(_) => {
                let page_state = self.capture_page_state(page).await;
                warn!(%url, stage, timeout_ms, ?page_state, "Chromium navigation timeout");
                Err(format!(
                    "Chromium goto timeout ({}): {}ms, page_state={}",
                    stage, timeout_ms, page_state
                ))
            }
        }
    }

    /// 等待页面内容加载
    async fn wait_for_content(&self, page: &chromiumoxide::page::Page) -> Result<(), String> {
        if self.config.wait_selectors.is_empty() {
            sleep(Duration::from_millis(800)).await;
            return Ok(());
        }

        let start = Instant::now();
        let selectors_json =
            serde_json::to_string(&self.config.wait_selectors).unwrap_or("[]".to_string());
        let script = format!(
            "() => {{ const sels = {}; for (const s of sels) {{ if (document.querySelector(s)) return s; }} return null; }}",
            selectors_json
        );

        let mut matched: Option<String> = None;
        loop {
            if let Ok(val) = page.evaluate(script.as_str()).await {
                if let Some(Value::String(sel)) = val.value() {
                    matched = Some(sel.clone());
                    break;
                }
            }

            if start.elapsed() >= Duration::from_millis(self.config.wait_timeout_ms) {
                let current_url = page
                    .evaluate("() => window.location.href")
                    .await
                    .ok()
                    .and_then(|val| val.value().and_then(|v| v.as_str().map(|s| s.to_string())))
                    .unwrap_or_else(|| "unknown".to_string());
                warn!(
                    stage = "wait_for_content",
                    timeout_ms = self.config.wait_timeout_ms,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    %current_url,
                    selectors = ?self.config.wait_selectors,
                    "⚠️ Content wait timeout"
                );
                break;
            }

            sleep(Duration::from_millis(self.config.wait_poll_ms.max(250))).await;
        }

        if let Some(sel) = matched {
            debug!(selector = %sel, "Waited selector matched");
        }

        Ok(())
    }

    async fn capture_page_state(&self, page: &chromiumoxide::page::Page) -> Value {
        page.evaluate(
            "() => ({ url: window.location.href, title: document.title, readyState: document.readyState, bodyChildren: document.body ? document.body.children.length : 0, bodyLength: document.body ? document.body.innerHTML.length : 0 })",
        )
        .await
        .ok()
        .and_then(|val| val.value().cloned())
        .unwrap_or_default()
    }

    /// 应用指纹配置到页面级别
    async fn apply_fingerprint_overrides(
        &self,
        page: &chromiumoxide::page::Page,
        config: &FingerprintConfig,
    ) -> Result<(), String> {
        let ua_params = emulation::SetUserAgentOverrideParams::builder()
            .user_agent(config.user_agent.clone())
            .accept_language(config.accept_language.clone())
            .platform(config.platform.clone())
            .build()
            .map_err(|e| format!("Failed to build user agent override: {}", e))?;
        page.execute(ua_params).await.map_err(|e| format!("Failed to set user agent: {}", e))?;

        let metrics = emulation::SetDeviceMetricsOverrideParams::builder()
            .width(config.viewport_width as i64)
            .height(config.viewport_height as i64)
            .device_scale_factor(config.device_scale_factor)
            .mobile(config.is_mobile)
            .screen_width(config.screen_width as i64)
            .screen_height(config.screen_height as i64)
            .build()
            .map_err(|e| format!("Failed to build device metrics override: {}", e))?;
        page.execute(metrics).await.map_err(|e| format!("Failed to set device metrics: {}", e))?;

        let mut touch_builder =
            emulation::SetTouchEmulationEnabledParams::builder().enabled(config.has_touch);
        if config.has_touch {
            touch_builder = touch_builder.max_touch_points(5);
        }
        let touch_params = touch_builder
            .build()
            .map_err(|e| format!("Failed to build touch emulation params: {}", e))?;
        page.execute(touch_params)
            .await
            .map_err(|e| format!("Failed to set touch emulation: {}", e))?;

        if !config.timezone_id.is_empty() {
            page.execute(emulation::SetTimezoneOverrideParams::new(config.timezone_id.clone()))
                .await
                .map_err(|e| format!("Failed to set timezone override: {}", e))?;
        }

        if !config.locale.is_empty() {
            let locale_params =
                emulation::SetLocaleOverrideParams::builder().locale(config.locale.clone()).build();
            page.execute(locale_params)
                .await
                .map_err(|e| format!("Failed to set locale override: {}", e))?;
        }

        let color_scheme = match config.color_scheme.as_str() {
            "dark" => "dark",
            _ => "light",
        };
        let media_params = emulation::SetEmulatedMediaParams::builder()
            .feature(emulation::MediaFeature::new("prefers-color-scheme", color_scheme))
            .build();
        page.execute(media_params)
            .await
            .map_err(|e| format!("Failed to set emulated media: {}", e))?;

        if self.config.bypass_csp {
            page.execute(cdp_page::SetBypassCspParams::new(true))
                .await
                .map_err(|e| format!("Failed to bypass CSP: {}", e))?;
        }

        Ok(())
    }

    /// 在页面级别设置HTTP头
    async fn set_page_http_headers(
        &self,
        page: &chromiumoxide::page::Page,
        config: &FingerprintConfig,
    ) -> Result<(), String> {
        page.execute(network::EnableParams::default())
            .await
            .map_err(|e| format!("Failed to enable network domain: {}", e))?;

        let headers = json!({
            "Accept-Language": config.accept_language.clone(),
            "Sec-Ch-Ua-Platform": format!("\"{}\"", config.platform),
            "Sec-Ch-Ua-Mobile": if config.is_mobile { "?1" } else { "?0" },
            "Sec-Ch-Ua": "\"Not A(Brand\";v=\"99\", \"Google Chrome\";v=\"131\", \"Chromium\";v=\"131\"",
        });

        page.execute(network::SetExtraHttpHeadersParams::new(network::Headers::new(headers)))
            .await
            .map_err(|e| format!("Failed to set extra HTTP headers: {}", e))?;

        Ok(())
    }

    /// 注入反检测脚本（增强版）
    async fn inject_anti_detection_scripts(
        &self,
        page: &chromiumoxide::page::Page,
    ) -> Result<(), String> {
        let anti_detection_script = r#"
            // ========== 1. 核心webdriver检测绕过 ==========
            Object.defineProperty(navigator, 'webdriver', {
                get: () => undefined,
                configurable: true
            });

            // 删除可能存在的自动化标识
            delete navigator.__proto__.webdriver;

            // ========== 2. Chrome对象完整模拟 ==========
            if (!window.chrome) {
                window.chrome = {};
            }

            // ========== 3. 插件模拟 ==========
            const pluginData = [
                { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' }
            ];
            Object.defineProperty(navigator, 'plugins', {
                get: () => {
                    const arr = Object.create(PluginArray.prototype);
                    return arr;
                },
                configurable: true
            });

            // ========== 4. languages数组 ==========
            Object.defineProperty(navigator, 'languages', {
                get: () => ['zh-CN', 'zh', 'en-US', 'en'],
                configurable: true
            });

            // ========== 12. 自动化检测函数 ==========
            delete window.__playwright;
            delete window.__pw_manual;
            delete window.callPhantom;
            delete window._phantom;
            delete window.phantom;
            delete window.__nightmare;
            delete window.domAutomation;
            delete window.domAutomationController;

            console.log('[AIPP] Anti-detection scripts injected successfully');
        "#;

        match page.evaluate_on_new_document(anti_detection_script).await {
            Ok(_) => {
                info!("Anti-detection scripts injected");
            }
            Err(e) => {
                let error_message = e.to_string();
                let lower = error_message.to_lowercase();
                if lower.contains("timed out") || lower.contains("timeout") {
                    warn!(
                        error = %error_message,
                        "Anti-detection injection timed out, continuing without it"
                    );
                    return Ok(());
                }
                return Err(format!("Failed to inject anti-detection script: {}", error_message));
            }
        }
        Ok(())
    }

    /// 获取用户数据目录
    fn get_user_data_dir(&self) -> Result<PathBuf, String> {
        if let Some(ref custom_dir) = self.config.user_data_dir {
            Ok(PathBuf::from(custom_dir))
        } else {
            let base = self
                .app_handle
                .path()
                .app_data_dir()
                .map_err(|e| format!("Failed to get app data dir: {}", e))?;
            Ok(base.join("chromiumoxide_profile"))
        }
    }

    /// 检查代理是否可用（快速TCP连接测试）
    async fn check_proxy_available(proxy_url: &str) -> Result<(), String> {
        use std::net::ToSocketAddrs;

        // 解析代理URL获取主机和端口
        let url = proxy_url.trim();
        let url =
            url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or(url);
        let url = url.strip_prefix("socks5://").unwrap_or(url);

        // 移除可能的路径部分
        let host_port = url.split('/').next().unwrap_or(url);

        // 尝试解析地址
        let addr = host_port
            .to_socket_addrs()
            .map_err(|e| format!("Failed to resolve proxy address '{}': {}", host_port, e))?
            .next()
            .ok_or_else(|| format!("No address found for proxy: {}", host_port))?;

        // 尝试TCP连接，超时3秒
        let timeout = Duration::from_secs(3);
        match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(_stream)) => {
                debug!(proxy = %proxy_url, "Proxy is reachable");
                Ok(())
            }
            Ok(Err(e)) => Err(format!("Failed to connect to proxy {}: {}", proxy_url, e)),
            Err(_) => {
                Err(format!("Proxy connection timeout ({}s): {}", timeout.as_secs(), proxy_url))
            }
        }
    }

    /// 使用HTTP直接请求
    async fn fetch_with_http(&self, url: &str) -> Result<String, String> {
        let user_agent = self.config.user_agent.as_deref().unwrap_or(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
        );

        let mut client_builder = reqwest::Client::builder()
            .user_agent(user_agent)
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(Duration::from_millis(self.config.wait_timeout_ms));

        if let Some(ref proxy) = self.config.proxy_server {
            let proxy = reqwest::Proxy::all(proxy)
                .map_err(|e| format!("Invalid proxy configuration: {}", e))?;
            client_builder = client_builder.proxy(proxy);
        }

        let client =
            client_builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        let resp = client
            .get(url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await
            .map_err(|e| format!("HTTP request error: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP status {} when fetching {}", status.as_u16(), url));
        }

        let text = resp.text().await.map_err(|e| format!("Failed to read response body: {}", e))?;

        if text.trim().is_empty() {
            warn!(%url, status = status.as_u16(), "Empty response body");
            return Err("Empty response body".to_string());
        }

        Ok(text)
    }

    /// WebView兜底导航（不提取内容）
    async fn fallback_webview_navigation(&self, url: &str) -> Result<String, String> {
        if let Err(e) = crate::window::ensure_hidden_search_window(self.app_handle.clone()).await {
            warn!(error = %e, "Failed to create hidden search window");
        } else if let Some(window) = self.app_handle.get_webview_window("hidden_search") {
            let _ = window.navigate(url.parse().map_err(|e| format!("Invalid URL: {}", e))?);
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        Err("All fetch strategies failed; WebView navigation attempted but no content extracted"
            .to_string())
    }

    /// 主要的内容抓取方法，按优先级尝试不同策略
    pub async fn fetch_content(
        &mut self,
        url: &str,
        browser_manager: &BrowserManager,
        browser_pool: Option<&BrowserPool>,
    ) -> Result<String, String> {
        info!(%url, "Starting content fetch");

        // 策略1: Chromiumoxide（最优，支持复杂动态内容）
        match self.fetch_with_chromiumoxide(url, browser_manager, browser_pool).await {
            Ok(html) => {
                info!(strategy = "chromiumoxide", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    timeout_like = Self::is_timeout_like(&e),
                    strategy = "chromiumoxide",
                    "Fetch attempt failed"
                );
            }
        }

        // 策略2: Headless Browser（次优，轻量级）
        match self.fetch_with_headless_browser(url, browser_manager).await {
            Ok(html) => {
                info!(strategy = "headless", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    timeout_like = Self::is_timeout_like(&e),
                    strategy = "headless",
                    "Fetch attempt failed"
                );
            }
        }

        // 策略3: HTTP直接请求（兜底，适合静态内容）
        match self.fetch_with_http(url).await {
            Ok(html) => {
                info!(strategy = "http", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    timeout_like = Self::is_timeout_like(&e),
                    strategy = "http",
                    "Fetch attempt failed"
                );
            }
        }

        // 策略4: WebView兜底（不提取内容，仅导航）
        self.fallback_webview_navigation(url).await
    }

    /// 使用Chromiumoxide抓取内容
    async fn fetch_with_chromiumoxide(
        &mut self,
        url: &str,
        browser_manager: &BrowserManager,
        browser_pool: Option<&BrowserPool>,
    ) -> Result<String, String> {
        // 如果有浏览器池，使用池化页面
        if let Some(pool) = browser_pool {
            return self.fetch_with_pooled_page(url, pool).await;
        }

        let browser_path = browser_manager.get_browser_path()?;

        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let stealth_args = FingerprintManager::get_stealth_launch_args();

        use chromiumoxide::BrowserConfig;

        let mut builder = BrowserConfig::builder()
            .user_data_dir(&user_data_dir)
            .no_sandbox()
            .launch_timeout(Duration::from_millis(self.config.wait_timeout_ms.max(30_000)));

        if !self.config.headless {
            builder = builder.with_head();
        }

        let browser_path_exists = browser_path.exists();
        if browser_path_exists {
            builder = builder.chrome_executable(&browser_path);
        } else {
            warn!(path = %browser_path.display(), "Browser executable not found, using default path");
        }

        for arg in &stealth_args {
            builder = builder.arg(arg);
        }

        // 处理代理配置
        if let Some(ref proxy) = self.config.proxy_server {
            if !proxy.trim().is_empty() {
                info!(proxy = %proxy, "Checking proxy availability for fetch");
                match Self::check_proxy_available(proxy).await {
                    Ok(_) => {
                        let proxy_arg = format!("--proxy-server={}", proxy);
                        builder = builder.arg(&proxy_arg);
                        info!(proxy = %proxy, "✅ Proxy configured for fetch");
                    }
                    Err(e) => {
                        warn!(proxy = %proxy, error = %e, "⚠️ Proxy not available, continuing without proxy");
                    }
                }
            }
        }

        info!(
            url = %url,
            headless = self.config.headless,
            wait_timeout_ms = self.config.wait_timeout_ms,
            wait_poll_ms = self.config.wait_poll_ms,
            browser_path = %browser_path.display(),
            browser_path_exists = browser_path_exists,
            user_data_dir = %user_data_dir.display(),
            "Launching Chromiumoxide for fetch"
        );

        let config =
            builder.build().map_err(|e| format!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = chromiumoxide::browser::Browser::launch(config)
            .await
            .map_err(|e| {
                format!(
                    "Failed to launch browser (path={}, exists={}, headless={}, user_data_dir={}): {}",
                    browser_path.display(),
                    browser_path_exists,
                    self.config.headless,
                    user_data_dir.display(),
                    e
                )
            })?;

        // 启动事件处理器
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                trace!(?event, "Chromium event received");
            }
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        let fingerprint = self.fingerprint_manager.get_stable_fingerprint(None).clone();

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 应用指纹配置和HTTP头
        self.apply_fingerprint_overrides(&page, &fingerprint).await?;
        self.set_page_http_headers(&page, &fingerprint).await?;

        self.goto_with_timeout(&page, url, "fetch_content").await?;

        // 等待页面加载完成
        self.wait_for_content(&page).await?;

        // 获取 HTML
        let html =
            page.content().await.map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            let page_state = self.capture_page_state(&page).await;
            warn!(
                stage = "fetch_content",
                %url,
                bytes = html.len(),
                ?page_state,
                "Empty HTML from Chromiumoxide"
            );
            return Err("Empty HTML from Chromiumoxide".to_string());
        }

        Ok(html)
    }

    /// 使用浏览器池抓取URL内容
    async fn fetch_with_pooled_page(
        &mut self,
        url: &str,
        pool: &BrowserPool,
    ) -> Result<String, String> {
        let mut pooled_page = pool.acquire_page().await?;
        let page = pooled_page.page();

        let fingerprint = self.fingerprint_manager.get_stable_fingerprint(None).clone();

        // 注入反检测脚本
        self.inject_anti_detection_scripts(page).await?;

        // 应用指纹配置和HTTP头
        self.apply_fingerprint_overrides(page, &fingerprint).await?;
        self.set_page_http_headers(page, &fingerprint).await?;

        // 导航到 URL
        self.goto_with_timeout(page, url, "fetch_content_pooled").await?;

        // 等待页面加载完成
        self.wait_for_content(page).await?;

        // 获取 HTML
        let html =
            page.content().await.map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            let page_state = self.capture_page_state(page).await;
            warn!(
                stage = "fetch_content_pooled",
                %url,
                bytes = html.len(),
                ?page_state,
                "Empty HTML from Chromiumoxide (pooled)"
            );
            return Err("Empty HTML from Chromiumoxide (pooled)".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched content (pooled)");

        // pooled_page 离开作用域时自动归还到池中
        Ok(html)
    }

    /// 使用系统浏览器headless模式抓取
    async fn fetch_with_headless_browser(
        &self,
        url: &str,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        let browser_path = browser_manager.get_browser_path()?;
        debug!(path = %browser_path.display(), "Headless fetch using browser");

        let mut cmd = TokioCommand::new(browser_path);

        let user_agent = self.config.user_agent.as_deref().unwrap_or(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
        );

        cmd.arg("--headless=new")
            .arg("--disable-gpu")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-extensions")
            .arg("--disable-blink-features=AutomationControlled")
            .arg("--virtual-time-budget=15000")
            .arg(format!("--timeout={}", self.config.wait_timeout_ms * 3))
            .arg("--hide-scrollbars")
            .arg("--window-size=1280,800")
            .arg("--dump-dom")
            .arg(format!("--user-agent={}", user_agent))
            .arg(url);

        let output =
            cmd.output().await.map_err(|e| format!("Failed to run headless browser: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Headless browser failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if stdout.trim().is_empty() {
            warn!(%url, "Empty DOM output from headless browser");
            return Err("Empty DOM output from headless browser".to_string());
        }

        Ok(stdout)
    }

    /// 等待搜索结果，使用指定的选择器列表
    async fn wait_for_results_with_selectors(
        &self,
        page: &chromiumoxide::page::Page,
        selectors: &[String],
    ) -> Result<(), String> {
        let start = Instant::now();
        let timeout = Duration::from_millis(self.config.wait_timeout_ms);
        let selectors_json = serde_json::to_string(selectors).unwrap_or("[]".to_string());

        let mut check_count = 0;
        loop {
            check_count += 1;

            let found_selector_script = format!(
                "() => {{ const sels = {}; for (const s of sels) {{ if (document.querySelector(s)) return s; }} return null; }}",
                selectors_json
            );

            let found = page
                .evaluate(found_selector_script.as_str())
                .await
                .ok()
                .and_then(|val| val.value().and_then(|v| v.as_str().map(|s| s.to_string())));

            if let Some(sel) = found {
                info!(
                    selector = %sel,
                    check_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "✅ Results loaded"
                );
                let extra_wait = 500 + rand::random::<u64>() % 500;
                sleep(Duration::from_millis(extra_wait)).await;
                return Ok(());
            }

            if start.elapsed() >= timeout {
                let current_url = page
                    .evaluate("() => window.location.href")
                    .await
                    .ok()
                    .and_then(|val| val.value().and_then(|v| v.as_str().map(|s| s.to_string())))
                    .unwrap_or_else(|| "unknown".to_string());
                warn!(
                    stage = "wait_for_results_selectors",
                    timeout_ms = self.config.wait_timeout_ms,
                    check_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    %current_url,
                    "Results wait timeout, continuing anyway"
                );
                break;
            }

            sleep(Duration::from_millis(self.config.wait_poll_ms.max(250))).await;
        }

        Ok(())
    }

    /// 执行人性化的搜索流程（模拟真实用户行为）
    async fn perform_humanized_search(
        &mut self,
        page: &chromiumoxide::page::Page,
        query: &str,
        search_engine: &SearchEngine,
    ) -> Result<String, String> {
        info!(%query, engine = search_engine.as_str(), "Starting humanized search");

        let action_range =
            self.timing_config.action_delay_max.saturating_sub(self.timing_config.action_delay_min);
        let initial_delay = self.timing_config.action_delay_min
            + if action_range > 0 { rand::random::<u64>() % action_range } else { 0 };
        sleep(Duration::from_millis(initial_delay)).await;

        // 带重试的导航到搜索引擎首页
        let homepage_url = search_engine.homepage_url();
        let navigate_stage_start = Instant::now();
        self.navigate_with_retry(page, homepage_url).await.map_err(|e| {
            let timeout_like = Self::is_timeout_like(&e);
            error!(
                stage = "navigate_homepage",
                engine = search_engine.as_str(),
                timeout_like,
                elapsed_ms = navigate_stage_start.elapsed().as_millis() as u64,
                error = %e,
                "Search stage failed"
            );
            format!("Search stage navigate_homepage failed: {}", e)
        })?;

        // 等待页面稳定
        sleep(Duration::from_millis(500 + rand::random::<u64>() % 500)).await;

        // 人性化的输入框定位和填写
        let input_stage_start = Instant::now();
        self.humanized_search_input(page, query, search_engine).await.map_err(|e| {
            let timeout_like = Self::is_timeout_like(&e);
            error!(
                stage = "fill_search_input",
                engine = search_engine.as_str(),
                timeout_like,
                elapsed_ms = input_stage_start.elapsed().as_millis() as u64,
                error = %e,
                "Search stage failed"
            );
            format!("Search stage fill_search_input failed: {}", e)
        })?;

        // 人性化的搜索触发
        let submit_stage_start = Instant::now();
        self.humanized_search_submit(page, search_engine).await.map_err(|e| {
            let timeout_like = Self::is_timeout_like(&e);
            error!(
                stage = "submit_search",
                engine = search_engine.as_str(),
                timeout_like,
                elapsed_ms = submit_stage_start.elapsed().as_millis() as u64,
                error = %e,
                "Search stage failed"
            );
            format!("Search stage submit_search failed: {}", e)
        })?;

        // 等待结果加载，使用配置的等待时间加随机延时
        let wait_time = self.config.wait_timeout_ms + rand::random::<u64>() % 2000;
        let wait_stage_start = Instant::now();
        self.wait_for_results_with_timeout(page, wait_time, search_engine).await.map_err(|e| {
            let timeout_like = Self::is_timeout_like(&e);
            error!(
                stage = "wait_search_results",
                engine = search_engine.as_str(),
                timeout_like,
                elapsed_ms = wait_stage_start.elapsed().as_millis() as u64,
                error = %e,
                "Search stage failed"
            );
            format!("Search stage wait_search_results failed: {}", e)
        })?;

        // 增强的HTML提取，带重试机制
        let extract_stage_start = Instant::now();
        let html = self.extract_page_html_with_retry(page).await.map_err(|e| {
            let timeout_like = Self::is_timeout_like(&e);
            error!(
                stage = "extract_html",
                engine = search_engine.as_str(),
                timeout_like,
                elapsed_ms = extract_stage_start.elapsed().as_millis() as u64,
                error = %e,
                "Search stage failed"
            );
            format!("Search stage extract_html failed: {}", e)
        })?;

        debug!("Successfully retrieved {} bytes", html.len());

        // 保存调试HTML
        Self::save_debug_html(&html, "search_result");

        Ok(html)
    }

    /// 带重试机制的HTML提取
    async fn extract_page_html_with_retry(
        &self,
        page: &chromiumoxide::page::Page,
    ) -> Result<String, String> {
        let max_retries = 3;
        let mut last_error = String::new();
        let mut last_html: Option<String> = None;

        for attempt in 1..=max_retries {
            info!(attempt, max_retries, "Attempting HTML extraction");

            sleep(Duration::from_millis(1000 + rand::random::<u64>() % 1000)).await;

            let current_url = page
                .evaluate("() => window.location.href")
                .await
                .ok()
                .and_then(|val| val.value().and_then(|v| v.as_str().map(|s| s.to_string())))
                .unwrap_or_else(|| "unknown".to_string());
            debug!(attempt, %current_url, "Current page URL");

            match self.check_page_ready(page).await {
                Ok(true) => {
                    info!(attempt, "Page ready check passed");
                    match page.evaluate("() => document.documentElement.outerHTML").await {
                        Ok(val) => {
                            let html_str =
                                val.value().and_then(|v| v.as_str()).unwrap_or("").to_string();
                            last_html = Some(html_str.clone());

                            if html_str.len() > 1000 {
                                info!(
                                    attempt,
                                    bytes = html_str.len(),
                                    "HTML extraction successful"
                                );
                                return Ok(html_str);
                            } else {
                                last_error = format!("HTML too short ({} bytes)", html_str.len());
                                warn!(len = html_str.len(), attempt, "HTML too short, retrying");
                                Self::save_debug_html(
                                    &html_str,
                                    &format!("short_html_attempt{}", attempt),
                                );
                            }
                        }
                        Err(e) => {
                            last_error = format!("HTML extraction error: {}", e);
                            warn!(error = %e, attempt, "HTML extraction failed");
                        }
                    }
                }
                Ok(false) => {
                    last_error = "Page not ready".to_string();
                    warn!(attempt, "Page not ready, waiting");

                    let page_info = page
                        .evaluate(
                            "() => ({ readyState: document.readyState, bodyChildren: document.body ? document.body.children.length : 0, title: document.title })",
                        )
                        .await
                        .ok()
                        .and_then(|val| val.value().cloned())
                        .unwrap_or_default();
                    debug!(attempt, ?page_info, "Page state info");
                }
                Err(e) => {
                    last_error = format!("Page check error: {}", e);
                    warn!(error = %e, attempt, "Page check error");
                }
            }

            if attempt < max_retries {
                sleep(Duration::from_millis(2000)).await;
            }
        }

        if let Some(html) = last_html {
            Self::save_debug_html(&html, "failed_extraction");
        }

        Err(format!(
            "Failed to extract HTML after {} attempts. Last error: {}",
            max_retries, last_error
        ))
    }

    /// 检查页面是否准备就绪
    async fn check_page_ready(&self, page: &chromiumoxide::page::Page) -> Result<bool, String> {
        let body_ready = page
            .evaluate("() => !!document.body && document.body.children.length > 0")
            .await
            .ok()
            .and_then(|val| val.value().and_then(|v| v.as_bool()))
            .unwrap_or(false);

        if !body_ready {
            return Ok(false);
        }

        let has_content = page
            .evaluate(
                "() => {
                const indicators = [
                    '#b_content', '#b_results', '.b_algo',
                    '#search', '#main', '.g', '.tF2Cxc',
                    '#results', '.result', '.web-result'
                ];
                return indicators.some(sel => document.querySelector(sel));
            }",
            )
            .await
            .ok()
            .and_then(|val| val.value().and_then(|v| v.as_bool()))
            .unwrap_or(false);

        Ok(has_content)
    }

    /// 带重试机制的页面导航
    async fn navigate_with_retry(
        &self,
        page: &chromiumoxide::page::Page,
        url: &str,
    ) -> Result<(), String> {
        let max_retries = 3;
        let mut last_error = String::new();

        for attempt in 1..=max_retries {
            debug!(attempt, max_retries, %url, "Attempting navigation");

            let stage = format!("navigate_with_retry_attempt_{}", attempt);
            match self.goto_with_timeout(page, url, &stage).await {
                Ok(_) => {
                    info!(attempt, "Navigation successful");

                    sleep(Duration::from_millis(1000)).await;

                    let page_loaded = page
                        .evaluate("() => document.readyState === 'complete' && !!document.body")
                        .await
                        .ok()
                        .and_then(|val| val.value().and_then(|v| v.as_bool()))
                        .unwrap_or(false);

                    if page_loaded {
                        return Ok(());
                    } else {
                        last_error = "Page did not load completely".to_string();
                        debug!("Page not fully loaded, retrying");
                    }
                }
                Err(e) => {
                    last_error = format!("Navigation error: {}", e);
                    debug!(error = %last_error, attempt, "Navigation failed");

                    if last_error.contains("ERR_CONNECTION_CLOSED")
                        || last_error.contains("ERR_NETWORK_CHANGED")
                    {
                        warn!("Network connection issue detected, waiting longer before retry");
                        sleep(Duration::from_millis(5000)).await;
                    }
                }
            }

            if attempt < max_retries {
                let wait_time = 2000 * attempt as u64;
                sleep(Duration::from_millis(wait_time)).await;
            }
        }

        Err(format!(
            "Failed to navigate to {} after {} attempts. Last error: {}",
            url, max_retries, last_error
        ))
    }

    /// 人性化的搜索输入处理
    async fn humanized_search_input(
        &self,
        page: &chromiumoxide::page::Page,
        query: &str,
        search_engine: &SearchEngine,
    ) -> Result<(), String> {
        fn build_activation_script(selector: &str) -> String {
            format!(
                r#"() => {{
    const el = document.querySelector('{sel}');
    if (!el) return {{ success: false, method: 'not_found' }};
    try {{
        if ('value' in el) el.value = '';
        el.focus();
        try {{ el.click(); }} catch(_e) {{}}
        return {{ success: true, method: 'activated' }};
    }} catch (e) {{
        return {{ success: false, method: 'exception', error: String(e), stack: (e && e.stack) ? String(e.stack) : 'no_stack' }};
    }}
}}"#,
                sel = selector.replace("'", "\\'")
            )
        }

        fn build_input_script(selector: &str, value: &str) -> String {
            format!(
                r#"() => {{
    const el = document.querySelector('{sel}');
    if (!el) return {{ success: false, reason: 'element_not_found' }};
    try {{
        el.focus();
        const v = '{val}';
        if ('value' in el) el.value = v;
        if ('textContent' in el) el.textContent = v;
        el.dispatchEvent(new Event('input', {{ bubbles: true }}));
        el.dispatchEvent(new Event('change', {{ bubbles: true }}));
        return {{ success: true, value: ('value' in el ? el.value : (el.textContent||'')) }};
    }} catch(e) {{
        return {{ success: false, reason: String(e) }};
    }}
}}"#,
                sel = selector.replace("'", "\\'"),
                val = value.replace("'", "\\'").replace('"', "\\\"")
            )
        }

        let consent_dismiss_scripts = [
            r#"() => { 
                const btns = document.querySelectorAll('button[jsname], div[role="button"][jsname]');
                for (const b of btns) {
                    const t = (b.textContent || '').toLowerCase();
                    if (t.includes('accept all') || t.includes('接受全部') || t.includes('全部接受') || t.includes('同意')) {
                        b.click();
                        return { dismissed: true, text: t };
                    }
                }
                return { dismissed: false };
            }"#,
            r#"() => { 
                const btns = Array.from(document.querySelectorAll('button, div[role=button]')); 
                const patterns = [/同意/, /接受全部/, /全部接受/, /Allow all/i, /Accept all/i, /I agree/i, /Akzeptieren/i, /Accepter/i, /Aceptar/i];
                for (const b of btns) { 
                    const t = b.textContent || ''; 
                    if (patterns.some(p => p.test(t))) { 
                        b.click(); 
                        return { dismissed: true, text: t }; 
                    } 
                } 
                return { dismissed: false }; 
            }"#,
            r#"() => { 
                const btns = Array.from(document.querySelectorAll('button, div[role=button]')); 
                const patterns = [/拒绝/, /拒絕/, /Reject all/i, /Decline/i, /Ablehnen/i, /Refuser/i];
                for (const b of btns) { 
                    const t = b.textContent || ''; 
                    if (patterns.some(p => p.test(t))) { 
                        b.click(); 
                        return { dismissed: true, text: t }; 
                    } 
                } 
                return { dismissed: false }; 
            }"#,
            r#"() => { 
                const dlg = document.querySelector('form[action*=consent], div[role=dialog], [class*=consent], [id*=consent]'); 
                if (!dlg) return { dismissed: false, reason: 'no_dialog' }; 
                const btn = dlg.querySelector('button, div[role=button], input[type=submit]'); 
                if (btn) { 
                    btn.click(); 
                    return { dismissed: true, method: 'dialog_button' }; 
                } 
                return { dismissed: false, reason: 'no_button_in_dialog' }; 
            }"#,
            r#"() => {
                const btn = document.querySelector('#bnp_btn_accept, .bnp_btn_accept, button[id*="accept"]');
                if (btn) {
                    btn.click();
                    return { dismissed: true, method: 'bing_accept' };
                }
                return { dismissed: false };
            }"#,
        ];
        info!("Checking for consent/cookie dialogs");

        sleep(Duration::from_millis(1500)).await;

        if DEBUG_SAVE_HTML {
            if let Ok(val) = page.evaluate("() => document.documentElement.outerHTML").await {
                if let Some(Value::String(html)) = val.value() {
                    Self::save_debug_html(html, "before_consent");
                }
            }
        }

        for (idx, script) in consent_dismiss_scripts.iter().enumerate() {
            let result = page
                .evaluate(*script)
                .await
                .ok()
                .and_then(|val| val.value().cloned())
                .unwrap_or_default();
            let dismissed = result.get("dismissed").and_then(|v| v.as_bool()).unwrap_or(false);
            info!(script_index = idx + 1, dismissed, ?result, "Consent script executed");
            if dismissed {
                info!("✅ Dismissed a consent/cookie dialog");
                sleep(Duration::from_millis(800)).await;
                break;
            }
        }

        let selectors = search_engine.search_input_selectors();
        debug!(count = selectors.len(), ?selectors, "Trying selectors for search input");

        for (idx, selector) in selectors.iter().enumerate() {
            debug!(index = idx + 1, total = selectors.len(), selector = %selector, "Trying selector");
            let element_info = page
                .evaluate(format!(
                    "() => {{
                        const el = document.querySelector('{}');
                        if (!el) return {{ exists: false, visible: false, disabled: false }};
                        return {{
                            exists: true,
                            visible: el.offsetParent !== null,
                            disabled: el.disabled || false,
                            tagName: el.tagName,
                            type: el.type || 'none',
                            name: el.name || 'none'
                        }};
                    }}",
                    selector.replace("'", "\\'")
                ))
                .await;

            match element_info {
                Ok(info) => {
                    let info_obj = info.value().cloned().unwrap_or_default();
                    trace!(selector = %selector, ?info_obj, "Element info");
                    let exists = info_obj.get("exists").and_then(|v| v.as_bool()).unwrap_or(false);
                    let visible =
                        info_obj.get("visible").and_then(|v| v.as_bool()).unwrap_or(false);
                    let disabled =
                        info_obj.get("disabled").and_then(|v| v.as_bool()).unwrap_or(true);

                    if !exists {
                        debug!(selector = %selector, "Element not found");
                        continue;
                    }
                    if !visible {
                        debug!(selector = %selector, "Element not visible");
                        continue;
                    }
                    if disabled {
                        debug!(selector = %selector, "Element disabled");
                        continue;
                    }
                }
                Err(e) => {
                    debug!(selector = %selector, error = %e, "Failed to check element");
                    continue;
                }
            }

            debug!(selector = %selector, "Found valid element");
            sleep(Duration::from_millis(100 + rand::random::<u64>() % 200)).await;

            let activation_script = build_activation_script(selector);
            trace!(%activation_script, "Activation script");
            let result = match page.evaluate(activation_script).await {
                Ok(v) => v.value().cloned().unwrap_or_default(),
                Err(e) => {
                    debug!(selector = %selector, error = %e, "Activation eval error");
                    json!({"success": false, "method": "eval_error", "error": e.to_string()})
                }
            };

            let success = result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            trace!(selector = %selector, ?result, "Activation result");

            if !success {
                debug!(selector = %selector, ?result, "Failed to activate element");
                continue;
            }

            sleep(Duration::from_millis(150 + rand::random::<u64>() % 200)).await;

            let input_script = build_input_script(selector, query);
            trace!(%input_script, "Input script");
            let input_result = match page.evaluate(input_script).await {
                Ok(v) => v.value().cloned().unwrap_or_default(),
                Err(e) => {
                    debug!(selector = %selector, error = %e, "Input eval error");
                    json!({"success": false, "reason": "eval_error", "error": e.to_string()})
                }
            };

            let input_success =
                input_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            trace!(selector = %selector, ?input_result, "Input result");

            if input_success {
                info!(selector = %selector, "Successfully filled search input");
                return Ok(());
            } else {
                debug!(selector = %selector, "Input failed, trying next selector");
                continue;
            }
        }

        warn!("All direct selectors failed, attempting fallback candidate strategy");
        let fallback_script = format!(
            r#"() => {{
                    const candSelectors = [
                        'textarea[name=\"q\"]','input[name=\"q\"]','textarea.gLFyf','input.gLFyf','#APjFqb',
                        'form[role=\"search\"] textarea','form[role=\"search\"] input[type=\"text\"]','form[role=\"search\"] input[type=\"search\"]'
                    ];
                    const cands = candSelectors.flatMap(sel => Array.from(document.querySelectorAll(sel)));
                    const dedup = Array.from(new Set(cands));
                    const visible = dedup.filter(el => el && el.offsetParent !== null && !el.disabled);
                    const target = visible[0] || dedup[0];
                    if(!target) return {{ success:false, stage:'fallback', reason:'no_candidates' }};
                    try {{ target.focus(); }} catch(e) {{}}
                    try {{ target.click(); }} catch(e) {{}}
                    try {{ if('value' in target) target.value = '{val}'; }} catch(e) {{}}
                    try {{ target.dispatchEvent(new Event('input', {{ bubbles:true }})); }} catch(e) {{}}
                    try {{ target.dispatchEvent(new Event('change', {{ bubbles:true }})); }} catch(e) {{}}
                    return {{ success:true, stage:'fallback', tag: target.tagName, id: target.id||'', name: target.name||'', className: target.className||'', value: target.value || target.textContent || '' }};
                }}"#,
            val = query.replace("'", "\\'").replace('"', "\\\"")
        );
        trace!(%fallback_script, "Fallback fill script");
        let fb_res = page
            .evaluate(fallback_script)
            .await
            .ok()
            .and_then(|val| val.value().cloned())
            .unwrap_or_else(
                || json!({"success":false, "stage":"fallback", "error": "eval_failed"}),
            );
        trace!(?fb_res, "Fallback fill result");
        if fb_res.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            info!("Fallback candidate strategy succeeded");
            return Ok(());
        }

        warn!("All selectors failed, dumping page info");

        let page_info = page
            .evaluate(
                "() => ({ 
            url: window.location.href, 
            title: document.title,
            readyState: document.readyState,
            bodyExists: !!document.body,
            inputCount: document.querySelectorAll('input').length,
            textareaCount: document.querySelectorAll('textarea').length,
            formCount: document.querySelectorAll('form').length
        })",
            )
            .await
            .ok()
            .and_then(|val| val.value().cloned())
            .unwrap_or_default();
        debug!(?page_info, "Page info");

        let input_elements = page
            .evaluate(
                "() => {
            const inputs = Array.from(document.querySelectorAll('input, textarea'));
            return inputs.slice(0, 10).map(el => ({
                tagName: el.tagName,
                type: el.type || 'none',
                name: el.name || 'none',
                id: el.id || 'none',
                className: el.className || 'none',
                placeholder: el.placeholder || 'none',
                visible: el.offsetParent !== null,
                disabled: el.disabled
            }));
        }",
            )
            .await
            .ok()
            .and_then(|val| val.value().cloned())
            .unwrap_or_default();
        warn!(?input_elements, "Found input elements (none worked)");

        if let Ok(val) = page.evaluate("() => document.documentElement.outerHTML").await {
            if let Some(Value::String(html)) = val.value() {
                Self::save_debug_html(html, "input_failed");
            }
        }

        Err("Could not find or fill any search input".to_string())
    }

    /// 人性化的搜索提交
    async fn humanized_search_submit(
        &self,
        page: &chromiumoxide::page::Page,
        search_engine: &SearchEngine,
    ) -> Result<(), String> {
        let think_delay = 300 + rand::random::<u64>() % 700;
        sleep(Duration::from_millis(think_delay)).await;

        let button_selectors = search_engine.search_button_selectors();
        for selector in button_selectors {
            let button_script = format!(
                "() => {{
                    const btn = document.querySelector('{}');
                    if (btn && btn.offsetParent !== null && !btn.disabled) {{
                        btn.click();
                        return true;
                    }}
                    return false;
                }}",
                selector.replace("'", "\\'")
            );

            let clicked = page
                .evaluate(button_script)
                .await
                .ok()
                .and_then(|val| val.value().and_then(|v| v.as_bool()))
                .unwrap_or(false);
            if clicked {
                info!(selector = %selector, "Clicked search button");
                return Ok(());
            }
        }

        let input_selectors = search_engine.search_input_selectors();
        for selector in input_selectors {
            let enter_script = format!(
                r#"() => {{
      const el = document.querySelector('{sel}');
      if(!el) return false;
      const evt = new KeyboardEvent('keydown', {{ key:'Enter', code:'Enter', keyCode:13, which:13, bubbles:true }});
      el.dispatchEvent(evt);
      return true;
    }}"#,
                sel = selector.replace("'", "\\'")
            );

            let pressed = page
                .evaluate(enter_script)
                .await
                .ok()
                .and_then(|val| val.value().and_then(|v| v.as_bool()))
                .unwrap_or(false);
            if pressed {
                info!(selector = %selector, "Pressed Enter on input");
                return Ok(());
            }
        }

        Err("Failed to submit search".to_string())
    }

    /// 等待搜索结果，带超时处理
    async fn wait_for_results_with_timeout(
        &self,
        page: &chromiumoxide::page::Page,
        timeout_ms: u64,
        search_engine: &SearchEngine,
    ) -> Result<(), String> {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        let selectors = search_engine.default_wait_selectors();
        let selectors_json = serde_json::to_string(&selectors).unwrap_or("[]".to_string());

        info!(
            engine = search_engine.as_str(),
            timeout_ms,
            selectors = ?selectors,
            "Waiting for search results"
        );

        let mut check_count = 0;
        loop {
            check_count += 1;

            let current_url = page
                .evaluate("() => window.location.href")
                .await
                .ok()
                .and_then(|val| val.value().and_then(|v| v.as_str().map(|s| s.to_string())))
                .unwrap_or_else(|| "unknown".to_string());

            let found_selector_script = format!(
                "() => {{ const sels = {}; for (const s of sels) {{ if (document.querySelector(s)) return s; }} return null; }}",
                selectors_json
            );

            let found = page
                .evaluate(found_selector_script.as_str())
                .await
                .ok()
                .and_then(|val| val.value().and_then(|v| v.as_str().map(|s| s.to_string())));

            if let Some(sel) = found {
                info!(
                    selector = %sel,
                    check_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    %current_url,
                    "✅ Results loaded"
                );
                let extra_wait = 500 + rand::random::<u64>() % 500;
                sleep(Duration::from_millis(extra_wait)).await;
                return Ok(());
            }

            if start.elapsed() >= timeout {
                let page_state = page
                    .evaluate(
                        "() => ({ url: window.location.href, title: document.title, readyState: document.readyState, bodyLength: document.body ? document.body.innerHTML.length : 0 })",
                    )
                    .await
                    .ok()
                    .and_then(|val| val.value().cloned())
                    .unwrap_or_default();

                warn!(
                    stage = "wait_for_results_timeout",
                    timeout_ms,
                    check_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    ?page_state,
                    "Search results wait timeout"
                );
                break;
            }

            sleep(Duration::from_millis(self.config.wait_poll_ms.max(250))).await;
        }

        Ok(())
    }

    /// 使用 Chromiumoxide 执行搜索流程
    async fn fetch_search_with_chromiumoxide(
        &mut self,
        query: &str,
        search_engine: &SearchEngine,
        browser_manager: &BrowserManager,
        browser_pool: Option<&BrowserPool>,
    ) -> Result<String, String> {
        info!(%query, engine = ?search_engine, "Starting Chromium search");

        // 如果有浏览器池，使用池化页面
        if let Some(pool) = browser_pool {
            return self.fetch_search_with_pooled_page(query, search_engine, pool).await;
        }

        // 使用新建浏览器执行搜索
        let browser_path = browser_manager.get_browser_path()?;
        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let stealth_args = FingerprintManager::get_stealth_launch_args();

        use chromiumoxide::BrowserConfig;

        let mut builder = BrowserConfig::builder()
            .user_data_dir(&user_data_dir)
            .no_sandbox()
            .launch_timeout(Duration::from_millis(self.config.wait_timeout_ms.max(30_000)));

        if !self.config.headless {
            builder = builder.with_head();
        }

        if browser_path.exists() {
            builder = builder.chrome_executable(&browser_path);
        }

        for arg in &stealth_args {
            builder = builder.arg(arg);
        }

        // 处理代理配置
        if let Some(ref proxy) = self.config.proxy_server {
            if !proxy.trim().is_empty() {
                info!(proxy = %proxy, "Checking proxy availability for search");
                match Self::check_proxy_available(proxy).await {
                    Ok(_) => {
                        let proxy_arg = format!("--proxy-server={}", proxy);
                        builder = builder.arg(&proxy_arg);
                        info!(proxy = %proxy, "✅ Proxy configured for search");
                    }
                    Err(e) => {
                        warn!(proxy = %proxy, error = %e, "⚠️ Proxy not available, continuing without proxy");
                    }
                }
            }
        }

        let config =
            builder.build().map_err(|e| format!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = chromiumoxide::browser::Browser::launch(config)
            .await
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        // 启动事件处理器
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                trace!(?event, "Chromium event received");
            }
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        let fingerprint = self.fingerprint_manager.get_stable_fingerprint(None).clone();

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 应用指纹配置和HTTP头
        self.apply_fingerprint_overrides(&page, &fingerprint).await?;
        self.set_page_http_headers(&page, &fingerprint).await?;

        // 执行搜索流程（使用人性化的延时）
        let html = self.perform_humanized_search(&page, query, search_engine).await?;

        if html.trim().is_empty() {
            warn!(
                stage = "search_flow",
                engine = search_engine.as_str(),
                %query,
                bytes = html.len(),
                "Empty HTML from search flow"
            );
            return Err("Empty HTML from search flow".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched search content");

        Ok(html)
    }

    /// 使用浏览器池执行搜索流程
    async fn fetch_search_with_pooled_page(
        &mut self,
        query: &str,
        search_engine: &SearchEngine,
        pool: &BrowserPool,
    ) -> Result<String, String> {
        let mut pooled_page = pool.acquire_page().await?;
        let page = pooled_page.page();

        let fingerprint = self.fingerprint_manager.get_stable_fingerprint(None).clone();

        // 注入反检测脚本
        self.inject_anti_detection_scripts(page).await?;

        // 应用指纹配置和HTTP头
        self.apply_fingerprint_overrides(page, &fingerprint).await?;
        self.set_page_http_headers(page, &fingerprint).await?;

        // 执行搜索流程（使用人性化的延时）
        let html = self.perform_humanized_search(page, query, search_engine).await?;

        if html.trim().is_empty() {
            warn!(
                stage = "search_flow_pooled",
                engine = search_engine.as_str(),
                %query,
                bytes = html.len(),
                "Empty HTML from search flow (pooled)"
            );
            return Err("Empty HTML from search flow (pooled)".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched search content (pooled)");

        // pooled_page 离开作用域时自动归还到池中
        Ok(html)
    }

    /// 为搜索请求定制的获取方法
    pub async fn fetch_search_content(
        &mut self,
        query: &str,
        search_engine: &SearchEngine,
        browser_manager: &BrowserManager,
        browser_pool: Option<&BrowserPool>,
    ) -> Result<String, String> {
        info!(%query, engine = ?search_engine, "Starting search content fetch");

        // 如果是 Kagi 且配置了会话链接，使用直接 URL 方式搜索
        if *search_engine == SearchEngine::Kagi {
            if let Some(session_url) = self.config.kagi_session_url.clone() {
                info!("Using Kagi session URL for direct search");
                return self
                    .fetch_kagi_with_session_url(query, &session_url, browser_manager, browser_pool)
                    .await;
            }
        }

        // 使用 Chromiumoxide 执行搜索流程
        match self
            .fetch_search_with_chromiumoxide(query, search_engine, browser_manager, browser_pool)
            .await
        {
            Ok(html) => {
                info!(
                    strategy = "chromiumoxide_search",
                    bytes = html.len(),
                    "Fetched search content"
                );
                return Ok(html);
            }
            Err(e) => {
                let timeout_like = Self::is_timeout_like(&e);
                error!(
                    query = %query,
                    engine = search_engine.as_str(),
                    timeout_like,
                    error = %e,
                    "Search flow failed"
                );
                Err(format!(
                    "Search flow failed for {} engine (timeout_like={}): {}",
                    search_engine.display_name(),
                    timeout_like,
                    e
                ))
            }
        }
    }

    /// 使用 Kagi 会话链接直接搜索
    async fn fetch_kagi_with_session_url(
        &mut self,
        query: &str,
        session_url: &str,
        browser_manager: &BrowserManager,
        browser_pool: Option<&BrowserPool>,
    ) -> Result<String, String> {
        // 构造搜索 URL：在会话链接后面拼接 &q=搜索词
        let encoded_query = urlencoding::encode(query);
        let search_url = if session_url.contains('?') {
            format!("{}&q={}", session_url, encoded_query)
        } else {
            format!("{}?q={}", session_url, encoded_query)
        };

        info!(%search_url, "Fetching Kagi search results with session URL");

        // 如果有浏览器池，使用池化页面
        if let Some(pool) = browser_pool {
            return self.fetch_kagi_with_session_url_pooled(&search_url, pool).await;
        }

        // 使用Chromiumoxide直接访问搜索结果页面
        let browser_path = browser_manager.get_browser_path()?;

        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let stealth_args = FingerprintManager::get_stealth_launch_args();

        use chromiumoxide::BrowserConfig;

        let mut builder = BrowserConfig::builder()
            .user_data_dir(&user_data_dir)
            .no_sandbox()
            .launch_timeout(Duration::from_millis(self.config.wait_timeout_ms.max(30_000)));

        if !self.config.headless {
            builder = builder.with_head();
        }

        if browser_path.exists() {
            builder = builder.chrome_executable(&browser_path);
        }

        for arg in &stealth_args {
            builder = builder.arg(arg);
        }

        // 处理代理配置
        if let Some(ref proxy) = self.config.proxy_server {
            if !proxy.trim().is_empty() {
                info!(proxy = %proxy, "Checking proxy availability for Kagi search");
                match Self::check_proxy_available(proxy).await {
                    Ok(_) => {
                        let proxy_arg = format!("--proxy-server={}", proxy);
                        builder = builder.arg(&proxy_arg);
                        info!(proxy = %proxy, "✅ Proxy configured for Kagi search");
                    }
                    Err(e) => {
                        warn!(proxy = %proxy, error = %e, "⚠️ Proxy not available, continuing without proxy");
                    }
                }
            }
        }

        let config =
            builder.build().map_err(|e| format!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = chromiumoxide::browser::Browser::launch(config)
            .await
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        // 启动事件处理器
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                trace!(?event, "Chromium event received");
            }
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create page: {}", e))?;

        let fingerprint = self.fingerprint_manager.get_stable_fingerprint(None).clone();

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 应用指纹配置和HTTP头
        self.apply_fingerprint_overrides(&page, &fingerprint).await?;
        self.set_page_http_headers(&page, &fingerprint).await?;

        // 直接导航到搜索结果页面
        self.goto_with_timeout(&page, &search_url, "kagi_session_search").await?;

        // 等待 Kagi 搜索结果加载
        let kagi_selectors = super::super::engines::kagi::KagiEngine::default_wait_selectors();
        self.wait_for_results_with_selectors(&page, &kagi_selectors).await?;

        // 提取 HTML
        let html =
            page.content().await.map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            let page_state = self.capture_page_state(&page).await;
            warn!(
                stage = "kagi_session_search",
                %search_url,
                bytes = html.len(),
                ?page_state,
                "Empty HTML from Kagi session URL search"
            );
            return Err("Empty HTML from Kagi session URL search".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched Kagi search results");

        // 保存调试HTML
        Self::save_debug_html(&html, "kagi_session_search");

        Ok(html)
    }

    async fn fetch_kagi_with_session_url_pooled(
        &mut self,
        search_url: &str,
        pool: &BrowserPool,
    ) -> Result<String, String> {
        let mut pooled_page = pool.acquire_page().await?;
        let page = pooled_page.page();

        let fingerprint = self.fingerprint_manager.get_stable_fingerprint(None).clone();

        // 注入反检测脚本
        self.inject_anti_detection_scripts(page).await?;

        // 应用指纹配置和HTTP头
        self.apply_fingerprint_overrides(page, &fingerprint).await?;
        self.set_page_http_headers(page, &fingerprint).await?;

        // 直接导航到搜索结果页面
        self.goto_with_timeout(page, search_url, "kagi_session_search_pooled").await?;

        // 等待 Kagi 搜索结果加载
        let kagi_selectors = super::super::engines::kagi::KagiEngine::default_wait_selectors();
        self.wait_for_results_with_selectors(page, &kagi_selectors).await?;

        // 提取 HTML
        let html =
            page.content().await.map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            let page_state = self.capture_page_state(page).await;
            warn!(
                stage = "kagi_session_search_pooled",
                %search_url,
                bytes = html.len(),
                ?page_state,
                "Empty HTML from Kagi session URL search"
            );
            return Err("Empty HTML from Kagi session URL search".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched Kagi search results (pooled)");

        // 保存调试HTML
        Self::save_debug_html(&html, "kagi_session_search_pooled");

        Ok(html)
    }
}

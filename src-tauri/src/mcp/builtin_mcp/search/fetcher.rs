use super::browser::BrowserManager;
use super::engine_manager::SearchEngine;
use super::fingerprint::{FingerprintConfig, FingerprintManager, TimingConfig};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use playwright::Playwright;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::process::Command as TokioCommand;
use tokio::time::sleep;
use tracing::{debug, info, trace, warn};

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

    /// 主要的内容抓取方法，按优先级尝试不同策略
    pub async fn fetch_content(
        &mut self,
        url: &str,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        info!(%url, "Starting content fetch");

        // 策略1: Playwright（最优，支持复杂动态内容）
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        match self.fetch_with_playwright(url, browser_manager).await {
            Ok(html) => {
                info!(strategy = "playwright", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(error = %e, strategy = "playwright", "Fetch attempt failed");
            }
        }

        // 策略2: Headless Browser（次优，轻量级）
        match self.fetch_with_headless_browser(url, browser_manager).await {
            Ok(html) => {
                info!(strategy = "headless", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(error = %e, strategy = "headless", "Fetch attempt failed");
            }
        }

        // 策略3: HTTP直接请求（兜底，适合静态内容）
        match self.fetch_with_http(url).await {
            Ok(html) => {
                info!(strategy = "http", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(error = %e, strategy = "http", "Fetch attempt failed");
            }
        }

        // 策略4: WebView兜底（不提取内容，仅导航）
        self.fallback_webview_navigation(url).await
    }

    /// 为搜索请求定制的获取方法
    pub async fn fetch_search_content(
        &mut self,
        query: &str,
        search_engine: &SearchEngine,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        info!(%query, engine = ?search_engine, "Starting search content fetch");

        // 如果是 Kagi 且配置了会话链接，使用直接 URL 方式搜索
        if *search_engine == SearchEngine::Kagi {
            if let Some(session_url) = self.config.kagi_session_url.clone() {
                info!("Using Kagi session URL for direct search");
                return self.fetch_kagi_with_session_url(query, &session_url, browser_manager).await;
            }
        }

        // 使用Playwright执行搜索流程
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        match self.fetch_search_with_playwright(query, search_engine, browser_manager).await {
            Ok(html) => {
                info!(strategy = "playwright_search", bytes = html.len(), "Fetched search content");
                return Ok(html);
            }
            Err(e) => {
                warn!(error = %e, strategy = "playwright_search", "Search flow failed");
            }
        }

        // 搜索流程失败，不再降级到直接URL访问
        Err(format!(
            "Search flow failed for {} engine: {}",
            search_engine.display_name(),
            "All interactive search attempts failed"
        ))
    }

    /// 使用 Kagi 会话链接直接搜索
    /// 会话链接格式：https://kagi.com/search?token=xxxxx
    /// 拼接搜索参数后：https://kagi.com/search?token=xxxxx&q=搜索词
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn fetch_kagi_with_session_url(
        &mut self,
        query: &str,
        session_url: &str,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        // 构造搜索 URL：在会话链接后面拼接 &q=搜索词
        let encoded_query = urlencoding::encode(query);
        let search_url = if session_url.contains('?') {
            format!("{}&q={}", session_url, encoded_query)
        } else {
            format!("{}?q={}", session_url, encoded_query)
        };
        
        info!(%search_url, "Fetching Kagi search results with session URL");

        // 使用 Playwright 直接访问搜索结果页面
        let (_browser_type, browser_path) = browser_manager.get_available_browser()?;

        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let playwright =
            Playwright::initialize().await.map_err(|e| format!("Playwright init error: {}", e))?;

        let chromium = playwright.chromium();
        let mut launcher = chromium.persistent_context_launcher(&user_data_dir);

        // 获取稳定的指纹配置
        let (fingerprint, stealth_args) = {
            let fp = self.fingerprint_manager.get_stable_fingerprint(None).clone();
            let args = FingerprintManager::get_stealth_launch_args();
            (fp, args)
        };

        // 应用指纹配置
        launcher = self.fingerprint_manager.apply_fingerprint_to_context(launcher, &fingerprint);

        // 配置浏览器启动参数
        launcher =
            launcher.executable(&browser_path).headless(self.config.headless).args(&stealth_args);

        if self.config.bypass_csp {
            launcher = launcher.bypass_csp(true);
        }

        // 处理代理配置
        if let Some(ref proxy) = self.config.proxy_server {
            if !proxy.trim().is_empty() {
                info!(proxy = %proxy, "Checking proxy availability for Kagi search");
                match Self::check_proxy_available(proxy).await {
                    Ok(_) => {
                        use playwright::api::ProxySettings;
                        let proxy_settings = ProxySettings {
                            server: proxy.clone(),
                            bypass: None,
                            username: None,
                            password: None,
                        };
                        launcher = launcher.proxy(proxy_settings);
                        info!(proxy = %proxy, "✅ Proxy configured for Kagi search");
                    }
                    Err(e) => {
                        warn!(proxy = %proxy, error = %e, "⚠️ Proxy not available, continuing without proxy");
                    }
                }
            }
        }

        let context =
            launcher.launch().await.map_err(|e| format!("Playwright launch error: {}", e))?;

        let page =
            context.new_page().await.map_err(|e| format!("Playwright new_page error: {}", e))?;

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 在页面级别设置额外的HTTP头
        self.set_page_http_headers(&page, &fingerprint).await?;

        // 直接导航到搜索结果页面
        page.goto_builder(&search_url).goto().await.map_err(|e| format!("Playwright goto error: {}", e))?;

        // 等待 Kagi 搜索结果加载
        let kagi_selectors = super::engines::kagi::KagiEngine::default_wait_selectors();
        self.wait_for_results_with_selectors(&page, &kagi_selectors).await?;

        // 提取 HTML
        let html: String = page
            .eval("() => document.documentElement.outerHTML")
            .await
            .map_err(|e| format!("Playwright eval error: {}", e))?;

        if html.trim().is_empty() {
            return Err("Empty HTML from Kagi session URL search".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched Kagi search results");
        
        // 保存调试HTML
        Self::save_debug_html(&html, "kagi_session_search");
        
        Ok(html)
    }

    /// 等待搜索结果，使用指定的选择器列表
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn wait_for_results_with_selectors(
        &self,
        page: &playwright::api::Page,
        selectors: &[String],
    ) -> Result<(), String> {
        let start = tokio::time::Instant::now();
        let timeout = Duration::from_millis(self.config.wait_timeout_ms);
        let selectors_json = serde_json::to_string(selectors).unwrap_or("[]".to_string());

        let mut check_count = 0;
        loop {
            check_count += 1;

            let found_selector_script = format!(
                "() => {{ const sels = {}; for (const s of sels) {{ if (document.querySelector(s)) return s; }} return null; }}",
                selectors_json
            );

            let found: Option<String> = page.eval(&found_selector_script).await.unwrap_or(None);

            if let Some(sel) = found {
                info!(
                    selector = %sel,
                    check_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "✅ Results loaded"
                );
                // 额外等待一点时间确保内容完全渲染
                sleep(Duration::from_millis(500 + fastrand::u64(0..500))).await;
                return Ok(());
            }

            if start.elapsed() >= timeout {
                warn!(
                    timeout_ms = self.config.wait_timeout_ms,
                    check_count,
                    "⚠️ Results wait timeout, continuing anyway"
                );
                break;
            }

            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }

    /// 使用Playwright执行搜索流程
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn fetch_search_with_playwright(
        &mut self,
        query: &str,
        search_engine: &SearchEngine,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        let (_browser_type, browser_path) = browser_manager.get_available_browser()?;

        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let playwright =
            Playwright::initialize().await.map_err(|e| format!("Playwright init error: {}", e))?;

        let chromium = playwright.chromium();
        let mut launcher = chromium.persistent_context_launcher(&user_data_dir);

        // 获取稳定的指纹配置（通过单独作用域避免借用冲突）
        let (fingerprint, stealth_args) = {
            let fp = self.fingerprint_manager.get_stable_fingerprint(None).clone();
            let args = FingerprintManager::get_stealth_launch_args();
            (fp, args)
        };

        // 应用指纹配置
        launcher = self.fingerprint_manager.apply_fingerprint_to_context(launcher, &fingerprint);

        // 配置浏览器启动参数
        launcher =
            launcher.executable(&browser_path).headless(self.config.headless).args(&stealth_args);

        if self.config.bypass_csp {
            launcher = launcher.bypass_csp(true);
        }

        // 处理代理配置
        let use_proxy = if let Some(ref proxy) = self.config.proxy_server {
            if !proxy.trim().is_empty() {
                info!(proxy = %proxy, "Checking proxy availability for search");
                match Self::check_proxy_available(proxy).await {
                    Ok(_) => {
                        use playwright::api::ProxySettings;
                        let proxy_settings = ProxySettings {
                            server: proxy.clone(),
                            bypass: None,
                            username: None,
                            password: None,
                        };
                        launcher = launcher.proxy(proxy_settings);
                        info!(proxy = %proxy, "✅ Proxy configured for search");
                        true
                    }
                    Err(e) => {
                        warn!(proxy = %proxy, error = %e, "⚠️ Proxy not available, continuing without proxy");
                        false
                    }
                }
            } else {
                false
            }
        } else {
            false
        };
        debug!(use_proxy, "Proxy decision made");

        let context =
            launcher.launch().await.map_err(|e| format!("Playwright launch error: {}", e))?;

        let page =
            context.new_page().await.map_err(|e| format!("Playwright new_page error: {}", e))?;

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 在页面级别设置额外的HTTP头（替代浏览器上下文级别的设置）
        self.set_page_http_headers(&page, &fingerprint).await?;

        // 执行搜索流程（使用人性化的延时）
        let html = self.perform_humanized_search(&page, query, search_engine).await?;

        if html.trim().is_empty() {
            return Err("Empty HTML from search flow".to_string());
        }

        Ok(html)
    }

    /// 使用Playwright抓取内容
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn fetch_with_playwright(
        &mut self,
        url: &str,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        let (_browser_type, browser_path) = browser_manager.get_available_browser()?;

        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let playwright =
            Playwright::initialize().await.map_err(|e| format!("Playwright init error: {}", e))?;

        let chromium = playwright.chromium();
        let mut launcher = chromium.persistent_context_launcher(&user_data_dir);

        // 获取稳定的指纹配置（通过单独作用域避免借用冲突）
        let (fingerprint, stealth_args) = {
            let fp = self.fingerprint_manager.get_stable_fingerprint(None).clone();
            let args = FingerprintManager::get_stealth_launch_args();
            (fp, args)
        };

        // 应用指纹配置
        launcher = self.fingerprint_manager.apply_fingerprint_to_context(launcher, &fingerprint);

        // 配置浏览器启动参数
        launcher =
            launcher.executable(&browser_path).headless(self.config.headless).args(&stealth_args);

        if self.config.bypass_csp {
            launcher = launcher.bypass_csp(true);
        }

        // 处理代理配置
        if let Some(ref proxy) = self.config.proxy_server {
            if !proxy.trim().is_empty() {
                info!(proxy = %proxy, "Checking proxy availability for fetch");
                match Self::check_proxy_available(proxy).await {
                    Ok(_) => {
                        use playwright::api::ProxySettings;
                        let proxy_settings = ProxySettings {
                            server: proxy.clone(),
                            bypass: None,
                            username: None,
                            password: None,
                        };
                        launcher = launcher.proxy(proxy_settings);
                        info!(proxy = %proxy, "✅ Proxy configured for fetch");
                    }
                    Err(e) => {
                        warn!(proxy = %proxy, error = %e, "⚠️ Proxy not available, continuing without proxy");
                    }
                }
            }
        }

        let context =
            launcher.launch().await.map_err(|e| format!("Playwright launch error: {}", e))?;

        let page =
            context.new_page().await.map_err(|e| format!("Playwright new_page error: {}", e))?;

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 在页面级别设置额外的HTTP头（替代浏览器上下文级别的设置）
        self.set_page_http_headers(&page, &fingerprint).await?;

        page.goto_builder(url).goto().await.map_err(|e| format!("Playwright goto error: {}", e))?;

        // 等待页面加载完成
        self.wait_for_content(&page).await?;

        let html: String = page
            .eval("() => document.documentElement.outerHTML")
            .await
            .map_err(|e| format!("Playwright eval error: {}", e))?;

        if html.trim().is_empty() {
            return Err("Empty HTML from Playwright".to_string());
        }

        Ok(html)
    }

    /// 等待页面内容加载
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn wait_for_content(&self, page: &playwright::api::Page) -> Result<(), String> {
        if self.config.wait_selectors.is_empty() {
            page.wait_for_timeout(800.0).await;
            return Ok(());
        }

        let start = std::time::Instant::now();
        let selectors_json =
            serde_json::to_string(&self.config.wait_selectors).unwrap_or("[]".to_string());

        let script = format!(
            "() => {{ const sels = {}; for (const s of sels) {{ if (document.querySelector(s)) return s; }} return null; }}",
            selectors_json
        );

        let mut matched: Option<String> = None;
        loop {
            let found: Option<String> = page
                .eval(&script)
                .await
                .map_err(|e| format!("Playwright wait eval error: {}", e))?;

            if let Some(sel) = found {
                matched = Some(sel);
                break;
            }

            if start.elapsed() >= Duration::from_millis(self.config.wait_timeout_ms) {
                break;
            }

            page.wait_for_timeout(self.config.wait_poll_ms as f64).await;
        }

        if let Some(sel) = matched {
            debug!(selector = %sel, "Waited selector matched");
        } else {
            debug!(timeout_ms = self.config.wait_timeout_ms, "Wait timeout");
        }

        Ok(())
    }

    /// 使用系统浏览器headless模式抓取
    async fn fetch_with_headless_browser(
        &self,
        url: &str,
        browser_manager: &BrowserManager,
    ) -> Result<String, String> {
        let (browser_type, browser_path) = browser_manager.get_available_browser()?;
        debug!(browser = browser_type.as_str(), path = %browser_path.display(), "Headless fetch using browser");

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
            .arg("--timeout=45000")
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
            return Err("Empty DOM output from headless browser".to_string());
        }

        Ok(stdout)
    }

    /// 检查代理是否可用（快速TCP连接测试）
    async fn check_proxy_available(proxy_url: &str) -> Result<(), String> {
        use std::net::ToSocketAddrs;
        
        // 解析代理URL获取主机和端口
        let url = proxy_url.trim();
        let url = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or(url);
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
            Ok(Err(e)) => {
                Err(format!("Failed to connect to proxy {}: {}", proxy_url, e))
            }
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
            .timeout(Duration::from_secs(15));

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
            Ok(base.join("playwright_profile"))
        }
    }

    /// 注入反检测脚本（增强版）
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn inject_anti_detection_scripts(
        &self,
        page: &playwright::api::Page,
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
            window.chrome.runtime = {
                connect: function() { return { onMessage: { addListener: function() {} }, postMessage: function() {}, disconnect: function() {} }; },
                sendMessage: function(msg, cb) { if(cb) cb(); },
                onMessage: { addListener: function() {}, removeListener: function() {} },
                onConnect: { addListener: function() {} },
                id: undefined
            };
            window.chrome.loadTimes = function() {
                return {
                    commitLoadTime: Date.now() / 1000 - Math.random() * 100,
                    finishDocumentLoadTime: Date.now() / 1000 - Math.random() * 50,
                    finishLoadTime: Date.now() / 1000 - Math.random() * 20,
                    firstPaintAfterLoadTime: 0,
                    firstPaintTime: Date.now() / 1000 - Math.random() * 30,
                    navigationType: "Other",
                    npnNegotiatedProtocol: "h2",
                    requestTime: Date.now() / 1000 - Math.random() * 200,
                    startLoadTime: Date.now() / 1000 - Math.random() * 300,
                    connectionInfo: "h2",
                    wasFetchedViaSpdy: true,
                    wasNpnNegotiated: true
                };
            };
            window.chrome.csi = function() {
                return {
                    startE: Date.now() - Math.random() * 1000,
                    onloadT: Date.now() - Math.random() * 500,
                    pageT: Date.now() - Math.random() * 300,
                    tran: Math.floor(Math.random() * 20)
                };
            };
            window.chrome.app = {
                isInstalled: false,
                InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
                RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' }
            };

            // ========== 3. 插件模拟 ==========
            const pluginData = [
                { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
                { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: 'Portable Document Format' },
                { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' }
            ];
            const pluginArray = pluginData.map(p => {
                const plugin = Object.create(Plugin.prototype);
                Object.defineProperties(plugin, {
                    name: { value: p.name, enumerable: true },
                    filename: { value: p.filename, enumerable: true },
                    description: { value: p.description, enumerable: true },
                    length: { value: 1, enumerable: true }
                });
                return plugin;
            });
            Object.defineProperty(navigator, 'plugins', {
                get: () => {
                    const arr = Object.create(PluginArray.prototype);
                    pluginArray.forEach((p, i) => arr[i] = p);
                    arr.length = pluginArray.length;
                    arr.item = (i) => arr[i];
                    arr.namedItem = (name) => pluginArray.find(p => p.name === name);
                    arr.refresh = () => {};
                    return arr;
                },
                configurable: true
            });

            // ========== 4. languages数组 ==========
            Object.defineProperty(navigator, 'languages', {
                get: () => ['zh-CN', 'zh', 'en-US', 'en'],
                configurable: true
            });

            // ========== 5. 权限API ==========
            const originalQuery = navigator.permissions.query.bind(navigator.permissions);
            navigator.permissions.query = (parameters) => {
                if (parameters.name === 'notifications') {
                    return Promise.resolve({ state: Notification.permission, onchange: null });
                }
                return originalQuery(parameters).catch(() => ({ state: 'prompt', onchange: null }));
            };

            // ========== 6. 硬件并发数 ==========
            Object.defineProperty(navigator, 'hardwareConcurrency', {
                get: () => 8,
                configurable: true
            });

            // ========== 7. 设备内存 ==========
            Object.defineProperty(navigator, 'deviceMemory', {
                get: () => 8,
                configurable: true
            });

            // ========== 8. 连接信息 ==========
            if (navigator.connection) {
                Object.defineProperty(navigator.connection, 'rtt', { get: () => 50 + Math.floor(Math.random() * 50) });
            }

            // ========== 9. WebGL指纹随机化 ==========
            const getParameterProxyHandler = {
                apply: function(target, thisArg, args) {
                    const param = args[0];
                    const gl = thisArg;
                    // UNMASKED_VENDOR_WEBGL
                    if (param === 37445) {
                        return 'Google Inc. (NVIDIA)';
                    }
                    // UNMASKED_RENDERER_WEBGL
                    if (param === 37446) {
                        return 'ANGLE (NVIDIA, NVIDIA GeForce GTX 1080 Direct3D11 vs_5_0 ps_5_0, D3D11)';
                    }
                    return Reflect.apply(target, thisArg, args);
                }
            };
            try {
                const canvas = document.createElement('canvas');
                const gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
                if (gl) {
                    gl.getParameter = new Proxy(gl.getParameter.bind(gl), getParameterProxyHandler);
                }
                const gl2 = canvas.getContext('webgl2');
                if (gl2) {
                    gl2.getParameter = new Proxy(gl2.getParameter.bind(gl2), getParameterProxyHandler);
                }
            } catch(e) {}

            // ========== 10. Canvas指纹噪声 ==========
            const originalToDataURL = HTMLCanvasElement.prototype.toDataURL;
            HTMLCanvasElement.prototype.toDataURL = function(type) {
                if (this.width > 16 && this.height > 16) {
                    const ctx = this.getContext('2d');
                    if (ctx) {
                        const imageData = ctx.getImageData(0, 0, this.width, this.height);
                        const data = imageData.data;
                        for (let i = 0; i < data.length; i += 4) {
                            data[i] = data[i] ^ (Math.random() > 0.5 ? 1 : 0);
                        }
                        ctx.putImageData(imageData, 0, 0);
                    }
                }
                return originalToDataURL.apply(this, arguments);
            };

            // ========== 11. 性能API噪声 ==========
            const originalGetEntriesByType = performance.getEntriesByType.bind(performance);
            performance.getEntriesByType = function(type) {
                const entries = originalGetEntriesByType(type);
                if (type === 'navigation' || type === 'resource') {
                    return entries.map(entry => {
                        const clone = {};
                        for (let key in entry) {
                            if (typeof entry[key] === 'number') {
                                clone[key] = entry[key] + (Math.random() * 2 - 1);
                            } else {
                                clone[key] = entry[key];
                            }
                        }
                        return clone;
                    });
                }
                return entries;
            };

            // ========== 12. 自动化检测函数 ==========
            // 移除Playwright/Puppeteer注入的函数
            delete window.__playwright;
            delete window.__pw_manual;
            delete window.__PW_inspect;
            delete window.callPhantom;
            delete window._phantom;
            delete window.phantom;
            delete window.__nightmare;
            delete window.domAutomation;
            delete window.domAutomationController;
            
            // ========== 13. 屏幕信息 ==========
            if (screen.availWidth === 0 || screen.availHeight === 0) {
                Object.defineProperty(screen, 'availWidth', { get: () => screen.width });
                Object.defineProperty(screen, 'availHeight', { get: () => screen.height - 40 });
            }
            
            console.log('[AIPP] Anti-detection scripts injected successfully');
        "#;

        page.add_init_script(anti_detection_script)
            .await
            .map_err(|e| format!("Failed to inject anti-detection script: {}", e))?;
        
        info!("Anti-detection scripts injected");
        Ok(())
    }

    /// 在页面级别设置HTTP头
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn set_page_http_headers(
        &self,
        page: &playwright::api::Page,
        config: &FingerprintConfig,
    ) -> Result<(), String> {
        use std::collections::HashMap;

        let mut headers = HashMap::new();
        headers.insert("Accept-Language".to_string(), config.accept_language.clone());
        headers.insert("Sec-Ch-Ua-Platform".to_string(), format!("\"{}\"", config.platform));
        headers.insert(
            "Sec-Ch-Ua-Mobile".to_string(),
            if config.is_mobile { "?1" } else { "?0" }.to_string(),
        );
        headers.insert(
            "Sec-Ch-Ua".to_string(),
            "\"Not A(Brand\";v=\"99\", \"Google Chrome\";v=\"131\", \"Chromium\";v=\"131\""
                .to_string(),
        );

        page.set_extra_http_headers(headers)
            .await
            .map_err(|e| format!("Failed to set extra HTTP headers: {}", e))?;

        Ok(())
    }

    /// 执行人性化的搜索流程
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn perform_humanized_search(
        &self,
        page: &playwright::api::Page,
        query: &str,
        search_engine: &SearchEngine,
    ) -> Result<String, String> {
        info!(%query, engine = search_engine.as_str(), "Starting humanized search");

        // 随机延时模拟网络延迟
        let initial_delay = self.timing_config.action_delay_min
            + fastrand::u64(
                0..self.timing_config.action_delay_max - self.timing_config.action_delay_min,
            );
        sleep(Duration::from_millis(initial_delay)).await;

        // 带重试的导航到搜索引擎首页
        let homepage_url = search_engine.homepage_url();
        self.navigate_with_retry(page, homepage_url).await?;

        // 等待页面稳定
        sleep(Duration::from_millis(500 + fastrand::u64(0..500))).await;

        // 人性化的输入框定位和填写
        self.humanized_search_input(page, query, search_engine).await?;

        // 人性化的搜索触发
        self.humanized_search_submit(page, search_engine).await?;

        // 等待结果加载，带随机延时
        let wait_time = self.timing_config.page_load_timeout + fastrand::u64(0..2000);
        self.wait_for_results_with_timeout(page, wait_time, search_engine).await?;

        // 增强的HTML提取，带重试机制
        let html = self.extract_page_html_with_retry(page).await?;

        debug!("Successfully retrieved {} bytes", html.len());
        
        // 保存调试HTML
        Self::save_debug_html(&html, "search_result");
        
        Ok(html)
    }

    /// 带重试机制的HTML提取
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn extract_page_html_with_retry(
        &self,
        page: &playwright::api::Page,
    ) -> Result<String, String> {
        let max_retries = 3;
        let mut last_error = String::new();
        let mut last_html: Option<String> = None;

        for attempt in 1..=max_retries {
            info!(attempt, max_retries, "Attempting HTML extraction");

            // 等待页面稳定
            sleep(Duration::from_millis(1000 + fastrand::u64(0..1000))).await;

            // 获取当前页面URL用于调试
            let current_url: String = page
                .eval("() => window.location.href")
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            debug!(attempt, %current_url, "Current page URL");

            // 检查页面是否准备就绪
            match self.check_page_ready(page).await {
                Ok(true) => {
                    info!(attempt, "Page ready check passed");
                    // 页面准备就绪，尝试提取HTML
                    match page.eval("() => document.documentElement.outerHTML").await {
                        Ok(html) => {
                            let html_str: String = html;
                            last_html = Some(html_str.clone());
                            
                            if html_str.len() > 1000 {
                                // 确保HTML内容足够丰富
                                info!(attempt, bytes = html_str.len(), "HTML extraction successful");
                                return Ok(html_str);
                            } else {
                                last_error = format!("HTML too short ({} bytes)", html_str.len());
                                warn!(len = html_str.len(), attempt, "HTML too short, retrying");
                                // 保存短HTML用于调试
                                Self::save_debug_html(&html_str, &format!("short_html_attempt{}", attempt));
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
                    
                    // 获取页面状态信息用于调试
                    let page_info: serde_json::Value = page
                        .eval("() => ({ readyState: document.readyState, bodyChildren: document.body ? document.body.children.length : 0, title: document.title })")
                        .await
                        .unwrap_or_default();
                    debug!(attempt, ?page_info, "Page state info");
                }
                Err(e) => {
                    last_error = format!("Page check error: {}", e);
                    warn!(error = %e, attempt, "Page check error");
                }
            }

            // 在重试之间等待
            if attempt < max_retries {
                sleep(Duration::from_millis(2000)).await;
            }
        }

        // 如果有获取到HTML但被认为太短，也保存下来用于分析
        if let Some(html) = last_html {
            Self::save_debug_html(&html, "failed_extraction");
        }

        Err(format!(
            "Failed to extract HTML after {} attempts. Last error: {}",
            max_retries, last_error
        ))
    }

    /// 检查页面是否准备就绪
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn check_page_ready(&self, page: &playwright::api::Page) -> Result<bool, String> {
        // 检查document是否存在
        // let doc_ready: bool = page
        //     .eval("() => !!document && document.readyState === 'complete'")
        //     .await
        //     .unwrap_or(false);

        // if !doc_ready {
        //     return Ok(false);
        // }

        // 检查body是否存在且有内容
        let body_ready: bool = page
            .eval("() => !!document.body && document.body.children.length > 0")
            .await
            .unwrap_or(false);

        if !body_ready {
            return Ok(false);
        }

        // 检查是否存在任何搜索结果标识
        let has_content: bool = page
            .eval(
                "() => {
                const indicators = [
                    '#b_content', '#b_results', '.b_algo', // Bing
                    '#search', '#main', '.g', '.tF2Cxc', // Google
                    '#results', '.result', '.web-result' // 通用
                ];
                return indicators.some(sel => document.querySelector(sel));
            }",
            )
            .await
            .unwrap_or(false);

        Ok(has_content)
    }

    /// 带重试机制的页面导航
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn navigate_with_retry(
        &self,
        page: &playwright::api::Page,
        url: &str,
    ) -> Result<(), String> {
        let max_retries = 3;
        let mut last_error = String::new();

        for attempt in 1..=max_retries {
            debug!(attempt, max_retries, %url, "Attempting navigation");

            match page.goto_builder(url).goto().await {
                Ok(_) => {
                    info!(attempt, "Navigation successful");

                    // 验证页面是否实际加载成功
                    sleep(Duration::from_millis(1000)).await;

                    let page_loaded: bool = page
                        .eval("() => document.readyState === 'complete' && !!document.body")
                        .await
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
                    debug!(error = %e, "Navigation failed");

                    // 对于特定的错误，我们可以尝试不同的策略
                    if e.to_string().contains("ERR_CONNECTION_CLOSED")
                        || e.to_string().contains("ERR_NETWORK_CHANGED")
                    {
                        warn!("Network connection issue detected, waiting longer before retry");
                        sleep(Duration::from_millis(5000)).await;
                    }
                }
            }

            // 在重试之间等待
            if attempt < max_retries {
                let wait_time = 2000 * attempt as u64; // 递增等待时间
                sleep(Duration::from_millis(wait_time)).await;
            }
        }

        Err(format!(
            "Failed to navigate to {} after {} attempts. Last error: {}",
            url, max_retries, last_error
        ))
    }

    /// 人性化的搜索输入处理
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn humanized_search_input(
        &self,
        page: &playwright::api::Page,
        query: &str,
        search_engine: &SearchEngine,
    ) -> Result<(), String> {
        // 内部辅助函数：构造激活脚本（避免重复 & 方便测试）
        fn build_activation_script(selector: &str) -> String {
            format!(
                r#"() => {{
    const el = document.querySelector('{sel}');
    if (!el) return {{ success: false, method: 'not_found' }};
    try {{
        // 清空（避免历史值影响）
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

        // 内部辅助函数：构造输入脚本
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
        // 触发基础事件（使用单层花括号，防止格式化错误）
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

        // 优先尝试关闭可能阻挡输入框的 Consent / Cookie 弹窗（特别是 Google）
        // 这些弹窗会导致 querySelector 找不到真正可见的输入框或输入失败
        let consent_dismiss_scripts = [
            // Google 新版同意弹窗（更精确的选择器）
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
            // Google 同意弹窗按钮：支持更多语言
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
            // Google "拒绝全部" 也可关闭遮罩
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
            // 直接点击同意框内的第一个可点击按钮（退而求其次）
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
            // Bing cookie同意
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

        // 等待页面完全加载和JavaScript执行
        sleep(Duration::from_millis(1500)).await;
        
        // 先保存一次当前页面HTML用于调试（在处理consent之前）
        if DEBUG_SAVE_HTML {
            if let Ok(html) = page.eval::<String>("() => document.documentElement.outerHTML").await {
                Self::save_debug_html(&html, "before_consent");
            }
        }

        for (idx, script) in consent_dismiss_scripts.iter().enumerate() {
            let result: serde_json::Value = page.eval(script).await.unwrap_or_default();
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
            // 检查元素是否存在和可见
            let element_info = page
                .eval(&format!(
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
                    trace!(selector = %selector, ?info, "Element info");
                    let info_obj: serde_json::Value = info;
                    let exists = info_obj["exists"].as_bool().unwrap_or(false);
                    let visible = info_obj["visible"].as_bool().unwrap_or(false);
                    let disabled = info_obj["disabled"].as_bool().unwrap_or(true);

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
            // 随机延时模拟真实用户行为
            sleep(Duration::from_millis(100 + fastrand::u64(0..200))).await;

            // 使用新的脚本构造器（避免语法错误）
            let activation_script = build_activation_script(selector);
            trace!(%activation_script, "Activation script");
            let result: serde_json::Value = match page.eval(&activation_script).await {
                Ok(v) => v,
                Err(e) => {
                    debug!(selector = %selector, error = %e, "Activation eval error");
                    serde_json::json!({"success": false, "method": "eval_error", "error": e.to_string()})
                }
            };

            let success = result["success"].as_bool().unwrap_or(false);
            trace!(selector = %selector, ?result, "Activation result");

            if !success {
                debug!(selector = %selector, ?result, "Failed to activate element");
                continue;
            }

            // 延时后开始输入
            sleep(Duration::from_millis(150 + fastrand::u64(0..200))).await;

            let input_script = build_input_script(selector, query);
            trace!(%input_script, "Input script");
            let input_result: serde_json::Value = match page.eval(&input_script).await {
                Ok(v) => v,
                Err(e) => {
                    debug!(selector = %selector, error = %e, "Input eval error");
                    serde_json::json!({"success": false, "reason": "eval_error", "error": e.to_string()})
                }
            };

            let input_success = input_result["success"].as_bool().unwrap_or(false);
            trace!(selector = %selector, ?input_result, "Input result");

            if input_success {
                info!(selector = %selector, "Successfully filled search input");
                return Ok(());
            } else {
                debug!(selector = %selector, "Input failed, trying next selector");
                continue;
            }
        }

        // Fallback broad candidate strategy before dumping diagnostics
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
        let fb_res: serde_json::Value = page.eval(&fallback_script).await.unwrap_or_else(
            |e| serde_json::json!({"success":false, "stage":"fallback", "error": e.to_string()}),
        );
        trace!(?fb_res, "Fallback fill result");
        if fb_res["success"].as_bool().unwrap_or(false) {
            info!("Fallback candidate strategy succeeded");
            return Ok(());
        }

        warn!("All selectors failed, dumping page info");

        // 输出页面基本信息
        let page_info: serde_json::Value = page
            .eval(
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
            .unwrap_or_default();
        debug!(?page_info, "Page info");

        // 查找所有可能的输入框
        let input_elements: serde_json::Value = page
            .eval(
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
            .unwrap_or_default();
        warn!(?input_elements, "Found input elements (none worked)");
        
        // 保存失败时的页面HTML用于调试
        if let Ok(html) = page.eval::<String>("() => document.documentElement.outerHTML").await {
            Self::save_debug_html(&html, "input_failed");
        }

        Err("Could not find or fill any search input".to_string())
    }

    /// 人性化的搜索提交
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn humanized_search_submit(
        &self,
        page: &playwright::api::Page,
        search_engine: &SearchEngine,
    ) -> Result<(), String> {
        // 短暂延时，模拟用户思考
        sleep(Duration::from_millis(300 + fastrand::u64(0..700))).await;

        // 尝试点击搜索按钮
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

            let clicked: bool = page.eval(&button_script).await.unwrap_or(false);
            if clicked {
                info!(selector = %selector, "Clicked search button");
                return Ok(());
            }
        }

        // 如果按钮点击失败，尝试按Enter键
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

            let pressed: bool = page.eval(&enter_script).await.unwrap_or(false);
            if pressed {
                info!(selector = %selector, "Pressed Enter on input");
                return Ok(());
            }
        }

        Err("Failed to submit search".to_string())
    }

    /// 等待搜索结果，带超时处理
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn wait_for_results_with_timeout(
        &self,
        page: &playwright::api::Page,
        timeout_ms: u64,
        search_engine: &SearchEngine,
    ) -> Result<(), String> {
        let start = tokio::time::Instant::now();
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
            
            // 检查当前URL，确认已经跳转到搜索结果页
            let current_url: String = page
                .eval("() => window.location.href")
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            
            // 检查是否有任何结果选择器匹配
            let found_selector_script = format!(
                "() => {{ const sels = {}; for (const s of sels) {{ if (document.querySelector(s)) return s; }} return null; }}",
                selectors_json
            );

            let found: Option<String> = page.eval(&found_selector_script).await.unwrap_or(None);

            if let Some(sel) = found {
                info!(
                    selector = %sel, 
                    check_count, 
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    %current_url,
                    "✅ Results loaded"
                );
                // 额外等待一点时间确保内容完全渲染
                sleep(Duration::from_millis(500 + fastrand::u64(0..500))).await;
                return Ok(());
            }

            if start.elapsed() >= timeout {
                // 超时时获取页面状态
                let page_state: serde_json::Value = page
                    .eval("() => ({ url: window.location.href, title: document.title, readyState: document.readyState, bodyLength: document.body ? document.body.innerHTML.length : 0 })")
                    .await
                    .unwrap_or_default();
                    
                warn!(
                    timeout_ms,
                    check_count,
                    ?page_state,
                    "⚠️ Results wait timeout, continuing anyway"
                );
                
                // 保存超时时的页面HTML
                if let Ok(html) = page.eval::<String>("() => document.documentElement.outerHTML").await {
                    Self::save_debug_html(&html, "wait_timeout");
                }
                break;
            }

            // 每5次检查输出一次状态
            if check_count % 5 == 0 {
                debug!(
                    check_count,
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    %current_url,
                    "Still waiting for results..."
                );
            }

            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }
}

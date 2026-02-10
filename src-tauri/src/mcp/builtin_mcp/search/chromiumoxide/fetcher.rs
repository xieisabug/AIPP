use super::browser_pool::BrowserPool;
use super::super::browser::BrowserManager;
use super::super::engine_manager::SearchEngine;
use super::super::fingerprint::{FingerprintConfig, FingerprintManager, TimingConfig};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
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

    /// 导航到URL并等待（带超时）
    async fn goto_with_timeout(
        &self,
        page: &chromiumoxide::page::Page,
        url: &str,
        stage: &str,
    ) -> Result<(), String> {
        let timeout_ms = self.config.wait_timeout_ms.max(30000);
        info!(%url, stage, timeout_ms, "Navigating with Chromium");

        // 导航
        page.goto(url)
            .await
            .map_err(|e| format!("Chromium goto error ({}): {}", stage, e))?;

        // 等待导航完成
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        loop {
            match page.wait_for_navigation().await {
                Ok(_) => {
                    info!(%url, stage, elapsed_ms = start.elapsed().as_millis(), "Navigation completed");
                    return Ok(());
                }
                Err(e) => {
                    if start.elapsed() >= timeout {
                        warn!(%url, stage, timeout_ms, error = %e, "Navigation timeout");
                        return Err(format!("Navigation timeout ({}): {}", stage, e));
                    }
                    // 短暂等待后重试
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }
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

        page.evaluate_on_new_document(anti_detection_script)
            .await
            .map_err(|e| format!("Failed to inject anti-detection script: {}", e))?;

        info!("Anti-detection scripts injected");
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
        match self
            .fetch_with_chromiumoxide(url, browser_manager, browser_pool)
            .await
        {
            Ok(html) => {
                info!(strategy = "chromiumoxide", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(error = %e, strategy = "chromiumoxide", "Fetch attempt failed");
            }
        }

        // 策略2: HTTP直接请求（兜底，适合静态内容）
        match self.fetch_with_http(url).await {
            Ok(html) => {
                info!(strategy = "http", bytes = html.len(), "Fetched content");
                return Ok(html);
            }
            Err(e) => {
                warn!(error = %e, strategy = "http", "Fetch attempt failed");
            }
        }

        // 策略3: WebView兜底（不提取内容，仅导航）
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

        let mut builder = BrowserConfig::builder().user_data_dir(&user_data_dir).no_sandbox();

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

        let config = builder
            .build()
            .map_err(|e| format!("Failed to build browser config: {}", e))?;

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

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        self.goto_with_timeout(&page, url, "fetch_content").await?;

        // 获取 HTML
        let html = page
            .content()
            .await
            .map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            warn!(
                stage = "fetch_content",
                %url,
                bytes = html.len(),
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

        // 注入反检测脚本
        self.inject_anti_detection_scripts(page).await?;

        // 导航到 URL
        self.goto_with_timeout(page, url, "fetch_content_pooled").await?;

        // 获取 HTML
        let html = page
            .content()
            .await
            .map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            warn!(
                stage = "fetch_content_pooled",
                %url,
                bytes = html.len(),
                "Empty HTML from Chromiumoxide (pooled)"
            );
            return Err("Empty HTML from Chromiumoxide (pooled)".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched content (pooled)");

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

        // 暂时未实现搜索流程，返回错误
        Err("Search flow not yet implemented".to_string())
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

        // 使用Chromiumoxide直接访问搜索结果页面
        let browser_path = browser_manager.get_browser_path()?;

        let user_data_dir = self.get_user_data_dir()?;
        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, dir = ?user_data_dir, "Failed to create user_data_dir");
        }

        let stealth_args = FingerprintManager::get_stealth_launch_args();

        use chromiumoxide::BrowserConfig;

        let mut builder = BrowserConfig::builder().user_data_dir(&user_data_dir).no_sandbox();

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

        let config = builder
            .build()
            .map_err(|e| format!("Failed to build browser config: {}", e))?;

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

        // 注入反检测脚本
        self.inject_anti_detection_scripts(&page).await?;

        // 直接导航到搜索结果页面
        self.goto_with_timeout(&page, &search_url, "kagi_session_search").await?;

        // 提取 HTML
        let html = page
            .content()
            .await
            .map_err(|e| format!("Failed to get page content: {}", e))?;

        if html.trim().is_empty() {
            warn!(
                stage = "kagi_session_search",
                %search_url,
                bytes = html.len(),
                "Empty HTML from Kagi session URL search"
            );
            return Err("Empty HTML from Kagi session URL search".to_string());
        }

        info!(bytes = html.len(), "Successfully fetched Kagi search results");

        // 保存调试HTML
        Self::save_debug_html(&html, "kagi_session_search");

        Ok(html)
    }
}

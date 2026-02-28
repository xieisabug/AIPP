use chromiumoxide::browser::Browser;
use futures::StreamExt;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// 单例浏览器池管理器
///
/// 管理单个浏览器实例和多个页面，支持并发访问
#[derive(Clone)]
pub struct BrowserPool {
    /// Browser 实例
    browser: Arc<Mutex<Option<Arc<Mutex<Browser>>>>>,
    /// 可用页面队列
    idle_pages: Arc<Mutex<Vec<chromiumoxide::page::Page>>>,
    /// 当前活跃页面计数
    active_count: Arc<AtomicUsize>,
    /// 配置
    config: BrowserPoolConfig,
}

/// 浏览器池配置
#[derive(Clone, Debug)]
pub struct BrowserPoolConfig {
    /// 最大并发页面数
    pub max_pages: usize,
    /// 页面空闲超时（秒），暂未实现
    pub page_idle_timeout_secs: u64,
    /// 用户数据目录
    pub user_data_dir: Option<String>,
    /// 浏览器路径
    pub browser_path: PathBuf,
    /// 是否 headless
    pub headless: bool,
    /// 启动参数
    pub launch_args: Vec<String>,
}

impl BrowserPool {
    /// 创建新的浏览器池（懒加载，首次使用时初始化）
    pub fn new(config: BrowserPoolConfig) -> Self {
        Self {
            browser: Arc::new(Mutex::new(None)),
            idle_pages: Arc::new(Mutex::new(Vec::new())),
            active_count: Arc::new(AtomicUsize::new(0)),
            config,
        }
    }

    /// 获取一个页面（自动创建或复用）
    pub async fn acquire_page(&self) -> Result<PooledPage, String> {
        // 检查并发限制
        let current = self.active_count.load(Ordering::Acquire);
        if current >= self.config.max_pages {
            return Err(format!(
                "Maximum concurrent page limit reached: {}",
                self.config.max_pages
            ));
        }

        // 增加活跃计数
        self.active_count.fetch_add(1, Ordering::AcqRel);

        // 确保浏览器已初始化
        let mut browser = self.get_or_init_browser().await.map_err(|e| {
            // 初始化失败，减少计数
            self.active_count.fetch_sub(1, Ordering::AcqRel);
            e
        })?;

        // 尝试从空闲队列获取页面
        loop {
            let maybe_page = {
                let mut idle = self.idle_pages.lock().await;
                idle.pop()
            };
            let Some(page) = maybe_page else {
                break;
            };

            match self.ensure_page_healthy(&page).await {
                Ok(_) => {
                    debug!("Reusing idle page");
                    return Ok(PooledPage { page: Some(page), pool: Some(self.clone()) });
                }
                Err(error_message) => {
                    warn!(error = %error_message, "Discarding unhealthy idle page from pool");
                    if Self::is_connection_closed_error(&error_message) {
                        browser = self.recreate_browser().await.map_err(|e| {
                            self.active_count.fetch_sub(1, Ordering::AcqRel);
                            e
                        })?;
                    }
                }
            }
        }

        // 创建新页面
        let page_result = {
            let browser_guard = browser.lock().await;
            browser_guard.new_page("about:blank").await
        };

        let page = match page_result {
            Ok(page) => page,
            Err(e) => {
                let error_message = e.to_string();
                if Self::is_connection_closed_error(&error_message) {
                    warn!(
                        error = %error_message,
                        "Browser connection appears closed when creating page, recreating browser"
                    );
                    browser = self.recreate_browser().await.map_err(|recreate_error| {
                        self.active_count.fetch_sub(1, Ordering::AcqRel);
                        recreate_error
                    })?;
                    let retry_result = {
                        let browser_guard = browser.lock().await;
                        browser_guard.new_page("about:blank").await
                    };
                    retry_result.map_err(|retry_error| {
                        self.active_count.fetch_sub(1, Ordering::AcqRel);
                        format!(
                            "Failed to create new page after browser recreation: {}",
                            retry_error
                        )
                    })?
                } else {
                    self.active_count.fetch_sub(1, Ordering::AcqRel);
                    return Err(format!("Failed to create new page: {}", error_message));
                }
            }
        };

        debug!("Created new page");
        Ok(PooledPage { page: Some(page), pool: Some(self.clone()) })
    }

    /// 获取或初始化浏览器
    async fn get_or_init_browser(&self) -> Result<Arc<Mutex<Browser>>, String> {
        let mut browser_slot = self.browser.lock().await;
        if let Some(existing) = browser_slot.as_ref() {
            return Ok(existing.clone());
        }

        let browser = Arc::new(Mutex::new(self.initialize_browser().await?));
        *browser_slot = Some(browser.clone());
        Ok(browser)
    }

    async fn recreate_browser(&self) -> Result<Arc<Mutex<Browser>>, String> {
        info!("Recreating Chromium BrowserPool browser instance");

        {
            let mut idle = self.idle_pages.lock().await;
            idle.clear();
        }

        let old_browser = {
            let mut browser_slot = self.browser.lock().await;
            browser_slot.take()
        };

        if let Some(browser) = old_browser {
            let mut guard = browser.lock().await;
            if let Err(e) = guard.close().await {
                warn!(error = %e, "Failed to close stale browser before recreation");
            }
            if let Err(e) = guard.wait().await {
                warn!(error = %e, "Failed to wait for stale browser exit before recreation");
            }
        }

        self.get_or_init_browser().await
    }

    async fn ensure_page_healthy(
        &self,
        page: &chromiumoxide::page::Page,
    ) -> Result<(), String> {
        page.evaluate("() => document.readyState")
            .await
            .map(|_| ())
            .map_err(|e| format!("Page health check failed: {}", e))
    }

    fn is_connection_closed_error(error: &str) -> bool {
        let lower = error.to_lowercase();
        lower.contains("alreadyclosed")
            || lower.contains("ws(")
            || lower.contains("connection closed")
            || lower.contains("websocket")
            || lower.contains("broken pipe")
    }

    /// 初始化浏览器（只在首次调用时执行）
    async fn initialize_browser(&self) -> Result<Browser, String> {
        info!("Initializing Chromium BrowserPool");

        // 创建用户数据目录
        let user_data_dir = if let Some(ref dir) = self.config.user_data_dir {
            PathBuf::from(dir)
        } else {
            std::env::temp_dir().join("aipp_chromiumoxide_pool")
        };

        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, "Failed to create user_data_dir");
        }

        use chromiumoxide::BrowserConfig;

        let mut builder = BrowserConfig::builder()
            .user_data_dir(&user_data_dir)
            .no_sandbox()
            .launch_timeout(Duration::from_secs(60));

        if !self.config.headless {
            builder = builder.with_head();
        }

        // 设置浏览器路径
        let browser_path_exists = self.config.browser_path.exists();
        if browser_path_exists {
            builder = builder.chrome_executable(&self.config.browser_path);
        } else {
            warn!(path = %self.config.browser_path.display(), "Browser executable not found, using default path");
        }

        // 添加启动参数
        for arg in &self.config.launch_args {
            builder = builder.arg(arg);
        }

        info!(
            headless = self.config.headless,
            browser_path = %self.config.browser_path.display(),
            browser_path_exists = browser_path_exists,
            user_data_dir = %user_data_dir.display(),
            launch_args = ?self.config.launch_args,
            "Launching Chromiumoxide BrowserPool"
        );

        let config =
            builder.build().map_err(|e| format!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| {
                format!(
                    "Failed to launch browser (path={}, exists={}, headless={}, user_data_dir={}, args={:?}): {}",
                    self.config.browser_path.display(),
                    browser_path_exists,
                    self.config.headless,
                    user_data_dir.display(),
                    self.config.launch_args,
                    e
                )
            })?;

        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                tracing::trace!(?event, "Chromium event received");
            }
        });

        info!("Chromium BrowserPool initialized successfully");
        Ok(browser)
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        let browser = {
            let mut browser_slot = self.browser.lock().await;
            browser_slot.take()
        };

        if let Some(browser) = browser {
            let mut guard = browser.lock().await;
            guard.close().await.map_err(|e| format!("Failed to close browser: {}", e))?;
            if let Err(e) = guard.wait().await {
                warn!(error = %e, "Failed to wait for browser process exit");
            }
        }

        {
            let mut idle = self.idle_pages.lock().await;
            idle.clear();
        }
        Ok(())
    }

    /// 归还页面到池中
    async fn return_page(&self, page: chromiumoxide::page::Page) {
        let mut idle = self.idle_pages.lock().await;
        idle.push(page);
        // 减少活跃计数
        self.active_count.fetch_sub(1, Ordering::AcqRel);
        debug!(
            "Returned page to pool, idle count: {}, active: {}",
            idle.len(),
            self.active_count.load(Ordering::Acquire)
        );
    }

    /// 获取当前活跃页面数
    pub fn active_count(&self) -> usize {
        self.active_count.load(Ordering::Acquire)
    }
}

/// 池化的页面，自动归还到池中
pub struct PooledPage {
    page: Option<chromiumoxide::page::Page>,
    pool: Option<BrowserPool>,
}

impl PooledPage {
    /// 获取底层页面引用
    pub fn page(&self) -> &chromiumoxide::page::Page {
        self.page.as_ref().expect("Page not available")
    }

    /// 获取底层页面可变引用
    pub fn page_mut(&mut self) -> &mut chromiumoxide::page::Page {
        self.page.as_mut().expect("Page not available")
    }

    /// 消费 self，不归还页面（用于出错时）
    pub fn consume(mut self) -> chromiumoxide::page::Page {
        // 消费时需要减少活跃计数
        if let Some(ref pool) = self.pool {
            pool.active_count.fetch_sub(1, Ordering::AcqRel);
        }
        self.page.take().expect("Page not available")
    }
}

impl Drop for PooledPage {
    fn drop(&mut self) {
        if let Some(page) = self.page.take() {
            if let Some(pool) = self.pool.take() {
                let pool_clone = pool.clone();
                tokio::spawn(async move {
                    pool_clone.return_page(page).await;
                });
            }
        }
    }
}

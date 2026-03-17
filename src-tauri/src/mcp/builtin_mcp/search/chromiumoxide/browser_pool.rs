use chromiumoxide::browser::Browser;
use futures::StreamExt;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// 单例浏览器池管理器
///
/// 管理单个浏览器实例和多个页面，支持并发访问
#[derive(Clone)]
pub struct BrowserPool {
    /// Browser 实例
    browser: Arc<Mutex<Option<Arc<Mutex<Browser>>>>>,
    /// 可用页面队列
    idle_pages: Arc<StdMutex<Vec<chromiumoxide::page::Page>>>,
    /// 控制最大并发页面数
    page_semaphore: Arc<Semaphore>,
    /// 配置
    config: BrowserPoolConfig,
}

/// 浏览器池配置
#[derive(Clone, Debug)]
pub struct BrowserPoolConfig {
    /// 最大并发页面数
    pub max_pages: usize,
    /// 获取页面 permit 的等待超时（毫秒）
    pub acquire_timeout_ms: u64,
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
            idle_pages: Arc::new(StdMutex::new(Vec::new())),
            page_semaphore: Arc::new(Semaphore::new(config.max_pages.max(1))),
            config,
        }
    }

    async fn acquire_slot_permit(&self) -> Result<OwnedSemaphorePermit, String> {
        match timeout(
            Duration::from_millis(self.config.acquire_timeout_ms.max(1_000)),
            self.page_semaphore.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => Err("Browser page semaphore closed unexpectedly".to_string()),
            Err(_) => Err(format!(
                "Timed out waiting for browser page slot after {}ms",
                self.config.acquire_timeout_ms.max(1_000)
            )),
        }
    }

    fn idle_pages_lock(&self) -> std::sync::MutexGuard<'_, Vec<chromiumoxide::page::Page>> {
        self.idle_pages.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 获取一个页面（自动创建或复用）
    pub async fn acquire_page(&self) -> Result<PooledPage, String> {
        let permit = self.acquire_slot_permit().await?;

        // 确保浏览器已初始化
        let mut browser = self.get_or_init_browser().await?;

        // 尝试从空闲队列获取页面
        loop {
            let maybe_page = { self.idle_pages_lock().pop() };
            let Some(page) = maybe_page else {
                break;
            };

            match self.ensure_page_healthy(&page).await {
                Ok(_) => {
                    debug!("Reusing idle page");
                    return Ok(PooledPage {
                        page: Some(page),
                        pool: Some(self.clone()),
                        permit: Some(permit),
                    });
                }
                Err(error_message) => {
                    warn!(error = %error_message, "Discarding unhealthy idle page from pool");
                    if Self::is_connection_closed_error(&error_message) {
                        browser = self.recreate_browser().await?;
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
                    browser = self.recreate_browser().await?;
                    let retry_result = {
                        let browser_guard = browser.lock().await;
                        browser_guard.new_page("about:blank").await
                    };
                    retry_result.map_err(|retry_error| {
                        format!(
                            "Failed to create new page after browser recreation: {}",
                            retry_error
                        )
                    })?
                } else {
                    return Err(format!("Failed to create new page: {}", error_message));
                }
            }
        };

        debug!("Created new page");
        Ok(PooledPage { page: Some(page), pool: Some(self.clone()), permit: Some(permit) })
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

        self.idle_pages_lock().clear();

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

    async fn ensure_page_healthy(&self, page: &chromiumoxide::page::Page) -> Result<(), String> {
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

        let browser_path_exists = self.config.browser_path.exists();
        if browser_path_exists {
            builder = builder.chrome_executable(&self.config.browser_path);
        } else {
            warn!(
                path = %self.config.browser_path.display(),
                "Browser executable not found, using default path"
            );
        }

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

        let (browser, mut handler) = Browser::launch(config).await.map_err(|e| {
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

        self.idle_pages_lock().clear();
        Ok(())
    }

    /// 归还页面到池中
    fn return_page(&self, page: chromiumoxide::page::Page) {
        let mut idle = self.idle_pages_lock();
        idle.push(page);
        debug!(
            "Returned page to pool, idle count: {}, active: {}",
            idle.len(),
            self.active_count()
        );
    }

    /// 获取当前活跃页面数
    pub fn active_count(&self) -> usize {
        self.config.max_pages.saturating_sub(self.page_semaphore.available_permits())
    }
}

/// 池化的页面，自动归还到池中
pub struct PooledPage {
    page: Option<chromiumoxide::page::Page>,
    pool: Option<BrowserPool>,
    permit: Option<OwnedSemaphorePermit>,
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
        let page = self.page.take().expect("Page not available");
        self.permit.take();
        page
    }
}

impl Drop for PooledPage {
    fn drop(&mut self) {
        if let Some(page) = self.page.take() {
            if let Some(pool) = self.pool.take() {
                pool.return_page(page);
            }
        }
        self.permit.take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task::JoinSet;

    fn test_pool(max_pages: usize) -> BrowserPool {
        BrowserPool::new(BrowserPoolConfig {
            max_pages,
            acquire_timeout_ms: 250,
            page_idle_timeout_secs: 60,
            user_data_dir: None,
            browser_path: PathBuf::from("/tmp/nonexistent-chromium"),
            headless: true,
            launch_args: Vec::new(),
        })
    }

    #[tokio::test]
    async fn acquire_slot_waits_until_permit_released() {
        let pool = test_pool(1);
        let first = pool.acquire_slot_permit().await.expect("first permit should succeed");
        assert_eq!(pool.active_count(), 1);

        let waiting_pool = pool.clone();
        let waiter = tokio::spawn(async move { waiting_pool.acquire_slot_permit().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!waiter.is_finished(), "second permit should still be waiting");

        drop(first);
        let second = waiter
            .await
            .expect("waiter task should complete")
            .expect("second permit should succeed");
        assert_eq!(pool.active_count(), 1);
        drop(second);
        assert_eq!(pool.active_count(), 0);
    }

    #[tokio::test]
    async fn acquire_slot_times_out_when_pool_is_full() {
        let pool = test_pool(1);
        let _first = pool.acquire_slot_permit().await.expect("first permit should succeed");

        let error = pool
            .acquire_slot_permit()
            .await
            .expect_err("second permit should time out while the pool is full");
        assert!(error.contains("Timed out waiting for browser page slot"));
    }

    #[tokio::test]
    async fn acquire_slot_is_bounded_under_concurrency() {
        let pool = Arc::new(test_pool(3));
        let mut tasks = JoinSet::new();

        for _ in 0..9 {
            let pool = pool.clone();
            tasks.spawn(async move {
                let permit =
                    pool.acquire_slot_permit().await.expect("permit should eventually succeed");
                tokio::time::sleep(Duration::from_millis(20)).await;
                drop(permit);
            });
        }

        while let Some(result) = tasks.join_next().await {
            result.expect("task should complete");
        }

        assert_eq!(pool.active_count(), 0);
        assert_eq!(pool.page_semaphore.available_permits(), 3);
    }
}

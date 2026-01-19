use playwright::api::{BrowserContext, Page};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};
use tracing::{debug, info, warn};

/// 单例浏览器池管理器
///
/// 管理单个浏览器实例和多个页面，支持并发访问
#[derive(Clone)]
pub struct BrowserPool {
    /// Playwright 实例，必须保持存活以维持连接
    playwright: Arc<OnceCell<Playwright>>,
    /// 单个浏览器上下文（persistent context）
    context: Arc<OnceCell<BrowserContext>>,
    /// 可用页面队列
    idle_pages: Arc<Mutex<Vec<Page>>>,
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
            playwright: Arc::new(OnceCell::new()),
            context: Arc::new(OnceCell::new()),
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

        // 确保浏览器已初始化（使用 OnceCell 确保只初始化一次）
        let context = self
            .get_or_init_context()
            .await
            .map_err(|e| {
                // 初始化失败，减少计数
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                e
            })?;

        // 尝试从空闲队列获取页面
        {
            let mut idle = self.idle_pages.lock().await;
            if let Some(page) = idle.pop() {
                debug!("Reusing idle page");
                return Ok(PooledPage {
                    page: Some(page),
                    pool: Some(self.clone()),
                });
            }
        }

        // 创建新页面
        let page = context
            .new_page()
            .await
            .map_err(|e| {
                // 创建失败，减少计数
                self.active_count.fetch_sub(1, Ordering::AcqRel);
                format!("Failed to create new page: {}", e)
            })?;

        debug!("Created new page");
        Ok(PooledPage {
            page: Some(page),
            pool: Some(self.clone()),
        })
    }

    /// 获取或初始化浏览器上下文
    async fn get_or_init_context(&self) -> Result<&BrowserContext, String> {
        // OnceCell::get_or_try_init 确保初始化只执行一次
        self.context
            .get_or_try_init(|| async {
                self.initialize_browser().await
            })
            .await
            .map_err(|e| e.to_string())
    }

    /// 初始化浏览器（只在首次调用时执行）
    async fn initialize_browser(&self) -> Result<BrowserContext, String> {
        info!("Initializing BrowserPool");

        let playwright = self
            .playwright
            .get_or_try_init(|| async {
                Playwright::initialize()
                    .await
                    .map_err(|e| format!("Playwright init error: {}", e))
            })
            .await?;

        let chromium = playwright.chromium();

        // 创建用户数据目录
        let user_data_dir = if let Some(ref dir) = self.config.user_data_dir {
            PathBuf::from(dir)
        } else {
            std::env::temp_dir().join("aipp_playwright_pool")
        };

        if let Err(e) = fs::create_dir_all(&user_data_dir) {
            warn!(error = %e, "Failed to create user_data_dir");
        }

        let mut launcher = chromium.persistent_context_launcher(&user_data_dir);
        launcher = launcher
            .executable(&self.config.browser_path)
            .headless(self.config.headless)
            .args(&self.config.launch_args);

        let context = launcher
            .launch()
            .await
            .map_err(|e| format!("Failed to launch browser: {}", e))?;

        info!("BrowserPool initialized successfully");
        Ok(context)
    }

    /// 归还页面到池中
    async fn return_page(&self, page: Page) {
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
    page: Option<Page>,
    pool: Option<BrowserPool>,
}

impl PooledPage {
    /// 获取底层页面引用
    pub fn page(&self) -> &Page {
        self.page.as_ref().expect("Page not available")
    }

    /// 消费 self，不归还页面（用于出错时）
    pub fn consume(mut self) -> Page {
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

// 将 Playwright 导入
use playwright::Playwright;

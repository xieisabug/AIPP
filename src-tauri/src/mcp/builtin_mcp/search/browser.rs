use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BrowserType {
    Chrome,
}

impl BrowserType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "chrome" => Some(BrowserType::Chrome),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            BrowserType::Chrome => "chrome",
        }
    }
}

pub struct BrowserManager {
    preferred_type: Option<BrowserType>,
}

impl BrowserManager {
    pub fn new(browser_type_config: Option<&str>) -> Self {
        let preferred_type = browser_type_config.and_then(|s| BrowserType::from_str(s));

        Self { preferred_type }
    }

    /// 获取 Chrome 浏览器路径
    pub fn get_browser_path(&self) -> Result<PathBuf, String> {
        if let Some(path) = self.find_chrome_path() {
            info!(browser = "chrome", path = %path.display(), "Using Chrome browser");
            return Ok(path);
        }

        Err("No supported browser (Chrome) found on system".to_string())
    }

    /// 查找Chrome浏览器路径
    fn find_chrome_path(&self) -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let candidates = [
                r"C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
                r"C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
                "chrome.exe", // 从PATH中查找
            ];
            self.try_candidates(&candidates)
        }

        #[cfg(target_os = "macos")]
        {
            let candidates = [
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                "chrome", // 从PATH中查找
                "google-chrome",
            ];
            self.try_candidates(&candidates)
        }

        #[cfg(target_os = "linux")]
        {
            let candidates =
                ["google-chrome", "google-chrome-stable", "chrome", "chromium", "chromium-browser"];
            self.try_candidates(&candidates)
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            None
        }
    }

    /// 尝试多个候选路径，找到第一个存在的
    fn try_candidates(&self, candidates: &[&str]) -> Option<PathBuf> {
        for candidate in candidates {
            let path = PathBuf::from(candidate);

            // 如果是绝对路径，直接检查文件是否存在
            if path.is_absolute() {
                if path.is_file() {
                    return Some(path);
                }
            } else {
                // 如果是相对路径或命令名，从PATH中查找
                if let Ok(found_path) = which::which(candidate) {
                    return Some(found_path);
                }
            }
        }
        None
    }
}

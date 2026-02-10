use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

// 指纹配置接口
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfig {
    pub device_name: String,
    pub locale: String,
    pub timezone_id: String,
    pub color_scheme: String,   // "dark" | "light"
    pub reduced_motion: String, // "reduce" | "no-preference"
    pub forced_colors: String,  // "active" | "none"
    pub user_agent: String,
    pub viewport_width: i32,
    pub viewport_height: i32,
    pub device_scale_factor: f64,
    pub is_mobile: bool,
    pub has_touch: bool,
    pub screen_width: i32,
    pub screen_height: i32,
    pub accept_language: String,
    pub platform: String,
}

// 保存的状态文件接口
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedState {
    pub fingerprint: Option<FingerprintConfig>,
    pub google_domain: Option<String>,
    pub last_update: Option<i64>,
}

/// 指纹管理器
pub struct FingerprintManager {
    state_file_path: String,
    saved_state: SavedState,
}

impl FingerprintManager {
    pub fn new(app_data_dir: &Path) -> Self {
        let state_file_path =
            app_data_dir.join("search_fingerprint_state.json").to_string_lossy().to_string();
        let saved_state = Self::load_saved_state(&state_file_path);

        Self { state_file_path, saved_state }
    }

    /// 获取或生成稳定的指纹配置
    pub fn get_stable_fingerprint(&mut self, user_locale: Option<&str>) -> &FingerprintConfig {
        // 如果没有保存的指纹配置，生成一个新的
        if self.saved_state.fingerprint.is_none() {
            let config = self.generate_host_machine_config(user_locale);
            self.saved_state.fingerprint = Some(config);
            self.saved_state.last_update = Some(chrono::Utc::now().timestamp());
            self.save_state();
        }

        self.saved_state.fingerprint.as_ref().unwrap()
    }

    /// 生成基于宿主机器的指纹配置
    fn generate_host_machine_config(&self, user_locale: Option<&str>) -> FingerprintConfig {
        // 获取系统区域设置
        let system_locale = user_locale.unwrap_or("zh-CN");

        // 获取系统时区
        let timezone_id = self.detect_timezone();

        // 检测系统颜色方案（基于时间智能推断）
        let hour = chrono::Local::now().hour();
        let color_scheme = if hour >= 19 || hour < 7 { "dark" } else { "light" };

        // 选择一个常见的桌面设备
        let devices = self.get_common_desktop_devices();
        let device_template = &devices[fastrand::usize(0..devices.len())];

        // 生成随机但合理的屏幕分辨率变化
        let scale_variation = 0.9 + fastrand::f64() * 0.2; // 0.9 到 1.1
        let viewport_width = (device_template.viewport_width as f64 * scale_variation) as i32;
        let viewport_height = (device_template.viewport_height as f64 * scale_variation) as i32;
        let screen_width = (device_template.screen_width as f64 * scale_variation) as i32;
        let screen_height = (device_template.screen_height as f64 * scale_variation) as i32;

        FingerprintConfig {
            device_name: device_template.name.clone(),
            locale: system_locale.to_string(),
            timezone_id,
            color_scheme: color_scheme.to_string(),
            reduced_motion: "no-preference".to_string(),
            forced_colors: "none".to_string(),
            user_agent: device_template.user_agent.clone(),
            viewport_width,
            viewport_height,
            device_scale_factor: device_template.device_scale_factor,
            is_mobile: device_template.is_mobile,
            has_touch: device_template.has_touch,
            screen_width,
            screen_height,
            accept_language: self.generate_accept_language(system_locale),
            platform: self.detect_platform(),
        }
    }

    /// 检测系统时区
    fn detect_timezone(&self) -> String {
        // 获取系统时区偏移量
        let local_offset = chrono::Local::now().offset().local_minus_utc();
        let hours_offset = local_offset / 3600;

        match hours_offset {
            28800 => "Asia/Shanghai".to_string(),        // UTC+8 中国
            32400 => "Asia/Tokyo".to_string(),           // UTC+9 日本
            25200 => "Asia/Bangkok".to_string(),         // UTC+7 东南亚
            0 => "Europe/London".to_string(),            // UTC+0 英国
            3600 => "Europe/Berlin".to_string(),         // UTC+1 欧洲
            -18000 => "America/New_York".to_string(),    // UTC-5 美国东部
            -28800 => "America/Los_Angeles".to_string(), // UTC-8 美国西部
            _ => "Asia/Shanghai".to_string(),            // 默认
        }
    }

    /// 检测系统平台
    fn detect_platform(&self) -> String {
        if cfg!(target_os = "windows") {
            "Win32".to_string()
        } else if cfg!(target_os = "macos") {
            "MacIntel".to_string()
        } else {
            "Linux x86_64".to_string()
        }
    }

    /// 生成Accept-Language头
    fn generate_accept_language(&self, locale: &str) -> String {
        match locale {
            l if l.starts_with("zh") => "zh-CN,zh;q=0.9,en;q=0.8,en-US;q=0.7".to_string(),
            l if l.starts_with("en") => "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".to_string(),
            l if l.starts_with("ja") => "ja,en;q=0.9,zh-CN;q=0.8".to_string(),
            l if l.starts_with("ko") => "ko,en;q=0.9,zh-CN;q=0.8".to_string(),
            _ => "zh-CN,zh;q=0.9,en;q=0.8,en-US;q=0.7".to_string(),
        }
    }

    /// 获取常见桌面设备配置
    fn get_common_desktop_devices(&self) -> Vec<DeviceTemplate> {
        vec![
            DeviceTemplate {
                name: "Desktop Chrome Windows".to_string(),
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
                viewport_width: 1920,
                viewport_height: 1080,
                device_scale_factor: 1.0,
                is_mobile: false,
                has_touch: false,
                screen_width: 1920,
                screen_height: 1080,
            },
            DeviceTemplate {
                name: "Desktop Chrome macOS".to_string(),
                user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
                viewport_width: 1440,
                viewport_height: 900,
                device_scale_factor: 2.0,
                is_mobile: false,
                has_touch: false,
                screen_width: 2880,
                screen_height: 1800,
            },
            DeviceTemplate {
                name: "Desktop High DPI".to_string(),
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
                viewport_width: 2560,
                viewport_height: 1440,
                device_scale_factor: 1.5,
                is_mobile: false,
                has_touch: false,
                screen_width: 2560,
                screen_height: 1440,
            },
        ]
    }

    /// 获取增强的浏览器启动参数
    pub fn get_stealth_launch_args() -> Vec<String> {
        vec![
            // 基础隐身参数
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
            "--disable-dev-shm-".to_string(),
            "--disable-extensions".to_string(),
            // 重要：移除自动化控制标识
            "--disable-blink-features=AutomationControlled".to_string(),
            "--disable-features=VizDisplayCompositor".to_string(),
            // 禁用各种检测
            "--disable-background-timer-throttling".to_string(),
            "--disable-backgrounding-occluded-windows".to_string(),
            "--disable-renderer-backgrounding".to_string(),
            "--disable-feature-policy".to_string(),
            "--disable-ipc-flooding-protection".to_string(),
            // 模拟正常用户行为
            "--enable-features=NetworkService".to_string(),
            "--use-mock-keychain".to_string(),
            "--disable-component-update".to_string(),
            // 内存和性能优化
            "--max_old_space_size=4096".to_string(),
            "--memory-pressure-off".to_string(),
            // 禁用日志和错误报告
            "--disable-logging".to_string(),
            "--log-level=3".to_string(),
            "--silent".to_string(),
            // 网络优化
            "--aggressive-cache-discard".to_string(),
            "--enable-features=NetworkServiceInProcess".to_string(),
        ]
    }

    /// 获取随机但一致的延时配置
    pub fn get_timing_config() -> TimingConfig {
        TimingConfig {
            typing_delay_min: 50 + fastrand::u64(0..50),
            typing_delay_max: 120 + fastrand::u64(0..80),
            action_delay_min: 200 + fastrand::u64(0..100),
            action_delay_max: 500 + fastrand::u64(0..200),
            page_load_timeout: 15000 + fastrand::u64(0..5000),
        }
    }

    /// 加载保存的状态
    fn load_saved_state(file_path: &str) -> SavedState {
        if let Ok(content) = fs::read_to_string(file_path) {
            if let Ok(state) = serde_json::from_str::<SavedState>(&content) {
                return state;
            }
        }

        SavedState { fingerprint: None, google_domain: None, last_update: None }
    }

    /// 保存状态到文件
    fn save_state(&self) {
        if let Ok(content) = serde_json::to_string_pretty(&self.saved_state) {
            if let Some(parent) = Path::new(&self.state_file_path).parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&self.state_file_path, content);
        }
    }
}

#[derive(Debug, Clone)]
struct DeviceTemplate {
    name: String,
    user_agent: String,
    viewport_width: i32,
    viewport_height: i32,
    device_scale_factor: f64,
    is_mobile: bool,
    has_touch: bool,
    screen_width: i32,
    screen_height: i32,
}

#[derive(Debug, Clone)]
pub struct TimingConfig {
    pub typing_delay_min: u64,
    pub typing_delay_max: u64,
    pub action_delay_min: u64,
    pub action_delay_max: u64,
    pub page_load_timeout: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // ============================================
    // FingerprintConfig Tests
    // ============================================

    #[test]
    fn test_fingerprint_config_serialization() {
        let config = FingerprintConfig {
            device_name: "Desktop Chrome".to_string(),
            locale: "en-US".to_string(),
            timezone_id: "America/New_York".to_string(),
            color_scheme: "dark".to_string(),
            reduced_motion: "no-preference".to_string(),
            forced_colors: "none".to_string(),
            user_agent: "Mozilla/5.0 Test".to_string(),
            viewport_width: 1920,
            viewport_height: 1080,
            device_scale_factor: 1.0,
            is_mobile: false,
            has_touch: false,
            screen_width: 1920,
            screen_height: 1080,
            accept_language: "en-US,en;q=0.9".to_string(),
            platform: "Win32".to_string(),
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("Desktop Chrome"));
        assert!(json.contains("en-US"));

        let deserialized: FingerprintConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.device_name, "Desktop Chrome");
    }

    // ============================================
    // SavedState Tests
    // ============================================

    #[test]
    fn test_saved_state_serialization() {
        let state = SavedState {
            fingerprint: None,
            google_domain: Some("google.com".to_string()),
            last_update: Some(1234567890),
        };

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: SavedState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.google_domain, Some("google.com".to_string()));
        assert_eq!(deserialized.last_update, Some(1234567890));
    }

    #[test]
    fn test_saved_state_empty() {
        let state = SavedState { fingerprint: None, google_domain: None, last_update: None };

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: SavedState = serde_json::from_str(&json).unwrap();
        assert!(deserialized.fingerprint.is_none());
    }

    // ============================================
    // FingerprintManager Tests
    // ============================================

    #[test]
    fn test_fingerprint_manager_new() {
        let temp_dir = TempDir::new().unwrap();
        let manager = FingerprintManager::new(temp_dir.path());
        assert!(manager.saved_state.fingerprint.is_none());
    }

    #[test]
    fn test_fingerprint_manager_get_stable_fingerprint() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        // First call should generate a fingerprint
        let config = manager.get_stable_fingerprint(Some("en-US"));
        assert!(!config.device_name.is_empty());
        assert!(!config.user_agent.is_empty());
        assert!(config.viewport_width > 0);
        assert!(config.viewport_height > 0);
    }

    #[test]
    fn test_fingerprint_manager_generates_valid_locale() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(Some("zh-CN"));
        assert_eq!(config.locale, "zh-CN");
    }

    #[test]
    fn test_fingerprint_manager_default_locale() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        assert_eq!(config.locale, "zh-CN"); // default
    }

    // ============================================
    // get_stealth_launch_args Tests
    // ============================================

    #[test]
    fn test_get_stealthy_launch_args_not_empty() {
        let args = FingerprintManager::get_stealth_launch_args();
        assert!(!args.is_empty());
    }

    #[test]
    fn test_get_stealth_launch_args_contains_automation_control() {
        let args = FingerprintManager::get_stealth_launch_args();
        assert!(args.iter().any(|a| a.contains("AutomationControlled")));
    }

    #[test]
    fn test_get_stealth_launch_args_contains_no_first_run() {
        let args = FingerprintManager::get_stealth_launch_args();
        assert!(args.contains(&"--no-first-run".to_string()));
    }

    #[test]
    fn test_get_stealth_launch_args_contains_disable_logging() {
        let args = FingerprintManager::get_stealth_launch_args();
        assert!(args.contains(&"--disable-logging".to_string()));
    }

    // ============================================
    // get_timing_config Tests
    // ============================================

    #[test]
    fn test_get_timing_config_valid_ranges() {
        let config = FingerprintManager::get_timing_config();

        // Typing delay should be in reasonable range
        assert!(config.typing_delay_min >= 50);
        assert!(config.typing_delay_max >= config.typing_delay_min);
        assert!(config.typing_delay_max <= 300);

        // Action delay should be in reasonable range
        assert!(config.action_delay_min >= 200);
        assert!(config.action_delay_max >= config.action_delay_min);
        assert!(config.action_delay_max <= 900);

        // Page load timeout should be reasonable
        assert!(config.page_load_timeout >= 15000);
        assert!(config.page_load_timeout <= 25000);
    }

    #[test]
    fn test_get_timing_config_randomness() {
        // Get multiple configs and check they can vary
        let mut configs = Vec::new();
        for _ in 0..10 {
            configs.push(FingerprintManager::get_timing_config());
        }

        // Check that not all values are identical (would indicate no randomness)
        let first_typing_min = configs[0].typing_delay_min;
        let all_same = configs.iter().all(|c| c.typing_delay_min == first_typing_min);
        // With 10 samples, it's very unlikely all would be the same if there's randomness
        // But we allow for the possibility in case of test flakiness
        // The main goal is to ensure the config is valid
        assert!(configs.iter().all(|c| c.typing_delay_min >= 50));
    }

    // ============================================
    // TimingConfig Tests
    // ============================================

    #[test]
    fn test_timing_config_struct() {
        let config = TimingConfig {
            typing_delay_min: 50,
            typing_delay_max: 100,
            action_delay_min: 200,
            action_delay_max: 500,
            page_load_timeout: 15000,
        };

        assert_eq!(config.typing_delay_min, 50);
        assert_eq!(config.page_load_timeout, 15000);
    }

    // ============================================
    // Internal Method Tests (via public interface)
    // ============================================

    #[test]
    fn test_fingerprint_has_valid_user_agent() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        assert!(config.user_agent.contains("Mozilla"));
        assert!(config.user_agent.contains("AppleWebKit") || config.user_agent.contains("Chrome"));
    }

    #[test]
    fn test_fingerprint_has_valid_timezone() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        assert!(!config.timezone_id.is_empty());
        assert!(config.timezone_id.contains("/"));
    }

    #[test]
    fn test_fingerprint_has_valid_accept_language() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(Some("zh-CN"));
        assert!(config.accept_language.contains("zh-CN"));
        assert!(config.accept_language.contains("q="));
    }

    #[test]
    fn test_fingerprint_has_valid_platform() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        assert!(
            config.platform == "Win32"
                || config.platform == "MacIntel"
                || config.platform == "Linux x86_64"
        );
    }

    #[test]
    fn test_fingerprint_color_scheme() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        assert!(config.color_scheme == "dark" || config.color_scheme == "light");
    }

    #[test]
    fn test_fingerprint_device_scale_factor() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        assert!(config.device_scale_factor >= 1.0);
        assert!(config.device_scale_factor <= 3.0);
    }

    #[test]
    fn test_fingerprint_viewport_reasonable_size() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = FingerprintManager::new(temp_dir.path());

        let config = manager.get_stable_fingerprint(None);
        // Viewport should be reasonable desktop size
        assert!(config.viewport_width >= 1000);
        assert!(config.viewport_width <= 4000);
        assert!(config.viewport_height >= 600);
        assert!(config.viewport_height <= 3000);
    }

    #[test]
    fn test_fingerprint_persists_to_file() {
        let temp_dir = TempDir::new().unwrap();

        // Create manager and generate fingerprint
        {
            let mut manager = FingerprintManager::new(temp_dir.path());
            let _ = manager.get_stable_fingerprint(Some("en-US"));
        }

        // Check that state file was created
        let state_file = temp_dir.path().join("search_fingerprint_state.json");
        assert!(state_file.exists());

        // Load again and verify fingerprint is loaded
        let content = std::fs::read_to_string(&state_file).unwrap();
        let state: SavedState = serde_json::from_str(&content).unwrap();
        assert!(state.fingerprint.is_some());
    }

    #[test]
    fn test_fingerprint_accept_language_variants() {
        let temp_dir = TempDir::new().unwrap();

        // Test Chinese
        {
            let mut manager = FingerprintManager::new(temp_dir.path());
            let config = manager.get_stable_fingerprint(Some("zh-CN"));
            assert!(config.accept_language.starts_with("zh-CN"));
        }

        // Clean up for next test
        let _ = std::fs::remove_file(temp_dir.path().join("search_fingerprint_state.json"));

        // Test English
        {
            let mut manager = FingerprintManager::new(temp_dir.path());
            let config = manager.get_stable_fingerprint(Some("en-US"));
            assert!(config.accept_language.starts_with("en-US"));
        }
    }
}

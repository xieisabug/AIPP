//! Sync configuration and management for libSQL embedded replica sync.
//!
//! Handles:
//! - Reading/writing sync settings to a device-local JSON file
//! - Translating user settings into `DatabaseMode`
//! - First-sync flow metadata for later rollout

use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

/// User-facing sync configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SyncMode {
    Manual,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SyncStatus {
    #[default]
    Never,
    Success,
    Error,
}

/// User-facing sync configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncConfig {
    /// Whether sync is enabled for this device.
    pub enabled: bool,
    /// The sqld server URL (e.g. "http://my-server:8080").
    pub server_url: Option<String>,
    /// JWT auth token for the sqld server.
    pub auth_token: Option<String>,
    /// Whether sync runs automatically or only on demand.
    pub sync_mode: SyncMode,
    /// Sync interval in seconds (default: 60).
    pub sync_interval_secs: u64,
    /// First sync behavior when connecting this device to an existing cloud DB.
    pub first_sync_strategy: FirstSyncStrategy,
    /// Whether this device already completed its first sync.
    pub initial_sync_completed: bool,
    /// Last sync start time in RFC3339.
    pub last_sync_started_at: Option<String>,
    /// Last sync completion time in RFC3339.
    pub last_sync_finished_at: Option<String>,
    /// Last sync status.
    pub last_sync_status: SyncStatus,
    /// User-visible sync result message.
    pub last_sync_message: Option<String>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server_url: None,
            auth_token: None,
            sync_mode: SyncMode::Auto,
            sync_interval_secs: 60,
            first_sync_strategy: FirstSyncStrategy::UseRemote,
            initial_sync_completed: false,
            last_sync_started_at: None,
            last_sync_finished_at: None,
            last_sync_status: SyncStatus::Never,
            last_sync_message: None,
        }
    }
}

/// First-sync strategy when a device connects for the first time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum FirstSyncStrategy {
    /// Use remote data, discard local.
    #[default]
    UseRemote,
    /// Use local data, overwrite remote.
    UseLocal,
    /// Append local data to remote (requires ID remapping).
    AppendLocal,
    /// Ask the user to back up local data first, then use remote data.
    BackupThenUseRemote,
}

/// Manages sync lifecycle: configuration persistence, periodic sync, first-sync flow.
pub struct SyncManager {
    config: SyncConfig,
}

impl SyncManager {
    pub fn new(config: SyncConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &SyncConfig {
        &self.config
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Convert the sync config into a `DatabaseMode` for `DatabaseManager`.
    pub fn to_database_mode(&self) -> super::connection::DatabaseMode {
        if self.config.enabled && self.config.initial_sync_completed {
            if let (Some(url), Some(token)) = (&self.config.server_url, &self.config.auth_token) {
                return super::connection::DatabaseMode::Synced {
                    url: url.clone(),
                    auth_token: token.clone(),
                };
            }
        }
        super::connection::DatabaseMode::Local
    }

    pub fn validate(config: &SyncConfig) -> std::result::Result<(), String> {
        if !config.enabled {
            return Ok(());
        }

        let Some(server_url) =
            config.server_url.as_deref().map(str::trim).filter(|value| !value.is_empty())
        else {
            return Err("启用同步时必须填写服务器地址".to_string());
        };

        if Url::parse(server_url).is_err() {
            return Err("同步服务地址格式无效，请填写完整的 http:// 或 https:// 地址".to_string());
        }

        if config.auth_token.as_deref().map(|value| value.trim().is_empty()).unwrap_or(true) {
            return Err("启用同步时必须填写访问令牌".to_string());
        }

        if matches!(config.sync_mode, SyncMode::Auto) && config.sync_interval_secs == 0 {
            return Err("同步间隔必须大于 0 秒".to_string());
        }

        Ok(())
    }

    pub fn config_path(app_handle: &tauri::AppHandle) -> std::result::Result<PathBuf, String> {
        let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        Ok(app_dir.join("sync-config.json"))
    }

    pub fn load_for_app(app_handle: &tauri::AppHandle) -> std::result::Result<SyncConfig, String> {
        let path = Self::config_path(app_handle)?;
        if !path.exists() {
            return Ok(SyncConfig::default());
        }

        let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let config: SyncConfig = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        Self::validate(&config)?;
        Ok(config)
    }

    pub fn save_for_app(
        app_handle: &tauri::AppHandle,
        config: &SyncConfig,
    ) -> std::result::Result<(), String> {
        Self::validate(config)?;
        let path = Self::config_path(app_handle)?;
        let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        std::fs::write(path, raw).map_err(|e| e.to_string())
    }
}

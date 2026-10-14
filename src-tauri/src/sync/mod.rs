use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::path::PathBuf;
use std::time::Duration;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use reqwest::StatusCode;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as JsonValue};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::db::get_db_path;
use crate::db::system_db::{FeatureConfig, SecureConfigEntry, SystemDatabase};

mod divergence;

const FEATURE_CODE: &str = "data_sync";
const TOKEN_SCOPE: &str = "data_sync";
const TOKEN_KEY: &str = "token";
const SECURE_MASTER_KEY_FILE: &str = "secure-config-master-key.bin";
const KEYRING_SERVICE: &str = "com.xieisabug.aipp";
const KEYRING_MASTER_KEY_USER: &str = "sync-secure-master-key";
const DEFAULT_SCOPE: &str = "default";
const CLIENT_SCHEMA_VERSION: i64 = 2;
const SYNC_INTERVAL_SECS: u64 = 45;
const DEBOUNCE_MS: u64 = 1200;
const HTTP_TIMEOUT_SECS: u64 = 30;
const NOTIFY_CONNECT_TIMEOUT_SECS: u64 = 10;
const NOTIFY_RECONNECT_SECS: u64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Local,
    SelfHosted,
}

impl Default for SyncMode {
    fn default() -> Self {
        SyncMode::Local
    }
}

impl SyncMode {
    fn from_config(value: &str) -> Self {
        match value {
            "self_hosted" | "self-hosted" => SyncMode::SelfHosted,
            _ => SyncMode::Local,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SyncRuntimeStatus {
    connected: bool,
    running: bool,
    syncing: bool,
    last_sync_at: Option<String>,
    last_error: Option<String>,
    /// 本轮进程内累计的"远端赢"冲突解决数，前端据此提示用户本地修改被覆盖
    conflict_resolved_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatusDto {
    mode: SyncMode,
    server_url: String,
    token_configured: bool,
    connected: bool,
    running: bool,
    syncing: bool,
    last_sync_at: Option<String>,
    last_error: Option<String>,
    pending_outbox_count: i64,
    pushing_outbox_count: i64,
    failed_outbox_count: i64,
    dead_letter_count: i64,
    needs_reset: bool,
    conflict_resolved_count: i64,
    server_cursor: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveSyncSettingsRequest {
    mode: SyncMode,
    server_url: Option<String>,
    token: Option<String>,
}

pub struct SyncState {
    worker_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    notify_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    debounce_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    sync_lock: Mutex<()>,
    /// Wakes the poll loop early when SSE reports a remote cursor advance.
    wake: Arc<tokio::sync::Notify>,
    status: Mutex<SyncRuntimeStatus>,
}

impl Default for SyncState {
    fn default() -> Self {
        Self {
            worker_task: Mutex::new(None),
            notify_task: Mutex::new(None),
            debounce_task: Mutex::new(None),
            sync_lock: Mutex::new(()),
            wake: Arc::new(tokio::sync::Notify::new()),
            status: Mutex::new(SyncRuntimeStatus::default()),
        }
    }
}

impl SyncState {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
struct SyncSettings {
    mode: SyncMode,
    server_url: String,
    token: Option<String>,
}

#[derive(Debug, Clone)]
struct SyncTableSpec {
    db_name: &'static str,
    table: &'static str,
    object_type: &'static str,
    columns: &'static [&'static str],
    natural_key_columns: &'static [&'static str],
    /// DB UNIQUE（或种子软身份）列。apply 时按这些列 find-or-update，
    /// 并将 sync_object_map 重映射为远端 sync_id（远端获胜），避免 UNIQUE 死信。
    unique_key_columns: &'static [&'static str],
    /// 无 flag 列的固定种子 local id（如 assistant id=1），使用 official:{type}:{id}。
    seed_official_ids: &'static [i64],
    /// 官方/内置种子行的标记列（如 is_official / is_builtin）。
    /// 值为真时该行使用确定性 object_id（official:{type}:{local_id}），多端收敛不重复。
    official_column: Option<&'static str>,
    foreign_keys: &'static [ForeignKeySpec],
    where_clause: Option<&'static str>,
    order_by: &'static str,
}

#[derive(Debug, Clone)]
struct ForeignKeySpec {
    column: &'static str,
    object_type: &'static str,
}

#[derive(Debug, Clone)]
struct LocalObjectSnapshot {
    object_type: String,
    object_id: String,
    payload_json: String,
    payload_hash: String,
}

#[derive(Debug, Clone, Serialize)]
struct PushRequest {
    device_id: String,
    device_name: String,
    events: Vec<PushEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct PushEvent {
    event_id: String,
    object_type: String,
    object_id: String,
    operation: String,
    base_version: Option<i64>,
    local_version: i64,
    payload: Option<JsonValue>,
    created_at: String,
    client_schema_version: i64,
    object_schema_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct PushResponse {
    accepted: Vec<AcceptedEvent>,
    conflicts: Vec<ConflictEvent>,
    rejected: Vec<RejectedEvent>,
}

#[derive(Debug, Clone, Deserialize)]
struct AcceptedEvent {
    event_id: String,
    object_type: String,
    object_id: String,
    server_version: i64,
    #[allow(dead_code)]
    server_seq: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ConflictEvent {
    event_id: String,
    object_type: String,
    object_id: String,
    server_version: i64,
    server_payload: Option<JsonValue>,
    /// 服务端当前的操作状态：upsert / delete（v2 协议新增；旧服务端缺省按 upsert 处理）
    #[serde(default = "default_server_operation")]
    server_operation: String,
}

fn default_server_operation() -> String {
    "upsert".to_string()
}

#[derive(Debug, Clone, Deserialize)]
struct RejectedEvent {
    event_id: String,
    reason: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PullResponse {
    cursor: i64,
    has_more: bool,
    changes: Vec<PullChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PullChange {
    #[allow(dead_code)]
    seq: i64,
    event_id: String,
    device_id: String,
    object_type: String,
    object_id: String,
    operation: String,
    version: i64,
    payload: Option<JsonValue>,
    deleted_at: Option<String>,
    #[allow(dead_code)]
    created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteStatus {
    #[allow(dead_code)]
    latest_cursor: i64,
    remote_empty: bool,
    #[allow(dead_code)]
    device_registered: bool,
    #[allow(dead_code)]
    min_client_schema_version: i64,
    #[allow(dead_code)]
    max_client_schema_version: i64,
}

fn specs() -> Vec<SyncTableSpec> {
    vec![
        SyncTableSpec {
            db_name: "llm.db",
            table: "llm_provider",
            object_type: "llm.provider",
            columns: &["id", "name", "api_type", "description", "is_official", "is_enabled"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: Some("is_official"),
            foreign_keys: &[],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "llm.db",
            table: "llm_provider_config",
            object_type: "llm.provider_config",
            columns: &["id", "name", "llm_provider_id", "value", "append_location", "is_addition"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "llm_provider_id", object_type: "llm.provider" }],
            // 白名单：只同步已知不含密钥的配置名。env 类配置（acp_env_vars / acp_env_* 等）
            // 的值可能内嵌 API key，刻意不同步；新增配置名时须评估敏感性后再加入。
            where_clause: Some("LOWER(name) IN ('endpoint', 'base_url', 'acp_cli_command', 'acp_working_directory', 'acp_additional_args', 'acp_claude_auth_mode')"),
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "llm.db",
            table: "llm_model",
            object_type: "llm.model",
            columns: &["id", "name", "llm_provider_id", "code", "description", "vision_support", "audio_support", "video_support"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "llm_provider_id", object_type: "llm.provider" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "llm.db",
            table: "llm_model_request_mode_preference",
            object_type: "llm.model_request_mode_preference",
            columns: &["id", "llm_provider_id", "model_code", "request_mode", "created_time", "updated_time"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "llm_provider_id", object_type: "llm.provider" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant",
            object_type: "assistant",
            columns: &["id", "name", "description", "assistant_type", "is_addition", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[1],
            official_column: None,
            foreign_keys: &[],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_prompt",
            object_type: "assistant.prompt",
            columns: &["id", "assistant_id", "prompt", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "assistant_id", object_type: "assistant" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_model",
            object_type: "assistant.model",
            columns: &["id", "assistant_id", "provider_id", "model_code", "alias"],
            natural_key_columns: &[],
            unique_key_columns: &["assistant_id", "provider_id", "model_code"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "assistant_id", object_type: "assistant" },
                ForeignKeySpec { column: "provider_id", object_type: "llm.provider" },
            ],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_model_config",
            object_type: "assistant.model_config",
            columns: &["id", "assistant_id", "assistant_model_id", "name", "value", "value_type"],
            natural_key_columns: &[],
            unique_key_columns: &["assistant_id", "name"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "assistant_id", object_type: "assistant" },
                ForeignKeySpec { column: "assistant_model_id", object_type: "assistant.model" },
            ],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "mcp.db",
            table: "mcp_server",
            object_type: "mcp.server",
            columns: &["id", "name", "description", "transport_type", "url", "timeout", "is_long_running", "is_enabled", "is_builtin", "is_deletable", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: Some("is_builtin"),
            foreign_keys: &[],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "mcp.db",
            table: "mcp_server_tool",
            object_type: "mcp.server_tool",
            columns: &["id", "server_id", "tool_name", "tool_description", "is_enabled", "is_auto_run", "parameters", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &["server_id", "tool_name"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "server_id", object_type: "mcp.server" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "mcp.db",
            table: "mcp_server_resource",
            object_type: "mcp.server_resource",
            columns: &["id", "server_id", "resource_uri", "resource_name", "resource_type", "resource_description", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &["server_id", "resource_uri"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "server_id", object_type: "mcp.server" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "mcp.db",
            table: "mcp_server_prompt",
            object_type: "mcp.server_prompt",
            columns: &["id", "server_id", "prompt_name", "prompt_description", "is_enabled", "arguments", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &["server_id", "prompt_name"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "server_id", object_type: "mcp.server" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_mcp_config",
            object_type: "assistant.mcp_config",
            columns: &["id", "assistant_id", "mcp_server_id", "is_enabled", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &["assistant_id", "mcp_server_id"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "assistant_id", object_type: "assistant" },
                ForeignKeySpec { column: "mcp_server_id", object_type: "mcp.server" },
            ],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_mcp_tool_config",
            object_type: "assistant.mcp_tool_config",
            columns: &["id", "assistant_id", "mcp_tool_id", "is_enabled", "is_auto_run", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &["assistant_id", "mcp_tool_id"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "assistant_id", object_type: "assistant" },
                ForeignKeySpec { column: "mcp_tool_id", object_type: "mcp.server_tool" },
            ],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "conversation.db",
            table: "conversation",
            object_type: "conversation",
            columns: &["id", "name", "assistant_id", "created_time", "updated_time", "conversation_kind", "parent_butler_conversation_id", "source_task_title", "is_hidden_from_normal_chat_list", "channel_source", "butler_task_status", "butler_task_summary", "butler_task_finalized_at"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "assistant_id", object_type: "assistant" },
                ForeignKeySpec { column: "parent_butler_conversation_id", object_type: "conversation" },
            ],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "conversation.db",
            table: "message",
            object_type: "conversation.message",
            columns: &["id", "conversation_id", "message_type", "content", "llm_model_id", "created_time", "token_count", "input_token_count", "output_token_count", "parent_id", "start_time", "finish_time", "llm_model_name", "generation_group_id", "parent_group_id", "tool_calls_json", "metadata_json", "first_token_time", "ttft_ms"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "conversation_id", object_type: "conversation" },
                ForeignKeySpec { column: "parent_id", object_type: "conversation.message" },
                ForeignKeySpec { column: "llm_model_id", object_type: "llm.model" },
            ],
            where_clause: Some("(message_type != 'response' OR finish_time IS NOT NULL)"),
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "conversation.db",
            table: "message_attachment",
            object_type: "conversation.message_attachment",
            columns: &["id", "message_id", "attachment_type", "attachment_url", "attachment_hash", "attachment_content", "use_vector", "token_count"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "message_id", object_type: "conversation.message" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "conversation.db",
            table: "conversation_summary",
            object_type: "conversation.summary",
            columns: &["id", "conversation_id", "summary", "user_intent", "key_outcomes", "created_time"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "conversation_id", object_type: "conversation" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "conversation.db",
            table: "conversation_todo",
            object_type: "conversation.todo",
            columns: &["id", "conversation_id", "content", "status", "active_form", "sort_order", "created_time", "updated_time"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "conversation_id", object_type: "conversation" }],
            where_clause: None,
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "system.db",
            table: "feature_config",
            object_type: "system.feature_config",
            columns: &["id", "feature_code", "key", "value", "data_type", "description"],
            natural_key_columns: &["feature_code", "key"],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[],
            where_clause: Some("feature_code != 'data_sync'"),
            order_by: "feature_code, key",
        },
        SyncTableSpec {
            db_name: "artifacts.db",
            table: "artifacts_collection",
            object_type: "artifacts.collection",
            columns: &["id", "name", "icon", "description", "artifact_type", "code", "tags", "created_time", "last_used_time", "use_count", "db_id", "assistant_id"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec { column: "assistant_id", object_type: "assistant" }],
            where_clause: None,
            order_by: "id",
        },
    ]
}

pub fn start_worker_from_config(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = ensure_sync_db(&app_handle) {
            warn!(error = %err, "Failed to initialize sync metadata database");
            return;
        }
        if matches!(load_settings(&app_handle).map(|s| s.mode), Ok(SyncMode::SelfHosted)) {
            start_worker(app_handle).await;
        }
    });
}

pub fn schedule_sync_after_local_change(app_handle: &AppHandle) {
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if !matches!(load_settings(&app).map(|s| s.mode), Ok(SyncMode::SelfHosted)) {
            return;
        }
        let Some(state) = app.try_state::<SyncState>() else {
            return;
        };
        let mut task = state.debounce_task.lock().await;
        if let Some(existing) = task.take() {
            existing.abort();
        }
        let sync_app = app.clone();
        *task = Some(tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
            run_sync_once(&sync_app).await;
        }));
    });
}

pub fn flush_before_exit(app_handle: &AppHandle) -> tauri::async_runtime::JoinHandle<()> {
    let app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if matches!(load_settings(&app).map(|s| s.mode), Ok(SyncMode::SelfHosted)) {
            run_sync_once(&app).await;
        }
    })
}

#[tauri::command]
pub async fn get_sync_status(app_handle: AppHandle) -> Result<SyncStatusDto, String> {
    sync_status(&app_handle).await
}

#[tauri::command]
pub async fn save_sync_settings(
    app_handle: AppHandle,
    feature_state: State<'_, crate::FeatureConfigState>,
    request: SaveSyncSettingsRequest,
) -> Result<SyncStatusDto, String> {
    let SaveSyncSettingsRequest { mode, server_url, token } = request;
    let server_url = server_url.unwrap_or_default().trim().trim_end_matches('/').to_string();

    match mode {
        SyncMode::Local => {
            save_feature_config_value(&app_handle, &feature_state, "mode", "local").await?;
            save_feature_config_value(&app_handle, &feature_state, "server_url", &server_url).await?;
            stop_worker(&app_handle).await;
        }
        SyncMode::SelfHosted => {
            if server_url.is_empty() {
                return Err("自建同步模式需要填写服务器地址".to_string());
            }
            let parsed = reqwest::Url::parse(&server_url)
                .map_err(|_| "服务器地址必须是有效的 http(s) URL".to_string())?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err("服务器地址必须使用 http 或 https".to_string());
            }
            let previous = load_settings(&app_handle)?;
            let token = token.unwrap_or_default().trim().to_string();
            let existing = previous.token.clone();
            if token.is_empty() && existing.is_none() {
                return Err("自建同步模式需要填写访问 token".to_string());
            }
            // 服务器地址或账号 token 变更：若本地已有同步状态（cursor/shadow/map），
            // 不能静默沿用——置 needs_reset，等用户确认后重新全量同步。
            let url_changed = previous.server_url != server_url;
            let token_changed = !token.is_empty() && existing.as_deref() != Some(token.as_str());
            if !token.is_empty() {
                save_sync_token(&app_handle, &token)?;
            }
            save_feature_config_value(&app_handle, &feature_state, "mode", "self_hosted").await?;
            save_feature_config_value(&app_handle, &feature_state, "server_url", &server_url).await?;
            let reset_required = (url_changed || token_changed) && {
                ensure_sync_db(&app_handle)?;
                let conn = open_sync_db(&app_handle)?;
                has_sync_state(&conn)?
            };
            if reset_required {
                let conn = open_sync_db(&app_handle)?;
                set_sync_meta(&conn, META_NEEDS_RESET, "1")?;
                warn!("Sync server/account changed with existing local state; waiting for reset confirmation");
            }
            start_worker(app_handle.clone()).await;
            if !reset_required {
                run_sync_once(&app_handle).await;
            }
        }
    }

    let _ = app_handle.emit("feature_config_changed", ());
    sync_status(&app_handle).await
}

#[tauri::command]
pub async fn trigger_sync_now(app_handle: AppHandle) -> Result<SyncStatusDto, String> {
    let settings = load_settings(&app_handle)?;
    if settings.mode != SyncMode::SelfHosted {
        return Err("当前是本地模式，未启用自建同步".to_string());
    }
    if settings.token.as_deref().unwrap_or_default().trim().is_empty() {
        return Err("缺少同步 token，请先保存自建同步配置".to_string());
    }
    run_sync_once(&app_handle).await;
    sync_status(&app_handle).await
}

#[tauri::command]
pub async fn retry_failed_sync_outbox(app_handle: AppHandle) -> Result<SyncStatusDto, String> {
    let settings = load_settings(&app_handle)?;
    if settings.mode != SyncMode::SelfHosted {
        return Err("当前是本地模式，未启用自建同步".to_string());
    }
    if settings.token.as_deref().unwrap_or_default().trim().is_empty() {
        return Err("缺少同步 token，请先保存自建同步配置".to_string());
    }

    let retried = reset_failed_events(&app_handle)?;
    if retried > 0 {
        emit_status_changed(&app_handle).await;
        run_sync_once(&app_handle).await;
    }
    sync_status(&app_handle).await
}

#[tauri::command]
pub async fn retry_sync_dead_letters(app_handle: AppHandle) -> Result<SyncStatusDto, String> {
    let settings = load_settings(&app_handle)?;
    if settings.mode != SyncMode::SelfHosted {
        return Err("当前是本地模式，未启用自建同步".to_string());
    }
    ensure_sync_db(&app_handle)?;
    let device_id = {
        let conn = open_sync_db(&app_handle)?;
        ensure_device_id(&conn)?
    };
    replay_dead_letters(&app_handle, &device_id)?;
    emit_status_changed(&app_handle).await;
    sync_status(&app_handle).await
}

#[tauri::command]
pub async fn reset_sync_state(app_handle: AppHandle) -> Result<SyncStatusDto, String> {
    let settings = load_settings(&app_handle)?;
    if settings.mode != SyncMode::SelfHosted {
        return Err("当前是本地模式，未启用自建同步".to_string());
    }
    ensure_sync_db(&app_handle)?;
    {
        let conn = open_sync_db(&app_handle)?;
        clear_sync_state(&conn)?;
    }
    emit_status_changed(&app_handle).await;
    // 重置完成后立即重新全量同步（bootstrap）
    run_sync_once(&app_handle).await;
    sync_status(&app_handle).await
}

async fn start_worker(app_handle: AppHandle) {
    let state = app_handle.state::<SyncState>();
    let mut task = state.worker_task.lock().await;
    if task.is_some() {
        return;
    }

    let app = app_handle.clone();
    let wake = state.wake.clone();
    *task = Some(tauri::async_runtime::spawn(async move {
        loop {
            if !matches!(load_settings(&app).map(|s| s.mode), Ok(SyncMode::SelfHosted)) {
                break;
            }
            run_sync_once(&app).await;
            // Wait for either the poll interval or an SSE wake (remote push).
            tokio::select! {
                _ = wake.notified() => {
                    debug!("Sync worker woken by notify channel");
                }
                _ = tokio::time::sleep(Duration::from_secs(SYNC_INTERVAL_SECS)) => {}
            }
        }
    }));
    drop(task);

    {
        let mut notify_task = state.notify_task.lock().await;
        if notify_task.is_none() {
            let notify_app = app_handle.clone();
            *notify_task = Some(tauri::async_runtime::spawn(async move {
                run_notify_listener(notify_app).await;
            }));
        }
    }

    let mut runtime = state.status.lock().await;
    runtime.running = true;
    drop(runtime);
    emit_status_changed(&app_handle).await;
}

async fn stop_worker(app_handle: &AppHandle) {
    if let Some(state) = app_handle.try_state::<SyncState>() {
        if let Some(task) = state.worker_task.lock().await.take() {
            task.abort();
        }
        if let Some(task) = state.notify_task.lock().await.take() {
            task.abort();
        }
        if let Some(task) = state.debounce_task.lock().await.take() {
            task.abort();
        }
        let mut runtime = state.status.lock().await;
        runtime.running = false;
        runtime.connected = false;
        runtime.last_error = None;
    }
    emit_status_changed(app_handle).await;
}

async fn run_sync_once(app_handle: &AppHandle) {
    let Some(state) = app_handle.try_state::<SyncState>() else {
        return;
    };
    let _guard = state.sync_lock.lock().await;
    {
        let mut runtime = state.status.lock().await;
        runtime.running = matches!(load_settings(app_handle).map(|s| s.mode), Ok(SyncMode::SelfHosted));
        runtime.syncing = true;
        runtime.last_error = None;
    }
    emit_status_changed(app_handle).await;

    let result = run_sync_once_inner(app_handle).await;
    let mut runtime = state.status.lock().await;
    match result {
        Ok(()) => {
            runtime.connected = true;
            runtime.last_error = None;
            let synced_at = now_string();
            runtime.last_sync_at = Some(synced_at.clone());
            let _ = set_device_last_sync_at(app_handle, &synced_at);
        }
        Err(err) => {
            runtime.connected = false;
            runtime.last_error = Some(err.clone());
            warn!(error = %err, "AIPP self-hosted sync failed");
        }
    }
    runtime.running = matches!(load_settings(app_handle).map(|s| s.mode), Ok(SyncMode::SelfHosted));
    runtime.syncing = false;
    drop(runtime);
    emit_status_changed(app_handle).await;
}

async fn run_sync_once_inner(app_handle: &AppHandle) -> Result<(), String> {
    ensure_sync_db(app_handle)?;
    let settings = load_settings(app_handle)?;
    if settings.mode != SyncMode::SelfHosted {
        return Ok(());
    }
    let token = settings
        .token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "缺少同步 token，请先保存自建同步配置".to_string())?;
    let device_id = {
        let conn = open_sync_db(app_handle)?;
        let device_id = ensure_device_id(&conn)?;
        if needs_reset(&conn)? {
            return Err(
                "检测到同步服务器或账号变更，请在设置中确认重置同步状态后再同步".to_string()
            );
        }
        device_id
    };
    let device_name = device_name();
    reset_pushing_events(app_handle)?;
    let remote_status = fetch_remote_status(&settings.server_url, token, &device_id).await?;

    let local_empty = is_local_sync_scope_empty(app_handle)?;
    if remote_status.remote_empty && !local_empty {
        let queued = scan_local_changes(app_handle)?;
        if queued > 0 {
            emit_status_changed(app_handle).await;
        }
        push_pending(app_handle, &settings, token, &device_id, &device_name).await?;
        return Ok(());
    }

    if !remote_status.remote_empty && local_empty {
        pull_remote(app_handle, &settings, token, &device_id).await?;
        return Ok(());
    }

    let queued = scan_local_changes(app_handle)?;
    if queued > 0 {
        emit_status_changed(app_handle).await;
    }
    push_pending(app_handle, &settings, token, &device_id, &device_name).await?;
    pull_remote(app_handle, &settings, token, &device_id).await?;
    Ok(())
}

async fn fetch_remote_status(
    server_url: &str,
    token: &str,
    device_id: &str,
) -> Result<RemoteStatus, String> {
    let url = format!("{server_url}/v1/sync/status");
    let response = sync_http_client()?
        .get(url)
        .bearer_auth(token)
        .header("X-AIPP-Device-ID", device_id)
        .send()
        .await
        .map_err(|e| format!("连接同步服务器失败: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("同步服务器 status 返回 {}", response.status()));
    }
    response.json::<RemoteStatus>().await.map_err(|e| e.to_string())
}

async fn push_pending(
    app_handle: &AppHandle,
    settings: &SyncSettings,
    token: &str,
    device_id: &str,
    device_name: &str,
) -> Result<(), String> {
    let mut total_conflict_resolved = 0usize;
    let result =
        push_pending_loop(app_handle, settings, token, device_id, device_name, &mut total_conflict_resolved).await;
    if total_conflict_resolved > 0 {
        if let Some(state) = app_handle.try_state::<SyncState>() {
            let mut runtime = state.status.lock().await;
            runtime.conflict_resolved_count += total_conflict_resolved as i64;
        }
        emit_status_changed(app_handle).await;
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn push_pending_loop(
    app_handle: &AppHandle,
    settings: &SyncSettings,
    token: &str,
    device_id: &str,
    device_name: &str,
    total_conflict_resolved: &mut usize,
) -> Result<(), String> {
    loop {
        let events = load_pending_outbox(app_handle, 100)?;
        if events.is_empty() {
            return Ok(());
        }
        mark_events_pushing(app_handle, &events)?;
        let request = PushRequest {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            events: events.iter().map(|(_, event)| event.clone()).collect(),
        };
        let url = format!("{}/v1/sync/push", settings.server_url);
        emit_status_changed(app_handle).await;
        let response = match sync_http_client()?
            .post(url)
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                mark_events_failed(app_handle, &events, &err.to_string())?;
                return Err(format!("推送同步变更失败: {err}"));
            }
        };
        if !response.status().is_success() {
            mark_events_failed(app_handle, &events, &format!("HTTP {}", response.status()))?;
            return Err(format!("同步服务器 push 返回 {}", response.status()));
        }
        let body = match response.json::<PushResponse>().await {
            Ok(body) => body,
            Err(err) => {
                mark_events_failed(app_handle, &events, &err.to_string())?;
                return Err(err.to_string());
            }
        };
        *total_conflict_resolved += handle_push_response(app_handle, &events, body)?;
        emit_status_changed(app_handle).await;
    }
}

async fn pull_remote(
    app_handle: &AppHandle,
    settings: &SyncSettings,
    token: &str,
    device_id: &str,
) -> Result<(), String> {
    loop {
        let cursor = get_cursor(app_handle)?;
        let url = format!("{}/v1/sync/pull?cursor={cursor}&limit=500", settings.server_url);
        let response = sync_http_client()?
            .get(url)
            .bearer_auth(token)
            .header("X-AIPP-Device-ID", device_id)
            .send()
            .await
            .map_err(|e| format!("拉取同步变更失败: {e}"))?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(());
        }
        if !response.status().is_success() {
            return Err(format!("同步服务器 pull 返回 {}", response.status()));
        }
        let body = response.json::<PullResponse>().await.map_err(|e| e.to_string())?;
        // 每页复用同一个 sync.db 连接应用所有变更
        let sync_conn = open_sync_db(app_handle)?;
        let mut deferred: Vec<&PullChange> = Vec::new();
        for change in &body.changes {
            if let Err(err) = apply_change(app_handle, &sync_conn, change, device_id) {
                if is_dependency_pending_error(&err) {
                    // 依赖对象可能在本页后面的 change 里：先记下，本页处理完后重放一次
                    deferred.push(change);
                } else {
                    record_dead_letter(&sync_conn, change, &err);
                }
            }
        }
        for change in deferred {
            if let Err(err) = apply_change(app_handle, &sync_conn, change, device_id) {
                record_dead_letter(&sync_conn, change, &err);
            }
        }
        drop(sync_conn);
        set_cursor(app_handle, body.cursor)?;
        emit_sync_domain_events(app_handle, body.changes.iter().map(|c| c.object_type.as_str()));
        emit_status_changed(app_handle).await;
        if !body.has_more {
            // pull 全部完成后自愈重放一次存量死信：跨页的依赖此时已就位
            if let Err(err) = replay_dead_letters(app_handle, device_id) {
                warn!(error = %err, "Replay sync dead letters after pull failed");
            }
            return Ok(());
        }
    }
}

/// pull/冲突落地后按 object_type 域通知前端刷新对应列表。
/// 复用既有事件约定：assistant_list_changed / mcp_state_changed；
/// llm 域新增 llm_provider_changed（前端 LLMProviderConfig 监听）。
fn emit_sync_domain_events<'a>(
    app_handle: &AppHandle,
    object_types: impl Iterator<Item = &'a str>,
) {
    let mut has_assistant = false;
    let mut has_mcp = false;
    let mut has_llm = false;
    for object_type in object_types {
        if object_type.starts_with("assistant") {
            has_assistant = true;
        } else if object_type.starts_with("mcp.") {
            has_mcp = true;
        } else if object_type.starts_with("llm.") {
            has_llm = true;
        }
    }
    if has_assistant {
        let _ = app_handle.emit("assistant_list_changed", ());
    }
    if has_mcp {
        let _ = app_handle.emit("mcp_state_changed", "sync_remote");
    }
    if has_llm {
        let _ = app_handle.emit("llm_provider_changed", ());
    }
}

/// 判断 apply 失败是否属于"依赖对象尚未同步"类错误（可等后续变更就位后重试）。
fn is_dependency_pending_error(err: &str) -> bool {
    err.starts_with("缺少同步依赖") || err.starts_with("远端变更缺少外键引用")
}

/// 将无法应用的远端变更写入死信表；同一对象只保留最新一条。
/// 死信写入本身失败只记日志，不阻断 pull 主流程。
fn record_dead_letter(conn: &Connection, change: &PullChange, error: &str) {
    let result = (|| -> Result<(), String> {
        let change_json =
            serde_json::to_string(change).map_err(|e| format!("serialize dead letter: {e}"))?;
        conn.execute(
            "DELETE FROM sync_dead_letter WHERE object_type = ?1 AND object_id = ?2",
            params![change.object_type, change.object_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO sync_dead_letter
             (object_type, object_id, operation, change_json, error, failed_at, retry_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![
                change.object_type,
                change.object_id,
                change.operation,
                change_json,
                error,
                now_string(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(err) = result {
        warn!(
            object_type = %change.object_type,
            object_id = %change.object_id,
            error = %err,
            "Failed to record sync dead letter"
        );
    }
    warn!(
        object_type = %change.object_type,
        object_id = %change.object_id,
        error = %error,
        "Remote change moved to sync dead letter"
    );
}

/// 按序重放死信表中的变更：成功则删除，失败则更新错误信息并保留。
/// 返回（成功数，剩余数）。
fn replay_dead_letters(app_handle: &AppHandle, device_id: &str) -> Result<(usize, usize), String> {
    ensure_sync_db(app_handle)?;
    let conn = open_sync_db(app_handle)?;
    let pending: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, change_json FROM sync_dead_letter ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    if pending.is_empty() {
        return Ok((0, 0));
    }
    let mut resolved = 0usize;
    for (id, change_json) in &pending {
        let change: PullChange = match serde_json::from_str(change_json) {
            Ok(change) => change,
            Err(err) => {
                update_dead_letter_failure(&conn, *id, &format!("死信反序列化失败: {err}"))?;
                continue;
            }
        };
        match apply_change(app_handle, &conn, &change, device_id) {
            Ok(()) => {
                conn.execute("DELETE FROM sync_dead_letter WHERE id = ?1", params![id])
                    .map_err(|e| e.to_string())?;
                resolved += 1;
            }
            Err(err) => {
                update_dead_letter_failure(&conn, *id, &err)?;
            }
        }
    }
    let remaining = pending.len() - resolved;
    if resolved > 0 {
        debug!(resolved, remaining, "Replayed sync dead letters");
    }
    Ok((resolved, remaining))
}

fn update_dead_letter_failure(conn: &Connection, id: i64, error: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE sync_dead_letter
         SET error = ?2, failed_at = ?3, retry_count = retry_count + 1
         WHERE id = ?1",
        params![id, error, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn sync_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())
}

fn sync_notify_http_client() -> Result<reqwest::Client, String> {
    // SSE streams outlive the normal request timeout; only bound connect.
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(NOTIFY_CONNECT_TIMEOUT_SECS))
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())
}

/// Subscribe to `GET /v1/sync/notify` and wake the poll worker when the account cursor advances.
async fn run_notify_listener(app_handle: AppHandle) {
    loop {
        if !matches!(load_settings(&app_handle).map(|s| s.mode), Ok(SyncMode::SelfHosted)) {
            break;
        }
        match listen_notify_once(&app_handle).await {
            Ok(()) => {
                debug!("Sync notify stream ended; reconnecting");
            }
            Err(err) => {
                debug!(error = %err, "Sync notify listener error; will retry");
            }
        }
        tokio::time::sleep(Duration::from_secs(NOTIFY_RECONNECT_SECS)).await;
    }
}

async fn listen_notify_once(app_handle: &AppHandle) -> Result<(), String> {
    let settings = load_settings(app_handle)?;
    if settings.mode != SyncMode::SelfHosted {
        return Ok(());
    }
    let token = settings
        .token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "missing sync token".to_string())?;
    let device_id = {
        let conn = open_sync_db(app_handle)?;
        ensure_device_id(&conn)?
    };
    let cursor = get_cursor(app_handle).unwrap_or(0);
    let url = format!(
        "{}/v1/sync/notify?cursor={}",
        settings.server_url.trim_end_matches('/'),
        cursor
    );
    let response = sync_notify_http_client()?
        .get(url)
        .bearer_auth(token)
        .header("X-AIPP-Device-ID", &device_id)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| format!("notify connect failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("notify HTTP {}", response.status()));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("notify read failed: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = buffer.find("

") {
            let raw = buffer[..idx].to_string();
            buffer = buffer[idx + 2..].to_string();
            if let Some(cursor_hint) = parse_sse_sync_cursor(&raw) {
                debug!(cursor = cursor_hint, "Sync notify received remote cursor");
                if let Some(state) = app_handle.try_state::<SyncState>() {
                    state.wake.notify_one();
                }
                // Server recycles after one actionable event; reconnect with fresh cursor.
                return Ok(());
            }
        }
    }
    Ok(())
}

fn parse_sse_sync_cursor(block: &str) -> Option<i64> {
    let mut event_name: Option<&str> = None;
    let mut data_line: Option<&str> = None;
    for line in block.lines() {
        if line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_line = Some(rest.trim());
        }
    }
    if event_name != Some("sync") {
        return None;
    }
    let data = data_line?;
    let value: JsonValue = serde_json::from_str(data).ok()?;
    value.get("cursor")?.as_i64()
}

fn scan_local_changes(app_handle: &AppHandle) -> Result<usize, String> {
    ensure_sync_db(app_handle)?;
    // 整轮扫描复用同一个 sync.db 连接，避免每行/每对象都重开连接
    let sync_conn = open_sync_db(app_handle)?;
    let device_id = ensure_device_id(&sync_conn)?;
    let mut enqueued = 0;
    for spec in specs() {
        let (snapshots, present_ids) = read_local_snapshots(app_handle, &spec, &sync_conn)?;
        for snapshot in snapshots {
            if enqueue_snapshot_if_changed(&sync_conn, &snapshot, &device_id)? {
                enqueued += 1;
            }
        }
        enqueued += scan_local_deletes(app_handle, &sync_conn, &spec, &present_ids)?;
    }
    Ok(enqueued)
}

/// 删除检测：shadow 中已同步（非墓碑）但本轮快照里不存在的对象，
/// 确认本地行确实不存在后入队 delete 事件。
/// 同时清理"本地行已删除但 outbox 里还留着 upsert"的过期事件。
fn scan_local_deletes(
    app_handle: &AppHandle,
    sync_conn: &Connection,
    spec: &SyncTableSpec,
    present_ids: &HashSet<String>,
) -> Result<usize, String> {
    let device_id = ensure_device_id(sync_conn)?;
    let mut enqueued = 0;

    let candidates = find_deleted_candidates(sync_conn, spec.object_type, present_ids)?;
    for (object_id, base_version) in candidates {
        // 行可能只是被 where_clause 过滤出同步范围，必须确认行真的不存在
        if local_row_exists(app_handle, sync_conn, spec, &object_id)? {
            continue;
        }
        if enqueue_delete_event(sync_conn, spec, &object_id, base_version, &device_id)? {
            enqueued += 1;
        }
    }

    // 新增后未推送即删除：没有 shadow，但 outbox 可能残留 pending/failed upsert，
    // 不清理的话会把已删除的对象推上服务器（幽灵复活）。
    let stale_upserts = find_stale_pending_upserts(sync_conn, spec.object_type, present_ids)?;
    for object_id in stale_upserts {
        if local_row_exists(app_handle, sync_conn, spec, &object_id)? {
            continue;
        }
        debug!(
            object_type = %spec.object_type,
            object_id = %object_id,
            "Dropping stale pending upsert for locally deleted row"
        );
        sync_conn
            .execute(
            "DELETE FROM sync_outbox
             WHERE object_type = ?1 AND object_id = ?2 AND operation = 'upsert'
               AND status IN ('pending', 'failed')",
            params![spec.object_type, object_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(enqueued)
}

/// shadow 中已同步且非墓碑、但不在 present_ids 中的对象，返回 (object_id, base_version)。
fn find_deleted_candidates(
    conn: &Connection,
    object_type: &str,
    present_ids: &HashSet<String>,
) -> Result<Vec<(String, i64)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT object_id, server_version FROM sync_shadow
             WHERE object_type = ?1 AND deleted_at IS NULL",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![object_type], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (object_id, server_version) = row.map_err(|e| e.to_string())?;
        if !present_ids.contains(&object_id) {
            result.push((object_id, server_version));
        }
    }
    Ok(result)
}

/// outbox 中 pending/failed 的 upsert 事件里，object_id 不在 present_ids 中的条目。
fn find_stale_pending_upserts(
    conn: &Connection,
    object_type: &str,
    present_ids: &HashSet<String>,
) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT object_id FROM sync_outbox
             WHERE object_type = ?1 AND operation = 'upsert'
               AND status IN ('pending', 'failed')",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![object_type], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let object_id = row.map_err(|e| e.to_string())?;
        if !present_ids.contains(&object_id) {
            result.push(object_id);
        }
    }
    Ok(result)
}

/// 确认本地行是否仍然存在（忽略 where_clause 的同步范围过滤）。
fn local_row_exists(
    app_handle: &AppHandle,
    sync_conn: &Connection,
    spec: &SyncTableSpec,
    object_id: &str,
) -> Result<bool, String> {
    let path = get_db_path(app_handle, spec.db_name)?;
    let conn = open_data_db(&path)?;
    if spec.natural_key_columns.is_empty() {
        let Some(local_id) = find_local_id_by_object_id(sync_conn, spec.object_type, object_id)?
        else {
            return Ok(false);
        };
        let found = conn
            .query_row(
                &format!("SELECT 1 FROM {} WHERE id = ?1", spec.table),
                params![local_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        return Ok(found.is_some());
    }
    let values = natural_values_from_object_id(object_id)?;
    let where_clause = spec
        .natural_key_columns
        .iter()
        .map(|column| format!("{column} = ?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let found = conn
        .query_row(
            &format!("SELECT 1 FROM {} WHERE {}", spec.table, where_clause),
            params_from_iter(values.iter()),
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(found.is_some())
}

/// 入队 delete 事件。已有未完成的 delete 事件时去重；
/// 同对象的 pending/failed upsert 一并移除（行已删除，不应再上传）。
fn enqueue_delete_event(
    conn: &Connection,
    spec: &SyncTableSpec,
    object_id: &str,
    base_version: i64,
    device_id: &str,
) -> Result<bool, String> {
    let pending_delete: Option<i64> = conn
        .query_row(
            "SELECT id FROM sync_outbox
             WHERE object_type = ?1 AND object_id = ?2 AND operation = 'delete'
               AND status IN ('pending', 'pushing', 'failed')
             LIMIT 1",
            params![spec.object_type, object_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if pending_delete.is_some() {
        return Ok(false);
    }
    conn.execute(
        "DELETE FROM sync_outbox
         WHERE object_type = ?1 AND object_id = ?2 AND operation = 'upsert'
           AND status IN ('pending', 'failed')",
        params![spec.object_type, object_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO sync_outbox
         (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
         VALUES (?1, ?2, ?3, 'delete', NULL, ?4, ?5, ?6, ?7, 'pending')",
        params![
            Uuid::new_v4().to_string(),
            spec.object_type,
            object_id,
            base_version,
            Utc::now().timestamp_millis(),
            device_id,
            now_string(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

fn read_local_snapshots(
    app_handle: &AppHandle,
    spec: &SyncTableSpec,
    sync_conn: &Connection,
) -> Result<(Vec<LocalObjectSnapshot>, HashSet<String>), String> {
    let path = get_db_path(app_handle, spec.db_name)?;
    let conn = open_data_db(&path)?;
    let columns = existing_columns(&conn, spec.table, spec.columns)?;
    if columns.is_empty() || !columns.iter().any(|c| c == "id") {
        return Ok((Vec::new(), HashSet::new()));
    }
    let sql = format!(
        "SELECT {} FROM {} {} ORDER BY {}",
        columns.join(", "),
        spec.table,
        spec.where_clause.map(|w| format!("WHERE {w}")).unwrap_or_default(),
        spec.order_by
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut snapshots = Vec::new();
    let mut present_ids = HashSet::new();
    // 跨库外键（如 message.llm_model_id → llm.model）在本表扫描内缓存连接
    let mut dep_conns: HashMap<&'static str, Connection> = HashMap::new();

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut fields = Map::new();
        let mut local_id = None;
        let mut raw_values: HashMap<String, JsonValue> = HashMap::new();

        for (index, column) in columns.iter().enumerate() {
            let value = json_from_value_ref(row.get_ref(index).map_err(|e| e.to_string())?);
            if column == "id" {
                local_id = value.as_i64();
            } else {
                fields.insert(column.clone(), value.clone());
            }
            raw_values.insert(column.clone(), value);
        }

        let local_id = local_id.ok_or_else(|| format!("{} row missing id", spec.table))?;
        let is_official_row = spec.seed_official_ids.contains(&local_id)
            || spec
                .official_column
                .and_then(|column| raw_values.get(column))
                .and_then(JsonValue::as_i64)
                .is_some_and(|value| value != 0);
        let object_id = if !spec.natural_key_columns.is_empty() {
            natural_object_id(spec, &raw_values)?
        } else if is_official_row {
            // 官方/内置种子行：确定性 object_id，各设备对同一内置行生成相同标识
            upsert_official_object_map(sync_conn, spec.object_type, spec.table, local_id)?
        } else if let Some(child_id) =
            try_deterministic_child_object_id(sync_conn, spec, &raw_values)?
        {
            // 内置父行下的子配置（tool/resource/prompt/model…）：确定性 id，避免多端各推 UUID
            remap_sync_object_map(sync_conn, spec.object_type, spec.table, local_id, &child_id)?;
            child_id
        } else {
            ensure_object_id(sync_conn, spec.object_type, spec.table, local_id)?
        };
        // 无论本行是否因外键未决而跳过，都要计入 present 集合，
        // 否则删除检测会把"暂时跳过"误判成"已删除"。
        present_ids.insert(object_id.clone());

        let Some(refs) = build_fk_refs(spec, &raw_values, &mut fields, |object_type, value| {
            if let Some(sync_id) = find_object_id(sync_conn, object_type, value)? {
                return Ok(FkLookup::Resolved(sync_id));
            }
            // 未入 map：区分「依赖行还在，只是本轮未映射」与「悬空外键」
            if dependency_row_exists(app_handle, object_type, value, &mut dep_conns)? {
                Ok(FkLookup::Pending)
            } else {
                Ok(FkLookup::Dangling)
            }
        })?
        else {
            // 外键指向的依赖行存在但尚未建立映射：本轮跳过该行，
            // 绝不把本地 rowid 作为外键值推送出去。下轮扫描会自然重试。
            debug!(
                object_type = %spec.object_type,
                local_id,
                "Skipping sync snapshot with unresolved foreign key"
            );
            continue;
        };

        let payload = json!({ "fields": fields, "refs": refs });
        let payload_json =
            serde_json::to_string(&payload).map_err(|e| format!("serialize payload: {e}"))?;
        let payload_hash = hash_text(&payload_json);
        snapshots.push(LocalObjectSnapshot {
            object_type: spec.object_type.to_string(),
            object_id,
            payload_json,
            payload_hash,
        });
    }
    Ok((snapshots, present_ids))
}

/// 外键本地解析结果：区分「已映射 / 依赖未入 map / 悬空」。
#[derive(Debug, Clone, PartialEq, Eq)]
enum FkLookup {
    /// sync_object_map 中已有同步 object_id
    Resolved(String),
    /// 依赖行本地存在，但尚未进入 sync_object_map —— 本轮跳过，下轮重试
    Pending,
    /// 依赖行本地不存在（悬空外键）—— 按 NULL 处理，避免永久卡住
    Dangling,
}

/// 构建一行的外键引用表。
///
/// 返回 `Ok(Some(refs))` 表示可推送：已解析的外键写入 refs；悬空外键从
/// `fields` 置为 NULL 且不进入 refs。
/// 返回 `Ok(None)` 表示存在「行在但尚未映射」的依赖，调用方必须跳过该行，
/// 避免把本地 rowid 作为外键值泄露到 payload 中。
fn build_fk_refs(
    spec: &SyncTableSpec,
    raw_values: &HashMap<String, JsonValue>,
    fields: &mut Map<String, JsonValue>,
    mut resolve: impl FnMut(&str, i64) -> Result<FkLookup, String>,
) -> Result<Option<Map<String, JsonValue>>, String> {
    let mut refs = Map::new();
    for fk in spec.foreign_keys {
        if let Some(value) = raw_values.get(fk.column).and_then(JsonValue::as_i64) {
            // provider_id=0 是「未选 provider」哨兵，不是真实 llm.provider 行；保留 0，不进 refs。
            if is_provider_id_unset_sentinel(fk, value) {
                fields.insert(fk.column.to_string(), JsonValue::Number(0.into()));
                continue;
            }
            match resolve(fk.object_type, value)? {
                FkLookup::Resolved(ref_id) => {
                    refs.insert(fk.column.to_string(), JsonValue::String(ref_id));
                }
                FkLookup::Pending => return Ok(None),
                FkLookup::Dangling => {
                    // 悬空外键：当作 NULL 推送，不永久卡住同步
                    fields.insert(fk.column.to_string(), JsonValue::Null);
                    debug!(
                        object_type = %spec.object_type,
                        column = fk.column,
                        local_fk = value,
                        target = fk.object_type,
                        "Omitting dangling foreign key from sync snapshot"
                    );
                }
            }
        }
    }
    Ok(Some(refs))
}

fn is_provider_id_unset_sentinel(fk: &ForeignKeySpec, value: i64) -> bool {
    value == 0 && fk.column == "provider_id" && fk.object_type == "llm.provider"
}

/// 检查外键指向的依赖行是否在本地业务库中真实存在。
///
/// 按目标 `db_name` 缓存只读连接；不复用正在扫描的 `conn`，避免与行游标争用借用。
fn dependency_row_exists(
    app_handle: &AppHandle,
    object_type: &str,
    local_id: i64,
    dep_conns: &mut HashMap<&'static str, Connection>,
) -> Result<bool, String> {
    let Some(dep_spec) = specs().into_iter().find(|s| s.object_type == object_type) else {
        // 未知 object_type：无法核实行是否存在，按悬空处理以免永久卡住
        return Ok(false);
    };
    if !dep_conns.contains_key(dep_spec.db_name) {
        let path = get_db_path(app_handle, dep_spec.db_name)?;
        let conn = open_data_db(&path)?;
        dep_conns.insert(dep_spec.db_name, conn);
    }
    let dep_conn = dep_conns
        .get(dep_spec.db_name)
        .expect("dep conn just inserted");
    row_exists_by_id(dep_conn, &dep_spec, local_id)
}

/// 用 refs 中的同步 object_id 外键引用覆盖 fields 里的值。
///
/// 外键在 fields 中有非空值但 refs 缺失时返回错误——拒绝把来源设备
/// 的本地 rowid 写进本机数据库（那会指向本机完全无关的行）。
fn resolve_remote_foreign_keys(
    spec: &SyncTableSpec,
    field_values: &mut Map<String, JsonValue>,
    refs: &Map<String, JsonValue>,
    mut resolve_local_id: impl FnMut(&str, &str) -> Result<Option<i64>, String>,
) -> Result<(), String> {
    for fk in spec.foreign_keys {
        if let Some(JsonValue::String(ref_object_id)) = refs.get(fk.column) {
            let local_id = resolve_local_id(fk.object_type, ref_object_id)?.ok_or_else(|| {
                format!(
                    "缺少同步依赖: {}.{} -> {}",
                    spec.object_type, fk.column, ref_object_id
                )
            })?;
            field_values.insert(fk.column.to_string(), JsonValue::Number(local_id.into()));
        } else if is_provider_id_unset_sentinel(fk, 0) {
            // 哨兵列：refs 缺失时保留 0；旧客户端推上来的 NULL 也回落为 0（NOT NULL 列）
            let keep_zero = field_values
                .get(fk.column)
                .map(|v| v.is_null() || v.as_i64() == Some(0))
                .unwrap_or(true);
            if keep_zero {
                field_values.insert(fk.column.to_string(), JsonValue::Number(0.into()));
            } else if field_values.get(fk.column).and_then(JsonValue::as_i64).is_some() {
                return Err(format!(
                    "远端变更缺少外键引用: {}.{}（refs 缺失，拒绝写入原始本地 id）",
                    spec.object_type, fk.column
                ));
            }
        } else if field_values
            .get(fk.column)
            .and_then(JsonValue::as_i64)
            .is_some()
        {
            return Err(format!(
                "远端变更缺少外键引用: {}.{}（refs 缺失，拒绝写入原始本地 id）",
                spec.object_type, fk.column
            ));
        }
    }
    Ok(())
}

fn enqueue_snapshot_if_changed(
    conn: &Connection,
    snapshot: &LocalObjectSnapshot,
    device_id: &str,
) -> Result<bool, String> {
    let shadow = conn
        .query_row(
            "SELECT server_version, payload_hash FROM sync_shadow WHERE object_type = ?1 AND object_id = ?2",
            params![snapshot.object_type, snapshot.object_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if shadow.as_ref().is_some_and(|(_, hash)| hash == &snapshot.payload_hash) {
        return Ok(false);
    }

    let pending_same: Option<i64> = conn
        .query_row(
            "SELECT id FROM sync_outbox
             WHERE object_type = ?1 AND object_id = ?2 AND operation = 'upsert'
               AND payload_json = ?3 AND status IN ('pending', 'pushing', 'failed')
             LIMIT 1",
            params![snapshot.object_type, snapshot.object_id, snapshot.payload_json],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if pending_same.is_some() {
        return Ok(false);
    }

    // 新 payload 入队前清理同对象的 failed 旧事件，避免旧 payload 之后被重试覆盖新数据
    drop_failed_upserts_for_object(conn, &snapshot.object_type, &snapshot.object_id)?;

    let base_version = shadow.map(|(version, _)| version);
    let now = now_string();
    conn.execute(
        "INSERT INTO sync_outbox
         (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
         VALUES (?1, ?2, ?3, 'upsert', ?4, ?5, ?6, ?7, ?8, 'pending')",
        params![
            Uuid::new_v4().to_string(),
            snapshot.object_type,
            snapshot.object_id,
            snapshot.payload_json,
            base_version,
            Utc::now().timestamp_millis(),
            device_id,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 删除同对象残留的 failed upsert 事件（已被更新的本地数据取代）。
fn drop_failed_upserts_for_object(
    conn: &Connection,
    object_type: &str,
    object_id: &str,
) -> Result<usize, String> {
    conn.execute(
        "DELETE FROM sync_outbox
         WHERE object_type = ?1 AND object_id = ?2 AND operation = 'upsert' AND status = 'failed'",
        params![object_type, object_id],
    )
    .map_err(|e| e.to_string())
}

fn apply_change(
    app_handle: &AppHandle,
    sync_conn: &Connection,
    change: &PullChange,
    local_device_id: &str,
) -> Result<(), String> {
    if change.device_id == local_device_id {
        upsert_shadow_from_remote(sync_conn, change)?;
        return Ok(());
    }

    let Some(spec) = specs().into_iter().find(|s| s.object_type == change.object_type) else {
        warn!(object_type = %change.object_type, "Ignoring unknown sync object type");
        return Ok(());
    };
    let db_path = get_db_path(app_handle, spec.db_name)?;
    let conn = open_data_db(&db_path)?;

    if change.operation == "delete" {
        apply_delete(sync_conn, &conn, &spec, change)?;
        upsert_shadow_from_remote(sync_conn, change)?;
        return Ok(());
    }

    let payload = change
        .payload
        .as_ref()
        .ok_or_else(|| format!("remote change {} missing payload", change.event_id))?;
    let fields = payload
        .get("fields")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| "remote payload missing fields".to_string())?;
    let refs = payload.get("refs").and_then(JsonValue::as_object).cloned().unwrap_or_default();
    let mut field_values = fields.clone();
    resolve_remote_foreign_keys(&spec, &mut field_values, &refs, |object_type, ref_object_id| {
        find_local_id_by_object_id(sync_conn, object_type, ref_object_id)
    })?;

    if let Some(local_id) = find_local_id_by_object_id(sync_conn, &change.object_type, &change.object_id)? {
        update_row(&conn, &spec, local_id, &field_values)?;
    } else if let Some(official_id) = parse_official_object_id(&change.object_id, &change.object_type) {
        // 官方/内置种子行按确定性 id 对齐：本机已有同 id 行则更新，
        // 没有则按固定 id 插入，保证多端收敛到同一行。
        if row_exists_by_id(&conn, &spec, official_id)? {
            update_row(&conn, &spec, official_id, &field_values)?;
        } else {
            insert_row_with_id(&conn, &spec, official_id, &field_values)?;
        }
        remap_sync_object_map(sync_conn, &change.object_type, spec.table, official_id, &change.object_id)?;
    } else if !spec.natural_key_columns.is_empty() {
        upsert_natural_row(&conn, &spec, &field_values)?;
    } else if !spec.unique_key_columns.is_empty() {
        // UNIQUE / 种子软身份：合并到已有行，远端 sync_id 获胜，避免 UNIQUE 死信
        upsert_by_unique_key(sync_conn, &conn, &spec, &change.object_id, &field_values)?;
    } else {
        // New insert: conversation.message may need concurrent-continue auto-fork.
        if change.object_type == "conversation.message" {
            maybe_fork_before_message_insert(app_handle, sync_conn, &conn, &mut field_values)?;
        }
        match insert_row(&conn, &spec, &field_values) {
            Ok(new_id) => {
                remap_sync_object_map(sync_conn, &change.object_type, spec.table, new_id, &change.object_id)?;
            }
            Err(err) if is_unique_constraint_error(&err) && !spec.unique_key_columns.is_empty() => {
                upsert_by_unique_key(sync_conn, &conn, &spec, &change.object_id, &field_values)?;
            }
            Err(err) => return Err(err),
        }
    }

    upsert_shadow_from_remote(sync_conn, change)?;
    Ok(())
}

fn field_as_i64(fields: &Map<String, JsonValue>, key: &str) -> Option<i64> {
    fields.get(key).and_then(|value| match value {
        JsonValue::Number(n) => n.as_i64(),
        JsonValue::Null => None,
        _ => None,
    })
}

fn field_as_string(fields: &Map<String, JsonValue>, key: &str) -> Option<String> {
    fields.get(key).and_then(|value| match value {
        JsonValue::String(s) if !s.is_empty() => Some(s.clone()),
        JsonValue::Null => None,
        _ => None,
    })
}

fn message_sibling_exists(
    conn: &Connection,
    conversation_id: i64,
    parent_id: i64,
    exclude_message_id: Option<i64>,
) -> Result<bool, String> {
    let count: i64 = if let Some(exclude_id) = exclude_message_id {
        conn.query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1 AND parent_id = ?2 AND id != ?3",
            params![conversation_id, parent_id, exclude_id],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1 AND parent_id = ?2",
            params![conversation_id, parent_id],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

fn generation_group_exists(
    conn: &Connection,
    conversation_id: i64,
    generation_group_id: &str,
    exclude_message_id: Option<i64>,
) -> Result<bool, String> {
    let count: i64 = if let Some(exclude_id) = exclude_message_id {
        conn.query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1 AND generation_group_id = ?2 AND id != ?3",
            params![conversation_id, generation_group_id, exclude_id],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1 AND generation_group_id = ?2",
            params![conversation_id, generation_group_id],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

/// Local has another generation_group that shares `parent_group_id` but is not
/// `exclude_generation_group_id` (sibling version / concurrent regenerate).
fn local_sibling_under_parent_group(
    conn: &Connection,
    conversation_id: i64,
    parent_group_id: &str,
    exclude_generation_group_id: Option<&str>,
    exclude_message_id: Option<i64>,
) -> Result<bool, String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1
               AND parent_group_id = ?2
               AND (?3 IS NULL OR generation_group_id IS NULL OR generation_group_id != ?3)
               AND (?4 IS NULL OR id != ?4)",
            params![
                conversation_id,
                parent_group_id,
                exclude_generation_group_id,
                exclude_message_id,
            ],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

/// Latest finalized tip generation_group_id (prefer response/assistant with finish_time,
/// else any message with a generation_group_id), newest id wins.
fn local_tip_generation_group_id(
    conn: &Connection,
    conversation_id: i64,
    exclude_message_id: Option<i64>,
) -> Result<Option<String>, String> {
    let sql = if exclude_message_id.is_some() {
        "SELECT generation_group_id FROM message
         WHERE conversation_id = ?1
           AND generation_group_id IS NOT NULL
           AND id != ?2
           AND (message_type NOT IN ('response', 'assistant') OR finish_time IS NOT NULL)
         ORDER BY id DESC LIMIT 1"
    } else {
        "SELECT generation_group_id FROM message
         WHERE conversation_id = ?1
           AND generation_group_id IS NOT NULL
           AND (message_type NOT IN ('response', 'assistant') OR finish_time IS NOT NULL)
         ORDER BY id DESC LIMIT 1"
    };
    if let Some(exclude_id) = exclude_message_id {
        conn.query_row(sql, params![conversation_id, exclude_id], |row| row.get(0))
            .optional()
            .map_err(|e| e.to_string())
    } else {
        conn.query_row(sql, params![conversation_id], |row| row.get(0))
            .optional()
            .map_err(|e| e.to_string())
    }
}

fn conversation_has_unfinished_response(
    conn: &Connection,
    conversation_id: i64,
    exclude_message_id: Option<i64>,
) -> Result<bool, String> {
    let count: i64 = if let Some(exclude_id) = exclude_message_id {
        conn.query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1
               AND message_type IN ('response', 'assistant')
               AND finish_time IS NULL
               AND id != ?2",
            params![conversation_id, exclude_id],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(1) FROM message
             WHERE conversation_id = ?1
               AND message_type IN ('response', 'assistant')
               AND finish_time IS NULL",
            params![conversation_id],
            |row| row.get(0),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(count > 0)
}

/// True when conversation C has at least one mapped message whose sync_id is not
/// yet in sync_shadow (local continue not confirmed by server).
fn local_has_unconfirmed_tip(
    data_conn: &Connection,
    sync_conn: &Connection,
    conversation_id: i64,
    exclude_message_id: Option<i64>,
) -> Result<bool, String> {
    let mut stmt = data_conn
        .prepare(
            "SELECT id FROM message WHERE conversation_id = ?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map(params![conversation_id], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    for id in ids {
        if exclude_message_id == Some(id) {
            continue;
        }
        let sync_id: Option<String> = sync_conn
            .query_row(
                "SELECT sync_id FROM sync_object_map
                 WHERE object_type = 'conversation.message' AND local_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(sync_id) = sync_id else {
            // Mapped on scan; unmapped local rows are still a local tip.
            return Ok(true);
        };
        let shadowed: bool = sync_conn
            .query_row(
                "SELECT 1 FROM sync_shadow
                 WHERE object_type = 'conversation.message' AND object_id = ?1
                   AND deleted_at IS NULL",
                params![sync_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(false);
        if !shadowed {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Last message id that is confirmed in sync_shadow — the common-history tip.
/// Used as fork_at so C' copies shared history only, not the local divergent tip.
fn common_history_tip_message_id(
    data_conn: &Connection,
    sync_conn: &Connection,
    conversation_id: i64,
    exclude_message_id: Option<i64>,
) -> Result<Option<i64>, String> {
    let mut stmt = data_conn
        .prepare(
            "SELECT id FROM message WHERE conversation_id = ?1 ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map(params![conversation_id], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut last_common = None;
    for id in ids {
        if exclude_message_id == Some(id) {
            continue;
        }
        let sync_id: Option<String> = sync_conn
            .query_row(
                "SELECT sync_id FROM sync_object_map
                 WHERE object_type = 'conversation.message' AND local_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(sync_id) = sync_id else {
            break;
        };
        let shadowed: bool = sync_conn
            .query_row(
                "SELECT 1 FROM sync_shadow
                 WHERE object_type = 'conversation.message' AND object_id = ?1
                   AND deleted_at IS NULL",
                params![sync_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(false);
        if shadowed {
            last_common = Some(id);
        } else {
            break;
        }
    }
    Ok(last_common)
}

fn last_message_id_in_generation_group(
    conn: &Connection,
    conversation_id: i64,
    generation_group_id: &str,
) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT id FROM message
         WHERE conversation_id = ?1 AND generation_group_id = ?2
         ORDER BY id DESC LIMIT 1",
        params![conversation_id, generation_group_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn gather_divergence_input(
    data_conn: &Connection,
    sync_conn: &Connection,
    field_values: &Map<String, JsonValue>,
    exclude_message_id: Option<i64>,
    is_new_insert: bool,
) -> Result<divergence::DivergenceInput, String> {
    let conversation_id = field_as_i64(field_values, "conversation_id")
        .ok_or_else(|| "conversation.message missing conversation_id".to_string())?;
    let remote_parent_id = field_as_i64(field_values, "parent_id");
    let remote_generation_group_id = field_as_string(field_values, "generation_group_id");
    let remote_parent_group_id = field_as_string(field_values, "parent_group_id");

    let sibling_exists_under_parent_id = match remote_parent_id {
        Some(p) => message_sibling_exists(data_conn, conversation_id, p, exclude_message_id)?,
        None => false,
    };
    let remote_generation_group_already_local = match remote_generation_group_id.as_deref() {
        Some(g) => generation_group_exists(data_conn, conversation_id, g, exclude_message_id)?,
        None => false,
    };
    let local_has_sibling_under_parent_group = match remote_parent_group_id.as_deref() {
        Some(pg) => local_sibling_under_parent_group(
            data_conn,
            conversation_id,
            pg,
            remote_generation_group_id.as_deref(),
            exclude_message_id,
        )?,
        None => false,
    };
    let tip_group =
        local_tip_generation_group_id(data_conn, conversation_id, exclude_message_id)?;
    let runtime_running =
        conversation_has_unfinished_response(data_conn, conversation_id, exclude_message_id)?;
    let unconfirmed_tip =
        local_has_unconfirmed_tip(data_conn, sync_conn, conversation_id, exclude_message_id)?;

    Ok(divergence::DivergenceInput {
        is_new_insert,
        remote_parent_id,
        remote_generation_group_id,
        remote_parent_group_id,
        sibling_exists_under_parent_id,
        local_has_sibling_under_parent_group,
        remote_generation_group_already_local,
        local_tip_generation_group_id: tip_group,
        local_runtime_running: runtime_running,
        local_has_unconfirmed_tip: unconfirmed_tip,
    })
}

fn resolve_sync_fork_point(
    data_conn: &Connection,
    sync_conn: &Connection,
    conversation_id: i64,
    remote_parent_id: Option<i64>,
    remote_parent_group_id: Option<&str>,
    exclude_message_id: Option<i64>,
) -> Result<Option<i64>, String> {
    if let Some(p) = remote_parent_id {
        return Ok(Some(p));
    }
    if let Some(pg) = remote_parent_group_id {
        if let Some(id) = last_message_id_in_generation_group(data_conn, conversation_id, pg)? {
            return Ok(Some(id));
        }
    }
    let common = common_history_tip_message_id(
        data_conn,
        sync_conn,
        conversation_id,
        exclude_message_id,
    )?;
    Ok(divergence::resolve_fork_at_message_id(None, common))
}

/// If inserting this remote message into C would diverge, fork C → C' and remap
/// `conversation_id` in `field_values` to C' so the insert lands on the fork.
fn maybe_fork_before_message_insert(
    app_handle: &AppHandle,
    sync_conn: &Connection,
    data_conn: &Connection,
    field_values: &mut Map<String, JsonValue>,
) -> Result<(), String> {
    let conversation_id = field_as_i64(field_values, "conversation_id")
        .ok_or_else(|| "conversation.message insert missing conversation_id".to_string())?;
    let input = gather_divergence_input(data_conn, sync_conn, field_values, None, true)?;
    if !divergence::detect_concurrent_message_divergence(&input) {
        return Ok(());
    }

    let fork_point = resolve_sync_fork_point(
        data_conn,
        sync_conn,
        conversation_id,
        input.remote_parent_id,
        input.remote_parent_group_id.as_deref(),
        None,
    )?;

    let forked_id = crate::api::conversation_api::fork_conversation_internal(
        app_handle,
        conversation_id,
        fork_point,
        crate::api::conversation_api::ForkNameStyle::SyncDivergence,
        false,
    )?;

    // Ensure the forked conversation has a sync_id so subsequent local scan uploads C'.
    let _ = ensure_object_id(sync_conn, "conversation", "conversation", forked_id)?;
    field_values.insert(
        "conversation_id".to_string(),
        JsonValue::Number(forked_id.into()),
    );
    schedule_sync_after_local_change(app_handle);
    debug!(
        original_conversation_id = conversation_id,
        forked_conversation_id = forked_id,
        fork_point = ?fork_point,
        remote_parent_id = ?input.remote_parent_id,
        remote_parent_group_id = ?input.remote_parent_group_id,
        "Sync concurrent divergence: forked conversation for remote message insert"
    );
    Ok(())
}

/// Conflict path: when remote-wins would overwrite a local tip that already diverged,
/// keep local tip and land the server payload on a fork (same §4.3 detection).
fn try_fork_message_conflict_instead_of_overwrite(
    app_handle: &AppHandle,
    sync_conn: &Connection,
    change: &PullChange,
) -> Result<bool, String> {
    if change.object_type != "conversation.message" || change.operation == "delete" {
        return Ok(false);
    }

    let Some(spec) = specs().into_iter().find(|s| s.object_type == change.object_type) else {
        return Ok(false);
    };
    let db_path = get_db_path(app_handle, spec.db_name)?;
    let data_conn = open_data_db(&db_path)?;

    let payload = match change.payload.as_ref() {
        Some(p) => p,
        None => return Ok(false),
    };
    let fields = match payload.get("fields").and_then(JsonValue::as_object) {
        Some(f) => f,
        None => return Ok(false),
    };
    let refs = payload.get("refs").and_then(JsonValue::as_object).cloned().unwrap_or_default();
    let mut field_values = fields.clone();
    resolve_remote_foreign_keys(&spec, &mut field_values, &refs, |object_type, ref_object_id| {
        find_local_id_by_object_id(sync_conn, object_type, ref_object_id)
    })?;

    let local_id = find_local_id_by_object_id(sync_conn, &change.object_type, &change.object_id)?;
    let conversation_id = match field_as_i64(&field_values, "conversation_id") {
        Some(id) => id,
        None => return Ok(false),
    };

    // Conflict is an update of an existing sync_id, but we evaluate divergence against
    // *other* local messages / tip state (exclude the mapped local row).
    let input = gather_divergence_input(&data_conn, sync_conn, &field_values, local_id, true)?;
    if !divergence::detect_concurrent_message_divergence(&input) {
        return Ok(false);
    }

    let fork_point = resolve_sync_fork_point(
        &data_conn,
        sync_conn,
        conversation_id,
        input.remote_parent_id,
        input.remote_parent_group_id.as_deref(),
        local_id,
    )?;

    let forked_id = crate::api::conversation_api::fork_conversation_internal(
        app_handle,
        conversation_id,
        fork_point,
        crate::api::conversation_api::ForkNameStyle::SyncDivergence,
        false,
    )?;
    let _ = ensure_object_id(sync_conn, "conversation", "conversation", forked_id)?;

    // Keep local tip: detach this sync_id from the local row so scan will re-id it,
    // then insert the server payload into the forked conversation under the server sync_id.
    if let Some(existing_local_id) = local_id {
        sync_conn
            .execute(
                "DELETE FROM sync_object_map WHERE object_type = ?1 AND sync_id = ?2",
                params![change.object_type, change.object_id],
            )
            .map_err(|e| e.to_string())?;
        // Ensure orphaned local tip gets a fresh sync identity on next scan.
        let _ = ensure_object_id(
            sync_conn,
            "conversation.message",
            "message",
            existing_local_id,
        )?;
    }

    field_values.insert(
        "conversation_id".to_string(),
        JsonValue::Number(forked_id.into()),
    );
    let new_id = insert_row(&data_conn, &spec, &field_values)?;
    save_object_map(
        sync_conn,
        &change.object_type,
        spec.table,
        new_id,
        &change.object_id,
    )?;
    upsert_shadow_from_remote(sync_conn, change)?;
    schedule_sync_after_local_change(app_handle);
    debug!(
        original_conversation_id = conversation_id,
        forked_conversation_id = forked_id,
        fork_point = ?fork_point,
        "Sync message conflict: kept local tip, applied server payload into fork"
    );
    Ok(true)
}

fn apply_delete(
    sync_conn: &Connection,
    conn: &Connection,
    spec: &SyncTableSpec,
    change: &PullChange,
) -> Result<(), String> {
    if let Some(local_id) = find_local_id_by_object_id(sync_conn, &change.object_type, &change.object_id)? {
        conn.execute(&format!("DELETE FROM {} WHERE id = ?1", spec.table), params![local_id])
            .map_err(|e| e.to_string())?;
        // 同步删除 object_map 映射：同 object_id 后续 upsert 时应重新 insert（对象复活），
        // 而不是命中这条指向已删除行的旧映射。
        sync_conn
            .execute(
                "DELETE FROM sync_object_map WHERE object_type = ?1 AND sync_id = ?2",
                params![change.object_type, change.object_id],
            )
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    if !spec.natural_key_columns.is_empty() {
        let values = natural_values_from_object_id(&change.object_id)?;
        let where_clause = spec
            .natural_key_columns
            .iter()
            .map(|column| format!("{column} = ?"))
            .collect::<Vec<_>>()
            .join(" AND ");
        conn.execute(
            &format!("DELETE FROM {} WHERE {}", spec.table, where_clause),
            params_from_iter(values.iter()),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn update_row(
    conn: &Connection,
    spec: &SyncTableSpec,
    local_id: i64,
    fields: &Map<String, JsonValue>,
) -> Result<(), String> {
    let columns = existing_columns(conn, spec.table, spec.columns)?;
    let mut set_columns = Vec::new();
    let mut values = Vec::new();
    for column in columns {
        if column == "id" {
            continue;
        }
        if let Some(value) = fields.get(&column) {
            set_columns.push(format!("{column} = ?"));
            values.push(sql_value_from_json(value));
        }
    }
    if set_columns.is_empty() {
        return Ok(());
    }
    values.push(Value::Integer(local_id));
    let sql = format!("UPDATE {} SET {} WHERE id = ?", spec.table, set_columns.join(", "));
    conn.execute(&sql, params_from_iter(values.iter())).map_err(|e| e.to_string())?;
    Ok(())
}

fn insert_row(conn: &Connection, spec: &SyncTableSpec, fields: &Map<String, JsonValue>) -> Result<i64, String> {
    insert_row_inner(conn, spec, None, fields)
}

/// 按指定 id 插入（官方/内置种子行的确定性 id 对齐用）。
fn insert_row_with_id(
    conn: &Connection,
    spec: &SyncTableSpec,
    id: i64,
    fields: &Map<String, JsonValue>,
) -> Result<i64, String> {
    insert_row_inner(conn, spec, Some(id), fields)
}

fn insert_row_inner(
    conn: &Connection,
    spec: &SyncTableSpec,
    id: Option<i64>,
    fields: &Map<String, JsonValue>,
) -> Result<i64, String> {
    let columns = existing_columns(conn, spec.table, spec.columns)?;
    let mut insert_columns = Vec::new();
    let mut placeholders = Vec::new();
    let mut values = Vec::new();
    if id.is_some() && columns.iter().any(|c| c == "id") {
        insert_columns.push("id".to_string());
        placeholders.push("?".to_string());
        values.push(Value::Integer(id.unwrap()));
    }
    for column in columns {
        if column == "id" {
            continue;
        }
        if let Some(value) = fields.get(&column) {
            insert_columns.push(column);
            placeholders.push("?".to_string());
            values.push(sql_value_from_json(value));
        }
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        spec.table,
        insert_columns.join(", "),
        placeholders.join(", ")
    );
    conn.execute(&sql, params_from_iter(values.iter())).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

fn row_exists_by_id(conn: &Connection, spec: &SyncTableSpec, id: i64) -> Result<bool, String> {
    let found = conn
        .query_row(&format!("SELECT 1 FROM {} WHERE id = ?1", spec.table), params![id], |_| Ok(()))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(found.is_some())
}


/// 内置父行下的子对象确定性 id：`official:{type}:{parent_sync_id}:{key…}`。
/// 仅当 unique_key 中的父 FK 已映射且为 official:* 时生成，使 A/B 对同一 builtin tool 推同一 object_id。
fn try_deterministic_child_object_id(
    sync_conn: &Connection,
    spec: &SyncTableSpec,
    raw_values: &HashMap<String, JsonValue>,
) -> Result<Option<String>, String> {
    if spec.unique_key_columns.is_empty() {
        return Ok(None);
    }
    let mut parent_sync_id: Option<String> = None;
    let mut name_parts: Vec<String> = Vec::new();
    for column in spec.unique_key_columns {
        if let Some(fk) = spec.foreign_keys.iter().find(|fk| fk.column == *column) {
            let Some(local_fk) = raw_values.get(*column).and_then(JsonValue::as_i64) else {
                return Ok(None);
            };
            if is_provider_id_unset_sentinel(fk, local_fk) {
                name_parts.push("0".to_string());
                continue;
            }
            let Some(sync_id) = find_object_id(sync_conn, fk.object_type, local_fk)? else {
                return Ok(None);
            };
            if !sync_id.starts_with("official:") {
                return Ok(None);
            }
            if parent_sync_id.is_none() {
                parent_sync_id = Some(sync_id);
            } else {
                name_parts.push(sync_id);
            }
        } else {
            let value = raw_values
                .get(*column)
                .ok_or_else(|| format!("unique key column {column} missing"))?;
            name_parts.push(
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            );
        }
    }
    let Some(parent) = parent_sync_id else {
        return Ok(None);
    };
    if name_parts.is_empty() {
        Ok(Some(format!("official:{}:{}", spec.object_type, parent)))
    } else {
        Ok(Some(format!(
            "official:{}:{}:{}",
            spec.object_type,
            parent,
            name_parts.join(":")
        )))
    }
}

/// 远端 sync_id 获胜：清掉占用该 sync_id 的旧映射，再按 local_id upsert。
fn remap_sync_object_map(
    conn: &Connection,
    object_type: &str,
    local_table: &str,
    local_id: i64,
    sync_id: &str,
) -> Result<(), String> {
    conn.execute(
        "DELETE FROM sync_object_map WHERE sync_id = ?1",
        params![sync_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(object_type, local_id) DO UPDATE SET sync_id = excluded.sync_id",
        params![object_type, local_table, local_id, sync_id, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn is_unique_constraint_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("unique constraint failed") || lower.contains("constraint failed")
}

fn find_row_by_unique_key(
    conn: &Connection,
    spec: &SyncTableSpec,
    fields: &Map<String, JsonValue>,
) -> Result<Option<i64>, String> {
    if spec.unique_key_columns.is_empty() {
        return Ok(None);
    }
    let where_clause = spec
        .unique_key_columns
        .iter()
        .map(|column| format!("{column} = ?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let mut where_values = Vec::new();
    for column in spec.unique_key_columns {
        let value = fields
            .get(*column)
            .ok_or_else(|| format!("unique key column {column} missing"))?;
        where_values.push(sql_value_from_json(value));
    }
    conn.query_row(
        &format!("SELECT id FROM {} WHERE {}", spec.table, where_clause),
        params_from_iter(where_values.iter()),
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// 按 unique_key_columns find-or-insert，并把 sync_object_map 重映射为远端 sync_id。
fn upsert_by_unique_key(
    sync_conn: &Connection,
    data_conn: &Connection,
    spec: &SyncTableSpec,
    remote_sync_id: &str,
    fields: &Map<String, JsonValue>,
) -> Result<(), String> {
    if let Some(existing_id) = find_row_by_unique_key(data_conn, spec, fields)? {
        update_row(data_conn, spec, existing_id, fields)?;
        remap_sync_object_map(
            sync_conn,
            spec.object_type,
            spec.table,
            existing_id,
            remote_sync_id,
        )?;
        return Ok(());
    }
    match insert_row(data_conn, spec, fields) {
        Ok(new_id) => {
            remap_sync_object_map(
                sync_conn,
                spec.object_type,
                spec.table,
                new_id,
                remote_sync_id,
            )?;
            Ok(())
        }
        Err(err) if is_unique_constraint_error(&err) => {
            let existing_id = find_row_by_unique_key(data_conn, spec, fields)?.ok_or_else(|| {
                format!(
                    "UNIQUE conflict on {} but row not found by unique key",
                    spec.table
                )
            })?;
            update_row(data_conn, spec, existing_id, fields)?;
            remap_sync_object_map(
                sync_conn,
                spec.object_type,
                spec.table,
                existing_id,
                remote_sync_id,
            )?;
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn upsert_natural_row(
    conn: &Connection,
    spec: &SyncTableSpec,
    fields: &Map<String, JsonValue>,
) -> Result<(), String> {
    let where_clause = spec
        .natural_key_columns
        .iter()
        .map(|column| format!("{column} = ?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let mut where_values = Vec::new();
    for column in spec.natural_key_columns {
        let value = fields
            .get(*column)
            .ok_or_else(|| format!("natural key column {column} missing"))?;
        where_values.push(sql_value_from_json(value));
    }
    let existing_id = conn
        .query_row(
            &format!("SELECT id FROM {} WHERE {}", spec.table, where_clause),
            params_from_iter(where_values.iter()),
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(id) = existing_id {
        update_row(conn, spec, id, fields)
    } else {
        let _ = insert_row(conn, spec, fields)?;
        Ok(())
    }
}

fn load_pending_outbox(app_handle: &AppHandle, limit: i64) -> Result<Vec<(i64, PushEvent)>, String> {
    let conn = open_sync_db(app_handle)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, event_id, object_type, object_id, operation, payload_json, base_version, local_version, created_at
             FROM sync_outbox
             WHERE status = 'pending'
               AND (next_retry_at IS NULL OR next_retry_at <= ?2)
             ORDER BY id
             LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let raw_rows = stmt
        .query_map(params![limit, Utc::now().timestamp()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;

    let mut events = Vec::with_capacity(raw_rows.len());
    for (id, event_id, object_type, object_id, operation, payload_json, base_version, local_version, created_at) in raw_rows
    {
        let payload = match payload_json.as_deref() {
            Some(raw) => match serde_json::from_str::<JsonValue>(raw) {
                Ok(value) => Some(value),
                Err(err) => {
                    // 损坏的 payload 不能静默当 None 推上去（upsert 会变空数据）：
                    // 记失败并跳过该事件，由退避/人工重试处理。
                    let message = format!("outbox event {event_id} payload 反序列化失败: {err}");
                    warn!(event_id = %event_id, "Skipping outbox event with corrupt payload");
                    record_event_failure(&conn, id, &message)?;
                    continue;
                }
            },
            None => None,
        };
        events.push((
            id,
            PushEvent {
                event_id,
                object_type,
                object_id,
                operation,
                payload,
                base_version,
                local_version,
                created_at,
                client_schema_version: CLIENT_SCHEMA_VERSION,
                object_schema_version: 1,
            },
        ));
    }
    Ok(events)
}

fn mark_events_pushing(app_handle: &AppHandle, events: &[(i64, PushEvent)]) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    for (id, _) in events {
        conn.execute("UPDATE sync_outbox SET status = 'pushing' WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 推送失败的退避策略：网络/5xx 类失败保持 pending 并按 retry_count 指数退避
/// （封顶 300s），连续失败达到上限才转 failed 等待人工重试。
const MAX_EVENT_RETRY_COUNT: i64 = 10;
const MAX_RETRY_BACKOFF_SECS: i64 = 300;

fn compute_next_retry_at(retry_count: i64) -> i64 {
    let backoff = (1i64 << retry_count.clamp(0, 8)).min(MAX_RETRY_BACKOFF_SECS);
    Utc::now().timestamp() + backoff
}

/// 记录一次推送失败：retry_count+1 并设置退避时间；超过上限转 failed。
fn record_event_failure(conn: &Connection, id: i64, error: &str) -> Result<(), String> {
    let retry_count: i64 = conn
        .query_row("SELECT retry_count FROM sync_outbox WHERE id = ?1", params![id], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    let next_retry_count = retry_count + 1;
    let (status, next_retry_at): (&str, Option<i64>) = if next_retry_count >= MAX_EVENT_RETRY_COUNT {
        ("failed", None)
    } else {
        ("pending", Some(compute_next_retry_at(next_retry_count)))
    };
    conn.execute(
        "UPDATE sync_outbox
         SET status = ?2, retry_count = ?3, last_error = ?4, next_retry_at = ?5
         WHERE id = ?1",
        params![id, status, next_retry_count, error, next_retry_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_events_failed(
    app_handle: &AppHandle,
    events: &[(i64, PushEvent)],
    error: &str,
) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    for (id, _) in events {
        record_event_failure(&conn, *id, error)?;
    }
    Ok(())
}

fn reset_pushing_events(app_handle: &AppHandle) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    conn.execute(
        "UPDATE sync_outbox
         SET status = 'pending', last_error = NULL
         WHERE status = 'pushing'",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn reset_failed_events(app_handle: &AppHandle) -> Result<usize, String> {
    let conn = open_sync_db(app_handle)?;
    reset_failed_events_inner(&conn)
}

/// 重试 failed 事件前，用 shadow 中当前的 server_version 刷新 base_version，
/// 消除"base_version 冻结 → 永远 conflict"的重试死循环。
fn reset_failed_events_inner(conn: &Connection) -> Result<usize, String> {
    conn.execute(
        "UPDATE sync_outbox
         SET base_version = COALESCE((
                 SELECT s.server_version FROM sync_shadow s
                 WHERE s.object_type = sync_outbox.object_type
                   AND s.object_id = sync_outbox.object_id
             ), base_version),
             status = 'pending',
             last_error = NULL,
             next_retry_at = NULL
         WHERE status = 'failed'",
        [],
    )
    .map_err(|e| e.to_string())
}

/// push ack 后更新 shadow：upsert 刷新版本与哈希；delete 置墓碑（保留行供删除检测去重）
/// 并清理 object_map，使同 object_id 的后续 upsert 走重新 insert（对象复活）。
fn apply_ack_to_shadow(
    conn: &Connection,
    object_type: &str,
    object_id: &str,
    server_version: i64,
    operation: &str,
    payload_hash: &str,
) -> Result<(), String> {
    if operation == "delete" {
        let now = now_string();
        conn.execute(
            "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(object_type, object_id)
             DO UPDATE SET server_version = excluded.server_version,
                           deleted_at = excluded.deleted_at,
                           updated_at = excluded.updated_at",
            params![object_type, object_id, server_version, payload_hash, now],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM sync_object_map WHERE object_type = ?1 AND sync_id = ?2",
            params![object_type, object_id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    conn.execute(
        "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)
         ON CONFLICT(object_type, object_id)
         DO UPDATE SET server_version = excluded.server_version,
                       payload_hash = excluded.payload_hash,
                       deleted_at = NULL,
                       updated_at = excluded.updated_at",
        params![object_type, object_id, server_version, payload_hash, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn handle_push_response(
    app_handle: &AppHandle,
    events: &[(i64, PushEvent)],
    response: PushResponse,
) -> Result<usize, String> {
    let conn = open_sync_db(app_handle)?;
    let by_event_id: HashMap<&str, &(i64, PushEvent)> =
        events.iter().map(|item| (item.1.event_id.as_str(), item)).collect();
    for accepted in response.accepted {
        if let Some((id, event)) = by_event_id.get(accepted.event_id.as_str()) {
            // ack 后直接删除 outbox 行，不再永久残留 'acked' 记录
            conn.execute("DELETE FROM sync_outbox WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
            let payload_hash = event
                .payload
                .as_ref()
                .map(|payload| hash_text(&serde_json::to_string(payload).unwrap_or_default()))
                .unwrap_or_else(|| hash_text(""));
            apply_ack_to_shadow(
                &conn,
                &accepted.object_type,
                &accepted.object_id,
                accepted.server_version,
                &event.operation,
                &payload_hash,
            )?;
        }
    }

    let mut rejected_count = 0;
    let mut latest_failure: Option<String> = None;

    for rejected in response.rejected {
        if let Some((id, _)) = by_event_id.get(rejected.event_id.as_str()) {
            rejected_count += 1;
            latest_failure = Some(rejected.reason.clone());
            conn.execute(
                "UPDATE sync_outbox
                 SET status = 'failed', retry_count = retry_count + 1, last_error = ?2
                 WHERE id = ?1",
                params![id, rejected.reason],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    let mut conflict_resolved = 0usize;
    let mut resolved_object_types: Vec<String> = Vec::new();
    for conflict in response.conflicts {
        if let Some((id, _)) = by_event_id.get(conflict.event_id.as_str()) {
            // 远端赢：把服务端版本按 pull 语义落地到本地，然后删除该 outbox 事件
            // （冲突已解决，无需重试；本地未推送修改被服务端版本覆盖）。
            let pseudo_change = PullChange {
                seq: 0,
                event_id: conflict.event_id.clone(),
                // 与本机 device_id（UUID）永不相等，确保 apply_change 走远端应用路径
                device_id: "server-conflict".to_string(),
                object_type: conflict.object_type.clone(),
                object_id: conflict.object_id.clone(),
                operation: conflict.server_operation.clone(),
                version: conflict.server_version,
                payload: conflict.server_payload.clone(),
                deleted_at: if conflict.server_operation == "delete" {
                    Some(now_string())
                } else {
                    None
                },
                created_at: now_string(),
            };
            let apply_result = match try_fork_message_conflict_instead_of_overwrite(
                app_handle,
                &conn,
                &pseudo_change,
            ) {
                Ok(true) => Ok(()),
                Ok(false) => apply_change(app_handle, &conn, &pseudo_change, ""),
                Err(err) => Err(err),
            };
            match apply_result {
                Ok(()) => {
                    conn.execute("DELETE FROM sync_outbox WHERE id = ?1", params![id])
                        .map_err(|e| e.to_string())?;
                    conflict_resolved += 1;
                    resolved_object_types.push(conflict.object_type.clone());
                }
                Err(err) => {
                    // 落地失败（如依赖未就绪）：标 failed 等待重试，重试前
                    // base_version 会被刷新（见 reset_failed_events）；
                    // 该变更后续也会经 pull 正常下发，不会丢。
                    let message = format!("冲突落地失败: {err}");
                    latest_failure = Some(message.clone());
                    conn.execute(
                        "UPDATE sync_outbox
                         SET status = 'failed', retry_count = retry_count + 1, last_error = ?2
                         WHERE id = ?1",
                        params![id, message],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    if !resolved_object_types.is_empty() {
        emit_sync_domain_events(app_handle, resolved_object_types.iter().map(String::as_str));
    }
    if rejected_count > 0 {
        return Err(format!(
            "同步服务器拒绝 {} 条；最近原因：{}",
            rejected_count,
            latest_failure.unwrap_or_else(|| "未提供原因".to_string())
        ));
    }
    Ok(conflict_resolved)
}

fn upsert_shadow_from_remote(conn: &Connection, change: &PullChange) -> Result<(), String> {
    let payload_hash = hash_text(&serde_json::to_string(&change.payload).unwrap_or_default());
    conn.execute(
        "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(object_type, object_id)
         DO UPDATE SET server_version = excluded.server_version,
                       payload_hash = excluded.payload_hash,
                       deleted_at = excluded.deleted_at,
                       updated_at = excluded.updated_at",
        params![
            change.object_type,
            change.object_id,
            change.version,
            payload_hash,
            change.deleted_at,
            now_string(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// sync.db 的完整建表 DDL。抽成常量以便单测在内存库上复用同一 schema。
const SYNC_SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS sync_object_map (
            object_type TEXT NOT NULL,
            local_table TEXT NOT NULL,
            local_id INTEGER NOT NULL,
            sync_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (object_type, local_id),
            UNIQUE (sync_id)
        );
        CREATE TABLE IF NOT EXISTS sync_device (
            device_id TEXT PRIMARY KEY,
            device_name TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_sync_at TEXT
        );
        CREATE TABLE IF NOT EXISTS sync_cursor (
            scope TEXT PRIMARY KEY,
            server_cursor INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sync_outbox (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id TEXT NOT NULL UNIQUE,
            object_type TEXT NOT NULL,
            object_id TEXT NOT NULL,
            operation TEXT NOT NULL CHECK(operation IN ('upsert', 'delete')),
            payload_json TEXT,
            base_version INTEGER,
            local_version INTEGER NOT NULL,
            device_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending'
                CHECK(status IN ('pending', 'pushing', 'acked', 'failed')),
            retry_count INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            next_retry_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_sync_outbox_status_id
            ON sync_outbox(status, id);
        CREATE TABLE IF NOT EXISTS sync_shadow (
            object_type TEXT NOT NULL,
            object_id TEXT NOT NULL,
            server_version INTEGER NOT NULL,
            payload_hash TEXT NOT NULL,
            deleted_at TEXT,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (object_type, object_id)
        );
        CREATE TABLE IF NOT EXISTS sync_dead_letter (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            object_type TEXT NOT NULL,
            object_id TEXT NOT NULL,
            operation TEXT NOT NULL,
            change_json TEXT NOT NULL,
            error TEXT NOT NULL,
            failed_at TEXT NOT NULL,
            retry_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_sync_dead_letter_object
            ON sync_dead_letter(object_type, object_id);
        CREATE TABLE IF NOT EXISTS sync_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );";

fn ensure_sync_db(app_handle: &AppHandle) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    conn.execute_batch(SYNC_SCHEMA_SQL).map_err(|e| e.to_string())?;
    ensure_sync_db_columns(&conn)?;
    Ok(())
}

/// CREATE TABLE IF NOT EXISTS 不会给已存在的表补列，这里做幂等的列迁移。
fn ensure_sync_db_columns(conn: &Connection) -> Result<(), String> {
    ensure_column(conn, "sync_outbox", "next_retry_at", "ALTER TABLE sync_outbox ADD COLUMN next_retry_at INTEGER")
}

fn ensure_column(conn: &Connection, table: &str, column: &str, alter_sql: &str) -> Result<(), String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).map_err(|e| e.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !columns.iter().any(|c| c == column) {
        conn.execute_batch(alter_sql).map_err(|e| e.to_string())?;
    }
    Ok(())
}

const META_NEEDS_RESET: &str = "needs_reset";

fn get_sync_meta(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM sync_meta WHERE key = ?1", params![key], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

fn set_sync_meta(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sync_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 是否处于"配置已变更、等待用户确认重置"状态。
fn needs_reset(conn: &Connection) -> Result<bool, String> {
    Ok(get_sync_meta(conn, META_NEEDS_RESET)?.as_deref() == Some("1"))
}

/// sync.db 中是否已有任何同步状态（cursor/shadow/map）。
/// 换服务器/账号时据此判断是否需要重置，而不是静默沿用旧状态。
fn has_sync_state(conn: &Connection) -> Result<bool, String> {
    for table in ["sync_cursor", "sync_shadow", "sync_object_map"] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if count > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 清空全部同步状态（cursor/shadow/map/outbox/dead_letter）并解除 needs_reset。
/// 调用方负责在此之前取得用户确认。
fn clear_sync_state(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "DELETE FROM sync_cursor;
         DELETE FROM sync_shadow;
         DELETE FROM sync_object_map;
         DELETE FROM sync_outbox;
         DELETE FROM sync_dead_letter;
         DELETE FROM sync_meta WHERE key = 'needs_reset';",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn open_sync_db(app_handle: &AppHandle) -> Result<Connection, String> {
    let path = get_db_path(app_handle, "sync.db")?;
    let conn = open_data_db(&path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// 打开业务数据 DB（读多写少），加 busy_timeout 避免与其他写路径偶发冲突直接报错。
fn open_data_db(path: &std::path::Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA busy_timeout=5000;").map_err(|e| e.to_string())?;
    Ok(conn)
}

/// 调用方需保证 schema 已建好（各入口都会先调 ensure_sync_db）。
fn ensure_device_id(conn: &Connection) -> Result<String, String> {
    if let Some(id) = conn
        .query_row("SELECT device_id FROM sync_device LIMIT 1", [], |row| row.get::<_, String>(0))
        .optional()
        .map_err(|e| e.to_string())?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO sync_device (device_id, device_name, created_at) VALUES (?1, ?2, ?3)",
        params![id, device_name(), now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

fn get_device_last_sync_at(app_handle: &AppHandle) -> Result<Option<String>, String> {
    ensure_sync_db(app_handle)?;
    let conn = open_sync_db(app_handle)?;
    let value = conn
        .query_row(
            "SELECT last_sync_at FROM sync_device LIMIT 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten();
    Ok(value)
}

fn set_device_last_sync_at(app_handle: &AppHandle, last_sync_at: &str) -> Result<(), String> {
    ensure_sync_db(app_handle)?;
    let conn = open_sync_db(app_handle)?;
    conn.execute(
        "UPDATE sync_device
         SET last_sync_at = ?1
         WHERE device_id = (SELECT device_id FROM sync_device LIMIT 1)",
        params![last_sync_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "AIPP Desktop".to_string())
}

fn get_cursor(app_handle: &AppHandle) -> Result<i64, String> {
    ensure_sync_db(app_handle)?;
    let conn = open_sync_db(app_handle)?;
    let cursor = conn
        .query_row(
            "SELECT server_cursor FROM sync_cursor WHERE scope = ?1",
            params![DEFAULT_SCOPE],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    Ok(cursor)
}

fn set_cursor(app_handle: &AppHandle, cursor: i64) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    conn.execute(
        "INSERT INTO sync_cursor (scope, server_cursor, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(scope)
         DO UPDATE SET server_cursor = excluded.server_cursor,
                       updated_at = excluded.updated_at",
        params![DEFAULT_SCOPE, cursor, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn ensure_object_id(
    conn: &Connection,
    object_type: &str,
    local_table: &str,
    local_id: i64,
) -> Result<String, String> {
    if let Some(existing) = conn
        .query_row(
            "SELECT sync_id FROM sync_object_map WHERE object_type = ?1 AND local_id = ?2",
            params![object_type, local_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
    {
        return Ok(existing);
    }
    let sync_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![object_type, local_table, local_id, sync_id, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(sync_id)
}

/// 官方/内置种子行的确定性 object_id：`official:{object_type}:{local_id}`。
fn official_object_id(object_type: &str, local_id: i64) -> String {
    format!("official:{object_type}:{local_id}")
}

/// 解析 `official:{object_type}:{local_id}`，类型不匹配或格式非法返回 None。
fn parse_official_object_id(object_id: &str, expected_object_type: &str) -> Option<i64> {
    let rest = object_id.strip_prefix("official:")?;
    let (object_type, local_id) = rest.rsplit_once(':')?;
    if object_type != expected_object_type {
        return None;
    }
    local_id.parse().ok()
}

/// 官方/内置种子行使用确定性 object_id，保证各设备对同一内置行生成相同标识。
/// 老设备上已存在的随机 UUID 映射在此被就地替换；服务器侧的旧随机对象
/// 会经删除检测（scan_local_deletes）自然收敛清理。
fn upsert_official_object_map(
    conn: &Connection,
    object_type: &str,
    local_table: &str,
    local_id: i64,
) -> Result<String, String> {
    let sync_id = official_object_id(object_type, local_id);
    conn.execute(
        "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(object_type, local_id) DO UPDATE SET sync_id = excluded.sync_id",
        params![object_type, local_table, local_id, sync_id, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(sync_id)
}

fn save_object_map(
    conn: &Connection,
    object_type: &str,
    local_table: &str,
    local_id: i64,
    sync_id: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![object_type, local_table, local_id, sync_id, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn find_object_id(conn: &Connection, object_type: &str, local_id: i64) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT sync_id FROM sync_object_map WHERE object_type = ?1 AND local_id = ?2",
        params![object_type, local_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn find_local_id_by_object_id(
    conn: &Connection,
    object_type: &str,
    object_id: &str,
) -> Result<Option<i64>, String> {
    conn.query_row(
        "SELECT local_id FROM sync_object_map WHERE object_type = ?1 AND sync_id = ?2",
        params![object_type, object_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn natural_object_id(
    spec: &SyncTableSpec,
    raw_values: &HashMap<String, JsonValue>,
) -> Result<String, String> {
    let mut parts = Vec::new();
    for column in spec.natural_key_columns {
        let value = raw_values
            .get(*column)
            .ok_or_else(|| format!("natural key {column} missing"))?;
        parts.push(value.as_str().map(ToOwned::to_owned).unwrap_or_else(|| value.to_string()));
    }
    Ok(format!("natural:{}", BASE64.encode(parts.join("\u{1f}"))))
}

fn natural_values_from_object_id(object_id: &str) -> Result<Vec<String>, String> {
    let encoded = object_id
        .strip_prefix("natural:")
        .ok_or_else(|| "invalid natural object id".to_string())?;
    let decoded = BASE64.decode(encoded).map_err(|e| e.to_string())?;
    let raw = String::from_utf8(decoded).map_err(|e| e.to_string())?;
    Ok(raw.split('\u{1f}').map(ToOwned::to_owned).collect())
}

fn existing_columns(
    conn: &Connection,
    table: &str,
    desired: &[&str],
) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    let existing = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    Ok(desired
        .iter()
        .filter(|column| existing.iter().any(|existing| existing == **column))
        .map(|column| (*column).to_string())
        .collect())
}

fn is_local_sync_scope_empty(app_handle: &AppHandle) -> Result<bool, String> {
    for spec in specs() {
        let path = get_db_path(app_handle, spec.db_name)?;
        let conn = open_data_db(&path)?;
        // 判空时排除官方/内置种子行：每台设备都有它们，
        // 不排除的话"纯拉取 bootstrap"分支永远不可达。
        let mut conditions: Vec<String> = spec.where_clause.map(|w| format!("({w})")).into_iter().collect();
        if let Some(column) = spec.official_column {
            conditions.push(format!("COALESCE({column}, 0) = 0"));
        }
        if !spec.seed_official_ids.is_empty() {
            let ids = spec
                .seed_official_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            conditions.push(format!("id NOT IN ({ids})"));
        }
        let where_sql =
            if conditions.is_empty() { String::new() } else { format!(" WHERE {}", conditions.join(" AND ")) };
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {}{}", spec.table, where_sql),
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count > 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn json_from_value_ref(value: ValueRef<'_>) -> JsonValue {
    match value {
        ValueRef::Null => JsonValue::Null,
        ValueRef::Integer(v) => JsonValue::Number(v.into()),
        ValueRef::Real(v) => serde_json::Number::from_f64(v)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        ValueRef::Text(v) => JsonValue::String(String::from_utf8_lossy(v).to_string()),
        ValueRef::Blob(v) => JsonValue::String(BASE64.encode(v)),
    }
}

fn sql_value_from_json(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(v) => Value::Integer(if *v { 1 } else { 0 }),
        JsonValue::Number(v) => {
            if let Some(i) = v.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = v.as_f64() {
                Value::Real(f)
            } else {
                Value::Null
            }
        }
        JsonValue::String(v) => Value::Text(v.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => Value::Text(value.to_string()),
    }
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

async fn sync_status(app_handle: &AppHandle) -> Result<SyncStatusDto, String> {
    let settings = load_settings(app_handle)?;
    ensure_sync_db(app_handle)?;
    let conn = open_sync_db(app_handle)?;
    let pending_outbox_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_outbox WHERE status = 'pending'", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    let pushing_outbox_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sync_outbox WHERE status = 'pushing'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let failed_outbox_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_outbox WHERE status = 'failed'", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    let latest_failed_error: Option<String> = conn
        .query_row(
            "SELECT last_error FROM sync_outbox
             WHERE status = 'failed' AND last_error IS NOT NULL AND last_error != ''
             ORDER BY id DESC
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);
    let dead_letter_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_dead_letter", [], |row| row.get(0))
        .unwrap_or(0);
    let needs_reset = needs_reset(&conn).unwrap_or(false);
    let runtime = if let Some(state) = app_handle.try_state::<SyncState>() {
        state.status.lock().await.clone()
    } else {
        SyncRuntimeStatus::default()
    };
    let persisted_last_sync_at = get_device_last_sync_at(app_handle).unwrap_or(None);
    let last_error = runtime.last_error.or(latest_failed_error);
    Ok(SyncStatusDto {
        mode: settings.mode,
        server_url: settings.server_url,
        token_configured: settings.token.is_some(),
        connected: runtime.connected,
        running: runtime.running,
        syncing: runtime.syncing,
        last_sync_at: runtime.last_sync_at.or(persisted_last_sync_at),
        last_error,
        pending_outbox_count,
        pushing_outbox_count,
        failed_outbox_count,
        dead_letter_count,
        needs_reset,
        conflict_resolved_count: runtime.conflict_resolved_count,
        server_cursor: get_cursor(app_handle).unwrap_or(0),
    })
}

async fn emit_status_changed(app_handle: &AppHandle) {
    if let Ok(status) = sync_status(app_handle).await {
        let _ = app_handle.emit("sync_status_changed", status);
    }
}

fn load_settings(app_handle: &AppHandle) -> Result<SyncSettings, String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let mode_value = db
        .get_feature_config(FEATURE_CODE, "mode")
        .map_err(|e| e.to_string())?
        .map(|config| config.value)
        .unwrap_or_else(|| "local".to_string());
    let server_url = db
        .get_feature_config(FEATURE_CODE, "server_url")
        .map_err(|e| e.to_string())?
        .map(|config| config.value)
        .unwrap_or_default();
    let mode = SyncMode::from_config(&mode_value);
    let token = load_sync_token(app_handle)?;
    Ok(SyncSettings { mode, server_url, token })
}

async fn save_feature_config_value(
    app_handle: &AppHandle,
    state: &State<'_, crate::FeatureConfigState>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    if db
        .get_feature_config(FEATURE_CODE, key)
        .map_err(|e| e.to_string())?
        .is_some()
    {
        db.update_feature_config(&FeatureConfig {
            id: None,
            feature_code: FEATURE_CODE.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            data_type: "string".to_string(),
            description: Some(String::new()),
        })
        .map_err(|e| e.to_string())?;
    } else {
        db.add_feature_config(&FeatureConfig {
            id: None,
            feature_code: FEATURE_CODE.to_string(),
            key: key.to_string(),
            value: value.to_string(),
            data_type: "string".to_string(),
            description: Some(String::new()),
        })
        .map_err(|e| e.to_string())?;
    }

    let config = FeatureConfig {
        id: None,
        feature_code: FEATURE_CODE.to_string(),
        key: key.to_string(),
        value: value.to_string(),
        data_type: "string".to_string(),
        description: Some(String::new()),
    };
    let mut configs = state.configs.lock().await;
    configs.retain(|item| !(item.feature_code == FEATURE_CODE && item.key == key));
    configs.push(config.clone());
    let mut map = state.config_feature_map.lock().await;
    map.entry(FEATURE_CODE.to_string())
        .or_insert_with(HashMap::new)
        .insert(key.to_string(), config);
    Ok(())
}

fn load_sync_token(app_handle: &AppHandle) -> Result<Option<String>, String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let Some(entry) = db.get_secure_config(TOKEN_SCOPE, TOKEN_KEY).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let key = get_or_create_master_key(app_handle)?;
    decrypt_secret_with_key(&key, &entry.ciphertext, &entry.nonce).map(Some)
}

fn save_sync_token(app_handle: &AppHandle, token: &str) -> Result<(), String> {
    let key = get_or_create_master_key(app_handle)?;
    let (ciphertext, nonce) = encrypt_secret_with_key(&key, token)?;
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.upsert_secure_config(&SecureConfigEntry {
        scope: TOKEN_SCOPE.to_string(),
        key: TOKEN_KEY.to_string(),
        ciphertext,
        nonce,
        updated_time: None,
    })
    .map_err(|e| e.to_string())
}

fn get_or_create_master_key(app_handle: &AppHandle) -> Result<[u8; 32], String> {
    // 与 feishu/config.rs 对齐：优先使用本地文件 master key，保证 Linux（无 Secret
    // Service / mock keyring）上 encrypt/decrypt 使用同一把持久化密钥。
    // OS keychain 仅在能真正 round-trip 持久化时使用。
    if let Some(key) = read_master_key_from_file(app_handle)? {
        return Ok(key);
    }

    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_MASTER_KEY_USER) {
        Ok(entry) => match entry.get_password() {
            Ok(encoded) => match decode_master_key_b64(&encoded) {
                Ok(key) => {
                    // 可选迁移：把已有 keychain key 落到文件，后续走稳定的 file-first 路径。
                    if let Err(err) = write_master_key_to_file(app_handle, &key) {
                        warn!(error = %err, "Failed to mirror OS keychain master key to local file");
                    }
                    return Ok(key);
                }
                Err(err) => {
                    warn!(error = %err, "OS keychain master key invalid, falling back to file");
                }
            },
            Err(keyring::Error::NoEntry) => {
                let key: [u8; 32] = rand::random();
                match try_persist_master_key_in_keyring(&entry, &key) {
                    Ok(()) => {
                        if let Err(err) = write_master_key_to_file(app_handle, &key) {
                            warn!(
                                error = %err,
                                "Keychain persist ok but failed to mirror master key to local file"
                            );
                        }
                        return Ok(key);
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            "OS keychain did not persist master key (mock/no backend); using file fallback"
                        );
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "OS keychain unavailable, falling back to file-based master key");
            }
        },
        Err(err) => {
            warn!(error = %err, "OS keychain entry creation failed, falling back to file-based master key");
        }
    }
    get_or_create_master_key_from_file(app_handle)
}

/// 写入 keychain 后立即 get+decode+compare；mock/无后端 keyring 的 set 看似成功但不持久化，
/// 必须在此时失败并让调用方退回文件，否则下次 NoEntry 会生成新 key 导致 aead 解密失败。
fn try_persist_master_key_in_keyring(
    entry: &keyring::Entry,
    key: &[u8; 32],
) -> Result<(), String> {
    persist_master_key_with_roundtrip_verify(
        |encoded| {
            entry
                .set_password(encoded)
                .map_err(|e| format!("写入同步主密钥到 OS keychain 失败: {e}"))
        },
        || {
            entry
                .get_password()
                .map_err(|e| format!("OS keychain round-trip get 失败: {e}"))
        },
        key,
    )
}

/// 可单测的 round-trip 校验：set 后立刻 get，解码并与刚生成的 key 比较。
fn persist_master_key_with_roundtrip_verify(
    set_password: impl FnOnce(&str) -> Result<(), String>,
    get_password: impl FnOnce() -> Result<String, String>,
    key: &[u8; 32],
) -> Result<(), String> {
    let encoded = BASE64.encode(key);
    set_password(&encoded)?;
    let stored = get_password()?;
    let stored_key = decode_master_key_b64(&stored)?;
    if &stored_key != key {
        return Err("OS keychain round-trip master key mismatch".to_string());
    }
    Ok(())
}

fn decode_master_key_b64(encoded: &str) -> Result<[u8; 32], String> {
    let bytes = BASE64
        .decode(encoded.trim())
        .map_err(|e| format!("同步主密钥解码失败: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "同步主密钥长度非法".to_string())
}

/// 文件存储的 master key（稳定回退；与 feishu 的 local-secrets 策略一致）。
fn get_or_create_master_key_from_file(app_handle: &AppHandle) -> Result<[u8; 32], String> {
    if let Some(key) = read_master_key_from_file(app_handle)? {
        return Ok(key);
    }
    let key: [u8; 32] = rand::random();
    write_master_key_to_file(app_handle, &key)?;
    Ok(key)
}

fn master_key_file_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let secret_dir = app_dir.join("local-secrets");
    std::fs::create_dir_all(&secret_dir).map_err(|e| e.to_string())?;
    Ok(secret_dir.join(SECURE_MASTER_KEY_FILE))
}

fn read_master_key_from_file(app_handle: &AppHandle) -> Result<Option<[u8; 32]>, String> {
    let path = master_key_file_path(app_handle)?;
    read_master_key_at_path(&path)
}

fn write_master_key_to_file(app_handle: &AppHandle, key: &[u8; 32]) -> Result<(), String> {
    let path = master_key_file_path(app_handle)?;
    write_master_key_at_path(&path, key)
}

fn read_master_key_at_path(path: &std::path::Path) -> Result<Option<[u8; 32]>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read secure master key `{}`: {}", path.display(), e))?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| format!("Invalid secure master key length in `{}`", path.display()))?;
    Ok(Some(key))
}

fn write_master_key_at_path(path: &std::path::Path, key: &[u8; 32]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, key)
        .map_err(|e| format!("Failed to write secure master key `{}`: {}", path.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            format!(
                "Failed to set secure permissions on master key `{}`: {}",
                path.display(),
                e
            )
        })?;
    }
    Ok(())
}

fn get_or_create_master_key_at_path(path: &std::path::Path) -> Result<[u8; 32], String> {
    if let Some(key) = read_master_key_at_path(path)? {
        return Ok(key);
    }
    let key: [u8; 32] = rand::random();
    write_master_key_at_path(path, &key)?;
    Ok(key)
}

fn encrypt_secret_with_key(key_bytes: &[u8; 32], plaintext: &str) -> Result<(String, String), String> {
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Failed to encrypt secret: {e}"))?;
    Ok((BASE64.encode(encrypted), BASE64.encode(nonce_bytes)))
}

fn decrypt_secret_with_key(key_bytes: &[u8; 32], ciphertext: &str, nonce: &str) -> Result<String, String> {
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes = BASE64.decode(nonce).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = BASE64.decode(ciphertext).map_err(|e| e.to_string())?;
    let decrypted = cipher
        .decrypt(nonce, encrypted.as_ref())
        .map_err(|e| format!("Failed to decrypt secret: {e}"))?;
    String::from_utf8(decrypted).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::FromIterator;

    #[test]
    fn dependency_pending_error_matches_only_dependency_prefixes() {
        assert!(is_dependency_pending_error("缺少同步依赖: message.conversation_id -> abc"));
        assert!(is_dependency_pending_error("远端变更缺少外键引用: message.conversation_id（refs 缺失，拒绝写入原始本地 id）"));
        assert!(!is_dependency_pending_error("remote change evt-1 missing payload"));
        assert!(!is_dependency_pending_error("数据库写入失败: locked"));
    }

    #[test]
    fn sync_mode_parses_legacy_and_default_values() {
        assert_eq!(SyncMode::from_config("self_hosted"), SyncMode::SelfHosted);
        assert_eq!(SyncMode::from_config("self-hosted"), SyncMode::SelfHosted);
        assert_eq!(SyncMode::from_config(""), SyncMode::Local);
    }

    #[test]
    fn parse_sse_sync_cursor_reads_sync_events() {
        let block = "event: sync\ndata: {\"cursor\":42,\"device_id\":\"d1\"}\n";
        assert_eq!(parse_sse_sync_cursor(block), Some(42));
        assert_eq!(parse_sse_sync_cursor(": keepalive\n"), None);
        assert_eq!(parse_sse_sync_cursor("event: ping\ndata: {\"cursor\":1}\n"), None);
    }

    #[test]
    fn secret_roundtrip_uses_aes_gcm_payload() {
        let key: [u8; 32] = [7; 32];
        let (ciphertext, nonce) = encrypt_secret_with_key(&key, "dev-token").unwrap();
        assert_ne!(ciphertext, "dev-token");
        assert_eq!(decrypt_secret_with_key(&key, &ciphertext, &nonce).unwrap(), "dev-token");
    }

    /// mock keyring：set_password 看似成功，但随后 get 仍 NoEntry（不持久化）。
    #[test]
    fn keyring_roundtrip_verify_fails_when_store_does_not_persist() {
        let key: [u8; 32] = [9; 32];
        let err = persist_master_key_with_roundtrip_verify(
            |_encoded| Ok(()),
            || Err("NoEntry".to_string()),
            &key,
        )
        .expect_err("non-persisting keyring must fail round-trip verify");
        assert!(err.contains("round-trip") || err.contains("NoEntry"), "unexpected err: {err}");
    }

    #[test]
    fn keyring_roundtrip_verify_fails_on_key_mismatch() {
        let key: [u8; 32] = [3; 32];
        let other = BASE64.encode([4u8; 32]);
        let err = persist_master_key_with_roundtrip_verify(
            |_encoded| Ok(()),
            move || Ok(other),
            &key,
        )
        .expect_err("mismatched key must fail verify");
        assert!(err.contains("mismatch"), "unexpected err: {err}");
    }

    #[test]
    fn keyring_roundtrip_verify_ok_when_get_returns_same_key() {
        let key: [u8; 32] = [5; 32];
        let encoded = BASE64.encode(key);
        persist_master_key_with_roundtrip_verify(
            |_encoded| Ok(()),
            move || Ok(encoded),
            &key,
        )
        .expect("persisting keyring should pass round-trip verify");
    }

    /// 模拟「keyring 不持久化 → 退回文件」后，同一文件 key 可稳定 encrypt/decrypt。
    #[test]
    fn file_fallback_after_failed_keyring_persist_keeps_encrypt_decrypt_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SECURE_MASTER_KEY_FILE);
        let key: [u8; 32] = [11; 32];

        let verify_err = persist_master_key_with_roundtrip_verify(
            |_encoded| Ok(()), // mock set "succeeds"
            || Err("NoEntry".to_string()), // mock get fails → not persisted
            &key,
        )
        .expect_err("fake keyring must not look persisted");
        assert!(!verify_err.is_empty());

        // 失败后走文件路径（与 get_or_create_master_key 的 fallthrough 一致）
        write_master_key_at_path(&path, &key).unwrap();
        let loaded = get_or_create_master_key_at_path(&path).unwrap();
        assert_eq!(loaded, key);
        let again = get_or_create_master_key_at_path(&path).unwrap();
        assert_eq!(again, key, "file master key must be stable across calls");

        let (ciphertext, nonce) = encrypt_secret_with_key(&loaded, "sync-token").unwrap();
        assert_eq!(
            decrypt_secret_with_key(&again, &ciphertext, &nonce).unwrap(),
            "sync-token"
        );
    }

    #[test]
    fn file_master_key_create_then_reload_roundtrips_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SECURE_MASTER_KEY_FILE);
        let key1 = get_or_create_master_key_at_path(&path).unwrap();
        let key2 = get_or_create_master_key_at_path(&path).unwrap();
        assert_eq!(key1, key2);
        let (ciphertext, nonce) = encrypt_secret_with_key(&key1, "hello").unwrap();
        assert_eq!(decrypt_secret_with_key(&key2, &ciphertext, &nonce).unwrap(), "hello");
    }

    #[test]
    fn natural_object_id_roundtrip_preserves_parts() {
        let spec = SyncTableSpec {
            db_name: "system.db",
            table: "feature_config",
            object_type: "system.feature_config",
            columns: &["id", "feature_code", "key"],
            natural_key_columns: &["feature_code", "key"],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[],
            where_clause: None,
            order_by: "id",
        };
        let values = HashMap::from([
            ("feature_code".to_string(), JsonValue::String("display".to_string())),
            ("key".to_string(), JsonValue::String("theme".to_string())),
        ]);
        let object_id = natural_object_id(&spec, &values).unwrap();
        assert_eq!(
            natural_values_from_object_id(&object_id).unwrap(),
            vec!["display".to_string(), "theme".to_string()]
        );
    }

    fn message_spec() -> SyncTableSpec {
        SyncTableSpec {
            db_name: "conversation.db",
            table: "message",
            object_type: "conversation.message",
            columns: &["id", "conversation_id", "parent_id", "content", "llm_model_id"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "conversation_id", object_type: "conversation" },
                ForeignKeySpec { column: "parent_id", object_type: "conversation.message" },
                ForeignKeySpec { column: "llm_model_id", object_type: "llm.model" },
            ],
            where_clause: None,
            order_by: "id",
        }
    }

    fn assistant_model_spec() -> SyncTableSpec {
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_model",
            object_type: "assistant.model",
            columns: &["id", "assistant_id", "provider_id", "model_code", "alias"],
            natural_key_columns: &[],
            unique_key_columns: &["assistant_id", "provider_id", "model_code"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec { column: "assistant_id", object_type: "assistant" },
                ForeignKeySpec { column: "provider_id", object_type: "llm.provider" },
            ],
            where_clause: None,
            order_by: "id",
        }
    }

    /// 所有非空外键都能解析时，refs 完整返回
    #[test]
    fn fk_refs_resolve_all_non_null_columns() {
        let spec = message_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(9)),
            ("conversation_id".to_string(), json!(3)),
            ("parent_id".to_string(), json!(7)),
            ("content".to_string(), json!("hi")),
        ]);
        let mut fields = Map::new();
        let refs = build_fk_refs(&spec, &raw_values, &mut fields, |object_type, value| {
            Ok(FkLookup::Resolved(format!("{object_type}-{value}")))
        })
        .unwrap()
        .expect("all foreign keys resolvable");
        assert_eq!(refs.get("conversation_id").unwrap(), &json!("conversation-3"));
        assert_eq!(refs.get("parent_id").unwrap(), &json!("conversation.message-7"));
    }

    /// 依赖行存在但尚未映射时返回 None（调用方跳过该行），不产出裸 rowid
    #[test]
    fn fk_refs_return_none_when_dependency_unresolved() {
        let spec = message_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(9)),
            ("conversation_id".to_string(), json!(3)),
            ("parent_id".to_string(), json!(7)),
        ]);
        let mut fields = Map::new();
        let result = build_fk_refs(&spec, &raw_values, &mut fields, |object_type, _| {
            if object_type == "conversation.message" {
                Ok(FkLookup::Pending)
            } else {
                Ok(FkLookup::Resolved(format!("{object_type}-x")))
            }
        })
        .unwrap();
        assert!(result.is_none());
    }

    /// 外键为 null 时不需要 ref，正常返回
    #[test]
    fn fk_refs_ignore_null_foreign_keys() {
        let spec = message_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(9)),
            ("conversation_id".to_string(), json!(3)),
            ("parent_id".to_string(), JsonValue::Null),
        ]);
        let mut fields = Map::new();
        let refs = build_fk_refs(&spec, &raw_values, &mut fields, |object_type, value| {
            Ok(FkLookup::Resolved(format!("{object_type}-{value}")))
        })
        .unwrap()
        .expect("null foreign key needs no ref");
        assert!(!refs.contains_key("parent_id"));
    }

    /// 悬空 llm_model_id：依赖行不存在时按 NULL 处理，仍可产出快照（不永久卡住）
    #[test]
    fn fk_refs_dangling_llm_model_id_nulls_field_and_pushes() {
        let spec = message_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(1)),
            ("conversation_id".to_string(), json!(10)),
            ("parent_id".to_string(), JsonValue::Null),
            ("llm_model_id".to_string(), json!(1)),
            ("content".to_string(), json!("hello")),
        ]);
        let mut fields = Map::from_iter([
            ("conversation_id".to_string(), json!(10)),
            ("llm_model_id".to_string(), json!(1)),
            ("content".to_string(), json!("hello")),
        ]);
        let refs = build_fk_refs(&spec, &raw_values, &mut fields, |object_type, value| {
            match object_type {
                "conversation" => Ok(FkLookup::Resolved(format!("conv-{value}"))),
                "llm.model" => Ok(FkLookup::Dangling),
                other => panic!("unexpected fk target {other}"),
            }
        })
        .unwrap()
        .expect("dangling llm_model_id must not block snapshot");
        assert_eq!(refs.get("conversation_id").unwrap(), &json!("conv-10"));
        assert!(!refs.contains_key("llm_model_id"));
        assert_eq!(fields.get("llm_model_id").unwrap(), &JsonValue::Null);
    }

    /// provider_id=0 是未选 provider 哨兵：保留 0、不进 refs、不挡快照
    #[test]
    fn fk_refs_provider_id_zero_kept_as_sentinel() {
        let spec = assistant_model_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(1)),
            ("assistant_id".to_string(), json!(1)),
            ("provider_id".to_string(), json!(0)),
            ("model_code".to_string(), json!("")),
        ]);
        let mut fields = Map::from_iter([
            ("assistant_id".to_string(), json!(1)),
            ("provider_id".to_string(), json!(0)),
            ("model_code".to_string(), json!("")),
        ]);
        let refs = build_fk_refs(&spec, &raw_values, &mut fields, |object_type, value| {
            match object_type {
                "assistant" => Ok(FkLookup::Resolved(format!("asst-{value}"))),
                "llm.provider" => panic!("provider_id=0 must not call resolve"),
                other => panic!("unexpected fk target {other}"),
            }
        })
        .unwrap()
        .expect("provider_id=0 must not block assistant.model");
        assert_eq!(refs.get("assistant_id").unwrap(), &json!("asst-1"));
        assert!(!refs.contains_key("provider_id"));
        assert_eq!(fields.get("provider_id").unwrap(), &json!(0));
    }

    /// provider_id=0 round-trip：push 侧保留 0；apply 侧 refs 缺失/NULL → 写回 0
    #[test]
    fn provider_id_zero_round_trip_on_resolve_remote() {
        let spec = assistant_model_spec();
        let mut fields = Map::from_iter([
            ("assistant_id".to_string(), json!(1)),
            ("provider_id".to_string(), JsonValue::Null),
            ("model_code".to_string(), json!("")),
        ]);
        let refs = Map::from_iter([("assistant_id".to_string(), json!("asst-1"))]);
        resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(Some(10))).unwrap();
        assert_eq!(fields.get("assistant_id").unwrap(), &json!(10));
        assert_eq!(fields.get("provider_id").unwrap(), &json!(0));

        let mut fields_keep = Map::from_iter([
            ("assistant_id".to_string(), json!(1)),
            ("provider_id".to_string(), json!(0)),
            ("model_code".to_string(), json!("")),
        ]);
        resolve_remote_foreign_keys(&spec, &mut fields_keep, &refs, |_, _| Ok(Some(10))).unwrap();
        assert_eq!(fields_keep.get("provider_id").unwrap(), &json!(0));
    }

    /// 真实「未映射但行存在」的依赖仍跳过（与悬空外键区分）
    #[test]
    fn fk_refs_pending_existing_dependency_still_skips() {
        let spec = message_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(2)),
            ("conversation_id".to_string(), json!(10)),
            ("llm_model_id".to_string(), json!(5)),
        ]);
        let mut fields = Map::from_iter([
            ("conversation_id".to_string(), json!(10)),
            ("llm_model_id".to_string(), json!(5)),
        ]);
        let result = build_fk_refs(&spec, &raw_values, &mut fields, |object_type, value| {
            match object_type {
                "conversation" => Ok(FkLookup::Resolved(format!("conv-{value}"))),
                // llm.model id=5 本地存在但尚未写入 sync_object_map
                "llm.model" => Ok(FkLookup::Pending),
                other => panic!("unexpected fk target {other}"),
            }
        })
        .unwrap();
        assert!(result.is_none(), "existing-but-unmapped dependency must still skip");
        // fields 未被清空（跳过时不应改写为 NULL）
        assert_eq!(fields.get("llm_model_id").unwrap(), &json!(5));
    }

    /// 接收端：refs 完整时外键被改写为本机 local_id
    #[test]
    fn remote_fk_resolved_from_refs() {
        let spec = message_spec();
        let mut fields = Map::from_iter([
            ("conversation_id".to_string(), json!(3)),
            ("parent_id".to_string(), json!(7)),
            ("content".to_string(), json!("hi")),
        ]);
        let refs = Map::from_iter([
            ("conversation_id".to_string(), json!("conv-uuid")),
            ("parent_id".to_string(), json!("msg-uuid")),
        ]);
        resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, object_id| {
            Ok(Some(if object_id == "conv-uuid" { 100 } else { 200 }))
        })
        .unwrap();
        assert_eq!(fields.get("conversation_id").unwrap(), &json!(100));
        assert_eq!(fields.get("parent_id").unwrap(), &json!(200));
    }

    /// 接收端：fields 有值但 refs 缺失时拒绝写入（防止写入来源设备的本地 rowid）
    #[test]
    fn remote_fk_missing_ref_with_raw_value_is_rejected() {
        let spec = message_spec();
        let mut fields = Map::from_iter([
            ("conversation_id".to_string(), json!(3)),
            ("content".to_string(), json!("hi")),
        ]);
        let refs = Map::new();
        let result = resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(None));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("缺少外键引用"));
    }

    /// 接收端：外键为 null 且 refs 缺失时允许（无需解析）
    #[test]
    fn remote_fk_null_without_ref_is_allowed() {
        let spec = message_spec();
        let mut fields = Map::from_iter([
            ("conversation_id".to_string(), JsonValue::Null),
            ("content".to_string(), json!("hi")),
        ]);
        let refs = Map::new();
        resolve_remote_foreign_keys(&spec, &mut fields, &refs, |_, _| Ok(None)).unwrap();
        assert_eq!(fields.get("conversation_id").unwrap(), &JsonValue::Null);
    }

    fn test_sync_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SYNC_SCHEMA_SQL).unwrap();
        conn
    }

    fn seed_shadow(conn: &Connection, object_id: &str, deleted: bool) {
        conn.execute(
            "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
             VALUES ('conversation', ?1, 3, 'hash', ?2, 'now')",
            params![object_id, if deleted { Some("now".to_string()) } else { None }],
        )
        .unwrap();
    }

    /// 删除检测：shadow 有而快照无（非墓碑）的对象成为 delete 候选；
    /// 墓碑与仍存在的对象不产生候选
    #[test]
    fn deleted_candidates_skip_tombstones_and_present_objects() {
        let conn = test_sync_conn();
        seed_shadow(&conn, "alive", false);
        seed_shadow(&conn, "gone", false);
        seed_shadow(&conn, "tombstoned", true);
        let present = HashSet::from_iter(["alive".to_string()]);
        let candidates =
            find_deleted_candidates(&conn, "conversation", &present).unwrap();
        assert_eq!(candidates, vec![("gone".to_string(), 3)]);
    }

    /// delete 事件入队：正常插入、重复去重、并清掉同对象的 pending upsert
    #[test]
    fn enqueue_delete_event_dedups_and_drops_stale_upserts() {
        let conn = test_sync_conn();
        let spec = message_spec();
        conn.execute(
            "INSERT INTO sync_outbox
             (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
             VALUES ('e1', 'conversation.message', 'obj-1', 'upsert', '{}', NULL, 1, 'dev', 'now', 'pending')",
            [],
        )
        .unwrap();

        assert!(enqueue_delete_event(&conn, &spec, "obj-1", 7, "dev").unwrap());
        // 重复入队被去重
        assert!(!enqueue_delete_event(&conn, &spec, "obj-1", 7, "dev").unwrap());

        let events: Vec<(String, Option<i64>)> = conn
            .prepare("SELECT operation, base_version FROM sync_outbox ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        // 只剩一条 delete 事件，upsert 被移除
        assert_eq!(events, vec![("delete".to_string(), Some(7))]);
    }

    /// delete ack：shadow 置墓碑且 object_map 映射被清理；upsert ack 正常刷新
    #[test]
    fn ack_delete_marks_tombstone_and_clears_object_map() {
        let conn = test_sync_conn();
        seed_shadow(&conn, "obj-1", false);
        conn.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES ('conversation', 'conversation', 42, 'obj-1', 'now')",
            [],
        )
        .unwrap();

        apply_ack_to_shadow(&conn, "conversation", "obj-1", 4, "delete", "hash").unwrap();
        let deleted_at: Option<String> = conn
            .query_row(
                "SELECT deleted_at FROM sync_shadow WHERE object_type = 'conversation' AND object_id = 'obj-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
        let map_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_object_map WHERE sync_id = 'obj-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(map_count, 0);

        // upsert ack 会清除墓碑标记（对象复活场景）
        apply_ack_to_shadow(&conn, "conversation", "obj-1", 5, "upsert", "hash2").unwrap();
        let (deleted_at, version): (Option<String>, i64) = conn
            .query_row(
                "SELECT deleted_at, server_version FROM sync_shadow WHERE object_id = 'obj-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(deleted_at.is_none());
        assert_eq!(version, 5);
    }

    /// 过期 upsert 检测：不在 present 集合中的 pending/failed upsert 被列出
    #[test]
    fn stale_pending_upserts_detects_vanished_rows() {
        let conn = test_sync_conn();
        for (event_id, object_id, status) in
            [("e1", "kept", "pending"), ("e2", "vanished", "pending"), ("e3", "gone-failed", "failed"), ("e4", "acked-obj", "acked")]
        {
            conn.execute(
                "INSERT INTO sync_outbox
                 (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
                 VALUES (?1, 'message', ?2, 'upsert', '{}', NULL, 1, 'dev', 'now', ?3)",
                params![event_id, object_id, status],
            )
            .unwrap();
        }
        let present = HashSet::from_iter(["kept".to_string(), "acked-obj".to_string()]);
        let mut stale = find_stale_pending_upserts(&conn, "message", &present).unwrap();
        stale.sort();
        assert_eq!(stale, vec!["gone-failed".to_string(), "vanished".to_string()]);
    }

    /// failed 重试前用 shadow 的 server_version 刷新 base_version，消除冲突死循环
    #[test]
    fn reset_failed_events_refreshes_base_version() {
        let conn = test_sync_conn();
        seed_shadow(&conn, "obj-1", false); // server_version = 3
        conn.execute(
            "INSERT INTO sync_outbox
             (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
             VALUES ('e1', 'conversation', 'obj-1', 'upsert', '{}', 1, 1, 'dev', 'now', 'failed')",
            [],
        )
        .unwrap();
        // 没有 shadow 的 failed 事件保持原 base_version
        conn.execute(
            "INSERT INTO sync_outbox
             (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
             VALUES ('e2', 'conversation', 'obj-2', 'upsert', '{}', 2, 1, 'dev', 'now', 'failed')",
            [],
        )
        .unwrap();

        let reset = reset_failed_events_inner(&conn).unwrap();
        assert_eq!(reset, 2);
        let rows: Vec<(String, Option<i64>, String)> = conn
            .prepare("SELECT object_id, base_version, status FROM sync_outbox ORDER BY event_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("obj-1".to_string(), Some(3), "pending".to_string()),
                ("obj-2".to_string(), Some(2), "pending".to_string()),
            ]
        );
    }

    /// 官方 object_id 的生成与解析（含类型不匹配/非法格式）
    #[test]
    fn official_object_id_roundtrip() {
        let id = official_object_id("llm.provider", 10);
        assert_eq!(id, "official:llm.provider:10");
        assert_eq!(parse_official_object_id(&id, "llm.provider"), Some(10));
        assert_eq!(parse_official_object_id(&id, "mcp.server"), None);
        assert_eq!(
            parse_official_object_id("550e8400-e29b-41d4-a716-446655440000", "llm.provider"),
            None
        );
        assert_eq!(parse_official_object_id("natural:abc", "llm.provider"), None);
    }

    /// 官方行的 map 迁移：已存在的随机 UUID 映射被就地替换为确定性 id
    #[test]
    fn official_object_map_migrates_random_uuid() {
        let conn = test_sync_conn();
        conn.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES ('llm.provider', 'llm_provider', 1, 'random-uuid-1', 'now')",
            [],
        )
        .unwrap();
        let sync_id = upsert_official_object_map(&conn, "llm.provider", "llm_provider", 1).unwrap();
        assert_eq!(sync_id, "official:llm.provider:1");
        let stored: String = conn
            .query_row(
                "SELECT sync_id FROM sync_object_map WHERE object_type = 'llm.provider' AND local_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "official:llm.provider:1");
        // 幂等
        upsert_official_object_map(&conn, "llm.provider", "llm_provider", 1).unwrap();
    }

    /// 确定性 id 插入：insert_row_with_id 使用给定 id，insert_row 保持自增
    #[test]
    fn insert_row_with_id_uses_given_id() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE llm_provider (
                id INTEGER PRIMARY KEY,
                name TEXT, api_type TEXT, description TEXT,
                is_official INTEGER, is_enabled INTEGER
            );",
        )
        .unwrap();
        let spec = SyncTableSpec {
            db_name: "llm.db",
            table: "llm_provider",
            object_type: "llm.provider",
            columns: &["id", "name", "api_type", "description", "is_official", "is_enabled"],
            natural_key_columns: &[],
            unique_key_columns: &[],
            seed_official_ids: &[],
            official_column: Some("is_official"),
            foreign_keys: &[],
            where_clause: None,
            order_by: "id",
        };
        let fields = Map::from_iter([
            ("name".to_string(), json!("OpenAI")),
            ("api_type".to_string(), json!("openai_api")),
            ("is_official".to_string(), json!(1)),
        ]);
        let id = insert_row_with_id(&conn, &spec, 1, &fields).unwrap();
        assert_eq!(id, 1);
        assert!(row_exists_by_id(&conn, &spec, 1).unwrap());
        let auto_id = insert_row(&conn, &spec, &fields).unwrap();
        assert_ne!(auto_id, 1);
    }

    /// 推送失败退避：低于上限保持 pending 并设置 next_retry_at，达到上限转 failed
    #[test]
    fn record_event_failure_backs_off_then_fails() {
        let conn = test_sync_conn();
        conn.execute(
            "INSERT INTO sync_outbox
             (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
             VALUES ('e1', 'conversation', 'obj-1', 'upsert', '{}', NULL, 1, 'dev', 'now', 'pushing')",
            [],
        )
        .unwrap();

        for expected_count in 1..MAX_EVENT_RETRY_COUNT {
            record_event_failure(&conn, 1, "network down").unwrap();
            let (status, retry_count, next_retry_at): (String, i64, Option<i64>) = conn
                .query_row(
                    "SELECT status, retry_count, next_retry_at FROM sync_outbox WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(status, "pending", "retry {expected_count} should stay pending");
            assert_eq!(retry_count, expected_count);
            assert!(next_retry_at.is_some_and(|t| t > Utc::now().timestamp()));
        }
        record_event_failure(&conn, 1, "network down").unwrap();
        let (status, retry_count, next_retry_at): (String, i64, Option<i64>) = conn
            .query_row(
                "SELECT status, retry_count, next_retry_at FROM sync_outbox WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(retry_count, MAX_EVENT_RETRY_COUNT);
        assert_eq!(next_retry_at, None);
    }

    /// 入队新 upsert 前清理同对象的 failed 旧事件，pending 事件不受影响
    #[test]
    fn drop_failed_upserts_keeps_pending_events() {
        let conn = test_sync_conn();
        for (event_id, status) in [("e1", "failed"), ("e2", "failed"), ("e3", "pending")] {
            conn.execute(
                "INSERT INTO sync_outbox
                 (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
                 VALUES (?1, 'conversation', 'obj-1', 'upsert', '{}', NULL, 1, 'dev', 'now', ?2)",
                params![event_id, status],
            )
            .unwrap();
        }
        let dropped = drop_failed_upserts_for_object(&conn, "conversation", "obj-1").unwrap();
        assert_eq!(dropped, 2);
        let remaining: Vec<String> = conn
            .prepare("SELECT event_id FROM sync_outbox")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(remaining, vec!["e3".to_string()]);
    }

    /// needs_reset 元数据的设置与读取
    #[test]
    fn needs_reset_roundtrip() {
        let conn = test_sync_conn();
        assert!(!needs_reset(&conn).unwrap());
        set_sync_meta(&conn, META_NEEDS_RESET, "1").unwrap();
        assert!(needs_reset(&conn).unwrap());
        set_sync_meta(&conn, META_NEEDS_RESET, "1").unwrap();
        assert!(needs_reset(&conn).unwrap());
    }

    /// has_sync_state：任一状态表有记录即视为已有同步状态
    #[test]
    fn has_sync_state_detects_any_state_table() {
        let conn = test_sync_conn();
        assert!(!has_sync_state(&conn).unwrap());
        seed_shadow(&conn, "obj-1", false);
        assert!(has_sync_state(&conn).unwrap());
    }

    /// clear_sync_state 清空全部状态表并解除 needs_reset
    #[test]
    fn clear_sync_state_wipes_all_state() {
        let conn = test_sync_conn();
        seed_shadow(&conn, "obj-1", false);
        conn.execute(
            "INSERT INTO sync_cursor (scope, server_cursor, updated_at) VALUES ('default', 10, 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES ('conversation', 'conversation', 1, 'obj-1', 'now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sync_outbox
             (event_id, object_type, object_id, operation, payload_json, base_version, local_version, device_id, created_at, status)
             VALUES ('e1', 'conversation', 'obj-1', 'delete', NULL, 3, 1, 'dev', 'now', 'pending')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sync_dead_letter (object_type, object_id, operation, change_json, error, failed_at, retry_count)
             VALUES ('conversation', 'obj-2', 'upsert', '{}', 'err', 'now', 0)",
            [],
        )
        .unwrap();
        set_sync_meta(&conn, META_NEEDS_RESET, "1").unwrap();

        clear_sync_state(&conn).unwrap();
        assert!(!needs_reset(&conn).unwrap());
        assert!(!has_sync_state(&conn).unwrap());
        for table in ["sync_outbox", "sync_dead_letter"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table} should be empty");
        }
    }

    fn message_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                message_type TEXT NOT NULL,
                content TEXT NOT NULL DEFAULT '',
                parent_id INTEGER,
                finish_time TEXT,
                generation_group_id TEXT,
                parent_group_id TEXT
            );",
        )
        .unwrap();
        conn
    }

    fn insert_msg(
        conn: &Connection,
        conversation_id: i64,
        message_type: &str,
        generation_group_id: Option<&str>,
        parent_group_id: Option<&str>,
        parent_id: Option<i64>,
        finish_time: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO message
             (conversation_id, message_type, content, parent_id, finish_time, generation_group_id, parent_group_id)
             VALUES (?1, ?2, 'x', ?3, ?4, ?5, ?6)",
            params![
                conversation_id,
                message_type,
                parent_id,
                finish_time,
                generation_group_id,
                parent_group_id,
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn concurrent_divergence_regression_nonempty_parent_id_none_no_fork() {
        // Guarantee: non-empty conversation + parent_id None normal insert must NOT fork
        // when there is no unconfirmed local tip / runtime / parent_group sibling.
        let input = divergence::DivergenceInput {
            is_new_insert: true,
            ..divergence::DivergenceInput::default()
        };
        assert!(!divergence::detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn concurrent_divergence_legacy_parent_id_sibling() {
        let mut input = divergence::DivergenceInput {
            is_new_insert: true,
            remote_parent_id: Some(42),
            sibling_exists_under_parent_id: true,
            ..divergence::DivergenceInput::default()
        };
        assert!(divergence::detect_concurrent_message_divergence(&input));
        input.sibling_exists_under_parent_id = false;
        assert!(!divergence::detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn concurrent_divergence_parent_group_sibling_and_conflict_path_inputs() {
        let mut input = divergence::DivergenceInput {
            is_new_insert: true,
            remote_parent_group_id: Some("pg".into()),
            remote_generation_group_id: Some("g-remote".into()),
            local_has_sibling_under_parent_group: true,
            ..divergence::DivergenceInput::default()
        };
        assert!(divergence::detect_concurrent_message_divergence(&input));
        // Conflict path reuses the same detector.
        input.local_has_sibling_under_parent_group = false;
        input.local_tip_generation_group_id = Some("pg".into());
        assert!(!divergence::detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn common_history_fork_point_excludes_unconfirmed_local_tip() {
        let data = message_test_db();
        let sync = test_sync_conn();
        let common = insert_msg(&data, 1, "user", Some("g0"), None, None, Some("t0"));
        let common_resp = insert_msg(&data, 1, "response", Some("g0"), None, None, Some("t1"));
        let local_tip = insert_msg(&data, 1, "user", Some("g-local"), None, None, Some("t2"));

        sync.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES
             ('conversation.message', 'message', ?1, 'sync-common-u', 'now'),
             ('conversation.message', 'message', ?2, 'sync-common-r', 'now'),
             ('conversation.message', 'message', ?3, 'sync-local-tip', 'now')",
            params![common, common_resp, local_tip],
        )
        .unwrap();
        for sid in ["sync-common-u", "sync-common-r"] {
            sync.execute(
                "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
                 VALUES ('conversation.message', ?1, 1, 'h', NULL, 'now')",
                params![sid],
            )
            .unwrap();
        }

        assert!(local_has_unconfirmed_tip(&data, &sync, 1, None).unwrap());
        let fork_at = common_history_tip_message_id(&data, &sync, 1, None).unwrap();
        assert_eq!(fork_at, Some(common_resp));
        // parent_id None → copy common history only, not local tip
        assert_eq!(
            resolve_sync_fork_point(&data, &sync, 1, None, None, None).unwrap(),
            Some(common_resp)
        );
        assert_ne!(fork_at, Some(local_tip));
    }

    #[test]
    fn gather_divergence_normal_insert_into_synced_nonempty_does_not_fork() {
        let data = message_test_db();
        let sync = test_sync_conn();
        let m1 = insert_msg(&data, 5, "user", Some("g0"), None, None, Some("t0"));
        let m2 = insert_msg(&data, 5, "response", Some("g0"), None, None, Some("t1"));
        sync.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES
             ('conversation.message', 'message', ?1, 's1', 'now'),
             ('conversation.message', 'message', ?2, 's2', 'now')",
            params![m1, m2],
        )
        .unwrap();
        for sid in ["s1", "s2"] {
            sync.execute(
                "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
                 VALUES ('conversation.message', ?1, 1, 'h', NULL, 'now')",
                params![sid],
            )
            .unwrap();
        }

        let fields = Map::from_iter([
            ("conversation_id".to_string(), json!(5)),
            ("parent_id".to_string(), JsonValue::Null),
            ("generation_group_id".to_string(), json!("g-remote")),
            ("parent_group_id".to_string(), JsonValue::Null),
        ]);
        let input = gather_divergence_input(&data, &sync, &fields, None, true).unwrap();
        assert!(!input.local_has_unconfirmed_tip);
        assert!(!input.local_runtime_running);
        assert!(!divergence::detect_concurrent_message_divergence(&input));
    }

    #[test]
    fn gather_divergence_unconfirmed_local_tip_forks_and_schedules_common_history() {
        let data = message_test_db();
        let sync = test_sync_conn();
        let common = insert_msg(&data, 9, "response", Some("g0"), None, None, Some("t0"));
        let _local = insert_msg(&data, 9, "user", Some("g-local"), None, None, Some("t1"));
        sync.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES ('conversation.message', 'message', ?1, 's-common', 'now')",
            params![common],
        )
        .unwrap();
        sync.execute(
            "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
             VALUES ('conversation.message', 's-common', 1, 'h', NULL, 'now')",
            [],
        )
        .unwrap();

        let fields = Map::from_iter([
            ("conversation_id".to_string(), json!(9)),
            ("parent_id".to_string(), JsonValue::Null),
            ("content".to_string(), json!("remote")),
        ]);
        let input = gather_divergence_input(&data, &sync, &fields, None, true).unwrap();
        assert!(input.local_has_unconfirmed_tip);
        assert!(divergence::detect_concurrent_message_divergence(&input));
        let fork_at = resolve_sync_fork_point(&data, &sync, 9, None, None, None).unwrap();
        assert_eq!(fork_at, Some(common));
    }

    fn mcp_server_tool_spec() -> SyncTableSpec {
        SyncTableSpec {
            db_name: "mcp.db",
            table: "mcp_server_tool",
            object_type: "mcp.server_tool",
            columns: &[
                "id",
                "server_id",
                "tool_name",
                "tool_description",
                "is_enabled",
                "is_auto_run",
                "parameters",
                "created_time",
            ],
            natural_key_columns: &[],
            unique_key_columns: &["server_id", "tool_name"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[ForeignKeySpec {
                column: "server_id",
                object_type: "mcp.server",
            }],
            where_clause: None,
            order_by: "id",
        }
    }

    fn assistant_model_config_spec() -> SyncTableSpec {
        SyncTableSpec {
            db_name: "assistant.db",
            table: "assistant_model_config",
            object_type: "assistant.model_config",
            columns: &["id", "assistant_id", "assistant_model_id", "name", "value", "value_type"],
            natural_key_columns: &[],
            unique_key_columns: &["assistant_id", "name"],
            seed_official_ids: &[],
            official_column: None,
            foreign_keys: &[
                ForeignKeySpec {
                    column: "assistant_id",
                    object_type: "assistant",
                },
                ForeignKeySpec {
                    column: "assistant_model_id",
                    object_type: "assistant.model",
                },
            ],
            where_clause: None,
            order_by: "id",
        }
    }

    /// 把远端 builtin tool 应用到已有种子行：UNIQUE 合并 + sync_id 重映射为远端
    #[test]
    fn apply_builtin_tool_onto_existing_seed_merges_unique_and_remaps() {
        let sync = test_sync_conn();
        let data = Connection::open_in_memory().unwrap();
        data.execute_batch(
            "CREATE TABLE mcp_server_tool (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server_id INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                tool_description TEXT,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                is_auto_run INTEGER NOT NULL DEFAULT 0,
                parameters TEXT,
                created_time TEXT,
                UNIQUE(server_id, tool_name)
            );",
        )
        .unwrap();
        data.execute(
            "INSERT INTO mcp_server_tool (id, server_id, tool_name, tool_description, is_enabled, is_auto_run)
             VALUES (1, 1, 'read_file', 'local seed', 1, 0)",
            [],
        )
        .unwrap();
        // 本地种子曾用随机 UUID 映射
        sync.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES ('mcp.server_tool', 'mcp_server_tool', 1, 'local-random-uuid', 'now')",
            [],
        )
        .unwrap();

        let spec = mcp_server_tool_spec();
        let remote_id = "official:mcp.server_tool:official:mcp.server:1:read_file";
        let fields = Map::from_iter([
            ("server_id".to_string(), json!(1)),
            ("tool_name".to_string(), json!("read_file")),
            ("tool_description".to_string(), json!("from remote")),
            ("is_enabled".to_string(), json!(1)),
            ("is_auto_run".to_string(), json!(0)),
        ]);
        upsert_by_unique_key(&sync, &data, &spec, remote_id, &fields).unwrap();

        let count: i64 = data
            .query_row("SELECT COUNT(*) FROM mcp_server_tool", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "must not insert a second tool row");
        let desc: String = data
            .query_row(
                "SELECT tool_description FROM mcp_server_tool WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(desc, "from remote");
        let mapped: String = sync
            .query_row(
                "SELECT sync_id FROM sync_object_map WHERE object_type = 'mcp.server_tool' AND local_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mapped, remote_id);
        let row_count: i64 = sync
            .query_row("SELECT COUNT(*) FROM sync_object_map WHERE object_type = 'mcp.server_tool'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count, 1, "must not leave two sync ids for one row");
    }

    /// 父 model 合并成功后，model_config 依赖可解析并按 (assistant_id, name) upsert
    #[test]
    fn model_config_applies_after_parent_model_unique_merge() {
        let sync = test_sync_conn();
        let data = Connection::open_in_memory().unwrap();
        data.execute_batch(
            "CREATE TABLE assistant_model (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER NOT NULL,
                provider_id INTEGER NOT NULL,
                model_code TEXT NOT NULL,
                alias TEXT
            );
            CREATE TABLE assistant_model_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                assistant_id INTEGER,
                assistant_model_id INTEGER,
                name TEXT NOT NULL,
                value TEXT,
                value_type TEXT NOT NULL DEFAULT 'float',
                UNIQUE(assistant_id, name)
            );",
        )
        .unwrap();
        data.execute(
            "INSERT INTO assistant_model (id, assistant_id, provider_id, model_code, alias)
             VALUES (1, 1, 0, '', '')",
            [],
        )
        .unwrap();
        data.execute(
            "INSERT INTO assistant_model_config (id, assistant_id, assistant_model_id, name, value, value_type)
             VALUES (1, 1, 1, 'temperature', '0.75', 'float')",
            [],
        )
        .unwrap();

        let model_spec = assistant_model_spec();
        let remote_model = "official:assistant.model:official:assistant:1:0:";
        let model_fields = Map::from_iter([
            ("assistant_id".to_string(), json!(1)),
            ("provider_id".to_string(), json!(0)),
            ("model_code".to_string(), json!("")),
            ("alias".to_string(), json!("")),
        ]);
        upsert_by_unique_key(&sync, &data, &model_spec, remote_model, &model_fields).unwrap();
        let model_local = find_local_id_by_object_id(&sync, "assistant.model", remote_model)
            .unwrap()
            .expect("model map after merge");
        assert_eq!(model_local, 1);

        // 模拟 resolve_remote_foreign_keys 后的 fields
        let config_spec = assistant_model_config_spec();
        let remote_config = "official:assistant.model_config:official:assistant:1:temperature";
        let config_fields = Map::from_iter([
            ("assistant_id".to_string(), json!(1)),
            ("assistant_model_id".to_string(), json!(model_local)),
            ("name".to_string(), json!("temperature")),
            ("value".to_string(), json!("0.9")),
            ("value_type".to_string(), json!("float")),
        ]);
        upsert_by_unique_key(&sync, &data, &config_spec, remote_config, &config_fields).unwrap();

        let count: i64 = data
            .query_row("SELECT COUNT(*) FROM assistant_model_config", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let value: String = data
            .query_row(
                "SELECT value FROM assistant_model_config WHERE assistant_id = 1 AND name = 'temperature'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "0.9");
        let mapped = find_local_id_by_object_id(&sync, "assistant.model_config", remote_config)
            .unwrap()
            .expect("config mapped");
        assert_eq!(mapped, 1);
    }

    /// 内置父 server 下的 tool 生成确定性 child object_id
    #[test]
    fn deterministic_child_object_id_for_builtin_server_tool() {
        let sync = test_sync_conn();
        sync.execute(
            "INSERT INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
             VALUES ('mcp.server', 'mcp_server', 1, 'official:mcp.server:1', 'now')",
            [],
        )
        .unwrap();
        let spec = mcp_server_tool_spec();
        let raw = HashMap::from([
            ("id".to_string(), json!(7)),
            ("server_id".to_string(), json!(1)),
            ("tool_name".to_string(), json!("read_file")),
        ]);
        let id = try_deterministic_child_object_id(&sync, &spec, &raw)
            .unwrap()
            .expect("builtin parent should yield deterministic id");
        assert_eq!(
            id,
            "official:mcp.server_tool:official:mcp.server:1:read_file"
        );
    }

    #[test]
    fn after_fork_ensure_object_id_maps_forked_conversation_for_upload() {
        // Sync-map scheduling contract used by maybe_fork / conflict path:
        // forked conversation must get a sync_id so scan uploads C'.
        let sync = test_sync_conn();
        let forked_local_id = 77_i64;
        let sync_id = ensure_object_id(&sync, "conversation", "conversation", forked_local_id).unwrap();
        let mapped: i64 = sync
            .query_row(
                "SELECT local_id FROM sync_object_map
                 WHERE object_type = 'conversation' AND sync_id = ?1",
                params![sync_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mapped, forked_local_id);
        // Second call is stable (upload scheduling idempotent).
        let again = ensure_object_id(&sync, "conversation", "conversation", forked_local_id).unwrap();
        assert_eq!(sync_id, again);
    }
}

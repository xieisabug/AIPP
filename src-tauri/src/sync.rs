use std::collections::{HashMap, HashSet};
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

const FEATURE_CODE: &str = "data_sync";
const TOKEN_SCOPE: &str = "data_sync";
const TOKEN_KEY: &str = "token";
const SECURE_MASTER_KEY_FILE: &str = "secure-config-master-key.bin";
const DEFAULT_SCOPE: &str = "default";
const CLIENT_SCHEMA_VERSION: i64 = 1;
const SYNC_INTERVAL_SECS: u64 = 45;
const DEBOUNCE_MS: u64 = 1200;
const HTTP_TIMEOUT_SECS: u64 = 30;

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
    server_cursor: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveSyncSettingsRequest {
    mode: SyncMode,
    server_url: Option<String>,
    token: Option<String>,
}

#[derive(Default)]
pub struct SyncState {
    worker_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    debounce_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    sync_lock: Mutex<()>,
    status: Mutex<SyncRuntimeStatus>,
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
            foreign_keys: &[ForeignKeySpec { column: "llm_provider_id", object_type: "llm.provider" }],
            where_clause: Some("LOWER(name) NOT LIKE '%key%' AND LOWER(name) NOT LIKE '%secret%' AND LOWER(name) NOT LIKE '%token%' AND LOWER(name) NOT LIKE '%password%'"),
            order_by: "id",
        },
        SyncTableSpec {
            db_name: "llm.db",
            table: "llm_model",
            object_type: "llm.model",
            columns: &["id", "name", "llm_provider_id", "code", "description", "vision_support", "audio_support", "video_support"],
            natural_key_columns: &[],
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
    let device_id = ensure_device_id(&app_handle)?;
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
    *task = Some(tauri::async_runtime::spawn(async move {
        loop {
            if !matches!(load_settings(&app).map(|s| s.mode), Ok(SyncMode::SelfHosted)) {
                break;
            }
            run_sync_once(&app).await;
            tokio::time::sleep(Duration::from_secs(SYNC_INTERVAL_SECS)).await;
        }
    }));

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
    let device_id = ensure_device_id(app_handle)?;
    let device_name = device_name();
    {
        let conn = open_sync_db(app_handle)?;
        if needs_reset(&conn)? {
            return Err(
                "检测到同步服务器或账号变更，请在设置中确认重置同步状态后再同步".to_string()
            );
        }
    }
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
    if remote_status.latest_cursor > get_cursor(app_handle)? {
        pull_remote(app_handle, &settings, token, &device_id).await?;
    } else {
        pull_remote(app_handle, &settings, token, &device_id).await?;
    }
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
        handle_push_response(app_handle, &events, body)?;
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
        let mut deferred: Vec<&PullChange> = Vec::new();
        for change in &body.changes {
            if let Err(err) = apply_change(app_handle, change, device_id) {
                if is_dependency_pending_error(&err) {
                    // 依赖对象可能在本页后面的 change 里：先记下，本页处理完后重放一次
                    deferred.push(change);
                } else {
                    record_dead_letter(app_handle, change, &err);
                }
            }
        }
        for change in deferred {
            if let Err(err) = apply_change(app_handle, change, device_id) {
                record_dead_letter(app_handle, change, &err);
            }
        }
        set_cursor(app_handle, body.cursor)?;
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

/// 判断 apply 失败是否属于"依赖对象尚未同步"类错误（可等后续变更就位后重试）。
fn is_dependency_pending_error(err: &str) -> bool {
    err.starts_with("缺少同步依赖") || err.starts_with("远端变更缺少外键引用")
}

/// 将无法应用的远端变更写入死信表；同一对象只保留最新一条。
/// 死信写入本身失败只记日志，不阻断 pull 主流程。
fn record_dead_letter(app_handle: &AppHandle, change: &PullChange, error: &str) {
    let result = (|| -> Result<(), String> {
        let change_json =
            serde_json::to_string(change).map_err(|e| format!("serialize dead letter: {e}"))?;
        let conn = open_sync_db(app_handle)?;
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
    let pending: Vec<(i64, String)> = {
        let conn = open_sync_db(app_handle)?;
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
                update_dead_letter_failure(app_handle, *id, &format!("死信反序列化失败: {err}"))?;
                continue;
            }
        };
        match apply_change(app_handle, &change, device_id) {
            Ok(()) => {
                let conn = open_sync_db(app_handle)?;
                conn.execute("DELETE FROM sync_dead_letter WHERE id = ?1", params![id])
                    .map_err(|e| e.to_string())?;
                resolved += 1;
            }
            Err(err) => {
                update_dead_letter_failure(app_handle, *id, &err)?;
            }
        }
    }
    let remaining = pending.len() - resolved;
    if resolved > 0 {
        debug!(resolved, remaining, "Replayed sync dead letters");
    }
    Ok((resolved, remaining))
}

fn update_dead_letter_failure(app_handle: &AppHandle, id: i64, error: &str) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
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

fn scan_local_changes(app_handle: &AppHandle) -> Result<usize, String> {
    ensure_sync_db(app_handle)?;
    let mut enqueued = 0;
    for spec in specs() {
        let (snapshots, present_ids) = read_local_snapshots(app_handle, &spec)?;
        for snapshot in snapshots {
            if enqueue_snapshot_if_changed(app_handle, &snapshot)? {
                enqueued += 1;
            }
        }
        enqueued += scan_local_deletes(app_handle, &spec, &present_ids)?;
    }
    Ok(enqueued)
}

/// 删除检测：shadow 中已同步（非墓碑）但本轮快照里不存在的对象，
/// 确认本地行确实不存在后入队 delete 事件。
/// 同时清理"本地行已删除但 outbox 里还留着 upsert"的过期事件。
fn scan_local_deletes(
    app_handle: &AppHandle,
    spec: &SyncTableSpec,
    present_ids: &HashSet<String>,
) -> Result<usize, String> {
    let conn = open_sync_db(app_handle)?;
    let device_id = ensure_device_id(app_handle)?;
    let mut enqueued = 0;

    let candidates = find_deleted_candidates(&conn, spec.object_type, present_ids)?;
    for (object_id, base_version) in candidates {
        // 行可能只是被 where_clause 过滤出同步范围，必须确认行真的不存在
        if local_row_exists(app_handle, spec, &object_id)? {
            continue;
        }
        if enqueue_delete_event(&conn, spec, &object_id, base_version, &device_id)? {
            enqueued += 1;
        }
    }

    // 新增后未推送即删除：没有 shadow，但 outbox 可能残留 pending/failed upsert，
    // 不清理的话会把已删除的对象推上服务器（幽灵复活）。
    let stale_upserts = find_stale_pending_upserts(&conn, spec.object_type, present_ids)?;
    for object_id in stale_upserts {
        if local_row_exists(app_handle, spec, &object_id)? {
            continue;
        }
        debug!(
            object_type = %spec.object_type,
            object_id = %object_id,
            "Dropping stale pending upsert for locally deleted row"
        );
        conn.execute(
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
    spec: &SyncTableSpec,
    object_id: &str,
) -> Result<bool, String> {
    let path = get_db_path(app_handle, spec.db_name)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    if spec.natural_key_columns.is_empty() {
        let Some(local_id) = find_local_id_by_object_id(app_handle, spec.object_type, object_id)?
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
) -> Result<(Vec<LocalObjectSnapshot>, HashSet<String>), String> {
    let path = get_db_path(app_handle, spec.db_name)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
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
        let object_id = if spec.natural_key_columns.is_empty() {
            ensure_object_id(app_handle, spec.object_type, spec.table, local_id)?
        } else {
            natural_object_id(spec, &raw_values)?
        };
        // 无论本行是否因外键未决而跳过，都要计入 present 集合，
        // 否则删除检测会把"暂时跳过"误判成"已删除"。
        present_ids.insert(object_id.clone());

        let Some(refs) = build_fk_refs(spec, &raw_values, |object_type, value| {
            find_object_id(app_handle, object_type, value)
        })?
        else {
            // 外键引用尚未建立映射（依赖对象未同步）：本轮跳过该行，
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

/// 构建一行的外键引用表。
///
/// 返回 `Ok(Some(refs))` 表示所有非空外键都已解析为同步 object_id；
/// 返回 `Ok(None)` 表示存在无法解析的外键（依赖对象尚未同步），
/// 调用方必须跳过该行，避免把本地 rowid 作为外键值泄露到 payload 中。
fn build_fk_refs(
    spec: &SyncTableSpec,
    raw_values: &HashMap<String, JsonValue>,
    mut resolve: impl FnMut(&str, i64) -> Result<Option<String>, String>,
) -> Result<Option<Map<String, JsonValue>>, String> {
    let mut refs = Map::new();
    for fk in spec.foreign_keys {
        if let Some(value) = raw_values.get(fk.column).and_then(JsonValue::as_i64) {
            match resolve(fk.object_type, value)? {
                Some(ref_id) => {
                    refs.insert(fk.column.to_string(), JsonValue::String(ref_id));
                }
                None => return Ok(None),
            }
        }
    }
    Ok(Some(refs))
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
    app_handle: &AppHandle,
    snapshot: &LocalObjectSnapshot,
) -> Result<bool, String> {
    let conn = open_sync_db(app_handle)?;
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

    let device_id = ensure_device_id(app_handle)?;
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

fn apply_change(app_handle: &AppHandle, change: &PullChange, local_device_id: &str) -> Result<(), String> {
    if change.device_id == local_device_id {
        upsert_shadow_from_remote(app_handle, change)?;
        return Ok(());
    }

    let Some(spec) = specs().into_iter().find(|s| s.object_type == change.object_type) else {
        warn!(object_type = %change.object_type, "Ignoring unknown sync object type");
        return Ok(());
    };
    let db_path = get_db_path(app_handle, spec.db_name)?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    if change.operation == "delete" {
        apply_delete(app_handle, &conn, &spec, change)?;
        upsert_shadow_from_remote(app_handle, change)?;
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
        find_local_id_by_object_id(app_handle, object_type, ref_object_id)
    })?;

    if let Some(local_id) = find_local_id_by_object_id(app_handle, &change.object_type, &change.object_id)? {
        update_row(&conn, &spec, local_id, &field_values)?;
    } else if !spec.natural_key_columns.is_empty() {
        upsert_natural_row(&conn, &spec, &field_values)?;
    } else {
        let new_id = insert_row(&conn, &spec, &field_values)?;
        save_object_map(app_handle, &change.object_type, spec.table, new_id, &change.object_id)?;
    }

    upsert_shadow_from_remote(app_handle, change)?;
    Ok(())
}

fn apply_delete(
    app_handle: &AppHandle,
    conn: &Connection,
    spec: &SyncTableSpec,
    change: &PullChange,
) -> Result<(), String> {
    if let Some(local_id) = find_local_id_by_object_id(app_handle, &change.object_type, &change.object_id)? {
        conn.execute(&format!("DELETE FROM {} WHERE id = ?1", spec.table), params![local_id])
            .map_err(|e| e.to_string())?;
        // 同步删除 object_map 映射：同 object_id 后续 upsert 时应重新 insert（对象复活），
        // 而不是命中这条指向已删除行的旧映射。
        let sync_conn = open_sync_db(app_handle)?;
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
    let columns = existing_columns(conn, spec.table, spec.columns)?;
    let mut insert_columns = Vec::new();
    let mut placeholders = Vec::new();
    let mut values = Vec::new();
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
             ORDER BY id
             LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([limit], |row| {
            let payload_json: Option<String> = row.get(5)?;
            let payload = payload_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<JsonValue>(raw).ok());
            Ok((
                row.get::<_, i64>(0)?,
                PushEvent {
                    event_id: row.get(1)?,
                    object_type: row.get(2)?,
                    object_id: row.get(3)?,
                    operation: row.get(4)?,
                    payload,
                    base_version: row.get(6)?,
                    local_version: row.get(7)?,
                    created_at: row.get(8)?,
                    client_schema_version: CLIENT_SCHEMA_VERSION,
                    object_schema_version: 1,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

fn mark_events_pushing(app_handle: &AppHandle, events: &[(i64, PushEvent)]) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    for (id, _) in events {
        conn.execute("UPDATE sync_outbox SET status = 'pushing' WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn mark_events_failed(
    app_handle: &AppHandle,
    events: &[(i64, PushEvent)],
    error: &str,
) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    for (id, _) in events {
        conn.execute(
            "UPDATE sync_outbox
             SET status = 'failed', retry_count = retry_count + 1, last_error = ?2
             WHERE id = ?1",
            params![id, error],
        )
        .map_err(|e| e.to_string())?;
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
    conn.execute(
        "UPDATE sync_outbox
         SET status = 'pending', last_error = NULL
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
) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    let by_event_id: HashMap<&str, &(i64, PushEvent)> =
        events.iter().map(|item| (item.1.event_id.as_str(), item)).collect();
    for accepted in response.accepted {
        if let Some((id, event)) = by_event_id.get(accepted.event_id.as_str()) {
            conn.execute("UPDATE sync_outbox SET status = 'acked', last_error = NULL WHERE id = ?1", params![id])
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
    let mut conflict_count = 0;
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

    for conflict in response.conflicts {
        if let Some((id, _)) = by_event_id.get(conflict.event_id.as_str()) {
            conflict_count += 1;
            let err = format!(
                "remote conflict on {}:{} at version {}",
                conflict.object_type, conflict.object_id, conflict.server_version
            );
            latest_failure = Some(err.clone());
            conn.execute(
                "UPDATE sync_outbox
                 SET status = 'failed', retry_count = retry_count + 1, last_error = ?2
                 WHERE id = ?1",
                params![id, err],
            )
            .map_err(|e| e.to_string())?;
            if let Some(payload) = conflict.server_payload {
                let hash = hash_text(&serde_json::to_string(&payload).unwrap_or_default());
                conn.execute(
                    "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, NULL, ?5)
                     ON CONFLICT(object_type, object_id)
                     DO UPDATE SET server_version = excluded.server_version,
                                   payload_hash = excluded.payload_hash,
                                   updated_at = excluded.updated_at",
                    params![conflict.object_type, conflict.object_id, conflict.server_version, hash, now_string()],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    if rejected_count > 0 || conflict_count > 0 {
        return Err(format!(
            "同步服务器拒绝 {} 条、冲突 {} 条；最近原因：{}",
            rejected_count,
            conflict_count,
            latest_failure.unwrap_or_else(|| "未提供原因".to_string())
        ));
    }
    Ok(())
}

fn upsert_shadow_from_remote(app_handle: &AppHandle, change: &PullChange) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
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
            last_error TEXT
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
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn ensure_device_id(app_handle: &AppHandle) -> Result<String, String> {
    ensure_sync_db(app_handle)?;
    let conn = open_sync_db(app_handle)?;
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
    app_handle: &AppHandle,
    object_type: &str,
    local_table: &str,
    local_id: i64,
) -> Result<String, String> {
    let conn = open_sync_db(app_handle)?;
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

fn save_object_map(
    app_handle: &AppHandle,
    object_type: &str,
    local_table: &str,
    local_id: i64,
    sync_id: &str,
) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_object_map (object_type, local_table, local_id, sync_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![object_type, local_table, local_id, sync_id, now_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn find_object_id(app_handle: &AppHandle, object_type: &str, local_id: i64) -> Result<Option<String>, String> {
    let conn = open_sync_db(app_handle)?;
    conn.query_row(
        "SELECT sync_id FROM sync_object_map WHERE object_type = ?1 AND local_id = ?2",
        params![object_type, local_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn find_local_id_by_object_id(
    app_handle: &AppHandle,
    object_type: &str,
    object_id: &str,
) -> Result<Option<i64>, String> {
    let conn = open_sync_db(app_handle)?;
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
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let where_sql = spec.where_clause.map(|w| format!(" WHERE {w}")).unwrap_or_default();
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
    let path = master_key_file_path(app_handle)?;
    if path.exists() {
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Failed to read secure master key `{}`: {}", path.display(), e))?;
        return bytes
            .try_into()
            .map_err(|_| format!("Invalid secure master key length in `{}`", path.display()));
    }
    let key: [u8; 32] = rand::random();
    std::fs::write(&path, key)
        .map_err(|e| format!("Failed to write secure master key `{}`: {}", path.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(key)
}

fn master_key_file_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let secret_dir = app_dir.join("local-secrets");
    std::fs::create_dir_all(&secret_dir).map_err(|e| e.to_string())?;
    Ok(secret_dir.join(SECURE_MASTER_KEY_FILE))
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
    fn secret_roundtrip_uses_aes_gcm_payload() {
        let key: [u8; 32] = [7; 32];
        let (ciphertext, nonce) = encrypt_secret_with_key(&key, "dev-token").unwrap();
        assert_ne!(ciphertext, "dev-token");
        assert_eq!(decrypt_secret_with_key(&key, &ciphertext, &nonce).unwrap(), "dev-token");
    }

    #[test]
    fn natural_object_id_roundtrip_preserves_parts() {
        let spec = SyncTableSpec {
            db_name: "system.db",
            table: "feature_config",
            object_type: "system.feature_config",
            columns: &["id", "feature_code", "key"],
            natural_key_columns: &["feature_code", "key"],
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
            columns: &["id", "conversation_id", "parent_id", "content"],
            natural_key_columns: &[],
            foreign_keys: &[
                ForeignKeySpec { column: "conversation_id", object_type: "conversation" },
                ForeignKeySpec { column: "parent_id", object_type: "conversation.message" },
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
        let refs = build_fk_refs(&spec, &raw_values, |object_type, value| {
            Ok(Some(format!("{object_type}-{value}")))
        })
        .unwrap()
        .expect("all foreign keys resolvable");
        assert_eq!(refs.get("conversation_id").unwrap(), &json!("conversation-3"));
        assert_eq!(refs.get("parent_id").unwrap(), &json!("conversation.message-7"));
    }

    /// 任一外键无法解析时返回 None（调用方跳过该行），不产出裸 rowid
    #[test]
    fn fk_refs_return_none_when_dependency_unresolved() {
        let spec = message_spec();
        let raw_values = HashMap::from([
            ("id".to_string(), json!(9)),
            ("conversation_id".to_string(), json!(3)),
            ("parent_id".to_string(), json!(7)),
        ]);
        let result = build_fk_refs(&spec, &raw_values, |object_type, _| {
            if object_type == "conversation.message" {
                Ok(None)
            } else {
                Ok(Some(format!("{object_type}-x")))
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
        let refs = build_fk_refs(&spec, &raw_values, |object_type, value| {
            Ok(Some(format!("{object_type}-{value}")))
        })
        .unwrap()
        .expect("null foreign key needs no ref");
        assert!(!refs.contains_key("parent_id"));
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
}

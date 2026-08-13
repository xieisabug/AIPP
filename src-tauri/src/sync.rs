use std::collections::HashMap;
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
use tracing::warn;
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

#[derive(Debug, Clone, Deserialize)]
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
            let token = token.unwrap_or_default().trim().to_string();
            let existing = load_sync_token(&app_handle)?;
            if token.is_empty() && existing.is_none() {
                return Err("自建同步模式需要填写访问 token".to_string());
            }
            if !token.is_empty() {
                save_sync_token(&app_handle, &token)?;
            }
            save_feature_config_value(&app_handle, &feature_state, "mode", "self_hosted").await?;
            save_feature_config_value(&app_handle, &feature_state, "server_url", &server_url).await?;
            start_worker(app_handle.clone()).await;
            run_sync_once(&app_handle).await;
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
        for change in &body.changes {
            apply_change(app_handle, change, device_id)?;
        }
        set_cursor(app_handle, body.cursor)?;
        emit_status_changed(app_handle).await;
        if !body.has_more {
            return Ok(());
        }
    }
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
        let snapshots = read_local_snapshots(app_handle, &spec)?;
        for snapshot in snapshots {
            if enqueue_snapshot_if_changed(app_handle, &snapshot)? {
                enqueued += 1;
            }
        }
    }
    Ok(enqueued)
}

fn read_local_snapshots(
    app_handle: &AppHandle,
    spec: &SyncTableSpec,
) -> Result<Vec<LocalObjectSnapshot>, String> {
    let path = get_db_path(app_handle, spec.db_name)?;
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let columns = existing_columns(&conn, spec.table, spec.columns)?;
    if columns.is_empty() || !columns.iter().any(|c| c == "id") {
        return Ok(Vec::new());
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

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut fields = Map::new();
        let mut refs = Map::new();
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

        for fk in spec.foreign_keys {
            if let Some(value) = raw_values.get(fk.column).and_then(JsonValue::as_i64) {
                if let Some(ref_id) = find_object_id(app_handle, fk.object_type, value)? {
                    refs.insert(fk.column.to_string(), JsonValue::String(ref_id));
                }
            }
        }

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
    Ok(snapshots)
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
    for fk in spec.foreign_keys {
        if let Some(JsonValue::String(ref_object_id)) = refs.get(fk.column) {
            let local_id = find_local_id_by_object_id(app_handle, fk.object_type, ref_object_id)?
                .ok_or_else(|| {
                    format!(
                        "缺少同步依赖: {}.{} -> {}",
                        change.object_type, fk.column, ref_object_id
                    )
                })?;
            field_values.insert(fk.column.to_string(), JsonValue::Number(local_id.into()));
        }
    }

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
            conn.execute(
                "INSERT INTO sync_shadow (object_type, object_id, server_version, payload_hash, deleted_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5)
                 ON CONFLICT(object_type, object_id)
                 DO UPDATE SET server_version = excluded.server_version,
                               payload_hash = excluded.payload_hash,
                               deleted_at = NULL,
                               updated_at = excluded.updated_at",
                params![
                    accepted.object_type,
                    accepted.object_id,
                    accepted.server_version,
                    payload_hash,
                    now_string(),
                ],
            )
            .map_err(|e| e.to_string())?;
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

fn ensure_sync_db(app_handle: &AppHandle) -> Result<(), String> {
    let conn = open_sync_db(app_handle)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_object_map (
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
        );",
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
}

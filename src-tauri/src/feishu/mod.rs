use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use openlark_client::ws_client::{EventDispatcherHandler, LarkWsClient};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::api::ai::types::AiRequest;
use crate::api::ai_api::ask_ai;
use crate::api::butler_api::{
    get_butler_main_continuation_lock, load_or_create_butler_main_internal,
    resolve_butler_execution_window, wait_for_butler_main_to_be_idle,
};
use crate::db::conversation_db::ConversationDatabase;
use crate::db::system_db::{SecureConfigEntry, SystemDatabase};

const EXPERIMENTAL_FEATURE_CODE: &str = "experimental";
const FEISHU_SCOPE: &str = "butler_feishu";
const FEISHU_SECRET_KEY: &str = "app_secret";
const SECURE_MASTER_KEY: &str = "secure_config_master_key";
const CHANNEL_FEISHU: &str = "feishu";
const BOTLER_SOURCE: &str = "feishu_butler";
const TERMINAL_TASK_STATUSES: [&str; 3] = ["succeeded", "failed", "cancelled"];

pub struct FeishuButlerState {
    runtime_task: StdMutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub ingress_lock: Mutex<()>,
    pub status: Mutex<FeishuRuntimeStatus>,
}

impl Default for FeishuButlerState {
    fn default() -> Self {
        Self {
            runtime_task: StdMutex::new(None),
            ingress_lock: Mutex::new(()),
            status: Mutex::new(FeishuRuntimeStatus::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeishuRuntimeStatus {
    pub butler_enabled: bool,
    pub enabled: bool,
    pub configured: bool,
    pub secret_configured: bool,
    pub running: bool,
    pub connected: bool,
    pub app_id: Option<String>,
    pub base_url: Option<String>,
    pub allow_p2p: bool,
    pub allow_group: bool,
    pub group_require_mention: bool,
    pub last_error: Option<String>,
    pub last_event_at: Option<String>,
    pub status_text: String,
}

#[derive(Debug, Clone)]
struct FeishuRuntimeConfig {
    butler_enabled: bool,
    enabled: bool,
    app_id: String,
    app_secret: String,
    base_url: String,
    allow_p2p: bool,
    allow_group: bool,
    group_require_mention: bool,
    allowed_open_ids: HashSet<String>,
    allowed_chat_ids: HashSet<String>,
}

#[derive(Debug, Deserialize)]
struct TenantAccessTokenResponse {
    code: i32,
    msg: String,
    tenant_access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageResponse {
    code: i32,
    msg: String,
    data: Option<SendMessageData>,
}

#[derive(Debug, Deserialize)]
struct SendMessageData {
    message_id: String,
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    header: EventHeader,
    event: EventBody,
}

#[derive(Debug, Deserialize)]
struct EventHeader {
    event_type: String,
}

#[derive(Debug, Deserialize)]
struct EventBody {
    sender: Sender,
    message: FeishuMessage,
}

#[derive(Debug, Deserialize)]
struct Sender {
    sender_id: SenderId,
}

#[derive(Debug, Deserialize)]
struct SenderId {
    open_id: String,
}

#[derive(Debug, Deserialize)]
struct FeishuMessage {
    message_id: String,
    message_type: String,
    content: String,
    chat_type: String,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    root_id: Option<String>,
    #[serde(default)]
    mentions: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct TextMessageContent {
    text: String,
}

#[derive(Debug, Clone)]
struct IncomingTextEvent {
    message_id: String,
    sender_open_id: String,
    chat_id: Option<String>,
    text: String,
    chat_type: String,
    parent_id: Option<String>,
    root_id: Option<String>,
    has_mentions: bool,
}

#[derive(Debug, Clone)]
struct ChannelLinkRecord<'a> {
    external_message_id: &'a str,
    external_chat_id: Option<&'a str>,
    external_user_id: Option<&'a str>,
    conversation_id: i64,
    local_message_id: Option<i64>,
    direction: &'a str,
    payload_type: &'a str,
}

fn parse_bool_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

fn now_string() -> String {
    Utc::now().to_rfc3339()
}

fn split_allowlist(raw: &str) -> HashSet<String> {
    raw.split(|ch| matches!(ch, '\n' | '\r' | ',' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn build_status(config: &FeishuRuntimeConfig) -> FeishuRuntimeStatus {
    FeishuRuntimeStatus {
        butler_enabled: config.butler_enabled,
        enabled: config.enabled,
        configured: !config.app_id.trim().is_empty() && !config.app_secret.trim().is_empty(),
        secret_configured: !config.app_secret.trim().is_empty(),
        running: false,
        connected: false,
        app_id: (!config.app_id.trim().is_empty()).then(|| config.app_id.clone()),
        base_url: Some(config.base_url.clone()),
        allow_p2p: config.allow_p2p,
        allow_group: config.allow_group,
        group_require_mention: config.group_require_mention,
        last_error: None,
        last_event_at: None,
        status_text: "飞书机器人未启动".to_string(),
    }
}

async fn replace_status(app_handle: &AppHandle, status: FeishuRuntimeStatus) {
    let state = app_handle.state::<FeishuButlerState>();
    *state.status.lock().await = status.clone();
    let _ = app_handle.emit("butler_feishu_status_changed", status);
}

async fn mutate_status<F>(app_handle: &AppHandle, apply: F)
where
    F: FnOnce(&mut FeishuRuntimeStatus),
{
    let state = app_handle.state::<FeishuButlerState>();
    let mut status = state.status.lock().await;
    apply(&mut status);
    let snapshot = status.clone();
    drop(status);
    let _ = app_handle.emit("butler_feishu_status_changed", snapshot);
}

fn get_or_create_master_key(app_handle: &AppHandle) -> Result<[u8; 32], String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let existing = db.get_config(SECURE_MASTER_KEY).map_err(|e| e.to_string())?;
    let key_b64 = if existing.trim().is_empty() {
        let bytes: [u8; 32] = rand::random();
        let encoded = BASE64.encode(bytes);
        match db.add_system_config(SECURE_MASTER_KEY, &encoded) {
            Ok(_) => encoded,
            Err(_) => {
                db.update_system_config(SECURE_MASTER_KEY, &encoded)
                    .map_err(|e| e.to_string())?;
                encoded
            }
        }
    } else {
        existing
    };
    let decoded = BASE64.decode(key_b64).map_err(|e| e.to_string())?;
    decoded
        .try_into()
        .map_err(|_| "Invalid secure config master key length".to_string())
}

fn encrypt_secret(app_handle: &AppHandle, plaintext: &str) -> Result<(String, String), String> {
    let key_bytes = get_or_create_master_key(app_handle)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Failed to encrypt secret: {e}"))?;
    Ok((BASE64.encode(encrypted), BASE64.encode(nonce_bytes)))
}

fn decrypt_secret(app_handle: &AppHandle, ciphertext: &str, nonce: &str) -> Result<String, String> {
    let key_bytes = get_or_create_master_key(app_handle)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes = BASE64.decode(nonce).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = BASE64.decode(ciphertext).map_err(|e| e.to_string())?;
    let decrypted = cipher
        .decrypt(nonce, encrypted.as_ref())
        .map_err(|e| format!("Failed to decrypt secret: {e}"))?;
    String::from_utf8(decrypted).map_err(|e| e.to_string())
}

pub(crate) fn save_feishu_secret(app_handle: &AppHandle, app_secret: &str) -> Result<(), String> {
    if app_secret.trim().is_empty() {
        return Err("飞书 App Secret 不能为空".to_string());
    }
    let (ciphertext, nonce) = encrypt_secret(app_handle, app_secret.trim())?;
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.upsert_secure_config(&SecureConfigEntry {
        scope: FEISHU_SCOPE.to_string(),
        key: FEISHU_SECRET_KEY.to_string(),
        ciphertext,
        nonce,
        updated_time: None,
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn clear_feishu_secret(app_handle: &AppHandle) -> Result<(), String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.delete_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY)
        .map_err(|e| e.to_string())
}

fn load_feishu_secret(app_handle: &AppHandle) -> Result<Option<String>, String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let Some(entry) = db
        .get_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY)
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(decrypt_secret(app_handle, &entry.ciphertext, &entry.nonce)?))
}

async fn load_runtime_config(app_handle: &AppHandle) -> Result<FeishuRuntimeConfig, String> {
    let feature_state = app_handle.state::<crate::FeatureConfigState>();
    let guard = feature_state.config_feature_map.lock().await;
    let experimental = guard.get(EXPERIMENTAL_FEATURE_CODE);
    let get = |key: &str| -> String {
        experimental
            .and_then(|map| map.get(key))
            .map(|config| config.value.clone())
            .unwrap_or_default()
    };
    let app_secret = load_feishu_secret(app_handle)?.unwrap_or_default();
    Ok(FeishuRuntimeConfig {
        butler_enabled: parse_bool_flag(&get("butler_experiment_enabled")),
        enabled: parse_bool_flag(&get("butler_feishu_enabled")),
        app_id: get("butler_feishu_app_id"),
        app_secret,
        base_url: {
            let value = get("butler_feishu_base_url");
            if value.trim().is_empty() {
                "https://open.feishu.cn".to_string()
            } else {
                value
            }
        },
        allow_p2p: !matches!(get("butler_feishu_receive_p2p").as_str(), "false" | "0"),
        allow_group: !matches!(get("butler_feishu_receive_group").as_str(), "false" | "0"),
        group_require_mention: !matches!(
            get("butler_feishu_group_require_mention").as_str(),
            "false" | "0"
        ),
        allowed_open_ids: split_allowlist(&get("butler_feishu_allowed_open_ids")),
        allowed_chat_ids: split_allowlist(&get("butler_feishu_allowed_chat_ids")),
    })
}

pub(crate) async fn get_runtime_status(
    app_handle: &AppHandle,
) -> Result<FeishuRuntimeStatus, String> {
    let config = load_runtime_config(app_handle).await?;
    let state = app_handle.state::<FeishuButlerState>();
    let mut status = state.status.lock().await.clone();
    status.butler_enabled = config.butler_enabled;
    status.enabled = config.enabled;
    status.configured = !config.app_id.trim().is_empty() && !config.app_secret.trim().is_empty();
    status.secret_configured = !config.app_secret.trim().is_empty();
    status.app_id = (!config.app_id.trim().is_empty()).then(|| config.app_id);
    status.base_url = Some(config.base_url);
    status.allow_p2p = config.allow_p2p;
    status.allow_group = config.allow_group;
    status.group_require_mention = config.group_require_mention;
    Ok(status)
}

pub(crate) fn refresh_runtime_async(app_handle: &AppHandle) {
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = refresh_runtime(&app_handle).await {
            warn!(error = %error, "failed to refresh feishu runtime");
        }
    });
}

pub(crate) async fn refresh_runtime(
    app_handle: &AppHandle,
) -> Result<FeishuRuntimeStatus, String> {
    crate::ensure_rustls_crypto_provider();
    let config = load_runtime_config(app_handle).await?;
    let state = app_handle.state::<FeishuButlerState>();

    if let Some(handle) = state.runtime_task.lock().unwrap().take() {
        handle.abort();
    }

    let mut status = build_status(&config);
    if !config.butler_enabled {
        status.status_text = "总管家模式未启用，飞书机器人不会启动".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }
    if !config.enabled {
        status.status_text = "飞书机器人未启用".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        status.status_text = "请先配置飞书 App ID 和 App Secret".to_string();
        replace_status(app_handle, status.clone()).await;
        return Ok(status);
    }

    status.running = true;
    status.status_text = "正在连接飞书长连接".to_string();
    replace_status(app_handle, status.clone()).await;

    let app_handle_clone = app_handle.clone();
    let config_clone = config.clone();
    let task = tauri::async_runtime::spawn(async move {
        run_runtime_loop(app_handle_clone, config_clone).await;
    });
    *state.runtime_task.lock().unwrap() = Some(task);
    Ok(status)
}

async fn run_runtime_loop(app_handle: AppHandle, config: FeishuRuntimeConfig) {
    loop {
        let ws_config = match openlark_client::Config::builder()
            .app_id(config.app_id.clone())
            .app_secret(config.app_secret.clone())
            .base_url(config.base_url.clone())
            .timeout(Duration::from_secs(30))
            .build()
        {
            Ok(config_value) => config_value,
            Err(error) => {
                mutate_status(&app_handle, |status| {
                    status.running = false;
                    status.connected = false;
                    status.last_error = Some(error.to_string());
                    status.status_text = "飞书配置无效".to_string();
                })
                .await;
                return;
            }
        };

        let (payload_tx, payload_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let event_handler = EventDispatcherHandler::builder()
            .payload_sender(payload_tx)
            .build();

        let processor_app = app_handle.clone();
        let processor_config = config.clone();
        let processor_task = tauri::async_runtime::spawn(async move {
            process_payload_loop(processor_app, processor_config, payload_rx).await;
        });

        mutate_status(&app_handle, |status| {
            status.running = true;
            status.connected = true;
            status.last_error = None;
            status.status_text = "飞书机器人已连接，等待消息".to_string();
        })
        .await;

        let result = LarkWsClient::open(Arc::new(ws_config), event_handler).await;
        processor_task.abort();

        match result {
            Ok(_) => {
                mutate_status(&app_handle, |status| {
                    status.connected = false;
                    status.status_text = "飞书长连接已断开，准备重连".to_string();
                })
                .await;
            }
            Err(error) => {
                mutate_status(&app_handle, |status| {
                    status.connected = false;
                    status.last_error = Some(error.to_string());
                    status.status_text = "飞书连接失败，准备重连".to_string();
                })
                .await;
            }
        }

        sleep(Duration::from_secs(5)).await;
    }
}

async fn process_payload_loop(
    app_handle: AppHandle,
    config: FeishuRuntimeConfig,
    mut payload_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(payload) = payload_rx.recv().await {
        mutate_status(&app_handle, |status| {
            status.last_event_at = Some(now_string());
        })
        .await;
        if let Err(error) = handle_payload(&app_handle, &config, &payload).await {
            warn!(error = %error, "failed to handle feishu payload");
        }
    }
}

async fn handle_payload(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    payload: &[u8],
) -> Result<(), String> {
    let envelope: EventEnvelope = match serde_json::from_slice(payload) {
        Ok(value) => value,
        Err(error) => {
            debug!(error = %error, "ignore unparsable feishu payload");
            return Ok(());
        }
    };
    if envelope.header.event_type != "im.message.receive_v1" {
        return Ok(());
    }

    let Some(event) = parse_incoming_text_event(config, envelope)? else {
        return Ok(());
    };

    if let Err(error) = process_incoming_text_message(app_handle, config, &event).await {
        warn!(message_id = %event.message_id, error = %error, "failed to process feishu message");
        let _ = reply_text_message(
            config,
            &event.message_id,
            &format!("总管家处理飞书消息失败：{}", truncate_text(&error, 180)),
        )
        .await;
        mutate_status(app_handle, |status| {
            status.last_error = Some(error);
            status.status_text = "处理飞书消息时发生错误".to_string();
        })
        .await;
    }

    Ok(())
}

fn parse_incoming_text_event(
    config: &FeishuRuntimeConfig,
    envelope: EventEnvelope,
) -> Result<Option<IncomingTextEvent>, String> {
    if envelope.event.message.message_type != "text" {
        return Ok(None);
    }

    let text_content: TextMessageContent =
        serde_json::from_str(&envelope.event.message.content).map_err(|e| e.to_string())?;
    let text = text_content.text.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }

    let sender_open_id = envelope.event.sender.sender_id.open_id.trim().to_string();
    if sender_open_id.is_empty() {
        return Ok(None);
    }

    let chat_type = envelope.event.message.chat_type.trim().to_string();
    if chat_type == "p2p" && !config.allow_p2p {
        return Ok(None);
    }
    if chat_type != "p2p" && !config.allow_group {
        return Ok(None);
    }

    let chat_id = envelope.event.message.chat_id.clone();
    if !config.allowed_open_ids.is_empty() && !config.allowed_open_ids.contains(&sender_open_id) {
        return Ok(None);
    }
    if chat_type != "p2p" {
        if let Some(value) = chat_id.as_ref() {
            if !config.allowed_chat_ids.is_empty() && !config.allowed_chat_ids.contains(value) {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
    }

    Ok(Some(IncomingTextEvent {
        message_id: envelope.event.message.message_id,
        sender_open_id,
        chat_id,
        text,
        chat_type,
        parent_id: envelope.event.message.parent_id,
        root_id: envelope.event.message.root_id,
        has_mentions: envelope
            .event
            .message
            .mentions
            .map(|mentions| !mentions.is_empty())
            .unwrap_or(false),
    }))
}

async fn process_incoming_text_message(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    event: &IncomingTextEvent,
) -> Result<(), String> {
    if external_message_exists(app_handle, CHANNEL_FEISHU, &event.message_id)? {
        return Ok(());
    }

    if event.chat_type != "p2p" && config.group_require_mention {
        let replied_to_bot = linked_to_outbound_message(
            app_handle,
            event.parent_id.as_deref().or(event.root_id.as_deref()),
        )?;
        if !event.has_mentions && !replied_to_bot {
            return Ok(());
        }
    }

    let state = app_handle.state::<FeishuButlerState>();
    let _ingress_guard = state.ingress_lock.lock().await;

    let butler_conversation = load_or_create_butler_main_internal(app_handle).await?;
    let assistant_id = butler_conversation
        .assistant_id
        .ok_or_else(|| "总管家主会话缺少 assistant".to_string())?;

    let before_message_max_id = get_latest_message_id(app_handle, butler_conversation.id)?;

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &event.message_id,
            external_chat_id: event.chat_id.as_deref(),
            external_user_id: Some(&event.sender_open_id),
            conversation_id: butler_conversation.id,
            local_message_id: None,
            direction: "inbound",
            payload_type: "text",
        },
    )?;

    let continuation_lock = get_butler_main_continuation_lock(butler_conversation.id).await;
    {
        let _guard = continuation_lock.lock().await;
        wait_for_butler_main_to_be_idle(app_handle, butler_conversation.id).await;
        let window = resolve_butler_execution_window(app_handle)?;
        let request = AiRequest {
            conversation_id: butler_conversation.id.to_string(),
            assistant_id,
            prompt: event.text.clone(),
            model: None,
            override_model_id: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: Some(true),
            attachment_list: None,
        };
        ask_ai(
            app_handle.clone(),
            app_handle.state::<crate::AppState>(),
            app_handle.state::<crate::AcpSessionState>(),
            app_handle.state::<crate::FeatureConfigState>(),
            app_handle.state::<crate::state::message_token::MessageTokenManager>(),
            app_handle.state::<crate::state::activity_state::ConversationActivityManager>(),
            window,
            request,
            None,
            None,
            None,
            Some(build_feishu_system_message(event)),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    wait_for_butler_to_settle(app_handle, butler_conversation.id).await?;

    if let Some(user_message_id) =
        find_latest_message_id_by_type(app_handle, butler_conversation.id, before_message_max_id, "user")?
    {
        update_external_link_local_message(
            app_handle,
            CHANNEL_FEISHU,
            &event.message_id,
            user_message_id,
        )?;
    }

    let assistant_message = find_latest_completed_assistant_message(
        app_handle,
        butler_conversation.id,
        before_message_max_id,
    )?
    .ok_or_else(|| "总管家未产生可回发的最终文本结果".to_string())?;
    let outbound_message_id = reply_text_message(config, &event.message_id, &assistant_message.content).await?;
    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &outbound_message_id,
            external_chat_id: event.chat_id.as_deref(),
            external_user_id: Some(&event.sender_open_id),
            conversation_id: butler_conversation.id,
            local_message_id: Some(assistant_message.id),
            direction: "outbound",
            payload_type: "text",
        },
    )?;
    mutate_status(app_handle, |status| {
        status.last_error = None;
        status.status_text = "飞书消息处理完成".to_string();
    })
    .await;
    Ok(())
}

fn build_feishu_system_message(event: &IncomingTextEvent) -> String {
    format!(
        "<external_channel_input>\nchannel=feishu\nsource={}\nmessage_id={}\nchat_type={}\nchat_id={}\nsender_open_id={}\nreply_parent_id={}\nreply_root_id={}\n</external_channel_input>",
        BOTLER_SOURCE,
        event.message_id,
        event.chat_type,
        event.chat_id.as_deref().unwrap_or(""),
        event.sender_open_id,
        event.parent_id.as_deref().unwrap_or(""),
        event.root_id.as_deref().unwrap_or(""),
    )
}

async fn wait_for_butler_to_settle(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
) -> Result<(), String> {
    let activity_manager = app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let mut idle_checks = 0;
    for _ in 0..1200 {
        let runtime_state = activity_manager.get_runtime_state(butler_conversation_id).await;
        let pending_tasks = count_pending_butler_tasks(app_handle, butler_conversation_id)?;
        if !runtime_state.is_running && pending_tasks == 0 {
            idle_checks += 1;
            if idle_checks >= 2 {
                return Ok(());
            }
        } else {
            idle_checks = 0;
        }
        sleep(Duration::from_millis(500)).await;
    }
    Err("等待总管家处理飞书消息超时".to_string())
}

fn count_pending_butler_tasks(app_handle: &AppHandle, butler_conversation_id: i64) -> Result<i64, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT COUNT(1)
         FROM conversation
         WHERE parent_butler_conversation_id = ?1
           AND conversation_kind = 'butler_task'
           AND (
             butler_task_finalized_at IS NULL
             OR COALESCE(butler_task_status, '') NOT IN (?2, ?3, ?4)
           )",
        params![
            butler_conversation_id,
            TERMINAL_TASK_STATUSES[0],
            TERMINAL_TASK_STATUSES[1],
            TERMINAL_TASK_STATUSES[2]
        ],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|e| e.to_string())
}

fn get_latest_message_id(app_handle: &AppHandle, conversation_id: i64) -> Result<i64, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    Ok(conn
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM message WHERE conversation_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?)
}

fn find_latest_message_id_by_type(
    app_handle: &AppHandle,
    conversation_id: i64,
    after_message_id: i64,
    message_type: &str,
) -> Result<Option<i64>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id
         FROM message
         WHERE conversation_id = ?1 AND id > ?2 AND message_type = ?3
         ORDER BY id DESC
         LIMIT 1",
        params![conversation_id, after_message_id, message_type],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn find_latest_completed_assistant_message(
    app_handle: &AppHandle,
    conversation_id: i64,
    after_message_id: i64,
) -> Result<Option<crate::db::conversation_db::Message>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let messages = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .list_by_conversation_id(conversation_id)
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let reply_messages: Vec<_> = messages
        .into_iter()
        .map(|(message, _)| message)
        .filter(|message| seen.insert(message.id))
        .filter(|message| {
            message.id > after_message_id
                && matches!(message.message_type.as_str(), "response" | "assistant")
                && !message.content.trim().is_empty()
        })
        .collect();

    Ok(reply_messages
        .iter()
        .filter(|message| message.finish_time.is_some())
        .max_by_key(|message| message.id)
        .cloned()
        .or_else(|| reply_messages.into_iter().max_by_key(|message| message.id)))
}

fn external_message_exists(
    app_handle: &AppHandle,
    channel: &str,
    external_message_id: &str,
) -> Result<bool, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let result = conn
        .query_row(
            "SELECT 1 FROM external_channel_message_link WHERE channel = ?1 AND external_message_id = ?2 LIMIT 1",
            params![channel, external_message_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(result.is_some())
}

fn linked_to_outbound_message(app_handle: &AppHandle, external_message_id: Option<&str>) -> Result<bool, String> {
    let Some(external_message_id) = external_message_id else {
        return Ok(false);
    };
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let result = conn
        .query_row(
            "SELECT 1
             FROM external_channel_message_link
             WHERE channel = ?1 AND external_message_id = ?2 AND direction = 'outbound'
             LIMIT 1",
            params![CHANNEL_FEISHU, external_message_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(result.is_some())
}

fn insert_external_link(app_handle: &AppHandle, record: ChannelLinkRecord<'_>) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO external_channel_message_link
            (channel, external_message_id, external_chat_id, external_user_id, conversation_id, local_message_id, direction, payload_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            CHANNEL_FEISHU,
            record.external_message_id,
            record.external_chat_id,
            record.external_user_id,
            record.conversation_id,
            record.local_message_id,
            record.direction,
            record.payload_type
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn update_external_link_local_message(
    app_handle: &AppHandle,
    channel: &str,
    external_message_id: &str,
    local_message_id: i64,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE external_channel_message_link
         SET local_message_id = ?1
         WHERE channel = ?2 AND external_message_id = ?3",
        params![local_message_id, channel, external_message_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn fetch_tenant_access_token(config: &FeishuRuntimeConfig) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        config.base_url.trim_end_matches('/')
    );
    let response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "app_id": config.app_id,
            "app_secret": config.app_secret
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: TenantAccessTokenResponse = response.json().await.map_err(|e| e.to_string())?;
    if body.code != 0 {
        return Err(format!("获取 tenant_access_token 失败: {}", body.msg));
    }
    body.tenant_access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "飞书未返回 tenant_access_token".to_string())
}

async fn reply_text_message(
    config: &FeishuRuntimeConfig,
    reply_to_message_id: &str,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(config).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "{}/open-apis/im/v1/messages/{}/reply",
        config.base_url.trim_end_matches('/'),
        reply_to_message_id
    );
    let response = client
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "msg_type": "text",
            "content": json!({ "text": text }).to_string()
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: SendMessageResponse = response.json().await.map_err(|e| e.to_string())?;
    if body.code != 0 {
        return Err(format!("回发飞书消息失败: {}", body.msg));
    }
    body.data
        .map(|data| data.message_id)
        .ok_or_else(|| "飞书回发成功但未返回 message_id".to_string())
}

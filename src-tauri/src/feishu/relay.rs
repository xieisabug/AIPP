use std::collections::HashSet;

use tauri::{AppHandle, Manager};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::db::connection::{params, OptionalExtension};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;

use super::api::{
    render_message_for_feishu_delivery, resolve_preview_file_tool_call_for_message,
    send_markdown_message_to_target,
};
use super::config::{load_feishu_secret, load_runtime_config, load_runtime_config_inner};
use super::events::count_pending_butler_tasks;
use super::runtime::{mutate_status, set_feishu_runtime_ready_status};
use super::types::*;

pub(super) async fn mark_relay_scope_failed_with_log(
    app_handle: &AppHandle,
    scope_id: i64,
    error_message: &str,
    context: &str,
) {
    if let Err(mark_error) = mark_relay_scope_failed(app_handle, scope_id, error_message) {
        error!(
            scope_id,
            relay_error = %error_message,
            mark_error = %mark_error,
            "{context}"
        );
    }
}

pub(super) async fn spawn_feishu_relay_scope_worker(
    app_handle: &AppHandle,
    scope_id: i64,
    conversation_id: i64,
) {
    let state = app_handle.state::<FeishuButlerState>();
    let mut relay_workers = state.relay_workers.lock().await;
    if !relay_workers.insert(scope_id) {
        return;
    }
    drop(relay_workers);

    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_feishu_relay_scope_worker(&app_handle, scope_id, conversation_id).await;
        let state = app_handle.state::<FeishuButlerState>();
        let mut relay_workers = state.relay_workers.lock().await;
        relay_workers.remove(&scope_id);
        drop(relay_workers);

        if let Err(error) = result {
            mark_relay_scope_failed_with_log(
                &app_handle,
                scope_id,
                &error,
                "failed to persist Feishu relay worker error",
            )
            .await;
            warn!(conversation_id, scope_id, error = %error, "feishu relay worker failed");
        }
    });
}

async fn run_feishu_relay_scope_worker(
    app_handle: &AppHandle,
    scope_id: i64,
    conversation_id: i64,
) -> Result<(), String> {
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let mut idle_checks = 0usize;

    // Load the Feishu secret once before the loop to avoid opening SystemDatabase
    // every 500ms. The secret rarely changes during a relay scope's lifetime.
    let cached_secret = load_feishu_secret(app_handle)?.unwrap_or_default();

    loop {
        let scope = load_relay_scope(app_handle, scope_id)?;
        if matches!(scope.status.as_str(), "completed" | "failed" | "superseded") {
            return Ok(());
        }

        let fresh_config = load_runtime_config_inner(app_handle, Some(&cached_secret)).await?;
        if !fresh_config.butler_enabled || !fresh_config.enabled {
            return Err("飞书回发已被停用，跳过当前回发任务".to_string());
        }

        flush_feishu_relay_scope(app_handle, &fresh_config, scope_id).await?;

        let scope = load_relay_scope(app_handle, scope_id)?;
        let runtime_state = activity_manager.get_runtime_state(conversation_id).await;
        let pending_tasks = count_pending_butler_tasks(app_handle, conversation_id)?;
        let latest_message_id = get_latest_message_id(app_handle, conversation_id)?;
        let no_new_messages = latest_message_id <= scope.last_delivered_local_message_id;

        if !runtime_state.is_running && pending_tasks == 0 && no_new_messages {
            idle_checks += 1;
            if idle_checks >= FEISHU_RELAY_IDLE_STABLE_CHECKS {
                mark_relay_scope_progress(
                    app_handle,
                    scope_id,
                    scope.last_delivered_local_message_id,
                    "completed",
                )?;
                if scope.origin == RELAY_ORIGIN_FEISHU {
                    set_feishu_runtime_ready_status(
                        app_handle,
                        format!("最近一条飞书消息处理链路已稳定结束；{FEISHU_STATUS_READY_DETAIL}"),
                    )
                    .await;
                }
                return Ok(());
            }
        } else {
            idle_checks = 0;
            let next_status = if runtime_state.is_running && pending_tasks == 0 && no_new_messages {
                "waiting_user_input"
            } else {
                "active"
            };
            mark_relay_scope_progress(
                app_handle,
                scope_id,
                scope.last_delivered_local_message_id,
                next_status,
            )?;
            if scope.origin == RELAY_ORIGIN_FEISHU {
                mutate_status(app_handle, |status| {
                    status.running = true;
                    status.connected = true;
                    status.last_error = None;
                    status.status_text = "总管家正在持续回发飞书消息".to_string();
                    status.status_detail = Some(format!(
                        "飞书消息已受理；会话运行中={}，待完成任务={}，已回发到消息 {}",
                        runtime_state.is_running,
                        pending_tasks,
                        scope.last_delivered_local_message_id
                    ));
                })
                .await;
            }
        }

        sleep(FEISHU_SETTLE_CHECK_INTERVAL).await;
    }
}

pub(crate) async fn maybe_schedule_butler_feishu_relay_for_aipp_turn(
    app_handle: &AppHandle,
    conversation_id: i64,
    start_after_local_message_id: i64,
    relay_origin: Option<&str>,
) -> Result<(), String> {
    match relay_origin.unwrap_or(RELAY_ORIGIN_AIPP) {
        RELAY_ORIGIN_FEISHU | RELAY_ORIGIN_INTERNAL => return Ok(()),
        _ => {}
    }

    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled || config.only_reply_feishu_originated {
        return Ok(());
    }

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation = db
        .conversation_repo()
        .map_err(|e| e.to_string())?
        .read(conversation_id)
        .map_err(|e| e.to_string())?;
    let Some(conversation) = conversation else {
        return Ok(());
    };
    if conversation.conversation_kind != "butler_main" {
        return Ok(());
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(());
    };

    let scope_id = create_relay_scope(
        app_handle,
        NewRelayScope {
            channel: CHANNEL_FEISHU,
            conversation_id,
            origin: RELAY_ORIGIN_AIPP,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            anchor_external_message_id: target.reply_to_message_id.as_deref().unwrap_or(""),
            start_after_local_message_id,
        },
    )?;
    spawn_feishu_relay_scope_worker(app_handle, scope_id, conversation_id).await;

    Ok(())
}

pub(super) fn get_latest_message_id(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<i64, String> {
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

pub(super) fn find_latest_message_id_by_type(
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

pub(super) fn create_relay_scope(
    app_handle: &AppHandle,
    new_scope: NewRelayScope<'_>,
) -> Result<i64, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.with_write_connection(|conn| {
        let superseded = conn
            .execute(
                "UPDATE external_channel_relay_scope
                 SET status = 'superseded', updated_time = CURRENT_TIMESTAMP
                 WHERE channel = ?1
                   AND conversation_id = ?2
                   AND COALESCE(status, '') NOT IN ('completed', 'failed', 'superseded')",
                params![new_scope.channel, new_scope.conversation_id],
            )
            .map_err(crate::errors::AppError::from)?;
        if superseded > 0 {
            info!(
                conversation_id = new_scope.conversation_id,
                superseded,
                new_origin = new_scope.origin,
                "superseded existing relay scopes before creating new scope"
            );
        }

        conn.execute(
            "INSERT INTO external_channel_relay_scope
                (channel, conversation_id, origin, external_chat_id, external_user_id,
                 anchor_external_message_id, start_after_local_message_id, last_delivered_local_message_id,
                 status, updated_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 'pending', CURRENT_TIMESTAMP)",
            params![
                new_scope.channel,
                new_scope.conversation_id,
                new_scope.origin,
                new_scope.external_chat_id,
                new_scope.external_user_id,
                new_scope.anchor_external_message_id,
                new_scope.start_after_local_message_id,
            ],
        )
        .map_err(crate::errors::AppError::from)?;
        Ok(conn.last_insert_rowid())
    })
    .map_err(|e| e.to_string())
}

pub(super) fn load_relay_scope(
    app_handle: &AppHandle,
    scope_id: i64,
) -> Result<RelayScopeRecord, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, channel, conversation_id, origin, external_chat_id, external_user_id,
                anchor_external_message_id, start_after_local_message_id,
                last_delivered_local_message_id, status
         FROM external_channel_relay_scope
         WHERE id = ?1",
        params![scope_id],
        |row| {
            Ok(RelayScopeRecord {
                id: row.get(0)?,
                channel: row.get(1)?,
                conversation_id: row.get(2)?,
                origin: row.get(3)?,
                external_chat_id: row.get(4)?,
                external_user_id: row.get(5)?,
                anchor_external_message_id: row.get(6)?,
                start_after_local_message_id: row.get(7)?,
                last_delivered_local_message_id: row.get(8)?,
                status: row.get(9)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

pub(super) fn find_active_relay_scope(
    app_handle: &AppHandle,
    conversation_id: i64,
    origin: &str,
) -> Result<Option<RelayScopeRecord>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT id, channel, conversation_id, origin, external_chat_id, external_user_id,
                anchor_external_message_id, start_after_local_message_id,
                last_delivered_local_message_id, status
         FROM external_channel_relay_scope
         WHERE channel = ?1
           AND conversation_id = ?2
           AND origin = ?3
           AND COALESCE(status, '') NOT IN ('completed', 'failed', 'superseded')
         ORDER BY id DESC
         LIMIT 1",
        params![CHANNEL_FEISHU, conversation_id, origin],
        |row| {
            Ok(RelayScopeRecord {
                id: row.get(0)?,
                channel: row.get(1)?,
                conversation_id: row.get(2)?,
                origin: row.get(3)?,
                external_chat_id: row.get(4)?,
                external_user_id: row.get(5)?,
                anchor_external_message_id: row.get(6)?,
                start_after_local_message_id: row.get(7)?,
                last_delivered_local_message_id: row.get(8)?,
                status: row.get(9)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub(super) fn mark_relay_scope_progress(
    app_handle: &AppHandle,
    scope_id: i64,
    last_delivered_local_message_id: i64,
    status: &str,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.with_write_connection(|conn| {
        conn.execute(
            "UPDATE external_channel_relay_scope
             SET last_delivered_local_message_id = ?2,
                 status = ?3,
                 last_error = NULL,
                 updated_time = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![scope_id, last_delivered_local_message_id, status],
        )
        .map_err(crate::errors::AppError::from)?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub(super) fn mark_relay_scope_failed(
    app_handle: &AppHandle,
    scope_id: i64,
    error: &str,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.with_write_connection(|conn| {
        conn.execute(
            "UPDATE external_channel_relay_scope
             SET status = 'failed',
                 last_error = ?2,
                 updated_time = CURRENT_TIMESTAMP
             WHERE id = ?1",
            params![scope_id, truncate_text(error, 500)],
        )
        .map_err(crate::errors::AppError::from)?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Cross-scope deduplication: check if a local message was already successfully
/// delivered to the external channel by ANY scope (not just the current one).
fn is_message_already_delivered(
    app_handle: &AppHandle,
    conversation_id: i64,
    local_message_id: i64,
) -> Result<bool, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM external_channel_message_delivery
             WHERE conversation_id = ?1
               AND local_message_id = ?2
               AND status = 'sent'
             LIMIT 1",
            params![conversation_id, local_message_id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    Ok(exists)
}

fn record_scope_delivery(
    app_handle: &AppHandle,
    scope_id: i64,
    channel: &str,
    conversation_id: i64,
    local_message_id: i64,
    external_message_id: Option<&str>,
    status: &str,
    rendered_text: &str,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.with_write_connection(|conn| {
        conn.execute(
            "INSERT INTO external_channel_message_delivery
                (scope_id, channel, conversation_id, local_message_id, external_message_id, status, rendered_text, updated_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CURRENT_TIMESTAMP)
             ON CONFLICT(scope_id, local_message_id) DO UPDATE SET
                external_message_id = excluded.external_message_id,
                status = excluded.status,
                rendered_text = excluded.rendered_text,
                updated_time = CURRENT_TIMESTAMP",
            params![
                scope_id,
                channel,
                conversation_id,
                local_message_id,
                external_message_id,
                status,
                rendered_text,
            ],
        )
        .map_err(crate::errors::AppError::from)?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub(super) fn find_latest_feishu_target(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<Option<ChannelLinkTarget>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let linked_target = conn
        .query_row(
            "SELECT external_message_id, external_chat_id, external_user_id
             FROM external_channel_message_link
             WHERE channel = ?1 AND conversation_id = ?2
             ORDER BY created_time DESC, id DESC
             LIMIT 1",
            params![CHANNEL_FEISHU, conversation_id],
            |row| {
                Ok(ChannelLinkTarget {
                    reply_to_message_id: normalize_optional_id(Some(row.get(0)?)),
                    external_chat_id: row.get(1)?,
                    external_user_id: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if linked_target.is_some() {
        return Ok(linked_target);
    }

    conn.query_row(
        "SELECT anchor_external_message_id, external_chat_id, external_user_id
         FROM external_channel_relay_scope
         WHERE channel = ?1 AND conversation_id = ?2
         ORDER BY updated_time DESC, created_time DESC, id DESC
         LIMIT 1",
        params![CHANNEL_FEISHU, conversation_id],
        |row| {
            Ok(ChannelLinkTarget {
                reply_to_message_id: normalize_optional_id(Some(row.get(0)?)),
                external_chat_id: row.get(1)?,
                external_user_id: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub(crate) fn conversation_has_feishu_target(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<bool, String> {
    Ok(find_latest_feishu_target(app_handle, conversation_id)?.is_some())
}

pub(crate) fn inherit_latest_feishu_target(
    app_handle: &AppHandle,
    source_conversation_id: i64,
    target_conversation_id: i64,
) -> Result<(), String> {
    if source_conversation_id == target_conversation_id {
        return Ok(());
    }
    if find_latest_feishu_target(app_handle, target_conversation_id)?.is_some() {
        return Ok(());
    }
    let Some(target) = find_latest_feishu_target(app_handle, source_conversation_id)? else {
        return Ok(());
    };

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let anchor_external_message_id = target.reply_to_message_id.unwrap_or_default();
    db.with_write_connection(|conn| {
        conn.execute(
            "INSERT INTO external_channel_relay_scope
                (channel, conversation_id, origin, external_chat_id, external_user_id,
                 anchor_external_message_id, start_after_local_message_id, last_delivered_local_message_id,
                 status, updated_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 'completed', CURRENT_TIMESTAMP)",
            params![
                CHANNEL_FEISHU,
                target_conversation_id,
                RELAY_ORIGIN_AIPP,
                target.external_chat_id.as_deref(),
                target.external_user_id.as_deref(),
                anchor_external_message_id,
            ],
        )
        .map_err(crate::errors::AppError::from)?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

fn find_scope_reply_anchor(
    app_handle: &AppHandle,
    scope_id: i64,
) -> Result<Option<String>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    let reply_to = conn
        .query_row(
            "SELECT external_message_id
         FROM external_channel_message_delivery
         WHERE scope_id = ?1
           AND status = 'sent'
           AND external_message_id IS NOT NULL
         ORDER BY local_message_id DESC, id DESC
         LIMIT 1",
            params![scope_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(normalize_optional_id(reply_to))
}

pub(super) fn list_relayable_messages(
    app_handle: &AppHandle,
    conversation_id: i64,
    after_message_id: i64,
) -> Result<Vec<crate::db::conversation_db::Message>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let messages = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .list_by_conversation_id(conversation_id)
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    Ok(messages
        .into_iter()
        .map(|(message, _)| message)
        .filter(|message| seen.insert(message.id))
        .filter(|message| {
            message.id > after_message_id
                && matches!(
                    message.message_type.as_str(),
                    "user" | "response" | "assistant" | "tool_result"
                )
                && is_message_ready_for_feishu_relay(message)
        })
        .collect())
}

pub(super) fn is_message_ready_for_feishu_relay(
    message: &crate::db::conversation_db::Message,
) -> bool {
    match message.message_type.as_str() {
        "response" | "assistant" => message.finish_time.is_some(),
        _ => true,
    }
}

pub(super) async fn flush_feishu_relay_scope(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    scope_id: i64,
) -> Result<(), String> {
    let scope = load_relay_scope(app_handle, scope_id)?;
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    if matches!(scope.status.as_str(), "completed" | "superseded") {
        return Ok(());
    }

    let mut current_reply_to = find_scope_reply_anchor(app_handle, scope.id)?
        .or_else(|| normalize_optional_id(Some(scope.anchor_external_message_id.clone())));
    let start_after = scope.last_delivered_local_message_id.max(scope.start_after_local_message_id);
    let messages = list_relayable_messages(app_handle, scope.conversation_id, start_after)?;
    let mut delivered_count = 0usize;
    let mut last_processed_message_id = scope.last_delivered_local_message_id;

    for message in messages {
        // Cross-scope dedup: skip if another scope already delivered this message
        if is_message_already_delivered(app_handle, scope.conversation_id, message.id)? {
            last_processed_message_id = message.id;
            record_scope_delivery(
                app_handle,
                scope.id,
                &scope.channel,
                scope.conversation_id,
                message.id,
                None,
                "skipped",
                "already delivered by another scope",
            )?;
            continue;
        }

        let preview_tool_call =
            resolve_preview_file_tool_call_for_message(&mcp_db, &message, &message);
        let rendered_parts = render_message_for_feishu_delivery(
            app_handle,
            &message,
            preview_tool_call,
            &scope.origin,
        )
        .await?;
        last_processed_message_id = message.id;

        let non_empty_parts: Vec<&str> =
            rendered_parts.iter().map(String::as_str).filter(|s| !s.trim().is_empty()).collect();

        if non_empty_parts.is_empty() {
            record_scope_delivery(
                app_handle,
                scope.id,
                &scope.channel,
                scope.conversation_id,
                message.id,
                None,
                "skipped",
                "",
            )?;
        } else {
            for rendered_text in &non_empty_parts {
                let outbound = send_markdown_message_to_target(
                    app_handle,
                    config,
                    &ChannelLinkTarget {
                        reply_to_message_id: current_reply_to.clone(),
                        external_chat_id: scope.external_chat_id.clone(),
                        external_user_id: scope.external_user_id.clone(),
                    },
                    rendered_text,
                )
                .await?;
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &outbound.external_message_id,
                        external_chat_id: scope.external_chat_id.as_deref(),
                        external_user_id: scope.external_user_id.as_deref(),
                        conversation_id: scope.conversation_id,
                        local_message_id: Some(message.id),
                        direction: "outbound",
                        payload_type: &outbound.payload_type,
                    },
                )?;
                record_scope_delivery(
                    app_handle,
                    scope.id,
                    &scope.channel,
                    scope.conversation_id,
                    message.id,
                    Some(&outbound.external_message_id),
                    "sent",
                    rendered_text,
                )?;
                current_reply_to = Some(outbound.external_message_id.clone());
                delivered_count += 1;
            }
        }

        mark_relay_scope_progress(app_handle, scope.id, message.id, "sending")?;
    }

    mark_relay_scope_progress(
        app_handle,
        scope.id,
        last_processed_message_id.max(scope.last_delivered_local_message_id),
        if delivered_count > 0 { "active" } else { &scope.status },
    )?;
    Ok(())
}

pub(super) fn external_message_exists(
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

pub(super) fn linked_to_outbound_message(
    app_handle: &AppHandle,
    external_message_id: Option<&str>,
) -> Result<bool, String> {
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

pub(super) fn insert_external_link(
    app_handle: &AppHandle,
    record: ChannelLinkRecord<'_>,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.with_write_connection(|conn| {
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
        .map_err(crate::errors::AppError::from)?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub(super) fn update_external_link_local_message(
    app_handle: &AppHandle,
    channel: &str,
    external_message_id: &str,
    local_message_id: i64,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.with_write_connection(|conn| {
        conn.execute(
            "UPDATE external_channel_message_link
             SET local_message_id = ?1
             WHERE channel = ?2 AND external_message_id = ?3",
            params![local_message_id, channel, external_message_id],
        )
        .map_err(crate::errors::AppError::from)?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

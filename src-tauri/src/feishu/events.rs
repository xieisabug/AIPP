use std::collections::HashMap;

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::api::ai::acp::AcpPermissionState;
use crate::api::ai::types::AiRequest;
use crate::api::ai_api::ask_ai;
use crate::api::butler_api::{
    get_butler_main_continuation_lock, load_or_create_butler_main_internal,
    reset_butler_main_conversation, resolve_butler_execution_window,
    wait_for_butler_main_to_be_idle,
};
use crate::api::operation_api::{confirm_acp_permission, confirm_operation_permission};
use crate::db::connection::params;
use crate::db::conversation_db::ConversationDatabase;
use crate::mcp::builtin_mcp::interaction::resolve_ask_user_question_response;

use super::types::*;
use super::runtime::{mutate_status, set_feishu_runtime_ready_status};
use super::relay::{
    create_relay_scope, spawn_feishu_relay_scope_worker, insert_external_link,
    external_message_exists, linked_to_outbound_message, update_external_link_local_message,
    get_latest_message_id, find_latest_message_id_by_type,
};
use super::api::{reply_text_message, send_text_message_to_open_id};
use super::interaction::{
    is_missing_ask_user_request_error, build_ask_user_question_tool_result,
    try_recover_feishu_ask_user_resolution, build_ask_user_answers_from_card_callback,
    recover_answers_from_callback_payload,
};

// ── Payload dispatch ───────────────────────────────────────────────

pub(super) async fn handle_payload(
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
    if envelope.header.event_type == "application.bot.menu_v6" {
        info!(
            event_type = %envelope.header.event_type,
            event_id = %envelope.header.event_id.as_deref().unwrap_or(""),
            "received Feishu bot menu event"
        );
    }
    match envelope.header.event_type.as_str() {
        "im.message.receive_v1" => {
            let Some(event) = parse_incoming_text_event(config, envelope)? else {
                return Ok(());
            };

            if let Err(error) = process_incoming_text_message(app_handle, config, &event).await {
                warn!(message_id = %event.message_id, error = %error, "failed to process feishu message");
                if let Err(reply_error) = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("总管家处理飞书消息失败：{}", truncate_text(&error, 180)),
                )
                .await
                {
                    warn!(
                        message_id = %event.message_id,
                        error = %reply_error,
                        "failed to send Feishu error reply"
                    );
                }
                mutate_status(app_handle, |status| {
                    status.last_error = Some(error);
                    status.status_text = "处理飞书消息时发生错误".to_string();
                })
                .await;
            }
        }
        "card.action.trigger" => {
            handle_card_action_trigger(app_handle, &envelope.event).await?;
        }
        "application.bot.menu_v6" => {
            handle_bot_menu_event(app_handle, config, &envelope.header, &envelope.event).await?;
        }
        _ => {}
    }

    Ok(())
}

// ── Bot menu events ────────────────────────────────────────────────

pub(super) fn parse_bot_menu_click_event(
    raw_event: &Value,
) -> Result<Option<FeishuBotMenuClickEvent>, String> {
    let event: FeishuBotMenuEvent =
        serde_json::from_value(raw_event.clone()).map_err(|e| e.to_string())?;
    let operator_open_id = event.operator.operator_id.open_id.trim().to_string();
    let event_key = event.event_key.trim().to_string();
    if operator_open_id.is_empty() || event_key.is_empty() {
        return Ok(None);
    }
    Ok(Some(FeishuBotMenuClickEvent { operator_open_id, event_key }))
}

async fn handle_bot_menu_event(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    header: &EventHeader,
    raw_event: &Value,
) -> Result<(), String> {
    let Some(event) = parse_bot_menu_click_event(raw_event)? else {
        warn!(
            event_id = %header.event_id.as_deref().unwrap_or(""),
            raw_event = %truncate_text(&raw_event.to_string(), 400),
            "ignored Feishu bot menu event because event_key/open_id was missing"
        );
        return Ok(());
    };
    if !config.allowed_open_ids.is_empty()
        && !config.allowed_open_ids.contains(&event.operator_open_id)
    {
        warn!(
            event_id = %header.event_id.as_deref().unwrap_or(""),
            operator_open_id = %event.operator_open_id,
            event_key = %event.event_key,
            "ignored Feishu bot menu event because operator is not in allowlist"
        );
        return Ok(());
    }
    if event.event_key != FEISHU_MENU_NEW_CONVERSATION_EVENT_KEY {
        info!(
            event_id = %header.event_id.as_deref().unwrap_or(""),
            operator_open_id = %event.operator_open_id,
            event_key = %event.event_key,
            "ignored unsupported Feishu bot menu event"
        );
        return Ok(());
    }
    info!(
        event_id = %header.event_id.as_deref().unwrap_or(""),
        operator_open_id = %event.operator_open_id,
        event_key = %event.event_key,
        "processing Feishu bot menu event"
    );

    let event_id = header.event_id.as_deref().map(str::trim).filter(|value| !value.is_empty());
    if let Some(event_id) = event_id {
        if external_message_exists(app_handle, CHANNEL_FEISHU, event_id)? {
            info!(event_id, "ignored duplicated Feishu bot menu event");
            return Ok(());
        }
    }

    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "正在处理飞书菜单事件".to_string();
        status.status_detail = Some("收到“新建会话”菜单点击，正在重置总管家上下文".to_string());
    })
    .await;

    let reset_response = match reset_butler_main_conversation(app_handle.clone()).await {
        Ok(response) => response,
        Err(error) => {
            warn!(
                event_id = %header.event_id.as_deref().unwrap_or(""),
                operator_open_id = %event.operator_open_id,
                event_key = %event.event_key,
                error = %error,
                "failed to reset Butler context from Feishu bot menu event"
            );
            mutate_status(app_handle, |status| {
                status.running = true;
                status.connected = true;
                status.last_error = Some(error.clone());
                status.status_text = "处理飞书菜单事件失败".to_string();
                status.status_detail = Some("总管家主会话重置失败".to_string());
            })
            .await;
            let _ = send_text_message_to_open_id(
                app_handle,
                config,
                &event.operator_open_id,
                &format!("清空上下文失败：{}", truncate_text(&error, 180)),
            )
            .await;
            return Err(error);
        }
    };
    info!(
        event_id = %header.event_id.as_deref().unwrap_or(""),
        operator_open_id = %event.operator_open_id,
        conversation_id = reset_response.conversation.id,
        "reset Butler context from Feishu bot menu event"
    );

    if let Some(event_id) = event_id {
        insert_external_link(
            app_handle,
            ChannelLinkRecord {
                external_message_id: event_id,
                external_chat_id: None,
                external_user_id: Some(&event.operator_open_id),
                conversation_id: reset_response.conversation.id,
                local_message_id: None,
                direction: "inbound",
                payload_type: "menu",
            },
        )?;
    }

    let confirmation_message_id = send_text_message_to_open_id(
        app_handle,
        config,
        &event.operator_open_id,
        "已经清空上下文，并创建了新的总管家会话。",
    )
    .await?;
    info!(
        event_id = %header.event_id.as_deref().unwrap_or(""),
        operator_open_id = %event.operator_open_id,
        confirmation_message_id = %confirmation_message_id,
        "sent Feishu bot menu confirmation message"
    );
    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &confirmation_message_id,
            external_chat_id: None,
            external_user_id: Some(&event.operator_open_id),
            conversation_id: reset_response.conversation.id,
            local_message_id: None,
            direction: "outbound",
            payload_type: "text",
        },
    )?;

    set_feishu_runtime_ready_status(app_handle, "已处理飞书“新建会话”菜单事件，总管家上下文已重置")
        .await;
    Ok(())
}

// ── Card action trigger ────────────────────────────────────────────

async fn handle_card_action_trigger(
    app_handle: &AppHandle,
    raw_event: &Value,
) -> Result<(), String> {
    let callback: FeishuCardActionCallback =
        serde_json::from_value(raw_event.clone()).map_err(|e| e.to_string())?;
    let event = callback.event();
    let action_value = callback
        .event()
        .action
        .value
        .as_ref()
        .ok_or_else(|| "飞书卡片回调缺少 action.value".to_string())?;

    if let Some(request_kind) = action_value.get("request_kind").and_then(Value::as_str) {
        match request_kind {
            "operation_permission" => {
                let request_id = action_value
                    .get("request_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "飞书操作权限卡片缺少 request_id".to_string())?;
                let decision = action_value
                    .get("decision")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "飞书操作权限卡片缺少 decision".to_string())?;
                let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
                let request = state
                    .get_permission_request(request_id)
                    .await
                    .ok_or_else(|| "操作权限请求不存在或已处理".to_string())?;
                if let Some(expected_open_id) = request.allowed_open_id.as_deref() {
                    if event.operator.open_id != expected_open_id {
                        return Err("当前飞书用户无权处理该操作权限请求".to_string());
                    }
                }
                confirm_operation_permission(
                    app_handle.clone(),
                    request_id.to_string(),
                    decision.to_string(),
                )
                .await?;
                return Ok(());
            }
            "acp_permission" => {
                let request_id = action_value
                    .get("request_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "飞书 ACP 权限卡片缺少 request_id".to_string())?;
                let state = app_handle.state::<AcpPermissionState>();
                let request = state
                    .get_request(request_id)
                    .await
                    .ok_or_else(|| "ACP 权限请求不存在或已处理".to_string())?;
                if let Some(expected_open_id) = request.allowed_open_id.as_deref() {
                    if event.operator.open_id != expected_open_id {
                        return Err("当前飞书用户无权处理该 ACP 权限请求".to_string());
                    }
                }
                let cancelled =
                    action_value.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
                let option_id =
                    action_value.get("option_id").and_then(Value::as_str).map(ToString::to_string);
                confirm_acp_permission(
                    app_handle.clone(),
                    request_id.to_string(),
                    option_id,
                    Some(cancelled),
                )
                .await?;
                return Ok(());
            }
            _ => {}
        }
    }

    let request_id = action_value
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "飞书卡片回调缺少 request_id".to_string())?;
    let action = action_value.get("action").and_then(Value::as_str).unwrap_or("submit");

    if action == "cancel" {
        match resolve_ask_user_question_response(app_handle, request_id, None, true).await {
            Ok(_) => {}
            Err(error) if is_missing_ask_user_request_error(&error) => {
                if !try_recover_feishu_ask_user_resolution(
                    app_handle,
                    &callback,
                    Err("User cancelled AskUserQuestion".to_string()),
                )
                .await?
                {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
        return Ok(());
    }

    let answers =
        match build_ask_user_answers_from_card_callback(app_handle, request_id, &callback).await {
            Ok(answers) => answers,
            Err(error) if is_missing_ask_user_request_error(&error) => {
                let form_value = callback
                    .event()
                    .action
                    .form_value
                    .as_ref()
                    .ok_or_else(|| "飞书卡片回调缺少 form_value".to_string())?;
                if try_recover_feishu_ask_user_resolution(
                    app_handle,
                    &callback,
                    Ok(build_ask_user_question_tool_result(
                        &recover_answers_from_callback_payload(app_handle, &callback, form_value)
                            .await?,
                    )),
                )
                .await?
                {
                    return Ok(());
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
    debug!(
        request_id,
        operator_open_id = %event.operator.open_id,
        "resolved AskUserQuestion from Feishu card callback"
    );
    match resolve_ask_user_question_response(app_handle, request_id, Some(answers), false).await {
        Ok(_) => {}
        Err(error) if is_missing_ask_user_request_error(&error) => {
            let form_value = callback
                .event()
                .action
                .form_value
                .as_ref()
                .ok_or_else(|| "飞书卡片回调缺少 form_value".to_string())?;
            if !try_recover_feishu_ask_user_resolution(
                app_handle,
                &callback,
                Ok(build_ask_user_question_tool_result(
                    &recover_answers_from_callback_payload(app_handle, &callback, form_value)
                        .await?,
                )),
            )
            .await?
            {
                return Err(error);
            }
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

// ── Incoming text messages ─────────────────────────────────────────

pub(super) fn parse_incoming_text_event(
    config: &FeishuRuntimeConfig,
    envelope: EventEnvelope,
) -> Result<Option<IncomingTextEvent>, String> {
    let event: EventBody = serde_json::from_value(envelope.event).map_err(|e| e.to_string())?;
    if event.message.message_type != "text" {
        return Ok(None);
    }

    let text_content: TextMessageContent =
        serde_json::from_str(&event.message.content).map_err(|e| e.to_string())?;
    let text = text_content.text.trim().to_string();
    if text.is_empty() {
        return Ok(None);
    }

    let sender_open_id = event.sender.sender_id.open_id.trim().to_string();
    if sender_open_id.is_empty() {
        return Ok(None);
    }

    let chat_type = event.message.chat_type.trim().to_string();
    if chat_type == "p2p" && !config.allow_p2p {
        return Ok(None);
    }
    if chat_type != "p2p" && !config.allow_group {
        return Ok(None);
    }

    let chat_id = event.message.chat_id.clone();
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
        message_id: event.message.message_id,
        sender_open_id,
        chat_id,
        text,
        chat_type,
        parent_id: event.message.parent_id,
        root_id: event.message.root_id,
        has_mentions: event.message.mentions.map(|mentions| !mentions.is_empty()).unwrap_or(false),
    }))
}

// ── Permission reply handling ──────────────────────────────────────

async fn try_handle_pending_permission_reply(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    event: &IncomingTextEvent,
) -> Result<bool, String> {
    let Some(command) = parse_permission_reply_command(&event.text) else {
        return Ok(false);
    };

    match command {
        PermissionReplyCommand::Operation { review_code, decision } => {
            let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
            let Some(request) = state.find_permission_request_by_review_code(&review_code).await
            else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("未找到待处理的操作权限审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            };
            if !feishu_reply_matches_permission_context(
                event,
                request.allowed_open_id.as_deref(),
                request.allowed_chat_id.as_deref(),
                request.feishu_message_id.as_deref(),
            ) {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("你无权处理审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            }
            confirm_operation_permission(
                app_handle.clone(),
                request.event.request_id.clone(),
                decision.to_string(),
            )
            .await?;
            if let Some(conversation_id) = request.conversation_id {
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &event.message_id,
                        external_chat_id: event.chat_id.as_deref(),
                        external_user_id: Some(&event.sender_open_id),
                        conversation_id,
                        local_message_id: None,
                        direction: "inbound",
                        payload_type: "text",
                    },
                )?;
            }
            let confirmation_text = match decision {
                "allow_for_conversation" => format!("已按“本任务信任”处理审批单 {}", review_code),
                "allow_for_assistant" => format!("已按“助手工作区信任”处理审批单 {}", review_code),
                "deny" => format!("已拒绝审批单 {}", review_code),
                _ => format!("已允许一次审批单 {}", review_code),
            };
            let _ =
                reply_text_message(app_handle, config, &event.message_id, &confirmation_text).await;
            Ok(true)
        }
        PermissionReplyCommand::AcpCancel { review_code } => {
            let state = app_handle.state::<AcpPermissionState>();
            let Some(request) = state.find_request_by_review_code(&review_code).await else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("未找到待处理的 ACP 审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            };
            if !feishu_reply_matches_permission_context(
                event,
                request.allowed_open_id.as_deref(),
                request.allowed_chat_id.as_deref(),
                request.feishu_message_id.as_deref(),
            ) {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("你无权处理审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            }
            confirm_acp_permission(
                app_handle.clone(),
                request.event.request_id.clone(),
                None,
                Some(true),
            )
            .await?;
            if let Some(conversation_id) = request.conversation_id {
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &event.message_id,
                        external_chat_id: event.chat_id.as_deref(),
                        external_user_id: Some(&event.sender_open_id),
                        conversation_id,
                        local_message_id: None,
                        direction: "inbound",
                        payload_type: "text",
                    },
                )?;
            }
            let _ = reply_text_message(
                app_handle,
                config,
                &event.message_id,
                &format!("已取消审批单 {}", review_code),
            )
            .await;
            Ok(true)
        }
        PermissionReplyCommand::AcpSelect { review_code, option_index } => {
            let state = app_handle.state::<AcpPermissionState>();
            let Some(request) = state.find_request_by_review_code(&review_code).await else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("未找到待处理的 ACP 审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            };
            if !feishu_reply_matches_permission_context(
                event,
                request.allowed_open_id.as_deref(),
                request.allowed_chat_id.as_deref(),
                request.feishu_message_id.as_deref(),
            ) {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("你无权处理审批单 {}", review_code),
                )
                .await;
                return Ok(true);
            }
            let Some(option) = request.event.options.get(option_index.saturating_sub(1)) else {
                let _ = reply_text_message(
                    app_handle,
                    config,
                    &event.message_id,
                    &format!("审批单 {} 没有第 {} 个选项", review_code, option_index),
                )
                .await;
                return Ok(true);
            };
            confirm_acp_permission(
                app_handle.clone(),
                request.event.request_id.clone(),
                Some(option.option_id.clone()),
                Some(false),
            )
            .await?;
            if let Some(conversation_id) = request.conversation_id {
                insert_external_link(
                    app_handle,
                    ChannelLinkRecord {
                        external_message_id: &event.message_id,
                        external_chat_id: event.chat_id.as_deref(),
                        external_user_id: Some(&event.sender_open_id),
                        conversation_id,
                        local_message_id: None,
                        direction: "inbound",
                        payload_type: "text",
                    },
                )?;
            }
            let _ = reply_text_message(
                app_handle,
                config,
                &event.message_id,
                &format!("已按“{}”处理审批单 {}", option.name, review_code),
            )
            .await;
            Ok(true)
        }
    }
}

// ── Process incoming text ──────────────────────────────────────────

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

    if try_handle_pending_permission_reply(app_handle, config, event).await? {
        return Ok(());
    }

    let state = app_handle.state::<FeishuButlerState>();
    let _ingress_guard = state.ingress_lock.lock().await;

    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "总管家正在处理飞书消息".to_string();
        status.status_detail =
            Some(format!("收到飞书消息，正在总管家主会话中处理（chat_type={}）", event.chat_type));
    })
    .await;

    let butler_conversation = load_or_create_butler_main_internal(app_handle).await?;
    let assistant_id =
        butler_conversation.assistant_id.ok_or_else(|| "总管家主会话缺少 assistant".to_string())?;

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
    let relay_scope_id = create_relay_scope(
        app_handle,
        NewRelayScope {
            channel: CHANNEL_FEISHU,
            conversation_id: butler_conversation.id,
            origin: RELAY_ORIGIN_FEISHU,
            external_chat_id: event.chat_id.as_deref(),
            external_user_id: Some(&event.sender_open_id),
            anchor_external_message_id: &event.message_id,
            start_after_local_message_id: before_message_max_id,
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
            Some(RELAY_ORIGIN_FEISHU.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    if let Some(user_message_id) = find_latest_message_id_by_type(
        app_handle,
        butler_conversation.id,
        before_message_max_id,
        "user",
    )? {
        update_external_link_local_message(
            app_handle,
            CHANNEL_FEISHU,
            &event.message_id,
            user_message_id,
        )?;
    }

    spawn_feishu_relay_scope_worker(app_handle, relay_scope_id, butler_conversation.id).await;
    mutate_status(app_handle, |status| {
        status.running = true;
        status.connected = true;
        status.last_error = None;
        status.status_text = "飞书消息已受理，正在持续回发".to_string();
        status.status_detail =
            Some("总管家会继续处理本轮消息，并把后续输出持续回发到飞书".to_string());
    })
    .await;
    Ok(())
}

// ── System message / settle / pending tasks ────────────────────────

pub(super) fn build_feishu_system_message(event: &IncomingTextEvent) -> String {
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

pub(super) async fn wait_for_butler_to_settle(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
) -> Result<(), String> {
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let mut idle_checks = 0;
    let max_checks =
        (FEISHU_SETTLE_TIMEOUT.as_millis() / FEISHU_SETTLE_CHECK_INTERVAL.as_millis()) as usize;
    for attempt in 0..max_checks {
        let runtime_state = activity_manager.get_runtime_state(butler_conversation_id).await;
        let pending_tasks = count_pending_butler_tasks(app_handle, butler_conversation_id)?;
        if !runtime_state.is_running && pending_tasks == 0 {
            idle_checks += 1;
            if idle_checks >= 2 {
                return Ok(());
            }
        } else {
            idle_checks = 0;
            if attempt % FEISHU_SETTLE_STATUS_INTERVAL_STEPS == 0 {
                let waited_seconds =
                    (((attempt + 1) as u128) * FEISHU_SETTLE_CHECK_INTERVAL.as_millis()) / 1000;
                mutate_status(app_handle, |status| {
                    status.running = true;
                    status.connected = true;
                    status.status_text = "总管家仍在处理飞书消息".to_string();
                    status.status_detail = Some(format!(
                        "正在等待总管家完成当前消息（运行中={}, 待完成任务={}，已等待 {} 秒）",
                        runtime_state.is_running, pending_tasks, waited_seconds
                    ));
                })
                .await;
            }
        }
        sleep(FEISHU_SETTLE_CHECK_INTERVAL).await;
    }
    let pending_tasks = count_pending_butler_tasks(app_handle, butler_conversation_id)?;
    let runtime_state = activity_manager.get_runtime_state(butler_conversation_id).await;
    Err(format!(
        "等待总管家处理飞书消息超时（运行中={}，待完成任务={}）",
        runtime_state.is_running, pending_tasks
    ))
}

pub(super) fn count_pending_butler_tasks(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
) -> Result<i64, String> {
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

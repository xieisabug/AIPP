use std::collections::HashSet;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;

use super::api::{
    build_feishu_interactive_payload, build_feishu_markdown_card,
    message_contains_preview_file_tool_call, message_is_preview_file_tool_result,
    render_message_for_feishu_delivery, resolve_preview_file_tool_call_for_message,
    send_markdown_message_to_target, split_markdown_into_feishu_blocks,
};
use super::config::load_runtime_config;
use super::relay::{find_latest_feishu_target, insert_external_link};
use super::types::*;

pub(super) fn collect_feishu_debug_resend_messages(
    selected_message: &crate::db::conversation_db::Message,
    conversation_messages: &[crate::db::conversation_db::Message],
) -> Vec<crate::db::conversation_db::Message> {
    if !message_contains_preview_file_tool_call(selected_message) {
        return vec![selected_message.clone()];
    }

    let mut selected_seen = false;
    let mut preview_results = Vec::new();

    for message in conversation_messages {
        if message.id == selected_message.id {
            selected_seen = true;
            continue;
        }
        if !selected_seen {
            continue;
        }
        if message.message_type == "response" || message.message_type == "assistant" {
            break;
        }
        if message_is_preview_file_tool_result(message) {
            preview_results.push(message.clone());
        }
    }

    if preview_results.is_empty() {
        vec![selected_message.clone()]
    } else {
        preview_results
    }
}

pub(crate) async fn resend_message_to_feishu_for_debug(
    app_handle: &AppHandle,
    message_id: i64,
) -> Result<FeishuDebugSendResult, String> {
    let config = load_runtime_config(app_handle).await?;
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        return Err("飞书 App ID 或 App Secret 未配置，无法执行调试重发".to_string());
    }

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let message = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .read(message_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("未找到消息: {message_id}"))?;

    let conversation_messages_raw = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .list_by_conversation_id(message.conversation_id)
        .map_err(|e| e.to_string())?;
    let mut seen_message_ids = HashSet::new();
    let mut conversation_messages = Vec::new();
    for (message_item, _) in conversation_messages_raw {
        if seen_message_ids.insert(message_item.id) {
            conversation_messages.push(message_item);
        }
    }
    let resend_messages = collect_feishu_debug_resend_messages(&message, &conversation_messages);
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;

    let target =
        find_latest_feishu_target(app_handle, message.conversation_id)?.ok_or_else(|| {
            "当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string()
        })?;

    let mut last_outcome: Option<FeishuDebugSendResult> = None;
    for resend_message in &resend_messages {
        let preview_tool_call =
            resolve_preview_file_tool_call_for_message(&mcp_db, &message, resend_message);
        let rendered_parts = render_message_for_feishu_delivery(
            app_handle,
            resend_message,
            preview_tool_call,
            RELAY_ORIGIN_AIPP,
        )
        .await?;
        let non_empty_parts: Vec<&str> =
            rendered_parts.iter().map(String::as_str).filter(|s| !s.trim().is_empty()).collect();
        if non_empty_parts.is_empty() {
            continue;
        }

        for rendered_text in &non_empty_parts {
            let outcome =
                send_markdown_message_to_target(app_handle, &config, &target, rendered_text)
                    .await?;
            insert_external_link(
                app_handle,
                ChannelLinkRecord {
                    external_message_id: &outcome.external_message_id,
                    external_chat_id: target.external_chat_id.as_deref(),
                    external_user_id: target.external_user_id.as_deref(),
                    conversation_id: message.conversation_id,
                    local_message_id: Some(resend_message.id),
                    direction: "outbound",
                    payload_type: &outcome.payload_type,
                },
            )?;
            last_outcome = Some(outcome);
        }
    }

    if last_outcome.is_none() {
        return Err("该消息没有可发送到飞书的可读内容".to_string());
    }

    Ok(last_outcome.expect("non_empty_parts guarantees at least one outcome"))
}

pub fn debug_build_feishu_markdown_card(markdown: &str) -> Result<Value, String> {
    build_feishu_markdown_card(markdown)
}

pub fn debug_build_feishu_interactive_payload(markdown: &str) -> Result<Value, String> {
    let card = build_feishu_markdown_card(markdown)?;
    Ok(build_feishu_interactive_payload(&card))
}

pub fn debug_describe_feishu_markdown_blocks(markdown: &str) -> Value {
    Value::Array(
        split_markdown_into_feishu_blocks(markdown)
            .into_iter()
            .map(|block| match block {
                FeishuCardBlock::Markdown(content) => json!({
                    "type": "markdown",
                    "content": content,
                }),
                FeishuCardBlock::Table(table) => json!({
                    "type": "table",
                    "headers": table.headers,
                    "rows": table.rows,
                }),
            })
            .collect(),
    )
}

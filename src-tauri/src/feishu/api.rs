use pulldown_cmark::{Options as MarkdownOptions, Parser as MarkdownParser};
use regex::Regex;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};
use tracing::{debug, warn};

use crate::api::ai::acp::{AcpPermissionRequestSnapshot, AcpPermissionState};
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::external_channels::presentation::{render_message_for_external_channel, RenderContext};
use crate::mcp::builtin_mcp::interaction::{
    inline_local_text_preview_files, prepare_preview_file_request_for_ui, PreviewFileRequest,
};
use crate::mcp::builtin_mcp::operation::state::PermissionRequestSnapshot;

use super::config::load_runtime_config;
use super::relay::{find_latest_feishu_target, insert_external_link};
use super::types::*;

pub(super) fn build_feishu_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(FEISHU_HTTP_TIMEOUT)
        .build()
        .expect("failed to build Feishu HTTP client")
}

pub(super) fn feishu_http_client(app_handle: &AppHandle) -> reqwest::Client {
    app_handle.state::<FeishuButlerState>().http_client.clone()
}

pub(super) async fn fetch_tenant_access_token(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
) -> Result<String, String> {
    crate::ensure_rustls_crypto_provider();
    let client = feishu_http_client(app_handle);
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

pub(super) fn build_feishu_markdown_card(markdown: &str) -> Result<Value, String> {
    let normalized = markdown.replace("\r\n", "\n").trim().to_string();
    if normalized.is_empty() {
        return Err("飞书卡片内容为空".to_string());
    }

    // Validate that the source is parseable markdown before building a card.
    let _ = MarkdownParser::new_ext(&normalized, MarkdownOptions::all()).count();

    let mut elements = Vec::new();
    for (index, block) in split_markdown_into_feishu_blocks(&normalized).into_iter().enumerate() {
        match block {
            FeishuCardBlock::Markdown(content) => {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    elements.push(build_feishu_markdown_element(trimmed));
                }
            }
            FeishuCardBlock::Table(table) => {
                elements.push(build_feishu_table_element(index, &table)?);
            }
        }
    }

    if elements.is_empty() {
        return Err("飞书卡片缺少可发送内容".to_string());
    }

    Ok(json!({
        "schema": "2.0",
        "body": {
            "elements": elements
        }
    }))
}

pub(super) fn split_markdown_into_feishu_blocks(markdown: &str) -> Vec<FeishuCardBlock> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut blocks = Vec::new();
    let mut markdown_buffer = Vec::new();
    let mut index = 0usize;
    let mut in_fence = false;
    let mut fence_marker = '`';
    let mut fence_length = 0usize;

    while index < lines.len() {
        let line = lines[index];

        if let Some((marker, length)) = parse_fence_delimiter(line) {
            if !in_fence {
                in_fence = true;
                fence_marker = marker;
                fence_length = length;
            } else if marker == fence_marker && length >= fence_length {
                in_fence = false;
            }
            markdown_buffer.push(line.to_string());
            index += 1;
            continue;
        }

        if !in_fence
            && index + 1 < lines.len()
            && looks_like_markdown_table_header(lines[index], lines[index + 1])
        {
            flush_markdown_block(&mut blocks, &mut markdown_buffer);

            let mut table_lines = vec![lines[index].to_string(), lines[index + 1].to_string()];
            index += 2;
            while index < lines.len() && looks_like_markdown_table_row(lines[index]) {
                table_lines.push(lines[index].to_string());
                index += 1;
            }

            match parse_markdown_table(&table_lines) {
                Ok(table) => blocks.push(FeishuCardBlock::Table(table)),
                Err(_) => blocks.push(FeishuCardBlock::Markdown(table_lines.join("\n"))),
            }
            continue;
        }

        markdown_buffer.push(line.to_string());
        index += 1;
    }

    flush_markdown_block(&mut blocks, &mut markdown_buffer);
    blocks
}

fn flush_markdown_block(blocks: &mut Vec<FeishuCardBlock>, markdown_buffer: &mut Vec<String>) {
    if markdown_buffer.is_empty() {
        return;
    }
    let content = markdown_buffer.join("\n");
    markdown_buffer.clear();
    if !content.trim().is_empty() {
        blocks.push(FeishuCardBlock::Markdown(content));
    }
}

pub(super) fn parse_fence_delimiter(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|ch| *ch == marker).count();
    (length >= 3).then_some((marker, length))
}

fn looks_like_markdown_table_header(header_line: &str, separator_line: &str) -> bool {
    looks_like_markdown_table_row(header_line) && is_markdown_table_separator(separator_line)
}

fn looks_like_markdown_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && trimmed.contains('|')
}

fn is_markdown_table_separator(line: &str) -> bool {
    let cells = split_markdown_table_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let trimmed = cell.trim();
            !trimmed.is_empty()
                && trimmed.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
                && trimmed.chars().any(|ch| ch == '-')
        })
}

pub(super) fn parse_markdown_table(lines: &[String]) -> Result<FeishuMarkdownTable, String> {
    if lines.len() < 2 {
        return Err("markdown 表格行数不足".to_string());
    }

    let headers = split_markdown_table_row(&lines[0])
        .into_iter()
        .map(|cell| cell.trim().to_string())
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Err("markdown 表格缺少表头".to_string());
    }
    if !is_markdown_table_separator(&lines[1]) {
        return Err("markdown 表格缺少分隔行".to_string());
    }

    let rows = lines
        .iter()
        .skip(2)
        .map(|line| normalize_table_row(split_markdown_table_row(line), headers.len()))
        .collect::<Vec<_>>();

    Ok(FeishuMarkdownTable { headers, rows })
}

fn split_markdown_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = trimmed.chars().peekable();

    if matches!(chars.peek(), Some('|')) {
        chars.next();
    }

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if matches!(chars.peek(), Some('|')) {
                    current.push('|');
                    chars.next();
                } else {
                    current.push(ch);
                }
            }
            '|' => {
                cells.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() || !trimmed.ends_with('|') {
        cells.push(current.trim().to_string());
    }

    cells
}

fn normalize_table_row(mut cells: Vec<String>, width: usize) -> Vec<String> {
    if cells.len() > width {
        cells.truncate(width);
        return cells;
    }
    while cells.len() < width {
        cells.push(String::new());
    }
    cells
}

fn build_feishu_markdown_element(content: &str) -> Value {
    json!({
        "tag": "markdown",
        "content": content,
        "text_align": "left"
    })
}

fn build_feishu_table_element(index: usize, table: &FeishuMarkdownTable) -> Result<Value, String> {
    if table.headers.is_empty() {
        return Err("飞书表格缺少列定义".to_string());
    }

    let columns = table
        .headers
        .iter()
        .enumerate()
        .map(|(column_index, header)| {
            json!({
                "name": format!("col_{}", column_index + 1),
                "display_name": if header.is_empty() {
                    format!("列{}", column_index + 1)
                } else {
                    header.clone()
                },
                "data_type": "lark_md",
                "width": "auto"
            })
        })
        .collect::<Vec<_>>();

    let rows = table
        .rows
        .iter()
        .map(|row| {
            let mut map = Map::new();
            for (column_index, cell) in row.iter().enumerate() {
                map.insert(format!("col_{}", column_index + 1), Value::String(cell.clone()));
            }
            Value::Object(map)
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "tag": "table",
        "element_id": format!("md_table_{}", index + 1),
        "row_height": "low",
        "page_size": rows.len().max(1),
        "columns": columns,
        "rows": rows
    }))
}

pub(super) async fn reply_markdown_message(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    reply_to_message_id: &str,
    markdown: &str,
) -> Result<FeishuReplyOutcome, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    let mut interactive_error = None;
    let interactive_card = match build_feishu_markdown_card(markdown) {
        Ok(card) => Some(card),
        Err(error) => {
            let error = format!("构建飞书卡片失败: {error}");
            debug!(error = %error, "failed to build feishu markdown card, falling back to raw text");
            interactive_error = Some(error);
            None
        }
    };

    if let Some(card) = interactive_card.as_ref() {
        match send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_interactive_payload(card),
        )
        .await
        {
            Ok(message_id) => {
                return Ok(FeishuReplyOutcome {
                    message_id,
                    payload_type: "interactive",
                    interactive_error,
                    interactive_card,
                })
            }
            Err(error) => {
                warn!(error = %error, "failed to send feishu interactive reply, falling back to raw text");
                interactive_error = Some(format!("发送飞书 interactive 卡片失败: {error}"));
            }
        }
    }

    let message_id = send_reply_message_request(
        &client,
        config,
        &token,
        reply_to_message_id,
        build_feishu_text_payload(markdown),
    )
    .await?;

    Ok(FeishuReplyOutcome { message_id, payload_type: "text", interactive_error, interactive_card })
}

pub(super) async fn send_message_request(
    client: &reqwest::Client,
    config: &FeishuRuntimeConfig,
    token: &str,
    receive_id_type: &str,
    receive_id: &str,
    payload: Value,
) -> Result<String, String> {
    let url = format!(
        "{}/open-apis/im/v1/messages?receive_id_type={}",
        config.base_url.trim_end_matches('/'),
        receive_id_type
    );
    let mut payload_object =
        payload.as_object().cloned().ok_or_else(|| "飞书发送消息 payload 格式非法".to_string())?;
    payload_object.insert("receive_id".to_string(), Value::String(receive_id.to_string()));
    let response = client
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&Value::Object(payload_object))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: SendMessageResponse = response.json().await.map_err(|e| e.to_string())?;
    if body.code != 0 {
        return Err(format!("发送飞书消息失败: {}", body.msg));
    }
    body.data
        .map(|data| data.message_id)
        .ok_or_else(|| "飞书发送成功但未返回 message_id".to_string())
}

pub(super) fn build_feishu_text_payload(text: &str) -> Value {
    json!({
        "msg_type": "text",
        "content": json!({ "text": text }).to_string()
    })
}

pub(super) fn build_feishu_interactive_payload(card: &Value) -> Value {
    json!({
        "msg_type": "interactive",
        "content": card.to_string()
    })
}

pub(super) async fn send_interactive_card_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    card: &Value,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    if let Some(reply_to_message_id) = target.reply_to_message_id.as_deref() {
        send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_interactive_payload(card),
        )
        .await
    } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
        send_message_request(
            &client,
            config,
            &token,
            receive_id_type,
            receive_id,
            build_feishu_interactive_payload(card),
        )
        .await
    } else {
        Err("当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string())
    }
}

pub(super) async fn send_text_message_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    if let Some(reply_to_message_id) = target.reply_to_message_id.as_deref() {
        send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_text_payload(text),
        )
        .await
    } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
        send_message_request(
            &client,
            config,
            &token,
            receive_id_type,
            receive_id,
            build_feishu_text_payload(text),
        )
        .await
    } else {
        Err("当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string())
    }
}

pub(super) async fn send_permission_review_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    card: &Value,
    fallback_text: &str,
) -> Result<FeishuReplyOutcome, String> {
    match send_interactive_card_to_target(app_handle, config, target, card).await {
        Ok(message_id) => Ok(FeishuReplyOutcome {
            message_id,
            payload_type: "interactive",
            interactive_error: None,
            interactive_card: Some(card.clone()),
        }),
        Err(error) => {
            warn!(error = %error, "failed to send permission review card, falling back to raw text");
            let message_id =
                send_text_message_to_target(app_handle, config, target, fallback_text).await?;
            Ok(FeishuReplyOutcome {
                message_id,
                payload_type: "text",
                interactive_error: Some(format!("发送飞书 interactive 卡片失败: {error}")),
                interactive_card: Some(card.clone()),
            })
        }
    }
}

pub(super) fn build_operation_permission_card(request: &PermissionRequestSnapshot) -> Value {
    let review_code = request.review_code.clone();
    let request_id = request.event.request_id.clone();
    json!({
        "schema": "2.0",
        "config": { "update_multi": true, "wide_screen_mode": true },
        "body": {
            "elements": [
                {
                    "tag": "markdown",
                    "content": format!(
                        "总管家收到一个操作权限请求。\n\n**审批号**：`{review_code}`\n**操作**：{operation}\n**路径**：`{path}`\n\n如果卡片按钮不可用，也可以直接回复：`批准一次 {review_code}` / `本任务批准 {review_code}` / `助手批准 {review_code}` / `拒绝 {review_code}`",
                        review_code = review_code,
                        operation = request.event.operation,
                        path = truncate_text(&request.event.path, 220),
                    ),
                    "text_align": "left"
                },
                {
                    "tag": "action",
                    "actions": [
                        {
                            "tag": "button",
                            "type": "primary",
                            "text": { "tag": "plain_text", "content": "允许一次" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request_id,
                                "decision": "allow"
                            }
                        },
                        {
                            "tag": "button",
                            "text": { "tag": "plain_text", "content": "本任务信任" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request.event.request_id.clone(),
                                "decision": "allow_for_conversation"
                            }
                        },
                        {
                            "tag": "button",
                            "text": { "tag": "plain_text", "content": "助手工作区信任" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request.event.request_id.clone(),
                                "decision": "allow_for_assistant"
                            }
                        },
                        {
                            "tag": "button",
                            "type": "danger",
                            "text": { "tag": "plain_text", "content": "拒绝" },
                            "value": {
                                "request_kind": "operation_permission",
                                "request_id": request.event.request_id.clone(),
                                "decision": "deny"
                            }
                        }
                    ]
                }
            ]
        }
    })
}

pub(super) fn build_acp_permission_card(request: &AcpPermissionRequestSnapshot) -> Value {
    let request_id = request.event.request_id.clone();
    let mut actions = request
        .event
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            json!({
                "tag": "button",
                "type": if option.kind.starts_with("allow") { "primary" } else { "default" },
                "text": {
                    "tag": "plain_text",
                    "content": format!("{} {}", index + 1, option.name)
                },
                "value": {
                    "request_kind": "acp_permission",
                    "request_id": request_id.clone(),
                    "option_id": option.option_id.clone()
                }
            })
        })
        .collect::<Vec<_>>();
    actions.push(json!({
        "tag": "button",
        "type": "danger",
        "text": { "tag": "plain_text", "content": "取消" },
        "value": {
            "request_kind": "acp_permission",
            "request_id": request.event.request_id.clone(),
            "cancelled": true
        }
    }));

    json!({
        "schema": "2.0",
        "config": { "update_multi": true, "wide_screen_mode": true },
        "body": {
            "elements": [
                {
                    "tag": "markdown",
                    "content": format!(
                        "总管家收到一个 ACP 权限请求。\n\n**审批号**：`{review_code}`\n**标题**：{title}\n**类型**：{kind}\n**参数**：`{parameters}`\n\n如果卡片按钮不可用，也可以直接回复：`批准 1 {review_code}`、`批准 2 {review_code}` 或 `取消 {review_code}`。",
                        review_code = request.review_code,
                        title = request.event.title.as_deref().unwrap_or("未命名"),
                        kind = request.event.kind.as_deref().unwrap_or("unknown"),
                        parameters = truncate_text(request.event.parameters.as_deref().unwrap_or("无"), 220),
                    ),
                    "text_align": "left"
                },
                {
                    "tag": "action",
                    "actions": actions
                }
            ]
        }
    })
}

pub(crate) async fn try_deliver_operation_permission_to_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    request: &PermissionRequestSnapshot,
) -> Result<bool, String> {
    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled {
        return Ok(false);
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(false);
    };

    let card = build_operation_permission_card(request);
    let fallback_text = build_operation_permission_fallback_text(request);
    let outcome =
        send_permission_review_to_target(app_handle, &config, &target, &card, &fallback_text)
            .await?;
    if let Some(interactive_error) = outcome.interactive_error.as_deref() {
        warn!(
            request_id = %request.event.request_id,
            error = %interactive_error,
            "operation permission Feishu delivery fell back to text"
        );
    }

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &outcome.message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id,
            local_message_id: None,
            direction: "outbound",
            payload_type: outcome.payload_type,
        },
    )?;
    let state = app_handle.state::<crate::mcp::builtin_mcp::OperationState>();
    state
        .set_permission_feishu_delivery(
            &request.event.request_id,
            Some(outcome.message_id.clone()),
            target.external_user_id.clone(),
            target.external_chat_id.clone(),
        )
        .await;
    Ok(true)
}

pub(crate) async fn try_deliver_acp_permission_to_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    request: &AcpPermissionRequestSnapshot,
) -> Result<bool, String> {
    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled {
        return Ok(false);
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(false);
    };

    let card = build_acp_permission_card(request);
    let fallback_text = build_acp_permission_fallback_text(request);
    let outcome =
        send_permission_review_to_target(app_handle, &config, &target, &card, &fallback_text)
            .await?;
    if let Some(interactive_error) = outcome.interactive_error.as_deref() {
        warn!(
            request_id = %request.event.request_id,
            error = %interactive_error,
            "ACP permission Feishu delivery fell back to text"
        );
    }

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &outcome.message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id,
            local_message_id: None,
            direction: "outbound",
            payload_type: outcome.payload_type,
        },
    )?;
    let state = app_handle.state::<AcpPermissionState>();
    state
        .set_feishu_delivery(
            &request.event.request_id,
            Some(outcome.message_id.clone()),
            target.external_user_id.clone(),
            target.external_chat_id.clone(),
        )
        .await;
    Ok(true)
}

pub(super) async fn send_reply_message_request(
    client: &reqwest::Client,
    config: &FeishuRuntimeConfig,
    token: &str,
    reply_to_message_id: &str,
    payload: Value,
) -> Result<String, String> {
    let url = format!(
        "{}/open-apis/im/v1/messages/{}/reply",
        config.base_url.trim_end_matches('/'),
        reply_to_message_id
    );
    let response = client
        .post(url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&payload)
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

pub(super) async fn reply_text_message(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    reply_to_message_id: &str,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    send_reply_message_request(
        &client,
        config,
        &token,
        reply_to_message_id,
        build_feishu_text_payload(text),
    )
    .await
}

pub(super) async fn send_text_message_to_open_id(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    open_id: &str,
    text: &str,
) -> Result<String, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    send_message_request(
        &client,
        config,
        &token,
        "open_id",
        open_id,
        build_feishu_text_payload(text),
    )
    .await
}

pub(super) async fn send_markdown_message_to_target(
    app_handle: &AppHandle,
    config: &FeishuRuntimeConfig,
    target: &ChannelLinkTarget,
    markdown: &str,
) -> Result<FeishuDebugSendResult, String> {
    let token = fetch_tenant_access_token(app_handle, config).await?;
    let client = feishu_http_client(app_handle);
    let mut interactive_error = None;
    let interactive_card = match build_feishu_markdown_card(markdown) {
        Ok(card) => Some(card),
        Err(error) => {
            let error = format!("构建飞书卡片失败: {error}");
            debug!(error = %error, "failed to build feishu markdown card, falling back to raw text");
            interactive_error = Some(error);
            None
        }
    };
    let delivery_mode = if target.reply_to_message_id.is_some() { "reply" } else { "direct" };
    let selected_target = select_receive_target(target)
        .map(|(target_type, target_id)| (target_type.to_string(), target_id.to_string()));

    if let Some(card) = interactive_card.as_ref() {
        let interactive_result = if let Some(reply_to_message_id) =
            target.reply_to_message_id.as_deref()
        {
            send_reply_message_request(
                &client,
                config,
                &token,
                reply_to_message_id,
                build_feishu_interactive_payload(card),
            )
            .await
        } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
            send_message_request(
                &client,
                config,
                &token,
                receive_id_type,
                receive_id,
                build_feishu_interactive_payload(card),
            )
            .await
        } else {
            Err("当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string())
        };

        match interactive_result {
            Ok(message_id) => {
                return Ok(FeishuDebugSendResult {
                    external_message_id: message_id,
                    payload_type: "interactive".to_string(),
                    delivery_mode: delivery_mode.to_string(),
                    reply_to_message_id: target.reply_to_message_id.clone(),
                    target_type: selected_target
                        .as_ref()
                        .map(|(target_type, _)| target_type.clone()),
                    target_id: selected_target.as_ref().map(|(_, target_id)| target_id.clone()),
                    rendered_text: markdown.to_string(),
                    interactive_error,
                    interactive_card,
                });
            }
            Err(error) => {
                warn!(error = %error, "failed to send feishu interactive message, falling back to raw text");
                interactive_error = Some(format!("发送飞书 interactive 卡片失败: {error}"));
            }
        }
    }

    let message_id = if let Some(reply_to_message_id) = target.reply_to_message_id.as_deref() {
        send_reply_message_request(
            &client,
            config,
            &token,
            reply_to_message_id,
            build_feishu_text_payload(markdown),
        )
        .await?
    } else if let Some((receive_id_type, receive_id)) = select_receive_target(target) {
        send_message_request(
            &client,
            config,
            &token,
            receive_id_type,
            receive_id,
            build_feishu_text_payload(markdown),
        )
        .await?
    } else {
        return Err(
            "当前对话没有可用的飞书发送目标，请先让该对话与飞书建立一次消息链路".to_string()
        );
    };

    Ok(FeishuDebugSendResult {
        external_message_id: message_id,
        payload_type: "text".to_string(),
        delivery_mode: delivery_mode.to_string(),
        reply_to_message_id: target.reply_to_message_id.clone(),
        target_type: selected_target.as_ref().map(|(target_type, _)| target_type.clone()),
        target_id: selected_target.as_ref().map(|(_, target_id)| target_id.clone()),
        rendered_text: markdown.to_string(),
        interactive_error,
        interactive_card,
    })
}

pub(super) fn message_contains_preview_file_tool_call(
    message: &crate::db::conversation_db::Message,
) -> bool {
    if message.message_type != "response" {
        return false;
    }

    let content = message.content.as_str();
    content.contains("MCP_TOOL_CALL:")
        && (content.contains("\"tool_name\":\"preview_file\"")
            || content.contains("\"tool_name\": \"preview_file\""))
}

pub(super) fn message_is_preview_file_tool_result(
    message: &crate::db::conversation_db::Message,
) -> bool {
    message.message_type == "tool_result" && message.content.contains("Tool: preview_file")
}

pub(super) fn find_preview_file_tool_call_for_message(
    mcp_db: &MCPDatabase,
    selected_message: &crate::db::conversation_db::Message,
    resend_message: &crate::db::conversation_db::Message,
) -> Option<MCPToolCall> {
    if selected_message.message_type == "tool_result" {
        let tool_call_id =
            crate::api::ai::conversation::extract_tool_call_id(&selected_message.content)?;
        let conversation_calls =
            mcp_db.get_mcp_tool_calls_by_conversation(resend_message.conversation_id).ok()?;
        return conversation_calls.into_iter().find(|call| {
            call.tool_name == "preview_file"
                && call.llm_call_id.as_deref() == Some(tool_call_id.as_str())
        });
    }

    let message_calls = mcp_db.get_mcp_tool_calls_by_message(selected_message.id).ok()?;
    message_calls.into_iter().find(|call| call.tool_name == "preview_file")
}

pub(super) async fn render_preview_file_mcp_call_for_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    tool_call: &MCPToolCall,
) -> Result<Option<Vec<String>>, String> {
    let request: PreviewFileRequest = serde_json::from_str(&tool_call.parameters)
        .map_err(|e| format!("解析 preview_file 参数失败: {e}"))?;
    render_preview_file_request_for_feishu(app_handle, conversation_id, request).await
}

async fn render_preview_file_request_for_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    request: PreviewFileRequest,
) -> Result<Option<Vec<String>>, String> {
    let mut request = request;
    inline_local_text_preview_files(&mut request.files)?;
    let hydrated_request =
        prepare_preview_file_request_for_ui(app_handle.clone(), Some(conversation_id), request)
            .await?;

    let files = hydrated_request
        .files
        .into_iter()
        .map(|file| {
            serde_json::json!({
                "title": file.title,
                "type": file.file_type,
                "content": file.content,
                "url": file.url,
                "language": file.language,
                "description": file.description,
            })
        })
        .collect::<Vec<_>>();

    let rendered =
        crate::external_channels::presentation::render_preview_file_result_parts_for_feishu(
            &serde_json::json!({ "files": files }),
        );
    Ok((!rendered.is_empty()).then_some(rendered))
}

async fn render_preview_tool_result_parameters_for_feishu(
    app_handle: &AppHandle,
    delivery_message: &crate::db::conversation_db::Message,
) -> Result<Option<Vec<String>>, String> {
    let Some(params) = preview_file_parameters_from_tool_result_message(delivery_message) else {
        return Ok(None);
    };
    let request: PreviewFileRequest = serde_json::from_value(params)
        .map_err(|e| format!("解析 preview_file tool_result 参数失败: {e}"))?;
    render_preview_file_request_for_feishu(app_handle, delivery_message.conversation_id, request)
        .await
}

pub(super) fn preview_file_parameters_from_response_message(
    message: &crate::db::conversation_db::Message,
) -> Option<Value> {
    if message.message_type != "response" {
        return None;
    }

    let regex = Regex::new(r"(?s)<!-- MCP_TOOL_CALL:(.*?) -->").ok()?;
    for capture in regex.captures_iter(&message.content) {
        let tool_data = serde_json::from_str::<Value>(&capture[1]).ok()?;
        if tool_data.get("tool_name").and_then(Value::as_str) != Some("preview_file") {
            continue;
        }
        let parameters = tool_data.get("parameters").and_then(Value::as_str)?;
        if let Ok(parsed) = serde_json::from_str::<Value>(parameters) {
            return Some(parsed);
        }
    }
    None
}

async fn render_preview_response_parameters_for_feishu(
    app_handle: &AppHandle,
    delivery_message: &crate::db::conversation_db::Message,
) -> Result<Option<Vec<String>>, String> {
    let Some(params) = preview_file_parameters_from_response_message(delivery_message) else {
        return Ok(None);
    };
    let request: PreviewFileRequest = serde_json::from_value(params)
        .map_err(|e| format!("解析 preview_file response 参数失败: {e}"))?;
    render_preview_file_request_for_feishu(app_handle, delivery_message.conversation_id, request)
        .await
}

pub(super) fn resolve_preview_file_tool_call_for_message(
    mcp_db: &MCPDatabase,
    source_message: &crate::db::conversation_db::Message,
    delivery_message: &crate::db::conversation_db::Message,
) -> Option<MCPToolCall> {
    if !message_is_preview_file_tool_result(delivery_message) {
        return None;
    }
    find_preview_file_tool_call_for_message(mcp_db, source_message, delivery_message)
}

fn preview_file_parameters_from_tool_result_message(
    message: &crate::db::conversation_db::Message,
) -> Option<Value> {
    if !message_is_preview_file_tool_result(message) {
        return None;
    }

    message
        .content
        .lines()
        .find_map(|line| line.strip_prefix("Parameters: "))
        .and_then(|value| serde_json::from_str::<Value>(value.trim()).ok())
}

pub(super) fn preview_tool_result_has_inline_content(
    message: &crate::db::conversation_db::Message,
) -> bool {
    preview_file_parameters_from_tool_result_message(message)
        .and_then(|params| params.get("files").and_then(Value::as_array).cloned())
        .is_some_and(|files| {
            files.iter().any(|file| {
                file.get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| !content.trim().is_empty())
            })
        })
}

pub(super) fn render_inline_preview_tool_result_parts_for_feishu(
    delivery_message: &crate::db::conversation_db::Message,
    relay_origin: &str,
) -> Option<Vec<String>> {
    if !preview_tool_result_has_inline_content(delivery_message) {
        return None;
    }

    let rendered_from_tool_result = render_message_for_external_channel(
        delivery_message,
        &RenderContext { channel: CHANNEL_FEISHU, relay_origin },
    );
    let non_empty_parts: Vec<String> =
        rendered_from_tool_result.into_iter().filter(|part| !part.trim().is_empty()).collect();
    (!non_empty_parts.is_empty()).then_some(non_empty_parts)
}

pub(super) async fn render_message_for_feishu_delivery(
    app_handle: &AppHandle,
    delivery_message: &crate::db::conversation_db::Message,
    preview_tool_call: Option<MCPToolCall>,
    relay_origin: &str,
) -> Result<Vec<String>, String> {
    if let Some(tool_call) = preview_tool_call {
        if let Some(rendered) = render_preview_file_mcp_call_for_feishu(
            app_handle,
            delivery_message.conversation_id,
            &tool_call,
        )
        .await?
        {
            return Ok(rendered);
        }
    }

    if let Some(rendered) =
        render_preview_response_parameters_for_feishu(app_handle, delivery_message).await?
    {
        return Ok(rendered);
    }

    if let Some(rendered) =
        render_preview_tool_result_parameters_for_feishu(app_handle, delivery_message).await?
    {
        return Ok(rendered);
    }

    if let Some(rendered) =
        render_inline_preview_tool_result_parts_for_feishu(delivery_message, relay_origin)
    {
        return Ok(rendered);
    }

    Ok(render_message_for_external_channel(
        delivery_message,
        &RenderContext { channel: CHANNEL_FEISHU, relay_origin },
    ))
}

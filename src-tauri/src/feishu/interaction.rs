use std::collections::HashMap;

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};
use tracing::{debug, info};

use crate::db::connection::{params, OptionalExtension};
use crate::db::conversation_db::ConversationDatabase;
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::mcp::builtin_mcp::interaction::{
    AskUserQuestionItem, AskUserQuestionRequest, AskUserQuestionRequestEvent,
};

use super::types::*;
use super::config::load_runtime_config;
use super::relay::{
    find_active_relay_scope, find_latest_feishu_target, insert_external_link,
    mark_relay_scope_progress, spawn_feishu_relay_scope_worker,
};
use super::api::send_interactive_card_to_target;

pub(super) fn is_missing_ask_user_request_error(error: &str) -> bool {
    error.contains("AskUserQuestion request not found")
}

pub(super) fn build_ask_user_question_tool_result(answers: &HashMap<String, String>) -> String {
    json!([{
        "type": "json",
        "json": {
            "answers": answers
        }
    }])
    .to_string()
}

pub(super) fn find_conversation_id_by_external_message(
    app_handle: &AppHandle,
    external_message_id: &str,
) -> Result<Option<i64>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = db.get_connection().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT conversation_id
         FROM external_channel_message_link
         WHERE channel = ?1 AND external_message_id = ?2
         LIMIT 1",
        params![CHANNEL_FEISHU, external_message_id],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub(super) fn find_latest_recoverable_ask_user_tool_call(calls: &[MCPToolCall]) -> Option<&MCPToolCall> {
    calls.iter().find(|call| {
        call.tool_name == "ask_user_question"
            && matches!(call.status.as_str(), "pending" | "executing")
    })
}

pub(super) async fn recover_answers_from_callback_payload(
    app_handle: &AppHandle,
    callback: &FeishuCardActionCallback,
    form_value: &Map<String, Value>,
) -> Result<HashMap<String, String>, String> {
    let open_message_id = callback
        .event()
        .context
        .as_ref()
        .and_then(|context| context.open_message_id.clone())
        .and_then(|value| normalize_optional_id(Some(value)))
        .ok_or_else(|| {
            "飞书卡片回调缺少 open_message_id，无法恢复 ask_user_question 状态".to_string()
        })?;
    let conversation_id =
        find_conversation_id_by_external_message(app_handle, &open_message_id)?
            .ok_or_else(|| format!("未找到飞书消息 {} 关联的会话", open_message_id))?;
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let calls =
        mcp_db.get_mcp_tool_calls_by_conversation(conversation_id).map_err(|e| e.to_string())?;
    let tool_call = find_latest_recoverable_ask_user_tool_call(&calls).ok_or_else(|| {
        format!("会话 {} 中没有可恢复的 ask_user_question 工具调用", conversation_id)
    })?;
    let request: AskUserQuestionRequest = serde_json::from_str(&tool_call.parameters)
        .map_err(|e| format!("解析 ask_user_question 参数失败: {}", e))?;
    map_ask_user_form_values_to_answers(&request.questions, form_value)
}

pub(super) async fn try_recover_feishu_ask_user_resolution(
    app_handle: &AppHandle,
    callback: &FeishuCardActionCallback,
    execution_result: Result<String, String>,
) -> Result<bool, String> {
    let open_message_id = callback
        .event()
        .context
        .as_ref()
        .and_then(|context| context.open_message_id.clone())
        .and_then(|value| normalize_optional_id(Some(value)));
    let Some(open_message_id) = open_message_id else {
        return Ok(false);
    };
    let Some(conversation_id) =
        find_conversation_id_by_external_message(app_handle, &open_message_id)?
    else {
        return Ok(false);
    };
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let calls =
        mcp_db.get_mcp_tool_calls_by_conversation(conversation_id).map_err(|e| e.to_string())?;

    if let Some(tool_call) = find_latest_recoverable_ask_user_tool_call(&calls) {
        crate::mcp::execution_api::finalize_tool_call_from_external_result(
            app_handle,
            tool_call.id,
            execution_result,
        )
        .await?;
        info!(
            call_id = tool_call.id,
            conversation_id,
            open_message_id = %open_message_id,
            "Recovered AskUserQuestion resolution from Feishu callback"
        );
        return Ok(true);
    }

    if calls.iter().any(|call| {
        call.tool_name == "ask_user_question"
            && matches!(call.status.as_str(), "success" | "failed")
    }) {
        debug!(
            conversation_id,
            open_message_id = %open_message_id,
            "Ignore duplicate or stale Feishu AskUserQuestion callback"
        );
        return Ok(true);
    }

    Ok(false)
}

pub(super) async fn build_ask_user_answers_from_card_callback(
    app_handle: &AppHandle,
    request_id: &str,
    callback: &FeishuCardActionCallback,
) -> Result<HashMap<String, String>, String> {
    let Some(interaction_state) =
        app_handle.try_state::<crate::mcp::builtin_mcp::interaction::InteractionState>()
    else {
        return Err("InteractionState not found".to_string());
    };
    let request = interaction_state
        .get_ask_user_request(request_id)
        .await
        .ok_or_else(|| "AskUserQuestion request not found".to_string())?;
    let form_value = callback
        .event()
        .action
        .form_value
        .as_ref()
        .ok_or_else(|| "飞书卡片回调缺少 form_value".to_string())?;
    map_ask_user_form_values_to_answers(&request.questions, form_value)
}

pub(super) fn map_ask_user_form_values_to_answers(
    questions: &[AskUserQuestionItem],
    form_value: &Map<String, Value>,
) -> Result<HashMap<String, String>, String> {
    let mut answers = HashMap::new();
    for (index, question) in questions.iter().enumerate() {
        let field_name = format!("question_{}", index);
        let raw_value = form_value
            .get(&field_name)
            .ok_or_else(|| format!("飞书卡片回答缺少字段 {}", field_name))?;
        let answer = match raw_value {
            Value::String(value) => value.clone(),
            Value::Array(items) => {
                items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
            }
            Value::Object(map) => map
                .get("value")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("values").and_then(Value::as_array).map(|items| {
                        items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
                    })
                })
                .ok_or_else(|| format!("飞书卡片回答字段 {} 结构无效", field_name))?,
            _ => return Err(format!("飞书卡片回答字段 {} 类型无效", field_name)),
        };
        answers.insert(question.question.clone(), answer);
    }
    Ok(answers)
}

pub(super) fn build_ask_user_question_card(event: &AskUserQuestionRequestEvent) -> Value {
    let mut elements = Vec::new();
    elements.push(json!({
        "tag": "markdown",
        "content": "总管家需要你补充一些信息后才能继续。",
        "text_align": "left"
    }));

    let mut form_elements = Vec::new();
    for (index, question) in event.questions.iter().enumerate() {
        let field_name = format!("question_{}", index);
        let options = question
            .options
            .iter()
            .map(|option| {
                json!({
                    "text": {
                        "tag": "plain_text",
                        "content": format!("{} - {}", option.label, option.description)
                    },
                    "value": option.label
                })
            })
            .collect::<Vec<_>>();
        form_elements.push(json!({
            "tag": "markdown",
            "content": format!("**{}**\n{}", question.header, question.question),
            "text_align": "left"
        }));
        let tag = if question.multi_select { "multi_select_static" } else { "select_static" };
        form_elements.push(json!({
            "tag": tag,
            "name": field_name,
            "placeholder": {
                "tag": "plain_text",
                "content": format!("请选择：{}", question.question)
            },
            "required": true,
            "options": options,
        }));
    }

    form_elements.push(json!({
        "tag": "button",
        "name": "ask_user_submit",
        "type": "primary",
        "text": { "tag": "plain_text", "content": "提交" },
        "behaviors": [{
            "type": "callback",
            "value": { "action": "submit", "request_id": event.request_id }
        }],
        "form_action_type": "submit"
    }));

    elements.push(json!({
        "tag": "form",
        "name": format!("ask_user_{}", event.request_id),
        "elements": form_elements
    }));

    elements.push(json!({
        "tag": "button",
        "name": "ask_user_cancel",
        "text": { "tag": "plain_text", "content": "取消" },
        "behaviors": [{
            "type": "callback",
            "value": { "action": "cancel", "request_id": event.request_id }
        }]
    }));

    json!({
        "schema": "2.0",
        "config": { "update_multi": true, "wide_screen_mode": true },
        "body": { "elements": elements }
    })
}

pub(crate) async fn try_deliver_ask_user_question_to_feishu(
    app_handle: &AppHandle,
    conversation_id: i64,
    event: &AskUserQuestionRequestEvent,
) -> Result<bool, String> {
    let config = load_runtime_config(app_handle).await?;
    if !config.butler_enabled || !config.enabled {
        return Ok(false);
    }

    let Some(target) = find_latest_feishu_target(app_handle, conversation_id)? else {
        return Ok(false);
    };
    let card = build_ask_user_question_card(event);
    let external_message_id =
        send_interactive_card_to_target(app_handle, &config, &target, &card).await?;

    insert_external_link(
        app_handle,
        ChannelLinkRecord {
            external_message_id: &external_message_id,
            external_chat_id: target.external_chat_id.as_deref(),
            external_user_id: target.external_user_id.as_deref(),
            conversation_id,
            local_message_id: None,
            direction: "outbound",
            payload_type: "interactive",
        },
    )?;

    if let Some(scope) = find_active_relay_scope(app_handle, conversation_id, RELAY_ORIGIN_FEISHU)?
    {
        mark_relay_scope_progress(
            app_handle,
            scope.id,
            scope.last_delivered_local_message_id,
            "waiting_user_input",
        )?;
        spawn_feishu_relay_scope_worker(app_handle, scope.id, conversation_id).await;
    }

    Ok(true)
}

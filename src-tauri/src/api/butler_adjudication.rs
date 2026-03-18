use std::collections::HashSet;
use std::time::Duration;

use genai::chat::ChatOptions;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::api::ai::acp::AcpPermissionRequestSnapshot;
use crate::api::ai::config::{get_network_proxy_from_config, get_request_timeout_from_config};
use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy};
use crate::api::butler_api::get_butler_model_selection;
use crate::api::genai_client;
use crate::api::operation_api::{confirm_acp_permission, confirm_operation_permission};
use crate::db::conversation_db::{ConversationDatabase, Message, Repository};
use crate::db::llm_db::LLMDatabase;
use crate::mcp::builtin_mcp::operation::state::PermissionRequestSnapshot;
use crate::mcp::builtin_mcp::operation::types::PermissionDecision;

const AUTO_REVIEW_INLINE_WAIT: Duration = Duration::from_secs(2);
const TASK_CONTEXT_MESSAGE_LIMIT: usize = 6;
const BUTLER_CONTEXT_MESSAGE_LIMIT: usize = 4;
const MESSAGE_CHAR_LIMIT: usize = 600;
const REVIEW_REASON_CHAR_LIMIT: usize = 160;
const REVIEW_RESPONSE_MAX_TOKENS: u32 = 220;
const BUTLER_KIND_TASK: &str = "butler_task";

const OPERATION_REVIEW_SYSTEM_PROMPT: &str = r#"你是 AIPP 总管家的权限裁决器。你只负责判断是否自动处理当前权限请求，不能调用任何工具，也不能假设额外信息。

裁决原则：
1. 安全优先，信息不足时必须选择 manual。
2. 只在请求与当前 Butler 子任务目标高度一致、范围清晰、风险可控时自动批准。
3. 对文件系统权限，优先使用最小授权：
   - approve_once：仅本次允许。
   - approve_task：仅当前任务/当前对话后续同目录继续允许。
   - approve_assistant：仅在明显属于稳定助手工作区且后续重复使用合理时使用。
   - approve_save：仅在非常明确应加入全局白名单时使用，默认不要使用。
4. 遇到可疑路径、系统级目录、用户主目录大范围写入、与任务目标无关的操作时，优先 deny 或 manual。
5. 你必须输出单个 JSON 对象，不能输出 Markdown、解释前缀或代码块。

允许的 action：
- approve_once
- approve_task
- approve_assistant
- approve_save
- deny
- manual
"#;

const ACP_REVIEW_SYSTEM_PROMPT: &str = r#"你是 AIPP 总管家的 ACP 权限裁决器。你只负责判断是否自动处理当前 ACP 权限请求，不能调用任何工具，也不能假设额外信息。

裁决原则：
1. 安全优先，信息不足时必须选择 manual。
2. 只能在当前 Butler 子任务目标与 ACP 工具调用高度一致时自动裁决。
3. 自动批准时必须选择最小权限选项。
4. 如果所有选项都不安全或明显偏离任务目标，可以 cancel。
5. 你必须输出单个 JSON 对象，不能输出 Markdown、解释前缀或代码块。

允许的 action：
- select_option
- cancel
- manual
"#;

#[derive(Debug, Clone, Serialize)]
struct MessageSnippet {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct ButlerPermissionContext {
    conversation_id: i64,
    conversation_kind: String,
    conversation_name: String,
    source_task_title: Option<String>,
    assistant_id: Option<i64>,
    task_definition: ButlerTaskDefinitionSummary,
    recent_task_messages: Vec<MessageSnippet>,
    recent_butler_messages: Vec<MessageSnippet>,
}

#[derive(Debug, Clone, Serialize)]
struct ButlerTaskDefinitionSummary {
    butler_conversation_id: i64,
    task_title: String,
    task_goal: String,
    executor_assistant_id: i64,
    executor_assistant_source: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
enum ReviewAction {
    ApproveOnce,
    ApproveTask,
    ApproveAssistant,
    ApproveSave,
    Deny,
    SelectOption,
    Cancel,
    Manual,
}

#[derive(Debug, Clone, Deserialize)]
struct ParsedReviewResponse {
    action: ReviewAction,
    option_id: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawReviewResponse {
    action: Option<String>,
    option_id: Option<String>,
    reason: Option<String>,
}

enum OperationAutoDecision {
    Resolve(PermissionDecision),
    Manual,
}

enum AcpAutoDecision {
    Resolve { option_id: Option<String>, cancelled: bool },
    Manual,
}

pub(crate) async fn start_operation_permission_auto_review(
    app_handle: AppHandle,
    snapshot: PermissionRequestSnapshot,
) -> bool {
    if !is_auto_review_candidate(snapshot.conversation_id) {
        return false;
    }

    let request_id = snapshot.event.request_id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        run_operation_permission_auto_review(app_handle, snapshot).await
    });

    match timeout(AUTO_REVIEW_INLINE_WAIT, handle).await {
        Ok(Ok(resolved)) => resolved,
        Ok(Err(error)) => {
            warn!(request_id = %request_id, error = %error, "Operation auto review task failed");
            false
        }
        Err(_) => false,
    }
}

pub(crate) async fn start_acp_permission_auto_review(
    app_handle: AppHandle,
    snapshot: AcpPermissionRequestSnapshot,
) -> bool {
    if !is_auto_review_candidate(snapshot.conversation_id) {
        return false;
    }

    let request_id = snapshot.event.request_id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        run_acp_permission_auto_review(app_handle, snapshot).await
    });

    match timeout(AUTO_REVIEW_INLINE_WAIT, handle).await {
        Ok(Ok(resolved)) => resolved,
        Ok(Err(error)) => {
            warn!(request_id = %request_id, error = %error, "ACP auto review task failed");
            false
        }
        Err(_) => false,
    }
}

async fn run_operation_permission_auto_review(
    app_handle: AppHandle,
    snapshot: PermissionRequestSnapshot,
) -> bool {
    let request_id = snapshot.event.request_id.clone();
    let decision = match adjudicate_operation_permission(&app_handle, &snapshot).await {
        Ok(OperationAutoDecision::Resolve(decision)) => decision,
        Ok(OperationAutoDecision::Manual) => return false,
        Err(error) => {
            warn!(request_id = %request_id, error = %error, "Operation auto review failed");
            return false;
        }
    };

    let decision_value = operation_decision_value(&decision).to_string();
    match confirm_operation_permission(app_handle, request_id.clone(), decision_value).await {
        Ok(true) => {
            info!(
                request_id = %request_id,
                decision = ?decision,
                "Butler auto adjudication resolved operation permission"
            );
            true
        }
        Ok(false) => false,
        Err(error) => {
            if is_benign_resolution_race(&error) {
                debug!(request_id = %request_id, error = %error, "Operation permission already resolved");
            } else {
                warn!(request_id = %request_id, error = %error, "Failed to apply operation auto decision");
            }
            false
        }
    }
}

async fn run_acp_permission_auto_review(
    app_handle: AppHandle,
    snapshot: AcpPermissionRequestSnapshot,
) -> bool {
    let request_id = snapshot.event.request_id.clone();
    let decision = match adjudicate_acp_permission(&app_handle, &snapshot).await {
        Ok(AcpAutoDecision::Resolve { option_id, cancelled }) => (option_id, cancelled),
        Ok(AcpAutoDecision::Manual) => return false,
        Err(error) => {
            warn!(request_id = %request_id, error = %error, "ACP auto review failed");
            return false;
        }
    };

    match confirm_acp_permission(app_handle, request_id.clone(), decision.0, Some(decision.1)).await {
        Ok(true) => {
            info!(
                request_id = %request_id,
                cancelled = decision.1,
                "Butler auto adjudication resolved ACP permission"
            );
            true
        }
        Ok(false) => false,
        Err(error) => {
            if is_benign_resolution_race(&error) {
                debug!(request_id = %request_id, error = %error, "ACP permission already resolved");
            } else {
                warn!(request_id = %request_id, error = %error, "Failed to apply ACP auto decision");
            }
            false
        }
    }
}

async fn adjudicate_operation_permission(
    app_handle: &AppHandle,
    snapshot: &PermissionRequestSnapshot,
) -> Result<OperationAutoDecision, String> {
    let Some(conversation_id) = snapshot.conversation_id else {
        return Ok(OperationAutoDecision::Manual);
    };
    let Some(context) = collect_butler_permission_context(app_handle, conversation_id).await? else {
        return Ok(OperationAutoDecision::Manual);
    };

    let response = run_review_completion(
        app_handle,
        OPERATION_REVIEW_SYSTEM_PROMPT,
        &build_operation_review_prompt(snapshot, &context)?,
    )
    .await?;
    let parsed = parse_review_response(&response)?;

    Ok(match parsed.action {
        ReviewAction::ApproveOnce => OperationAutoDecision::Resolve(PermissionDecision::Allow),
        ReviewAction::ApproveTask => {
            OperationAutoDecision::Resolve(PermissionDecision::AllowForConversation)
        }
        ReviewAction::ApproveAssistant => {
            OperationAutoDecision::Resolve(PermissionDecision::AllowForAssistant)
        }
        ReviewAction::ApproveSave => OperationAutoDecision::Resolve(PermissionDecision::AllowAndSave),
        ReviewAction::Deny => OperationAutoDecision::Resolve(PermissionDecision::Deny),
        ReviewAction::Manual | ReviewAction::SelectOption | ReviewAction::Cancel => {
            debug!(
                request_id = %snapshot.event.request_id,
                reason = parsed.reason.as_deref().unwrap_or(""),
                "Butler kept operation permission in manual review"
            );
            OperationAutoDecision::Manual
        }
    })
}

async fn adjudicate_acp_permission(
    app_handle: &AppHandle,
    snapshot: &AcpPermissionRequestSnapshot,
) -> Result<AcpAutoDecision, String> {
    let Some(conversation_id) = snapshot.conversation_id else {
        return Ok(AcpAutoDecision::Manual);
    };
    let Some(context) = collect_butler_permission_context(app_handle, conversation_id).await? else {
        return Ok(AcpAutoDecision::Manual);
    };

    let response = run_review_completion(
        app_handle,
        ACP_REVIEW_SYSTEM_PROMPT,
        &build_acp_review_prompt(snapshot, &context)?,
    )
    .await?;
    let parsed = parse_review_response(&response)?;

    Ok(match parsed.action {
        ReviewAction::SelectOption => {
            let Some(option_id) = parsed.option_id.clone() else {
                debug!(request_id = %snapshot.event.request_id, "ACP auto review omitted option_id");
                return Ok(AcpAutoDecision::Manual);
            };
            if snapshot
                .event
                .options
                .iter()
                .any(|option| option.option_id == option_id)
            {
                AcpAutoDecision::Resolve { option_id: Some(option_id), cancelled: false }
            } else {
                debug!(
                    request_id = %snapshot.event.request_id,
                    option_id = %option_id,
                    "ACP auto review selected unknown option, falling back to manual"
                );
                AcpAutoDecision::Manual
            }
        }
        ReviewAction::Cancel | ReviewAction::Deny => {
            AcpAutoDecision::Resolve { option_id: None, cancelled: true }
        }
        ReviewAction::Manual
        | ReviewAction::ApproveOnce
        | ReviewAction::ApproveTask
        | ReviewAction::ApproveAssistant
        | ReviewAction::ApproveSave => {
            debug!(
                request_id = %snapshot.event.request_id,
                reason = parsed.reason.as_deref().unwrap_or(""),
                "Butler kept ACP permission in manual review"
            );
            AcpAutoDecision::Manual
        }
    })
}

async fn run_review_completion(
    app_handle: &AppHandle,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String, String> {
    let feature_state = app_handle.state::<crate::FeatureConfigState>();
    let config_feature_map = feature_state.config_feature_map.lock().await.clone();
    let model_selection = get_butler_model_selection(app_handle).await?;
    let llm_db = LLMDatabase::new(app_handle).map_err(|error| error.to_string())?;
    let model_detail = llm_db
        .get_llm_model_detail(&model_selection.provider_id, &model_selection.model_code)
        .map_err(|error| format!("无法读取总管家模型配置: {error}"))?;

    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let request_timeout = get_request_timeout_from_config(&config_feature_map);
    let client = genai_client::create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        network_proxy.is_some(),
        Some(request_timeout),
        false,
        &config_feature_map,
    )
    .map_err(|error| error.to_string())?;

    let messages = vec![
        ("system".to_string(), system_prompt.to_string(), Vec::new()),
        ("user".to_string(), user_prompt.to_string(), Vec::new()),
    ];
    let chat_request = build_chat_request_from_messages(&messages, ToolCallStrategy::NonNative, None)
        .chat_request;
    let chat_options = ChatOptions::default()
        .with_temperature(0.0)
        .with_max_tokens(REVIEW_RESPONSE_MAX_TOKENS);
    let response = client
        .exec_chat(&model_detail.model.code, chat_request, Some(&chat_options))
        .await
        .map_err(|error| error.to_string())?;

    Ok(response.first_text().unwrap_or("").trim().to_string())
}

async fn collect_butler_permission_context(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<Option<ButlerPermissionContext>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|error| error.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|error| error.to_string())?;
    let message_repo = db.message_repo().map_err(|error| error.to_string())?;
    let butler_repo = db.butler_repo().map_err(|error| error.to_string())?;

    let Some(conversation) = conversation_repo
        .read(conversation_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Ok(None);
    }

    let Some(task_definition) = butler_repo
        .get_task_definition_by_task_conversation_id(conversation_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    let recent_task_messages =
        collect_recent_message_snippets(&message_repo.list_by_conversation_id(conversation_id).map_err(|error| error.to_string())?, TASK_CONTEXT_MESSAGE_LIMIT);

    let recent_butler_messages = if let Some(parent_id) = conversation.parent_butler_conversation_id {
        collect_recent_message_snippets(
            &message_repo.list_by_conversation_id(parent_id).map_err(|error| error.to_string())?,
            BUTLER_CONTEXT_MESSAGE_LIMIT,
        )
    } else {
        Vec::new()
    };

    Ok(Some(ButlerPermissionContext {
        conversation_id,
        conversation_kind: conversation.conversation_kind.clone(),
        conversation_name: conversation.name.clone(),
        source_task_title: conversation.source_task_title.clone(),
        assistant_id: conversation.assistant_id,
        task_definition: ButlerTaskDefinitionSummary {
            butler_conversation_id: task_definition.butler_conversation_id,
            task_title: task_definition.title.clone(),
            task_goal: task_definition.goal.clone(),
            executor_assistant_id: task_definition.executor_assistant_id,
            executor_assistant_source: task_definition.executor_assistant_source.clone(),
        },
        recent_task_messages,
        recent_butler_messages,
    }))
}

fn collect_recent_message_snippets(
    messages: &[(Message, Option<crate::db::conversation_db::MessageAttachment>)],
    limit: usize,
) -> Vec<MessageSnippet> {
    let mut seen = HashSet::new();
    let mut collected = Vec::new();
    for (message, _) in messages {
        if !seen.insert(message.id) {
            continue;
        }
        let Some(role) = map_message_role(message) else {
            continue;
        };
        let content = truncate_text(&message.content, MESSAGE_CHAR_LIMIT);
        if content.is_empty() {
            continue;
        }
        collected.push(MessageSnippet { role: role.to_string(), content });
    }
    let keep_from = collected.len().saturating_sub(limit);
    collected.into_iter().skip(keep_from).collect()
}

fn map_message_role(message: &Message) -> Option<&'static str> {
    match message.message_type.as_str() {
        "user" => Some("user"),
        "response" => Some("assistant"),
        "system" => Some("system"),
        _ => None,
    }
}

fn build_operation_review_prompt(
    snapshot: &PermissionRequestSnapshot,
    context: &ButlerPermissionContext,
) -> Result<String, String> {
    let payload = serde_json::json!({
        "request_type": "operation_permission",
        "request_id": snapshot.event.request_id,
        "review_code": snapshot.review_code,
        "operation": snapshot.event.operation,
        "path": snapshot.event.path,
        "conversation_context": context,
        "response_schema": {
            "action": "approve_once | approve_task | approve_assistant | approve_save | deny | manual",
            "reason": format!("不超过 {} 个字符，可选", REVIEW_REASON_CHAR_LIMIT)
        }
    });
    serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())
}

fn build_acp_review_prompt(
    snapshot: &AcpPermissionRequestSnapshot,
    context: &ButlerPermissionContext,
) -> Result<String, String> {
    let options: Vec<_> = snapshot
        .event
        .options
        .iter()
        .map(|option| {
            serde_json::json!({
                "option_id": option.option_id,
                "name": option.name,
                "kind": option.kind,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "request_type": "acp_permission",
        "request_id": snapshot.event.request_id,
        "review_code": snapshot.review_code,
        "tool_call_id": snapshot.event.tool_call_id,
        "title": snapshot.event.title,
        "kind": snapshot.event.kind,
        "parameters": truncate_text(snapshot.event.parameters.as_deref().unwrap_or(""), 1200),
        "options": options,
        "conversation_context": context,
        "response_schema": {
            "action": "select_option | cancel | manual",
            "option_id": "当 action=select_option 时必填",
            "reason": format!("不超过 {} 个字符，可选", REVIEW_REASON_CHAR_LIMIT)
        }
    });
    serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())
}

fn parse_review_response(raw: &str) -> Result<ParsedReviewResponse, String> {
    let json_body = extract_json_body(raw).ok_or_else(|| "权限裁决响应不是 JSON".to_string())?;
    let payload: RawReviewResponse =
        serde_json::from_str(json_body).map_err(|error| format!("解析权限裁决响应失败: {error}"))?;
    let action = parse_review_action(payload.action.as_deref().unwrap_or_default())
        .ok_or_else(|| "未知的权限裁决 action".to_string())?;
    let option_id = payload.option_id.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let reason = payload
        .reason
        .map(|value| truncate_text(&value, REVIEW_REASON_CHAR_LIMIT))
        .filter(|value| !value.is_empty());
    Ok(ParsedReviewResponse { action, option_id, reason })
}

fn extract_json_body(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    let fenced = trimmed.strip_prefix("```json").or_else(|| trimmed.strip_prefix("```"))?;
    let fenced = fenced.trim();
    let fenced = fenced.strip_suffix("```")?.trim();
    if fenced.starts_with('{') && fenced.ends_with('}') {
        Some(fenced)
    } else {
        None
    }
}

fn parse_review_action(raw: &str) -> Option<ReviewAction> {
    let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "approve_once" | "allow" => Some(ReviewAction::ApproveOnce),
        "approve_task" | "approve_conversation" | "allow_for_conversation" => {
            Some(ReviewAction::ApproveTask)
        }
        "approve_assistant" | "allow_for_assistant" => Some(ReviewAction::ApproveAssistant),
        "approve_save" | "allow_and_save" => Some(ReviewAction::ApproveSave),
        "deny" => Some(ReviewAction::Deny),
        "select_option" | "select" => Some(ReviewAction::SelectOption),
        "cancel" | "cancelled" => Some(ReviewAction::Cancel),
        "manual" | "manual_review" | "needs_human" => Some(ReviewAction::Manual),
        _ => None,
    }
}

fn operation_decision_value(decision: &PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow => "allow",
        PermissionDecision::AllowForConversation => "allow_for_conversation",
        PermissionDecision::AllowForAssistant => "allow_for_assistant",
        PermissionDecision::AllowAndSave => "allow_and_save",
        PermissionDecision::Deny => "deny",
    }
}

fn is_auto_review_candidate(conversation_id: Option<i64>) -> bool {
    conversation_id.is_some()
}

fn is_benign_resolution_race(error: &str) -> bool {
    error.contains("not found or already resolved")
        || error.contains("receiver dropped before resolution")
        || error.contains("cancelled")
}

fn truncate_text(input: &str, limit: usize) -> String {
    let trimmed = input.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(limit).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_review_response_accepts_plain_json() {
        let parsed = parse_review_response(
            r#"{"action":"approve_task","reason":"当前任务需要继续操作同目录"}"#,
        )
        .unwrap();
        assert_eq!(parsed.action, ReviewAction::ApproveTask);
        assert_eq!(parsed.reason.as_deref(), Some("当前任务需要继续操作同目录"));
    }

    #[test]
    fn parse_review_response_accepts_fenced_json() {
        let parsed = parse_review_response(
            "```json\n{\"action\":\"select_option\",\"option_id\":\"allow_once\"}\n```",
        )
        .unwrap();
        assert_eq!(parsed.action, ReviewAction::SelectOption);
        assert_eq!(parsed.option_id.as_deref(), Some("allow_once"));
    }

    #[test]
    fn parse_review_action_supports_aliases() {
        assert_eq!(parse_review_action("allow"), Some(ReviewAction::ApproveOnce));
        assert_eq!(
            parse_review_action("allow_for_conversation"),
            Some(ReviewAction::ApproveTask)
        );
        assert_eq!(parse_review_action("needs_human"), Some(ReviewAction::Manual));
    }
}

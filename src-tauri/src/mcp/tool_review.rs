//! 工具调用自动审核。
//!
//! 已开启自动运行的工具直接执行，不调用审核模型。
//! 未开启自动运行且助手 `tool_review_mode=auto_model` 时，用辅助 AI 里配置的审核模型判断安全性。
//! 安全则直接执行；有风险、超时、模型缺失或返回无法解析时保持 pending，由用户确认。

use crate::api::ai::config::get_network_proxy_from_config;
use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy};
use crate::api::assistant_api::get_assistant_field_value;
use crate::api::genai_client;
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::{MCPDatabase, NewToolReviewLog, ToolReviewLog};
use crate::db::system_db::FeatureConfig;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::Manager;
use tracing::warn;

pub const REVIEW_TIMEOUT_SECS: u64 = 20;
const PROMPT_PARAM_LIMIT: usize = 8000;

const REVIEW_SYSTEM_PROMPT: &str = r#"你是工具调用安全审核员。只根据这一次调用的实际参数和它会造成的效果，自己判断能不能自动执行。不要执行工具，也不要按工具名称或工具类型直接下结论。

请重点看这次调用是否出现下面这些情况：

- 访问敏感数据：读取、打印或外传密钥、令牌、密码、cookie、私钥、环境变量、凭据文件，或把这些内容发到对话以外的地方。
- 参数含糊：目标不明确，范围过大，使用通配符或“全部”，路径为空，相对路径可能逃出当前工作区，或者命令、地址、收件方由变量拼接，看不出实际会作用到哪里。
- 造成破坏：删除、覆盖、清空、格式化或批量改写，却没有明确、有限的范围；递归删除目录；无条件覆盖已有内容；丢掉尚未保存的修改。

这些危险命令也按同样的方式判断，看到相似效果就视为有风险，但不要因为调用了某一类工具就一律拒绝：

- 下载远程内容后直接执行，或执行编码、混淆后的命令，实际行为从参数里看不出来。
- 提权后再执行，或把权限放宽到所有人可写、可执行。
- 关闭防火墙、安全软件或执行策略，修改启动项、计划任务、注册表、系统服务。
- 结束系统关键进程，关机、重启，或用循环、炸弹耗尽资源。
- 清空磁盘、重建文件系统，或把数据直接写进磁盘设备。
- 数据库删表、清空表，或更新、删除时没有限定范围。
- 强制覆盖远端历史，或硬重置、清理会丢掉未提交的修改。
- 向外部发送消息、邮件、请求或发布内容，但内容、收件方或目标不受控。

只返回 JSON，不要 Markdown，不要额外说明：{"safe":true,"reason":"一句中文理由"} 或 {"safe":false,"reason":"一句中文理由"}"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRecordDraft {
    pub model_code: String,
    pub provider_id: Option<i64>,
    pub verdict: String,
    pub reason: String,
    pub duration_ms: i64,
    pub error_detail: Option<String>,
}

pub fn is_review_exempt_tool(tool_name: &str) -> bool {
    matches!(tool_name, "ask_user_question" | "preview_code" | "preview_file")
}

/// 已开启自动运行、未启用模型审核，或工具本身就是用户交互时，沿用原来的 auto-run 结果。
/// 只有未开启自动运行且启用模型审核时，才看审核结论；失败时 `review_safe` 为 None，保持不执行。
pub fn resolve_auto_execute(
    mode: &str,
    tool_name: &str,
    legacy_auto_run: bool,
    review_safe: Option<bool>,
) -> bool {
    if legacy_auto_run || mode != "auto_model" || is_review_exempt_tool(tool_name) {
        return legacy_auto_run;
    }
    review_safe.unwrap_or(false)
}

pub fn unconfigured_model_reason() -> String {
    "自动审核模型未配置，请在设置 → 辅助AI 中选择模型".to_string()
}

pub fn missing_model_reason(model_code: &str, provider_id: i64, source: &str) -> String {
    format!(
        "配置的自动审核模型不存在 (model_code={model_code}, provider_id={provider_id})，请检查设置: {source}"
    )
}

pub fn timeout_reason() -> String {
    format!("自动审核模型超时（{REVIEW_TIMEOUT_SECS}秒）")
}

pub fn error_outcome(
    model_code: impl Into<String>,
    provider_id: Option<i64>,
    duration_ms: i64,
    detail: impl Into<String>,
) -> ReviewRecordDraft {
    let detail = detail.into();
    ReviewRecordDraft {
        model_code: model_code.into(),
        provider_id,
        verdict: "error".to_string(),
        reason: detail.clone(),
        duration_ms,
        error_detail: Some(detail),
    }
}

pub fn outcome_from_model_text(
    text: &str,
    model_code: &str,
    provider_id: i64,
    duration_ms: i64,
) -> ReviewRecordDraft {
    match parse_review_response(text) {
        Ok((true, reason)) => ReviewRecordDraft {
            model_code: model_code.to_string(),
            provider_id: Some(provider_id),
            verdict: "safe".to_string(),
            reason,
            duration_ms,
            error_detail: None,
        },
        Ok((false, reason)) => ReviewRecordDraft {
            model_code: model_code.to_string(),
            provider_id: Some(provider_id),
            verdict: "risky".to_string(),
            reason,
            duration_ms,
            error_detail: None,
        },
        Err(detail) => error_outcome(model_code, Some(provider_id), duration_ms, detail),
    }
}

pub fn parse_review_response(text: &str) -> Result<(bool, String), String> {
    let json_text = extract_json_object(text).ok_or_else(|| {
        format!("审核模型返回无法解析: {}", preview_text(text))
    })?;
    let value: serde_json::Value = serde_json::from_str(json_text).map_err(|error| {
        format!("审核模型返回无法解析: {error}; 原文: {}", preview_text(text))
    })?;
    let safe = value.get("safe").and_then(|item| item.as_bool()).ok_or_else(|| {
        format!("审核模型返回缺少 safe 字段: {}", preview_text(text))
    })?;
    let reason = value.get("reason").and_then(|item| item.as_str()).unwrap_or("").trim();
    let reason = if reason.is_empty() {
        if safe {
            "审核模型判定为安全".to_string()
        } else {
            "审核模型判定为有风险，但未给出理由".to_string()
        }
    } else {
        reason.to_string()
    };
    Ok((safe, reason))
}

fn extract_json_object(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    let without_fence = if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest).trim_start();
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    };
    let start = without_fence.find('{')?;
    let end = without_fence.rfind('}')?;
    if end < start {
        return None;
    }
    Some(&without_fence[start..=end])
}

fn preview_text(text: &str) -> String {
    let truncated: String = text.chars().take(200).collect();
    if text.chars().count() > 200 {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn truncate_for_prompt(parameters: &str) -> String {
    let count = parameters.chars().count();
    if count <= PROMPT_PARAM_LIMIT {
        return parameters.to_string();
    }
    let truncated: String = parameters.chars().take(PROMPT_PARAM_LIMIT).collect();
    format!("{truncated}\n...[参数已截断，原长度 {count} 字符]")
}

fn read_review_mode(app_handle: &tauri::AppHandle, assistant_id: i64) -> String {
    match get_assistant_field_value(app_handle.clone(), assistant_id, "tool_review_mode") {
        Ok(value) if value == "auto_model" => "auto_model".to_string(),
        _ => "off".to_string(),
    }
}

async fn load_feature_config_map(
    app_handle: &tauri::AppHandle,
) -> HashMap<String, HashMap<String, FeatureConfig>> {
    let state = app_handle.state::<crate::FeatureConfigState>();
    let config_feature_map = state.config_feature_map.lock().await.clone();
    config_feature_map
}

fn configured_review_model(
    config_feature_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Result<(String, i64), String> {
    let summary = config_feature_map.get("conversation_summary");
    let model_code = summary
        .and_then(|config| config.get("auto_review_model"))
        .map(|config| config.value.trim().to_string())
        .unwrap_or_default();
    let provider_id = summary
        .and_then(|config| config.get("auto_review_provider_id"))
        .map(|config| config.value.trim().to_string())
        .unwrap_or_default();
    if model_code.is_empty() || provider_id.is_empty() {
        return Err(unconfigured_model_reason());
    }
    let provider_id = provider_id.parse::<i64>().map_err(|_| {
        format!("自动审核模型 provider_id 解析失败: {provider_id}")
    })?;
    Ok((model_code, provider_id))
}

async fn review_with_configured_model(
    app_handle: &tauri::AppHandle,
    server_name: &str,
    tool_name: &str,
    parameters: &str,
) -> ReviewRecordDraft {
    let started = Instant::now();
    let config_feature_map = load_feature_config_map(app_handle).await;
    let (model_code, provider_id) = match configured_review_model(&config_feature_map) {
        Ok(model) => model,
        Err(detail) => return error_outcome("", None, 0, detail),
    };

    let llm_db = match LLMDatabase::new(app_handle) {
        Ok(db) => db,
        Err(error) => {
            return error_outcome(
                &model_code,
                Some(provider_id),
                started.elapsed().as_millis() as i64,
                format!("读取自动审核模型失败: {error}"),
            );
        }
    };
    let model_detail = match llm_db.get_llm_model_detail(&provider_id, &model_code) {
        Ok(detail) => detail,
        Err(error) => {
            return error_outcome(
                &model_code,
                Some(provider_id),
                started.elapsed().as_millis() as i64,
                missing_model_reason(&model_code, provider_id, &error.to_string()),
            );
        }
    };

    let network_proxy = get_network_proxy_from_config(&config_feature_map);
    let prepared_configs = match crate::api::copilot_token_manager::prepare_provider_configs(
        app_handle,
        &model_detail.provider.api_type,
        &model_detail.configs,
        network_proxy.as_deref(),
    )
    .await
    {
        Ok(configs) => configs,
        Err(error) => {
            return error_outcome(
                &model_code,
                Some(provider_id),
                started.elapsed().as_millis() as i64,
                format!("准备自动审核模型配置失败: {error}"),
            );
        }
    };
    let client = match genai_client::create_client_with_config(
        &prepared_configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        Some(&model_detail.model.request_mode),
        network_proxy.as_deref(),
        false,
        Some(REVIEW_TIMEOUT_SECS),
        false,
        &config_feature_map,
    ) {
        Ok(client) => client,
        Err(error) => {
            return error_outcome(
                &model_code,
                Some(provider_id),
                started.elapsed().as_millis() as i64,
                format!("创建自动审核模型客户端失败: {error}"),
            );
        }
    };

    let user_prompt = format!(
        "工具: {server_name}::{tool_name}\n参数:\n{}",
        truncate_for_prompt(parameters)
    );
    let chat_request = build_chat_request_from_messages(
        &[
            ("system".to_string(), REVIEW_SYSTEM_PROMPT.to_string(), Vec::new()),
            ("user".to_string(), user_prompt, Vec::new()),
        ],
        ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;

    let elapsed = || started.elapsed().as_millis() as i64;
    match tokio::time::timeout(
        Duration::from_secs(REVIEW_TIMEOUT_SECS),
        client.exec_chat(&model_detail.model.code, chat_request, None),
    )
    .await
    {
        Ok(Ok(response)) => outcome_from_model_text(
            response.first_text().unwrap_or(""),
            &model_code,
            provider_id,
            elapsed(),
        ),
        Ok(Err(error)) => error_outcome(
            &model_code,
            Some(provider_id),
            elapsed(),
            format!("自动审核模型调用失败: {error}"),
        ),
        Err(_) => error_outcome(&model_code, Some(provider_id), elapsed(), timeout_reason()),
    }
}

fn emit_tool_review_status(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    call_id: i64,
    phase: &str,
    verdict: Option<&str>,
    reason: &str,
) {
    let event = crate::api::ai::events::ConversationEvent {
        r#type: "tool_review_update".to_string(),
        data: serde_json::json!({
            "call_id": call_id,
            "conversation_id": conversation_id,
            "phase": phase,
            "verdict": verdict,
            "reason": reason,
        }),
    };
    crate::utils::window_utils::send_conversation_event_to_chat_windows(
        app_handle,
        conversation_id,
        event,
    );
}

fn existing_safe_pending(db: &MCPDatabase, call_id: i64) -> Option<bool> {
    let existing = db.get_tool_review_by_call_id(call_id).ok()??;
    if existing.verdict != "safe" {
        return Some(false);
    }
    match db.get_mcp_tool_call(call_id) {
        Ok(call) => Some(call.status == "pending"),
        Err(_) => Some(false),
    }
}

/// 返回是否应立即执行。未开启自动运行且启用审核时会写入 `tool_review_log`。
pub async fn should_auto_execute_after_review(
    app_handle: &tauri::AppHandle,
    assistant_id: i64,
    conversation_id: i64,
    call_id: i64,
    server_name: &str,
    tool_name: &str,
    parameters: &str,
    legacy_auto_run: bool,
) -> bool {
    let mode = read_review_mode(app_handle, assistant_id);
    if legacy_auto_run || mode != "auto_model" || is_review_exempt_tool(tool_name) {
        return resolve_auto_execute(&mode, tool_name, legacy_auto_run, None);
    }

    if let Ok(db) = MCPDatabase::new(app_handle) {
        if let Some(should_run) = existing_safe_pending(&db, call_id) {
            return should_run;
        }
    }

    emit_tool_review_status(app_handle, conversation_id, call_id, "reviewing", None, "");
    let draft = review_with_configured_model(app_handle, server_name, tool_name, parameters).await;
    emit_tool_review_status(
        app_handle,
        conversation_id,
        call_id,
        "done",
        Some(draft.verdict.as_str()),
        &draft.reason,
    );
    let should_run = draft.verdict == "safe";
    match MCPDatabase::new(app_handle) {
        Ok(db) => {
            let record = NewToolReviewLog {
                conversation_id,
                mcp_tool_call_id: call_id,
                server_name: server_name.to_string(),
                tool_name: tool_name.to_string(),
                parameters: parameters.to_string(),
                model_code: draft.model_code,
                provider_id: draft.provider_id,
                verdict: draft.verdict,
                reason: draft.reason.clone(),
                duration_ms: draft.duration_ms,
                error_detail: draft.error_detail,
            };
            if let Err(error) = db.insert_tool_review_log(&record) {
                warn!(
                    call_id,
                    conversation_id,
                    error = %error,
                    reason = %draft.reason,
                    "failed to persist tool review log; holding tool call for user confirmation"
                );
                return false;
            }
        }
        Err(error) => {
            warn!(
                call_id,
                conversation_id,
                error = %error,
                reason = %draft.reason,
                "failed to open MCP database for tool review log; holding tool call for user confirmation"
            );
            return false;
        }
    }
    should_run
}

pub fn record_user_allow_if_review_held(app_handle: &tauri::AppHandle, call_id: i64) {
    let Ok(db) = MCPDatabase::new(app_handle) else {
        return;
    };
    let Ok(Some(review)) = db.get_tool_review_by_call_id(call_id) else {
        return;
    };
    if review.verdict == "safe" {
        return;
    }
    if let Err(error) = db.set_tool_review_user_decision(call_id, "allow") {
        warn!(call_id, error = %error, "failed to record tool review allow");
    }
}

#[tauri::command]
pub fn get_tool_review(
    app_handle: tauri::AppHandle,
    call_id: i64,
) -> Result<Option<ToolReviewLog>, String> {
    let db = MCPDatabase::new(&app_handle).map_err(|error| error.to_string())?;
    db.get_tool_review_by_call_id(call_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_tool_reviews(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
) -> Result<Vec<ToolReviewLog>, String> {
    let db = MCPDatabase::new(&app_handle).map_err(|error| error.to_string())?;
    db.list_tool_reviews_by_conversation(conversation_id)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn memory_db() -> MCPDatabase {
        let conn = Connection::open_in_memory().expect("open memory db");
        let db = MCPDatabase { conn };
        db.create_tables().expect("create mcp tables");
        db
    }

    #[test]
    fn test_parse_review_response_accepts_safe_json() {
        let (safe, reason) = parse_review_response(r#"{"safe":true,"reason":"只读查询"}"#).unwrap();
        assert!(safe);
        assert_eq!(reason, "只读查询");
    }

    #[test]
    fn test_parse_review_response_marks_unsafe_json() {
        let (safe, reason) = parse_review_response(r#"{"safe":false,"reason":"会删除文件"}"#).unwrap();
        assert!(!safe);
        assert_eq!(reason, "会删除文件");
        let outcome = outcome_from_model_text(
            r#"{"safe":false,"reason":"会删除文件"}"#,
            "review-model",
            3,
            12,
        );
        assert_eq!(outcome.verdict, "risky");
        assert!(outcome.reason.contains("会删除文件"));
    }

    #[test]
    fn test_parse_review_response_rejects_missing_safe_and_non_json() {
        let missing = parse_review_response(r#"{"reason":"没有结论"}"#).unwrap_err();
        assert!(missing.contains("缺少 safe"));
        let garbage = parse_review_response("这不是 JSON").unwrap_err();
        assert!(garbage.contains("无法解析"));
        let outcome = outcome_from_model_text("这不是 JSON", "review-model", 3, 8);
        assert_eq!(outcome.verdict, "error");
        assert!(outcome.reason.contains("无法解析"));
    }

    #[test]
    fn test_parse_review_response_reads_fenced_json() {
        let (safe, reason) = parse_review_response("```json\n{\"safe\":true,\"reason\":\"可以\"}\n```").unwrap();
        assert!(safe);
        assert_eq!(reason, "可以");
    }

    #[test]
    fn test_resolve_auto_execute_keeps_legacy_when_review_disabled() {
        assert!(resolve_auto_execute("off", "write_file", true, None));
        assert!(!resolve_auto_execute("off", "write_file", false, Some(true)));
        assert!(!resolve_auto_execute("", "bash", false, Some(true)));
        assert!(resolve_auto_execute("auto_model", "preview_code", true, Some(false)));
        assert!(!resolve_auto_execute("auto_model", "preview_file", false, Some(true)));
        assert!(resolve_auto_execute("auto_model", "ask_user_question", true, None));
    }

    #[test]
    fn test_resolve_auto_execute_skips_review_when_tool_auto_runs() {
        assert!(resolve_auto_execute("auto_model", "write_file", true, Some(false)));
        assert!(resolve_auto_execute("auto_model", "bash", true, None));
        assert!(resolve_auto_execute("auto_model", "load_mcp_tool", true, Some(false)));
    }

    #[test]
    fn test_resolve_auto_execute_requires_explicit_safe_verdict_when_not_auto_run() {
        assert!(resolve_auto_execute("auto_model", "write_file", false, Some(true)));
        assert!(!resolve_auto_execute("auto_model", "write_file", false, Some(false)));
        assert!(!resolve_auto_execute("auto_model", "bash", false, None));
    }

    #[test]
    fn test_review_failure_reasons_stay_visible() {
        let missing_config = error_outcome("", None, 0, unconfigured_model_reason());
        assert_eq!(missing_config.verdict, "error");
        assert!(missing_config.reason.contains("自动审核模型未配置"));
        assert_eq!(missing_config.error_detail.as_deref(), Some(missing_config.reason.as_str()));

        let missing_model = error_outcome(
            "gone",
            Some(9),
            4,
            missing_model_reason("gone", 9, "QueryReturnedNoRows"),
        );
        assert!(missing_model.reason.contains("配置的自动审核模型不存在"));
        assert!(missing_model.reason.contains("QueryReturnedNoRows"));
        assert!(missing_model.reason.contains("model_code=gone"));

        let timed_out = error_outcome("slow", Some(2), 20_000, timeout_reason());
        assert!(timed_out.reason.contains("超时"));
        assert!(timed_out.reason.contains("20"));
        assert!(!resolve_auto_execute("auto_model", "bash", false, None));
    }

    #[test]
    fn test_tool_review_log_roundtrip_and_user_deny() {
        let db = memory_db();
        let inserted = db
            .insert_tool_review_log(&NewToolReviewLog {
                conversation_id: 11,
                mcp_tool_call_id: 42,
                server_name: "ops".to_string(),
                tool_name: "bash".to_string(),
                parameters: "{\"cmd\":\"rm\"}".to_string(),
                model_code: "review-model".to_string(),
                provider_id: Some(3),
                verdict: "risky".to_string(),
                reason: "会删除文件".to_string(),
                duration_ms: 80,
                error_detail: None,
            })
            .unwrap();

        let by_call = db.get_tool_review_by_call_id(42).unwrap().unwrap();
        assert_eq!(by_call.id, inserted.id);
        assert_eq!(by_call.reason, "会删除文件");
        assert!(by_call.user_decision.is_none());

        let listed = db.list_tool_reviews_by_conversation(11).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].mcp_tool_call_id, 42);

        db.set_tool_review_user_decision(42, "deny").unwrap();
        let denied = db.get_tool_review_by_call_id(42).unwrap().unwrap();
        assert_eq!(denied.user_decision.as_deref(), Some("deny"));
        assert!(db.list_tool_reviews_by_conversation(99).unwrap().is_empty());
    }
}

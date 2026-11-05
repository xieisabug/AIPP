//! 总管家进度摘要。
//!
//! 管家会话中的工具调用开始/结束时，用总管家设置里的「小型辅助模型」把
//! "用户请求 + 最近几步" 压缩成一句进度，并以 `butler_progress_summary` 事件广播。
//! 只用于展示：不写入会话历史，也不进入模型上下文。未配置辅助模型时什么也不做，
//! 前端保持原来的「正在处理…」。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{sleep, Duration};
use tracing::warn;

use crate::api::ai::config::{get_network_proxy_from_config, get_request_timeout_from_config};
use crate::api::genai_client;
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::{MCPDatabase, MCPToolCall};
use crate::mcp::builtin_mcp::templates::is_butler_conversation_kind;
use crate::state::activity_state::ConversationActivityManager;

pub const HELPER_MODEL_KEY: &str = "butler_helper_model_id";
pub const PROGRESS_EVENT: &str = "butler_progress_summary";
/// 同一会话两次摘要之间的最小间隔；间隔内的新进展合并到下一次。
const MIN_INTERVAL: Duration = Duration::from_secs(6);
const MAX_STEPS: usize = 6;
const MAX_SUMMARY_CHARS: usize = 40;

const SYSTEM_PROMPT: &str = "你是进度播报助手。根据用户的请求和助手最近执行的步骤，\
用一句不超过 25 个汉字的中文描述助手当前进展，例如「已找到三家供应商，正在对比报价」。\
只输出这一句话，不要引号、标点结尾、解释或 Markdown。";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProgressStep {
    pub label: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ButlerProgressSummary {
    pub conversation_id: i64,
    /// 生成时的运行轮次；前端/查询据此丢弃上一轮遗留的摘要。
    pub epoch: u64,
    pub text: String,
    pub steps: Vec<ProgressStep>,
}

/// conversation_id -> 是否有未处理的新进展（存在即表示该会话已有摘要 worker 在运行）。
fn workers() -> &'static Mutex<HashMap<i64, bool>> {
    static WORKERS: OnceLock<Mutex<HashMap<i64, bool>>> = OnceLock::new();
    WORKERS.get_or_init(Default::default)
}

fn cache() -> &'static Mutex<HashMap<i64, ButlerProgressSummary>> {
    static CACHE: OnceLock<Mutex<HashMap<i64, ButlerProgressSummary>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// 工具调用状态变化时调用。每个会话最多一个 worker 串行生成，天然保证新结果不会被旧结果覆盖。
pub fn notify_tool_progress(app: &AppHandle, conversation_id: i64) {
    {
        let mut workers = workers().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dirty) = workers.get_mut(&conversation_id) {
            *dirty = true;
            return;
        }
        workers.insert(conversation_id, true);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let run = {
                let mut workers = workers().lock().unwrap_or_else(|e| e.into_inner());
                match workers.get_mut(&conversation_id) {
                    Some(dirty) if *dirty => {
                        *dirty = false;
                        true
                    }
                    _ => {
                        workers.remove(&conversation_id);
                        false
                    }
                }
            };
            if !run {
                break;
            }
            match summarize_once(&app, conversation_id).await {
                Ok(true) => sleep(MIN_INTERVAL).await,
                Ok(false) => {
                    // 未配置模型或非管家会话：直接结束，后续触发会重新判断。
                    workers().lock().unwrap_or_else(|e| e.into_inner()).remove(&conversation_id);
                    break;
                }
                Err(error) => {
                    warn!(conversation_id, %error, "butler progress summary failed");
                    sleep(MIN_INTERVAL).await;
                }
            }
        }
    });
}

#[tauri::command]
pub async fn get_butler_progress_summary(
    app_handle: AppHandle,
    conversation_id: i64,
) -> Option<ButlerProgressSummary> {
    let cached = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&conversation_id).cloned()?;
    let runtime = app_handle
        .try_state::<ConversationActivityManager>()?
        .get_runtime_state(conversation_id)
        .await;
    (runtime.epoch == cached.epoch).then_some(cached)
}

fn parse_model(value: &str) -> Option<(String, i64)> {
    let (code, provider) = value.trim().split_once("%%")?;
    let provider = provider.trim().parse().ok()?;
    (!code.trim().is_empty()).then(|| (code.trim().to_string(), provider))
}

fn clip(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    format!("{}…", text.chars().take(max).collect::<String>())
}

/// 规则步骤：工具名 + 最能代表意图的一个参数。
pub(crate) fn step_label(call: &MCPToolCall) -> String {
    const PREFERRED: [&str; 10] =
        ["query", "keyword", "path", "file_path", "url", "command", "title", "name", "task", "prompt"];
    let params = serde_json::from_str::<Value>(&call.parameters).ok();
    let object = params.as_ref().and_then(Value::as_object);
    let arg = object.and_then(|map| {
        PREFERRED
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .or_else(|| map.values().find_map(Value::as_str))
    });
    match arg.map(|value| clip(value, 30)).filter(|value| !value.is_empty()) {
        Some(arg) => format!("{} {}", call.tool_name, arg),
        None => call.tool_name.clone(),
    }
}

/// 只保留当前这一轮（最后一条用户消息之后）的调用，按时间正序取最近几步。
pub(crate) fn recent_steps(calls: &[MCPToolCall], after_message_id: i64) -> Vec<ProgressStep> {
    let mut steps: Vec<ProgressStep> = calls
        .iter()
        .filter(|call| call.message_id.is_some_and(|id| id > after_message_id))
        .take(MAX_STEPS)
        .map(|call| ProgressStep { label: step_label(call), status: call.status.clone() })
        .collect();
    steps.reverse();
    steps
}

pub(crate) fn clean_summary(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let trimmed = line.trim_matches(|c: char| "\"'「」“”`。.".contains(c)).trim();
    clip(trimmed, MAX_SUMMARY_CHARS)
}

/// 返回 Ok(false) 表示不适用（未配置模型 / 非管家会话 / 本轮没有步骤）。
async fn summarize_once(app: &AppHandle, conversation_id: i64) -> Result<bool, String> {
    let Some((model_code, provider_id)) =
        super::butler_api::get_experimental_config_value(app, HELPER_MODEL_KEY)
            .await
            .as_deref()
            .and_then(parse_model)
    else {
        return Ok(false);
    };

    let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
    let conversation = db
        .conversation_repo()
        .map_err(|e| e.to_string())?
        .read(conversation_id)
        .map_err(|e| e.to_string())?;
    if !conversation.is_some_and(|c| is_butler_conversation_kind(&c.conversation_kind)) {
        return Ok(false);
    }
    let messages = db
        .message_repo()
        .map_err(|e| e.to_string())?
        .list_by_conversation_id(conversation_id)
        .map_err(|e| e.to_string())?;
    let Some(request) = messages.iter().rev().map(|(m, _)| m).find(|m| m.message_type == "user") else {
        return Ok(false);
    };
    let calls = MCPDatabase::new(app)
        .map_err(|e| e.to_string())?
        .get_mcp_tool_calls_by_conversation(conversation_id)
        .map_err(|e| e.to_string())?;
    let steps = recent_steps(&calls, request.id);
    if steps.is_empty() {
        return Ok(false);
    }
    let epoch = match app.try_state::<ConversationActivityManager>() {
        Some(manager) => manager.get_runtime_state(conversation_id).await.epoch,
        None => 0,
    };

    let step_lines = steps
        .iter()
        .enumerate()
        .map(|(i, step)| format!("{}. [{}] {}", i + 1, step.status, step.label))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!("用户请求：{}\n\n最近步骤：\n{}", clip(&request.content, 400), step_lines);

    let config_map = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let detail = LLMDatabase::new(app)
        .map_err(|e| e.to_string())?
        .get_llm_model_detail(&provider_id, &model_code)
        .map_err(|e| format!("辅助模型不可用: {e}"))?;
    let proxy = get_network_proxy_from_config(&config_map);
    let configs = crate::api::copilot_token_manager::prepare_provider_configs(
        app,
        &detail.provider.api_type,
        &detail.configs,
        proxy.as_deref(),
    )
    .await?;
    // 部分供应商（如 OpenCode Go）按会话建立网关 session，必须带上会话标识。
    let client = genai_client::create_client_with_config(
        Some((app, conversation_id)),
        &configs,
        &detail.model.code,
        &detail.provider.api_type,
        Some(&detail.model.request_mode),
        proxy.as_deref(),
        false,
        Some(get_request_timeout_from_config(&config_map)),
        false,
        &config_map,
    )
    .map_err(|e| e.to_string())?;
    let request = crate::api::ai::conversation::build_chat_request_from_messages(
        &[("system".to_string(), SYSTEM_PROMPT.to_string(), Vec::new()), ("user".to_string(), prompt, Vec::new())],
        crate::api::ai::conversation::ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;
    // 进度只在当下有意义：失败不重试，等下一次进展。
    let response = client.exec_chat(&detail.model.code, request, None).await.map_err(|e| e.to_string())?;
    let text = clean_summary(response.first_text().unwrap_or(""));
    if text.is_empty() {
        warn!(conversation_id, model = %detail.model.code, "butler progress summary model returned empty text");
        return Ok(true);
    }

    let summary = ButlerProgressSummary { conversation_id, epoch, text, steps };
    cache().lock().unwrap_or_else(|e| e.into_inner()).insert(conversation_id, summary.clone());
    let _ = app.emit(PROGRESS_EVENT, &summary);
    Ok(true)
}

#[cfg(test)]
#[path = "butler_progress_tests.rs"]
mod tests;

//! 无头（headless）无人值守 Agentic Loop：供定时任务与管家主动整理共用。
//!
//! 每轮支持 Native / NonNative 工具调用，工具调用记录完整落库（`message` + `mcp_tool_call`），
//! 每个工具调用前必须通过调用方提供的 `UnattendedToolGate` 准入，绝不挂起等待人工审批。
use chrono::Utc;
use genai::chat::{ChatOptions, ChatResponse, ToolCall};
use std::fmt;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy, ToolConfig};
use crate::api::ai_api::{build_tools_with_mapping, resolve_tool_name, ToolNameMapping};
use crate::db::conversation_db::{ConversationDatabase, MessageAttachment, Repository as ConversationRepository};
use crate::db::mcp_db::MCPDatabase;
use crate::mcp::MCPInfoForAssistant;
use crate::scheduler::gate::Admission;

#[derive(Debug, Clone)]
pub(crate) enum AgenticLoopStatus {
    Completed { message_id: i64 },
    Cancelled,
    MaxRoundsReached,
    Timeout,
    Error(String),
}

impl fmt::Display for AgenticLoopStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgenticLoopStatus::Completed { .. } => write!(f, "completed"),
            AgenticLoopStatus::Cancelled => write!(f, "cancelled"),
            AgenticLoopStatus::MaxRoundsReached => write!(f, "max_rounds_reached"),
            AgenticLoopStatus::Timeout => write!(f, "timeout"),
            AgenticLoopStatus::Error(e) => write!(f, "error: {}", e),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgenticLoopResult {
    pub(crate) final_text: String,
    pub(crate) rounds: usize,
    pub(crate) tool_calls_total: usize,
    pub(crate) tool_calls_success: usize,
    pub(crate) tool_calls_failed: usize,
    pub(crate) status: AgenticLoopStatus,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopLimits {
    pub(crate) max_rounds: usize,
    pub(crate) per_tool_timeout_secs: u64,
    pub(crate) total_timeout_secs: u64,
    pub(crate) retry_delays_secs: &'static [u64],
}

#[async_trait::async_trait]
pub(crate) trait UnattendedToolGate: Send + Sync {
    async fn admit(&self, call_db_id: i64, progress: &str) -> Admission;
}

pub(crate) trait UnattendedLoopObserver: Send + Sync {
    fn on_log(&self, _kind: &str, _content: String) {}
    fn on_response(&self, _response: &ChatResponse) {}
}

pub(crate) struct UnattendedLoopContext<'a> {
    pub(crate) app_handle: &'a tauri::AppHandle,
    pub(crate) client: &'a crate::api::genai_client::Client,
    pub(crate) model_name: &'a str,
    pub(crate) chat_options: &'a ChatOptions,
    pub(crate) conversation_id: i64,
    pub(crate) conversation_db: &'a ConversationDatabase,
    pub(crate) tool_name_mapping: &'a ToolNameMapping,
    pub(crate) tool_config: Option<ToolConfig>,
    pub(crate) tool_call_strategy: ToolCallStrategy,
    pub(crate) llm_model_id: i64,
    pub(crate) llm_model_code: String,
    pub(crate) run_label: &'a str,
    pub(crate) limits: LoopLimits,
    pub(crate) cancel_token: CancellationToken,
    pub(crate) gate: &'a dyn UnattendedToolGate,
    pub(crate) observer: &'a dyn UnattendedLoopObserver,
}

/// 将 LLM ToolCall 的 arguments 归一化为 JSON 对象字符串
pub(crate) fn normalize_tool_arguments(arguments: &serde_json::Value) -> String {
    if arguments.is_object() {
        arguments.to_string()
    } else {
        "{}".to_string()
    }
}

/// 从 prompt 模式的 `<mcp_tool_call>` 标签中提取工具调用。
pub(crate) fn extract_prompt_tool_calls(content: &str) -> (Vec<ToolCall>, String) {
    let mcp_regex = regex::Regex::new(
        r"<mcp_tool_call>\s*<server_name>([^<]*)</server_name>\s*<tool_name>([^<]*)</tool_name>\s*<parameters>([\s\S]*?)</parameters>\s*</mcp_tool_call>",
    )
    .expect("valid mcp tool call regex");

    let mut tool_calls = Vec::new();
    for cap in mcp_regex.captures_iter(content) {
        let Some(server_name) = cap.get(1).map(|m| m.as_str().trim()).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(tool_name) = cap.get(2).map(|m| m.as_str().trim()).filter(|s| !s.is_empty())
        else {
            continue;
        };
        let parameters_raw = cap.get(3).map(|m| m.as_str().trim()).unwrap_or("{}");
        let fn_arguments = serde_json::from_str::<serde_json::Value>(parameters_raw)
            .ok()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({}));

        tool_calls.push(ToolCall {
            call_id: Uuid::new_v4().to_string(),
            fn_name: crate::api::ai_api::build_tool_name(server_name, tool_name),
            fn_arguments,
            thought_signatures: None,
        });
    }

    if tool_calls.is_empty() {
        (tool_calls, content.to_string())
    } else {
        let sanitized_content = mcp_regex.replace_all(content, "").to_string();
        (tool_calls, sanitized_content)
    }
}

pub(crate) fn build_mcp_tool_call_ui_hint(
    server_name: &str,
    tool_name: &str,
    parameters: &str,
    call_db_id: Option<i64>,
    llm_call_id: &str,
) -> String {
    let payload = match call_db_id {
        Some(call_id) => serde_json::json!({
            "server_name": server_name,
            "tool_name": tool_name,
            "parameters": parameters,
            "call_id": call_id,
            "llm_call_id": llm_call_id,
        }),
        None => serde_json::json!({
            "server_name": server_name,
            "tool_name": tool_name,
            "parameters": parameters,
            "llm_call_id": llm_call_id,
        }),
    };
    format!("\n\n<!-- MCP_TOOL_CALL:{} -->\n", payload)
}

pub(crate) fn build_tool_config_for_mcp(mcp_info: &MCPInfoForAssistant, enable_tools: bool) -> Option<ToolConfig> {
    use crate::mcp::builtin_mcp::templates::is_dynamic_loading_fixed_tool;
    if !enable_tools {
        return None;
    }
    let servers_for_injection = if mcp_info.dynamic_loading_enabled {
        mcp_info
            .enabled_servers
            .iter()
            .filter_map(|server| {
                let tools: Vec<_> = server
                    .tools
                    .iter()
                    .filter(|tool| is_dynamic_loading_fixed_tool(server.command.as_deref(), &tool.name, true))
                    .cloned()
                    .collect();
                (!tools.is_empty()).then(|| {
                    let mut server = server.clone();
                    server.tools = tools;
                    server
                })
            })
            .collect()
    } else {
        mcp_info.enabled_servers.clone()
    };
    let (tools, tool_name_mapping) = build_tools_with_mapping(&servers_for_injection);
    Some(ToolConfig { tools, tool_name_mapping })
}

#[derive(Debug, Clone)]
struct PreparedAgenticToolCall {
    llm_call_id: String,
    call_db_id: Option<i64>,
    call_record_error: Option<String>,
    server_name: String,
    tool_name: String,
    params_str: String,
    params_json: serde_json::Value,
}

async fn create_tool_call_record(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    response_message_id: Option<i64>,
    server_name: &str,
    tool_name: &str,
    parameters: &str,
    llm_call_id: &str,
) -> Result<i64, String> {
    crate::mcp::execution_api::create_mcp_tool_call_with_llm_id(
        app_handle.clone(),
        conversation_id,
        response_message_id,
        server_name.to_string(),
        tool_name.to_string(),
        parameters.to_string(),
        Some(llm_call_id),
        response_message_id,
    )
    .await
    .map(|record| record.id)
}

fn fail_call(mcp_db: &MCPDatabase, call_db_id: i64, error: &str) {
    let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "failed", None, Some(error));
}

async fn execute_single_tool(
    app_handle: &tauri::AppHandle,
    call_db_id: i64,
    timeout: Duration,
    cancel_token: CancellationToken,
) -> (bool, String) {
    let mcp_db = match MCPDatabase::new(app_handle) {
        Ok(db) => db,
        Err(e) => return (false, format!("Error: 初始化 MCP 数据库失败: {}", e)),
    };
    let mut record = match mcp_db.get_mcp_tool_call(call_db_id) {
        Ok(record) => record,
        Err(e) => return (false, format!("Error: 获取工具调用记录失败: {}", e)),
    };
    match mcp_db.mark_mcp_tool_call_executing_if_pending(call_db_id) {
        Ok(true) => {}
        Ok(false) => {
            if record.status == "success" {
                return (true, record.result.unwrap_or_default());
            }
            if record.status == "failed" {
                let err = record.error.unwrap_or_else(|| "工具执行失败".to_string());
                return (false, format!("Error: {}", err));
            }
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "executing", None, None);
        }
        Err(e) => {
            let err = format!("Error: 更新工具状态失败: {}", e);
            fail_call(&mcp_db, call_db_id, &err);
            return (false, err);
        }
    }
    if let Ok(latest_record) = mcp_db.get_mcp_tool_call(call_db_id) {
        record = latest_record;
    }
    let server = match mcp_db.get_mcp_server(record.server_id) {
        Ok(s) => s,
        Err(e) => {
            let err = format!("Error: 获取 MCP 服务器失败: {}", e);
            fail_call(&mcp_db, call_db_id, &err);
            return (false, err);
        }
    };
    if cancel_token.is_cancelled() {
        let err = "Error: 工具执行已取消".to_string();
        fail_call(&mcp_db, call_db_id, &err);
        return (false, err);
    }
    let execution = tokio::time::timeout(
        timeout,
        crate::mcp::execution_api::execute_tool_by_transport_on_large_stack(
            app_handle,
            &server,
            &record.tool_name,
            &record.parameters,
            Some(record.conversation_id),
            Some(cancel_token.clone()),
        ),
    );
    let exec_result = tokio::select! {
        biased;
        _ = cancel_token.cancelled() => {
            let error = "工具执行已停止".to_string();
            fail_call(&mcp_db, call_db_id, &error);
            return (false, error);
        }
        result = execution => result,
    };
    match exec_result {
        Ok(Ok(result)) => {
            let _ = mcp_db.update_mcp_tool_call_status(call_db_id, "success", Some(&result), None);
            (true, result)
        }
        Ok(Err(e)) => {
            let err_str = format!("Error: {}", e);
            fail_call(&mcp_db, call_db_id, &err_str);
            (false, err_str)
        }
        Err(_) => {
            let err_str = format!("Error: 工具执行超时 ({}s)", timeout.as_secs());
            fail_call(&mcp_db, call_db_id, &err_str);
            (false, err_str)
        }
    }
}

pub(crate) enum LlmWait<T> {
    Done(T),
    Cancelled,
    Timeout,
}

/// 等待模型回复时同时监听停止信号与总时限；停止优先，保证停止后不会再按完成处理。
pub(crate) async fn await_llm_or_stop<F: std::future::Future>(
    fut: F,
    cancel_token: &CancellationToken,
    deadline: Instant,
) -> LlmWait<F::Output> {
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => LlmWait::Cancelled,
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => LlmWait::Timeout,
        out = fut => LlmWait::Done(out),
    }
}

/// 将非正常结束的循环状态映射为失败结果；Completed 返回 None。
pub(crate) fn loop_status_failure_with_limits(
    status: &AgenticLoopStatus,
    rounds: usize,
    tool_calls_success: usize,
    tool_calls_failed: usize,
    total_timeout_secs: u64,
    max_rounds: usize,
) -> Option<(&'static str, String)> {
    let executed = if tool_calls_success + tool_calls_failed > 0 {
        format!(
            "；停止前已执行工具 {} 次（成功 {}，失败 {}），已产生的外部操作无法撤回",
            tool_calls_success + tool_calls_failed,
            tool_calls_success,
            tool_calls_failed
        )
    } else {
        String::new()
    };
    match status {
        AgenticLoopStatus::Completed { .. } => None,
        AgenticLoopStatus::Cancelled => Some(("cancelled", format!("任务已手动停止（已进行 {rounds} 轮）{executed}"))),
        AgenticLoopStatus::Timeout => Some((
            "timeout",
            format!("执行超时：超过总时限 {total_timeout_secs}s，任务未完成（已进行 {rounds} 轮）{executed}"),
        )),
        AgenticLoopStatus::MaxRoundsReached => Some((
            "max_rounds_reached",
            format!("达到最大执行轮次 {max_rounds}，任务未完成{executed}"),
        )),
        AgenticLoopStatus::Error(e) => Some(("error", format!("{e}（已进行 {rounds} 轮）{executed}"))),
    }
}

async fn exec_chat_with_retry(
    ctx: &UnattendedLoopContext<'_>,
    messages: &[(String, String, Vec<MessageAttachment>)],
    round: usize,
    deadline: Instant,
) -> Result<ChatResponse, AgenticLoopStatus> {
    let mut attempt = 0usize;
    loop {
        let request = build_chat_request_from_messages(messages, ctx.tool_call_strategy, ctx.tool_config.clone()).chat_request;
        let result = match await_llm_or_stop(
            ctx.client.exec_chat(ctx.model_name, request, Some(ctx.chat_options)),
            &ctx.cancel_token,
            deadline,
        )
        .await
        {
            LlmWait::Done(result) => result,
            LlmWait::Cancelled => {
                ctx.observer.on_log("cancel", "等待模型回复时任务已停止".to_string());
                return Err(AgenticLoopStatus::Cancelled);
            }
            LlmWait::Timeout => {
                ctx.observer.on_log(
                    "timeout",
                    format!("等待模型回复时达到总超时 ({}s)，已进入第 {} 轮", ctx.limits.total_timeout_secs, round),
                );
                return Err(AgenticLoopStatus::Timeout);
            }
        };
        let error = match result {
            Ok(response) => {
                ctx.observer.on_response(&response);
                return Ok(response);
            }
            Err(error) => error,
        };
        error!(run = ctx.run_label, round, attempt, error = %error, "LLM call failed in unattended run");
        let Some(delay) = ctx.limits.retry_delays_secs.get(attempt).copied() else {
            return Err(AgenticLoopStatus::Error(format!(
                "模型调用失败（已重试 {} 次）：{error}",
                ctx.limits.retry_delays_secs.len()
            )));
        };
        attempt += 1;
        ctx.observer.on_log(
            "llm_retry",
            format!("第 {round} 轮模型调用失败，{delay} 秒后第 {attempt} 次重试：{error}"),
        );
        tokio::select! {
            biased;
            _ = ctx.cancel_token.cancelled() => return Err(AgenticLoopStatus::Cancelled),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => return Err(AgenticLoopStatus::Timeout),
            _ = tokio::time::sleep(Duration::from_secs(delay)) => {}
        }
    }
}

pub(crate) struct SavedRoundResponse {
    pub(crate) message: crate::db::conversation_db::Message,
    pub(crate) tool_calls: Vec<ToolCall>,
}

impl SavedRoundResponse {
    pub(crate) fn next_step(
        self, cancel: &CancellationToken, rounds: usize, total: usize, success: usize, failed: usize,
    ) -> std::ops::ControlFlow<AgenticLoopResult, Self> {
        if !self.tool_calls.is_empty() {
            return std::ops::ControlFlow::Continue(self);
        }
        let status = if cancel.is_cancelled() {
            AgenticLoopStatus::Cancelled
        } else {
            AgenticLoopStatus::Completed { message_id: self.message.id }
        };
        std::ops::ControlFlow::Break(AgenticLoopResult {
            final_text: self.message.content,
            rounds,
            tool_calls_total: total,
            tool_calls_success: success,
            tool_calls_failed: failed,
            status,
        })
    }
}

pub(crate) fn save_round_response(
    db: &ConversationDatabase,
    conversation_id: i64,
    chat_response: &ChatResponse,
    model_id: i64,
    model_name: &str,
    generation_group: &str,
) -> Result<SavedRoundResponse, String> {
    let mut response_text = chat_response.first_text().unwrap_or("").to_string();
    let mut captured_tool_calls: Vec<ToolCall> = chat_response.tool_calls().into_iter().cloned().collect();
    if captured_tool_calls.is_empty() {
        let (prompt_tool_calls, sanitized_text) = extract_prompt_tool_calls(&response_text);
        if !prompt_tool_calls.is_empty() {
            captured_tool_calls = prompt_tool_calls;
            response_text = sanitized_text;
        }
    }
    let now = Utc::now();
    let message = crate::api::ai_api::save_message_with_metadata(
        db,
        None,
        conversation_id,
        "response".into(),
        response_text,
        Some(model_id),
        Some(model_name.into()),
        Some(now),
        captured_tool_calls.is_empty().then_some(now),
        0,
        Some(generation_group.into()),
        None,
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(SavedRoundResponse { message, tool_calls: captured_tool_calls })
}

pub(crate) async fn run_unattended_loop(ctx: &UnattendedLoopContext<'_>) -> AgenticLoopResult {
    let mut messages: Vec<(String, String, Vec<MessageAttachment>)> = Vec::new();
    let mut round: usize = 0;
    let mut tool_calls_total: usize = 0;
    let mut tool_calls_success: usize = 0;
    let mut tool_calls_failed: usize = 0;
    let mut final_text = String::new();
    let mut progress = String::new();
    let deadline = Instant::now() + Duration::from_secs(ctx.limits.total_timeout_secs);

    let result = |final_text: String, rounds, total, success, failed, status| AgenticLoopResult {
        final_text,
        rounds,
        tool_calls_total: total,
        tool_calls_success: success,
        tool_calls_failed: failed,
        status,
    };

    match ctx.conversation_db.message_repo().map_err(|e| e.to_string()).and_then(|repo| {
        repo.list_by_conversation_id(ctx.conversation_id).map_err(|e| e.to_string())
    }) {
        Ok(rows) => {
            for (msg, attachment_opt) in &rows {
                messages.push((msg.message_type.clone(), msg.content.clone(), attachment_opt.iter().cloned().collect()));
            }
        }
        Err(e) => {
            return result(
                String::new(),
                0,
                0,
                0,
                0,
                AgenticLoopStatus::Error(format!("读取初始消息失败: {}", e)),
            );
        }
    }

    let generation_group_id = Uuid::new_v4().to_string();

    loop {
        if ctx.cancel_token.is_cancelled() {
            ctx.observer.on_log("cancel", "任务已停止".to_string());
            return result(final_text, round, tool_calls_total, tool_calls_success, tool_calls_failed, AgenticLoopStatus::Cancelled);
        }
        if Instant::now() >= deadline {
            ctx.observer.on_log(
                "timeout",
                format!("总超时 ({}s)，已完成 {} 轮", ctx.limits.total_timeout_secs, round),
            );
            return result(final_text, round, tool_calls_total, tool_calls_success, tool_calls_failed, AgenticLoopStatus::Timeout);
        }
        if round >= ctx.limits.max_rounds {
            ctx.observer.on_log("max_rounds", format!("达到最大轮次 ({})", ctx.limits.max_rounds));
            return result(final_text, round, tool_calls_total, tool_calls_success, tool_calls_failed, AgenticLoopStatus::MaxRoundsReached);
        }

        round += 1;
        info!(run = ctx.run_label, round, "unattended loop round start");

        let chat_response = match exec_chat_with_retry(ctx, &messages, round, deadline).await {
            Ok(response) => response,
            Err(status) => return result(String::new(), round, tool_calls_total, tool_calls_success, tool_calls_failed, status),
        };

        let saved = match save_round_response(
            ctx.conversation_db,
            ctx.conversation_id,
            &chat_response,
            ctx.llm_model_id,
            &ctx.llm_model_code,
            &generation_group_id,
        ) {
            Ok(saved) => saved,
            Err(e) => {
                return result(
                    String::new(),
                    round,
                    tool_calls_total,
                    tool_calls_success,
                    tool_calls_failed,
                    AgenticLoopStatus::Error(format!("运行 {} 会话 {} 第 {round} 轮回复写入失败：{e}", ctx.run_label, ctx.conversation_id)),
                );
            }
        };
        debug!(run = ctx.run_label, round, text_len = saved.message.content.len(), tool_calls = saved.tool_calls.len(), "unattended loop response");
        let saved = match saved.next_step(&ctx.cancel_token, round, tool_calls_total, tool_calls_success, tool_calls_failed) {
            std::ops::ControlFlow::Break(completed) => {
                if matches!(completed.status, AgenticLoopStatus::Cancelled) {
                    ctx.observer.on_log("cancel", "模型已回复但任务已停止，不按完成处理".to_string());
                }
                return completed;
            }
            std::ops::ControlFlow::Continue(saved) => saved,
        };
        let response_message_id = saved.message.id;
        let mut response_content = saved.message.content;
        let captured_tool_calls = saved.tool_calls;
        if !response_content.trim().is_empty() {
            progress = response_content.clone();
        }

        ctx.observer.on_log("tool_round", format!("第 {} 轮：{} 个工具调用", round, captured_tool_calls.len()));

        let mut prepared_tool_calls: Vec<PreparedAgenticToolCall> = Vec::new();
        for tc in &captured_tool_calls {
            let (server_name, tool_name) = resolve_tool_name(&tc.fn_name, ctx.tool_name_mapping);
            let params_str = normalize_tool_arguments(&tc.fn_arguments);
            let params_json = serde_json::from_str::<serde_json::Value>(&params_str)
                .unwrap_or_else(|_| serde_json::Value::String(params_str.clone()));
            let (call_db_id, call_record_error) = match create_tool_call_record(
                ctx.app_handle,
                ctx.conversation_id,
                Some(response_message_id),
                &server_name,
                &tool_name,
                &params_str,
                &tc.call_id,
            )
            .await
            {
                Ok(call_id) => (Some(call_id), None),
                Err(e) => (None, Some(format!("Error: failed to create tool call record: {}", e))),
            };
            response_content.push_str(&build_mcp_tool_call_ui_hint(&server_name, &tool_name, &params_str, call_db_id, &tc.call_id));
            prepared_tool_calls.push(PreparedAgenticToolCall {
                llm_call_id: tc.call_id.clone(),
                call_db_id,
                call_record_error,
                server_name,
                tool_name,
                params_str,
                params_json,
            });
        }

        if let Ok(repo) = ctx.conversation_db.message_repo() {
            if let Ok(Some(mut msg)) = repo.read(response_message_id) {
                msg.content = response_content.clone();
                msg.tool_calls_json = serde_json::to_string(&captured_tool_calls).ok();
                let _ = repo.update(&msg);
            }
        }
        messages.push(("response".to_string(), response_content.clone(), Vec::new()));

        for tool_call in &prepared_tool_calls {
            if ctx.cancel_token.is_cancelled() {
                ctx.observer.on_log("cancel", "任务已停止".to_string());
                return result(final_text, round, tool_calls_total, tool_calls_success, tool_calls_failed, AgenticLoopStatus::Cancelled);
            }
            tool_calls_total += 1;
            ctx.observer.on_log(
                "tool_call",
                serde_json::json!({
                    "callId": tool_call.llm_call_id.clone(),
                    "callDbId": tool_call.call_db_id,
                    "serverName": tool_call.server_name.clone(),
                    "toolName": tool_call.tool_name.clone(),
                    "parameters": tool_call.params_json.clone(),
                })
                .to_string(),
            );

            let (success, result_text) = match tool_call.call_db_id {
                Some(call_db_id) => match ctx.gate.admit(call_db_id, &progress).await {
                    Admission::Run(basis) => {
                        ctx.observer.on_log("admitted", basis);
                        execute_single_tool(
                            ctx.app_handle,
                            call_db_id,
                            Duration::from_secs(ctx.limits.per_tool_timeout_secs),
                            ctx.cancel_token.clone(),
                        )
                        .await
                    }
                    Admission::Refuse(reason) => {
                        if let Ok(db) = MCPDatabase::new(ctx.app_handle) {
                            fail_call(&db, call_db_id, &reason);
                        }
                        (false, reason)
                    }
                },
                None => (
                    false,
                    tool_call
                        .call_record_error
                        .clone()
                        .unwrap_or_else(|| "Error: failed to create tool call record before execution".to_string()),
                ),
            };
            if success {
                tool_calls_success += 1;
            } else {
                tool_calls_failed += 1;
            }

            let tool_result_call_id = if tool_call.llm_call_id.trim().is_empty() {
                tool_call.call_db_id.map(|id| format!("mcp_tool_call_{}", id)).unwrap_or_else(|| Uuid::new_v4().to_string())
            } else {
                tool_call.llm_call_id.clone()
            };
            let tool_result_content = format!(
                "Tool execution completed:\n\nTool Call ID: {}\nTool: {}\nServer: {}\nParameters: {}\nResult:\n{}",
                tool_result_call_id.as_str(),
                tool_call.tool_name.as_str(),
                tool_call.server_name.as_str(),
                tool_call.params_str.as_str(),
                result_text
            );
            let _ = crate::api::ai_api::add_message(
                ctx.app_handle,
                None,
                ctx.conversation_id,
                "tool_result".to_string(),
                tool_result_content.clone(),
                Some(ctx.llm_model_id),
                Some(ctx.llm_model_code.clone()),
                Some(Utc::now()),
                Some(Utc::now()),
                0,
                Some(generation_group_id.clone()),
                None,
            );
            messages.push(("tool_result".to_string(), tool_result_content, Vec::new()));
            ctx.observer.on_log(
                "tool_result",
                serde_json::json!({
                    "callId": tool_result_call_id,
                    "callDbId": tool_call.call_db_id,
                    "serverName": tool_call.server_name.clone(),
                    "toolName": tool_call.tool_name.clone(),
                    "success": success,
                    "parameters": tool_call.params_json.clone(),
                    "result": result_text,
                })
                .to_string(),
            );
        }

        if let Ok(repo) = ctx.conversation_db.message_repo() {
            if let Ok(Some(mut msg)) = repo.read(response_message_id) {
                msg.finish_time = Some(Utc::now());
                let _ = repo.update(&msg);
            }
        }
        final_text.clear();
    }
}

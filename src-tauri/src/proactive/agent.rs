//! 主动整理执行引擎：支持普通 LLM（无头 Agentic Loop）与原生 Agent（ACP / Claude Code / Codex / NativeAgents）。
//!
//! 两种执行路径共享同一套主动整理工具集（`aipp:agent` 下的 `proactive_*`）、全局 MCP 工具、
//! 以及 `proactive/gate.rs` 无人值守安全审核门禁。
use super::{gate::ProactiveToolGate, Config};
use crate::api::ai::config::{
    get_network_proxy_from_config, get_request_timeout_from_config, ConfigBuilder,
};
use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy};
use crate::api::ai::unattended_loop::{
    build_tool_config_for_mcp, run_unattended_loop, AgenticLoopStatus, LoopLimits,
    UnattendedLoopContext, UnattendedLoopObserver,
};
use crate::api::{ai, butler_api, genai_client};
use crate::db::conversation_db::{ConversationDatabase, Repository as _};
use crate::db::llm_db::LLMDatabase;
use crate::mcp::{collect_mcp_info_for_assistant, format_mcp_prompt};
use crate::skills::format_skills_prompt;
use anyhow::{anyhow, ensure, Context, Result};
use genai::chat::ChatResponse;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

const PROACTIVE_MAX_ROUNDS: usize = 24;
const PROACTIVE_PER_TOOL_TIMEOUT_SECS: u64 = 90;
const PROACTIVE_TOTAL_TIMEOUT_SECS: u64 = 300;
const PROACTIVE_RETRY_DELAYS_SECS: [u64; 2] = [5, 15];

pub(super) const NATIVE_ASSISTANT_PREFIX: &str = "assistant:";

pub(super) struct ExecutionOutput {
    pub final_text: String,
    pub tool_calls_total: usize,
}

/// 轻量单轮文本调用：用于小模型初筛（`ACT`/`QUIET`）与联系人提醒文案润色。
pub(super) async fn chat_simple(
    app: &AppHandle,
    session: i64,
    selection: &str,
    system: &str,
    input: &str,
) -> Result<String> {
    let (code, provider) = resolve_llm_selection(app, selection).await?;
    let cfg = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let detail = LLMDatabase::new(app)?
        .get_model_for_request(&provider, &code)
        .with_context(|| format!("读取模型 {code} / {provider}"))?;
    let proxy = ai::config::get_network_proxy_from_config(&cfg);
    let configs = crate::api::copilot_token_manager::prepare_provider_configs(
        app,
        &detail.provider.api_type,
        &detail.configs,
        proxy.as_deref(),
    )
    .await
    .map_err(anyhow::Error::msg)?;
    let client = genai_client::create_client_with_config(
        Some((app, session)),
        &configs,
        &code,
        &detail.provider.api_type,
        Some(&detail.model.request_mode),
        proxy.as_deref(),
        false,
        Some(ai::config::get_request_timeout_from_config(&cfg)),
        false,
        &cfg,
    )
    .map_err(|e| anyhow!("创建模型客户端 {code} / {provider}: {e}"))?;
    let request = build_chat_request_from_messages(
        &[("system".into(), system.into(), Vec::new()), ("user".into(), input.into(), Vec::new())],
        ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;
    let reply = client
        .exec_chat(&code, request, None)
        .await
        .with_context(|| format!("主动整理会话 #{session} 模型 {code} / {provider}"))?;
    record_usage(app, session, detail.model.id, &detail.model.name, &reply);
    Ok(reply.first_text().context("模型没有返回正文")?.trim().to_owned())
}

fn record_usage(app: &AppHandle, session: i64, model_id: i64, model_name: &str, response: &ChatResponse) {
    if let Ok(db) = ConversationDatabase::new(app) {
        crate::api::token_statistics_api::record_auxiliary_usage(
            &db,
            crate::api::token_statistics_api::auxiliary_usage_from_genai(
                session,
                "butler_proactive",
                Some(model_id),
                model_name,
                &response.usage,
            ),
        );
    }
}

async fn resolve_llm_selection(app: &AppHandle, selection: &str) -> Result<(String, i64)> {
    let trimmed = selection.trim();
    if trimmed.is_empty() {
        let m = butler_api::get_butler_model_selection(app).await.map_err(anyhow::Error::msg)?;
        Ok((m.model_code, m.provider_id))
    } else {
        let (code, id) = trimmed.rsplit_once("%%").context("请选择模型及供应商")?;
        Ok((code.to_owned(), id.parse()?))
    }
}

pub(super) fn parse_native_assistant_id(selection: &str) -> Option<i64> {
    selection
        .trim()
        .strip_prefix(NATIVE_ASSISTANT_PREFIX)
        .and_then(|id| id.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
}

/// 统一执行入口：根据 `cfg.deep_model` 自动路由到原生 Agent 或普通 LLM 无头循环。
pub(super) async fn execute(
    app: &AppHandle,
    session: i64,
    cfg: &Config,
    chief: i64,
    system: &str,
    user_input: &str,
) -> Result<ExecutionOutput> {
    let native = if cfg.deep_model.trim().is_empty()
        && butler_api::get_butler_model_selection(app).await.map_err(anyhow::Error::msg)?.local_agent {
        Some(chief)
    } else {
        parse_native_assistant_id(&cfg.deep_model)
    };
    if let Some(assistant_id) = native {
        execute_native_agent(app, session, assistant_id, system, user_input).await
    } else {
        execute_llm_loop(app, session, cfg, chief, system, user_input).await
    }
}

struct UsageObserver<'a> {
    app: &'a AppHandle,
    session: i64,
    model_id: i64,
    model_name: String,
}

impl UnattendedLoopObserver for UsageObserver<'_> {
    fn on_response(&self, response: &ChatResponse) {
        record_usage(self.app, self.session, self.model_id, &self.model_name, response);
    }
}

async fn execute_llm_loop(
    app: &AppHandle,
    session: i64,
    cfg: &Config,
    chief: i64,
    system: &str,
    user_input: &str,
) -> Result<ExecutionOutput> {
    let (code, provider_id) = resolve_llm_selection(app, &cfg.deep_model).await?;
    let feature_config_map = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let model_detail = LLMDatabase::new(app)?
        .get_model_for_request(&provider_id, &code)
        .with_context(|| format!("读取整理模型 {code} / {provider_id}"))?;

    let mut mcp_info = collect_mcp_info_for_assistant(app, chief, None, None)
        .await
        .map_err(|e| anyhow!("加载主动整理工具失败：{e}"))?;
    crate::mcp::butler_tools::prepare_proactive_run_tools(
        app,
        &mut mcp_info.enabled_servers,
        cfg.allow_notifications,
    )
    .map_err(|e| anyhow!("准备主动整理工具失败：{e}"))?;

    let mut full_system = if !mcp_info.enabled_servers.is_empty() && !mcp_info.use_native_toolcall {
        format_mcp_prompt(system.to_string(), &mcp_info).await
    } else {
        system.to_string()
    };
    let skills_info = crate::skills::collect_all_skills_for_assistant(app, chief, &[])
        .await
        .map_err(|e| anyhow!("加载 Skills 失败：{e}"))?;
    if !skills_info.enabled_skills.is_empty() {
        full_system = format_skills_prompt(app, full_system, &skills_info).await;
    }

    let network_proxy = get_network_proxy_from_config(&feature_config_map);
    let request_timeout = get_request_timeout_from_config(&feature_config_map);
    let proxy_enabled = model_detail
        .configs
        .iter()
        .find(|c| c.name == "proxy_enabled")
        .and_then(|c| c.value.parse::<bool>().ok())
        .unwrap_or(false);
    let provider_configs = crate::api::copilot_token_manager::prepare_provider_configs(
        app,
        &model_detail.provider.api_type,
        &model_detail.configs,
        network_proxy.as_deref(),
    )
    .await
    .map_err(anyhow::Error::msg)?;
    let client = genai_client::create_client_with_config(
        Some((app, session)),
        &provider_configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        Some(&model_detail.model.request_mode),
        network_proxy.as_deref(),
        proxy_enabled,
        Some(request_timeout),
        false,
        &feature_config_map,
    )
    .map_err(|e| anyhow!("创建整理模型客户端失败：{e}"))?;

    let merged_configs = ConfigBuilder::merge_model_configs(Vec::new(), &model_detail, None);
    let config_map: HashMap<String, String> = merged_configs
        .iter()
        .filter_map(|c| c.value.as_ref().map(|v| (c.name.clone(), v.clone())))
        .collect();
    let model_name = config_map.get("model").cloned().unwrap_or_else(|| model_detail.model.code.clone());
    let provider_api_type_lc = model_detail.provider.api_type.to_lowercase();
    let model_code_lc = model_detail.model.code.to_lowercase();
    let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
    let is_gemini = model_code_lc.contains("gemini");
    let capture_usage = !(is_openai_like && is_gemini);
    let has_tools = mcp_info.use_native_toolcall && !mcp_info.enabled_servers.is_empty();
    let capture_reasoning = is_openai_like && has_tools && config_map.contains_key("reasoning_effort");
    let chat_options = ConfigBuilder::build_chat_options(&config_map, None, None)
        .with_normalize_reasoning_content(true)
        .with_capture_reasoning_content(capture_reasoning)
        .with_capture_usage(capture_usage)
        .with_capture_tool_calls(has_tools);
    let tool_call_strategy = if has_tools { ToolCallStrategy::Native } else { ToolCallStrategy::NonNative };
    let tool_config = build_tool_config_for_mcp(&mcp_info, has_tools);
    let tool_name_mapping = tool_config.as_ref().map(|c| c.tool_name_mapping.clone()).unwrap_or_default();

    let conversation_db = ConversationDatabase::new(app)?;
    let now = chrono::Utc::now();
    crate::api::ai_api::save_message_with_metadata(
        &conversation_db,
        None,
        session,
        "system".into(),
        full_system,
        Some(model_detail.model.id),
        Some(model_detail.model.code.clone()),
        Some(now),
        Some(now),
        0,
        None,
        None,
        None,
    )?;
    crate::api::ai_api::save_message_with_metadata(
        &conversation_db,
        None,
        session,
        "user".into(),
        user_input.to_string(),
        Some(model_detail.model.id),
        Some(model_detail.model.code.clone()),
        Some(now),
        Some(now),
        0,
        None,
        None,
        None,
    )?;

    let gate = ProactiveToolGate { app, conversation_id: session };
    let observer = UsageObserver {
        app,
        session,
        model_id: model_detail.model.id,
        model_name: model_detail.model.name.clone(),
    };
    let cancel_token = CancellationToken::new();
    let run_label = format!("proactive-{session}");
    let ctx = UnattendedLoopContext {
        app_handle: app,
        client: &client,
        model_name: &model_name,
        chat_options: &chat_options,
        conversation_id: session,
        conversation_db: &conversation_db,
        tool_name_mapping: &tool_name_mapping,
        tool_config,
        tool_call_strategy,
        llm_model_id: model_detail.model.id,
        llm_model_code: model_detail.model.code.clone(),
        run_label: &run_label,
        limits: LoopLimits {
            max_rounds: PROACTIVE_MAX_ROUNDS,
            per_tool_timeout_secs: PROACTIVE_PER_TOOL_TIMEOUT_SECS,
            total_timeout_secs: PROACTIVE_TOTAL_TIMEOUT_SECS,
            retry_delays_secs: &PROACTIVE_RETRY_DELAYS_SECS,
        },
        cancel_token,
        gate: &gate,
        observer: &observer,
    };
    let loop_result = run_unattended_loop(&ctx).await;
    match loop_result.status {
        AgenticLoopStatus::Completed { .. } => Ok(ExecutionOutput {
            final_text: loop_result.final_text,
            tool_calls_total: loop_result.tool_calls_total,
        }),
        AgenticLoopStatus::MaxRoundsReached => Ok(ExecutionOutput {
            final_text: loop_result.final_text,
            tool_calls_total: loop_result.tool_calls_total,
        }),
        AgenticLoopStatus::Timeout => Err(anyhow!("主动整理超过 {PROACTIVE_TOTAL_TIMEOUT_SECS} 秒")),
        AgenticLoopStatus::Cancelled => Err(anyhow!("主动整理被取消")),
        AgenticLoopStatus::Error(err) => Err(anyhow!("{err}")),
    }
}

async fn execute_native_agent(
    app: &AppHandle,
    session: i64,
    assistant_id: i64,
    system: &str,
    user_input: &str,
) -> Result<ExecutionOutput> {
    if cfg!(mobile) {
        anyhow::bail!("移动端不支持使用本地原生 Agent 执行主动整理");
    }
    let assistant_detail = crate::api::assistant_api::get_assistant(app.clone(), assistant_id)
        .map_err(|e| anyhow!("读取原生 Agent 助手 #{assistant_id} 失败：{e}"))?;
    ensure!(
        assistant_detail.assistant.assistant_type == Some(4),
        "所选助手「{}」不是原生 Agent 助手",
        assistant_detail.assistant.name
    );

    // 将隐藏运行会话绑定到该原生 Agent 助手，并写入触发消息
    let conversation_db = ConversationDatabase::new(app)?;
    if let Some(mut conv) = conversation_db.conversation_repo()?.read(session)? {
        conv.assistant_id = Some(assistant_id);
        conversation_db.conversation_repo()?.update(&conv)?;
    }
    let now = chrono::Utc::now();
    let user_msg = crate::api::ai_api::save_message_with_metadata(
        &conversation_db,
        None,
        session,
        "user".into(),
        user_input.to_string(),
        Some(0),
        Some(assistant_detail.assistant.name.clone()),
        Some(now),
        Some(now),
        0,
        None,
        None,
        None,
    )?;

    let window = butler_api::resolve_or_create_butler_execution_window(app)
        .map_err(|e| anyhow!("获取原生 Agent 执行窗口失败：{e}"))?;
    let feature_configs = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let messages = vec![("user".to_string(), user_input.to_string(), Vec::new())];
    let overrides = None;
    let request = crate::api::ai::agent_driver::AgentTurnRequest {
        assistant: &assistant_detail,
        conversation_id: session,
        native_butler_role: Some(system.to_string()),
        source_message_id: Some(user_msg.id),
        prompt: user_input.to_string(),
        display_prompt: user_input.to_string(),
        override_model_id: None,
        overrides: &overrides,
        feature_configs: &feature_configs,
        messages: &messages,
    };

    let dispatch_res = crate::api::ai::agent_driver::send_turn(app.clone(), window, request).await;
    let result = match dispatch_res {
        Ok(resp) => {
            if let Some(err) = resp.error_message {
                Err(anyhow!("原生 Agent 启动失败：{err}"))
            } else {
                wait_native_turn_completion(app, session, user_msg.id).await
            }
        }
        Err(err) => Err(anyhow!("原生 Agent 派发失败：{err}")),
    };
    // 后台临时整理会话结束后主动释放原生子进程，避免常驻泄漏。
    release_native_session(app, session).await;
    result
}

async fn wait_native_turn_completion(app: &AppHandle, session: i64, user_msg_id: i64) -> Result<ExecutionOutput> {
    let deadline = Instant::now() + Duration::from_secs(PROACTIVE_TOTAL_TIMEOUT_SECS);
    let activity = app.state::<crate::ConversationActivityManager>();
    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("原生 Agent 主动整理超过 {PROACTIVE_TOTAL_TIMEOUT_SECS} 秒");
        }
        let running = activity.get_runtime_state(session).await.is_running;
        let conn = super::connection(app)?;
        let finished: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT content, finish_time FROM message WHERE conversation_id=?1 AND parent_id=?2 AND message_type='response' ORDER BY id DESC LIMIT 1",
                rusqlite::params![session, user_msg_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if let Some((content, Some(_))) = finished {
            if !running {
                let tool_calls_total: usize = conn
                    .query_row("SELECT count(*) FROM mcp_tool_call WHERE conversation_id=?1", [session], |r| r.get(0))
                    .unwrap_or(0);
                return Ok(ExecutionOutput { final_text: content, tool_calls_total });
            }
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

async fn release_native_session(app: &AppHandle, conversation_id: i64) {
    if let Some(state) = app.try_state::<crate::AcpSessionState>() {
        let mut sessions = state.sessions.lock().await;
        sessions.remove(&conversation_id);
    }
    if let Some(state) = app.try_state::<crate::CodexSessionState>() {
        let mut sessions = state.sessions.lock().await;
        if let Some(entry) = sessions.remove(&conversation_id) {
            entry.handle.shutdown("主动整理临时会话结束，释放 Codex 进程".to_string());
        }
    }
    if let Some(state) = app.try_state::<crate::ClaudeSessionState>() {
        let mut sessions = state.sessions.lock().await;
        sessions.remove(&conversation_id);
    }
    crate::api::ai::agent_driver::native_agents::release_conversation(conversation_id).await;
}

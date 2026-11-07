//! Provider-specific preparation behind one AIPP agent turn entry point.
//! Session ownership, persistence, events and protocol implementations stay in the
//! existing native adapters. There is deliberately no second process/session pool.
use super::session::{AgentSessionHandle, AgentSessionKind};
use crate::api::ai::acp::{AcpConfig, AcpSessionConfigChoicePayload, apply_network_proxy_to_env_vars, build_selected_mcp_tools_payload, extract_acp_config, refresh_acp_config_signature, spawn_agent_idle_reaper_once, spawn_acp_session_task};
use crate::api::ai::codex_app_server::{CodexAppServerConfig, extract_codex_app_server_config, refresh_codex_session_signature, spawn_codex_session_task, CodexSessionEntry, CODEX_APPROVAL_POLICIES, CODEX_SANDBOX_MODES};
use crate::api::ai::claude_sdk::{ClaudeSdkConfig, claude_model_choices, extract_claude_sdk_config, prepare_claude_mcp_bridge, refresh_claude_session_signature, spawn_claude_session_task, ClaudeSessionEntry};
use crate::api::ai::config::get_network_proxy_from_config;
use crate::api::ai::events::ConversationEvent;
use crate::api::ai::types::AiResponse;
use crate::api::ai_api::parse_agent_model_override;
use crate::api::assistant_api::{AssistantDetail, resolve_acp_provider_id, CLAUDE_CODE_DEFAULT_MODEL};
use crate::db::conversation_db::{ConversationDatabase, Message, MessageAttachment, Repository};
use super::dispatch::{dispatch_prepared, response_message, provider_name, TurnEffects, TurnInput};
use crate::db::llm_db::{LLMDatabase, LLMProviderConfig};
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::state::activity_state::ConversationActivityManager;
use crate::{AcpSessionState, ClaudeSessionState, CodexSessionState};
use std::collections::HashMap;
use tauri::{Emitter, Manager};
use tracing::{debug, info, warn};

pub(crate) struct AgentTurnRequest<'a> {
    pub assistant: &'a AssistantDetail,
    pub conversation_id: i64,
    pub prompt: String,
    pub display_prompt: String,
    pub override_model_id: Option<String>,
    pub overrides: &'a Option<HashMap<String, serde_json::Value>>,
    pub feature_configs: &'a HashMap<String, HashMap<String, FeatureConfig>>,
    pub messages: &'a [(String, String, Vec<MessageAttachment>)],
}

async fn prepare_config(
    app_handle: tauri::AppHandle,
    request: &AgentTurnRequest<'_>,
) -> Result<PreparedAgentSession, AppError> {
    let assistant_detail = request.assistant;
    let conversation_id = request.conversation_id;
    info!("Agent assistant detected (type=4), preparing native driver");

    // 移动端不支持本机 CLI Agent；禁止静默换模型/通道
    if cfg!(mobile) {
        return Err(AppError::InternalError(
            crate::api::system_api::mobile_unsupported_error(
                "本地 CLI Agent（Codex/Claude/ACP）",
            ),
        ));
    }

    let selected = load_agent_provider(assistant_detail, request.override_model_id.as_deref(), |provider_id| {
        let llm_db = LLMDatabase::new(&app_handle).map_err(|e| {
            AppError::UnknownError(format!("Failed to open LLM database: {}", e))
        })?;
        let provider = llm_db.get_llm_provider(provider_id).map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => AppError::UnknownError(format!(
                "Agent 助手 {} 引用的提供商 ID {provider_id} 不存在，请在助手配置中重新选择提供商",
                assistant_detail.assistant.id
            )),
            other => AppError::from(other),
        })?;
        let configs = llm_db.get_llm_provider_config(provider_id).map_err(AppError::from)?;
        Ok((provider.api_type, configs))
    })?;
    let model_choices = if selected.kind == AgentSessionKind::Claude {
        let llm_db = LLMDatabase::new(&app_handle).map_err(AppError::from)?;
        claude_model_choices(
            &llm_db.get_llm_models(selected.provider_id.to_string())
                .map_err(|e| AppError::UnknownError(e.to_string()))?,
        )
    } else {
        Vec::new()
    };
    let mut prepared = prepare_selected_config(request, &selected, model_choices)?;
    let selected_mcp_tools_payload = build_selected_mcp_tools_payload(
        &app_handle, assistant_detail.assistant.id, Some(conversation_id),
    ).map_err(AppError::UnknownError)?;
    prepared.finalize_mcp_tools(selected_mcp_tools_payload);
    if let PreparedAgentSession::Claude(config) = &mut prepared {
        prepare_claude_mcp_bridge(&app_handle, conversation_id, config)
            .await
            .map_err(|error| AppError::UnknownError(format!("Claude Code MCP bridge 初始化失败：{error}")))?;
        info!(
            conversation_id,
            provider_id = selected.provider_id,
            model = ?config.model,
            cli = %config.cli_command,
            "Claude Code provider selected; starting stream-json session"
        );
    }
    Ok(prepared)
}

struct SelectedAgentProvider {
    provider_id: i64,
    kind: AgentSessionKind,
    model_override: Option<String>,
    configs: Vec<LLMProviderConfig>,
}

/// Validate the explicit selection before opening the provider database. A
/// nonblank malformed override must never become a request for the assistant's
/// default. Blank values retain the existing no-override compatibility behavior.
fn load_agent_provider(
    assistant: &AssistantDetail,
    override_model_id: Option<&str>,
    load: impl FnOnce(i64) -> Result<(String, Vec<LLMProviderConfig>), AppError>,
) -> Result<SelectedAgentProvider, AppError> {
    let model_override = override_model_id
        .filter(|value| !value.trim().is_empty())
        .map(|value| parse_agent_model_override(value).ok_or_else(|| {
            AppError::UnknownError(format!(
                "Agent 所选模型 ID「{value}」格式无效，预期为 model%%provider_id 且提供商 ID 必须大于 0；请重新选择模型，未回退到默认配置"
            ))
        }))
        .transpose()?;
    let default_provider_id = resolve_acp_provider_id(&assistant.model, &assistant.model_configs);
    let provider_id = model_override.map(|(_, id)| id).or(default_provider_id)
        .ok_or_else(|| AppError::UnknownError("Agent 助手尚未配置模型提供商".to_string()))?;
    if model_override.is_some() {
        info!(
            default_provider_id = ?default_provider_id,
            override_provider_id = provider_id,
            "Agent model override selects provider for routing"
        );
    }
    let (api_type, configs) = load(provider_id)?;
    let kind = AgentSessionKind::from_provider(&api_type, &configs)?;
    // Preserve the existing Claude requirement for a configured assistant.
    if kind == AgentSessionKind::Claude && default_provider_id.is_none() {
        return Err(AppError::UnknownError("Claude Code 助手没有配置模型提供商".to_string()));
    }
    debug!(
        provider_id,
        provider_api_type = %api_type,
        config_names = ?configs.iter().map(|config| config.name.as_str()).collect::<Vec<_>>(),
        "Agent provider configuration loaded"
    );
    Ok(SelectedAgentProvider {
        provider_id, kind, model_override: model_override.map(|(model, _)| model.to_string()), configs,
    })
}

/// The production preparation boundary after provider reads and before MCP or
/// actor I/O. Uses the native extractors, not a parallel test implementation.
fn prepare_selected_config(
    request: &AgentTurnRequest<'_>,
    selected: &SelectedAgentProvider,
    model_choices: Vec<AcpSessionConfigChoicePayload>,
) -> Result<PreparedAgentSession, AppError> {
    let assistant_detail = request.assistant;
    let conversation_id = request.conversation_id;
    let override_model_config = request.overrides;
    let provider_configs = &selected.configs;
    let driver = selected.kind;

    if driver == AgentSessionKind::Codex {
        info!(conversation_id, "Codex app-server provider detected");
        let model_code = assistant_detail.model.first().map(|model| model.model_code.clone()).filter(|value| !value.trim().is_empty());
        let mut codex_config = extract_codex_app_server_config(
            &assistant_detail.model_configs,
            provider_configs,
            model_code,
        )?;
        if let Some(model) = &selected.model_override {
            codex_config.model = Some(model.clone());
        }
        if let Some(config) = override_model_config.as_ref() {
            if let Some(effort) = config.get("reasoning_effort").and_then(|value| value.as_str()) { codex_config.reasoning_effort = Some(effort.to_string()); }
            // 新对话界面传入的会话级审批策略/沙箱覆盖，非法值直接报错（不做回退）
            if let Some(approval) = config.get("approval_policy").and_then(|value| value.as_str()).filter(|value| !value.trim().is_empty()) {
                if !CODEX_APPROVAL_POLICIES.contains(&approval) { return Err(AppError::UnknownError(format!("Codex 不支持审批策略：{approval}"))); }
                codex_config.approval_policy = Some(approval.to_string());
            }
            if let Some(sandbox) = config.get("sandbox").and_then(|value| value.as_str()).filter(|value| !value.trim().is_empty()) {
                if !CODEX_SANDBOX_MODES.contains(&sandbox) { return Err(AppError::UnknownError(format!("Codex 不支持沙箱模式：{sandbox}"))); }
                codex_config.sandbox = Some(sandbox.to_string());
            }
            if let Some(mode) = config.get("collaboration_mode").and_then(|value| value.as_str()).filter(|value| !value.trim().is_empty()) {
                if !matches!(mode, "default" | "plan") { return Err(AppError::UnknownError(format!("Codex 不支持协作模式：{mode}"))); }
                codex_config.collaboration_mode = Some(mode.to_string());
            }
        }
        return Ok(PreparedAgentSession::Codex(codex_config));
    }

    if driver == AgentSessionKind::Claude {
        info!(conversation_id, "Entering Claude Code stream-json provider branch");
        let model_code = assistant_detail.model.first().map(|model| model.model_code.clone()).filter(|value| !value.trim().is_empty());
        let selected_model = selected.model_override.clone().or(model_code);
        let selected_model = selected_model.filter(|model| model != CLAUDE_CODE_DEFAULT_MODEL);
        let mut config = extract_claude_sdk_config(&assistant_detail.model_configs, provider_configs, selected_model, model_choices)?;
        if let Some(overrides) = override_model_config.as_ref() {
            if let Some(effort) = overrides.get("claude_effort").and_then(|value| value.as_str()) { config.effort = (effort != "default").then_some(effort.to_string()); }
            if let Some(mode) = overrides.get("permission_mode").and_then(|value| value.as_str()).filter(|value| !value.trim().is_empty()) {
                if !matches!(mode, "default" | "plan") { return Err(AppError::UnknownError(format!("Claude Code 不支持工作模式：{mode}"))); }
                config.permission_mode = Some(mode.to_string());
            }
        }
        return Ok(PreparedAgentSession::Claude(config));
    }

    // 从 assistant_model_configs 和 llm_provider_configs 提取 ACP 配置
    let proxy_enabled = provider_configs
        .iter()
        .find(|config| config.name == "proxy_enabled")
        .and_then(|config| config.value.parse::<bool>().ok())
        .unwrap_or(false);
    let network_proxy =
        proxy_enabled.then(|| get_network_proxy_from_config(request.feature_configs)).flatten();
    let mut acp_config =
        extract_acp_config(&assistant_detail.model_configs, provider_configs)?;
    acp_config.model = selected.model_override.clone()
        .or_else(|| {
            assistant_detail
                .model
                .first()
                .map(|model| model.model_code.trim())
                .filter(|model| !model.is_empty())
                .map(str::to_string)
        });
    if let Some(thought_level) = override_model_config
        .as_ref()
        .and_then(|config| config.get("reasoning_effort"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        acp_config.thought_level = Some(thought_level.to_string());
    }
    if let Some(proxy_url) = network_proxy.as_deref() {
        let injected = apply_network_proxy_to_env_vars(&mut acp_config.env_vars, proxy_url);
        info!(
            proxy_url = %proxy_url,
            injected,
            conversation_id,
            "ACP proxy env vars applied"
        );
    }
    info!(
        "ACP config: cli_command={}, working_directory={}, env_vars={}, additional_args={}",
        acp_config.cli_command,
        acp_config.working_directory.display(),
        acp_config.env_vars.len(),
        acp_config.additional_args.len()
    );

    Ok(PreparedAgentSession::Acp(acp_config))
}

pub(super) enum PreparedAgentSession {
    Codex(CodexAppServerConfig),
    Claude(ClaudeSdkConfig),
    Acp(AcpConfig),
}

impl PreparedAgentSession {
    fn kind(&self) -> AgentSessionKind {
        match self {
            Self::Codex(_) => AgentSessionKind::Codex,
            Self::Claude(_) => AgentSessionKind::Claude,
            Self::Acp(_) => AgentSessionKind::Acp,
        }
    }

    /// MCP selection is part of native actor reuse identity. Refresh only after
    /// all turn overrides and the current tool payload have been applied.
    fn finalize_mcp_tools(&mut self, payload: String) {
        match self {
            Self::Codex(config) => {
                config.selected_mcp_tools_payload = payload;
                refresh_codex_session_signature(config);
            }
            Self::Claude(config) => {
                config.selected_mcp_tools_payload = payload;
                refresh_claude_session_signature(config);
            }
            Self::Acp(config) => {
                config.selected_mcp_tools_payload = payload;
                refresh_acp_config_signature(config);
            }
        }
    }

    /// Acquire only after the response placeholder has been persisted. A DB
    /// write failure must not replace or shut down the existing native session.
    async fn acquire(self, app_handle: &tauri::AppHandle, conversation_id: i64) -> AgentSessionHandle {
        spawn_agent_idle_reaper_once(app_handle.clone());
        match self {
            Self::Codex(codex_config) => {
                let codex_session_state = app_handle.state::<CodexSessionState>();
                let handle = {
                    let mut sessions = codex_session_state.sessions.lock().await;
                    if let Some(handle) = sessions.get_mut(&conversation_id)
                        .and_then(|entry| entry.reuse_handle(&codex_config.session_signature)) {
                        handle
                    } else {
                        let handle = spawn_codex_session_task(app_handle.clone(), conversation_id, codex_config.clone());
                        if let Some(previous) = sessions.get(&conversation_id) {
                            let reason = format!(
                                "ask_ai 因用户本次请求的 Codex 配置发生变化而替换现有会话（old_run_id={}, new_run_id={}）",
                                previous.run_id,
                                handle.run_id()
                            );
                            warn!(conversation_id, reason, "Replacing Codex session from ask_ai");
                            previous.handle.shutdown(reason);
                        }
                        sessions.insert(conversation_id, CodexSessionEntry::new(handle.clone(), conversation_id, codex_config.session_signature.clone()));
                        handle
                    }
                };
                AgentSessionHandle::Codex(handle)
            }
            Self::Claude(config) => {
                let claude_state = app_handle.state::<ClaudeSessionState>();
                let handle = {
                    let mut sessions = claude_state.sessions.lock().await;
                    if let Some(handle) = sessions.get_mut(&conversation_id)
                        .and_then(|entry| entry.reuse_handle(&config.session_signature)) {
                        handle
                    } else {
                        let handle = spawn_claude_session_task(app_handle.clone(), conversation_id, config.clone());
                        sessions.insert(
                            conversation_id,
                            ClaudeSessionEntry::new(
                                handle.clone(),
                                conversation_id,
                                config.session_signature.clone(),
                                config.model.clone(),
                                config.permission_mode.clone(),
                                config.effort.clone(),
                            ),
                        );
                        handle
                    }
                };
                AgentSessionHandle::Claude(handle)
            }
            Self::Acp(acp_config) => {
                let acp_session_state = app_handle.state::<AcpSessionState>();
                let session_handle = {
                    let mut sessions = acp_session_state.sessions.lock().await;
                    if let Some(handle) = sessions.get_mut(&conversation_id)
                        .and_then(|entry| entry.reuse_handle(&acp_config.session_signature)) {
                        handle
                    } else {
                        if sessions.contains_key(&conversation_id) {
                            info!(
                                conversation_id,
                                "ACP session config changed; replacing existing session"
                            );
                        }
                        let handle = spawn_acp_session_task(
                            app_handle.clone(),
                            conversation_id,
                            acp_config.clone(),
                        );
                        sessions.insert(
                            conversation_id,
                            crate::api::ai::acp::AcpSessionEntry::new(
                                handle.clone(),
                                conversation_id,
                                acp_config.session_signature.clone(),
                            ),
                        );
                        handle
                    }
                };

                AgentSessionHandle::Acp(session_handle)
            }
        }
    }
}

/// Prepare the selected native driver, then dispatch through one shared control
/// boundary. Provider-specific signatures/MCP are finalized before actor reuse.
pub(crate) async fn send_turn(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    request: AgentTurnRequest<'_>,
) -> Result<AiResponse, AppError> {
    let prepared = prepare_config(app_handle.clone(), &request).await?;
    let kind = prepared.kind();
    dispatch_prepared(
        &DesktopTurnEffects { app_handle, window }, prepared, kind,
        TurnInput {
            conversation_id: request.conversation_id,
            prompt: request.prompt,
            display_prompt: request.display_prompt,
            messages: request.messages,
        },
    ).await
}

struct DesktopTurnEffects {
    app_handle: tauri::AppHandle,
    window: tauri::Window,
}

#[async_trait::async_trait]
impl TurnEffects for DesktopTurnEffects {
    type Prepared = PreparedAgentSession;
    type Handle = AgentSessionHandle;

    fn create_response(&self, conversation_id: i64, kind: AgentSessionKind) -> Result<Message, AppError> {
        // This is the same response-only repository write as add_message. User
        // message summary invalidation is deliberately not part of this path.
        ConversationDatabase::new(&self.app_handle)?
            .message_repo()?.create(&response_message(conversation_id, kind)).map_err(AppError::from)
    }

    fn emit(&self, conversation_id: i64, event: ConversationEvent) {
        let _ = self.window.emit(format!("conversation_event_{conversation_id}").as_str(), event);
    }

    async fn acquire(&self, prepared: PreparedAgentSession, conversation_id: i64) -> AgentSessionHandle {
        prepared.acquire(&self.app_handle, conversation_id).await
    }

    fn submit(&self, handle: &AgentSessionHandle, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>) -> Result<(), AppError> {
        handle.send_prompt(message_id, prompt, attachments, self.window.clone())
    }

    async fn fail(&self, conversation_id: i64, message_id: i64, kind: AgentSessionKind, detail: &str) {
        super::failure::emit_agent_failure(
            &self.app_handle, conversation_id, message_id, &self.window, detail, provider_name(kind),
        );
        self.app_handle.state::<ConversationActivityManager>()
            .clear_focus(&self.app_handle, conversation_id).await;
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;

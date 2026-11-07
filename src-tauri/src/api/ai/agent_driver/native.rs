//! Provider-specific preparation behind one AIPP agent turn entry point.
//! Session ownership, persistence, events and protocol implementations stay in the
//! existing native adapters. There is deliberately no second process/session pool.
use super::session::{AgentSessionHandle, AgentSessionKind};
use crate::api::ai::acp::{AcpConfig, AcpSessionConfigChoicePayload, apply_network_proxy_to_env_vars, extract_acp_config, refresh_acp_config_signature, spawn_agent_idle_reaper_once, spawn_acp_session_task};
use crate::api::ai::codex_app_server::{CodexAppServerConfig, extract_codex_app_server_config, refresh_codex_session_signature, spawn_codex_session_task, CodexSessionEntry, CODEX_APPROVAL_POLICIES, CODEX_SANDBOX_MODES};
use crate::api::ai::claude_sdk::{ClaudeSdkConfig, claude_model_choices, extract_claude_sdk_config, prepare_claude_mcp_bridge, refresh_claude_session_signature, spawn_claude_session_task, ClaudeSessionEntry};
use crate::api::ai::config::get_network_proxy_from_config;
use crate::api::assistant_api::{AssistantDetail, resolve_acp_provider_id, CLAUDE_CODE_DEFAULT_MODEL};
use super::turn::AgentTurnRequest;
use super::registry::{AgentDriver, AgentDriverMetadata, AgentDriverRegistry, AgentInstanceConfig, AgentSessionLookup, AgentConfigField, AgentConfigValueType};
use super::capabilities::{AgentCapabilities, AgentCapabilityDeclaration};
use std::sync::Arc;
use crate::db::llm_db::{LLMDatabase, LLMProviderConfig};
#[cfg(test)]
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use crate::{AcpSessionState, ClaudeSessionState, CodexSessionState};
#[cfg(test)]
use std::collections::HashMap;
use tauri::Manager;
use tracing::{info, warn};

struct SelectedAgentProvider {
    provider_id: i64,
    kind: AgentSessionKind,
    model_override: Option<String>,
    configs: Vec<LLMProviderConfig>,
}

/// Validate the explicit selection before opening the provider database. A
/// nonblank malformed override must never become a request for the assistant's
/// default. Blank values retain the existing no-override compatibility behavior.
#[cfg(test)]
fn load_agent_provider(
    assistant: &AssistantDetail,
    override_model_id: Option<&str>,
    load: impl FnOnce(i64) -> Result<(String, Vec<LLMProviderConfig>), AppError>,
) -> Result<SelectedAgentProvider, AppError> {
    let registry = AgentDriverRegistry::default();
    register_builtin_drivers(&registry)?;
    let (instance, model_override) = super::turn::load_agent_instance(&registry, assistant, override_model_id, |provider_id| {
        let (api_type, mut configs) = load(provider_id)?;
        // Older pure-preparation fixtures did not model provider row ownership.
        // Normalize fixture rows here; ownership rejection has registry tests.
        for config in &mut configs { config.llm_provider_id = provider_id; }
        Ok((api_type, configs))
    })?;
    let kind = AgentSessionKind::from_provider(&instance.driver_id, &instance.configs)?;
    Ok(SelectedAgentProvider { provider_id: instance.provider_id, kind, model_override, configs: instance.configs })
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
        codex_config.instance_id = Some(selected.provider_id);
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
        config.instance_id = Some(selected.provider_id);
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

    acp_config.instance_id = Some(selected.provider_id);
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
                let run_id = handle.run_id().to_string();
                AgentSessionHandle::new(handle, run_id)
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
                let run_id = handle.run_id().to_string();
                AgentSessionHandle::new(handle, run_id)
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

                let run_id = session_handle.run_id().to_string();
                AgentSessionHandle::new(session_handle, run_id)
            }
        }
    }
}


/// The closed native preparation enum is private to these three existing
/// adapters. A registered extension returns its own PreparedAgentSession trait.
struct BuiltinDriver(AgentSessionKind);

pub(super) fn register_builtin_drivers(registry: &AgentDriverRegistry) -> Result<(), AppError> {
    for kind in [AgentSessionKind::Acp, AgentSessionKind::Codex, AgentSessionKind::Claude] {
        registry.register(Arc::new(BuiltinDriver(kind)))?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl AgentDriver for BuiltinDriver {
    fn metadata(&self) -> AgentDriverMetadata { self.0.into() }
    fn matches_provider(&self, api_type: &str, configs: &[LLMProviderConfig]) -> bool {
        AgentSessionKind::from_provider(api_type, configs).ok() == Some(self.0)
    }
    fn capabilities(&self) -> AgentCapabilities {
        let declaration = match self.0 {
            AgentSessionKind::Acp => AgentCapabilityDeclaration::acp(),
            AgentSessionKind::Codex => AgentCapabilityDeclaration::codex(),
            AgentSessionKind::Claude => AgentCapabilityDeclaration::claude(),
        };
        AgentCapabilities::baseline(self.metadata().id, declaration)
    }
    fn config_schema(&self) -> Vec<AgentConfigField> {
        let string = |name: &str| AgentConfigField { name: name.into(), required: false, value_type: AgentConfigValueType::String };
        let choice = |name: &str, values: &[&str]| AgentConfigField { name: name.into(), required: false, value_type: AgentConfigValueType::Choice(values.iter().map(|value| (*value).into()).collect()) };
        let mut fields = vec![string("acp_working_directory"), string("acp_env_vars")];
        match self.0 {
            AgentSessionKind::Acp => fields.extend([
                string("acp_cli_command"), string("acp_additional_args"),
                AgentConfigField { name: "proxy_enabled".into(), required: false, value_type: AgentConfigValueType::Boolean },
            ]),
            AgentSessionKind::Codex => fields.extend([
                string("codex_cli_command"), string("codex_additional_args"),
                choice("codex_approval_policy", CODEX_APPROVAL_POLICIES),
                choice("codex_sandbox", CODEX_SANDBOX_MODES),
                choice("codex_approvals_reviewer", crate::api::ai::codex_app_server::CODEX_APPROVALS_REVIEWERS),
            ]),
            AgentSessionKind::Claude => fields.extend([
                string("claude_cli_command"),
                choice("claude_permission_mode", &["default", "plan"]),
                choice("claude_effort", &["default", "low", "medium", "high", "max"]),
            ]),
        }
        fields
    }
    fn validate_config(&self, _instance: &AgentInstanceConfig) -> Result<(), AppError> {
        // Existing extractors normalize persisted values and apply assistant
        // overrides before validating the effective configuration. Validating
        // lower-priority raw defaults here would change that contract.
        Ok(())
    }
    fn prepare(&self, request: &AgentTurnRequest<'_>, instance: &AgentInstanceConfig, model_override: Option<&str>) -> Result<Box<dyn super::registry::PreparedAgentSession>, AppError> {
        if self.0 == AgentSessionKind::Claude && resolve_acp_provider_id(&request.assistant.model, &request.assistant.model_configs).is_none() {
            return Err(AppError::UnknownError("Claude Code 助手没有配置模型提供商".into()));
        }
        let selected = SelectedAgentProvider {
            provider_id: instance.provider_id, kind: self.0,
            model_override: model_override.map(str::to_string), configs: instance.configs.clone(),
        };
        Ok(Box::new(prepare_selected_config(request, &selected, Vec::new())?))
    }
    async fn warmup(&self, app: &tauri::AppHandle, window: tauri::Window, request: &AgentTurnRequest<'_>, instance: &AgentInstanceConfig) -> Result<Option<AgentSessionLookup>, AppError> {
        // Preserve the existing ACP eager handshake. Codex and Claude remain
        // demand-driven; no default actor can replace a per-message override.
        if self.0 != AgentSessionKind::Acp { return Ok(None); }
        let mut prepared = self.prepare(request, instance, None)?;
        let payload = crate::api::ai::acp::build_selected_mcp_tools_payload(app, request.assistant.assistant.id, Some(request.conversation_id)).map_err(AppError::UnknownError)?;
        prepared.finalize(app, request.conversation_id, payload).await?;
        let handle = prepared.acquire(app, request.conversation_id).await?;
        super::lifecycle::bind_session(app, super::lifecycle::AgentSessionBinding {
            conversation_id: request.conversation_id, provider_id: instance.provider_id,
            driver_id: instance.driver_id.clone(), config_fingerprint: instance.config_fingerprint.clone(), run_id: handle.run_id().into(),
        })?;
        let state = app.state::<AcpSessionState>();
        let native = {
            let sessions = state.sessions.lock().await;
            sessions.get(&request.conversation_id).filter(|entry| entry.run_id == handle.run_id()).map(|entry| entry.handle.clone())
                .ok_or_else(|| super::registry::routing_error("ACP warmup actor changed before start"))?
        };
        native.start(window).await?;
        self.lookup(app, request.conversation_id).await
    }
    async fn lookup(&self, app: &tauri::AppHandle, conversation_id: i64) -> Result<Option<AgentSessionLookup>, AppError> {
        macro_rules! lookup {
            ($state:ty) => {{
                let state = app.state::<$state>();
                let mut sessions = state.sessions.lock().await;
                sessions.get_mut(&conversation_id).map(|entry| {
                    entry.touch();
                    Ok(AgentSessionLookup {
                        handle: AgentSessionHandle::new(entry.handle.clone(), entry.run_id.clone()),
                        snapshot: serde_json::to_value(&entry.snapshot).map_err(|error| AppError::ProviderError(format!("{} session state serialization failed: {error}", self.metadata().id)))?,
                        run_id: entry.run_id.clone(), binding: None,
                    })
                }).transpose()
            }};
        }
        match self.0 {
            AgentSessionKind::Acp => lookup!(AcpSessionState),
            AgentSessionKind::Codex => lookup!(CodexSessionState),
            AgentSessionKind::Claude => lookup!(ClaudeSessionState),
        }
    }
}

#[async_trait::async_trait]
impl super::registry::PreparedAgentSession for PreparedAgentSession {
    fn metadata(&self) -> AgentDriverMetadata { self.kind().into() }
    async fn finalize(&mut self, app: &tauri::AppHandle, conversation_id: i64, payload: String) -> Result<(), AppError> {
        self.finalize_mcp_tools(payload);
        if let Self::Claude(config) = self {
            let provider_id = config.instance_id.ok_or_else(|| super::registry::routing_error("Claude configuration has no provider instance"))?;
            let database = LLMDatabase::new(app)?;
            config.model_choices = claude_model_choices(&database.get_llm_models(provider_id.to_string())?);
            drop(database);
            if let Some(model) = config.model.as_ref() {
                if !config.model_choices.iter().any(|choice| &choice.value == model) {
                    config.model_choices.insert(0, AcpSessionConfigChoicePayload { value: model.clone(), name: format!("{model}（当前模型）"), description: None, group_name: None });
                }
            }
            prepare_claude_mcp_bridge(app, conversation_id, config).await.map_err(|error| AppError::UnknownError(format!("Claude Code MCP bridge 初始化失败：{error}")))?;
        }
        Ok(())
    }
    async fn acquire(self: Box<Self>, host: &dyn super::registry::AgentSessionHost, conversation_id: i64) -> Result<AgentSessionHandle, AppError> {
        Ok((*self).acquire(host.app_handle()?, conversation_id).await)
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

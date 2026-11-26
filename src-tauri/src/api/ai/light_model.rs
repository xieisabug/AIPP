//! One-turn background text generation. Native selections keep the provider's own
//! protocol and account and run in a fresh directory without AIPP tools or history.
use super::{acp, agent_driver::native_agents, claude_sdk, codex_app_server, config};
use crate::api::{copilot_token_manager, genai_client};
use crate::db::{llm_db::{is_local_agent_api_type, LLMProvider, LLMProviderConfig, ModelDetail}, system_db::FeatureConfig};
use crate::errors::AppError;
use genai::chat::{ChatRequest, Usage};
use std::{collections::HashMap, time::Duration};
use tauri::AppHandle;

type Features = HashMap<String, HashMap<String, FeatureConfig>>;

pub(crate) enum LocalChannel {
    Codex(codex_app_server::CodexAppServerConfig),
    Claude(claude_sdk::ClaudeSdkConfig),
    Acp(acp::AcpConfig),
    Native(String, native_agents::NativeConfig),
}

impl LocalChannel {
    pub(crate) fn new(app: &AppHandle, provider: &LLMProvider, settings: &[LLMProviderConfig], features: &Features)
        -> Result<Self, AppError> {
        let kind = provider.api_type.as_str();
        if !is_local_agent_api_type(kind) {
            return Err(AppError::ProviderError(format!("提供商 {} 不是本机智能体", provider.name)));
        }
        let proxy = config::agent_network_proxy(settings, features);
        let configure_env = |env: &mut HashMap<String, String>| {
            if let Some(proxy) = &proxy { acp::apply_network_proxy_to_env_vars(env, proxy); }
        };
        if claude_sdk::is_claude_code_provider(kind, settings) {
            let mut c = claude_sdk::extract_claude_sdk_config(&[], settings, None, Vec::new())?;
            configure_env(&mut c.env_vars);
            return Ok(Self::Claude(c));
        }
        match kind {
            "codex_app_server" => {
                let mut c = codex_app_server::extract_codex_app_server_config(&[], settings, None)?;
                configure_env(&mut c.env_vars);
                Ok(Self::Codex(c))
            }
            "acp" => {
                let mut c = acp::extract_acp_config(&[], settings)?;
                configure_env(&mut c.env_vars);
                Ok(Self::Acp(c))
            }
            driver if native_agents::is_native_agent(driver) => {
                let mut c = native_agents::configuration(&[], settings, None)?;
                native_agents::configure_instance(app, provider.id, driver, &mut c)?;
                c.apply_proxy(driver, proxy.as_deref());
                Ok(Self::Native(driver.to_owned(), c))
            }
            _ => Err(AppError::ProviderError(format!("不支持的本地智能体 {kind}"))),
        }
    }

    pub(crate) async fn catalog(&self, app: &AppHandle, id: i64) -> Result<Vec<(String, String)>, String> {
        match self {
            Self::Codex(c) => Ok(codex_app_server::codex_model_catalog(app, c, true).await?.models
                .into_iter().map(|m| (m.id, m.name)).collect()),
            Self::Claude(c) => claude_sdk::onboarding_models(c, id).await,
            Self::Acp(c) => Ok(acp::probe_acp_session_config_options(c).await?.into_iter()
                .filter(|o| o.category.as_deref() == Some("model")).flat_map(|o| o.options)
                .map(|m| (m.value, m.name)).collect()),
            Self::Native(driver, c) => native_agents::probe_models(driver, c.clone()).await,
        }
    }

    pub(crate) async fn text(&self, model: Option<&str>, prompt: &str) -> Result<Reply, String> {
        let cwd = tempfile::tempdir().map_err(|e| format!("创建轻量模型工作目录失败：{e}"))?;
        let run = async {
            match self {
                Self::Codex(c) => {
                    let mut c = c.clone();
                    c.working_directory = cwd.path().to_owned();
                    codex_app_server::light_text(&c, model, prompt).await
                }
                Self::Claude(c) => {
                    let mut c = c.clone();
                    c.working_directory = cwd.path().to_owned();
                    if let Some(code) = model { c.model = Some(code.to_owned()); }
                    claude_sdk::light_text(c, prompt).await
                }
                Self::Acp(c) => {
                    let mut c = c.clone();
                    c.working_directory = cwd.path().to_owned();
                    acp::light_text(&c, model, prompt).await
                }
                Self::Native(driver, c) => native_agents::light_text(driver, c.clone(), cwd.path(), model, prompt).await,
            }
        };
        let label = model.unwrap_or("智能体默认模型");
        let reply = tokio::time::timeout(Duration::from_secs(150), run).await
            .map_err(|_| format!("等待模型 {label} 单轮响应超过 150 秒"))??;
        if reply.text.trim().is_empty() { return Err(format!("模型 {label} 未返回正文")); }
        Ok(reply)
    }
}

pub(crate) struct LightClient {
    model: String,
    provider: i64,
    channel: Channel,
}
enum Channel { Api(genai_client::Client), Local(LocalChannel) }
#[derive(Default)]
pub(crate) struct Reply { pub text: String, pub usage: Usage }

pub(super) fn reported_usage(input: i64, output: i64, read: Option<i64>, write: Option<i64>, thought: Option<i64>) -> Usage {
    let clamp = |count: i64| count.clamp(0, i32::MAX as i64) as i32;
    Usage {
        prompt_tokens: Some(clamp(input)), completion_tokens: Some(clamp(output)),
        total_tokens: Some(clamp(input.saturating_add(output))),
        prompt_tokens_details: Some(genai::chat::PromptTokensDetails {
            cached_tokens: read.map(clamp), cache_creation_tokens: write.map(clamp), ..Default::default()
        }),
        completion_tokens_details: Some(genai::chat::CompletionTokensDetails {
            reasoning_tokens: thought.map(clamp), ..Default::default()
        }),
    }
}

impl LightClient {
    pub(crate) async fn new(app: &AppHandle, conversation: Option<i64>, detail: &ModelDetail, features: &Features) -> Result<Self, AppError> {
        let kind = &detail.provider.api_type;
        let channel = if is_local_agent_api_type(kind) {
            Channel::Local(LocalChannel::new(app, &detail.provider, &detail.configs, features)?)
        } else {
            let proxy = config::get_network_proxy_from_config(features);
            let configs = copilot_token_manager::prepare_provider_configs(app, kind, &detail.configs, proxy.as_deref())
                .await.map_err(AppError::ProviderError)?;
            Channel::Api(genai_client::create_client_with_config(conversation.map(|id| (app, id)), &configs,
                &detail.model.code, kind, Some(&detail.model.request_mode), proxy.as_deref(), false,
                Some(config::get_request_timeout_from_config(features)), false, features)?)
        };
        Ok(Self { model: detail.model.code.clone(), provider: detail.provider.id, channel })
    }

    pub(crate) fn attempts(&self, configured: u32) -> u32 {
        if matches!(self.channel, Channel::Local(_)) { 1 } else { configured }
    }

    pub(crate) async fn complete(&self, request: ChatRequest) -> Result<Reply, String> {
        match &self.channel {
            Channel::Api(client) => {
                let response = client.exec_chat(&self.model, request, None).await.map_err(|e| e.to_string())?;
                Ok(Reply { text: response.first_text().unwrap_or_default().to_owned(), usage: response.usage })
            }
            Channel::Local(channel) => {
                let mut prompt = request.system.unwrap_or_default();
                for message in request.messages {
                    prompt.push_str(&format!("\n\n{:?}\n{}", message.role, message.content.joined_texts().unwrap_or_default()));
                }
                channel.text(Some(&self.model), &prompt).await.map_err(|e| format!("本机轻量模型 {}（提供商 #{}）：{e}", self.model, self.provider))
            }
        }
    }
}

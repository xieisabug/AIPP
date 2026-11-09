//! Typed control facade over the existing per-conversation session actors.
//!
//! Ownership, persistence, event delivery and process teardown stay with the
//! native drivers and their existing session maps. Cloning this facade only
//! clones a command sender; it does not create or reconnect a session.

use crate::api::ai::acp::AcpSessionHandle;
use crate::api::ai::claude_sdk::{
    is_claude_code_provider, ClaudeSessionHandle, CLAUDE_SDK_API_TYPE,
};
use crate::api::ai::codex_app_server::{CodexSessionHandle, CODEX_APP_SERVER_API_TYPE};
use crate::db::conversation_db::MessageAttachment;
use crate::db::llm_db::LLMProviderConfig;
use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSessionKind {
    Acp,
    Codex,
    Claude,
}

/// Local adapter operations, not claims about a remote agent's negotiated
/// capabilities. Configuration IDs/values are validated by the native actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSessionCapabilities {
    pub structured_attachments: bool,
    pub cancel_prompt: bool,
    pub set_config_option: bool,
}

impl AgentSessionKind {
    /// Resolve only known agent transports. Legacy Claude ACP configurations
    /// deliberately select native Claude, never an ACP fallback.
    pub fn from_provider(
        api_type: &str,
        configs: &[LLMProviderConfig],
    ) -> Result<Self, AppError> {
        if is_claude_code_provider(api_type, configs) {
            Ok(Self::Claude)
        } else if api_type == CODEX_APP_SERVER_API_TYPE {
            Ok(Self::Codex)
        } else if api_type == "acp" {
            Ok(Self::Acp)
        } else {
            Err(AppError::ProviderError(format!(
                "Agent driver routing failed: unsupported provider API type '{api_type}'; no fallback was attempted"
            )))
        }
    }

    pub fn api_type(self) -> &'static str {
        match self {
            Self::Acp => "acp",
            Self::Codex => CODEX_APP_SERVER_API_TYPE,
            Self::Claude => CLAUDE_SDK_API_TYPE,
        }
    }

    pub fn capabilities(self) -> AgentSessionCapabilities {
        AgentSessionCapabilities {
            structured_attachments: matches!(self, Self::Acp),
            cancel_prompt: true,
            set_config_option: true,
        }
    }

    #[cfg(test)]
    fn validate_attachments(self, attachment_count: usize, message_id: i64) -> Result<(), AppError> {
        if attachment_count > 0 && !self.capabilities().structured_attachments {
            return Err(AppError::ProviderError(format!(
                "{} send_prompt failed [message_id={}]: the native session handle does not accept structured attachments (received {}); attachments must be prepared in the runtime prompt before dispatch",
                self.api_type(), message_id, attachment_count
            )));
        }
        Ok(())
    }
}

/// Native controls obtain a desktop window only when their transport needs it.
/// A headless extension can consume the prompt without creating a UI runtime.
pub trait AgentPromptContext: Send + Sync {
    fn window(&self) -> Result<tauri::Window, AppError>;
}
impl AgentPromptContext for tauri::Window {
    fn window(&self) -> Result<tauri::Window, AppError> { Ok(self.clone()) }
}

/// Object-safe command adapter. Extensions implement this contract without
/// changing a central transport enum or owning another session/process pool.
#[async_trait::async_trait]
pub trait AgentSessionControl: Send + Sync {
    fn metadata(&self) -> super::registry::AgentDriverMetadata;
    fn send_prompt(&self, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>, context: &dyn AgentPromptContext) -> Result<(), AppError>;
    async fn cancel_current_prompt(&self) -> Result<(), AppError>;
    async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError>;
}

#[derive(Clone)]
pub struct AgentSessionHandle {
    control: std::sync::Arc<dyn AgentSessionControl>,
    run_id: String,
}

impl AgentSessionHandle {
    pub fn new(control: impl AgentSessionControl + 'static, run_id: String) -> Self {
        Self { control: std::sync::Arc::new(control), run_id }
    }

    pub fn metadata(&self) -> super::registry::AgentDriverMetadata { self.control.metadata() }
    pub fn run_id(&self) -> &str { &self.run_id }
    pub fn capabilities(&self) -> AgentSessionCapabilities { self.metadata().operations }

    pub fn send_prompt(&self, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>, window: tauri::Window) -> Result<(), AppError> {
        self.send_prompt_with_context(message_id, prompt, attachments, &window)
    }

    pub fn send_prompt_with_context(&self, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>, context: &dyn AgentPromptContext) -> Result<(), AppError> {
        if !attachments.is_empty() && !self.capabilities().structured_attachments {
            return Err(AppError::ProviderError(format!(
                "{} send_prompt failed [message_id={}]: the native session handle does not accept structured attachments (received {}); attachments must be prepared in the runtime prompt before dispatch",
                self.metadata().id, message_id, attachments.len()
            )));
        }
        self.control.send_prompt(message_id, prompt, attachments, context)
    }

    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        if !self.capabilities().cancel_prompt {
            return Err(AppError::ProviderError(format!("driver '{}' does not support cancelling a prompt", self.metadata().id)));
        }
        self.control.cancel_current_prompt().await
    }

    pub async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> {
        if !self.capabilities().set_config_option {
            return Err(AppError::ProviderError(format!("driver '{}' does not support setting session configuration", self.metadata().id)));
        }
        self.control.set_config_option(config_id, value).await
    }
}

#[async_trait::async_trait]
impl AgentSessionControl for AcpSessionHandle {
    fn metadata(&self) -> super::registry::AgentDriverMetadata { AgentSessionKind::Acp.into() }
    fn send_prompt(&self, message_id: i64, prompt: String, attachments: Vec<MessageAttachment>, context: &dyn AgentPromptContext) -> Result<(), AppError> {
        AcpSessionHandle::send_prompt(self, message_id, prompt, attachments, context.window()?)
    }
    async fn cancel_current_prompt(&self) -> Result<(), AppError> { AcpSessionHandle::cancel_current_prompt(self).await }
    async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> { AcpSessionHandle::set_config_option(self, config_id, value).await }
}

macro_rules! native_text_session {
    ($handle:ty, $kind:expr) => {
        #[async_trait::async_trait]
        impl AgentSessionControl for $handle {
            fn metadata(&self) -> super::registry::AgentDriverMetadata { $kind.into() }
            fn send_prompt(&self, message_id: i64, prompt: String, _attachments: Vec<MessageAttachment>, context: &dyn AgentPromptContext) -> Result<(), AppError> {
                <$handle>::send_prompt(self, message_id, prompt, context.window()?)
            }
            async fn cancel_current_prompt(&self) -> Result<(), AppError> { <$handle>::cancel_current_prompt(self).await }
            async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> { <$handle>::set_config_option(self, config_id, value).await }
        }
    };
}
native_text_session!(CodexSessionHandle, AgentSessionKind::Codex);
native_text_session!(ClaudeSessionHandle, AgentSessionKind::Claude);

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

/// Test-only observations from real native command senders. No process, window,
/// database, or persisted session is created by these fixtures.
#[cfg(test)]
pub(crate) enum TestSessionCommand {
    Cancel {
        response: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetConfig {
        config_id: String,
        value: String,
        response: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}

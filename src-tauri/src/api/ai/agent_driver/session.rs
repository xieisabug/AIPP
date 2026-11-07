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

#[derive(Clone)]
pub enum AgentSessionHandle {
    Acp(AcpSessionHandle),
    Codex(CodexSessionHandle),
    Claude(ClaudeSessionHandle),
}

impl AgentSessionHandle {
    pub fn kind(&self) -> AgentSessionKind {
        match self {
            Self::Acp(_) => AgentSessionKind::Acp,
            Self::Codex(_) => AgentSessionKind::Codex,
            Self::Claude(_) => AgentSessionKind::Claude,
        }
    }

    pub fn capabilities(&self) -> AgentSessionCapabilities {
        self.kind().capabilities()
    }

    /// Queue a prompt on exactly the selected actor. Native Codex/Claude callers
    /// pass an empty attachment vector after preparing their runtime prompt;
    /// silently dropping structured attachments here would lose user input.
    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        attachments: Vec<MessageAttachment>,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        self.kind().validate_attachments(attachments.len(), message_id)?;
        match self {
            Self::Acp(handle) => handle.send_prompt(message_id, prompt, attachments, window),
            Self::Codex(handle) => handle.send_prompt(message_id, prompt, window),
            Self::Claude(handle) => handle.send_prompt(message_id, prompt, window),
        }
    }

    /// Interrupt the current turn without replacing or deleting its session.
    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        match self {
            Self::Acp(handle) => handle.cancel_current_prompt().await,
            Self::Codex(handle) => handle.cancel_current_prompt().await,
            Self::Claude(handle) => handle.cancel_current_prompt().await,
        }
    }

    pub async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> {
        match self {
            Self::Acp(handle) => handle.set_config_option(config_id, value).await,
            Self::Codex(handle) => handle.set_config_option(config_id, value).await,
            Self::Claude(handle) => handle.set_config_option(config_id, value).await,
        }
    }
}

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

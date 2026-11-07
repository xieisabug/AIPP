//! Driver declarations and live discovery. A local encoder/command is not proof
//! that the connected agent negotiated that feature. Unknown stays unknown.
use crate::api::ai::acp::AcpSessionConfigOptionPayload;
use crate::errors::AppError;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport { Supported, Unsupported, Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource { Adapter, Session, Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub support: CapabilitySupport,
    pub source: CapabilitySource,
}

impl Capability {
    fn known(supported: bool, source: CapabilitySource) -> Self {
        Self { support: if supported { CapabilitySupport::Supported } else { CapabilitySupport::Unsupported }, source }
    }

    fn optional(supported: Option<bool>, source: CapabilitySource) -> Self {
        supported.map(|value| Self::known(value, source)).unwrap_or(Self {
            support: CapabilitySupport::Unknown, source: CapabilitySource::Unknown,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentInput { Structured, PromptText }

/// Registered adapter contract, never a claim about an unconnected server.
/// Optional fields are deliberately unknown for third-party driver declarations.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AgentCapabilityDeclaration {
    pub send_prompt: bool,
    pub cancel_prompt: bool,
    pub set_config_option: bool,
    pub respond_permission: Option<bool>,
    pub attachment_input: AttachmentInput,
    pub image_input: Option<bool>,
    pub audio_input: Option<bool>,
    pub embedded_context_input: Option<bool>,
    pub load_session: Option<bool>,
    pub resume_session: Option<bool>,
    /// Whether the native driver constructs an exhaustive configuration catalog.
    /// ACP instead reports config_options_complete from its negotiated response.
    pub config_catalog_complete: bool,
}

impl AgentCapabilityDeclaration {
    pub fn unknown(cancel_prompt: bool, set_config_option: bool, structured_attachments: bool) -> Self {
        Self {
            send_prompt: true, cancel_prompt, set_config_option, respond_permission: None,
            attachment_input: if structured_attachments { AttachmentInput::Structured } else { AttachmentInput::PromptText },
            image_input: None, audio_input: None, embedded_context_input: None,
            load_session: None, resume_session: None, config_catalog_complete: false,
        }
    }

    pub fn acp() -> Self {
        Self {
            respond_permission: Some(true), image_input: Some(true), audio_input: Some(false),
            embedded_context_input: Some(true), ..Self::unknown(true, true, true)
        }
    }

    pub fn codex() -> Self {
        Self {
            respond_permission: Some(true), image_input: Some(false), audio_input: Some(false),
            embedded_context_input: Some(false), load_session: Some(false), resume_session: Some(true),
            config_catalog_complete: true, ..Self::unknown(true, true, false)
        }
    }

    pub fn claude() -> Self {
        Self {
            respond_permission: Some(true), image_input: Some(false), audio_input: Some(false),
            embedded_context_input: Some(false), load_session: Some(false), resume_session: Some(true),
            config_catalog_complete: true, ..Self::unknown(true, true, false)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDiscovery { Baseline, Session }

#[derive(Debug, Clone, Serialize)]
pub struct AgentOperationCapabilities {
    pub send_prompt: Capability,
    pub cancel_prompt: Capability,
    pub set_config_option: Capability,
    pub respond_permission: Capability,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAttachmentCapabilities {
    pub input: AttachmentInput,
    pub structured: Capability,
    pub image: Capability,
    pub audio: Capability,
    pub embedded_context: Capability,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResumeCapabilities {
    pub load: Capability,
    pub resume: Capability,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConfigurationCapabilities {
    pub source: CapabilitySource,
    pub complete: bool,
    /// Native IDs, categories and values are opaque, including empty strings.
    pub options: Vec<AcpSessionConfigOptionPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentCapabilities {
    pub schema_version: u32,
    pub driver_id: String,
    pub discovery: CapabilityDiscovery,
    pub adapter: AgentCapabilityDeclaration,
    pub operations: AgentOperationCapabilities,
    pub attachments: AgentAttachmentCapabilities,
    pub sessions: AgentResumeCapabilities,
    pub configuration: AgentConfigurationCapabilities,
}

#[derive(Debug, Clone, Copy)]
pub enum AgentOperation { SendPrompt, CancelPrompt, SetConfigOption, RespondPermission }

impl AgentOperation {
    fn name(self) -> &'static str {
        match self {
            Self::SendPrompt => "send_prompt", Self::CancelPrompt => "cancel_prompt",
            Self::SetConfigOption => "set_config_option", Self::RespondPermission => "respond_permission",
        }
    }
}

impl AgentCapabilities {
    pub fn baseline(driver_id: &str, adapter: AgentCapabilityDeclaration) -> Self {
        let adapter_fact = |value| Capability::known(value, CapabilitySource::Adapter);
        // Encoding support alone cannot prove that the remote agent accepts it.
        let attachment = |supported| match supported {
            Some(false) => adapter_fact(false),
            _ => Capability::optional(None, CapabilitySource::Unknown),
        };
        Self {
            schema_version: 1, driver_id: driver_id.to_string(), discovery: CapabilityDiscovery::Baseline,
            operations: AgentOperationCapabilities {
                send_prompt: adapter_fact(adapter.send_prompt), cancel_prompt: adapter_fact(adapter.cancel_prompt),
                set_config_option: adapter_fact(adapter.set_config_option),
                respond_permission: Capability::optional(adapter.respond_permission, CapabilitySource::Adapter),
            },
            attachments: AgentAttachmentCapabilities {
                input: adapter.attachment_input,
                structured: adapter_fact(adapter.attachment_input == AttachmentInput::Structured),
                image: attachment(adapter.image_input), audio: attachment(adapter.audio_input),
                embedded_context: attachment(adapter.embedded_context_input),
            },
            sessions: AgentResumeCapabilities {
                load: Capability::optional(adapter.load_session, CapabilitySource::Adapter),
                resume: Capability::optional(adapter.resume_session, CapabilitySource::Adapter),
            },
            configuration: AgentConfigurationCapabilities {
                source: CapabilitySource::Unknown, complete: false, options: Vec::new(),
            },
            adapter,
        }
    }

    /// Pure projection of this instance's current native snapshot. No global
    /// cache, process spawn, provider switching, or model-name allowlist.
    pub fn with_session(&self, snapshot: &Value) -> Result<Self, AppError> {
        let object = snapshot.as_object().ok_or_else(|| self.invalid("snapshot must be an object"))?;
        // Start from baseline each time so missing fields cannot retain a previous
        // provider/session's negotiated capability or configuration catalog.
        let mut result = Self::baseline(&self.driver_id, self.adapter);
        let session_id = match object.get("session_id") {
            Some(Value::String(value)) => Some(value.as_str()),
            Some(Value::Null) | None => None,
            _ => return Err(self.invalid("session_id must be a string or null")),
        };
        if session_id.is_none() {
            return Ok(result);
        }
        result.discovery = CapabilityDiscovery::Session;
        result.sessions.load = self.session_fact(snapshot, "load_session_supported", self.adapter.load_session)?;
        result.sessions.resume = self.session_fact(snapshot, "session_resume_supported", self.adapter.resume_session)?;
        if let Some(prompt) = object.get("prompt_capabilities") {
            if !prompt.is_object() {
                return Err(self.invalid("prompt_capabilities must be an object"));
            }
            result.attachments.image = self.negotiated_input(prompt, "image", self.adapter.image_input)?;
            result.attachments.audio = self.negotiated_input(prompt, "audio", self.adapter.audio_input)?;
            result.attachments.embedded_context = self.negotiated_input(prompt, "embedded_context", self.adapter.embedded_context_input)?;
        }
        if let Some(options) = object.get("config_options") {
            result.configuration.options = serde_json::from_value(options.clone())
                .map_err(|error| self.invalid(&format!("config_options: {error}")))?;
            result.configuration.source = CapabilitySource::Session;
            result.configuration.complete = self.adapter.config_catalog_complete
                || self.bool_field(snapshot, "config_options_complete")? == Some(true);
            // A complete empty catalog proves config mutation unavailable; an
            // absent/partial catalog must not be interpreted as unsupported.
            if self.adapter.set_config_option && result.configuration.complete {
                result.operations.set_config_option = Capability::known(
                    !result.configuration.options.is_empty(), CapabilitySource::Session,
                );
            }
        }
        Ok(result)
    }

    /// Additive wire contract: all legacy native fields retain their exact value.
    pub fn enrich_snapshot(&self, snapshot: Value) -> Result<Value, AppError> {
        self.with_session(&snapshot)?.attach_to_snapshot(snapshot)
    }

    /// Attach a driver's already-discovered result, including custom discovery
    /// overrides, without replacing it with the generic snapshot projection.
    pub fn attach_to_snapshot(&self, mut snapshot: Value) -> Result<Value, AppError> {
        let payload = serde_json::to_value(self)
            .map_err(|error| self.invalid(&format!("capability serialization: {error}")))?;
        snapshot.as_object_mut().ok_or_else(|| self.invalid("snapshot must be an object"))?
            .insert("agent_capabilities".to_string(), payload);
        Ok(snapshot)
    }

    pub fn require_operation(&self, operation: AgentOperation) -> Result<(), AppError> {
        let capability = match operation {
            AgentOperation::SendPrompt => self.operations.send_prompt,
            AgentOperation::CancelPrompt => self.operations.cancel_prompt,
            AgentOperation::SetConfigOption => self.operations.set_config_option,
            AgentOperation::RespondPermission => self.operations.respond_permission,
        };
        if capability.support == CapabilitySupport::Unsupported {
            return Err(AppError::ProviderError(format!(
                "{} {} is unsupported by this agent driver/session; no fallback was attempted",
                self.driver_id, operation.name(),
            )));
        }
        Ok(())
    }

    /// For actor-time validation against a current catalog, or UI preflight only.
    /// Unknown/partial catalogs defer to the native protocol's own validation.
    pub fn validate_config_option(&self, config_id: &str, value: &str) -> Result<(), AppError> {
        self.require_operation(AgentOperation::SetConfigOption)?;
        let option = self.configuration.options.iter().find(|option| option.id == config_id);
        match option {
            Some(option) if option.current_value == value || option.options.iter().any(|choice| choice.value == value) => Ok(()),
            Some(_) => Err(AppError::ProviderError(format!(
                "{} set_config_option failed: value {value:?} is not advertised for config {config_id:?}", self.driver_id,
            ))),
            None if self.configuration.complete => Err(AppError::ProviderError(format!(
                "{} set_config_option failed: config {config_id:?} is unsupported by the current session catalog", self.driver_id,
            ))),
            None => Ok(()),
        }
    }

    fn session_fact(&self, snapshot: &Value, key: &str, baseline: Option<bool>) -> Result<Capability, AppError> {
        if baseline == Some(false) {
            return Ok(Capability::known(false, CapabilitySource::Adapter));
        }
        Ok(match self.bool_field(snapshot, key)? {
            Some(value) => Capability::known(value, CapabilitySource::Session),
            None => Capability::optional(baseline, CapabilitySource::Adapter),
        })
    }

    fn negotiated_input(&self, prompt: &Value, key: &str, encoder: Option<bool>) -> Result<Capability, AppError> {
        let remote = self.bool_field(prompt, key)?;
        Ok(match (encoder, remote) {
            (Some(false), _) => Capability::known(false, CapabilitySource::Adapter),
            (_, Some(false)) => Capability::known(false, CapabilitySource::Session),
            (Some(true), Some(true)) => Capability::known(true, CapabilitySource::Session),
            _ => Capability::optional(None, CapabilitySource::Unknown),
        })
    }

    fn bool_field(&self, value: &Value, key: &str) -> Result<Option<bool>, AppError> {
        match value.get(key) {
            None => Ok(None),
            Some(Value::Bool(value)) => Ok(Some(*value)),
            _ => Err(self.invalid(&format!("{key} must be a boolean"))),
        }
    }

    fn invalid(&self, detail: &str) -> AppError {
        AppError::ProviderError(format!("{} capability discovery failed: {detail}", self.driver_id))
    }
}


/// Shared wire projection for native push events. None retains the existing
/// { state: null } teardown contract; a projection failure is never fabricated.
pub fn snapshot_event_payload<T: Serialize>(
    driver_id: &str,
    declaration: AgentCapabilityDeclaration,
    state: Option<&T>,
) -> Result<Value, AppError> {
    let baseline = AgentCapabilities::baseline(driver_id, declaration);
    let state = state.map(|state| {
        let snapshot = serde_json::to_value(state)
            .map_err(|error| baseline.invalid(&format!("native snapshot serialization: {error}")))?;
        baseline.enrich_snapshot(snapshot)
    }).transpose()?;
    Ok(serde_json::json!({ "state": state }))
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod tests;

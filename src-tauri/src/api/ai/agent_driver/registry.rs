//! Driver registrations describe implementations; instances are snapshots of the
//! existing LLM provider rows. Neither registry owns processes or conversations.
use super::capabilities::{AgentCapabilities, AgentCapabilityDeclaration, AgentOperation};
use super::session::{AgentSessionCapabilities, AgentSessionHandle, AgentSessionKind};
use super::turn::AgentTurnRequest;
use crate::db::llm_db::LLMProviderConfig;
use crate::errors::AppError;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentDriverMetadata {
    pub id: &'static str,
    pub display_name: &'static str,
    pub message_model_name: &'static str,
    pub operations: AgentSessionCapabilities,
}

// Compatibility metadata for built-ins only. The registry/dispatch contracts
// take metadata directly, so another driver never needs another enum variant.
impl From<AgentSessionKind> for AgentDriverMetadata {
    fn from(kind: AgentSessionKind) -> Self {
        let (display_name, message_model_name) = match kind {
            AgentSessionKind::Acp => ("ACP", "acp"),
            AgentSessionKind::Codex => ("Codex", "codex-app-server"),
            AgentSessionKind::Claude => ("Claude Code", "claude-code"),
        };
        Self { id: kind.api_type(), display_name, message_model_name, operations: kind.capabilities() }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConfigField {
    pub name: String,
    pub required: bool,
    pub value_type: AgentConfigValueType,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "values", rename_all = "snake_case")]
pub enum AgentConfigValueType { String, Boolean, Choice(Vec<String>) }

#[derive(Debug, Clone)]
pub struct AgentInstanceConfig {
    /// The existing llm_provider.id, not a new persistent instance identifier.
    pub provider_id: i64,
    pub driver_id: String,
    pub config_fingerprint: String,
    pub configs: Vec<LLMProviderConfig>,
}

impl AgentInstanceConfig {
    pub fn new(provider_id: i64, driver_id: &str, configs: Vec<LLMProviderConfig>) -> Result<Self, AppError> {
        if provider_id <= 0 {
            return Err(routing_error(format!("invalid provider instance ID {provider_id}")));
        }
        let mut names = HashSet::new();
        for config in &configs {
            if config.llm_provider_id != provider_id {
                return Err(routing_error(format!("provider instance {provider_id} received config '{}' owned by provider {}", config.name, config.llm_provider_id)));
            }
            if !names.insert(config.name.as_str()) {
                return Err(routing_error(format!("provider instance {provider_id} has duplicate config '{}'", config.name)));
            }
        }
        let mut canonical = configs.iter().map(|config| (&config.name, &config.value, &config.append_location, config.is_addition)).collect::<Vec<_>>();
        canonical.sort();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        provider_id.hash(&mut hasher);
        driver_id.hash(&mut hasher);
        canonical.hash(&mut hasher);
        let config_fingerprint = format!("{:016x}", hasher.finish());
        Ok(Self { provider_id, driver_id: driver_id.into(), config_fingerprint, configs })
    }
}

/// Desktop access for built-in actors. A headless registered driver may not
/// require a desktop at all; contract tests can supply that same host boundary.
pub trait AgentSessionHost: Sync {
    fn app_handle(&self) -> Result<&tauri::AppHandle, AppError>;
}
impl AgentSessionHost for tauri::AppHandle {
    fn app_handle(&self) -> Result<&tauri::AppHandle, AppError> { Ok(self) }
}

#[async_trait::async_trait]
pub trait PreparedAgentSession: Send {
    fn metadata(&self) -> AgentDriverMetadata;
    async fn finalize(&mut self, app: &tauri::AppHandle, conversation_id: i64, payload: String) -> Result<(), AppError>;
    async fn acquire(self: Box<Self>, host: &dyn AgentSessionHost, conversation_id: i64) -> Result<AgentSessionHandle, AppError>;
}

#[derive(Clone)]
pub struct AgentSessionLookup {
    pub handle: AgentSessionHandle,
    pub snapshot: serde_json::Value,
    pub run_id: String,
    pub binding: Option<super::lifecycle::AgentSessionBinding>,
}

#[async_trait::async_trait]
pub trait AgentDriver: Send + Sync {
    fn metadata(&self) -> AgentDriverMetadata;
    fn matches_provider(&self, api_type: &str, configs: &[LLMProviderConfig]) -> bool;
    fn config_schema(&self) -> Vec<AgentConfigField> { Vec::new() }
    fn validate_config(&self, instance: &AgentInstanceConfig) -> Result<(), AppError> {
        validate_schema(instance, &self.config_schema())
    }
    fn prepare(&self, request: &AgentTurnRequest<'_>, instance: &AgentInstanceConfig, model_override: Option<&str>) -> Result<Box<dyn PreparedAgentSession>, AppError>;
    async fn lookup(&self, app: &tauri::AppHandle, conversation_id: i64) -> Result<Option<AgentSessionLookup>, AppError>;
    /// Eager connection is optional; demand-driven drivers return no session.
    /// Callers hold the shared conversation acquisition gate and have confirmed
    /// no authoritative actor exists before invoking this extension point.
    async fn warmup(&self, _app: &tauri::AppHandle, _window: tauri::Window, _request: &AgentTurnRequest<'_>, _instance: &AgentInstanceConfig) -> Result<Option<AgentSessionLookup>, AppError> { Ok(None) }
    fn capabilities(&self) -> AgentCapabilities {
        let metadata = self.metadata();
        AgentCapabilities::baseline(metadata.id, AgentCapabilityDeclaration::unknown(
            metadata.operations.cancel_prompt, metadata.operations.set_config_option, metadata.operations.structured_attachments,
        ))
    }
    fn session_capabilities(&self, snapshot: &serde_json::Value) -> Result<AgentCapabilities, AppError> {
        self.capabilities().with_session(snapshot)
    }
}

pub fn validate_schema(instance: &AgentInstanceConfig, schema: &[AgentConfigField]) -> Result<(), AppError> {
    for field in schema {
        let value = instance.configs.iter().find(|config| config.name == field.name).map(|config| config.value.as_str());
        if field.required && value.is_none_or(|value| value.trim().is_empty()) {
            return Err(routing_error(format!("driver '{}' provider instance {} requires config '{}'", instance.driver_id, instance.provider_id, field.name)));
        }
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            let valid = match &field.value_type {
                AgentConfigValueType::String => true,
                AgentConfigValueType::Boolean => matches!(value, "true" | "false"),
                AgentConfigValueType::Choice(choices) => choices.iter().any(|choice| choice == value),
            };
            if !valid {
                return Err(routing_error(format!("driver '{}' provider instance {} has invalid value for config '{}'", instance.driver_id, instance.provider_id, field.name)));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct AgentDriverRegistry {
    drivers: RwLock<HashMap<String, Arc<dyn AgentDriver>>>,
    instances: RwLock<HashMap<i64, AgentInstanceConfig>>,
}

impl AgentDriverRegistry {
    pub fn register(&self, driver: Arc<dyn AgentDriver>) -> Result<(), AppError> {
        let id = driver.metadata().id;
        if id.trim().is_empty() { return Err(routing_error("driver ID is empty")); }
        let mut drivers = self.drivers.write().map_err(|_| routing_error("driver registry lock poisoned"))?;
        if drivers.contains_key(id) { return Err(routing_error(format!("driver '{id}' is already registered"))); }
        drivers.insert(id.into(), driver);
        Ok(())
    }

    pub fn driver(&self, id: &str) -> Result<Arc<dyn AgentDriver>, AppError> {
        self.drivers.read().map_err(|_| routing_error("driver registry lock poisoned"))?
            .get(id).cloned().ok_or_else(|| routing_error(format!("unknown driver '{id}'")))
    }

    pub fn drivers(&self) -> Result<Vec<Arc<dyn AgentDriver>>, AppError> {
        let mut drivers = self.drivers.read().map_err(|_| routing_error("driver registry lock poisoned"))?
            .values().cloned().collect::<Vec<_>>();
        drivers.sort_by_key(|driver| driver.metadata().id);
        Ok(drivers)
    }

    pub fn register_instance(&self, instance: AgentInstanceConfig) -> Result<(), AppError> {
        self.driver(&instance.driver_id)?.validate_config(&instance)?;
        let mut instances = self.instances.write().map_err(|_| routing_error("instance registry lock poisoned"))?;
        if instances.contains_key(&instance.provider_id) {
            return Err(routing_error(format!("provider instance {} is already registered", instance.provider_id)));
        }
        instances.insert(instance.provider_id, instance);
        Ok(())
    }

    pub fn instance(&self, provider_id: i64) -> Result<AgentInstanceConfig, AppError> {
        self.instances.read().map_err(|_| routing_error("instance registry lock poisoned"))?
            .get(&provider_id).cloned().ok_or_else(|| routing_error(format!("unknown provider instance {provider_id}")))
    }

    /// Refresh from the authoritative provider database on every turn. Never
    /// retry another instance when matching or validation fails.
    pub fn resolve_provider(&self, provider_id: i64, api_type: &str, configs: Vec<LLMProviderConfig>) -> Result<AgentInstanceConfig, AppError> {
        let matches = self.drivers()?.into_iter().filter(|driver| driver.matches_provider(api_type, &configs)).collect::<Vec<_>>();
        let driver = match matches.as_slice() {
            [driver] => driver,
            [] => return Err(routing_error(format!("unsupported provider API type '{api_type}'"))),
            _ => return Err(routing_error(format!("ambiguous drivers for provider API type '{api_type}'"))),
        };
        let instance = AgentInstanceConfig::new(provider_id, driver.metadata().id, configs)?;
        driver.validate_config(&instance)?;
        self.instances.write().map_err(|_| routing_error("instance registry lock poisoned"))?.insert(provider_id, instance.clone());
        Ok(instance)
    }

    pub fn prepare(&self, instance: &AgentInstanceConfig, request: &AgentTurnRequest<'_>, model_override: Option<&str>) -> Result<Box<dyn PreparedAgentSession>, AppError> {
        let driver = self.driver(&instance.driver_id)?;
        driver.capabilities().require_operation(AgentOperation::SendPrompt)?;
        driver.validate_config(instance)?;
        let prepared = driver.prepare(request, instance, model_override)?;
        if prepared.metadata().id != instance.driver_id {
            return Err(routing_error(format!("driver '{}' prepared mismatched driver '{}'", instance.driver_id, prepared.metadata().id)));
        }
        Ok(prepared)
    }
}

pub fn registry() -> &'static AgentDriverRegistry {
    static REGISTRY: OnceLock<AgentDriverRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let registry = AgentDriverRegistry::default();
        super::native::register_builtin_drivers(&registry).expect("built-in driver IDs are unique");
        registry
    })
}

pub(super) fn routing_error(detail: impl std::fmt::Display) -> AppError {
    AppError::ProviderError(format!("Agent driver routing failed: {detail}; no fallback was attempted"))
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

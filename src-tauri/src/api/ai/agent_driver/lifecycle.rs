//! Routing metadata only. Native maps remain the sole owners of actors and
//! protocol snapshots, and ConversationDatabase remains the conversation store.
use super::registry::{registry, routing_error, AgentSessionLookup};
use crate::errors::AppError;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tauri::Manager;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSessionBinding {
    pub conversation_id: i64,
    pub provider_id: i64,
    pub driver_id: String,
    pub config_fingerprint: String,
    pub run_id: String,
}

#[derive(Default)]
pub struct AgentSessionBindings {
    bindings: Mutex<HashMap<i64, AgentSessionBinding>>,
    gates: Mutex<HashMap<i64, Weak<tokio::sync::Mutex<()>>>>,
}

impl AgentSessionBindings {
    pub fn bind(&self, binding: AgentSessionBinding) -> Result<(), AppError> {
        if binding.conversation_id <= 0 || binding.provider_id <= 0 || binding.run_id.is_empty() || binding.driver_id.is_empty() {
            return Err(routing_error("invalid native session binding identity"));
        }
        self.bindings.lock().map_err(|_| routing_error("session binding lock poisoned"))?.insert(binding.conversation_id, binding);
        Ok(())
    }

    pub fn publish_if_current(&self, conversation_id: i64, driver_id: &str, run_id: Option<&str>, publish: impl FnOnce()) -> Result<bool, AppError> {
        let bindings = self.bindings.lock().map_err(|_| routing_error("session binding lock poisoned"))?;
        if let Some(binding) = bindings.get(&conversation_id) {
            if binding.driver_id != driver_id || run_id != Some(binding.run_id.as_str()) { return Ok(false); }
        }
        publish();
        Ok(true)
    }

    pub fn get(&self, conversation_id: i64) -> Result<Option<AgentSessionBinding>, AppError> {
        Ok(self.bindings.lock().map_err(|_| routing_error("session binding lock poisoned"))?.get(&conversation_id).cloned())
    }
}

/// Serialize warmup and turn acquisition for one conversation. These are only
/// short-lived coordination locks, never another owner of native sessions.
pub async fn lock_conversation(app: &tauri::AppHandle, conversation_id: i64) -> Result<tokio::sync::OwnedMutexGuard<()>, AppError> {
    let state = app.try_state::<AgentSessionBindings>().ok_or_else(|| routing_error("session binding state is not initialized"))?;
    let gate = {
        let mut gates = state.gates.lock().map_err(|_| routing_error("session gate lock poisoned"))?;
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(&conversation_id).and_then(Weak::upgrade) { gate } else {
            let gate = Arc::new(tokio::sync::Mutex::new(()));
            gates.insert(conversation_id, Arc::downgrade(&gate));
            gate
        }
    };
    Ok(gate.lock_owned().await)
}

pub fn session_binding(app: &tauri::AppHandle, conversation_id: i64) -> Result<Option<AgentSessionBinding>, AppError> {
    app.try_state::<AgentSessionBindings>().ok_or_else(|| routing_error("session binding state is not initialized"))?.get(conversation_id)
}

/// Publish under the metadata lock so an old actor cannot race a replacement's
/// binding and clear or overwrite the current driver's UI snapshot.
pub fn publish_if_current_binding(app: &tauri::AppHandle, conversation_id: i64, driver_id: &str, run_id: Option<&str>, publish: impl FnOnce()) -> Result<bool, AppError> {
    let state = app.try_state::<AgentSessionBindings>().ok_or_else(|| routing_error("session binding state is not initialized"))?;
    state.publish_if_current(conversation_id, driver_id, run_id, publish)
}

pub fn bind_session(app: &tauri::AppHandle, binding: AgentSessionBinding) -> Result<(), AppError> {
    app.try_state::<AgentSessionBindings>().ok_or_else(|| routing_error("session binding state is not initialized"))?.bind(binding)
}

/// Shared by cancel, config, state and capability discovery. A recorded binding
/// selects exactly one driver, even when old entries remain in another map.
pub async fn lookup_session(app: &tauri::AppHandle, conversation_id: i64) -> Result<Option<AgentSessionLookup>, AppError> {
    let state = app.try_state::<AgentSessionBindings>().ok_or_else(|| routing_error("session binding state is not initialized"))?;
    let driver_ids = registry().drivers()?.iter().map(|driver| driver.metadata().id.to_string()).collect();
    lookup_registered_session(&state, conversation_id, driver_ids, |driver_id| async move {
        registry().driver(&driver_id)?.lookup(app, conversation_id).await
    }).await
}

/// Production selection algorithm, with only the native map read substituted
/// in headless tests. This verifies repeated lookups after an actor exits.
pub(super) async fn lookup_registered_session<F, Fut>(bindings: &AgentSessionBindings, conversation_id: i64, driver_ids: Vec<String>, mut lookup: F) -> Result<Option<AgentSessionLookup>, AppError>
where F: FnMut(String) -> Fut,
      Fut: std::future::Future<Output = Result<Option<AgentSessionLookup>, AppError>> {
    if let Some(binding) = bindings.get(conversation_id)? {
        let selected = lookup(binding.driver_id.clone()).await?;
        if bindings.get(conversation_id)?.as_ref() != Some(&binding) {
            return Err(routing_error(format!("conversation {conversation_id} binding changed during native session lookup")));
        }
        if selected.is_none() {
            // Retain the authoritative identity as a tombstone. Clearing it
            // would let a later lookup adopt a stale actor from another map.
            return Ok(None);
        }
        return validate_lookup(Some(&binding), selected);
    }
    let mut candidates = Vec::new();
    for driver_id in driver_ids {
        if let Some(session) = lookup(driver_id).await? { candidates.push(session); }
    }
    if bindings.get(conversation_id)?.is_some() {
        return Err(routing_error(format!("conversation {conversation_id} gained an authoritative binding during legacy lookup")));
    }
    resolve_unbound(candidates, conversation_id)
}

pub async fn lookup_instance_session(app: &tauri::AppHandle, conversation_id: i64, provider_id: i64, driver_id: &str) -> Result<Option<AgentSessionLookup>, AppError> {
    let binding = session_binding(app, conversation_id)?.ok_or_else(|| routing_error(format!("conversation {conversation_id} has no registered provider instance binding")))?;
    validate_instance(&binding, provider_id, driver_id)?;
    let session = lookup_session(app, conversation_id).await?;
    if let Some(session) = &session {
        let selected = session.binding.as_ref().ok_or_else(|| routing_error("selected session lost its provider instance binding"))?;
        validate_instance(selected, provider_id, driver_id)?;
    }
    Ok(session)
}

pub(super) fn validate_instance(binding: &AgentSessionBinding, provider_id: i64, driver_id: &str) -> Result<(), AppError> {
    if binding.provider_id != provider_id || binding.driver_id != driver_id {
        return Err(routing_error(format!("conversation {} belongs to driver '{}' provider instance {}, not driver '{driver_id}' instance {provider_id}", binding.conversation_id, binding.driver_id, binding.provider_id)));
    }
    Ok(())
}

pub(super) fn validate_lookup(binding: Option<&AgentSessionBinding>, selected: Option<AgentSessionLookup>) -> Result<Option<AgentSessionLookup>, AppError> {
    let Some(binding) = binding else { return Ok(selected); };
    let mut selected = selected.ok_or_else(|| routing_error(format!("stale session binding [conversation_id={}, driver={}, instance={}, run_id={}]: native actor is no longer present", binding.conversation_id, binding.driver_id, binding.provider_id, binding.run_id)))?;
    if selected.handle.metadata().id != binding.driver_id || selected.run_id != binding.run_id || selected.handle.run_id() != binding.run_id {
        return Err(routing_error(format!("stale or cross-driver native session [conversation_id={}, driver={}, instance={}, expected_run_id={}, actual_driver={}, actual_run_id={}]", binding.conversation_id, binding.driver_id, binding.provider_id, binding.run_id, selected.handle.metadata().id, selected.run_id)));
    }
    selected.binding = Some(binding.clone());
    Ok(Some(selected))
}

pub(super) fn resolve_unbound(mut candidates: Vec<AgentSessionLookup>, conversation_id: i64) -> Result<Option<AgentSessionLookup>, AppError> {
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.pop()),
        _ => Err(routing_error(format!("conversation {conversation_id} has multiple unbound native sessions; choose an explicit provider instance before controlling the session"))),
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

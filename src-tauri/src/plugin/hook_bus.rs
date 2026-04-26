use crate::api::plugin_api::{get_enabled_plugin_manifests, PluginHookContribution, ResolvedPluginManifest};
use crate::plugin::runtime::js_bridge::execute_js_hook;
use crate::db::plugin_db::{NewPluginHookAuditLog, PluginDatabase};
use crate::plugin::runtime::process::execute_process_hook;
use crate::plugin::runtime::wasm::execute_wasm_hook;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookKind {
    Event,
    Filter,
    Guard,
}

impl HookKind {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "filter" => Self::Filter,
            "guard" => Self::Guard,
            _ => Self::Event,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePolicy {
    Log,
    Block,
}

impl FailurePolicy {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "block" => Self::Block,
            _ => Self::Log,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HookAction {
    Continue,
    Replace,
    Patch,
    Block,
    ApprovalRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HookRuntimeResult {
    #[serde(default = "default_continue_action")]
    pub(crate) action: HookAction,
    #[serde(default)]
    pub(crate) context: Option<Value>,
    #[serde(default)]
    pub(crate) patch: Option<Value>,
    #[serde(default)]
    pub(crate) message: Option<String>,
    #[serde(default)]
    pub(crate) metadata: Value,
}

fn default_continue_action() -> HookAction {
    HookAction::Continue
}

impl Default for HookRuntimeResult {
    fn default() -> Self {
        Self {
            action: HookAction::Continue,
            context: None,
            patch: None,
            message: None,
            metadata: Value::Object(Default::default()),
        }
    }
}

#[derive(Debug, Clone)]
struct HookRegistration {
    manifest: ResolvedPluginManifest,
    hook: PluginHookContribution,
    kind: HookKind,
    failure_policy: FailurePolicy,
}

#[derive(Debug, Clone)]
pub struct HookRunResult {
    pub context: Value,
}

#[derive(Debug, Clone)]
pub struct PluginHookBus {
    app_handle: tauri::AppHandle,
}

impl PluginHookBus {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }

    pub async fn run_guard_filter(
        &self,
        hook_name: &str,
        context: Value,
    ) -> Result<HookRunResult, String> {
        let mut registrations = self.load_hook_registrations(hook_name)?;
        registrations.retain(|registration| registration.kind != HookKind::Event);
        registrations.sort_by(|a, b| {
            a.hook
                .priority
                .cmp(&b.hook.priority)
                .then_with(|| a.manifest.code.cmp(&b.manifest.code))
        });

        let mut current_context = context;
        for registration in registrations {
            let result = self.execute_registration(&registration, hook_name, &current_context).await;
            match result {
                Ok(runtime_result) => match runtime_result.action {
                    HookAction::Continue => {}
                    HookAction::Replace => {
                        current_context = runtime_result.context.ok_or_else(|| {
                            format!(
                                "Plugin {} returned replace without context for hook {}",
                                registration.manifest.code, hook_name
                            )
                        })?;
                    }
                    HookAction::Patch => {
                        let patch = runtime_result.patch.or(runtime_result.context).ok_or_else(|| {
                            format!(
                                "Plugin {} returned patch without context for hook {}",
                                registration.manifest.code, hook_name
                            )
                        })?;
                        merge_json_patch(&mut current_context, patch);
                    }
                    HookAction::Block => {
                        return Err(runtime_result.message.unwrap_or_else(|| {
                            format!(
                                "Plugin {} blocked hook {}",
                                registration.manifest.code, hook_name
                            )
                        }));
                    }
                    HookAction::ApprovalRequired => {
                        return Err(runtime_result.message.unwrap_or_else(|| {
                            format!(
                                "Plugin {} requested approval for hook {}, but plugin hook approval UI is not available yet",
                                registration.manifest.code, hook_name
                            )
                        }));
                    }
                },
                Err(error) => {
                    if registration.failure_policy == FailurePolicy::Block {
                        return Err(error);
                    }
                    warn!(
                        plugin_code = %registration.manifest.code,
                        hook_name,
                        error = %error,
                        "Plugin hook failed with log failure policy"
                    );
                }
            }
        }

        Ok(HookRunResult { context: current_context })
    }

    pub async fn emit_event(&self, hook_name: &str, context: Value) -> Result<(), String> {
        let mut registrations = self.load_hook_registrations(hook_name)?;
        registrations.retain(|registration| registration.kind == HookKind::Event);
        registrations.sort_by(|a, b| {
            a.hook
                .priority
                .cmp(&b.hook.priority)
                .then_with(|| a.manifest.code.cmp(&b.manifest.code))
        });

        let tasks = registrations.into_iter().map(|registration| {
            let bus = self.clone();
            let hook_name = hook_name.to_string();
            let context = context.clone();
            async move {
                if let Err(error) = bus.execute_registration(&registration, &hook_name, &context).await
                {
                    warn!(
                        plugin_code = %registration.manifest.code,
                        hook_name = %hook_name,
                        error = %error,
                        "Plugin event hook failed"
                    );
                }
            }
        });
        futures::future::join_all(tasks).await;

        Ok(())
    }

    fn load_hook_registrations(&self, hook_name: &str) -> Result<Vec<HookRegistration>, String> {
        let manifests = get_enabled_plugin_manifests(&self.app_handle)?;
        let mut registrations = Vec::new();

        for manifest in manifests {
            if !is_hook_activation_enabled(&manifest, hook_name) {
                continue;
            }
            for hook in manifest
                .contributions
                .hooks
                .iter()
                .filter(|hook| hook.is_active && hook.name == hook_name)
            {
                registrations.push(HookRegistration {
                    manifest: manifest.clone(),
                    hook: hook.clone(),
                    kind: HookKind::parse(&hook.kind),
                    failure_policy: FailurePolicy::parse(&hook.failure_policy),
                });
            }
        }

        debug!(hook_name, count = registrations.len(), "Loaded plugin hook registrations");
        Ok(registrations)
    }

    async fn execute_registration(
        &self,
        registration: &HookRegistration,
        hook_name: &str,
        context: &Value,
    ) -> Result<HookRuntimeResult, String> {
        let start = Instant::now();
        let plugin_id = registration.manifest.plugin_id.unwrap_or_default();
        let mut status = "success".to_string();
        let mut action = None;
        let mut error = None;

        let result = async {
            self.ensure_hook_permission(registration, hook_name)?;
            let timeout_ms = registration.hook.timeout_ms.max(1) as u64;
            timeout(
                Duration::from_millis(timeout_ms),
                self.dispatch_to_runtime(registration, hook_name, context),
            )
            .await
            .map_err(|_| {
                format!(
                    "Plugin {} hook {} timed out after {}ms",
                    registration.manifest.code, hook_name, timeout_ms
                )
            })?
        }
        .await;

        match &result {
            Ok(runtime_result) => {
                action = Some(format!("{:?}", runtime_result.action).to_ascii_lowercase());
                if matches!(
                    runtime_result.action,
                    HookAction::Block | HookAction::ApprovalRequired
                ) {
                    status = "blocked".to_string();
                }
            }
            Err(err) => {
                status = "failed".to_string();
                error = Some(err.clone());
            }
        }

        self.write_audit_log(
            plugin_id,
            hook_name,
            context,
            status,
            action,
            Some(start.elapsed().as_millis() as i64),
            error,
        );

        result
    }

    fn ensure_hook_permission(
        &self,
        registration: &HookRegistration,
        hook_name: &str,
    ) -> Result<(), String> {
        let required = format!("hook.{}", hook_name).to_ascii_lowercase();
        let has_permission = registration
            .manifest
            .permissions
            .iter()
            .any(|permission| permission.eq_ignore_ascii_case(&required));
        if has_permission {
            Ok(())
        } else {
            Err(format!(
                "Plugin {} registered hook {} without required permission {}",
                registration.manifest.code, hook_name, required
            ))
        }
    }

    async fn dispatch_to_runtime(
        &self,
        registration: &HookRegistration,
        hook_name: &str,
        context: &Value,
    ) -> Result<HookRuntimeResult, String> {
        match registration.manifest.runtime.runtime_type.as_str() {
            "mock" => Ok(HookRuntimeResult::default()),
            "js" => {
                execute_js_hook(
                    &self.app_handle,
                    &registration.manifest,
                    hook_name,
                    context,
                    registration.hook.timeout_ms.max(1) as u64,
                )
                .await
            }
            "wasm" => {
                let manifest = registration.manifest.clone();
                let hook_name = hook_name.to_string();
                let context = context.clone();
                tokio::task::spawn_blocking(move || {
                    execute_wasm_hook(&manifest, &hook_name, &context)
                })
                .await
                .map_err(|error| format!("WASM plugin task failed: {}", error))?
            }
            "process" => execute_process_hook(&registration.manifest, hook_name, context).await,
            "native" => Err(format!(
                "Native plugin runtime is not implemented yet for hook {} in plugin {}",
                hook_name, registration.manifest.code
            )),
            other => Err(format!(
                "Unsupported plugin runtime '{}' for hook {} in plugin {}",
                other, hook_name, registration.manifest.code
            )),
        }
    }

    fn write_audit_log(
        &self,
        plugin_id: i64,
        hook_name: &str,
        context: &Value,
        status: String,
        action: Option<String>,
        duration_ms: Option<i64>,
        error: Option<String>,
    ) {
        if plugin_id <= 0 {
            return;
        }

        let audit = NewPluginHookAuditLog {
            plugin_id,
            hook_name: hook_name.to_string(),
            conversation_id: extract_i64(context, "conversationId"),
            message_id: extract_i64(context, "messageId"),
            status,
            action,
            duration_ms,
            error,
        };

        match PluginDatabase::new(&self.app_handle)
            .and_then(|db| db.add_plugin_hook_audit_log(&audit))
        {
            Ok(_) => {}
            Err(err) => {
                warn!(
                    plugin_id,
                    hook_name,
                    error = %err,
                    "Failed to write plugin hook audit log"
                );
            }
        }
    }
}

fn extract_i64(context: &Value, key: &str) -> Option<i64> {
    context
        .get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse::<i64>().ok()))
}

fn is_hook_activation_enabled(manifest: &ResolvedPluginManifest, hook_name: &str) -> bool {
    if manifest.activation_events.is_empty() {
        return true;
    }
    let expected = format!("onHook:{}", hook_name);
    manifest
        .activation_events
        .iter()
        .any(|event| event.eq_ignore_ascii_case(&expected))
}

fn merge_json_patch(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(&key);
                } else {
                    merge_json_patch(target_map.entry(key).or_insert(Value::Null), value);
                }
            }
        }
        (target_slot, patch_value) => {
            *target_slot = patch_value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::merge_json_patch;
    use serde_json::json;

    #[test]
    fn test_merge_json_patch_updates_removes_and_replaces_values() {
        let mut target = json!({
            "prompt": "old",
            "metadata": {
                "keep": true,
                "remove": "yes"
            },
            "items": [1, 2]
        });

        merge_json_patch(
            &mut target,
            json!({
                "prompt": "new",
                "metadata": {
                    "remove": null,
                    "added": 42
                },
                "items": [3],
                "extra": "value"
            }),
        );

        assert_eq!(
            target,
            json!({
                "prompt": "new",
                "metadata": {
                    "keep": true,
                    "added": 42
                },
                "items": [3],
                "extra": "value"
            })
        );
    }
}

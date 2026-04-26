use crate::api::plugin_api::ResolvedPluginManifest;
use crate::plugin::hook_bus::HookRuntimeResult;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::Emitter;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};

type PendingSender = oneshot::Sender<Result<HookRuntimeResult, String>>;

static PENDING_JS_HOOKS: OnceLock<Mutex<HashMap<String, PendingSender>>> = OnceLock::new();

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct JsPluginHookRequest {
    request_id: String,
    plugin_code: String,
    hook_name: String,
    context: Value,
}

fn pending_hooks() -> &'static Mutex<HashMap<String, PendingSender>> {
    PENDING_JS_HOOKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) async fn execute_js_hook(
    app_handle: &tauri::AppHandle,
    manifest: &ResolvedPluginManifest,
    hook_name: &str,
    context: &Value,
    timeout_ms: u64,
) -> Result<HookRuntimeResult, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    {
        let mut pending = pending_hooks()
            .lock()
            .map_err(|_| "JS hook pending registry lock poisoned".to_string())?;
        pending.insert(request_id.clone(), sender);
    }

    let cleanup_request_id = request_id.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(timeout_ms.saturating_add(1000))).await;
        if let Ok(mut pending) = pending_hooks().lock() {
            pending.remove(&cleanup_request_id);
        }
    });

    let payload = JsPluginHookRequest {
        request_id: request_id.clone(),
        plugin_code: manifest.code.clone(),
        hook_name: hook_name.to_string(),
        context: context.clone(),
    };

    if let Err(error) = app_handle.emit("plugin_hook_request", payload) {
        if let Ok(mut pending) = pending_hooks().lock() {
            pending.remove(&request_id);
        }
        return Err(format!(
            "Failed to emit JS hook request for plugin {} hook {}: {}",
            manifest.code, hook_name, error
        ));
    }

    receiver.await.map_err(|_| {
        format!(
            "JS hook bridge for plugin {} hook {} was dropped before returning a result",
            manifest.code, hook_name
        )
    })?
}

pub(crate) fn submit_js_plugin_hook_result(
    request_id: String,
    result: Option<Value>,
    error: Option<String>,
) -> Result<(), String> {
    let sender = {
        let mut pending = pending_hooks()
            .lock()
            .map_err(|_| "JS hook pending registry lock poisoned".to_string())?;
        pending.remove(&request_id)
    };
    let Some(sender) = sender else {
        return Err(format!("Unknown or expired JS hook request '{}'", request_id));
    };

    let response = if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        Err(error)
    } else {
        let raw_result = result.unwrap_or_else(|| serde_json::json!({ "action": "continue" }));
        serde_json::from_value::<HookRuntimeResult>(raw_result)
            .map_err(|error| format!("Invalid JS hook result payload: {}", error))
    };

    sender
        .send(response)
        .map_err(|_| format!("JS hook request '{}' receiver is no longer active", request_id))
}

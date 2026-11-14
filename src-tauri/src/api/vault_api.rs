//! Tauri commands for the personal credential vault (凭据保险箱).
//!
//! Red line: no command here returns a secret value or any fragment of one. Secrets go in through
//! `vault_create` / `vault_replace_secret` and only come out inside the trusted executor.
use crate::sync::personal_vault::{self, AuthInput, Capability, CreateInput, ItemMeta, UpdateInput, UsageEntry};
use crate::vault::config_ref::{self, ConfigReference};
use tauri::{AppHandle, Emitter};

pub const VAULT_CHANGED_EVENT: &str = "vault_changed";

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    // Keyring access can block (e.g. a locked keychain prompt); keep it off the async runtime.
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("保险箱任务失败: {e}"))?
}

pub(crate) fn notify_changed(app: &AppHandle) {
    if let Err(error) = app.emit(VAULT_CHANGED_EVENT, "local") {
        tracing::warn!(%error, "Failed to broadcast vault_changed");
    }
}

#[tauri::command]
pub async fn vault_capability(app_handle: AppHandle) -> Result<Capability, String> {
    blocking(move || Ok(personal_vault::capability(&app_handle))).await
}

#[tauri::command]
pub async fn vault_list(app_handle: AppHandle) -> Result<Vec<ItemMeta>, String> {
    blocking(move || personal_vault::list(&app_handle)).await
}

#[tauri::command]
pub async fn vault_create(app_handle: AppHandle, input: CreateInput) -> Result<ItemMeta, String> {
    let app = app_handle.clone();
    let meta = blocking(move || personal_vault::create(&app, &input)).await?;
    notify_changed(&app_handle);
    Ok(meta)
}

#[tauri::command]
pub async fn vault_update(app_handle: AppHandle, id: String, input: UpdateInput) -> Result<ItemMeta, String> {
    let app = app_handle.clone();
    let meta = blocking(move || personal_vault::update(&app, &id, &input)).await?;
    notify_changed(&app_handle);
    Ok(meta)
}

#[tauri::command]
pub async fn vault_replace_secret(app_handle: AppHandle, id: String, auth: AuthInput) -> Result<ItemMeta, String> {
    let app = app_handle.clone();
    let meta = blocking(move || personal_vault::replace_secret(&app, &id, &auth)).await?;
    notify_changed(&app_handle);
    Ok(meta)
}

#[tauri::command]
pub async fn vault_delete(app_handle: AppHandle, id: String) -> Result<(), String> {
    let app = app_handle.clone();
    blocking(move || personal_vault::delete(&app, &id)).await?;
    notify_changed(&app_handle);
    Ok(())
}

#[tauri::command]
pub async fn vault_usage(app_handle: AppHandle, id: String, limit: Option<i64>) -> Result<Vec<UsageEntry>, String> {
    blocking(move || personal_vault::usage(&app_handle, &id, limit.unwrap_or(20))).await
}

/// Configurations (MCP servers, providers) referencing this credential, with their current
/// destination and whether the user confirmed it.
#[tauri::command]
pub async fn vault_references(app_handle: AppHandle, id: String) -> Result<Vec<ConfigReference>, String> {
    blocking(move || config_ref::references(&app_handle, &id)).await
}

/// The user confirms the destination shown on the vault page for an unconfirmed reference.
#[tauri::command]
pub async fn vault_confirm_reference(app_handle: AppHandle, kind: String, config_id: i64) -> Result<(), String> {
    blocking(move || config_ref::confirm_reference(&app_handle, &kind, config_id)).await
}

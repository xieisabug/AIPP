use std::path::PathBuf;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tauri::{AppHandle, Manager};
use tracing::warn;

use crate::db::system_db::{SecureConfigEntry, SystemDatabase};

use super::types::*;

pub(crate) fn migrate_secure_storage_if_needed(app_handle: &AppHandle) -> Result<(), String> {
    if let Some(local_key) = read_master_key_from_file(app_handle)? {
        return finalize_secure_storage_migration(app_handle, &local_key);
    }

    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let existing_key_b64 = db.get_config(SECURE_MASTER_KEY).map_err(|e| e.to_string())?;
    if existing_key_b64.trim().is_empty() {
        return Ok(());
    }

    let legacy_key = decode_master_key(existing_key_b64.trim())?;
    let local_key: [u8; 32] = rand::random();
    write_master_key_to_file(app_handle, &local_key)?;

    if let Some(entry) =
        db.get_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY).map_err(|e| e.to_string())?
    {
        match decrypt_secret_with_key(&legacy_key, &entry.ciphertext, &entry.nonce) {
            Ok(secret) => {
                let (ciphertext, nonce) = encrypt_secret_with_key(&local_key, &secret)?;
                db.upsert_secure_config(&SecureConfigEntry {
                    scope: entry.scope,
                    key: entry.key,
                    ciphertext,
                    nonce,
                    updated_time: None,
                })
                .map_err(|e| e.to_string())?;
            }
            Err(error) => {
                warn!(
                    error = %error,
                    "Stored Feishu secret could not be re-encrypted during secure storage migration"
                );
            }
        }
    }

    db.delete_system_config(SECURE_MASTER_KEY).map_err(|e| e.to_string())
}

fn get_or_create_master_key(app_handle: &AppHandle) -> Result<[u8; 32], String> {
    if let Some(key) = read_master_key_from_file(app_handle)? {
        migrate_secure_storage_if_needed(app_handle)?;
        return Ok(key);
    }

    migrate_secure_storage_if_needed(app_handle)?;

    if let Some(key) = read_master_key_from_file(app_handle)? {
        return Ok(key);
    }

    let key: [u8; 32] = rand::random();
    write_master_key_to_file(app_handle, &key)?;
    Ok(key)
}

fn local_secret_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let secret_dir = app_dir.join("local-secrets");
    std::fs::create_dir_all(&secret_dir).map_err(|e| e.to_string())?;
    Ok(secret_dir)
}

fn master_key_file_path(app_handle: &AppHandle) -> Result<PathBuf, String> {
    Ok(local_secret_dir(app_handle)?.join(SECURE_MASTER_KEY_FILE))
}

fn read_master_key_from_file(app_handle: &AppHandle) -> Result<Option<[u8; 32]>, String> {
    let path = master_key_file_path(app_handle)?;
    if !path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(&path)
        .map_err(|e| format!("Failed to read secure master key `{}`: {}", path.display(), e))?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| format!("Invalid secure master key length in `{}`", path.display()))?;
    Ok(Some(key))
}

fn write_master_key_to_file(app_handle: &AppHandle, key: &[u8; 32]) -> Result<(), String> {
    let path = master_key_file_path(app_handle)?;
    std::fs::write(&path, key)
        .map_err(|e| format!("Failed to write secure master key `{}`: {}", path.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, permissions).map_err(|e| {
            format!("Failed to set secure permissions on master key `{}`: {}", path.display(), e)
        })?;
    }
    Ok(())
}

fn finalize_secure_storage_migration(
    app_handle: &AppHandle,
    local_key: &[u8; 32],
) -> Result<(), String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let existing_key_b64 = db.get_config(SECURE_MASTER_KEY).map_err(|e| e.to_string())?;
    if existing_key_b64.trim().is_empty() {
        return Ok(());
    }

    let Some(entry) =
        db.get_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY).map_err(|e| e.to_string())?
    else {
        return db.delete_system_config(SECURE_MASTER_KEY).map_err(|e| e.to_string());
    };

    if decrypt_secret_with_key(local_key, &entry.ciphertext, &entry.nonce).is_ok() {
        return db.delete_system_config(SECURE_MASTER_KEY).map_err(|e| e.to_string());
    }

    let legacy_key = decode_master_key(existing_key_b64.trim())?;
    match decrypt_secret_with_key(&legacy_key, &entry.ciphertext, &entry.nonce) {
        Ok(secret) => {
            let (ciphertext, nonce) = encrypt_secret_with_key(local_key, &secret)?;
            db.upsert_secure_config(&SecureConfigEntry {
                scope: entry.scope,
                key: entry.key,
                ciphertext,
                nonce,
                updated_time: None,
            })
            .map_err(|e| e.to_string())?;
            db.delete_system_config(SECURE_MASTER_KEY).map_err(|e| e.to_string())
        }
        Err(error) => {
            warn!(
                error = %error,
                "Secure storage migration is incomplete; keeping legacy DB master key for recovery"
            );
            Ok(())
        }
    }
}

fn decode_master_key(key_b64: &str) -> Result<[u8; 32], String> {
    let decoded = BASE64.decode(key_b64).map_err(|e| e.to_string())?;
    decoded.try_into().map_err(|_| "Invalid secure config master key length".to_string())
}

pub(super) fn encrypt_secret_with_key(
    key_bytes: &[u8; 32],
    plaintext: &str,
) -> Result<(String, String), String> {
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Failed to encrypt secret: {e}"))?;
    Ok((BASE64.encode(encrypted), BASE64.encode(nonce_bytes)))
}

pub(super) fn decrypt_secret_with_key(
    key_bytes: &[u8; 32],
    ciphertext: &str,
    nonce: &str,
) -> Result<String, String> {
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce_bytes = BASE64.decode(nonce).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let encrypted = BASE64.decode(ciphertext).map_err(|e| e.to_string())?;
    let decrypted = cipher
        .decrypt(nonce, encrypted.as_ref())
        .map_err(|e| format!("Failed to decrypt secret: {e}"))?;
    String::from_utf8(decrypted).map_err(|e| e.to_string())
}

fn encrypt_secret(app_handle: &AppHandle, plaintext: &str) -> Result<(String, String), String> {
    let key_bytes = get_or_create_master_key(app_handle)?;
    encrypt_secret_with_key(&key_bytes, plaintext)
}

fn decrypt_secret(app_handle: &AppHandle, ciphertext: &str, nonce: &str) -> Result<String, String> {
    let key_bytes = get_or_create_master_key(app_handle)?;
    decrypt_secret_with_key(&key_bytes, ciphertext, nonce)
}

pub(crate) fn save_feishu_secret(app_handle: &AppHandle, app_secret: &str) -> Result<(), String> {
    if app_secret.trim().is_empty() {
        return Err("飞书 App Secret 不能为空".to_string());
    }
    let (ciphertext, nonce) = encrypt_secret(app_handle, app_secret.trim())?;
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.upsert_secure_config(&SecureConfigEntry {
        scope: FEISHU_SCOPE.to_string(),
        key: FEISHU_SECRET_KEY.to_string(),
        ciphertext,
        nonce,
        updated_time: None,
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn clear_feishu_secret(app_handle: &AppHandle) -> Result<(), String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    db.delete_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY).map_err(|e| e.to_string())
}

pub(super) fn load_feishu_secret(app_handle: &AppHandle) -> Result<Option<String>, String> {
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let Some(entry) =
        db.get_secure_config(FEISHU_SCOPE, FEISHU_SECRET_KEY).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    match decrypt_secret(app_handle, &entry.ciphertext, &entry.nonce) {
        Ok(secret) => Ok(Some(secret)),
        Err(error) => {
            warn!(
                error = %error,
                "Stored Feishu secret cannot be decrypted on this device; treating it as unavailable"
            );
            Ok(None)
        }
    }
}

pub(super) async fn load_runtime_config(
    app_handle: &AppHandle,
) -> Result<FeishuRuntimeConfig, String> {
    load_runtime_config_inner(app_handle, None).await
}

/// Inner version that accepts a pre-loaded secret to avoid repeated SystemDatabase opens
/// in tight polling loops.
pub(super) async fn load_runtime_config_inner(
    app_handle: &AppHandle,
    cached_secret: Option<&str>,
) -> Result<FeishuRuntimeConfig, String> {
    let feature_state = app_handle.state::<crate::FeatureConfigState>();
    let guard = feature_state.config_feature_map.lock().await;
    let experimental = guard.get(EXPERIMENTAL_FEATURE_CODE);
    let get = |key: &str| -> String {
        experimental
            .and_then(|map| map.get(key))
            .map(|config| config.value.clone())
            .unwrap_or_default()
    };
    let app_secret = match cached_secret {
        Some(s) => s.to_string(),
        None => load_feishu_secret(app_handle)?.unwrap_or_default(),
    };
    Ok(FeishuRuntimeConfig {
        butler_enabled: parse_bool_flag(&get("butler_experiment_enabled")),
        enabled: parse_bool_flag(&get("butler_feishu_enabled")),
        app_id: get("butler_feishu_app_id"),
        app_secret,
        base_url: {
            let value = get("butler_feishu_base_url");
            if value.trim().is_empty() {
                "https://open.feishu.cn".to_string()
            } else {
                value
            }
        },
        allow_p2p: !matches!(get("butler_feishu_receive_p2p").as_str(), "false" | "0"),
        allow_group: !matches!(get("butler_feishu_receive_group").as_str(), "false" | "0"),
        group_require_mention: !matches!(
            get("butler_feishu_group_require_mention").as_str(),
            "false" | "0"
        ),
        only_reply_feishu_originated: matches!(
            get("butler_feishu_only_reply_feishu_originated").as_str(),
            "true" | "1"
        ),
        allowed_open_ids: split_allowlist(&get("butler_feishu_allowed_open_ids")),
        allowed_chat_ids: split_allowlist(&get("butler_feishu_allowed_chat_ids")),
    })
}

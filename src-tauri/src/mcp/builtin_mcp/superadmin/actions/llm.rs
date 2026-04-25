use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::api::llm_api::{fetch_model_list, preview_model_list};
use crate::db::llm_db::{
    resolve_request_mode_or_default, LLMDatabase, LLMProvider, ModelDetail,
    DEFAULT_MODEL_REQUEST_MODE,
};
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;

type StoredModelRow = (i64, String, i64, String, String, bool, bool, bool);

struct LlmListProvidersHandler;
struct LlmGetProviderHandler;
struct LlmAddProviderHandler;
struct LlmUpdateProviderHandler;
struct LlmListModelsHandler;
struct LlmAddModelHandler;
struct LlmGetModelsHandler;
struct LlmGetModelHandler;

fn provider_id_arg(args: &Value) -> Result<i64, String> {
    args.get("provider_id")
        .and_then(|value| value.as_i64())
        .ok_or("Missing required parameter: provider_id".to_string())
}

fn required_string_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| format!("Missing required parameter: {key}"))
}

fn optional_string_arg(args: &Value, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("Invalid parameter: {key} must be string")),
    }
}

fn optional_bool_arg(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("Invalid parameter: {key} must be boolean")),
    }
}

fn validate_request_mode(request_mode: &str) -> Result<(), String> {
    match request_mode {
        "chat_completions" | "responses" => Ok(()),
        _ => Err(format!("Unsupported request_mode: {request_mode}")),
    }
}

fn load_request_mode_map(
    db: &LLMDatabase,
    provider_id: i64,
) -> Result<HashMap<String, String>, String> {
    Ok(db.list_model_request_modes(provider_id).map_err(|e| e.to_string())?.into_iter().collect())
}

fn stored_model_to_json(row: &StoredModelRow, request_mode: String) -> Value {
    json!({
        "id": row.0,
        "name": row.1,
        "provider_id": row.2,
        "code": row.3,
        "description": row.4,
        "vision_support": row.5,
        "audio_support": row.6,
        "video_support": row.7,
        "request_mode": request_mode,
    })
}

fn provider_summary_json(
    provider: &LLMProvider,
    stored_model_count: usize,
    config_count: Option<usize>,
    request_mode_override_count: Option<usize>,
) -> Value {
    let mut value = json!({
        "id": provider.id,
        "name": provider.name,
        "api_type": provider.api_type,
        "description": provider.description,
        "is_official": provider.is_official,
        "is_enabled": provider.is_enabled,
        "stored_model_count": stored_model_count,
    });

    if let Some(config_count) = config_count {
        value["config_count"] = json!(config_count);
    }
    if let Some(request_mode_override_count) = request_mode_override_count {
        value["request_mode_override_count"] = json!(request_mode_override_count);
    }

    value
}

fn model_detail_json(detail: &ModelDetail) -> Value {
    json!({
        "model": {
            "id": detail.model.id,
            "name": detail.model.name,
            "provider_id": detail.model.llm_provider_id,
            "code": detail.model.code,
            "description": detail.model.description,
            "vision_support": detail.model.vision_support,
            "audio_support": detail.model.audio_support,
            "video_support": detail.model.video_support,
            "request_mode": detail.model.request_mode,
        },
        "provider": {
            "id": detail.provider.id,
            "name": detail.provider.name,
            "api_type": detail.provider.api_type,
            "description": detail.provider.description,
            "is_official": detail.provider.is_official,
            "is_enabled": detail.provider.is_enabled,
        },
        "provider_config_count": detail.configs.len(),
    })
}

fn remote_model_to_json(model: &crate::api::llm_api::LlmModel) -> Value {
    json!({
        "id": model.id,
        "name": model.name,
        "provider_id": model.llm_provider_id,
        "code": model.code,
        "description": model.description,
        "vision_support": model.vision_support,
        "audio_support": model.audio_support,
        "video_support": model.video_support,
        "request_mode": model.request_mode,
    })
}

#[async_trait]
impl ActionHandler for LlmListProvidersHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        _args: Value,
        _dry_run: bool,
    ) -> Result<Value, String> {
        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let providers = db.get_llm_providers().map_err(|e| e.to_string())?;
        let all_models = db.get_all_llm_models().map_err(|e| e.to_string())?;

        let mut model_counts: HashMap<i64, usize> = HashMap::new();
        for (_, _, provider_id, _, _, _, _, _) in &all_models {
            *model_counts.entry(*provider_id).or_insert(0) += 1;
        }

        let items: Vec<Value> = providers
            .into_iter()
            .map(|(id, name, api_type, description, is_official, is_enabled)| {
                let provider =
                    LLMProvider { id, name, api_type, description, is_official, is_enabled };
                provider_summary_json(
                    &provider,
                    model_counts.get(&provider.id).copied().unwrap_or(0),
                    None,
                    None,
                )
            })
            .collect();

        Ok(json!({ "providers": items, "count": items.len() }))
    }
}

#[async_trait]
impl ActionHandler for LlmGetProviderHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        _dry_run: bool,
    ) -> Result<Value, String> {
        let provider_id = provider_id_arg(&args)?;
        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
        let stored_models =
            db.get_llm_models(provider_id.to_string()).map_err(|e| e.to_string())?;
        let config_count =
            db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?.len();
        let request_mode_override_count =
            db.list_model_request_modes(provider_id).map_err(|e| e.to_string())?.len();

        Ok(provider_summary_json(
            &provider,
            stored_models.len(),
            Some(config_count),
            Some(request_mode_override_count),
        ))
    }
}

#[async_trait]
impl ActionHandler for LlmAddProviderHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let name = required_string_arg(&args, "name")?;
        let api_type = required_string_arg(&args, "api_type")?;
        let description = optional_string_arg(&args, "description")?.unwrap_or_default();
        let is_official = optional_bool_arg(&args, "is_official")?.unwrap_or(false);
        let is_enabled = optional_bool_arg(&args, "is_enabled")?.unwrap_or(false);

        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let existing = db.get_llm_providers().map_err(|e| e.to_string())?;
        if existing.iter().any(|(_, existing_name, existing_api_type, _, _, _)| {
            existing_name == &name && existing_api_type == &api_type
        }) {
            return Err(format!("LLM provider '{name}' with api_type '{api_type}' already exists"));
        }

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_create": {
                    "name": name,
                    "api_type": api_type,
                    "description": description,
                    "is_official": is_official,
                    "is_enabled": is_enabled,
                }
            }));
        }

        db.add_llm_provider(&name, &api_type, &description, is_official, is_enabled)
            .map_err(|e| e.to_string())?;
        let provider_id = db.conn.last_insert_rowid();
        let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "provider_id": provider_id,
            "provider": provider_summary_json(&provider, 0, Some(0), Some(0)),
        }))
    }

    async fn snapshot_before(&self, _app_handle: &AppHandle, args: &Value) -> Option<Value> {
        Some(json!({
            "_type": "llm.add_provider",
            "name": args.get("name")?.as_str()?,
            "api_type": args.get("api_type")?.as_str()?,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        _snapshot: &Value,
        original_args: &Value,
    ) -> Result<Value, String> {
        let name = original_args
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("Missing name in original_args")?;
        let api_type = original_args
            .get("api_type")
            .and_then(|value| value.as_str())
            .ok_or("Missing api_type in original_args")?;

        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let provider_id = db
            .get_llm_providers()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|(_, provider_name, provider_api_type, _, _, _)| {
                provider_name == name && provider_api_type == api_type
            })
            .map(|(id, _, _, _, _, _)| id)
            .ok_or_else(|| format!("LLM provider not found for undo: {name}/{api_type}"))?;

        db.delete_llm_provider(provider_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "provider_id": provider_id,
            "name": name,
        }))
    }
}

#[async_trait]
impl ActionHandler for LlmUpdateProviderHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let provider_id = provider_id_arg(&args)?;
        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
        let stored_model_count =
            db.get_llm_models(provider_id.to_string()).map_err(|e| e.to_string())?.len();
        let config_count =
            db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?.len();
        let request_mode_override_count =
            db.list_model_request_modes(provider_id).map_err(|e| e.to_string())?.len();

        let mut updated_fields = Vec::new();

        let mut name = provider.name.clone();
        if let Some(next_name) = optional_string_arg(&args, "name")? {
            name = next_name;
            updated_fields.push("name");
        }

        let mut api_type = provider.api_type.clone();
        if let Some(next_api_type) = optional_string_arg(&args, "api_type")? {
            api_type = next_api_type;
            updated_fields.push("api_type");
        }

        let mut description = provider.description.clone();
        if let Some(next_description) = optional_string_arg(&args, "description")? {
            description = next_description;
            updated_fields.push("description");
        }

        let mut is_enabled = provider.is_enabled;
        if let Some(next_is_enabled) = optional_bool_arg(&args, "is_enabled")? {
            is_enabled = next_is_enabled;
            updated_fields.push("is_enabled");
        }

        let duplicate = db.get_llm_providers().map_err(|e| e.to_string())?.into_iter().any(
            |(id, existing_name, existing_api_type, _, _, _)| {
                id != provider_id && existing_name == name && existing_api_type == api_type
            },
        );
        if duplicate {
            return Err(format!(
                "Another LLM provider already uses '{name}' with api_type '{api_type}'"
            ));
        }

        let preview_provider = LLMProvider {
            id: provider_id,
            name: name.clone(),
            api_type: api_type.clone(),
            description: description.clone(),
            is_official: provider.is_official,
            is_enabled,
        };

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "provider_id": provider_id,
                "updated_fields": updated_fields,
                "would_update_to": provider_summary_json(
                    &preview_provider,
                    stored_model_count,
                    Some(config_count),
                    Some(request_mode_override_count),
                ),
            }));
        }

        db.update_llm_provider(provider_id, &name, &api_type, &description, is_enabled)
            .map_err(|e| e.to_string())?;
        let updated = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "provider_id": provider_id,
            "updated_fields": updated_fields,
            "provider": provider_summary_json(
                &updated,
                stored_model_count,
                Some(config_count),
                Some(request_mode_override_count),
            ),
        }))
    }

    async fn snapshot_before(&self, app_handle: &AppHandle, args: &Value) -> Option<Value> {
        let provider_id = args.get("provider_id")?.as_i64()?;
        let db = LLMDatabase::new(app_handle).ok()?;
        let provider = db.get_llm_provider(provider_id).ok()?;
        Some(json!({
            "_type": "llm.update_provider",
            "provider_id": provider.id,
            "name": provider.name,
            "api_type": provider.api_type,
            "description": provider.description,
            "is_enabled": provider.is_enabled,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &Value,
        _original_args: &Value,
    ) -> Result<Value, String> {
        let provider_id = snapshot
            .get("provider_id")
            .and_then(|value| value.as_i64())
            .ok_or("Missing provider_id in snapshot")?;
        let name = snapshot
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("Missing name in snapshot")?;
        let api_type = snapshot
            .get("api_type")
            .and_then(|value| value.as_str())
            .ok_or("Missing api_type in snapshot")?;
        let description = snapshot
            .get("description")
            .and_then(|value| value.as_str())
            .ok_or("Missing description in snapshot")?;
        let is_enabled = snapshot
            .get("is_enabled")
            .and_then(|value| value.as_bool())
            .ok_or("Missing is_enabled in snapshot")?;

        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.update_llm_provider(provider_id, name, api_type, description, is_enabled)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "provider_id": provider_id,
            "restored": "provider metadata",
        }))
    }
}

#[async_trait]
impl ActionHandler for LlmListModelsHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        _dry_run: bool,
    ) -> Result<Value, String> {
        let provider_id = args.get("provider_id").and_then(|value| value.as_i64());
        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;

        let rows = match provider_id {
            Some(provider_id) => {
                db.get_llm_models(provider_id.to_string()).map_err(|e| e.to_string())?
            }
            None => db.get_all_llm_models().map_err(|e| e.to_string())?,
        };

        let provider_ids: Vec<i64> = {
            let mut ids: Vec<i64> = rows.iter().map(|row| row.2).collect();
            ids.sort();
            ids.dedup();
            ids
        };
        let mut request_mode_maps: HashMap<i64, HashMap<String, String>> = HashMap::new();
        let mut provider_api_types: HashMap<i64, String> = HashMap::new();
        for provider_id in provider_ids {
            request_mode_maps.insert(provider_id, load_request_mode_map(&db, provider_id)?);
            provider_api_types.insert(
                provider_id,
                db.get_llm_provider(provider_id).map_err(|e| e.to_string())?.api_type,
            );
        }

        let models: Vec<Value> = rows
            .iter()
            .map(|row| {
                let request_mode = request_mode_maps
                    .get(&row.2)
                    .and_then(|map| map.get(&row.3))
                    .cloned()
                    .unwrap_or_else(|| {
                        resolve_request_mode_or_default(
                            provider_api_types.get(&row.2).map(String::as_str).unwrap_or(""),
                            &row.3,
                            None,
                        )
                        .to_string()
                    });
                stored_model_to_json(row, request_mode)
            })
            .collect();

        Ok(json!({
            "models": models,
            "count": models.len(),
            "provider_id": provider_id,
        }))
    }
}

#[async_trait]
impl ActionHandler for LlmAddModelHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let provider_id = provider_id_arg(&args)?;
        let code = required_string_arg(&args, "code")?;
        let name = optional_string_arg(&args, "name")?.unwrap_or_else(|| code.clone());
        let description =
            optional_string_arg(&args, "description")?.unwrap_or_else(|| code.clone());
        let vision_support = optional_bool_arg(&args, "vision_support")?.unwrap_or(false);
        let audio_support = optional_bool_arg(&args, "audio_support")?.unwrap_or(false);
        let video_support = optional_bool_arg(&args, "video_support")?.unwrap_or(false);
        let request_mode = optional_string_arg(&args, "request_mode")?;
        if let Some(request_mode) = request_mode.as_deref() {
            validate_request_mode(request_mode)?;
        }

        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
        let exists = db
            .get_llm_models(provider_id.to_string())
            .map_err(|e| e.to_string())?
            .into_iter()
            .any(|(_, _, _, existing_code, _, _, _, _)| existing_code == code);
        if exists {
            return Err(format!("Model '{code}' already exists for provider {provider_id}"));
        }

        let effective_request_mode = request_mode.clone().unwrap_or_else(|| {
            resolve_request_mode_or_default(&provider.api_type, &code, None).to_string()
        });

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_create": {
                    "provider_id": provider_id,
                    "code": code,
                    "name": name,
                    "description": description,
                    "vision_support": vision_support,
                    "audio_support": audio_support,
                    "video_support": video_support,
                    "request_mode": effective_request_mode,
                }
            }));
        }

        db.add_llm_model(
            &name,
            provider_id,
            &code,
            &description,
            vision_support,
            audio_support,
            video_support,
        )
        .map_err(|e| e.to_string())?;
        if let Some(request_mode) = request_mode.as_deref() {
            db.upsert_model_request_mode(provider_id, &code, request_mode)
                .map_err(|e| e.to_string())?;
        }

        let model_id = db.conn.last_insert_rowid();

        Ok(json!({
            "model": {
                "id": model_id,
                "name": name,
                "provider_id": provider_id,
                "code": code,
                "description": description,
                "vision_support": vision_support,
                "audio_support": audio_support,
                "video_support": video_support,
                "request_mode": effective_request_mode,
            }
        }))
    }

    async fn snapshot_before(&self, _app_handle: &AppHandle, args: &Value) -> Option<Value> {
        Some(json!({
            "_type": "llm.add_model",
            "provider_id": args.get("provider_id")?.as_i64()?,
            "code": args.get("code")?.as_str()?,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &Value,
        _original_args: &Value,
    ) -> Result<Value, String> {
        let provider_id = snapshot
            .get("provider_id")
            .and_then(|value| value.as_i64())
            .ok_or("Missing provider_id in snapshot")?;
        let code = snapshot
            .get("code")
            .and_then(|value| value.as_str())
            .ok_or("Missing code in snapshot")?;

        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.delete_llm_model(provider_id, code.to_string()).map_err(|e| e.to_string())?;
        db.conn
            .execute(
                "DELETE FROM llm_model_request_mode_preference WHERE llm_provider_id = ? AND model_code = ?",
                rusqlite::params![provider_id, code],
            )
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "provider_id": provider_id,
            "code": code,
        }))
    }
}

#[async_trait]
impl ActionHandler for LlmGetModelsHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        dry_run: bool,
    ) -> Result<Value, String> {
        let provider_id = provider_id_arg(&args)?;

        if dry_run {
            let preview = preview_model_list(app_handle.clone(), provider_id).await?;
            let models: Vec<Value> = preview
                .available_models
                .iter()
                .map(|model| {
                    json!({
                        "name": model.name,
                        "code": model.code,
                        "description": model.description,
                        "vision_support": model.vision_support,
                        "audio_support": model.audio_support,
                        "video_support": model.video_support,
                        "request_mode": model.request_mode,
                        "is_selected": model.is_selected,
                    })
                })
                .collect();
            return Ok(json!({
                "dry_run": true,
                "provider_id": provider_id,
                "models": models,
                "count": models.len(),
                "missing_models": preview.missing_models,
            }));
        }

        let models = fetch_model_list(app_handle.clone(), provider_id).await?;
        let items: Vec<Value> = models.iter().map(remote_model_to_json).collect();

        Ok(json!({
            "provider_id": provider_id,
            "models": items,
            "count": items.len(),
            "synced": true,
        }))
    }

    async fn snapshot_before(&self, app_handle: &AppHandle, args: &Value) -> Option<Value> {
        let provider_id = args.get("provider_id")?.as_i64()?;
        let db = LLMDatabase::new(app_handle).ok()?;
        let provider = db.get_llm_provider(provider_id).ok()?;
        let models = db.get_llm_models(provider_id.to_string()).ok()?;
        let request_mode_map = load_request_mode_map(&db, provider_id).ok()?;
        let snapshot_models: Vec<Value> = models
            .iter()
            .map(|row| {
                json!({
                    "name": row.1,
                    "provider_id": row.2,
                    "code": row.3,
                    "description": row.4,
                    "vision_support": row.5,
                    "audio_support": row.6,
                    "video_support": row.7,
                    "request_mode": request_mode_map
                        .get(&row.3)
                        .cloned()
                        .unwrap_or_else(|| {
                            resolve_request_mode_or_default(&provider.api_type, &row.3, None)
                                .to_string()
                        }),
                })
            })
            .collect();
        Some(json!({
            "_type": "llm.get_models",
            "provider_id": provider_id,
            "models": snapshot_models,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &Value,
        _original_args: &Value,
    ) -> Result<Value, String> {
        let provider_id = snapshot
            .get("provider_id")
            .and_then(|value| value.as_i64())
            .ok_or("Missing provider_id in snapshot")?;
        let models = snapshot
            .get("models")
            .and_then(|value| value.as_array())
            .ok_or("Missing models in snapshot")?;

        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.delete_llm_model_by_provider(provider_id).map_err(|e| e.to_string())?;

        for model in models {
            let name = model
                .get("name")
                .and_then(|value| value.as_str())
                .ok_or("Missing name in snapshot model")?;
            let code = model
                .get("code")
                .and_then(|value| value.as_str())
                .ok_or("Missing code in snapshot model")?;
            let description =
                model.get("description").and_then(|value| value.as_str()).unwrap_or(code);
            let vision_support =
                model.get("vision_support").and_then(|value| value.as_bool()).unwrap_or(false);
            let audio_support =
                model.get("audio_support").and_then(|value| value.as_bool()).unwrap_or(false);
            let video_support =
                model.get("video_support").and_then(|value| value.as_bool()).unwrap_or(false);
            let request_mode = model
                .get("request_mode")
                .and_then(|value| value.as_str())
                .unwrap_or(DEFAULT_MODEL_REQUEST_MODE);

            db.add_llm_model(
                name,
                provider_id,
                code,
                description,
                vision_support,
                audio_support,
                video_support,
            )
            .map_err(|e| e.to_string())?;
            db.upsert_model_request_mode(provider_id, code, request_mode)
                .map_err(|e| e.to_string())?;
        }

        Ok(json!({
            "undone": true,
            "provider_id": provider_id,
            "restored_model_count": models.len(),
        }))
    }
}

#[async_trait]
impl ActionHandler for LlmGetModelHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: Value,
        _dry_run: bool,
    ) -> Result<Value, String> {
        let db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let detail = if let Some(model_id) = args.get("model_id").and_then(|value| value.as_i64()) {
            db.get_llm_model_detail_by_id(&model_id).map_err(|e| e.to_string())?
        } else {
            let provider_id = provider_id_arg(&args)?;
            let model_code = required_string_arg(&args, "model_code")?;
            db.get_llm_model_detail(&provider_id, &model_code).map_err(|e| e.to_string())?
        };

        Ok(model_detail_json(&detail))
    }
}

pub fn register(registry: &mut ActionRegistry) {
    registry.register(
        ActionMeta {
            action_id: "llm.list_providers".into(),
            domain: "llm".into(),
            summary: "列出 LLM 提供商".into(),
            description: "列出所有 LLM 提供商，仅返回安全元数据，不包含敏感配置值。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "read".into(), "provider".into(), "list".into()],
            args_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "providers": { "type": "array" },
                    "count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(LlmListProvidersHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.get_provider".into(),
            domain: "llm".into(),
            summary: "获取 LLM 提供商详情".into(),
            description: "获取单个 LLM 提供商的安全详情，不返回配置值或密钥。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "read".into(), "provider".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer", "description": "提供商 ID" }
                },
                "required": ["provider_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "api_type": { "type": "string" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(LlmGetProviderHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.add_provider".into(),
            domain: "llm".into(),
            summary: "新增 LLM 提供商".into(),
            description: "新增一个 LLM 提供商，仅处理基础元数据，不写入敏感配置值。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "write".into(), "provider".into(), "create".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "提供商名称" },
                    "api_type": { "type": "string", "description": "API 类型，如 openai" },
                    "description": { "type": "string", "description": "描述（可选）" },
                    "is_official": { "type": "boolean", "description": "是否官方提供商（可选）" },
                    "is_enabled": { "type": "boolean", "description": "是否启用（可选）" }
                },
                "required": ["name", "api_type"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer" },
                    "provider": { "type": "object" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 superadmin_undo 撤销新增提供商。".into()),
        },
        Box::new(LlmAddProviderHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.update_provider".into(),
            domain: "llm".into(),
            summary: "更新 LLM 提供商".into(),
            description: "更新 LLM 提供商基础元数据，不返回或暴露敏感配置值。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "write".into(), "provider".into(), "update".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer", "description": "提供商 ID" },
                    "name": { "type": "string", "description": "新名称（可选）" },
                    "api_type": { "type": "string", "description": "新 API 类型（可选）" },
                    "description": { "type": "string", "description": "新描述（可选）" },
                    "is_enabled": { "type": "boolean", "description": "是否启用（可选）" }
                },
                "required": ["provider_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer" },
                    "updated_fields": { "type": "array" },
                    "provider": { "type": "object" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 superadmin_undo 恢复更新前的提供商元数据。".into()),
        },
        Box::new(LlmUpdateProviderHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.list_models".into(),
            domain: "llm".into(),
            summary: "列出本地模型".into(),
            description: "列出数据库中已保存的模型，可按提供商过滤。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "read".into(), "model".into(), "list".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer", "description": "提供商 ID（可选）" }
                },
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "models": { "type": "array" },
                    "count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(LlmListModelsHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.add_model".into(),
            domain: "llm".into(),
            summary: "新增本地模型".into(),
            description: "向指定提供商新增一个本地模型记录。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "write".into(), "model".into(), "create".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer", "description": "提供商 ID" },
                    "code": { "type": "string", "description": "模型代码" },
                    "name": { "type": "string", "description": "模型名称（可选）" },
                    "description": { "type": "string", "description": "模型描述（可选）" },
                    "vision_support": { "type": "boolean", "description": "是否支持图像（可选）" },
                    "audio_support": { "type": "boolean", "description": "是否支持音频（可选）" },
                    "video_support": { "type": "boolean", "description": "是否支持视频（可选）" },
                    "request_mode": { "type": "string", "description": "请求模式（可选）" }
                },
                "required": ["provider_id", "code"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "model": { "type": "object" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 superadmin_undo 撤销新增模型。".into()),
        },
        Box::new(LlmAddModelHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.get_models".into(),
            domain: "llm".into(),
            summary: "同步远程模型列表".into(),
            description: "根据指定提供商的现有配置远程获取模型列表，并同步回本地数据库。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "write".into(), "model".into(), "sync".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer", "description": "提供商 ID" }
                },
                "required": ["provider_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "provider_id": { "type": "integer" },
                    "models": { "type": "array" },
                    "count": { "type": "integer" },
                    "synced": { "type": "boolean" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some(
                "会覆盖本地该 provider 的模型集合；可通过 superadmin_undo 恢复同步前状态。".into(),
            ),
        },
        Box::new(LlmGetModelsHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "llm.get_model".into(),
            domain: "llm".into(),
            summary: "获取模型详情".into(),
            description: "获取单个模型的详情和所属提供商的安全元数据。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["llm".into(), "read".into(), "model".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "model_id": { "type": "integer", "description": "模型 ID（可选，优先使用）" },
                    "provider_id": { "type": "integer", "description": "提供商 ID（与 model_code 搭配使用）" },
                    "model_code": { "type": "string", "description": "模型代码（与 provider_id 搭配使用）" }
                },
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "model": { "type": "object" },
                    "provider": { "type": "object" },
                    "provider_config_count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(LlmGetModelHandler),
    );
}

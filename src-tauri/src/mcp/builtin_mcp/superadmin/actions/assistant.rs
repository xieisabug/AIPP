use async_trait::async_trait;
use serde_json::json;
use tauri::AppHandle;

use crate::db::assistant_db::AssistantDatabase;
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

struct AssistantListHandler;
struct AssistantGetHandler;
struct AssistantCreateHandler;
struct AssistantUpdatePromptHandler;
struct AssistantUpdateModelHandler;
struct AssistantUpdateMcpConfigHandler;
struct AssistantUpdateSkillConfigHandler;

#[async_trait]
impl ActionHandler for AssistantListHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        _args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let assistants = db.get_assistants().map_err(|e| e.to_string())?;
        let items: Vec<serde_json::Value> = assistants
            .iter()
            .map(|a| {
                json!({
                    "id": a.id,
                    "name": a.name,
                    "description": a.description,
                    "assistant_type": a.assistant_type,
                    "is_addition": a.is_addition,
                    "created_time": a.created_time,
                })
            })
            .collect();
        Ok(json!({ "assistants": items, "count": items.len() }))
    }
}

#[async_trait]
impl ActionHandler for AssistantGetHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = args
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: assistant_id")?;

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let assistant = db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
        let prompts = db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;
        let models = db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
        let model_configs =
            db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;

        let prompt_text = prompts.first().map(|p| p.prompt.as_str()).unwrap_or("");
        let model_info: Vec<serde_json::Value> = models
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "provider_id": m.provider_id,
                    "model_code": m.model_code,
                    "alias": m.alias,
                })
            })
            .collect();
        let config_info: Vec<serde_json::Value> = model_configs
            .iter()
            .map(|c| {
                json!({
                    "name": c.name,
                    "value": c.value,
                    "value_type": c.value_type,
                })
            })
            .collect();

        Ok(json!({
            "id": assistant.id,
            "name": assistant.name,
            "description": assistant.description,
            "assistant_type": assistant.assistant_type,
            "prompt": prompt_text,
            "models": model_info,
            "model_configs": config_info,
        }))
    }
}

#[async_trait]
impl ActionHandler for AssistantCreateHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let name =
            args.get("name").and_then(|v| v.as_str()).ok_or("Missing required parameter: name")?;
        let description = args.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let assistant_type = args.get("assistant_type").and_then(|v| v.as_i64());

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_create": { "name": name, "description": description, "assistant_type": assistant_type }
            }));
        }

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let new_id =
            db.add_assistant(name, description, assistant_type, true).map_err(|e| e.to_string())?;

        // If a prompt was provided, set it
        if let Some(prompt) = args.get("prompt").and_then(|v| v.as_str()) {
            if !prompt.is_empty() {
                db.add_assistant_prompt(new_id, prompt).map_err(|e| e.to_string())?;
            }
        }

        Ok(json!({ "assistant_id": new_id, "name": name }))
    }

    async fn snapshot_before(
        &self,
        _app_handle: &AppHandle,
        _args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        // Create action: snapshot records that nothing existed before
        Some(json!({ "_type": "assistant.create", "existed": false }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        _snapshot: &serde_json::Value,
        original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // Undo create = delete the created assistant
        // We need the result from the original execution to get the ID
        // The original_args won't have the ID, but the result_json in audit has it.
        // We'll handle this in the undo handler by passing result data.
        Err("Undo for assistant.create requires the created assistant_id from the execution result. Use superadmin_execute with assistant.delete instead.".to_string())
    }
}

#[async_trait]
impl ActionHandler for AssistantUpdatePromptHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = args
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: assistant_id")?;
        let prompt = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: prompt")?;

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;

        // Verify assistant exists
        let _ = db.get_assistant(assistant_id).map_err(|e| e.to_string())?;

        if dry_run {
            let old_prompts = db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;
            let old_text = old_prompts.first().map(|p| p.prompt.as_str()).unwrap_or("");
            return Ok(json!({
                "dry_run": true,
                "assistant_id": assistant_id,
                "old_prompt_length": old_text.len(),
                "new_prompt_length": prompt.len(),
            }));
        }

        let existing = db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;

        if let Some(p) = existing.first() {
            db.update_assistant_prompt(p.id, prompt).map_err(|e| e.to_string())?;
        } else {
            db.add_assistant_prompt(assistant_id, prompt).map_err(|e| e.to_string())?;
        }

        Ok(json!({
            "assistant_id": assistant_id,
            "updated_fields": ["prompt"],
        }))
    }

    async fn snapshot_before(
        &self,
        app_handle: &AppHandle,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let assistant_id = args.get("assistant_id")?.as_i64()?;
        let db = AssistantDatabase::new(app_handle).ok()?;
        let prompts = db.get_assistant_prompt(assistant_id).ok()?;
        let old_prompt = prompts.first().map(|p| p.prompt.as_str()).unwrap_or("");
        Some(json!({
            "_type": "assistant.update_prompt",
            "assistant_id": assistant_id,
            "prompt": old_prompt,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &serde_json::Value,
        _original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = snapshot
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing assistant_id in snapshot")?;
        let old_prompt =
            snapshot.get("prompt").and_then(|v| v.as_str()).ok_or("Missing prompt in snapshot")?;

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let existing = db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;

        if let Some(p) = existing.first() {
            db.update_assistant_prompt(p.id, old_prompt).map_err(|e| e.to_string())?;
        } else {
            db.add_assistant_prompt(assistant_id, old_prompt).map_err(|e| e.to_string())?;
        }

        Ok(json!({
            "undone": true,
            "assistant_id": assistant_id,
            "restored_prompt_length": old_prompt.len(),
        }))
    }
}

#[async_trait]
impl ActionHandler for AssistantUpdateModelHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = args
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: assistant_id")?;
        let provider_id = args
            .get("provider_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: provider_id")?;
        let model_code = args
            .get("model_code")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: model_code")?;
        let alias = args.get("alias").and_then(|v| v.as_str()).unwrap_or(model_code);

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let _ = db.get_assistant(assistant_id).map_err(|e| e.to_string())?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "assistant_id": assistant_id,
                "would_set": { "provider_id": provider_id, "model_code": model_code, "alias": alias }
            }));
        }

        let existing_models = db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;

        if let Some(m) = existing_models.first() {
            db.update_assistant_model(m.id, provider_id, model_code, alias)
                .map_err(|e| e.to_string())?;
        } else {
            db.add_assistant_model(assistant_id, provider_id, model_code, alias)
                .map_err(|e| e.to_string())?;
        }

        Ok(json!({
            "assistant_id": assistant_id,
            "updated_fields": ["model"],
            "provider_id": provider_id,
            "model_code": model_code,
        }))
    }

    async fn snapshot_before(
        &self,
        app_handle: &AppHandle,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let assistant_id = args.get("assistant_id")?.as_i64()?;
        let db = AssistantDatabase::new(app_handle).ok()?;
        let models = db.get_assistant_model(assistant_id).ok()?;
        let model_info: Vec<serde_json::Value> = models.iter().map(|m| {
            json!({ "id": m.id, "provider_id": m.provider_id, "model_code": m.model_code, "alias": m.alias })
        }).collect();
        Some(json!({
            "_type": "assistant.update_model",
            "assistant_id": assistant_id,
            "models": model_info,
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &serde_json::Value,
        _original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = snapshot
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing assistant_id in snapshot")?;
        let models = snapshot
            .get("models")
            .and_then(|v| v.as_array())
            .ok_or("Missing models in snapshot")?;

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;

        if let Some(old_model) = models.first() {
            let provider_id = old_model.get("provider_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let model_code = old_model.get("model_code").and_then(|v| v.as_str()).unwrap_or("");
            let alias = old_model.get("alias").and_then(|v| v.as_str()).unwrap_or(model_code);

            let existing = db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
            if let Some(m) = existing.first() {
                db.update_assistant_model(m.id, provider_id, model_code, alias)
                    .map_err(|e| e.to_string())?;
            }
        }

        Ok(json!({ "undone": true, "assistant_id": assistant_id, "restored": "model" }))
    }
}

#[async_trait]
impl ActionHandler for AssistantUpdateMcpConfigHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = args
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: assistant_id")?;
        let mcp_server_id = args
            .get("mcp_server_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: mcp_server_id")?;
        let is_enabled = args
            .get("is_enabled")
            .and_then(|v| v.as_bool())
            .ok_or("Missing required parameter: is_enabled")?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "assistant_id": assistant_id,
                "mcp_server_id": mcp_server_id,
                "would_set_enabled": is_enabled,
            }));
        }

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.upsert_assistant_mcp_config(assistant_id, mcp_server_id, is_enabled)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "assistant_id": assistant_id,
            "mcp_server_id": mcp_server_id,
            "is_enabled": is_enabled,
        }))
    }

    async fn snapshot_before(
        &self,
        app_handle: &AppHandle,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let assistant_id = args.get("assistant_id")?.as_i64()?;
        let mcp_server_id = args.get("mcp_server_id")?.as_i64()?;
        let db = AssistantDatabase::new(app_handle).ok()?;
        let configs = db.get_assistant_mcp_configs(assistant_id).ok()?;
        let old_config = configs.iter().find(|c| c.mcp_server_id == mcp_server_id);
        Some(json!({
            "_type": "assistant.update_mcp_config",
            "assistant_id": assistant_id,
            "mcp_server_id": mcp_server_id,
            "was_enabled": old_config.map(|c| c.is_enabled),
            "existed": old_config.is_some(),
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &serde_json::Value,
        _original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = snapshot
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing assistant_id in snapshot")?;
        let mcp_server_id = snapshot
            .get("mcp_server_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing mcp_server_id in snapshot")?;
        let was_enabled = snapshot.get("was_enabled").and_then(|v| v.as_bool()).unwrap_or(false);

        let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.upsert_assistant_mcp_config(assistant_id, mcp_server_id, was_enabled)
            .map_err(|e| e.to_string())?;

        Ok(json!({ "undone": true, "assistant_id": assistant_id, "restored_enabled": was_enabled }))
    }
}

#[async_trait]
impl ActionHandler for AssistantUpdateSkillConfigHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = args
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: assistant_id")?;
        let skill_identifier = args
            .get("skill_identifier")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: skill_identifier")?;
        let is_enabled = args
            .get("is_enabled")
            .and_then(|v| v.as_bool())
            .ok_or("Missing required parameter: is_enabled")?;
        let priority = args.get("priority").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "assistant_id": assistant_id,
                "skill_identifier": skill_identifier,
                "would_set_enabled": is_enabled,
                "priority": priority,
            }));
        }

        // Use skill_db to update
        let skill_db =
            crate::db::skill_db::SkillDatabase::new(app_handle).map_err(|e| e.to_string())?;
        skill_db
            .upsert_assistant_skill_config(assistant_id, skill_identifier, is_enabled, priority)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "assistant_id": assistant_id,
            "skill_identifier": skill_identifier,
            "is_enabled": is_enabled,
            "priority": priority,
        }))
    }

    async fn snapshot_before(
        &self,
        app_handle: &AppHandle,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let assistant_id = args.get("assistant_id")?.as_i64()?;
        let skill_identifier = args.get("skill_identifier")?.as_str()?;
        let skill_db = crate::db::skill_db::SkillDatabase::new(app_handle).ok()?;
        let configs = skill_db.get_assistant_skill_configs(assistant_id).ok()?;
        let old_config = configs.iter().find(|c| c.skill_identifier == skill_identifier);
        Some(json!({
            "_type": "assistant.update_skill_config",
            "assistant_id": assistant_id,
            "skill_identifier": skill_identifier,
            "was_enabled": old_config.map(|c| c.is_enabled),
            "was_priority": old_config.map(|c| c.priority),
            "existed": old_config.is_some(),
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &serde_json::Value,
        _original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let assistant_id = snapshot
            .get("assistant_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing assistant_id in snapshot")?;
        let skill_identifier = snapshot
            .get("skill_identifier")
            .and_then(|v| v.as_str())
            .ok_or("Missing skill_identifier in snapshot")?;
        let was_enabled = snapshot.get("was_enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let was_priority =
            snapshot.get("was_priority").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        let skill_db =
            crate::db::skill_db::SkillDatabase::new(app_handle).map_err(|e| e.to_string())?;
        skill_db
            .upsert_assistant_skill_config(
                assistant_id,
                skill_identifier,
                was_enabled,
                was_priority,
            )
            .map_err(|e| e.to_string())?;

        Ok(json!({ "undone": true, "assistant_id": assistant_id, "restored_enabled": was_enabled }))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut ActionRegistry) {
    registry.register(
        ActionMeta {
            action_id: "assistant.list".into(),
            domain: "assistant".into(),
            summary: "列出所有助手".into(),
            description: "返回 AIPP 中所有已配置的助手列表，包含基本信息。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["assistant".into(), "read".into(), "list".into()],
            args_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "assistants": { "type": "array" },
                    "count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(AssistantListHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "assistant.get".into(),
            domain: "assistant".into(),
            summary: "获取助手详情".into(),
            description: "根据 assistant_id 获取助手的完整信息，包括提示词、模型配置等。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::Assistant],
            tags: vec!["assistant".into(), "read".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer", "description": "助手 ID" }
                },
                "required": ["assistant_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "prompt": { "type": "string" },
                    "models": { "type": "array" },
                    "model_configs": { "type": "array" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(AssistantGetHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "assistant.create".into(),
            domain: "assistant".into(),
            summary: "创建新助手".into(),
            description: "创建一个新的助手，可指定名称、描述、类型和初始提示词。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["assistant".into(), "write".into(), "create".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "助手名称" },
                    "description": { "type": "string", "description": "助手描述" },
                    "assistant_type": { "type": "integer", "description": "助手类型 (0=普通, 1=多模型对比, 2=工作流, 3=展示, 4=ACP)" },
                    "prompt": { "type": "string", "description": "初始系统提示词" }
                },
                "required": ["name"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer" },
                    "name": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 assistant.delete 删除创建的助手".into()),
        },
        Box::new(AssistantCreateHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "assistant.update_prompt".into(),
            domain: "assistant".into(),
            summary: "更新助手系统提示词".into(),
            description: "修改指定助手的系统提示词。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Assistant],
            tags: vec!["assistant".into(), "write".into(), "prompt".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer", "description": "助手 ID" },
                    "prompt": { "type": "string", "description": "新的系统提示词" }
                },
                "required": ["assistant_id", "prompt"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer" },
                    "updated_fields": { "type": "array" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可再次调用 assistant.update_prompt 恢复旧提示词".into()),
        },
        Box::new(AssistantUpdatePromptHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "assistant.update_model".into(),
            domain: "assistant".into(),
            summary: "更新助手模型配置".into(),
            description: "修改指定助手使用的 LLM 模型（provider + model_code）。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Assistant],
            tags: vec!["assistant".into(), "write".into(), "model".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer", "description": "助手 ID" },
                    "provider_id": { "type": "integer", "description": "LLM 提供商 ID" },
                    "model_code": { "type": "string", "description": "模型代码 (如 gpt-4o)" },
                    "alias": { "type": "string", "description": "模型别名（可选，默认与 model_code 相同）" }
                },
                "required": ["assistant_id", "provider_id", "model_code"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer" },
                    "updated_fields": { "type": "array" },
                    "provider_id": { "type": "integer" },
                    "model_code": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可再次调用 assistant.update_model 恢复旧模型".into()),
        },
        Box::new(AssistantUpdateModelHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "assistant.update_mcp_config".into(),
            domain: "assistant".into(),
            summary: "更新助手 MCP 服务配置".into(),
            description: "启用或禁用指定助手的某个 MCP 服务。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Assistant],
            tags: vec!["assistant".into(), "write".into(), "mcp".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer", "description": "助手 ID" },
                    "mcp_server_id": { "type": "integer", "description": "MCP 服务 ID" },
                    "is_enabled": { "type": "boolean", "description": "是否启用" }
                },
                "required": ["assistant_id", "mcp_server_id", "is_enabled"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer" },
                    "mcp_server_id": { "type": "integer" },
                    "is_enabled": { "type": "boolean" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可再次调用本 action 切换启用状态".into()),
        },
        Box::new(AssistantUpdateMcpConfigHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "assistant.update_skill_config".into(),
            domain: "assistant".into(),
            summary: "更新助手 Skill 配置".into(),
            description: "启用或禁用指定助手的某个 Skill，并可设置优先级。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Assistant],
            tags: vec!["assistant".into(), "write".into(), "skill".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer", "description": "助手 ID" },
                    "skill_identifier": { "type": "string", "description": "Skill 标识符" },
                    "is_enabled": { "type": "boolean", "description": "是否启用" },
                    "priority": { "type": "integer", "description": "优先级（默认 0）" }
                },
                "required": ["assistant_id", "skill_identifier", "is_enabled"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "assistant_id": { "type": "integer" },
                    "skill_identifier": { "type": "string" },
                    "is_enabled": { "type": "boolean" },
                    "priority": { "type": "integer" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可再次调用本 action 切换启用状态".into()),
        },
        Box::new(AssistantUpdateSkillConfigHandler),
    );
}

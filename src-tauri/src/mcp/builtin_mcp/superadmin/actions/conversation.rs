use async_trait::async_trait;
use serde_json::json;
use tauri::AppHandle;

use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::mcp::builtin_mcp::superadmin::registry::{ActionHandler, ActionRegistry};
use crate::mcp::builtin_mcp::superadmin::types::*;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

struct ConversationListHandler;
struct ConversationGetHandler;
struct ConversationCreateHandler;
struct ConversationArchiveHandler;
struct ConversationInjectSystemMessageHandler;

#[async_trait]
impl ActionHandler for ConversationListHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let page = args.get("page").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
        let page_size = args.get("page_size").and_then(|v| v.as_u64()).unwrap_or(20) as u32;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let conversations = repo.list(page, page_size).map_err(|e| e.to_string())?;

        let items: Vec<serde_json::Value> = conversations
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "assistant_id": c.assistant_id,
                    "conversation_kind": c.conversation_kind,
                    "created_time": c.created_time.to_rfc3339(),
                    "updated_time": c.updated_time.to_rfc3339(),
                    "butler_task_status": c.butler_task_status,
                })
            })
            .collect();

        Ok(json!({ "conversations": items, "count": items.len(), "page": page }))
    }
}

#[async_trait]
impl ActionHandler for ConversationGetHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        _dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let conversation_id = args
            .get("conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: conversation_id")?;
        let include_messages =
            args.get("include_messages").and_then(|v| v.as_bool()).unwrap_or(true);
        let message_limit =
            args.get("message_limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let conversation = repo
            .read(conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;

        let mut result = json!({
            "id": conversation.id,
            "name": conversation.name,
            "assistant_id": conversation.assistant_id,
            "conversation_kind": conversation.conversation_kind,
            "parent_butler_conversation_id": conversation.parent_butler_conversation_id,
            "butler_task_status": conversation.butler_task_status,
            "created_time": conversation.created_time.to_rfc3339(),
            "updated_time": conversation.updated_time.to_rfc3339(),
        });

        if include_messages {
            let msg_repo = db.message_repo().map_err(|e| e.to_string())?;
            let messages_with_attachments =
                msg_repo.list_by_conversation_id(conversation_id).map_err(|e| e.to_string())?;

            let messages: Vec<serde_json::Value> = messages_with_attachments
                .iter()
                .take(message_limit)
                .map(|(m, _att)| {
                    json!({
                        "id": m.id,
                        "message_type": m.message_type,
                        "content": m.content,
                        "llm_model_name": m.llm_model_name,
                        "created_time": m.created_time.to_rfc3339(),
                    })
                })
                .collect();

            result["messages"] = json!(messages);
            result["message_count"] = json!(messages_with_attachments.len());
        }

        Ok(result)
    }
}

#[async_trait]
impl ActionHandler for ConversationCreateHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("New Conversation");
        let assistant_id = args.get("assistant_id").and_then(|v| v.as_i64());

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "would_create": { "name": name, "assistant_id": assistant_id }
            }));
        }

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;

        use crate::db::conversation_db::Conversation;
        use chrono::Utc;

        let conv = Conversation {
            id: 0,
            name: name.to_string(),
            assistant_id,
            created_time: Utc::now(),
            updated_time: Utc::now(),
            conversation_kind: "normal".to_string(),
            parent_butler_conversation_id: None,
            source_task_title: None,
            is_hidden_from_normal_chat_list: false,
            channel_source: None,
            butler_task_status: None,
            butler_task_summary: None,
            butler_task_finalized_at: None,
        };

        let created = repo.create(&conv).map_err(|e| e.to_string())?;

        Ok(json!({
            "conversation_id": created.id,
            "name": created.name,
        }))
    }

    async fn snapshot_before(
        &self,
        _app_handle: &AppHandle,
        _args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        Some(json!({ "_type": "conversation.create", "existed": false }))
    }
}

#[async_trait]
impl ActionHandler for ConversationArchiveHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let conversation_id = args
            .get("conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: conversation_id")?;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;

        let conversation = repo
            .read(conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "conversation_id": conversation_id,
                "current_name": conversation.name,
                "would_delete": true,
            }));
        }

        repo.delete(conversation_id).map_err(|e| e.to_string())?;

        Ok(json!({
            "conversation_id": conversation_id,
            "archived": true,
        }))
    }

    async fn snapshot_before(
        &self,
        app_handle: &AppHandle,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let conversation_id = args.get("conversation_id")?.as_i64()?;
        let db = ConversationDatabase::new(app_handle).ok()?;
        let repo = db.conversation_repo().ok()?;
        let conv = repo.read(conversation_id).ok()??;

        // Capture full conversation state before deletion
        let mut snapshot = json!({
            "_type": "conversation.archive",
            "conversation_id": conv.id,
            "name": conv.name,
            "assistant_id": conv.assistant_id,
            "conversation_kind": conv.conversation_kind,
            "parent_butler_conversation_id": conv.parent_butler_conversation_id,
            "source_task_title": conv.source_task_title,
            "is_hidden_from_normal_chat_list": conv.is_hidden_from_normal_chat_list,
            "channel_source": conv.channel_source,
            "butler_task_status": conv.butler_task_status,
            "butler_task_summary": conv.butler_task_summary,
        });

        // Also capture messages for full restore
        if let Ok(msg_repo) = db.message_repo() {
            if let Ok(messages) = msg_repo.list_by_conversation_id(conversation_id) {
                let msg_data: Vec<serde_json::Value> = messages
                    .iter()
                    .take(200)
                    .map(|(m, _)| {
                        json!({
                            "message_type": m.message_type,
                            "content": m.content,
                            "llm_model_name": m.llm_model_name,
                            "created_time": m.created_time.to_rfc3339(),
                        })
                    })
                    .collect();
                snapshot["messages"] = json!(msg_data);
                snapshot["message_count"] = json!(messages.len());
            }
        }

        Some(snapshot)
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        snapshot: &serde_json::Value,
        _original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // Re-create the conversation from snapshot
        let name = snapshot.get("name").and_then(|v| v.as_str()).unwrap_or("Restored Conversation");
        let assistant_id = snapshot.get("assistant_id").and_then(|v| v.as_i64());

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;

        use crate::db::conversation_db::Conversation;
        use chrono::Utc;

        let conv = Conversation {
            id: 0,
            name: name.to_string(),
            assistant_id,
            created_time: Utc::now(),
            updated_time: Utc::now(),
            conversation_kind: snapshot
                .get("conversation_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("normal")
                .to_string(),
            parent_butler_conversation_id: snapshot
                .get("parent_butler_conversation_id")
                .and_then(|v| v.as_i64()),
            source_task_title: snapshot
                .get("source_task_title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            is_hidden_from_normal_chat_list: snapshot
                .get("is_hidden_from_normal_chat_list")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            channel_source: snapshot
                .get("channel_source")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            butler_task_status: snapshot
                .get("butler_task_status")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            butler_task_summary: snapshot
                .get("butler_task_summary")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            butler_task_finalized_at: None,
        };

        let created = repo.create(&conv).map_err(|e| e.to_string())?;

        Ok(json!({
            "undone": true,
            "new_conversation_id": created.id,
            "name": created.name,
            "note": "Conversation re-created. Original messages are not restored (they were captured in snapshot for reference).",
        }))
    }
}

#[async_trait]
impl ActionHandler for ConversationInjectSystemMessageHandler {
    async fn execute(
        &self,
        app_handle: &AppHandle,
        args: serde_json::Value,
        dry_run: bool,
    ) -> Result<serde_json::Value, String> {
        let conversation_id = args
            .get("conversation_id")
            .and_then(|v| v.as_i64())
            .ok_or("Missing required parameter: conversation_id")?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: content")?;

        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;

        // Verify conversation exists
        let repo = db.conversation_repo().map_err(|e| e.to_string())?;
        let _ = repo
            .read(conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;

        if dry_run {
            return Ok(json!({
                "dry_run": true,
                "conversation_id": conversation_id,
                "content_length": content.len(),
            }));
        }

        use crate::db::conversation_db::Message;
        use chrono::Utc;

        let msg = Message {
            id: 0,
            parent_id: None,
            conversation_id,
            message_type: "system".to_string(),
            content: content.to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: Utc::now(),
            start_time: None,
            finish_time: Some(Utc::now()),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: None,
            parent_group_id: None,
            tool_calls_json: None,
            first_token_time: None,
            ttft_ms: None,
        };

        let msg_repo = db.message_repo().map_err(|e| e.to_string())?;
        let created = msg_repo.create(&msg).map_err(|e| e.to_string())?;

        Ok(json!({
            "conversation_id": conversation_id,
            "message_id": created.id,
            "message_type": "system",
        }))
    }

    async fn snapshot_before(
        &self,
        _app_handle: &AppHandle,
        args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        let conversation_id = args.get("conversation_id")?.as_i64()?;
        Some(json!({
            "_type": "conversation.inject_system_message",
            "conversation_id": conversation_id,
            "note": "Message will be created; undo will delete it by ID from result",
        }))
    }

    async fn undo(
        &self,
        app_handle: &AppHandle,
        _snapshot: &serde_json::Value,
        original_args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // The original execution result_json should contain message_id
        // We get the message_id from the execute result stored in audit
        // For now, we can't easily extract it here - the undo handler passes original_args
        // We'll need the result to find the message_id
        Err("Undo for inject_system_message requires manual deletion. The injected message_id can be found in the audit log result_json.".to_string())
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut ActionRegistry) {
    registry.register(
        ActionMeta {
            action_id: "conversation.list".into(),
            domain: "conversation".into(),
            summary: "列出会话".into(),
            description: "分页列出 AIPP 中的对话，包含基本信息。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["conversation".into(), "read".into(), "list".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "page": { "type": "integer", "description": "页码（默认 1）" },
                    "page_size": { "type": "integer", "description": "每页数量（默认 20）" }
                },
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "conversations": { "type": "array" },
                    "count": { "type": "integer" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(ConversationListHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "conversation.get".into(),
            domain: "conversation".into(),
            summary: "获取会话详情".into(),
            description: "根据 conversation_id 获取会话的完整信息和消息列表。".into(),
            risk_level: RiskLevel::SAFE,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::Conversation],
            tags: vec!["conversation".into(), "read".into(), "detail".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer", "description": "会话 ID" },
                    "include_messages": { "type": "boolean", "description": "是否包含消息（默认 true）" },
                    "message_limit": { "type": "integer", "description": "消息数量上限（默认 50）" }
                },
                "required": ["conversation_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "messages": { "type": "array" }
                }
            }),
            supports_dry_run: false,
            rollback_hint: None,
        },
        Box::new(ConversationGetHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "conversation.create".into(),
            domain: "conversation".into(),
            summary: "创建新会话".into(),
            description: "创建一个新的对话，可关联助手。".into(),
            risk_level: RiskLevel::LOW,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AutoAllow,
            allowed_scopes: vec![ActionScope::App],
            tags: vec!["conversation".into(), "write".into(), "create".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "会话名称" },
                    "assistant_id": { "type": "integer", "description": "关联的助手 ID（可选）" }
                },
                "required": []
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer" },
                    "name": { "type": "string" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: Some("可通过 conversation.archive 删除创建的会话".into()),
        },
        Box::new(ConversationCreateHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "conversation.archive".into(),
            domain: "conversation".into(),
            summary: "归档（删除）会话".into(),
            description: "删除指定会话及其所有消息。此操作不可逆。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Conversation],
            tags: vec!["conversation".into(), "write".into(), "delete".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer", "description": "会话 ID" }
                },
                "required": ["conversation_id"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer" },
                    "archived": { "type": "boolean" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: None,
        },
        Box::new(ConversationArchiveHandler),
    );

    registry.register(
        ActionMeta {
            action_id: "conversation.inject_system_message".into(),
            domain: "conversation".into(),
            summary: "注入系统消息".into(),
            description: "向指定会话注入一条系统消息。".into(),
            risk_level: RiskLevel::MEDIUM,
            requires_approval: false,
            approval_policy: ApprovalPolicy::AllowInScope,
            allowed_scopes: vec![ActionScope::Conversation],
            tags: vec!["conversation".into(), "write".into(), "message".into()],
            args_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer", "description": "会话 ID" },
                    "content": { "type": "string", "description": "系统消息内容" }
                },
                "required": ["conversation_id", "content"]
            }),
            result_schema: json!({
                "type": "object",
                "properties": {
                    "conversation_id": { "type": "integer" },
                    "message_id": { "type": "integer" }
                }
            }),
            supports_dry_run: true,
            rollback_hint: None,
        },
        Box::new(ConversationInjectSystemMessageHandler),
    );
}

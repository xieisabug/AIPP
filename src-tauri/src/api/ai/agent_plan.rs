use serde_json::{json, Value};
use tauri::Emitter;

use crate::api::ai::events::{ConversationEvent, MessageAddEvent, MessageUpdateEvent};
use crate::db::conversation_db::{ConversationDatabase, Repository};

pub const AGENT_PLAN_MESSAGE_TYPE: &str = "agent_plan";

pub struct AgentPlanMessage {
    pub message_id: i64,
    pub content: String,
    metadata: Value,
}

impl AgentPlanMessage {
    pub fn create(
        app: &tauri::AppHandle,
        window: &tauri::Window,
        conversation_id: i64,
        parent_message_id: i64,
        provider: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        item_id: Option<&str>,
    ) -> Result<Self, String> {
        let metadata = json!({
            "agent_plan": {
                "provider": provider,
                "session_id": session_id,
                "turn_id": turn_id,
                "item_id": item_id,
                "status": "streaming",
            }
        });
        let message = crate::api::ai_api::add_message(
            app,
            Some(parent_message_id),
            conversation_id,
            AGENT_PLAN_MESSAGE_TYPE.to_string(),
            String::new(),
            None,
            Some(provider.to_string()),
            Some(chrono::Utc::now()),
            None,
            0,
            None,
            None,
        )
        .map_err(|error| error.to_string())?;
        let db = ConversationDatabase::new(app).map_err(|error| error.to_string())?;
        let repo = db.message_repo().map_err(|error| error.to_string())?;
        repo.update_metadata(message.id, serde_json::to_string(&metadata).ok().as_deref())
            .map_err(|error| error.to_string())?;
        let _ = window.emit(
            format!("conversation_event_{conversation_id}").as_str(),
            ConversationEvent {
                r#type: "message_add".to_string(),
                data: serde_json::to_value(MessageAddEvent {
                    message_id: message.id,
                    message_type: AGENT_PLAN_MESSAGE_TYPE.to_string(),
                })
                .unwrap(),
            },
        );
        let _ = window.emit(
            format!("conversation_event_{conversation_id}").as_str(),
            ConversationEvent {
                r#type: "message_metadata_update".to_string(),
                data: json!({"message_id": message.id}),
            },
        );
        Ok(Self { message_id: message.id, content: String::new(), metadata })
    }

    pub fn replace_content(
        &mut self,
        app: &tauri::AppHandle,
        window: &tauri::Window,
        conversation_id: i64,
        content: String,
        done: bool,
    ) {
        self.content = content;
        let db = ConversationDatabase::new(app);
        if let Ok(db) = db {
            if let Ok(repo) = db.message_repo() {
                let _ = repo.update_content(self.message_id, &self.content);
                if done {
                    if let Some(plan) = self.metadata.get_mut("agent_plan").and_then(Value::as_object_mut) {
                        plan.insert("status".to_string(), json!("completed"));
                    }
                    let _ = repo.update_metadata(
                        self.message_id,
                        serde_json::to_string(&self.metadata).ok().as_deref(),
                    );
                    if let Ok(Some(mut message)) = repo.read(self.message_id) {
                        message.finish_time = Some(chrono::Utc::now());
                        let _ = repo.update(&message);
                    }
                }
            }
        }
        let _ = window.emit(
            format!("conversation_event_{conversation_id}").as_str(),
            ConversationEvent {
                r#type: "message_update".to_string(),
                data: serde_json::to_value(MessageUpdateEvent {
                    message_id: self.message_id,
                    message_type: AGENT_PLAN_MESSAGE_TYPE.to_string(),
                    content: self.content.clone(),
                    is_done: done,
                    token_count: None,
                    input_token_count: None,
                    output_token_count: None,
                    ttft_ms: None,
                    tps: None,
                })
                .unwrap(),
            },
        );
    }

    pub fn append_delta(
        &mut self,
        app: &tauri::AppHandle,
        window: &tauri::Window,
        conversation_id: i64,
        delta: &str,
    ) {
        let mut content = self.content.clone();
        content.push_str(delta);
        self.replace_content(app, window, conversation_id, content, false);
    }
}

//! Todo API - Tauri commands for accessing todo list state
//!
//! This module provides commands to retrieve the current todo list
//! for a conversation, maintained by the built-in agent's todo_write tool.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::mcp::builtin_mcp::TodoState;

/// Response structure for get_todos command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItemResponse {
    pub content: String,
    pub status: String,
    pub active_form: String,
}

/// Get todos for a specific conversation
#[tauri::command]
pub async fn get_todos(
    conversation_id: i64,
    todo_state: State<'_, TodoState>,
) -> Result<Vec<TodoItemResponse>, String> {
    let todos = todo_state.get_todos(conversation_id);
    
    let response: Vec<TodoItemResponse> = todos
        .into_iter()
        .map(|t| TodoItemResponse {
            content: t.content,
            status: t.status.to_string(),
            active_form: t.active_form,
        })
        .collect();
    
    Ok(response)
}

/// Emit a todo update event to the frontend
pub fn emit_todo_update(app_handle: &AppHandle, conversation_id: i64, todos: &[TodoItemResponse]) {
    use tauri::Emitter;
    
    #[derive(Clone, Serialize)]
    struct TodoUpdateEvent {
        conversation_id: i64,
        todos: Vec<TodoItemResponse>,
    }
    
    let event = TodoUpdateEvent {
        conversation_id,
        todos: todos.to_vec(),
    };
    
    if let Err(e) = app_handle.emit("todo_update", event) {
        tracing::warn!(error = %e, "Failed to emit todo_update event");
    }
}

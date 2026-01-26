//! Todo handler - implements todo write operations
//!
//! This handler manages the structured task list for AI agent sessions,
//! allowing tracking of progress on complex multi-step tasks.

use super::types::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, instrument, warn};

/// State for todo lists, keyed by conversation_id
#[derive(Debug, Clone, Default)]
pub struct TodoState {
    /// Todo lists per conversation
    todos: Arc<RwLock<HashMap<i64, Vec<TodoItem>>>>,
}

impl TodoState {
    pub fn new() -> Self {
        Self { todos: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Get todos for a conversation
    pub fn get_todos(&self, conversation_id: i64) -> Vec<TodoItem> {
        self.todos.read().unwrap().get(&conversation_id).cloned().unwrap_or_default()
    }

    /// Set todos for a conversation
    pub fn set_todos(&self, conversation_id: i64, todos: Vec<TodoItem>) {
        self.todos.write().unwrap().insert(conversation_id, todos);
    }

    /// Clear todos for a conversation
    #[allow(dead_code)]
    pub fn clear_todos(&self, conversation_id: i64) {
        self.todos.write().unwrap().remove(&conversation_id);
    }
}

/// Handler for Todo tools
pub struct TodoHandler {
    state: TodoState,
}

impl TodoHandler {
    pub fn new(state: TodoState) -> Self {
        Self { state }
    }

    /// Write/update the todo list for a conversation
    #[instrument(skip(self, request), fields(todo_count = request.todos.len()))]
    pub fn todo_write(
        &self,
        request: TodoWriteRequest,
        conversation_id: Option<i64>,
    ) -> Result<TodoWriteResponse, String> {
        let conv_id = conversation_id.unwrap_or(0);

        // Validate todos
        for (i, todo) in request.todos.iter().enumerate() {
            if todo.content.trim().is_empty() {
                return Err(format!("Todo item {} has empty content", i + 1));
            }
            if todo.active_form.trim().is_empty() {
                return Err(format!("Todo item {} has empty activeForm", i + 1));
            }
        }

        // Count status
        let pending = request.todos.iter().filter(|t| t.status == TodoStatus::Pending).count();
        let in_progress =
            request.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
        let completed = request.todos.iter().filter(|t| t.status == TodoStatus::Completed).count();
        let total = request.todos.len();

        // Warn if more than one in_progress (but don't error - let AI manage this)
        if in_progress > 1 {
            warn!(
                conversation_id = conv_id,
                in_progress_count = in_progress,
                "Multiple tasks marked as in_progress (ideally should be exactly 1)"
            );
        }

        // Get current active task
        let current_task = request
            .todos
            .iter()
            .find(|t| t.status == TodoStatus::InProgress)
            .map(|t| t.active_form.clone());

        // Store the todos
        self.state.set_todos(conv_id, request.todos.clone());

        // Build summary message
        let message = if total == 0 {
            "Todo list cleared".to_string()
        } else {
            let progress_pct = if total > 0 { (completed * 100) / total } else { 0 };
            format!(
                "Todo list updated: {}/{} tasks completed ({}%)",
                completed, total, progress_pct
            )
        };

        info!(
            conversation_id = conv_id,
            total = total,
            pending = pending,
            in_progress = in_progress,
            completed = completed,
            "Todo list updated"
        );

        debug!(
            conversation_id = conv_id,
            current_task = ?current_task,
            "Current active task"
        );

        Ok(TodoWriteResponse {
            success: true,
            message,
            total,
            pending,
            in_progress,
            completed,
            current_task,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_todo_write_empty_list() {
        let state = TodoState::new();
        let handler = TodoHandler::new(state);

        let request = TodoWriteRequest { todos: vec![] };
        let result = handler.todo_write(request, Some(1)).unwrap();

        assert!(result.success);
        assert_eq!(result.total, 0);
        assert_eq!(result.message, "Todo list cleared");
    }

    #[test]
    fn test_todo_write_with_items() {
        let state = TodoState::new();
        let handler = TodoHandler::new(state);

        let request = TodoWriteRequest {
            todos: vec![
                TodoItem {
                    content: "Fix bug".into(),
                    status: TodoStatus::Completed,
                    active_form: "Fixing bug".into(),
                },
                TodoItem {
                    content: "Add tests".into(),
                    status: TodoStatus::InProgress,
                    active_form: "Adding tests".into(),
                },
                TodoItem {
                    content: "Update docs".into(),
                    status: TodoStatus::Pending,
                    active_form: "Updating docs".into(),
                },
            ],
        };

        let result = handler.todo_write(request, Some(1)).unwrap();

        assert!(result.success);
        assert_eq!(result.total, 3);
        assert_eq!(result.pending, 1);
        assert_eq!(result.in_progress, 1);
        assert_eq!(result.completed, 1);
        assert_eq!(result.current_task, Some("Adding tests".into()));
    }

    #[test]
    fn test_todo_write_empty_content_error() {
        let state = TodoState::new();
        let handler = TodoHandler::new(state);

        let request = TodoWriteRequest {
            todos: vec![TodoItem {
                content: "".into(),
                status: TodoStatus::Pending,
                active_form: "Doing something".into(),
            }],
        };

        let result = handler.todo_write(request, Some(1));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty content"));
    }

    #[test]
    fn test_todo_state_persistence() {
        let state = TodoState::new();
        let handler = TodoHandler::new(state.clone());

        let request = TodoWriteRequest {
            todos: vec![TodoItem {
                content: "Task 1".into(),
                status: TodoStatus::Pending,
                active_form: "Doing task 1".into(),
            }],
        };

        handler.todo_write(request, Some(42)).unwrap();

        // Verify state was persisted
        let stored = state.get_todos(42);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].content, "Task 1");
    }
}

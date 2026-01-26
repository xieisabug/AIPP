//! Types for the TodoWrite tool
//!
//! This module defines request/response types for managing structured task lists
//! during AI agent sessions.

use serde::{Deserialize, Serialize};

/// Status of a todo item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// Task is waiting to be started
    Pending,
    /// Task is currently being worked on
    InProgress,
    /// Task has been completed
    Completed,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Completed => write!(f, "completed"),
        }
    }
}

/// A single todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Imperative form: what needs to be done (e.g., "Fix authentication bug")
    pub content: String,
    /// Current status of the task
    pub status: TodoStatus,
    /// Present continuous form: what's being done (e.g., "Fixing authentication bug")
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

/// Request to write/update the todo list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoWriteRequest {
    /// The complete updated todo list
    pub todos: Vec<TodoItem>,
}

/// Response from the todo write operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoWriteResponse {
    /// Whether the operation was successful
    pub success: bool,
    /// Summary message about the todo list state
    pub message: String,
    /// Total number of todos
    pub total: usize,
    /// Number of pending todos
    pub pending: usize,
    /// Number of in-progress todos
    pub in_progress: usize,
    /// Number of completed todos
    pub completed: usize,
    /// The current active task (in_progress), if any
    pub current_task: Option<String>,
}

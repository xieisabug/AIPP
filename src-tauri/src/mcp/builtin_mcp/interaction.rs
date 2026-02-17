use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

type AskUserQuestionDecision = Result<HashMap<String, String>, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionItem {
    pub question: String,
    pub header: String,
    pub options: Vec<AskUserQuestionOption>,
    #[serde(default, rename = "multiSelect", alias = "multi_select")]
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionMetadata {
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionRequest {
    pub questions: Vec<AskUserQuestionItem>,
    #[serde(default)]
    pub answers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub metadata: Option<AskUserQuestionMetadata>,
}

impl AskUserQuestionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.questions.is_empty() || self.questions.len() > 4 {
            return Err("AskUserQuestion requires 1-4 questions".to_string());
        }

        for (index, question) in self.questions.iter().enumerate() {
            if question.question.trim().is_empty() {
                return Err(format!("Question #{} cannot be empty", index + 1));
            }
            if question.header.trim().is_empty() {
                return Err(format!("Question #{} header cannot be empty", index + 1));
            }
            if question.options.len() < 2 || question.options.len() > 4 {
                return Err(format!("Question #{} must provide 2-4 options", index + 1));
            }

            let mut seen_labels = HashSet::new();
            for option in &question.options {
                if option.label.trim().is_empty() {
                    return Err(format!("Question #{} option label cannot be empty", index + 1));
                }
                if option.description.trim().is_empty() {
                    return Err(format!("Question #{} option description cannot be empty", index + 1));
                }
                let normalized = option.label.trim().to_lowercase();
                if !seen_labels.insert(normalized) {
                    return Err(format!("Question #{} option labels must be unique", index + 1));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestionRequestEvent {
    pub request_id: String,
    pub conversation_id: Option<i64>,
    pub questions: Vec<AskUserQuestionItem>,
    #[serde(default)]
    pub metadata: Option<AskUserQuestionMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewFileItem {
    pub title: String,
    #[serde(rename = "type")]
    pub file_type: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewFileMetadata {
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewFileRequest {
    pub files: Vec<PreviewFileItem>,
    #[serde(default, rename = "viewMode", alias = "view_mode")]
    pub view_mode: Option<String>,
    #[serde(default)]
    pub metadata: Option<PreviewFileMetadata>,
}

impl PreviewFileRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.files.is_empty() || self.files.len() > 10 {
            return Err("PreviewFile requires 1-10 files".to_string());
        }

        for (index, file) in self.files.iter().enumerate() {
            if file.title.trim().is_empty() {
                return Err(format!("File #{} title cannot be empty", index + 1));
            }

            let supported = matches!(
                file.file_type.as_str(),
                "markdown" | "text" | "image" | "pdf" | "html"
            );
            if !supported {
                return Err(format!(
                    "Unsupported file type '{}' for file '{}'",
                    file.file_type, file.title
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewFileRequestEvent {
    pub request_id: String,
    pub conversation_id: Option<i64>,
    pub files: Vec<PreviewFileItem>,
    #[serde(rename = "viewMode")]
    pub view_mode: String,
    #[serde(default)]
    pub metadata: Option<PreviewFileMetadata>,
}

#[derive(Clone)]
pub struct InteractionState {
    pending_ask_user:
        Arc<Mutex<HashMap<String, oneshot::Sender<AskUserQuestionDecision>>>>,
}

impl InteractionState {
    pub fn new() -> Self {
        Self {
            pending_ask_user: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn store_ask_user_request(
        &self,
        request_id: String,
        sender: oneshot::Sender<AskUserQuestionDecision>,
    ) {
        let mut pending = self.pending_ask_user.lock().await;
        pending.insert(request_id, sender);
    }

    pub async fn remove_ask_user_request(&self, request_id: &str) {
        let mut pending = self.pending_ask_user.lock().await;
        pending.remove(request_id);
    }

    pub async fn resolve_ask_user_request(
        &self,
        request_id: &str,
        decision: AskUserQuestionDecision,
    ) -> bool {
        let mut pending = self.pending_ask_user.lock().await;
        if let Some(sender) = pending.remove(request_id) {
            sender.send(decision).is_ok()
        } else {
            false
        }
    }
}

impl Default for InteractionState {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn request_ask_user_question(
    app_handle: &AppHandle,
    interaction_state: &InteractionState,
    conversation_id: Option<i64>,
    request: AskUserQuestionRequest,
) -> Result<HashMap<String, String>, String> {
    request.validate()?;

    let request_id = uuid::Uuid::new_v4().to_string();
    let event = AskUserQuestionRequestEvent {
        request_id: request_id.clone(),
        conversation_id,
        questions: request.questions,
        metadata: request.metadata,
    };

    let (tx, rx) = oneshot::channel::<AskUserQuestionDecision>();
    interaction_state
        .store_ask_user_request(request_id.clone(), tx)
        .await;

    info!(
        request_id = %request_id,
        conversation_id = ?conversation_id,
        "Requesting AskUserQuestion input from frontend"
    );

    if let Err(e) = app_handle.emit("ask-user-question-request", &event) {
        interaction_state.remove_ask_user_request(&request_id).await;
        warn!(request_id = %request_id, error = %e, "Failed to emit ask-user-question-request");
        return Err("Failed to emit AskUserQuestion event".to_string());
    }

    match rx.await {
        Ok(result) => result,
        Err(_) => Err("AskUserQuestion request was cancelled".to_string()),
    }
}

pub fn emit_preview_file_request(
    app_handle: &AppHandle,
    conversation_id: Option<i64>,
    request: PreviewFileRequest,
) -> Result<String, String> {
    request.validate()?;

    let request_id = uuid::Uuid::new_v4().to_string();
    let resolved_view_mode = match request.view_mode.as_deref() {
        Some("tabs") | Some("list") | Some("grid") => {
            request.view_mode.unwrap_or_else(|| "tabs".to_string())
        }
        _ => "tabs".to_string(),
    };
    let event = PreviewFileRequestEvent {
        request_id: request_id.clone(),
        conversation_id,
        files: request.files,
        view_mode: resolved_view_mode,
        metadata: request.metadata,
    };

    if let Err(e) = app_handle.emit("preview-file-request", &event) {
        warn!(request_id = %request_id, error = %e, "Failed to emit preview-file-request");
        return Err("Failed to emit PreviewFile event".to_string());
    }

    Ok(request_id)
}

#[tauri::command]
pub async fn submit_ask_user_question_response(
    app_handle: AppHandle,
    request_id: String,
    answers: Option<HashMap<String, String>>,
    cancelled: Option<bool>,
) -> Result<bool, String> {
    let state = app_handle
        .try_state::<InteractionState>()
        .ok_or_else(|| "InteractionState not found".to_string())?;

    let decision = if cancelled.unwrap_or(false) {
        Err("User cancelled AskUserQuestion".to_string())
    } else {
        let Some(value) = answers else {
            return Err("Missing answers".to_string());
        };
        if value.is_empty() {
            return Err("Answers cannot be empty".to_string());
        }
        Ok(value)
    };

    let resolved = state
        .resolve_ask_user_request(&request_id, decision)
        .await;

    if resolved {
        Ok(true)
    } else {
        Err("AskUserQuestion request not found or already resolved".to_string())
    }
}

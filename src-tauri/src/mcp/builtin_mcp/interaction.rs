use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};

type AskUserQuestionDecision = Result<HashMap<String, String>, String>;
pub const PREVIEW_FILE_RELAY_SCHEME: &str = "aipp-preview";
const PREVIEW_FILE_RELAY_TTL_SECS: u64 = 10 * 60;
const PREVIEW_FILE_RELAY_MAX_BYTES: u64 = 20 * 1024 * 1024;

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
                    return Err(format!(
                        "Question #{} option description cannot be empty",
                        index + 1
                    ));
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

            let supported =
                matches!(file.file_type.as_str(), "markdown" | "text" | "image" | "pdf" | "html");
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

#[derive(Debug, Clone)]
struct PreviewFileRelayEntry {
    file_path: PathBuf,
    file_type: String,
    expires_at: Instant,
    conversation_id: Option<i64>,
}

#[derive(Clone)]
pub struct PreviewFileRelayState {
    entries: Arc<StdMutex<HashMap<String, PreviewFileRelayEntry>>>,
}

impl PreviewFileRelayState {
    pub fn new() -> Self {
        Self { entries: Arc::new(StdMutex::new(HashMap::new())) }
    }

    fn prune_expired_locked(entries: &mut HashMap<String, PreviewFileRelayEntry>) {
        let now = Instant::now();
        entries.retain(|_, entry| entry.expires_at > now);
    }

    fn register_local_file(
        &self,
        file_path: PathBuf,
        file_type: String,
        conversation_id: Option<i64>,
    ) -> Result<String, String> {
        let token = uuid::Uuid::new_v4().to_string();
        let mut entries =
            self.entries.lock().map_err(|_| "Preview relay state poisoned".to_string())?;
        Self::prune_expired_locked(&mut entries);
        entries.insert(
            token.clone(),
            PreviewFileRelayEntry {
                file_path,
                file_type,
                expires_at: Instant::now() + Duration::from_secs(PREVIEW_FILE_RELAY_TTL_SECS),
                conversation_id,
            },
        );
        Ok(token)
    }

    fn get_entry(&self, token: &str) -> Result<Option<PreviewFileRelayEntry>, String> {
        let mut entries =
            self.entries.lock().map_err(|_| "Preview relay state poisoned".to_string())?;
        Self::prune_expired_locked(&mut entries);
        Ok(entries.get(token).cloned())
    }
}

impl Default for PreviewFileRelayState {
    fn default() -> Self {
        Self::new()
    }
}

fn build_relay_error_response(status: StatusCode, message: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(message.as_bytes().to_vec())
        .unwrap_or_else(|_| Response::new(message.as_bytes().to_vec()))
}

fn detect_relay_content_type(file_type: &str, file_path: &Path) -> String {
    match file_type {
        "html" => "text/html; charset=utf-8".to_string(),
        "pdf" => "application/pdf".to_string(),
        "markdown" => "text/markdown; charset=utf-8".to_string(),
        "text" => {
            let guessed = mime_guess::from_path(file_path).first_or_octet_stream();
            format!("{}; charset=utf-8", guessed.essence_str())
        }
        "image" => {
            let guessed = mime_guess::from_path(file_path).first_or_octet_stream();
            if guessed.type_().as_str() == "image" {
                guessed.essence_str().to_string()
            } else {
                "application/octet-stream".to_string()
            }
        }
        _ => mime_guess::from_path(file_path).first_or_octet_stream().essence_str().to_string(),
    }
}

fn resolve_local_file_path(raw_url: &str) -> Option<PathBuf> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("file://") {
        let path_part = if let Some(without_host) = rest.strip_prefix("localhost") {
            without_host
        } else {
            rest
        };
        let decoded = urlencoding::decode(path_part).ok()?;
        #[cfg(windows)]
        let decoded = {
            let v = decoded.into_owned();
            if v.starts_with('/') && v.as_bytes().get(2) == Some(&b':') {
                v[1..].to_string()
            } else {
                v
            }
        };
        #[cfg(not(windows))]
        let decoded = decoded.into_owned();
        let candidate = PathBuf::from(decoded);
        if candidate.is_absolute() {
            return Some(candidate);
        }
        return None;
    }

    if trimmed.starts_with("data:")
        || trimmed.starts_with("asset:")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with(&format!("{}://", PREVIEW_FILE_RELAY_SCHEME))
    {
        return None;
    }

    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() {
        Some(candidate)
    } else {
        None
    }
}

fn inline_text_file_content(file: &mut PreviewFileItem, local_path: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(local_path)
        .map_err(|e| format!("Failed to read metadata for '{}': {}", local_path.display(), e))?;
    if metadata.len() > PREVIEW_FILE_RELAY_MAX_BYTES {
        return Err(format!(
            "Local preview file is too large ({} bytes): {}",
            metadata.len(),
            local_path.display()
        ));
    }
    let content = std::fs::read_to_string(local_path).map_err(|e| {
        format!("Failed to read text preview file '{}': {}", local_path.display(), e)
    })?;
    file.content = Some(content);
    file.url = None;
    Ok(())
}

fn rewrite_local_preview_urls(
    app_handle: &AppHandle,
    conversation_id: Option<i64>,
    files: &mut [PreviewFileItem],
) -> Result<(), String> {
    let relay_state = app_handle
        .try_state::<PreviewFileRelayState>()
        .ok_or_else(|| "PreviewFileRelayState not found".to_string())?;

    for file in files.iter_mut() {
        if file.content.as_ref().is_some_and(|content| !content.trim().is_empty()) {
            continue;
        }

        let Some(raw_url) = file.url.clone() else {
            continue;
        };
        let Some(local_path) = resolve_local_file_path(&raw_url) else {
            continue;
        };

        if !local_path.exists() {
            return Err(format!("Local preview file not found: {}", local_path.display()));
        }
        if local_path.is_dir() {
            return Err(format!("Preview path is a directory: {}", local_path.display()));
        }

        match file.file_type.as_str() {
            "markdown" | "text" => {
                inline_text_file_content(file, &local_path)?;
                debug!(
                    path = %local_path.display(),
                    file_type = %file.file_type,
                    "Inlined local preview text file"
                );
            }
            "image" | "pdf" | "html" => {
                let metadata = std::fs::metadata(&local_path).map_err(|e| {
                    format!("Failed to read metadata for '{}': {}", local_path.display(), e)
                })?;
                if metadata.len() > PREVIEW_FILE_RELAY_MAX_BYTES {
                    return Err(format!(
                        "Local preview file is too large ({} bytes): {}",
                        metadata.len(),
                        local_path.display()
                    ));
                }
                let token = relay_state.register_local_file(
                    local_path.clone(),
                    file.file_type.clone(),
                    conversation_id,
                )?;
                file.url = Some(format!("{}://localhost/{}", PREVIEW_FILE_RELAY_SCHEME, token));
                debug!(
                    path = %local_path.display(),
                    file_type = %file.file_type,
                    relay_token = %token,
                    "Rewrote local preview URL to relay"
                );
            }
            _ => {}
        }
    }

    Ok(())
}

pub fn handle_preview_file_relay_request<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let token =
        request.uri().path().trim_start_matches('/').split('/').next().unwrap_or_default().trim();
    if token.is_empty() {
        return build_relay_error_response(StatusCode::BAD_REQUEST, "Missing preview relay token");
    }

    let Some(relay_state) = ctx.app_handle().try_state::<PreviewFileRelayState>() else {
        return build_relay_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Preview relay state unavailable",
        );
    };

    let entry = match relay_state.get_entry(token) {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            return build_relay_error_response(
                StatusCode::NOT_FOUND,
                "Preview relay token not found or expired",
            )
        }
        Err(e) => return build_relay_error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };

    let bytes = match std::fs::read(&entry.file_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return build_relay_error_response(
                StatusCode::NOT_FOUND,
                &format!("Failed to read preview file '{}': {}", entry.file_path.display(), e),
            )
        }
    };

    let content_type = detect_relay_content_type(&entry.file_type, &entry.file_path);
    debug!(
        token = %token,
        path = %entry.file_path.display(),
        file_type = %entry.file_type,
        conversation_id = ?entry.conversation_id,
        webview_label = %ctx.webview_label(),
        "Serving preview relay file"
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(bytes)
        .unwrap_or_else(|_| {
            build_relay_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build relay response",
            )
        })
}

#[derive(Clone)]
pub struct InteractionState {
    pending_ask_user: Arc<Mutex<HashMap<String, oneshot::Sender<AskUserQuestionDecision>>>>,
}

impl InteractionState {
    pub fn new() -> Self {
        Self { pending_ask_user: Arc::new(Mutex::new(HashMap::new())) }
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
    interaction_state.store_ask_user_request(request_id.clone(), tx).await;

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
    let mut request = request;
    request.validate()?;
    rewrite_local_preview_urls(app_handle, conversation_id, &mut request.files)?;

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
pub async fn prepare_preview_file_request_for_ui(
    app_handle: AppHandle,
    conversation_id: Option<i64>,
    request: PreviewFileRequest,
) -> Result<PreviewFileRequest, String> {
    let mut request = request;
    request.validate()?;
    rewrite_local_preview_urls(&app_handle, conversation_id, &mut request.files)?;
    Ok(request)
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

    let resolved = state.resolve_ask_user_request(&request_id, decision).await;

    if resolved {
        Ok(true)
    } else {
        Err("AskUserQuestion request not found or already resolved".to_string())
    }
}

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
type PreviewCodeDecision = Result<PreviewCodeResponse, String>;
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
pub struct PreviewCodeMetadata {
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewCodeRequest {
    pub title: String,
    pub renderer: String,
    pub code: String,
    #[serde(default, rename = "loadingMessages", alias = "loading_messages")]
    pub loading_messages: Vec<String>,
    #[serde(default, rename = "interactionMode", alias = "interaction_mode")]
    pub interaction_mode: Option<String>,
    #[serde(default)]
    pub metadata: Option<PreviewCodeMetadata>,
}

impl PreviewCodeRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("PreviewCode title cannot be empty".to_string());
        }
        if self.renderer.trim() != "html" {
            return Err(format!(
                "Unsupported preview_code renderer '{}'; only 'html' is currently supported",
                self.renderer
            ));
        }
        if self.code.trim().is_empty() {
            return Err("PreviewCode code cannot be empty".to_string());
        }
        if self.loading_messages.len() > 8 {
            return Err("PreviewCode loading_messages cannot exceed 8 items".to_string());
        }
        if self.loading_messages.iter().any(|message| message.trim().is_empty()) {
            return Err("PreviewCode loading_messages cannot contain empty items".to_string());
        }
        match self.interaction_mode() {
            "none" | "submit_once" => Ok(()),
            other => Err(format!("Unsupported preview_code interaction_mode '{}'", other)),
        }
    }

    pub fn interaction_mode(&self) -> &str {
        self.interaction_mode
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("none")
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewCodeRequestEvent {
    pub request_id: String,
    pub conversation_id: Option<i64>,
    pub title: String,
    pub renderer: String,
    pub code: String,
    #[serde(default, rename = "loadingMessages", alias = "loading_messages")]
    pub loading_messages: Vec<String>,
    #[serde(rename = "interactionMode")]
    pub interaction_mode: String,
    #[serde(default)]
    pub metadata: Option<PreviewCodeMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewCodeResponse {
    pub status: String,
    pub request_id: String,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
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

fn strip_url_query_and_fragment(raw: &str) -> &str {
    raw.split(['?', '#']).next().unwrap_or(raw)
}

fn normalize_local_path_string(mut raw_path: String) -> String {
    #[cfg(windows)]
    {
        raw_path = raw_path.replace('/', "\\");
        if raw_path.starts_with('\\') && raw_path.as_bytes().get(2) == Some(&b':') {
            raw_path = raw_path[1..].to_string();
        }
    }
    raw_path
}

fn resolve_local_file_path(raw_url: &str) -> Option<PathBuf> {
    let trimmed = raw_url.trim().trim_matches(|c| matches!(c, '"' | '\''));
    if trimmed.is_empty() {
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

    let decoded_direct_path =
        normalize_local_path_string(urlencoding::decode(trimmed).ok()?.into_owned());
    let candidate = PathBuf::from(&decoded_direct_path);
    if candidate.is_absolute() {
        return Some(candidate);
    }

    let sanitized = strip_url_query_and_fragment(trimmed);
    if sanitized.is_empty() {
        return None;
    }

    if let Ok(parsed_url) = reqwest::Url::parse(sanitized) {
        if parsed_url.scheme() == "file" {
            if parsed_url.host_str().is_some_and(|host| !host.is_empty() && host != "localhost") {
                return None;
            }
        } else if parsed_url.host_str() != Some("file") {
            return None;
        }

        let decoded_url_path =
            normalize_local_path_string(urlencoding::decode(parsed_url.path()).ok()?.into_owned());
        let candidate = PathBuf::from(decoded_url_path);
        if candidate.is_absolute() {
            return Some(candidate);
        }
    }

    None
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

pub(crate) fn inline_local_text_preview_files(files: &mut [PreviewFileItem]) -> Result<(), String> {
    for file in files.iter_mut() {
        if !matches!(file.file_type.as_str(), "markdown" | "text") {
            continue;
        }
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

        inline_text_file_content(file, &local_path)?;
        debug!(
            path = %local_path.display(),
            file_type = %file.file_type,
            "Inlined local preview text file"
        );
    }

    Ok(())
}

fn rewrite_local_preview_urls(
    app_handle: &AppHandle,
    conversation_id: Option<i64>,
    files: &mut [PreviewFileItem],
) -> Result<(), String> {
    inline_local_text_preview_files(files)?;

    let mut relay_state: Option<tauri::State<'_, PreviewFileRelayState>> = None;

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
                let relay_state = relay_state.get_or_insert(
                    app_handle
                        .try_state::<PreviewFileRelayState>()
                        .ok_or_else(|| "PreviewFileRelayState not found".to_string())?,
                );
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
    pending_ask_user_events: Arc<Mutex<HashMap<String, AskUserQuestionRequestEvent>>>,
    pending_preview_code: Arc<Mutex<HashMap<String, oneshot::Sender<PreviewCodeDecision>>>>,
    pending_preview_code_events: Arc<Mutex<HashMap<String, PreviewCodeRequestEvent>>>,
}

#[derive(Clone, Copy)]
enum InteractionCleanupKind {
    AskUser,
    PreviewCode,
}

struct PendingInteractionCleanup {
    interaction_state: InteractionState,
    request_id: Option<String>,
    kind: InteractionCleanupKind,
}

impl PendingInteractionCleanup {
    fn new(
        interaction_state: &InteractionState,
        request_id: String,
        kind: InteractionCleanupKind,
    ) -> Self {
        Self { interaction_state: interaction_state.clone(), request_id: Some(request_id), kind }
    }

    fn disarm(&mut self) {
        self.request_id = None;
    }
}

impl Drop for PendingInteractionCleanup {
    fn drop(&mut self) {
        let Some(request_id) = self.request_id.take() else {
            return;
        };
        let interaction_state = self.interaction_state.clone();
        let kind = self.kind;
        tauri::async_runtime::spawn(async move {
            match kind {
                InteractionCleanupKind::AskUser => {
                    interaction_state.remove_ask_user_request(&request_id).await;
                }
                InteractionCleanupKind::PreviewCode => {
                    interaction_state.remove_preview_code_request(&request_id).await;
                }
            }
        });
    }
}

impl InteractionState {
    pub fn new() -> Self {
        Self {
            pending_ask_user: Arc::new(Mutex::new(HashMap::new())),
            pending_ask_user_events: Arc::new(Mutex::new(HashMap::new())),
            pending_preview_code: Arc::new(Mutex::new(HashMap::new())),
            pending_preview_code_events: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn store_ask_user_request(
        &self,
        event: AskUserQuestionRequestEvent,
        sender: oneshot::Sender<AskUserQuestionDecision>,
    ) {
        let request_id = event.request_id.clone();
        let mut pending = self.pending_ask_user.lock().await;
        pending.insert(request_id, sender);
        drop(pending);

        let mut pending_events = self.pending_ask_user_events.lock().await;
        pending_events.insert(event.request_id.clone(), event);
    }

    pub async fn get_ask_user_request(
        &self,
        request_id: &str,
    ) -> Option<AskUserQuestionRequestEvent> {
        let pending_events = self.pending_ask_user_events.lock().await;
        pending_events.get(request_id).cloned()
    }

    pub async fn remove_ask_user_request(&self, request_id: &str) {
        let mut pending = self.pending_ask_user.lock().await;
        pending.remove(request_id);
        drop(pending);

        let mut pending_events = self.pending_ask_user_events.lock().await;
        pending_events.remove(request_id);
    }

    pub async fn list_requests_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Vec<AskUserQuestionRequestEvent> {
        let pending_events = self.pending_ask_user_events.lock().await;
        pending_events
            .values()
            .filter(|event| event.conversation_id == Some(conversation_id))
            .cloned()
            .collect()
    }

    pub async fn resolve_ask_user_request(
        &self,
        request_id: &str,
        decision: AskUserQuestionDecision,
    ) -> bool {
        let mut pending = self.pending_ask_user.lock().await;
        let sender = pending.remove(request_id);
        drop(pending);

        let mut pending_events = self.pending_ask_user_events.lock().await;
        pending_events.remove(request_id);
        drop(pending_events);

        if let Some(sender) = sender {
            sender.send(decision).is_ok()
        } else {
            false
        }
    }

    pub async fn store_preview_code_request(
        &self,
        event: PreviewCodeRequestEvent,
        sender: oneshot::Sender<PreviewCodeDecision>,
    ) {
        let request_id = event.request_id.clone();
        let mut pending = self.pending_preview_code.lock().await;
        pending.insert(request_id, sender);
        drop(pending);

        let mut pending_events = self.pending_preview_code_events.lock().await;
        pending_events.insert(event.request_id.clone(), event);
    }

    pub async fn get_preview_code_request(
        &self,
        request_id: &str,
    ) -> Option<PreviewCodeRequestEvent> {
        let pending_events = self.pending_preview_code_events.lock().await;
        pending_events.get(request_id).cloned()
    }

    pub async fn remove_preview_code_request(&self, request_id: &str) {
        let mut pending = self.pending_preview_code.lock().await;
        pending.remove(request_id);
        drop(pending);

        let mut pending_events = self.pending_preview_code_events.lock().await;
        pending_events.remove(request_id);
    }

    pub async fn list_preview_code_requests_for_conversation(
        &self,
        conversation_id: i64,
    ) -> Vec<PreviewCodeRequestEvent> {
        let pending_events = self.pending_preview_code_events.lock().await;
        pending_events
            .values()
            .filter(|event| event.conversation_id == Some(conversation_id))
            .cloned()
            .collect()
    }

    pub async fn resolve_preview_code_request(
        &self,
        request_id: &str,
        decision: PreviewCodeDecision,
    ) -> bool {
        let mut pending = self.pending_preview_code.lock().await;
        let sender = pending.remove(request_id);
        drop(pending);

        let mut pending_events = self.pending_preview_code_events.lock().await;
        pending_events.remove(request_id);
        drop(pending_events);

        if let Some(sender) = sender {
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
    interaction_state.store_ask_user_request(event.clone(), tx).await;
    let mut cleanup_guard = PendingInteractionCleanup::new(
        interaction_state,
        request_id.clone(),
        InteractionCleanupKind::AskUser,
    );

    info!(
        request_id = %request_id,
        conversation_id = ?conversation_id,
        "Requesting AskUserQuestion input from frontend"
    );

    let delivered_to_feishu = if let Some(conversation_id) = conversation_id {
        match crate::feishu::try_deliver_ask_user_question_to_feishu(
            app_handle,
            conversation_id,
            &event,
        )
        .await
        {
            Ok(delivered) => delivered,
            Err(error) => {
                interaction_state.remove_ask_user_request(&request_id).await;
                cleanup_guard.disarm();
                warn!(
                    request_id = %request_id,
                    conversation_id,
                    error = %error,
                    "Failed to deliver AskUserQuestion to Feishu"
                );
                return Err(error);
            }
        }
    } else {
        false
    };

    if let Err(e) = app_handle.emit("ask-user-question-request", &event) {
        if delivered_to_feishu {
            warn!(
                request_id = %request_id,
                error = %e,
                "AskUserQuestion frontend emit failed, but Feishu delivery is active"
            );
        } else {
            interaction_state.remove_ask_user_request(&request_id).await;
            cleanup_guard.disarm();
            warn!(request_id = %request_id, error = %e, "Failed to emit ask-user-question-request");
            return Err("Failed to emit AskUserQuestion event".to_string());
        }
    }

    // Notify Butler main conversation if this is a Butler sub-task
    if let Some(conversation_id) = conversation_id {
        if let Err(error) =
            crate::api::butler_api::emit_butler_task_ask_user_attention(app_handle, conversation_id)
                .await
        {
            warn!(
                conversation_id,
                error = %error,
                "Failed to send Butler attention for ask_user_question"
            );
        }
    }

    let result = match rx.await {
        Ok(result) => result,
        Err(_) => Err("AskUserQuestion request was cancelled".to_string()),
    };
    interaction_state.remove_ask_user_request(&request_id).await;
    cleanup_guard.disarm();
    result
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

pub async fn request_preview_code(
    app_handle: &AppHandle,
    interaction_state: &InteractionState,
    conversation_id: Option<i64>,
    request: PreviewCodeRequest,
) -> Result<PreviewCodeResponse, String> {
    request.validate()?;

    let request_id = uuid::Uuid::new_v4().to_string();
    let interaction_mode = request.interaction_mode().to_string();
    let event = PreviewCodeRequestEvent {
        request_id: request_id.clone(),
        conversation_id,
        title: request.title,
        renderer: request.renderer,
        code: request.code,
        loading_messages: request.loading_messages,
        interaction_mode,
        metadata: request.metadata,
    };

    let interaction_mode = event.interaction_mode.clone();

    if interaction_mode == "none" {
        if let Err(e) = app_handle.emit("preview-code-request", &event) {
            warn!(request_id = %request_id, error = %e, "Failed to emit preview-code-request");
            return Err("Failed to emit PreviewCode event".to_string());
        }

        return Ok(PreviewCodeResponse {
            status: "displayed".to_string(),
            request_id,
            payload: None,
        });
    }

    let (tx, rx) = oneshot::channel::<PreviewCodeDecision>();
    interaction_state.store_preview_code_request(event.clone(), tx).await;
    let mut cleanup_guard = PendingInteractionCleanup::new(
        interaction_state,
        request_id.clone(),
        InteractionCleanupKind::PreviewCode,
    );

    if let Err(e) = app_handle.emit("preview-code-request", &event) {
        interaction_state.remove_preview_code_request(&request_id).await;
        cleanup_guard.disarm();
        warn!(request_id = %request_id, error = %e, "Failed to emit preview-code-request");
        return Err("Failed to emit PreviewCode event".to_string());
    }

    let result = match rx.await {
        Ok(result) => result,
        Err(_) => Err("PreviewCode request was cancelled".to_string()),
    };
    interaction_state.remove_preview_code_request(&request_id).await;
    cleanup_guard.disarm();
    result
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
pub async fn list_preview_code_requests_for_conversation(
    app_handle: AppHandle,
    conversation_id: i64,
) -> Result<Vec<PreviewCodeRequestEvent>, String> {
    let state = app_handle
        .try_state::<InteractionState>()
        .ok_or_else(|| "InteractionState not found".to_string())?;
    Ok(state.list_preview_code_requests_for_conversation(conversation_id).await)
}

pub(crate) async fn resolve_ask_user_question_response(
    app_handle: &AppHandle,
    request_id: &str,
    answers: Option<HashMap<String, String>>,
    cancelled: bool,
) -> Result<bool, String> {
    let state = app_handle
        .try_state::<InteractionState>()
        .ok_or_else(|| "InteractionState not found".to_string())?;

    let decision = if cancelled {
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

    let resolved = state.resolve_ask_user_request(request_id, decision).await;

    if resolved {
        Ok(true)
    } else {
        Err("AskUserQuestion request not found or already resolved".to_string())
    }
}

pub(crate) async fn resolve_preview_code_response(
    app_handle: &AppHandle,
    request_id: &str,
    payload: Option<serde_json::Value>,
    dismissed: bool,
) -> Result<bool, String> {
    let state = app_handle
        .try_state::<InteractionState>()
        .ok_or_else(|| "InteractionState not found".to_string())?;

    let decision = if dismissed {
        Ok(PreviewCodeResponse {
            status: "dismissed".to_string(),
            request_id: request_id.to_string(),
            payload: None,
        })
    } else {
        let Some(payload) = payload else {
            return Err("Missing preview_code payload".to_string());
        };
        Ok(PreviewCodeResponse {
            status: "submitted".to_string(),
            request_id: request_id.to_string(),
            payload: Some(payload),
        })
    };

    let resolved = state.resolve_preview_code_request(request_id, decision).await;

    if resolved {
        Ok(true)
    } else {
        Err("PreviewCode request not found or already resolved".to_string())
    }
}

#[tauri::command]
pub async fn submit_ask_user_question_response(
    app_handle: AppHandle,
    request_id: String,
    answers: Option<HashMap<String, String>>,
    cancelled: Option<bool>,
) -> Result<bool, String> {
    resolve_ask_user_question_response(
        &app_handle,
        &request_id,
        answers,
        cancelled.unwrap_or(false),
    )
    .await
}

#[tauri::command]
pub async fn submit_preview_code_response(
    app_handle: AppHandle,
    request_id: String,
    payload: Option<serde_json::Value>,
    dismissed: Option<bool>,
) -> Result<bool, String> {
    resolve_preview_code_response(&app_handle, &request_id, payload, dismissed.unwrap_or(false))
        .await
}

#[cfg(test)]
mod tests {
    use super::{
        inline_local_text_preview_files, inline_text_file_content, resolve_local_file_path,
        PreviewCodeRequest, PreviewFileItem,
    };
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn preview_code_request_accepts_html_submit_once() {
        let request = PreviewCodeRequest {
            title: "compound_interest".to_string(),
            renderer: "html".to_string(),
            code: "<div>Hello</div>".to_string(),
            loading_messages: vec!["正在生成交互面板".to_string()],
            interaction_mode: Some("submit_once".to_string()),
            metadata: None,
        };

        assert!(request.validate().is_ok());
    }

    #[test]
    fn preview_code_request_defaults_to_display_only_when_mode_is_omitted() {
        let request = PreviewCodeRequest {
            title: "display_only".to_string(),
            renderer: "html".to_string(),
            code: "<div>Hello</div>".to_string(),
            loading_messages: vec![],
            interaction_mode: None,
            metadata: None,
        };

        assert_eq!(request.interaction_mode(), "none");
        assert!(request.validate().is_ok());
    }

    #[test]
    fn preview_code_request_rejects_unknown_renderer() {
        let request = PreviewCodeRequest {
            title: "bad".to_string(),
            renderer: "markdown".to_string(),
            code: "<div>Hello</div>".to_string(),
            loading_messages: vec![],
            interaction_mode: Some("submit_once".to_string()),
            metadata: None,
        };

        assert!(request.validate().is_err());
    }

    #[test]
    fn inline_text_file_content_reads_local_markdown_file() {
        let temp_path =
            std::env::temp_dir().join(format!("aipp-preview-inline-{}.md", std::process::id()));
        fs::write(&temp_path, "# 标题\n正文内容").expect("temp markdown should be written");

        let mut file = PreviewFileItem {
            title: "demo".to_string(),
            file_type: "markdown".to_string(),
            content: None,
            url: Some(temp_path.to_string_lossy().to_string()),
            language: Some("markdown".to_string()),
            description: None,
        };

        inline_text_file_content(&mut file, &temp_path).expect("local markdown should inline");

        assert_eq!(file.content.as_deref(), Some("# 标题\n正文内容"));
        assert!(file.url.is_none());

        let _ = fs::remove_file(&temp_path);
    }

    #[test]
    fn inline_local_text_preview_files_reads_absolute_text_path() {
        let temp_dir = std::env::temp_dir()
            .join(format!("aipp-preview-inline-text-{}-中文目录", std::process::id()));
        fs::create_dir_all(&temp_dir).expect("temp preview dir should be created");
        let temp_path = temp_dir.join("第二章样稿.md");
        fs::write(&temp_path, "第二章完整正文").expect("temp text file should be written");

        let mut files = vec![PreviewFileItem {
            title: "第二章样稿.md".to_string(),
            file_type: "text".to_string(),
            content: None,
            url: Some(temp_path.to_string_lossy().to_string()),
            language: None,
            description: None,
        }];

        inline_local_text_preview_files(&mut files)
            .expect("absolute local text file should inline");

        assert_eq!(files[0].content.as_deref(), Some("第二章完整正文"));
        assert!(files[0].url.is_none());

        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn resolve_local_file_path_accepts_file_url_with_query_and_fragment() {
        #[cfg(windows)]
        let (raw_url, expected) =
            ("file:///C:/Temp/demo.txt?download=1#L3", PathBuf::from(r"C:\Temp\demo.txt"));
        #[cfg(not(windows))]
        let (raw_url, expected) =
            ("file:///tmp/demo.txt?download=1#L3", PathBuf::from("/tmp/demo.txt"));

        assert_eq!(resolve_local_file_path(raw_url), Some(expected));
    }

    #[test]
    fn resolve_local_file_path_accepts_editor_style_file_url() {
        #[cfg(windows)]
        let (raw_url, expected) =
            ("vscode://file/C%3A/Temp/demo.txt", PathBuf::from(r"C:\Temp\demo.txt"));
        #[cfg(not(windows))]
        let (raw_url, expected) = ("vscode://file/tmp/demo.txt", PathBuf::from("/tmp/demo.txt"));

        assert_eq!(resolve_local_file_path(raw_url), Some(expected));
    }

    #[test]
    fn resolve_local_file_path_rejects_preview_relay_urls() {
        assert!(resolve_local_file_path("aipp-preview://localhost/token-123").is_none());
    }
}

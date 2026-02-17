use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const MANIFEST_VERSION: u32 = 1;
const MAX_PREVIEW_FILE_BYTES: u64 = 2 * 1024 * 1024;
const ARTIFACT_EVENT_NAME: &str = "artifact-manifest-updated";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactWorkspaceResponse {
    pub workspace_path: String,
    pub manifest_path: String,
    pub recommended_flow: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowArtifactRequest {
    pub conversation_id: i64,
    pub artifact_key: String,
    pub entry_file: String,
    pub title: Option<String>,
    pub language: Option<String>,
    pub preview_type: Option<String>,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowArtifactResponse {
    pub artifact_key: String,
    pub title: String,
    pub language: String,
    pub preview_type: String,
    pub entry_file: String,
    pub absolute_path: String,
    pub published: bool,
    pub updated_at: String,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationArtifactItem {
    pub artifact_key: String,
    pub title: String,
    pub language: String,
    pub preview_type: String,
    pub entry_file: String,
    pub code: String,
    pub updated_at: String,
    pub db_id: Option<String>,
    pub assistant_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactManifest {
    version: u32,
    conversation_id: i64,
    artifacts: Vec<ArtifactManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactManifestEntry {
    artifact_key: String,
    title: String,
    artifact_dir: String,
    entry_file: String,
    language: String,
    preview_type: String,
    status: String,
    files: Vec<String>,
    created_at: String,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    db_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactManifestEvent {
    conversation_id: i64,
    action: String,
    artifact: ArtifactManifestEventArtifact,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactManifestEventArtifact {
    artifact_key: String,
    title: String,
    language: String,
    preview_type: String,
    entry_file: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct ArtifactJsonFile {
    schema_version: u32,
    conversation_id: i64,
    artifact_key: String,
    title: String,
    entry_file: String,
    language: String,
    preview_type: String,
    files: Vec<String>,
    updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    db_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_id: Option<i64>,
}

#[derive(Debug, Clone)]
struct WorkspaceContext {
    workspace_path: PathBuf,
    manifest_path: PathBuf,
}

#[tauri::command]
pub async fn list_conversation_artifacts(
    app_handle: AppHandle,
    conversation_id: i64,
) -> Result<Vec<ConversationArtifactItem>, String> {
    list_published_artifacts(&app_handle, conversation_id)
}

pub fn get_artifact_workspace(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<ArtifactWorkspaceResponse, String> {
    let context = ensure_workspace_context(app_handle, conversation_id)?;
    Ok(ArtifactWorkspaceResponse {
        workspace_path: context.workspace_path.to_string_lossy().to_string(),
        manifest_path: context.manifest_path.to_string_lossy().to_string(),
        recommended_flow: vec![
            "get_artifact_workspace".to_string(),
            "write_file/edit_file".to_string(),
            "show_artifact".to_string(),
        ],
    })
}

pub fn show_artifact(
    app_handle: &AppHandle,
    request: ShowArtifactRequest,
) -> Result<ShowArtifactResponse, String> {
    let context = ensure_workspace_context(app_handle, request.conversation_id)?;
    let mut manifest = read_manifest(&context, request.conversation_id)?;

    let normalized_artifact_key = sanitize_relative_path(&request.artifact_key, "artifact_key")?;
    let normalized_entry_file = sanitize_relative_path(&request.entry_file, "entry_file")?;

    let artifact_dir_relative = PathBuf::from("artifacts").join(&normalized_artifact_key);
    let artifact_dir_absolute = context.workspace_path.join(&artifact_dir_relative);
    let entry_absolute = artifact_dir_absolute.join(&normalized_entry_file);
    if !entry_absolute.exists() {
        return Err(format!("Artifact entry file does not exist: {}", entry_absolute.display()));
    }
    if !entry_absolute.is_file() {
        return Err(format!("Artifact entry path is not a file: {}", entry_absolute.display()));
    }
    ensure_within_workspace(&context.workspace_path, &entry_absolute)?;
    ensure_preview_size(&entry_absolute)?;

    let inferred_language = infer_language_from_path(&entry_absolute);
    let language = request.language.as_deref().map(normalize_language).unwrap_or(inferred_language);
    if !is_supported_language(&language) {
        return Err(format!(
            "Unsupported artifact language '{}'. Supported: html, markdown, mermaid, drawio, react/vue and code component formats.",
            language
        ));
    }
    let preview_type = request
        .preview_type
        .as_deref()
        .map(normalize_language)
        .unwrap_or_else(|| infer_preview_type(&language));
    if !is_supported_language(&preview_type) {
        return Err(format!("Unsupported preview_type '{}'", preview_type));
    }

    let now = Utc::now().to_rfc3339();
    let artifact_key = normalize_path_string(&normalized_artifact_key);
    let entry_file = normalize_path_string(&normalized_entry_file);
    let artifact_dir = normalize_path_string(&artifact_dir_relative);
    let title = request.title.clone().unwrap_or_else(|| artifact_key.clone());
    let files = collect_artifact_files(&artifact_dir_absolute)?;

    let mut entry_to_persist = ArtifactManifestEntry {
        artifact_key: artifact_key.clone(),
        title,
        artifact_dir,
        entry_file: entry_file.clone(),
        language: language.clone(),
        preview_type: preview_type.clone(),
        status: "published".to_string(),
        files,
        created_at: now.clone(),
        updated_at: now.clone(),
        db_id: request.db_id.clone(),
        assistant_id: request.assistant_id,
    };

    if let Some(existing) =
        manifest.artifacts.iter_mut().find(|artifact| artifact.artifact_key == artifact_key)
    {
        entry_to_persist.created_at = existing.created_at.clone();
        if request.title.is_none() && !existing.title.trim().is_empty() {
            entry_to_persist.title = existing.title.clone();
        }
        if request.db_id.is_none() {
            entry_to_persist.db_id = existing.db_id.clone();
        }
        if request.assistant_id.is_none() {
            entry_to_persist.assistant_id = existing.assistant_id;
        }
        *existing = entry_to_persist.clone();
    } else {
        manifest.artifacts.push(entry_to_persist.clone());
    }

    write_manifest(&context, &manifest)?;
    write_artifact_json(&artifact_dir_absolute, request.conversation_id, &entry_to_persist)?;

    let event_payload = ArtifactManifestEvent {
        conversation_id: request.conversation_id,
        action: "upsert".to_string(),
        artifact: ArtifactManifestEventArtifact {
            artifact_key: entry_to_persist.artifact_key.clone(),
            title: entry_to_persist.title.clone(),
            language: entry_to_persist.language.clone(),
            preview_type: entry_to_persist.preview_type.clone(),
            entry_file: entry_to_persist.entry_file.clone(),
            updated_at: entry_to_persist.updated_at.clone(),
        },
    };
    app_handle
        .emit(ARTIFACT_EVENT_NAME, &event_payload)
        .map_err(|e| format!("Failed to emit artifact manifest update event: {}", e))?;

    Ok(ShowArtifactResponse {
        artifact_key: entry_to_persist.artifact_key,
        title: entry_to_persist.title,
        language: entry_to_persist.language,
        preview_type: entry_to_persist.preview_type,
        entry_file: entry_to_persist.entry_file,
        absolute_path: entry_absolute.to_string_lossy().to_string(),
        published: true,
        updated_at: entry_to_persist.updated_at,
        db_id: entry_to_persist.db_id,
        assistant_id: entry_to_persist.assistant_id,
    })
}

pub fn list_published_artifacts(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<Vec<ConversationArtifactItem>, String> {
    let context = ensure_workspace_context(app_handle, conversation_id)?;
    let manifest = read_manifest(&context, conversation_id)?;
    let mut items = Vec::new();

    for artifact in manifest.artifacts.iter().filter(|artifact| artifact.status == "published") {
        let entry_path =
            context.workspace_path.join(&artifact.artifact_dir).join(&artifact.entry_file);
        if !entry_path.exists() {
            return Err(format!(
                "Published artifact entry file not found: {}",
                entry_path.display()
            ));
        }
        ensure_within_workspace(&context.workspace_path, &entry_path)?;
        ensure_preview_size(&entry_path)?;
        let code = fs::read_to_string(&entry_path).map_err(|e| {
            format!("Failed to read artifact entry file '{}': {}", entry_path.display(), e)
        })?;
        items.push(ConversationArtifactItem {
            artifact_key: artifact.artifact_key.clone(),
            title: artifact.title.clone(),
            language: artifact.language.clone(),
            preview_type: artifact.preview_type.clone(),
            entry_file: format!("{}/{}", artifact.artifact_key, artifact.entry_file),
            code,
            updated_at: artifact.updated_at.clone(),
            db_id: artifact.db_id.clone(),
            assistant_id: artifact.assistant_id,
        });
    }

    items.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    Ok(items)
}

fn ensure_workspace_context(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<WorkspaceContext, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
    let workspace_path =
        app_data_dir.join("artifact_workspaces").join(format!("conversation_{}", conversation_id));
    let metadata_dir = workspace_path.join(".aipp");
    fs::create_dir_all(&metadata_dir)
        .map_err(|e| format!("Failed to create artifact metadata directory: {}", e))?;
    let manifest_path = metadata_dir.join("artifacts.json");
    if !manifest_path.exists() {
        let initial_manifest =
            ArtifactManifest { version: MANIFEST_VERSION, conversation_id, artifacts: Vec::new() };
        write_manifest_to_path(&manifest_path, &initial_manifest)?;
    }

    Ok(WorkspaceContext { workspace_path, manifest_path })
}

fn read_manifest(
    context: &WorkspaceContext,
    conversation_id: i64,
) -> Result<ArtifactManifest, String> {
    let content = fs::read_to_string(&context.manifest_path).map_err(|e| {
        format!("Failed to read artifact manifest '{}': {}", context.manifest_path.display(), e)
    })?;
    let mut manifest: ArtifactManifest = serde_json::from_str(&content).map_err(|e| {
        format!("Failed to parse artifact manifest '{}': {}", context.manifest_path.display(), e)
    })?;

    if manifest.conversation_id != conversation_id {
        return Err(format!(
            "Artifact manifest conversation_id mismatch: expected {}, got {}",
            conversation_id, manifest.conversation_id
        ));
    }

    if manifest.version == 0 {
        manifest.version = MANIFEST_VERSION;
    }
    Ok(manifest)
}

fn write_manifest(context: &WorkspaceContext, manifest: &ArtifactManifest) -> Result<(), String> {
    write_manifest_to_path(&context.manifest_path, manifest)
}

fn write_manifest_to_path(path: &Path, manifest: &ArtifactManifest) -> Result<(), String> {
    let content = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize artifact manifest: {}", e))?;
    fs::write(path, format!("{}\n", content))
        .map_err(|e| format!("Failed to write artifact manifest '{}': {}", path.display(), e))
}

fn write_artifact_json(
    artifact_dir_absolute: &Path,
    conversation_id: i64,
    artifact: &ArtifactManifestEntry,
) -> Result<(), String> {
    let artifact_json = ArtifactJsonFile {
        schema_version: MANIFEST_VERSION,
        conversation_id,
        artifact_key: artifact.artifact_key.clone(),
        title: artifact.title.clone(),
        entry_file: artifact.entry_file.clone(),
        language: artifact.language.clone(),
        preview_type: artifact.preview_type.clone(),
        files: artifact.files.clone(),
        updated_at: artifact.updated_at.clone(),
        db_id: artifact.db_id.clone(),
        assistant_id: artifact.assistant_id,
    };
    let content = serde_json::to_string_pretty(&artifact_json)
        .map_err(|e| format!("Failed to serialize artifact.json: {}", e))?;
    let path = artifact_dir_absolute.join("artifact.json");
    fs::write(&path, format!("{}\n", content))
        .map_err(|e| format!("Failed to write artifact.json '{}': {}", path.display(), e))
}

fn collect_artifact_files(artifact_dir_absolute: &Path) -> Result<Vec<String>, String> {
    if !artifact_dir_absolute.is_dir() {
        return Err(format!("Artifact directory not found: {}", artifact_dir_absolute.display()));
    }
    let mut files = Vec::new();
    collect_artifact_files_recursive(artifact_dir_absolute, artifact_dir_absolute, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_artifact_files_recursive(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(current_dir).map_err(|e| {
        format!("Failed to read artifact directory '{}': {}", current_dir.display(), e)
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read artifact directory entry: {}", e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_artifact_files_recursive(base_dir, &path, files)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("artifact.json") {
            continue;
        }
        let relative = path.strip_prefix(base_dir).map_err(|e| {
            format!(
                "Failed to strip artifact base path '{}' from '{}': {}",
                base_dir.display(),
                path.display(),
                e
            )
        })?;
        files.push(normalize_path_string(relative));
    }
    Ok(())
}

fn sanitize_relative_path(raw: &str, field_name: &str) -> Result<PathBuf, String> {
    let normalized = raw.trim().replace('\\', "/");
    if normalized.is_empty() {
        return Err(format!("{} cannot be empty", field_name));
    }
    let path = Path::new(&normalized);
    if path.is_absolute() {
        return Err(format!("{} must be a relative path", field_name));
    }
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => output.push(value),
            _ => {
                return Err(format!(
                    "{} contains invalid segment '{}'",
                    field_name,
                    component.as_os_str().to_string_lossy()
                ))
            }
        }
    }
    if output.as_os_str().is_empty() {
        return Err(format!("{} cannot be empty", field_name));
    }
    Ok(output)
}

fn normalize_language(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "md" => "markdown".to_string(),
        "htm" => "html".to_string(),
        "drawio:xml" => "drawio".to_string(),
        "typescript" => "ts".to_string(),
        "javascript" => "js".to_string(),
        value => value.to_string(),
    }
}

fn infer_language_from_path(path: &Path) -> String {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("text").to_lowercase();
    match extension.as_str() {
        "md" => "markdown".to_string(),
        "htm" => "html".to_string(),
        "mmd" => "mermaid".to_string(),
        "drawio" => "drawio".to_string(),
        "xml" => {
            let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            if file_name.contains("drawio") {
                "drawio".to_string()
            } else {
                "xml".to_string()
            }
        }
        "tsx" | "jsx" | "vue" | "svg" | "html" | "mermaid" | "ts" | "js" => extension,
        _ => "text".to_string(),
    }
}

fn infer_preview_type(language: &str) -> String {
    match language {
        "markdown" => "markdown".to_string(),
        "text" => "markdown".to_string(),
        other => other.to_string(),
    }
}

fn is_supported_language(language: &str) -> bool {
    matches!(
        language,
        "powershell"
            | "applescript"
            | "mermaid"
            | "xml"
            | "svg"
            | "html"
            | "markdown"
            | "drawio"
            | "react"
            | "jsx"
            | "vue"
            | "tsx"
            | "ts"
            | "js"
    )
}

fn ensure_preview_size(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|e| format!("Failed to read file metadata '{}': {}", path.display(), e))?;
    if metadata.len() > MAX_PREVIEW_FILE_BYTES {
        return Err(format!(
            "Artifact entry file is too large ({} bytes), max allowed is {} bytes",
            metadata.len(),
            MAX_PREVIEW_FILE_BYTES
        ));
    }
    Ok(())
}

fn ensure_within_workspace(workspace_root: &Path, target_path: &Path) -> Result<(), String> {
    let workspace_canonical = workspace_root.canonicalize().map_err(|e| {
        format!("Failed to canonicalize workspace root '{}': {}", workspace_root.display(), e)
    })?;
    let target_canonical = target_path.canonicalize().map_err(|e| {
        format!("Failed to canonicalize target path '{}': {}", target_path.display(), e)
    })?;
    if !target_canonical.starts_with(&workspace_canonical) {
        return Err(format!(
            "Path '{}' is outside artifact workspace '{}'",
            target_path.display(),
            workspace_root.display()
        ));
    }
    Ok(())
}

fn normalize_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

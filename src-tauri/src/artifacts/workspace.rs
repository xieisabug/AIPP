use std::fs;
use std::path::{Component, Path, PathBuf};
#[cfg(desktop)]
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
#[cfg(desktop)]
use tokio::time::{sleep, Instant};

#[cfg(desktop)]
use crate::api::export_api::render_markdown_preview_html;
#[cfg(desktop)]
use crate::artifacts::code_utils::{
    extract_component_name, extract_vue_component_name, is_react_component, is_vue_component,
};
#[cfg(desktop)]
use crate::artifacts::react_preview::ReactPreviewManager;
#[cfg(desktop)]
use crate::artifacts::shared_components::SharedPreviewUtils;
#[cfg(desktop)]
use crate::artifacts::vue_preview::VuePreviewManager;
#[cfg(desktop)]
use crate::mcp::builtin_mcp::search::browser::BrowserManager;
#[cfg(desktop)]
use base64::{engine::general_purpose::STANDARD, Engine as _};
#[cfg(desktop)]
use chromiumoxide::browser::{Browser, BrowserConfig};
#[cfg(desktop)]
use chromiumoxide::cdp::browser_protocol::{
    emulation::SetDeviceMetricsOverrideParams, page::CaptureScreenshotFormat,
};
#[cfg(desktop)]
use chromiumoxide::page::ScreenshotParams;
#[cfg(desktop)]
use futures::StreamExt;

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
pub struct CaptureArtifactScreenshotRequest {
    pub conversation_id: i64,
    pub artifact_key: String,
    pub entry_file: String,
    pub language: Option<String>,
    pub preview_type: Option<String>,
    pub output_mode: Option<String>,
    pub selector: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureArtifactScreenshotResponse {
    pub artifact_key: String,
    pub entry_file: String,
    pub language: String,
    pub preview_type: String,
    pub output_mode: String,
    pub width: u32,
    pub height: u32,
    pub mime_type: String,
    pub base64: Option<String>,
    pub path: Option<String>,
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

#[cfg(desktop)]
struct PreparedArtifactCapture {
    artifact_key: String,
    entry_file: String,
    language: String,
    preview_type: String,
    code: String,
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
            "capture_artifact_screenshot".to_string(),
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

#[cfg(desktop)]
pub async fn capture_artifact_screenshot(
    app_handle: &AppHandle,
    request: CaptureArtifactScreenshotRequest,
) -> Result<CaptureArtifactScreenshotResponse, String> {
    let prepared = prepare_artifact_capture(app_handle, &request)?;
    let output_mode = normalize_screenshot_output_mode(request.output_mode.as_deref())?;
    let width = request.width.unwrap_or(800).clamp(1, 4096);
    let height = request.height.unwrap_or(600).clamp(1, 4096);
    let delay_ms = request.delay_ms.unwrap_or(300).clamp(0, 10_000);

    let screenshot_bytes = match prepared.preview_type.as_str() {
        "html" | "svg" | "xml" => {
            let html = build_html_like_capture_document(&prepared.code);
            capture_html_screenshot(
                app_handle,
                &html,
                request.selector.as_deref(),
                width,
                height,
                delay_ms,
            )
            .await?
        }
        "markdown" | "md" => {
            let html = render_markdown_preview_html(&prepared.code);
            capture_html_screenshot(
                app_handle,
                &html,
                request.selector.as_deref(),
                width,
                height,
                delay_ms,
            )
            .await?
        }
        "react" | "jsx" => {
            capture_component_preview_screenshot(
                app_handle,
                &prepared,
                request.selector.as_deref(),
                width,
                height,
                delay_ms,
            )
            .await?
        }
        "vue" => {
            capture_component_preview_screenshot(
                app_handle,
                &prepared,
                request.selector.as_deref(),
                width,
                height,
                delay_ms,
            )
            .await?
        }
        "tsx" | "ts" | "js" => {
            if is_vue_component(&prepared.code) || is_react_component(&prepared.code) {
                capture_component_preview_screenshot(
                    app_handle,
                    &prepared,
                    request.selector.as_deref(),
                    width,
                    height,
                    delay_ms,
                )
                .await?
            } else {
                return Err(
                    "The artifact entry is ts/js-style code but is not a complete React or Vue component."
                        .to_string(),
                );
            }
        }
        "mermaid" | "drawio" | "powershell" | "applescript" => {
            return Err(format!(
                "capture_artifact_screenshot does not currently support preview_type '{}'",
                prepared.preview_type
            ));
        }
        other => {
            return Err(format!(
                "capture_artifact_screenshot does not support preview_type '{}'",
                other
            ));
        }
    };

    let (base64, path) = match output_mode {
        "base64" => (Some(STANDARD.encode(&screenshot_bytes)), None),
        "path" => {
            let path = write_artifact_screenshot_temp_file(
                app_handle,
                &prepared.artifact_key,
                &prepared.entry_file,
                &screenshot_bytes,
            )?;
            (None, Some(path))
        }
        _ => unreachable!("normalize_screenshot_output_mode must validate output mode"),
    };

    Ok(CaptureArtifactScreenshotResponse {
        artifact_key: prepared.artifact_key,
        entry_file: prepared.entry_file,
        language: prepared.language,
        preview_type: prepared.preview_type,
        output_mode: output_mode.to_string(),
        width,
        height,
        mime_type: "image/png".to_string(),
        base64,
        path,
    })
}

#[cfg(not(desktop))]
pub async fn capture_artifact_screenshot(
    _app_handle: &AppHandle,
    _request: CaptureArtifactScreenshotRequest,
) -> Result<CaptureArtifactScreenshotResponse, String> {
    Err("capture_artifact_screenshot is only supported on desktop".to_string())
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

#[cfg(desktop)]
fn prepare_artifact_capture(
    app_handle: &AppHandle,
    request: &CaptureArtifactScreenshotRequest,
) -> Result<PreparedArtifactCapture, String> {
    let context = ensure_workspace_context(app_handle, request.conversation_id)?;
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
        return Err(format!("Unsupported artifact language '{}'", language));
    }

    let preview_type = request
        .preview_type
        .as_deref()
        .map(normalize_language)
        .unwrap_or_else(|| infer_preview_type(&language));
    if !is_supported_language(&preview_type) {
        return Err(format!("Unsupported preview_type '{}'", preview_type));
    }

    let code = fs::read_to_string(&entry_absolute).map_err(|e| {
        format!("Failed to read artifact entry file '{}': {}", entry_absolute.display(), e)
    })?;

    Ok(PreparedArtifactCapture {
        artifact_key: normalize_path_string(&normalized_artifact_key),
        entry_file: normalize_path_string(&normalized_entry_file),
        language,
        preview_type,
        code,
    })
}

#[cfg(desktop)]
async fn capture_component_preview_screenshot(
    app_handle: &AppHandle,
    prepared: &PreparedArtifactCapture,
    selector: Option<&str>,
    width: u32,
    height: u32,
    delay_ms: u64,
) -> Result<Vec<u8>, String> {
    let session = launch_component_preview(app_handle, prepared).await?;
    let capture_result =
        capture_url_screenshot(app_handle, &session.url, selector, width, height, delay_ms).await;
    session.cleanup(app_handle);
    capture_result
}

#[cfg(desktop)]
async fn launch_component_preview(
    app_handle: &AppHandle,
    prepared: &PreparedArtifactCapture,
) -> Result<ComponentPreviewSession, String> {
    let preview_id =
        format!("artifact-capture-{}", Utc::now().timestamp_nanos_opt().unwrap_or_default());
    let shared_utils = SharedPreviewUtils::new(app_handle.clone());
    let port = shared_utils
        .find_available_port(42000, 52000)
        .map_err(|e| format!("Failed to find available preview port: {}", e))?;

    match detect_component_preview_kind(prepared)? {
        ComponentPreviewKind::React { component_name } => {
            let manager = ReactPreviewManager::new(app_handle.clone());
            let (template_path, need_install_deps) = manager
                .setup_capture_preview_project(&preview_id, &prepared.code, &component_name)
                .map_err(|e| e.to_string())?;
            manager
                .start_capture_dev_server(&template_path, port, need_install_deps)
                .map_err(|e| e.to_string())?;
            wait_for_local_url_ready(port, Duration::from_secs(20)).await?;
            return Ok(ComponentPreviewSession {
                url: format!("http://127.0.0.1:{port}"),
                preview_id,
                kind: ComponentPreviewRuntime::React,
            });
        }
        ComponentPreviewKind::Vue { component_name } => {
            let manager = VuePreviewManager::new(app_handle.clone());
            let (template_path, need_install_deps) = manager
                .setup_capture_preview_project(&preview_id, &prepared.code, &component_name)
                .map_err(|e| e.to_string())?;
            manager
                .start_capture_dev_server(&template_path, port, need_install_deps)
                .map_err(|e| e.to_string())?;
            wait_for_local_url_ready(port, Duration::from_secs(20)).await?;
            return Ok(ComponentPreviewSession {
                url: format!("http://127.0.0.1:{port}"),
                preview_id,
                kind: ComponentPreviewRuntime::Vue,
            });
        }
    }
}

#[cfg(desktop)]
enum ComponentPreviewKind {
    React { component_name: String },
    Vue { component_name: String },
}

#[cfg(desktop)]
enum ComponentPreviewRuntime {
    React,
    Vue,
}

#[cfg(desktop)]
struct ComponentPreviewSession {
    url: String,
    preview_id: String,
    kind: ComponentPreviewRuntime,
}

#[cfg(desktop)]
impl ComponentPreviewSession {
    fn cleanup(&self, app_handle: &AppHandle) {
        match self.kind {
            ComponentPreviewRuntime::React => {
                let manager = ReactPreviewManager::new(app_handle.clone());
                let _ = manager.close_preview(&self.preview_id);
            }
            ComponentPreviewRuntime::Vue => {
                let manager = VuePreviewManager::new(app_handle.clone());
                let _ = manager.close_preview(&self.preview_id);
            }
        }
    }
}

#[cfg(desktop)]
fn detect_component_preview_kind(
    prepared: &PreparedArtifactCapture,
) -> Result<ComponentPreviewKind, String> {
    match prepared.preview_type.as_str() {
        "vue" => Ok(ComponentPreviewKind::Vue {
            component_name: extract_vue_component_name(&prepared.code)
                .unwrap_or_else(|| "UserComponent".to_string()),
        }),
        "react" | "jsx" => Ok(ComponentPreviewKind::React {
            component_name: extract_component_name(&prepared.code)
                .unwrap_or_else(|| "UserComponent".to_string()),
        }),
        "tsx" | "ts" | "js" => {
            if is_vue_component(&prepared.code) {
                Ok(ComponentPreviewKind::Vue {
                    component_name: extract_vue_component_name(&prepared.code)
                        .unwrap_or_else(|| "UserComponent".to_string()),
                })
            } else if is_react_component(&prepared.code) {
                Ok(ComponentPreviewKind::React {
                    component_name: extract_component_name(&prepared.code)
                        .unwrap_or_else(|| "UserComponent".to_string()),
                })
            } else {
                Err("Unable to detect React or Vue component from artifact code".to_string())
            }
        }
        other => Err(format!("Unsupported component preview type '{}'", other)),
    }
}

#[cfg(desktop)]
async fn wait_for_local_url_ready(port: u16, timeout_duration: Duration) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < timeout_duration {
        if SharedPreviewUtils::is_port_open("127.0.0.1", port) {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
    }
    Err(format!("Timed out waiting for local preview server on port {}", port))
}

#[cfg(desktop)]
async fn capture_html_screenshot(
    app_handle: &AppHandle,
    html: &str,
    selector: Option<&str>,
    width: u32,
    height: u32,
    delay_ms: u64,
) -> Result<Vec<u8>, String> {
    let mut browser = launch_capture_browser(app_handle).await?;
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| format!("Failed to create capture page: {}", e))?;

    let result = async {
        configure_capture_viewport(&page, width, height).await?;
        page.set_content(html.to_string())
            .await
            .map_err(|e| format!("Failed to set page content: {}", e))?;
        wait_for_render_stability(&page, delay_ms).await?;
        click_selector_if_requested(&page, selector, delay_ms).await?;
        take_png_screenshot(&page).await
    }
    .await;

    let _ = page.close().await;
    let _ = browser.close().await;
    let _ = browser.wait().await;

    result
}

#[cfg(desktop)]
async fn capture_url_screenshot(
    app_handle: &AppHandle,
    url: &str,
    selector: Option<&str>,
    width: u32,
    height: u32,
    delay_ms: u64,
) -> Result<Vec<u8>, String> {
    let mut browser = launch_capture_browser(app_handle).await?;
    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| format!("Failed to create capture page: {}", e))?;

    let result = async {
        configure_capture_viewport(&page, width, height).await?;
        page.goto(url).await.map_err(|e| format!("Failed to open preview URL '{}': {}", url, e))?;
        wait_for_render_stability(&page, delay_ms).await?;
        click_selector_if_requested(&page, selector, delay_ms).await?;
        take_png_screenshot(&page).await
    }
    .await;

    let _ = page.close().await;
    let _ = browser.close().await;
    let _ = browser.wait().await;

    result
}

#[cfg(desktop)]
async fn launch_capture_browser(app_handle: &AppHandle) -> Result<Browser, String> {
    let browser_manager = BrowserManager::new(None);
    let browser_path = browser_manager
        .get_browser_path()
        .map_err(|e| format!("Unable to locate Chromium browser: {}", e))?;

    let mut config_builder =
        BrowserConfig::builder().no_sandbox().launch_timeout(Duration::from_secs(45));
    if browser_path.exists() {
        config_builder = config_builder.chrome_executable(&browser_path);
    }

    let config =
        config_builder.build().map_err(|e| format!("Failed to build Chromium config: {}", e))?;

    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("Failed to launch Chromium for screenshot capture: {}", e))?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });

    let _ = app_handle;
    Ok(browser)
}

#[cfg(desktop)]
async fn configure_capture_viewport(
    page: &chromiumoxide::Page,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let metrics = SetDeviceMetricsOverrideParams::builder()
        .width(width as i64)
        .height(height as i64)
        .screen_width(width as i64)
        .screen_height(height as i64)
        .device_scale_factor(1.0)
        .mobile(false)
        .build()
        .map_err(|e| format!("Failed to build screenshot viewport config: {}", e))?;
    page.execute(metrics)
        .await
        .map_err(|e| format!("Failed to apply screenshot viewport: {}", e))?;
    Ok(())
}

#[cfg(desktop)]
async fn wait_for_render_stability(
    page: &chromiumoxide::Page,
    delay_ms: u64,
) -> Result<(), String> {
    page.evaluate_function(
        r#"
        async function() {
            const images = Array.from(document.images || []);
            if (images.length === 0) return true;
            await Promise.race([
                Promise.all(images.map((img) => {
                    if (img.complete) return Promise.resolve(true);
                    return new Promise((resolve) => {
                        const done = () => resolve(true);
                        img.addEventListener("load", done, { once: true });
                        img.addEventListener("error", done, { once: true });
                    });
                })),
                new Promise((resolve) => setTimeout(resolve, 8000))
            ]);
            return true;
        }
        "#,
    )
    .await
    .map_err(|e| format!("Failed to wait for rendered assets: {}", e))?;

    if delay_ms > 0 {
        sleep(Duration::from_millis(delay_ms)).await;
    }
    Ok(())
}

#[cfg(desktop)]
async fn click_selector_if_requested(
    page: &chromiumoxide::Page,
    selector: Option<&str>,
    delay_ms: u64,
) -> Result<(), String> {
    let Some(selector) = selector.map(str::trim).filter(|selector| !selector.is_empty()) else {
        return Ok(());
    };

    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    loop {
        match page.find_element(selector).await {
            Ok(element) => {
                element
                    .click()
                    .await
                    .map_err(|e| format!("Failed to click selector '{}': {}", selector, e))?;
                if delay_ms > 0 {
                    sleep(Duration::from_millis(delay_ms)).await;
                }
                return Ok(());
            }
            Err(_) if start.elapsed() < timeout => {
                sleep(Duration::from_millis(200)).await;
            }
            Err(e) => {
                return Err(format!(
                    "Timed out waiting for selector '{}' before screenshot: {}",
                    selector, e
                ));
            }
        }
    }
}

#[cfg(desktop)]
async fn take_png_screenshot(page: &chromiumoxide::Page) -> Result<Vec<u8>, String> {
    page.screenshot(
        ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .full_page(false)
            .omit_background(false)
            .build(),
    )
    .await
    .map_err(|e| format!("Failed to capture screenshot: {}", e))
}

#[cfg(desktop)]
fn build_html_like_capture_document(content: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <style>
    html, body {{
      margin: 0;
      padding: 0;
      width: 100%;
      min-height: 100%;
      background: white;
    }}
    body {{
      overflow: auto;
    }}
  </style>
</head>
<body>
{content}
</body>
</html>"#
    )
}

#[cfg(desktop)]
fn normalize_screenshot_output_mode(output_mode: Option<&str>) -> Result<&'static str, String> {
    match output_mode.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("base64") => Ok("base64"),
        Some("path") => Ok("path"),
        Some(other) => {
            Err(format!("Unsupported output_mode '{}'. Expected 'base64' or 'path'.", other))
        }
    }
}

#[cfg(desktop)]
fn write_artifact_screenshot_temp_file(
    app_handle: &AppHandle,
    artifact_key: &str,
    entry_file: &str,
    screenshot_bytes: &[u8],
) -> Result<String, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
    let temp_dir = app_data_dir.join("temp").join("artifact-screenshots");
    fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create screenshot temp directory: {}", e))?;

    write_artifact_screenshot_temp_file_in_dir(
        &temp_dir,
        artifact_key,
        entry_file,
        screenshot_bytes,
    )
    .map(|path| normalize_path_string(&path))
}

#[cfg(desktop)]
fn write_artifact_screenshot_temp_file_in_dir(
    temp_dir: &Path,
    artifact_key: &str,
    entry_file: &str,
    screenshot_bytes: &[u8],
) -> Result<PathBuf, String> {
    let artifact_slug = screenshot_filename_slug(artifact_key);
    let entry_slug = screenshot_filename_slug(entry_file);
    let timestamp = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let file_name =
        format!("artifact-screenshot-{}-{}-{}.png", artifact_slug, entry_slug, timestamp);
    let path = temp_dir.join(file_name);
    fs::write(&path, screenshot_bytes)
        .map_err(|e| format!("Failed to write artifact screenshot '{}': {}", path.display(), e))?;
    Ok(path)
}

#[cfg(desktop)]
fn screenshot_filename_slug(value: &str) -> String {
    let slug: String = value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "artifact".to_string()
    } else {
        slug.to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(desktop)]
    use chromiumoxide::browser::BrowserConfig;
    #[cfg(desktop)]
    use tempfile::TempDir;
    #[cfg(desktop)]
    use tokio::runtime::Runtime;

    #[test]
    fn build_html_like_capture_document_wraps_content() {
        let html = build_html_like_capture_document("<div id='demo'>hi</div>");
        assert!(html.contains("<!doctype html>"));
        assert!(html.contains("<div id='demo'>hi</div>"));
    }

    #[test]
    fn detect_component_preview_kind_rejects_non_component_js() {
        let prepared = PreparedArtifactCapture {
            artifact_key: "demo".to_string(),
            entry_file: "index.js".to_string(),
            language: "js".to_string(),
            preview_type: "js".to_string(),
            code: "console.log('hello')".to_string(),
        };

        let error = match detect_component_preview_kind(&prepared) {
            Ok(_) => panic!("should reject plain js"),
            Err(error) => error,
        };
        assert!(error.contains("Unable to detect React or Vue component"));
    }

    #[test]
    fn detect_component_preview_kind_detects_react_component() {
        let prepared = PreparedArtifactCapture {
            artifact_key: "demo".to_string(),
            entry_file: "App.tsx".to_string(),
            language: "tsx".to_string(),
            preview_type: "tsx".to_string(),
            code: "export default function App() { return <div>Hello</div>; }".to_string(),
        };

        let kind = detect_component_preview_kind(&prepared).expect("should detect react");
        assert!(matches!(kind, ComponentPreviewKind::React { .. }));
    }

    #[cfg(desktop)]
    #[test]
    fn capture_html_screenshot_returns_png_bytes() {
        let runtime = Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let browser_manager = BrowserManager::new(None);
            let browser_path = browser_manager
                .get_browser_path()
                .expect("chromium browser should be available for screenshot capture");
            let temp_dir = TempDir::new().expect("temp dir");
            let config = BrowserConfig::builder()
                .no_sandbox()
                .chrome_executable(&browser_path)
                .user_data_dir(temp_dir.path())
                .build()
                .expect("browser config");
            let (mut browser, mut handler) =
                Browser::launch(config).await.expect("launch headless chromium");
            tokio::spawn(async move { while handler.next().await.is_some() {} });

            let page = browser.new_page("about:blank").await.expect("new page");
            configure_capture_viewport(&page, 320, 240).await.expect("configure viewport");
            page.set_content(build_html_like_capture_document(
                "<button id='toggle' onclick=\"document.body.dataset.clicked='yes'\">ok</button>",
            ))
            .await
            .expect("set content");
            click_selector_if_requested(&page, Some("#toggle"), 50).await.expect("click selector");
            let bytes = take_png_screenshot(&page).await.expect("take screenshot");
            assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));

            let _ = page.close().await;
            let _ = browser.close().await;
            let _ = browser.wait().await;
        });
    }

    #[cfg(desktop)]
    #[test]
    fn write_artifact_screenshot_temp_file_uses_png_extension() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = write_artifact_screenshot_temp_file_in_dir(
            temp_dir.path(),
            "demo/card",
            "src/App.tsx",
            b"png-bytes",
        )
        .expect("should write screenshot");
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("png"));
        assert!(path.file_name().and_then(|name| name.to_str()).unwrap().contains("demo-card"));
        assert!(path.file_name().and_then(|name| name.to_str()).unwrap().contains("src-app-tsx"));
        assert_eq!(fs::read(path).expect("read written file"), b"png-bytes");
    }

    #[cfg(desktop)]
    #[test]
    fn normalize_screenshot_output_mode_defaults_to_base64() {
        assert_eq!(normalize_screenshot_output_mode(None).unwrap(), "base64");
        assert_eq!(normalize_screenshot_output_mode(Some("base64")).unwrap(), "base64");
        assert_eq!(normalize_screenshot_output_mode(Some("path")).unwrap(), "path");
        assert!(normalize_screenshot_output_mode(Some("json")).is_err());
    }
}

use super::interaction::{
    resolve_local_file_path, PreviewCodeRequest, PreviewCodeRequestEvent, PreviewFileRelayState,
    PreviewFileRequestEvent, PREVIEW_FILE_RELAY_SCHEME,
};
use crate::api::ai::config::get_network_proxy_from_config;
use crate::db::mcp_db::MCPDatabase;
use crate::db::system_db::{FeatureConfig, SystemDatabase};
use regex::Regex;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

const FEATURE_CODE: &str = "preview_external_resources";
const DEFAULT_MAX_RESOURCE_BYTES: u64 = 20 * 1024 * 1024;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
const DEFAULT_CACHE_TTL_SECS: u64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewResourceType {
    Image,
    Css,
    Script,
    Font,
    Pdf,
    Html,
    Text,
    Markdown,
    Media,
    Unknown,
}

impl PreviewResourceType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Css => "css",
            Self::Script => "script",
            Self::Font => "font",
            Self::Pdf => "pdf",
            Self::Html => "html",
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::Media => "media",
            Self::Unknown => "unknown",
        }
    }

    fn risk(self) -> PreviewResourceRisk {
        match self {
            Self::Script | Self::Html | Self::Unknown => PreviewResourceRisk::High,
            Self::Css | Self::Font | Self::Media | Self::Pdf => PreviewResourceRisk::Medium,
            Self::Image | Self::Text | Self::Markdown => PreviewResourceRisk::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewResourceStatus {
    Allowed,
    Pending,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewResourceAllowedBy {
    Whitelist,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewResourceRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewExternalResourceInfo {
    pub id: String,
    #[serde(rename = "originalUrl")]
    pub original_url: String,
    #[serde(rename = "normalizedUrl")]
    pub normalized_url: String,
    #[serde(rename = "type")]
    pub resource_type: PreviewResourceType,
    pub source: String,
    pub occurrence: String,
    pub status: PreviewResourceStatus,
    #[serde(rename = "allowedBy", skip_serializing_if = "Option::is_none")]
    pub allowed_by: Option<PreviewResourceAllowedBy>,
    pub risk: PreviewResourceRisk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewExternalResourcesPayload {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub resources: Vec<PreviewExternalResourceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResourceDomainRule {
    pub domain: String,
    #[serde(default)]
    pub include_subdomains: bool,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub auto_load: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResourceLimits {
    pub max_resource_bytes: u64,
    pub request_timeout_secs: u64,
    pub cache_ttl_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewExternalResourcePolicy {
    #[serde(default)]
    pub allowed_domains: Vec<PreviewResourceDomainRule>,
    pub limits: PreviewResourceLimits,
}

impl Default for PreviewExternalResourcePolicy {
    fn default() -> Self {
        Self {
            allowed_domains: vec![PreviewResourceDomainRule {
                domain: "cdn.jsdelivr.net".to_string(),
                include_subdomains: false,
                types: vec!["script".to_string(), "css".to_string()],
                auto_load: true,
            }],
            limits: PreviewResourceLimits {
                max_resource_bytes: DEFAULT_MAX_RESOURCE_BYTES,
                request_timeout_secs: DEFAULT_REQUEST_TIMEOUT_SECS,
                cache_ttl_secs: DEFAULT_CACHE_TTL_SECS,
            },
        }
    }
}

#[derive(Debug, Clone)]
enum StoredPreviewRequest {
    Code(PreviewCodeRequestEvent),
    File(PreviewFileRequestEvent),
}

#[derive(Clone)]
pub struct PreviewResourceState {
    requests: std::sync::Arc<std::sync::Mutex<HashMap<String, StoredPreviewRequest>>>,
    user_allowed: std::sync::Arc<std::sync::Mutex<HashMap<String, HashSet<String>>>>,
    resource_meta: std::sync::Arc<std::sync::Mutex<HashMap<String, PreviewExternalResourceInfo>>>,
}

impl PreviewResourceState {
    pub fn new() -> Self {
        Self {
            requests: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            user_allowed: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            resource_meta: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    fn store_code(&self, event: PreviewCodeRequestEvent) -> Result<(), String> {
        self.requests
            .lock()
            .map_err(|_| "Preview resource request state poisoned".to_string())?
            .insert(event.request_id.clone(), StoredPreviewRequest::Code(event));
        Ok(())
    }

    fn store_file(&self, event: PreviewFileRequestEvent) -> Result<(), String> {
        self.requests
            .lock()
            .map_err(|_| "Preview resource request state poisoned".to_string())?
            .insert(event.request_id.clone(), StoredPreviewRequest::File(event));
        Ok(())
    }

    fn remember_resources(&self, resources: &[PreviewExternalResourceInfo]) -> Result<(), String> {
        let mut meta = self
            .resource_meta
            .lock()
            .map_err(|_| "Preview resource metadata state poisoned".to_string())?;
        for resource in resources {
            meta.insert(resource.id.clone(), resource.clone());
        }
        Ok(())
    }

    fn allowed_for_request(&self, request_id: &str) -> Result<HashSet<String>, String> {
        Ok(self
            .user_allowed
            .lock()
            .map_err(|_| "Preview resource authorization state poisoned".to_string())?
            .get(request_id)
            .cloned()
            .unwrap_or_default())
    }

    fn authorize(&self, request_id: &str, resource_ids: &[String]) -> Result<(), String> {
        let mut allowed = self
            .user_allowed
            .lock()
            .map_err(|_| "Preview resource authorization state poisoned".to_string())?;
        let entry = allowed.entry(request_id.to_string()).or_default();
        entry.extend(resource_ids.iter().cloned());
        Ok(())
    }

    fn get_request(&self, request_id: &str) -> Result<Option<StoredPreviewRequest>, String> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| "Preview resource request state poisoned".to_string())?
            .get(request_id)
            .cloned())
    }

    fn get_resource_meta(
        &self,
        resource_ids: &[String],
    ) -> Result<Vec<PreviewExternalResourceInfo>, String> {
        let meta = self
            .resource_meta
            .lock()
            .map_err(|_| "Preview resource metadata state poisoned".to_string())?;
        Ok(resource_ids.iter().filter_map(|id| meta.get(id).cloned()).collect())
    }
}

impl Default for PreviewResourceState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewResourceAuthorizationResult {
    #[serde(rename = "previewCode", skip_serializing_if = "Option::is_none")]
    pub preview_code: Option<PreviewCodeRequestEvent>,
    #[serde(rename = "previewFile", skip_serializing_if = "Option::is_none")]
    pub preview_file: Option<PreviewFileRequestEvent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSelectedExternalResource {
    pub original_url: String,
    #[serde(default)]
    pub normalized_url: Option<String>,
    #[serde(rename = "type", default = "default_preview_resource_type")]
    pub resource_type: PreviewResourceType,
    #[serde(default)]
    pub occurrence: Option<String>,
}

fn default_preview_resource_type() -> PreviewResourceType {
    PreviewResourceType::Unknown
}

struct RewriteContext<'a> {
    app_handle: &'a AppHandle,
    relay_state: &'a PreviewFileRelayState,
    policy: &'a PreviewExternalResourcePolicy,
    source: &'a str,
    user_allowed: &'a HashSet<String>,
    user_allowed_urls: &'a HashSet<String>,
    force_proxy: bool,
    resources: &'a mut Vec<PreviewExternalResourceInfo>,
}

pub fn resource_id(kind: PreviewResourceType, original_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_str().as_bytes());
    hasher.update(b"\n");
    hasher.update(original_url.as_bytes());
    hex::encode(hasher.finalize())
}

fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn html_resource_attr_regex() -> Regex {
    Regex::new(r#"(?is)\b(srcset|src|href|data|xlink:href)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'<>`]+))"#)
        .expect("valid preview HTML resource attribute regex")
}

fn html_style_attr_regex() -> Regex {
    Regex::new(r#"(?is)\bstyle\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
        .expect("valid preview HTML style attribute regex")
}

fn css_url_regex() -> Regex {
    Regex::new(r#"(?is)url\(\s*(?:"([^"]*)"|'([^']*)'|([^'")\s][^)]*?))\s*\)"#)
        .expect("valid preview CSS url regex")
}

fn css_import_regex() -> Regex {
    Regex::new(r#"(?is)@import\s+(?:url\(\s*)?(?:"([^"]*)"|'([^']*)'|([^\s"'()]+))\s*\)?"#)
        .expect("valid preview CSS import regex")
}

fn normalize_url(raw_url: &str, base_url: Option<&str>) -> Option<String> {
    let trimmed = raw_url.trim().trim_matches(|c| matches!(c, '"' | '\''));
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("data:")
        || trimmed.starts_with("blob:")
        || trimmed.starts_with("javascript:")
        || trimmed.starts_with(&format!("{}://", PREVIEW_FILE_RELAY_SCHEME))
    {
        return None;
    }
    if let Some(base) = base_url {
        if let Ok(base) = reqwest::Url::parse(base) {
            if let Ok(joined) = base.join(trimmed) {
                return Some(joined.to_string());
            }
        }
    }
    if let Ok(parsed) = reqwest::Url::parse(trimmed) {
        return Some(parsed.to_string());
    }
    if let Some(local_path) = resolve_local_file_path(trimmed) {
        return Some(local_path.to_string_lossy().to_string());
    }
    Some(trimmed.to_string())
}

fn is_remote_url(normalized_url: &str) -> bool {
    reqwest::Url::parse(normalized_url)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https"))
}

fn is_https_url(normalized_url: &str) -> bool {
    reqwest::Url::parse(normalized_url)
        .ok()
        .is_some_and(|url| url.scheme() == "https")
}

fn host_for_url(normalized_url: &str) -> Option<String> {
    reqwest::Url::parse(normalized_url)
        .ok()
        .and_then(|url| url.host_str().map(normalize_domain))
}

pub fn whitelist_allows(
    policy: &PreviewExternalResourcePolicy,
    normalized_url: &str,
    resource_type: PreviewResourceType,
) -> bool {
    if !is_https_url(normalized_url) {
        return false;
    }
    let Some(host) = host_for_url(normalized_url) else {
        return false;
    };
    let resource_type = resource_type.as_str();
    policy.allowed_domains.iter().any(|rule| {
        if !rule.auto_load {
            return false;
        }
        if !rule.types.iter().any(|ty| ty.eq_ignore_ascii_case(resource_type)) {
            return false;
        }
        let domain = normalize_domain(&rule.domain);
        if host == domain {
            return true;
        }
        rule.include_subdomains
            && host.len() > domain.len() + 1
            && host.ends_with(&format!(".{domain}"))
    })
}

fn placeholder_for(kind: PreviewResourceType) -> &'static str {
    match kind {
        PreviewResourceType::Image => "data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22/%3E",
        PreviewResourceType::Css => "data:text/css,",
        PreviewResourceType::Script => "data:text/javascript,",
        PreviewResourceType::Font => "data:font/woff2;base64,",
        PreviewResourceType::Pdf => "about:blank",
        PreviewResourceType::Html => "about:blank",
        PreviewResourceType::Text | PreviewResourceType::Markdown | PreviewResourceType::Media | PreviewResourceType::Unknown => "",
    }
}

fn detect_type_from_url(url: &str, fallback: PreviewResourceType) -> PreviewResourceType {
    let path = reqwest::Url::parse(url)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_else(|| url.to_string());
    let guessed = mime_guess::from_path(path).first_or_octet_stream();
    match guessed.type_().as_str() {
        "image" => PreviewResourceType::Image,
        "font" => PreviewResourceType::Font,
        "audio" | "video" => PreviewResourceType::Media,
        "text" if guessed.subtype().as_str() == "css" => PreviewResourceType::Css,
        "text" if guessed.subtype().as_str() == "html" => PreviewResourceType::Html,
        "text" => fallback,
        "application" if guessed.subtype().as_str() == "pdf" => PreviewResourceType::Pdf,
        "application" if guessed.subtype().as_str().contains("javascript") => {
            PreviewResourceType::Script
        }
        _ => fallback,
    }
}

fn push_resource(
    resources: &mut Vec<PreviewExternalResourceInfo>,
    original_url: &str,
    normalized_url: &str,
    resource_type: PreviewResourceType,
    source: &str,
    occurrence: &str,
    status: PreviewResourceStatus,
    allowed_by: Option<PreviewResourceAllowedBy>,
    reason: Option<String>,
) -> String {
    let id = resource_id(resource_type, normalized_url);
    if let Some(existing) = resources.iter_mut().find(|item| item.id == id) {
        if existing.status != PreviewResourceStatus::Failed {
            existing.status = status;
            existing.allowed_by = allowed_by;
            existing.reason = reason.clone();
        }
        if !existing.occurrence.contains(occurrence) {
            existing.occurrence = format!("{}, {}", existing.occurrence, occurrence);
        }
        return id;
    }
    resources.push(PreviewExternalResourceInfo {
        id: id.clone(),
        original_url: original_url.to_string(),
        normalized_url: normalized_url.to_string(),
        resource_type,
        source: source.to_string(),
        occurrence: occurrence.to_string(),
        status,
        allowed_by,
        risk: resource_type.risk(),
        reason,
    });
    id
}

fn validate_content_type(kind: PreviewResourceType, content_type: Option<&str>) -> Result<(), String> {
    let Some(content_type) = content_type.map(|value| value.to_ascii_lowercase()) else {
        return Ok(());
    };
    let essence = content_type.split(';').next().unwrap_or("").trim();
    let ok = match kind {
        PreviewResourceType::Image => essence.starts_with("image/"),
        PreviewResourceType::Css => essence == "text/css",
        PreviewResourceType::Script => {
            essence.contains("javascript") || matches!(essence, "text/ecmascript" | "application/ecmascript")
        }
        PreviewResourceType::Font => essence.starts_with("font/") || essence == "application/font-woff" || essence == "application/octet-stream",
        PreviewResourceType::Pdf => essence == "application/pdf",
        PreviewResourceType::Html => essence == "text/html",
        PreviewResourceType::Text => essence.starts_with("text/") || essence == "application/json",
        PreviewResourceType::Markdown => essence == "text/markdown" || essence == "text/plain",
        PreviewResourceType::Media => essence.starts_with("audio/") || essence.starts_with("video/"),
        PreviewResourceType::Unknown => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("Content-Type '{}' does not match {:?}", essence, kind))
    }
}

fn ext_for_content(kind: PreviewResourceType, content_type: Option<&str>, normalized_url: &str) -> String {
    if let Some(content_type) = content_type {
        if let Some(ext) = mime_guess::get_mime_extensions_str(
            content_type.split(';').next().unwrap_or(content_type).trim(),
        )
        .and_then(|items| items.first().copied())
        {
            return ext.to_string();
        }
    }
    let path = reqwest::Url::parse(normalized_url)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_else(|| normalized_url.to_string());
    Path::new(&path)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && value.len() <= 12)
        .map(str::to_string)
        .unwrap_or_else(|| match kind {
            PreviewResourceType::Css => "css",
            PreviewResourceType::Script => "js",
            PreviewResourceType::Html => "html",
            PreviewResourceType::Markdown => "md",
            PreviewResourceType::Pdf => "pdf",
            PreviewResourceType::Text => "txt",
            PreviewResourceType::Font => "bin",
            PreviewResourceType::Image => "img",
            PreviewResourceType::Media => "bin",
            PreviewResourceType::Unknown => "bin",
        }.to_string())
}

fn cache_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let dir = app_data_dir.join("preview_resources").join("cache");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create preview resource cache: {e}"))?;
    Ok(dir)
}

fn preview_network_proxy_from_config_map(
    config_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Option<String> {
    get_network_proxy_from_config(config_map)
}

async fn build_http_client(
    timeout_secs: u64,
    proxy_url: Option<&str>,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.max(1)))
        .user_agent("AIPP preview resource loader");
    if let Some(proxy_url) = proxy_url.filter(|value| !value.trim().is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| format!("Invalid network proxy for preview resource: {e}"))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| format!("Failed to build preview resource client: {e}"))
}

async fn get_preview_network_proxy(app_handle: &AppHandle) -> Option<String> {
    if let Some(feature_state) = app_handle.try_state::<crate::FeatureConfigState>() {
        let config_map = feature_state.config_feature_map.lock().await;
        preview_network_proxy_from_config_map(&config_map)
    } else {
        None
    }
}

struct RemoteResourceBytes {
    bytes: Vec<u8>,
    content_type: Option<String>,
}

struct RemoteResourceError {
    message: String,
    timed_out: bool,
}

async fn fetch_remote_resource_bytes(
    client: &reqwest::Client,
    normalized_url: &str,
    kind: PreviewResourceType,
    max_bytes: u64,
) -> Result<RemoteResourceBytes, RemoteResourceError> {
    let response = client.get(normalized_url).send().await.map_err(|e| RemoteResourceError {
        timed_out: e.is_timeout(),
        message: format!("Failed to download preview resource: {e}"),
    })?;
    if !response.status().is_success() {
        return Err(RemoteResourceError {
            timed_out: false,
            message: format!("Preview resource returned HTTP {}", response.status()),
        });
    }
    if let Some(len) = response.content_length() {
        if len > max_bytes {
            return Err(RemoteResourceError {
                timed_out: false,
                message: format!("Preview resource is too large ({len} bytes)"),
            });
        }
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    validate_content_type(kind, content_type.as_deref()).map_err(|message| RemoteResourceError {
        timed_out: false,
        message,
    })?;
    let bytes = response.bytes().await.map_err(|e| RemoteResourceError {
        timed_out: e.is_timeout(),
        message: format!("Failed to read preview resource response: {e}"),
    })?;
    if bytes.len() as u64 > max_bytes {
        return Err(RemoteResourceError {
            timed_out: false,
            message: format!("Preview resource is too large ({} bytes)", bytes.len()),
        });
    }
    Ok(RemoteResourceBytes { bytes: bytes.to_vec(), content_type })
}

async fn download_or_read_resource(
    ctx: &RewriteContext<'_>,
    normalized_url: &str,
    kind: PreviewResourceType,
) -> Result<(PathBuf, String), String> {
    if is_remote_url(normalized_url) {
        let timeout_secs = ctx.policy.limits.request_timeout_secs;
        let max_bytes = ctx.policy.limits.max_resource_bytes;
        if ctx.force_proxy {
            let proxy_url = get_preview_network_proxy(ctx.app_handle)
                .await
                .ok_or_else(|| "network_proxy is not configured".to_string())?;
            let proxy_client = build_http_client(timeout_secs, Some(&proxy_url)).await?;
            let fetched = fetch_remote_resource_bytes(&proxy_client, normalized_url, kind, max_bytes)
                .await
                .map_err(|proxy_error| proxy_error.message)?;
            let content_type = fetched.content_type;
            let ext = ext_for_content(kind, content_type.as_deref(), normalized_url);
            let path = cache_dir(ctx.app_handle)?.join(format!("{}.{}", resource_id(kind, normalized_url), ext));
            fs::write(&path, &fetched.bytes).map_err(|e| format!("Failed to cache preview resource: {e}"))?;
            return Ok((path, content_type.unwrap_or_else(|| mime_guess::from_path(normalized_url).first_or_octet_stream().essence_str().to_string())));
        }
        let direct_client = build_http_client(timeout_secs, None).await?;
        let fetched = match fetch_remote_resource_bytes(&direct_client, normalized_url, kind, max_bytes).await {
            Ok(fetched) => fetched,
            Err(direct_error) => {
                let proxy_url = get_preview_network_proxy(ctx.app_handle).await;
                if direct_error.timed_out {
                    if let Some(proxy_url) = proxy_url.as_deref() {
                        let proxy_client = build_http_client(timeout_secs, Some(proxy_url)).await?;
                        fetch_remote_resource_bytes(&proxy_client, normalized_url, kind, max_bytes)
                            .await
                            .map_err(|proxy_error| {
                                format!(
                                    "{}; retry with configured proxy failed: {}",
                                    direct_error.message, proxy_error.message
                                )
                            })?
                    } else {
                        return Err(format!(
                            "{}; no network_proxy configured for retry",
                            direct_error.message
                        ));
                    }
                } else {
                    return Err(direct_error.message);
                }
            }
        };
        let content_type = fetched.content_type;
        let ext = ext_for_content(kind, content_type.as_deref(), normalized_url);
        let path = cache_dir(ctx.app_handle)?.join(format!("{}.{}", resource_id(kind, normalized_url), ext));
        fs::write(&path, &fetched.bytes).map_err(|e| format!("Failed to cache preview resource: {e}"))?;
        return Ok((path, content_type.unwrap_or_else(|| mime_guess::from_path(normalized_url).first_or_octet_stream().essence_str().to_string())));
    }

    let local_path = resolve_local_file_path(normalized_url)
        .or_else(|| Some(PathBuf::from(normalized_url)))
        .ok_or_else(|| "Unsupported local preview resource URL".to_string())?;
    if !local_path.exists() {
        return Err(format!("Local preview resource not found: {}", local_path.display()));
    }
    if local_path.is_dir() {
        return Err(format!("Preview resource path is a directory: {}", local_path.display()));
    }
    let metadata = fs::metadata(&local_path)
        .map_err(|e| format!("Failed to read metadata for '{}': {e}", local_path.display()))?;
    if metadata.len() > ctx.policy.limits.max_resource_bytes {
        return Err(format!("Local preview resource is too large ({} bytes)", metadata.len()));
    }
    let content_type = mime_guess::from_path(&local_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();
    validate_content_type(kind, Some(&content_type))?;
    Ok((local_path, content_type))
}

async fn allowed_resource_url(
    ctx: &mut RewriteContext<'_>,
    original_url: &str,
    normalized_url: &str,
    kind: PreviewResourceType,
    occurrence: &str,
    allowed_by: PreviewResourceAllowedBy,
) -> String {
    match download_or_read_resource(ctx, normalized_url, kind).await {
        Ok((mut path, mut content_type)) => {
            if kind == PreviewResourceType::Css {
                match fs::read_to_string(&path) {
                    Ok(css) => {
                        let processed = Box::pin(rewrite_css_resources(
                            ctx,
                            &css,
                            Some(normalized_url),
                            "css secondary",
                        ))
                        .await;
                        match processed {
                            Ok(processed_css) => {
                                let ext = ext_for_content(kind, Some("text/css"), normalized_url);
                                path = match cache_dir(ctx.app_handle) {
                                    Ok(dir) => dir.join(format!("{}-processed.{}", resource_id(kind, normalized_url), ext)),
                                    Err(error) => {
                                        push_resource(
                                            ctx.resources,
                                            original_url,
                                            normalized_url,
                                            kind,
                                            ctx.source,
                                            occurrence,
                                            PreviewResourceStatus::Failed,
                                            None,
                                            Some(error),
                                        );
                                        return placeholder_for(kind).to_string();
                                    }
                                };
                                if let Err(error) = fs::write(&path, processed_css) {
                                    push_resource(
                                        ctx.resources,
                                        original_url,
                                        normalized_url,
                                        kind,
                                        ctx.source,
                                        occurrence,
                                        PreviewResourceStatus::Failed,
                                        None,
                                        Some(format!("Failed to cache processed CSS: {error}")),
                                    );
                                    return placeholder_for(kind).to_string();
                                }
                                content_type = "text/css; charset=utf-8".to_string();
                            }
                            Err(error) => {
                                push_resource(
                                    ctx.resources,
                                    original_url,
                                    normalized_url,
                                    kind,
                                    ctx.source,
                                    occurrence,
                                    PreviewResourceStatus::Failed,
                                    None,
                                    Some(error),
                                );
                                return placeholder_for(kind).to_string();
                            }
                        }
                    }
                    Err(error) => {
                        push_resource(
                            ctx.resources,
                            original_url,
                            normalized_url,
                            kind,
                            ctx.source,
                            occurrence,
                            PreviewResourceStatus::Failed,
                            None,
                            Some(format!("Failed to read CSS resource: {error}")),
                        );
                        return placeholder_for(kind).to_string();
                    }
                }
            }
            let token = resource_id(kind, normalized_url);
            if let Err(error) =
                ctx.relay_state.register_preview_resource(token.clone(), path, content_type, None)
            {
                push_resource(
                    ctx.resources,
                    original_url,
                    normalized_url,
                    kind,
                    ctx.source,
                    occurrence,
                    PreviewResourceStatus::Failed,
                    None,
                    Some(error),
                );
                return placeholder_for(kind).to_string();
            }
            push_resource(
                ctx.resources,
                original_url,
                normalized_url,
                kind,
                ctx.source,
                occurrence,
                PreviewResourceStatus::Allowed,
                Some(allowed_by),
                None,
            );
            format!("{}://localhost/{}", PREVIEW_FILE_RELAY_SCHEME, token)
        }
        Err(error) => {
            push_resource(
                ctx.resources,
                original_url,
                normalized_url,
                kind,
                ctx.source,
                occurrence,
                PreviewResourceStatus::Failed,
                None,
                Some(error),
            );
            placeholder_for(kind).to_string()
        }
    }
}

async fn rewrite_resource_url(
    ctx: &mut RewriteContext<'_>,
    original_url: &str,
    fallback_type: PreviewResourceType,
    occurrence: &str,
    base_url: Option<&str>,
) -> String {
    let Some(normalized_url) = normalize_url(original_url, base_url) else {
        return original_url.to_string();
    };
    let kind = detect_type_from_url(&normalized_url, fallback_type);
    let id = resource_id(kind, &normalized_url);
    if ctx.user_allowed.contains(&id) {
        return allowed_resource_url(
            ctx,
            original_url,
            &normalized_url,
            kind,
            occurrence,
            PreviewResourceAllowedBy::User,
        )
        .await;
    }
    if ctx.user_allowed_urls.contains(&normalized_url) {
        return allowed_resource_url(
            ctx,
            original_url,
            &normalized_url,
            kind,
            occurrence,
            PreviewResourceAllowedBy::User,
        )
        .await;
    }
    if whitelist_allows(ctx.policy, &normalized_url, kind) {
        return allowed_resource_url(
            ctx,
            original_url,
            &normalized_url,
            kind,
            occurrence,
            PreviewResourceAllowedBy::Whitelist,
        )
        .await;
    }
    let status = if is_remote_url(&normalized_url) || resolve_local_file_path(&normalized_url).is_some() || PathBuf::from(&normalized_url).is_absolute() {
        PreviewResourceStatus::Pending
    } else {
        PreviewResourceStatus::Blocked
    };
    push_resource(
        ctx.resources,
        original_url,
        &normalized_url,
        kind,
        ctx.source,
        occurrence,
        status,
        None,
        (status == PreviewResourceStatus::Blocked).then(|| "Unsupported preview resource URL".to_string()),
    );
    placeholder_for(kind).to_string()
}

async fn rewrite_css_resources(
    ctx: &mut RewriteContext<'_>,
    css: &str,
    base_url: Option<&str>,
    occurrence: &str,
) -> Result<String, String> {
    let url_regex = css_url_regex();
    let import_regex = css_import_regex();
    let mut output = css.to_string();

    let matches = url_regex
        .captures_iter(css)
        .filter_map(|captures| {
            let raw_url = captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))?
                .as_str()
                .trim()
                .to_string();
            Some((
                captures.get(0)?.as_str().to_string(),
                raw_url,
            ))
        })
        .collect::<Vec<_>>();
    for (full, raw_url) in matches {
        let fallback = detect_type_from_url(&raw_url, PreviewResourceType::Image);
        let rewritten = rewrite_resource_url(ctx, &raw_url, fallback, occurrence, base_url).await;
        output = output.replace(&full, &format!("url(\"{}\")", rewritten));
    }

    let import_matches = import_regex
        .captures_iter(css)
        .filter_map(|captures| {
            let raw_url = captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))?
                .as_str()
                .trim()
                .to_string();
            Some((
                captures.get(0)?.as_str().to_string(),
                raw_url,
            ))
        })
        .collect::<Vec<_>>();
    for (full, raw_url) in import_matches {
        let rewritten =
            rewrite_resource_url(ctx, &raw_url, PreviewResourceType::Css, occurrence, base_url)
                .await;
        output = output.replace(&full, &format!("@import url(\"{}\")", rewritten));
    }

    Ok(output)
}

async fn rewrite_srcset(
    ctx: &mut RewriteContext<'_>,
    raw_srcset: &str,
    fallback_type: PreviewResourceType,
    occurrence: &str,
    base_url: Option<&str>,
) -> String {
    let mut rewritten_entries = Vec::new();
    for entry in raw_srcset.split(',') {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(raw_url) = parts.next() else {
            continue;
        };
        let descriptor = parts.collect::<Vec<_>>().join(" ");
        let rewritten = rewrite_resource_url(ctx, raw_url, fallback_type, occurrence, base_url).await;
        if descriptor.is_empty() {
            rewritten_entries.push(rewritten);
        } else {
            rewritten_entries.push(format!("{rewritten} {descriptor}"));
        }
    }
    rewritten_entries.join(", ")
}

async fn rewrite_html_resources(
    ctx: &mut RewriteContext<'_>,
    html: &str,
    base_url: Option<&str>,
) -> Result<String, String> {
    let tag_regex = Regex::new(r#"(?is)<(img|script|link|video|audio|source|iframe|object|embed|image)\b[^>]*>"#).unwrap();
    let attr_regex = html_resource_attr_regex();
    let style_block_regex = Regex::new(r#"(?is)<style\b[^>]*>(.*?)</style>"#).unwrap();
    let style_attr_regex = html_style_attr_regex();
    let mut output = html.to_string();

    let style_blocks = style_block_regex
        .captures_iter(html)
        .filter_map(|captures| {
            Some((
                captures.get(0)?.as_str().to_string(),
                captures.get(1)?.as_str().to_string(),
            ))
        })
        .collect::<Vec<_>>();
    for (full, css) in style_blocks {
        let rewritten_css = rewrite_css_resources(ctx, &css, base_url, "style block").await?;
        output = output.replace(&full, &full.replace(&css, &rewritten_css));
    }

    let style_attrs = style_attr_regex
        .captures_iter(&output)
        .filter_map(|captures| {
            let (quote, css) = if let Some(css) = captures.get(1) {
                ("\"".to_string(), css.as_str().to_string())
            } else {
                ("'".to_string(), captures.get(2)?.as_str().to_string())
            };
            Some((
                captures.get(0)?.as_str().to_string(),
                quote,
                css,
            ))
        })
        .collect::<Vec<_>>();
    for (full, quote, css) in style_attrs {
        let rewritten_css = rewrite_css_resources(ctx, &css, base_url, "inline style").await?;
        output = output.replace(&full, &format!("style={quote}{rewritten_css}{quote}"));
    }

    let tags = tag_regex.find_iter(&output).map(|m| m.as_str().to_string()).collect::<Vec<_>>();
    for tag in tags {
        let tag_name = tag
            .trim_start_matches('<')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('/')
            .to_ascii_lowercase();
        let is_stylesheet = tag_name == "link" && tag.to_ascii_lowercase().contains("stylesheet");
        let fallback = match tag_name.as_str() {
            "img" | "image" => PreviewResourceType::Image,
            "script" => PreviewResourceType::Script,
            "link" if is_stylesheet => PreviewResourceType::Css,
            "video" | "audio" | "source" => PreviewResourceType::Media,
            "iframe" | "object" | "embed" => PreviewResourceType::Html,
            _ => PreviewResourceType::Unknown,
        };
        if fallback == PreviewResourceType::Unknown && tag_name == "link" {
            continue;
        }
        let mut rewritten_tag = tag.clone();
        let attrs = attr_regex
            .captures_iter(&tag)
            .filter_map(|captures| {
                let (quote, raw_url) = if let Some(raw_url) = captures.get(2) {
                    ("\"".to_string(), raw_url.as_str().to_string())
                } else if let Some(raw_url) = captures.get(3) {
                    ("'".to_string(), raw_url.as_str().to_string())
                } else {
                    ("\"".to_string(), captures.get(4)?.as_str().to_string())
                };
                Some((
                    captures.get(0)?.as_str().to_string(),
                    captures.get(1)?.as_str().to_ascii_lowercase(),
                    quote,
                    raw_url,
                ))
            })
            .collect::<Vec<_>>();
        for (full_attr, attr_name, quote, raw_url) in attrs {
            let occurrence = format!("<{tag_name}> {attr_name}");
            let rewritten = if attr_name == "srcset" {
                rewrite_srcset(ctx, &raw_url, fallback, &occurrence, base_url).await
            } else {
                rewrite_resource_url(ctx, &raw_url, fallback, &occurrence, base_url).await
            };
            rewritten_tag = rewritten_tag.replace(
                &full_attr,
                &format!(r#"{attr_name}={quote}{rewritten}{quote}"#),
            );
        }
        output = output.replace(&tag, &rewritten_tag);
    }

    Ok(output)
}

async fn rewrite_markdown_resources(
    ctx: &mut RewriteContext<'_>,
    markdown: &str,
    base_url: Option<&str>,
) -> Result<String, String> {
    let image_regex = Regex::new(r#"!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap();
    let mut output = markdown.to_string();
    let images = image_regex
        .captures_iter(markdown)
        .filter_map(|captures| {
            Some((
                captures.get(0)?.as_str().to_string(),
                captures.get(1)?.as_str().to_string(),
                captures.get(2)?.as_str().to_string(),
            ))
        })
        .collect::<Vec<_>>();
    for (full, alt, raw_url) in images {
        let rewritten =
            rewrite_resource_url(ctx, &raw_url, PreviewResourceType::Image, "markdown image", base_url)
                .await;
        output = output.replace(&full, &format!("![{}]({})", alt, rewritten));
    }

    if output.contains('<') {
        output = rewrite_html_resources(ctx, &output, base_url).await?;
    }
    Ok(output)
}

async fn rewrite_preview_file(
    ctx: &mut RewriteContext<'_>,
    request: &mut PreviewFileRequestEvent,
) -> Result<(), String> {
    for file in request.files.iter_mut() {
        match file.file_type.as_str() {
            "markdown" => {
                if let Some(content) = file.content.clone() {
                    file.content =
                        Some(rewrite_markdown_resources(ctx, &content, file.url.as_deref()).await?);
                } else if let Some(raw_url) = file.url.clone() {
                    let normalized = normalize_url(&raw_url, None).unwrap_or(raw_url.clone());
                    let id = resource_id(PreviewResourceType::Markdown, &normalized);
                    if ctx.user_allowed.contains(&id)
                        || whitelist_allows(ctx.policy, &normalized, PreviewResourceType::Markdown)
                    {
                        let relay = allowed_resource_url(
                            ctx,
                            &raw_url,
                            &normalized,
                            PreviewResourceType::Markdown,
                            "file url",
                            if ctx.user_allowed.contains(&id) {
                                PreviewResourceAllowedBy::User
                            } else {
                                PreviewResourceAllowedBy::Whitelist
                            },
                        )
                        .await;
                        if relay.starts_with(&format!("{}://", PREVIEW_FILE_RELAY_SCHEME)) {
                            if let Ok((path, _)) = download_or_read_resource(ctx, &normalized, PreviewResourceType::Markdown).await {
                                if let Ok(content) = fs::read_to_string(path) {
                                    file.content = Some(rewrite_markdown_resources(ctx, &content, Some(&normalized)).await?);
                                    file.url = None;
                                }
                            }
                        }
                    } else {
                        let placeholder =
                            rewrite_resource_url(ctx, &raw_url, PreviewResourceType::Markdown, "file url", None).await;
                        file.url = (!placeholder.is_empty()).then_some(placeholder);
                    }
                }
            }
            "text" => {
                if let Some(raw_url) = file.url.clone() {
                    let normalized = normalize_url(&raw_url, None).unwrap_or(raw_url.clone());
                    let id = resource_id(PreviewResourceType::Text, &normalized);
                    if ctx.user_allowed.contains(&id)
                        || whitelist_allows(ctx.policy, &normalized, PreviewResourceType::Text)
                    {
                        let allowed_by = if ctx.user_allowed.contains(&id) {
                            PreviewResourceAllowedBy::User
                        } else {
                            PreviewResourceAllowedBy::Whitelist
                        };
                        let _ = allowed_resource_url(
                            ctx,
                            &raw_url,
                            &normalized,
                            PreviewResourceType::Text,
                            "file url",
                            allowed_by,
                        )
                        .await;
                        if let Ok((path, _)) = download_or_read_resource(ctx, &normalized, PreviewResourceType::Text).await {
                            if let Ok(content) = fs::read_to_string(path) {
                                file.content = Some(content);
                                file.url = None;
                            }
                        }
                    } else {
                        let placeholder =
                            rewrite_resource_url(ctx, &raw_url, PreviewResourceType::Text, "file url", None).await;
                        file.url = (!placeholder.is_empty()).then_some(placeholder);
                    }
                }
            }
            "html" => {
                if let Some(content) = file.content.clone() {
                    file.content = Some(rewrite_html_resources(ctx, &content, file.url.as_deref()).await?);
                } else if let Some(raw_url) = file.url.clone() {
                    let rewritten =
                        rewrite_resource_url(ctx, &raw_url, PreviewResourceType::Html, "file url", None).await;
                    file.url = (!rewritten.is_empty()).then_some(rewritten);
                }
            }
            "image" | "pdf" => {
                if let Some(raw_url) = file.url.clone() {
                    let fallback = if file.file_type == "pdf" {
                        PreviewResourceType::Pdf
                    } else {
                        PreviewResourceType::Image
                    };
                    let rewritten = rewrite_resource_url(ctx, &raw_url, fallback, "file url", None).await;
                    file.url = (!rewritten.is_empty()).then_some(rewritten);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn build_context<'a>(
    app_handle: &'a AppHandle,
    _request_id: &'a str,
    source: &'a str,
    user_allowed: &'a HashSet<String>,
    user_allowed_urls: &'a HashSet<String>,
    policy: &'a PreviewExternalResourcePolicy,
    force_proxy: bool,
    resources: &'a mut Vec<PreviewExternalResourceInfo>,
) -> Result<RewriteContext<'a>, String> {
    let relay_state = app_handle
        .try_state::<PreviewFileRelayState>()
        .ok_or_else(|| "PreviewFileRelayState not found".to_string())?;
    Ok(RewriteContext {
        app_handle,
        relay_state: relay_state.inner(),
        policy,
        source,
        user_allowed,
        user_allowed_urls,
        force_proxy,
        resources,
    })
}

fn attach_code_resources(
    mut event: PreviewCodeRequestEvent,
    resources: Vec<PreviewExternalResourceInfo>,
) -> PreviewCodeRequestEvent {
    event.external_resources = (!resources.is_empty()).then_some(PreviewExternalResourcesPayload {
        request_id: event.request_id.clone(),
        resources,
    });
    event
}

fn attach_file_resources(
    mut event: PreviewFileRequestEvent,
    resources: Vec<PreviewExternalResourceInfo>,
) -> PreviewFileRequestEvent {
    event.external_resources = (!resources.is_empty()).then_some(PreviewExternalResourcesPayload {
        request_id: event.request_id.clone(),
        resources,
    });
    event
}

pub async fn prepare_preview_code_event(
    app_handle: &AppHandle,
    state: &PreviewResourceState,
    event: PreviewCodeRequestEvent,
) -> Result<PreviewCodeRequestEvent, String> {
    prepare_preview_code_event_with_proxy(app_handle, state, event, false).await
}

async fn prepare_preview_code_event_with_proxy(
    app_handle: &AppHandle,
    state: &PreviewResourceState,
    mut event: PreviewCodeRequestEvent,
    force_proxy: bool,
) -> Result<PreviewCodeRequestEvent, String> {
    let original_event = event.clone();
    let policy = load_policy_from_state(app_handle).await;
    let user_allowed = state.allowed_for_request(&event.request_id)?;
    let user_allowed_urls = HashSet::new();
    let mut resources = Vec::new();
    let mut ctx = build_context(
        app_handle,
        &event.request_id,
        "preview_code",
        &user_allowed,
        &user_allowed_urls,
        &policy,
        force_proxy,
        &mut resources,
    )
    .await?;
    event.code = rewrite_html_resources(&mut ctx, &event.code, None).await?;
    drop(ctx);
    state.remember_resources(&resources)?;
    let event = attach_code_resources(event, resources);
    state.store_code(original_event)?;
    Ok(event)
}

pub async fn prepare_preview_file_event(
    app_handle: &AppHandle,
    state: &PreviewResourceState,
    event: PreviewFileRequestEvent,
) -> Result<PreviewFileRequestEvent, String> {
    prepare_preview_file_event_with_proxy(app_handle, state, event, false).await
}

async fn prepare_preview_file_event_with_proxy(
    app_handle: &AppHandle,
    state: &PreviewResourceState,
    mut event: PreviewFileRequestEvent,
    force_proxy: bool,
) -> Result<PreviewFileRequestEvent, String> {
    let original_event = event.clone();
    let policy = load_policy_from_state(app_handle).await;
    let user_allowed = state.allowed_for_request(&event.request_id)?;
    let user_allowed_urls = HashSet::new();
    let mut resources = Vec::new();
    let request_id = event.request_id.clone();
    let mut ctx = build_context(
        app_handle,
        &request_id,
        "preview_file",
        &user_allowed,
        &user_allowed_urls,
        &policy,
        force_proxy,
        &mut resources,
    )
    .await?;
    rewrite_preview_file(&mut ctx, &mut event).await?;
    drop(ctx);
    state.remember_resources(&resources)?;
    let event = attach_file_resources(event, resources);
    state.store_file(original_event)?;
    Ok(event)
}

pub async fn scan_preview_code_event(
    app_handle: &AppHandle,
    state: &PreviewResourceState,
    mut event: PreviewCodeRequestEvent,
) -> Result<PreviewCodeRequestEvent, String> {
    let original_event = event.clone();
    let mut policy = load_policy_from_state(app_handle).await;
    policy.allowed_domains.clear();
    let user_allowed = HashSet::new();
    let user_allowed_urls = HashSet::new();
    let mut resources = Vec::new();
    let mut ctx = build_context(
        app_handle,
        &event.request_id,
        "preview_code",
        &user_allowed,
        &user_allowed_urls,
        &policy,
        false,
        &mut resources,
    )
    .await?;
    event.code = rewrite_html_resources(&mut ctx, &event.code, None).await?;
    drop(ctx);
    state.remember_resources(&resources)?;
    let event = attach_code_resources(event, resources);
    state.store_code(original_event)?;
    Ok(event)
}

pub async fn load_policy_from_state(app_handle: &AppHandle) -> PreviewExternalResourcePolicy {
    let mut policy = if let Some(feature_state) = app_handle.try_state::<crate::FeatureConfigState>() {
        let config_map = feature_state.config_feature_map.lock().await;
        let enabled = config_map
            .get(FEATURE_CODE)
            .and_then(|feature| feature.get("enabled"))
            .map(|config| config.value.trim() == "true")
            .unwrap_or(true);
        let mut policy = if enabled {
            config_map
                .get(FEATURE_CODE)
                .and_then(|feature| feature.get("policy"))
                .and_then(|config| serde_json::from_str::<PreviewExternalResourcePolicy>(&config.value).ok())
                .unwrap_or_default()
        } else {
            PreviewExternalResourcePolicy::default()
        };
        if !enabled {
            policy.allowed_domains.clear();
        }
        policy
    } else {
        PreviewExternalResourcePolicy::default()
    };
    apply_ui_interaction_env_policy(app_handle, &mut policy);
    policy
}

fn load_builtin_env_config(app_handle: &AppHandle, command: &str) -> HashMap<String, String> {
    let Ok(db) = MCPDatabase::new(app_handle) else {
        return HashMap::new();
    };
    let Ok(mut stmt) = db.conn.prepare(
        "SELECT environment_variables FROM mcp_server WHERE command = ? AND is_builtin = 1 LIMIT 1",
    ) else {
        return HashMap::new();
    };
    let env_text: Option<String> =
        stmt.query_row([command], |row| row.get::<_, Option<String>>(0)).unwrap_or(None);
    let mut config = HashMap::new();
    if let Some(text) = env_text {
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                config.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    config
}

fn parse_bool_config(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_positive_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok().filter(|value| *value > 0)
}

fn apply_ui_interaction_env_policy(app_handle: &AppHandle, policy: &mut PreviewExternalResourcePolicy) {
    let config = load_builtin_env_config(app_handle, "aipp:ui_interaction");
    if config.is_empty() {
        return;
    }

    if let Some(raw_policy) = config.get("PREVIEW_EXTERNAL_RESOURCE_POLICY") {
        if let Ok(parsed) = serde_json::from_str::<PreviewExternalResourcePolicy>(raw_policy) {
            *policy = parsed;
        }
    }
    if let Some(raw_domains) = config.get("PREVIEW_ALLOWED_DOMAINS_JSON") {
        if let Ok(domains) = serde_json::from_str::<Vec<PreviewResourceDomainRule>>(raw_domains) {
            policy.allowed_domains = domains;
        }
    }
    if let Some(value) = config.get("PREVIEW_MAX_RESOURCE_BYTES").and_then(|value| parse_positive_u64(value)) {
        policy.limits.max_resource_bytes = value;
    }
    if let Some(value) = config.get("PREVIEW_REQUEST_TIMEOUT_SECS").and_then(|value| parse_positive_u64(value)) {
        policy.limits.request_timeout_secs = value;
    }
    if let Some(value) = config.get("PREVIEW_CACHE_TTL_SECS").and_then(|value| parse_positive_u64(value)) {
        policy.limits.cache_ttl_secs = value;
    }
    if config
        .get("PREVIEW_EXTERNAL_RESOURCES_ENABLED")
        .and_then(|value| parse_bool_config(value))
        == Some(false)
    {
        policy.allowed_domains.clear();
    }
}

async fn save_policy_to_db_and_state(
    app_handle: &AppHandle,
    state: &State<'_, crate::FeatureConfigState>,
    policy: &PreviewExternalResourcePolicy,
) -> Result<(), String> {
    let policy_json = serde_json::to_string(policy).map_err(|e| e.to_string())?;
    let mut config = HashMap::new();
    config.insert("enabled".to_string(), "true".to_string());
    config.insert("policy".to_string(), policy_json);
    let db = SystemDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let _ = db.delete_feature_config_by_feature_code(FEATURE_CODE);
    for (key, value) in &config {
        db.add_feature_config(&FeatureConfig {
            id: None,
            feature_code: FEATURE_CODE.to_string(),
            key: key.clone(),
            value: value.clone(),
            data_type: "string".to_string(),
            description: Some("".to_string()),
        })
        .map_err(|e| e.to_string())?;
    }

    let mut configs = state.configs.lock().await;
    let mut config_feature_map = state.config_feature_map.lock().await;
    configs.retain(|item| item.feature_code != FEATURE_CODE);
    config_feature_map.remove(FEATURE_CODE);
    for (key, value) in config {
        let item = FeatureConfig {
            id: None,
            feature_code: FEATURE_CODE.to_string(),
            key: key.clone(),
            value,
            data_type: "string".to_string(),
            description: Some("".to_string()),
        };
        configs.push(item.clone());
        config_feature_map
            .entry(FEATURE_CODE.to_string())
            .or_insert_with(HashMap::new)
            .insert(key, item);
    }
    let _ = app_handle.emit("feature_config_changed", ());
    Ok(())
}

fn add_domains_to_policy(
    policy: &mut PreviewExternalResourcePolicy,
    resources: &[PreviewExternalResourceInfo],
) {
    for resource in resources {
        if !is_https_url(&resource.normalized_url) {
            continue;
        }
        let Some(domain) = host_for_url(&resource.normalized_url) else {
            continue;
        };
        let ty = resource.resource_type.as_str().to_string();
        if let Some(rule) = policy
            .allowed_domains
            .iter_mut()
            .find(|rule| normalize_domain(&rule.domain) == domain && !rule.include_subdomains)
        {
            if !rule.types.iter().any(|existing| existing.eq_ignore_ascii_case(&ty)) {
                rule.types.push(ty);
            }
            rule.auto_load = true;
            continue;
        }
        policy.allowed_domains.push(PreviewResourceDomainRule {
            domain,
            include_subdomains: false,
            types: vec![ty],
            auto_load: true,
        });
    }
}

fn selected_resource_infos(
    selected_resources: &[PreviewSelectedExternalResource],
) -> Vec<PreviewExternalResourceInfo> {
    selected_resources
        .iter()
        .filter_map(|resource| {
            let raw_url = resource
                .normalized_url
                .as_deref()
                .unwrap_or(&resource.original_url);
            let normalized_url = normalize_url(raw_url, None)
                .or_else(|| normalize_url(&resource.original_url, None))?;
            let kind = detect_type_from_url(&normalized_url, resource.resource_type);
            Some(PreviewExternalResourceInfo {
                id: resource_id(kind, &normalized_url),
                original_url: resource.original_url.clone(),
                normalized_url,
                resource_type: kind,
                source: "preview_code".to_string(),
                occurrence: resource
                    .occurrence
                    .clone()
                    .unwrap_or_else(|| "selected resource".to_string()),
                status: PreviewResourceStatus::Pending,
                allowed_by: None,
                risk: kind.risk(),
                reason: None,
            })
        })
        .collect()
}

fn ensure_selected_resources_reported(
    resources: &mut Vec<PreviewExternalResourceInfo>,
    selected_resources: &[PreviewExternalResourceInfo],
    source: &str,
) {
    for resource in selected_resources {
        if resources.iter().any(|item| item.id == resource.id) {
            continue;
        }
        push_resource(
            resources,
            &resource.original_url,
            &resource.normalized_url,
            resource.resource_type,
            source,
            &resource.occurrence,
            PreviewResourceStatus::Failed,
            None,
            Some("Selected preview resource was not rewritten".to_string()),
        );
    }
}

#[tauri::command]
pub async fn prepare_preview_code_request_for_ui(
    app_handle: AppHandle,
    conversation_id: Option<i64>,
    request: PreviewCodeRequest,
) -> Result<PreviewCodeRequestEvent, String> {
    request.validate()?;
    let interaction_mode = request.interaction_mode().to_string();
    let event = PreviewCodeRequestEvent {
        request_id: uuid::Uuid::new_v4().to_string(),
        conversation_id,
        title: request.title,
        renderer: request.renderer,
        code: request.code,
        loading_messages: request.loading_messages,
        interaction_mode,
        metadata: request.metadata,
        external_resources: None,
    };
    let state = app_handle
        .try_state::<PreviewResourceState>()
        .ok_or_else(|| "PreviewResourceState not found".to_string())?;
    prepare_preview_code_event(&app_handle, state.inner(), event).await
}

#[tauri::command]
pub async fn scan_preview_code_external_resources_for_ui(
    app_handle: AppHandle,
    conversation_id: Option<i64>,
    request: PreviewCodeRequest,
) -> Result<PreviewCodeRequestEvent, String> {
    request.validate()?;
    let interaction_mode = request.interaction_mode().to_string();
    let event = PreviewCodeRequestEvent {
        request_id: uuid::Uuid::new_v4().to_string(),
        conversation_id,
        title: request.title,
        renderer: request.renderer,
        code: request.code,
        loading_messages: request.loading_messages,
        interaction_mode,
        metadata: request.metadata,
        external_resources: None,
    };
    let state = app_handle
        .try_state::<PreviewResourceState>()
        .ok_or_else(|| "PreviewResourceState not found".to_string())?;
    scan_preview_code_event(&app_handle, state.inner(), event).await
}

#[tauri::command]
pub async fn authorize_preview_code_external_resource_urls(
    app_handle: AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    conversation_id: Option<i64>,
    request: PreviewCodeRequest,
    resources: Vec<PreviewSelectedExternalResource>,
    add_to_whitelist: Option<bool>,
    use_proxy: Option<bool>,
) -> Result<PreviewResourceAuthorizationResult, String> {
    request.validate()?;
    if resources.is_empty() {
        return Err("No preview resources selected".to_string());
    }
    let state = app_handle
        .try_state::<PreviewResourceState>()
        .ok_or_else(|| "PreviewResourceState not found".to_string())?;
    let selected_infos = selected_resource_infos(&resources);
    if selected_infos.is_empty() {
        return Err("No valid preview resources selected".to_string());
    }
    if add_to_whitelist.unwrap_or(false) {
        let mut policy = load_policy_from_state(&app_handle).await;
        add_domains_to_policy(&mut policy, &selected_infos);
        save_policy_to_db_and_state(&app_handle, &feature_config_state, &policy).await?;
    }

    let selected_urls = selected_infos
        .iter()
        .map(|resource| resource.normalized_url.clone())
        .collect::<HashSet<_>>();
    let interaction_mode = request.interaction_mode().to_string();
    let mut event = PreviewCodeRequestEvent {
        request_id: uuid::Uuid::new_v4().to_string(),
        conversation_id,
        title: request.title,
        renderer: request.renderer,
        code: request.code,
        loading_messages: request.loading_messages,
        interaction_mode,
        metadata: request.metadata,
        external_resources: None,
    };
    let original_event = event.clone();
    let policy = load_policy_from_state(&app_handle).await;
    let user_allowed = HashSet::new();
    let mut scanned_resources = Vec::new();
    let mut ctx = build_context(
        &app_handle,
        &event.request_id,
        "preview_code",
        &user_allowed,
        &selected_urls,
        &policy,
        use_proxy.unwrap_or(false),
        &mut scanned_resources,
    )
    .await?;
    event.code = rewrite_html_resources(&mut ctx, &event.code, None).await?;
    drop(ctx);
    ensure_selected_resources_reported(&mut scanned_resources, &selected_infos, "preview_code");
    state.remember_resources(&scanned_resources)?;
    state.store_code(original_event)?;
    Ok(PreviewResourceAuthorizationResult {
        preview_code: Some(attach_code_resources(event, scanned_resources)),
        preview_file: None,
    })
}

#[tauri::command]
pub async fn authorize_preview_external_resources(
    app_handle: AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    request_id: String,
    resource_ids: Vec<String>,
    #[allow(unused_variables)] add_to_whitelist: Option<bool>,
    use_proxy: Option<bool>,
) -> Result<PreviewResourceAuthorizationResult, String> {
    if resource_ids.is_empty() {
        return Err("No preview resources selected".to_string());
    }
    let state = app_handle
        .try_state::<PreviewResourceState>()
        .ok_or_else(|| "PreviewResourceState not found".to_string())?;
    state.authorize(&request_id, &resource_ids)?;
    let selected_resources = state.get_resource_meta(&resource_ids)?;
    if add_to_whitelist.unwrap_or(false) {
        let mut policy = load_policy_from_state(&app_handle).await;
        add_domains_to_policy(&mut policy, &selected_resources);
        save_policy_to_db_and_state(&app_handle, &feature_config_state, &policy).await?;
    }
    let stored = state
        .get_request(&request_id)?
        .ok_or_else(|| "Preview resource request not found or expired".to_string())?;
    match stored {
        StoredPreviewRequest::Code(event) => {
            let mut prepared = prepare_preview_code_event_with_proxy(
                &app_handle,
                state.inner(),
                event,
                use_proxy.unwrap_or(false),
            )
            .await?;
            let mut resources = prepared
                .external_resources
                .take()
                .map(|payload| payload.resources)
                .unwrap_or_default();
            ensure_selected_resources_reported(&mut resources, &selected_resources, "preview_code");
            state.remember_resources(&resources)?;
            let prepared = attach_code_resources(prepared, resources);
            Ok(PreviewResourceAuthorizationResult {
                preview_code: Some(prepared),
                preview_file: None,
            })
        }
        StoredPreviewRequest::File(event) => {
            let mut prepared = prepare_preview_file_event_with_proxy(
                &app_handle,
                state.inner(),
                event,
                use_proxy.unwrap_or(false),
            )
            .await?;
            let mut resources = prepared
                .external_resources
                .take()
                .map(|payload| payload.resources)
                .unwrap_or_default();
            ensure_selected_resources_reported(&mut resources, &selected_resources, "preview_file");
            state.remember_resources(&resources)?;
            let prepared = attach_file_resources(prepared, resources);
            Ok(PreviewResourceAuthorizationResult {
                preview_code: None,
                preview_file: Some(prepared),
            })
        }
    }
}

#[tauri::command]
pub async fn get_preview_external_resource_policy(
    app_handle: AppHandle,
) -> Result<PreviewExternalResourcePolicy, String> {
    Ok(load_policy_from_state(&app_handle).await)
}

#[tauri::command]
pub async fn save_preview_external_resource_policy(
    app_handle: AppHandle,
    feature_config_state: State<'_, crate::FeatureConfigState>,
    policy: PreviewExternalResourcePolicy,
) -> Result<(), String> {
    save_policy_to_db_and_state(&app_handle, &feature_config_state, &policy).await
}

#[cfg(test)]
mod tests {
    use super::{
        css_import_regex, css_url_regex, html_resource_attr_regex, html_style_attr_regex,
        placeholder_for, preview_network_proxy_from_config_map, resource_id, whitelist_allows,
        PreviewExternalResourcePolicy, PreviewResourceDomainRule, PreviewResourceLimits,
        PreviewResourceType,
    };
    use crate::db::system_db::FeatureConfig;
    use std::collections::HashMap;

    #[test]
    fn test_resource_id_changes_by_type() {
        let image_id = resource_id(PreviewResourceType::Image, "https://example.com/a.png");
        let script_id = resource_id(PreviewResourceType::Script, "https://example.com/a.png");
        assert_ne!(image_id, script_id);
    }

    #[test]
    fn test_image_placeholder_is_safe_for_single_quoted_attributes() {
        let placeholder = placeholder_for(PreviewResourceType::Image);
        assert!(!placeholder.contains('\''));
    }

    #[test]
    fn test_whitelist_requires_https_domain_and_type_match() {
        let policy = PreviewExternalResourcePolicy {
            allowed_domains: vec![PreviewResourceDomainRule {
                domain: "cdn.example.com".to_string(),
                include_subdomains: false,
                types: vec!["css".to_string()],
                auto_load: true,
            }],
            limits: PreviewResourceLimits {
                max_resource_bytes: 1024,
                request_timeout_secs: 3,
                cache_ttl_secs: 60,
            },
        };
        assert!(whitelist_allows(
            &policy,
            "https://cdn.example.com/app.css",
            PreviewResourceType::Css
        ));
        assert!(!whitelist_allows(
            &policy,
            "http://cdn.example.com/app.css",
            PreviewResourceType::Css
        ));
        assert!(!whitelist_allows(
            &policy,
            "https://cdn.example.com/app.js",
            PreviewResourceType::Script
        ));
    }

    #[test]
    fn test_whitelist_subdomain_matching_requires_real_subdomain() {
        let policy = PreviewExternalResourcePolicy {
            allowed_domains: vec![PreviewResourceDomainRule {
                domain: "example.com".to_string(),
                include_subdomains: true,
                types: vec!["image".to_string()],
                auto_load: true,
            }],
            limits: PreviewResourceLimits {
                max_resource_bytes: 1024,
                request_timeout_secs: 3,
                cache_ttl_secs: 60,
            },
        };
        assert!(whitelist_allows(
            &policy,
            "https://assets.example.com/a.png",
            PreviewResourceType::Image
        ));
        assert!(!whitelist_allows(
            &policy,
            "https://badexample.com/a.png",
            PreviewResourceType::Image
        ));
    }

    #[test]
    fn test_html_attribute_regex_supports_single_and_double_quotes() {
        let attr_regex = html_resource_attr_regex();
        let tag = r#"<img src="https://example.com/a.png" srcset='https://example.com/a.png 1x, https://example.com/a@2x.png 2x' data=https://example.com/raw.json>"#;
        let attrs = attr_regex
            .captures_iter(tag)
            .filter_map(|captures| {
                let value = captures
                    .get(2)
                    .or_else(|| captures.get(3))
                    .or_else(|| captures.get(4))?
                    .as_str()
                    .to_string();
                Some((captures.get(1)?.as_str().to_ascii_lowercase(), value))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            attrs,
            vec![
                ("src".to_string(), "https://example.com/a.png".to_string()),
                (
                    "srcset".to_string(),
                    "https://example.com/a.png 1x, https://example.com/a@2x.png 2x".to_string()
                ),
                ("data".to_string(), "https://example.com/raw.json".to_string()),
            ]
        );
    }

    #[test]
    fn test_html_style_attribute_regex_supports_single_and_double_quotes() {
        let style_regex = html_style_attr_regex();
        let html = r#"<div style='background:url("https://example.com/a.png")'></div><span style="color:red"></span>"#;
        let values = style_regex
            .captures_iter(html)
            .filter_map(|captures| {
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .map(|value| value.as_str().to_string())
            })
            .collect::<Vec<_>>();

        assert_eq!(
            values,
            vec![
                r#"background:url("https://example.com/a.png")"#.to_string(),
                "color:red".to_string(),
            ]
        );
    }

    #[test]
    fn test_css_url_regex_supports_single_double_and_unquoted_urls() {
        let url_regex = css_url_regex();
        let css = r#"
            .a { background: url("https://example.com/a.png"); }
            .b { background: url('https://example.com/b.png'); }
            .c { background: url(https://example.com/c.png); }
        "#;
        let values = url_regex
            .captures_iter(css)
            .filter_map(|captures| {
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .or_else(|| captures.get(3))
                    .map(|value| value.as_str().trim().to_string())
            })
            .collect::<Vec<_>>();

        assert_eq!(
            values,
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.png".to_string(),
                "https://example.com/c.png".to_string(),
            ]
        );
    }

    #[test]
    fn test_css_import_regex_supports_single_and_double_quotes() {
        let import_regex = css_import_regex();
        let css = r#"
            @import "https://example.com/a.css";
            @import url('https://example.com/b.css');
            @import url(https://example.com/c.css);
        "#;
        let values = import_regex
            .captures_iter(css)
            .filter_map(|captures| {
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .or_else(|| captures.get(3))
                    .map(|value| value.as_str().trim().to_string())
            })
            .collect::<Vec<_>>();

        assert_eq!(
            values,
            vec![
                "https://example.com/a.css".to_string(),
                "https://example.com/b.css".to_string(),
                "https://example.com/c.css".to_string(),
            ]
        );
    }

    #[test]
    fn test_preview_network_proxy_uses_global_network_config_only() {
        let mut network_config = HashMap::new();
        network_config.insert(
            "network_proxy".to_string(),
            FeatureConfig {
                id: None,
                feature_code: "network_config".to_string(),
                key: "network_proxy".to_string(),
                value: "http://proxy.example.com:8080".to_string(),
                data_type: "string".to_string(),
                description: None,
            },
        );

        let mut config_map = HashMap::new();
        config_map.insert("network_config".to_string(), network_config);

        assert_eq!(
            preview_network_proxy_from_config_map(&config_map),
            Some("http://proxy.example.com:8080".to_string())
        );

        let mut wrong_scope_map = HashMap::new();
        let mut preview_feature = HashMap::new();
        preview_feature.insert(
            "network_proxy".to_string(),
            FeatureConfig {
                id: None,
                feature_code: "preview_external_resources".to_string(),
                key: "network_proxy".to_string(),
                value: "http://wrong.example.com:8080".to_string(),
                data_type: "string".to_string(),
                description: None,
            },
        );
        wrong_scope_map.insert("preview_external_resources".to_string(), preview_feature);

        assert_eq!(preview_network_proxy_from_config_map(&wrong_scope_map), None);
    }

    #[test]
    fn test_preview_network_proxy_reads_saved_feature_config_rows() {
        let saved_rows = vec![
            FeatureConfig {
                id: Some(1),
                feature_code: "network_config".to_string(),
                key: "request_timeout".to_string(),
                value: "180".to_string(),
                data_type: "string".to_string(),
                description: None,
            },
            FeatureConfig {
                id: Some(2),
                feature_code: "network_config".to_string(),
                key: "retry_attempts".to_string(),
                value: "3".to_string(),
                data_type: "string".to_string(),
                description: None,
            },
            FeatureConfig {
                id: Some(3),
                feature_code: "network_config".to_string(),
                key: "network_proxy".to_string(),
                value: " http://127.0.0.1:7890 ".to_string(),
                data_type: "string".to_string(),
                description: None,
            },
            FeatureConfig {
                id: Some(4),
                feature_code: "network_config".to_string(),
                key: "custom_headers".to_string(),
                value: "{}".to_string(),
                data_type: "string".to_string(),
                description: None,
            },
        ];
        let mut config_map: HashMap<String, HashMap<String, FeatureConfig>> = HashMap::new();
        for config in saved_rows {
            config_map
                .entry(config.feature_code.clone())
                .or_default()
                .insert(config.key.clone(), config);
        }

        assert_eq!(
            preview_network_proxy_from_config_map(&config_map),
            Some("http://127.0.0.1:7890".to_string())
        );
    }
}

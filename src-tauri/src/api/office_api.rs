use crate::api::plugin_api::{
    emit_plugin_registry_changed, fetch_official_plugins, get_plugin_root_path,
    install_plugin_archive_source, install_plugin_dir_atomic, sync_registry,
    PluginInstallProgressEvent, PluginInstallRecipeDir, PluginInstallRecipeSource,
    PluginInstallRecipeSourceType,
};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::plugin_db::PluginDatabase;
use crate::mcp::builtin_mcp::interaction::PreviewFileRelayState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
#[cfg(target_os = "android")]
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::{Component, Path, PathBuf};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(target_os = "android"))]
use tauri_plugin_opener::OpenerExt;

pub const OFFICE_PREVIEW_SCHEME: &str = "aipp-office";
pub const OFFICE_PREVIEW_PLUGIN_CODE: &str = "office-preview-plugin";
pub const DESKTOP_MAX_OFFICE_BYTES: u64 = 32 * 1024 * 1024;
pub const MOBILE_MAX_OFFICE_BYTES: u64 = 20 * 1024 * 1024;

const OLE2_MAGIC: &[u8; 8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const ZIP_LOCAL_MAGIC: &[u8; 4] = b"PK\x03\x04";
const ZIP_EMPTY_MAGIC: &[u8; 4] = b"PK\x05\x06";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfficeFormat {
    Docx,
    Xlsx,
    Pptx,
}

impl OfficeFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
        }
    }

    pub fn required_ooxml_entry(self) -> &'static str {
        match self {
            Self::Docx => "word/document.xml",
            Self::Xlsx => "xl/workbook.xml",
            Self::Pptx => "ppt/presentation.xml",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficePreviewError {
    pub code: String,
    pub message: String,
}

impl OfficePreviewError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn to_wire_string(&self) -> String {
        format!("OFFICE_PREVIEW_ERROR:{}:{}", self.code, self.message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OfficePreviewSource {
    /// `office_file_ref` 引用号：用户交给助手的文件或 AI 生成的副本。
    #[serde(default)]
    pub ref_id: Option<String>,
    pub attachment_id: Option<i64>,
    pub relay_token: Option<String>,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficePreviewPluginStatus {
    pub plugin_code: String,
    pub installed: bool,
    pub enabled: bool,
    pub viewer_ready: bool,
    pub version: Option<String>,
    pub local_dev_available: bool,
    /// 已安装但版本低于宿主要求（查看器协议随宿主演进），需要重新下载新版。
    pub update_required: bool,
    /// 已安装版本支持选区、编辑与 AI 修改（协议 2）。
    pub edit_supported: bool,
}

pub fn classify_office_extension(file_name_or_path: &str) -> Result<OfficeFormat, OfficePreviewError> {
    let ext = Path::new(file_name_or_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "docx" => Ok(OfficeFormat::Docx),
        "xlsx" => Ok(OfficeFormat::Xlsx),
        "pptx" => Ok(OfficeFormat::Pptx),
        "doc" | "xls" | "ppt" => Err(OfficePreviewError::new(
            "unsupported_legacy_format",
            format!(
                "暂不支持旧版 .{} 二进制格式预览，请转换为 .{}x 或使用外部办公软件打开",
                ext, ext
            ),
        )),
        other => Err(OfficePreviewError::new(
            "unsupported_format",
            format!("不支持的 Office 文件格式：.{}", if other.is_empty() { "unknown" } else { other }),
        )),
    }
}

pub fn validate_office_bytes(
    file_name: &str,
    bytes: &[u8],
    max_bytes: u64,
) -> Result<OfficeFormat, OfficePreviewError> {
    let format = classify_office_extension(file_name)?;
    let size = bytes.len() as u64;
    if size == 0 {
        return Err(OfficePreviewError::new("corrupt_file", "文件内容为空，无法预览"));
    }
    if size > max_bytes {
        let limit_mb = max_bytes / (1024 * 1024);
        let actual_mb = (size as f64) / (1024.0 * 1024.0);
        return Err(OfficePreviewError::new(
            "file_too_large",
            format!(
                "文件大小（{:.1} MiB）超过当前平台预览上限（{} MiB），请使用外部办公软件打开",
                actual_mb, limit_mb
            ),
        ));
    }

    if bytes.len() >= 8 && &bytes[..8] == OLE2_MAGIC {
        return Err(OfficePreviewError::new(
            "password_encrypted",
            "该文件受密码保护或为旧版 OLE2 加密容器，暂不支持应用内预览，请使用外部办公软件打开",
        ));
    }

    if bytes.len() < 4
        || (&bytes[..4] != ZIP_LOCAL_MAGIC && &bytes[..4] != ZIP_EMPTY_MAGIC)
    {
        return Err(OfficePreviewError::new(
            "corrupt_file",
            "文件头不是有效的 OOXML (ZIP) 容器，文件可能已损坏",
        ));
    }

    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| {
        OfficePreviewError::new("corrupt_file", format!("OOXML 压缩结构已损坏：{}", e))
    })?;

    let mut has_encrypted_package = false;
    let mut has_required_entry = false;
    let required = format.required_ooxml_entry();

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index_raw(i) {
            let name = entry.name().replace('\\', "/");
            if name.eq_ignore_ascii_case("EncryptedPackage")
                || name.eq_ignore_ascii_case("EncryptionInfo")
            {
                has_encrypted_package = true;
                break;
            }
            if name.eq_ignore_ascii_case(required) {
                has_required_entry = true;
            }
        }
    }

    if has_encrypted_package {
        return Err(OfficePreviewError::new(
            "password_encrypted",
            "该文档已设置密码加密，暂不支持应用内预览，请使用外部办公软件打开",
        ));
    }

    if !has_required_entry {
        return Err(OfficePreviewError::new(
            "corrupt_file",
            format!("文档缺少核心部件 {}，文件可能已损坏或扩展名不匹配", required),
        ));
    }

    Ok(format)
}

/// 宿主要求的最低插件版本。查看器与宿主通过 postMessage 协作，协议或查看器行为修复后
/// 提高此值，已安装的旧版会在下次预览时自动更新。
pub const OFFICE_PLUGIN_REQUIRED_VERSION: &str = "0.1.1";
/// 选区、手动编辑与 AI 修改需要的插件版本；只预览时不强制更新，用到这些功能时再更新。
pub const OFFICE_PLUGIN_EDIT_VERSION: &str = "0.2.0";

/// `a.b.c` 数值比较；无法解析的版本视为过旧。
pub fn office_plugin_version_satisfies(installed: &str, required: &str) -> bool {
    let parse = |raw: &str| -> Option<(u64, u64, u64)> {
        let core = raw.trim().trim_start_matches('v').split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        Some((parts.next()??, parts.next()??, parts.next()??))
    };
    match (parse(installed), parse(required)) {
        (Some(have), Some(need)) => have >= need,
        _ => false,
    }
}

/// 串行化安装：多个预览面板同时触发时只下载一次。
static OFFICE_PLUGIN_INSTALL_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 官方插件目录不可用时的兜底下载地址（插件发布到 GitHub Releases，避开 Cloudflare 单文件上限）。
const OFFICE_PLUGIN_FALLBACK_URL: &str =
    "https://github.com/xieisabug/AIPP/releases/download/plugin-office-preview-plugin-v0.2.0/office-preview-plugin-0.2.0.aipp-plugin.zip";

/// 插件运行时需要的文件；本地开发安装只复制这些，不带 node_modules / 源码。
const OFFICE_PLUGIN_RUNTIME_ENTRIES: &[&str] = &["plugin.json", "dist", "assets"];

/// 仅调试构建：仓库内已构建的插件目录，便于开发时不走网络安装。
fn find_local_dev_plugin_dir() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("plugin")
        .join(OFFICE_PREVIEW_PLUGIN_CODE);
    let ready = candidate.join("plugin.json").is_file()
        && candidate.join("dist").join("viewer").join("index.html").is_file();
    ready.then(|| dunce::canonicalize(&candidate).unwrap_or(candidate))
}

fn plugin_root_for<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{}", e))?
        .join("plugin"))
}

fn office_viewer_root(plugin_root: &Path) -> PathBuf {
    plugin_root.join(OFFICE_PREVIEW_PLUGIN_CODE).join("dist").join("viewer")
}

fn emit_office_install_progress(
    app_handle: &AppHandle,
    stage: &str,
    percent: Option<u8>,
    message: impl Into<String>,
) {
    let _ = app_handle.emit(
        "plugin_install_progress",
        PluginInstallProgressEvent {
            plugin_code: OFFICE_PREVIEW_PLUGIN_CODE.to_string(),
            stage: stage.to_string(),
            downloaded_bytes: 0,
            total_bytes: None,
            percent,
            message: message.into(),
        },
    );
}

fn find_office_plugin_record(
    db: &PluginDatabase,
) -> Result<Option<crate::db::plugin_db::Plugin>, String> {
    Ok(db
        .get_plugins()
        .map_err(|e| format!("读取插件列表失败：{}", e))?
        .into_iter()
        .find(|p| p.folder_name == OFFICE_PREVIEW_PLUGIN_CODE))
}

fn read_office_plugin_status(app_handle: &AppHandle) -> Result<OfficePreviewPluginStatus, String> {
    let plugin_root = get_plugin_root_path(app_handle)?;
    let viewer_ready = office_viewer_root(&plugin_root).join("index.html").is_file();

    let db = PluginDatabase::new(app_handle).map_err(|e| format!("打开插件数据库失败：{}", e))?;
    sync_registry(&db, app_handle)?;
    let record = find_office_plugin_record(&db)?;
    let is_active = match &record {
        Some(plugin) => db
            .get_plugin_status(plugin.plugin_id)
            .map_err(|e| format!("读取插件启用状态失败：{}", e))?
            .map(|status| status.is_active)
            .unwrap_or(false),
        None => false,
    };

    let installed = record.is_some() && viewer_ready;
    let version = record.map(|r| r.version);
    let update_required = installed
        && !version
            .as_deref()
            .is_some_and(|v| office_plugin_version_satisfies(v, OFFICE_PLUGIN_REQUIRED_VERSION));
    let edit_supported = installed
        && version
            .as_deref()
            .is_some_and(|v| office_plugin_version_satisfies(v, OFFICE_PLUGIN_EDIT_VERSION));
    Ok(OfficePreviewPluginStatus {
        plugin_code: OFFICE_PREVIEW_PLUGIN_CODE.to_string(),
        installed,
        enabled: is_active && viewer_ready,
        viewer_ready,
        version,
        local_dev_available: find_local_dev_plugin_dir().is_some(),
        update_required,
        edit_supported,
    })
}

fn set_office_plugin_active(app_handle: &AppHandle, reason: &str) -> Result<(), String> {
    let db = PluginDatabase::new(app_handle).map_err(|e| format!("打开插件数据库失败：{}", e))?;
    sync_registry(&db, app_handle)?;
    let plugin = find_office_plugin_record(&db)?
        .ok_or_else(|| "插件目录已写入，但插件注册表中找不到 office-preview-plugin".to_string())?;
    db.upsert_plugin_status(plugin.plugin_id, true, None)
        .map_err(|e| format!("启用 Office 预览插件失败：{}", e))?;
    emit_plugin_registry_changed(app_handle, reason);
    Ok(())
}

/// 把本地构建目录中的运行时文件暂存后原子安装（不复制 node_modules 与源码）。
fn install_office_plugin_from_local_dir(dev_dir: &Path, plugin_root: &Path) -> Result<(), String> {
    fs::create_dir_all(plugin_root).map_err(|e| format!("创建插件目录失败：{}", e))?;
    let stage_root = plugin_root.join(format!(".office-dev-stage-{}", uuid::Uuid::new_v4()));
    let stage_dir = stage_root.join(OFFICE_PREVIEW_PLUGIN_CODE);
    let result = (|| {
        fs::create_dir_all(&stage_dir).map_err(|e| format!("创建暂存目录失败：{}", e))?;
        for entry in OFFICE_PLUGIN_RUNTIME_ENTRIES {
            let src = dev_dir.join(entry);
            let dst = stage_dir.join(entry);
            if src.is_dir() {
                crate::skills::installer::copy_dir_recursive(&src, &dst)?;
            } else if src.is_file() {
                fs::copy(&src, &dst).map_err(|e| format!("复制 {} 失败：{}", entry, e))?;
            }
        }
        install_plugin_dir_atomic(&stage_dir, plugin_root, OFFICE_PREVIEW_PLUGIN_CODE)
    })();
    let _ = fs::remove_dir_all(&stage_root);
    result
}

async fn has_configured_network_proxy(app_handle: &AppHandle) -> bool {
    let Some(state) = app_handle.try_state::<crate::FeatureConfigState>() else {
        return false;
    };
    let map = state.config_feature_map.lock().await;
    crate::api::ai::config::get_network_proxy_from_config(&map).is_some()
}

/// 官方 zip 顶层是单个 `<code>/` 目录，安装器会自动剥掉这一层，所以 from 用 "."。
fn default_office_plugin_dirs() -> Vec<PluginInstallRecipeDir> {
    vec![PluginInstallRecipeDir {
        from: ".".to_string(),
        to: OFFICE_PREVIEW_PLUGIN_CODE.to_string(),
    }]
}

async fn download_office_plugin(app_handle: &AppHandle, use_proxy: bool) -> Result<(), String> {
    emit_office_install_progress(app_handle, "downloading", None, "正在获取官方插件下载地址...");
    // 官方目录不可达时用固定发布地址兜底；目录给出的 sha256 会被强制校验
    let official = fetch_official_plugins(app_handle.clone(), use_proxy)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.code == OFFICE_PREVIEW_PLUGIN_CODE || p.id == OFFICE_PREVIEW_PLUGIN_CODE);

    let (source, dirs, expected_sha256) = match official {
        Some(entry) => {
            let source = entry
                .source
                .ok_or_else(|| "官方插件目录中的 office-preview-plugin 缺少下载地址".to_string())?;
            let dirs = if entry.dirs.is_empty() { default_office_plugin_dirs() } else { entry.dirs };
            (source, dirs, entry.sha256)
        }
        None => (
            PluginInstallRecipeSource {
                source_type: PluginInstallRecipeSourceType::Zip,
                repo: None,
                git_ref: "main".to_string(),
                url: Some(OFFICE_PLUGIN_FALLBACK_URL.to_string()),
                path: None,
            },
            default_office_plugin_dirs(),
            None,
        ),
    };

    install_plugin_archive_source(app_handle.clone(), source, dirs, expected_sha256, use_proxy, true)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn get_office_preview_plugin_status(
    app_handle: AppHandle,
) -> Result<OfficePreviewPluginStatus, String> {
    read_office_plugin_status(&app_handle)
}

/// 下载并安装 Office 预览插件；已安装时直接启用，不重复下载。
/// `use_proxy` 省略时：配置了网络代理就走代理。`prefer_local_dev` 省略时：调试构建优先用仓库内构建产物。
/// `require_edit` 为 true 时（选区、编辑、AI 修改）旧版插件也会更新到支持编辑的版本。
#[tauri::command]
pub async fn install_office_preview_plugin(
    app_handle: AppHandle,
    use_proxy: Option<bool>,
    prefer_local_dev: Option<bool>,
    require_edit: Option<bool>,
) -> Result<OfficePreviewPluginStatus, String> {
    let _guard = OFFICE_PLUGIN_INSTALL_LOCK.lock().await;
    let require_edit = require_edit.unwrap_or(false);

    let current = read_office_plugin_status(&app_handle)?;
    if current.installed && !current.update_required && (!require_edit || current.edit_supported) {
        if !current.enabled {
            set_office_plugin_active(&app_handle, "office-plugin-enabled")?;
        }
        return read_office_plugin_status(&app_handle);
    }

    let result = async {
        let local_dev = find_local_dev_plugin_dir().filter(|_| prefer_local_dev.unwrap_or(true));
        if let Some(dev_dir) = local_dev {
            emit_office_install_progress(
                &app_handle,
                "installing",
                Some(50),
                "正在从本地构建目录安装 Office 预览组件...",
            );
            let plugin_root = get_plugin_root_path(&app_handle)?;
            install_office_plugin_from_local_dir(&dev_dir, &plugin_root)?;
        } else {
            let use_proxy = match use_proxy {
                Some(flag) => flag,
                None => has_configured_network_proxy(&app_handle).await,
            };
            download_office_plugin(&app_handle, use_proxy).await?;
        }
        set_office_plugin_active(&app_handle, "office-plugin-installed")?;
        let status = read_office_plugin_status(&app_handle)?;
        if !status.viewer_ready {
            return Err(
                "插件已安装，但缺少查看器文件 dist/viewer/index.html，插件包可能不完整".to_string(),
            );
        }
        if status.update_required || (require_edit && !status.edit_supported) {
            return Err(format!(
                "已安装的 Office 预览插件版本 {} 低于所需的 {}，下载源可能尚未发布新版",
                status.version.as_deref().unwrap_or("未知"),
                if require_edit { OFFICE_PLUGIN_EDIT_VERSION } else { OFFICE_PLUGIN_REQUIRED_VERSION }
            ));
        }
        Ok(status)
    }
    .await;

    match &result {
        Ok(_) => emit_office_install_progress(&app_handle, "done", Some(100), "Office 预览组件安装完成"),
        Err(e) => emit_office_install_progress(&app_handle, "error", None, format!("安装失败：{}", e)),
    }
    result
}

#[tauri::command]
pub async fn enable_office_preview_plugin(
    app_handle: AppHandle,
) -> Result<OfficePreviewPluginStatus, String> {
    let current = read_office_plugin_status(&app_handle)?;
    if !current.installed {
        return Err("Office 预览组件尚未安装".to_string());
    }
    set_office_plugin_active(&app_handle, "office-plugin-enabled")?;
    read_office_plugin_status(&app_handle)
}

pub(crate) fn resolve_authorized_office_path(
    app_handle: &AppHandle,
    source: &OfficePreviewSource,
) -> Result<(PathBuf, String), OfficePreviewError> {
    // 0. By office_file_ref id（用户交给助手的文件、AI 生成的副本）
    if let Some(ref_id) = source.ref_id.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        let conn = ConversationDatabase::new(app_handle)
            .and_then(|db| db.get_connection())
            .map_err(|e| OfficePreviewError::new("read_error", format!("打开会话数据库失败：{}", e)))?;
        let found = crate::db::office_refs::get(&conn, ref_id)
            .map_err(|e| OfficePreviewError::new("read_error", e.to_string()))?
            .ok_or_else(|| OfficePreviewError::new("file_not_found", "找不到这个文件记录（所在会话可能已删除）"))?;
        return Ok((PathBuf::from(found.file_path), found.file_name));
    }

    // 1. By attachment_id in ConversationDatabase
    if let Some(attachment_id) = source.attachment_id {
        let db = ConversationDatabase::new(app_handle).map_err(|e| {
            OfficePreviewError::new("read_error", format!("打开会话数据库失败：{}", e))
        })?;
        let attachment = db
            .attachment_repo()
            .map_err(|e| OfficePreviewError::new("read_error", e.to_string()))?
            .read(attachment_id)
            .map_err(|e| OfficePreviewError::new("read_error", e.to_string()))?
            .ok_or_else(|| {
                OfficePreviewError::new(
                    "file_not_found",
                    format!("找不到附件记录（ID={}）", attachment_id),
                )
            })?;

        let raw_url = attachment.attachment_url.ok_or_else(|| {
            OfficePreviewError::new("file_not_found", "附件记录缺少文件路径")
        })?;
        let candidate = PathBuf::from(&raw_url);
        let resolved = if candidate.is_file() {
            candidate
        } else {
            let rel = Path::new("attachments").join(&raw_url);
            if rel.is_file() {
                rel
            } else {
                candidate
            }
        };
        #[cfg(mobile)]
        crate::mobile_files::require_attachment_path(app_handle, &resolved)
            .map_err(|e| OfficePreviewError::new("permission_denied", e.to_string()))?;

        let file_name = resolved
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();
        return Ok((resolved, file_name));
    }

    // 2. By preview_file relay_token in PreviewFileRelayState
    if let Some(token) = source.relay_token.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let relay_state = app_handle
            .try_state::<PreviewFileRelayState>()
            .ok_or_else(|| {
                OfficePreviewError::new("read_error", "预览中继状态不可用")
            })?;
        let Some((path, _file_type)) = relay_state
            .get_authorized_file_path(token)
            .map_err(|e| OfficePreviewError::new("read_error", e))?
        else {
            return Err(OfficePreviewError::new(
                "file_not_found",
                "预览链接已过期（本地文件预览链接 10 分钟有效），请让助手重新预览该文件",
            ));
        };
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("document")
            .to_string();
        return Ok((path, file_name));
    }

    // 3. By local_path — strictly verify it belongs to an authorized attachment or mobile import directory!
    if let Some(raw_path) = source.local_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(raw_path);
        #[cfg(mobile)]
        {
            crate::mobile_files::require_attachment_path(app_handle, &path)
                .map_err(|e| OfficePreviewError::new("permission_denied", e.to_string()))?;
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("document")
                .to_string();
            return Ok((path, file_name));
        }
        #[cfg(not(mobile))]
        {
            // Verify the path matches an existing attachment_url in ConversationDatabase
            let db = ConversationDatabase::new(app_handle).map_err(|e| {
                OfficePreviewError::new("read_error", format!("打开会话数据库失败：{}", e))
            })?;
            let conn = db
                .get_connection()
                .map_err(|e| OfficePreviewError::new("read_error", e.to_string()))?;
            let norm = raw_path.replace('\\', "/");
            let mut stmt = conn
                .prepare(
                    "SELECT COUNT(1) FROM message_attachment WHERE REPLACE(attachment_url, '\\', '/') = ?1 OR attachment_url = ?2",
                )
                .map_err(|e| OfficePreviewError::new("read_error", e.to_string()))?;
            let count: i64 = stmt
                .query_row(rusqlite::params![norm, raw_path], |row| row.get(0))
                .unwrap_or(0);
            if count <= 0 {
                return Err(OfficePreviewError::new(
                    "permission_denied",
                    "未授权访问该本地文件路径，仅允许预览会话已授权附件或工具显式登记的文件",
                ));
            }
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("document")
                .to_string();
            return Ok((path, file_name));
        }
    }

    Err(OfficePreviewError::new(
        "invalid_request",
        "未提供有效的文件来源标识（attachmentId / relayToken / localPath）",
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeFileHeader {
    pub file_name: String,
    pub format: String,
    pub mime: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// 二进制响应格式：`[u32 LE 头长度][UTF-8 JSON 头][文件字节]`。
/// 走 Tauri 原始字节 IPC，避免 base64 膨胀 1/3 并在前端主线程再解码一遍。
pub fn encode_office_file_response(header: &OfficeFileHeader, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let header_json =
        serde_json::to_vec(header).map_err(|e| format!("序列化文件头失败：{}", e))?;
    let header_len = u32::try_from(header_json.len()).map_err(|_| "文件头过长".to_string())?;
    let mut body = Vec::with_capacity(4 + header_json.len() + bytes.len());
    body.extend_from_slice(&header_len.to_le_bytes());
    body.extend_from_slice(&header_json);
    body.extend_from_slice(bytes);
    Ok(body)
}

pub(crate) fn office_preview_max_bytes() -> u64 {
    if cfg!(mobile) {
        MOBILE_MAX_OFFICE_BYTES
    } else {
        DESKTOP_MAX_OFFICE_BYTES
    }
}

fn file_too_large_error(actual: u64, max_bytes: u64) -> OfficePreviewError {
    OfficePreviewError::new(
        "file_too_large",
        format!(
            "文件大小（{:.1} MiB）超过当前平台预览上限（{} MiB），请使用外部办公软件打开",
            (actual as f64) / (1024.0 * 1024.0),
            max_bytes / (1024 * 1024)
        ),
    )
}

/// 读取、校验并编码已授权的 Office 文件（阻塞 IO，调用方放到 blocking 线程）。
fn load_office_file_blocking(file_path: &Path, file_name: &str) -> Result<Vec<u8>, OfficePreviewError> {
    if file_path.is_dir() {
        return Err(OfficePreviewError::new("invalid_request", "目标路径是目录而非文件"));
    }
    let max_bytes = office_preview_max_bytes();
    let metadata = fs::metadata(file_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            OfficePreviewError::new("file_not_found", format!("找不到文件：{}", file_name))
        } else {
            OfficePreviewError::new("read_error", format!("读取文件信息失败：{}", e))
        }
    })?;
    // 先按元数据拦截超限文件，避免整个读进内存
    if metadata.len() > max_bytes {
        return Err(file_too_large_error(metadata.len(), max_bytes));
    }
    let bytes = fs::read(file_path)
        .map_err(|e| OfficePreviewError::new("read_error", format!("读取文件失败：{}", e)))?;
    let format = validate_office_bytes(file_name, &bytes, max_bytes)?;
    let header = OfficeFileHeader {
        file_name: file_name.to_string(),
        format: format.as_str().to_string(),
        mime: format.mime().to_string(),
        size_bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(&bytes)),
    };
    encode_office_file_response(&header, &bytes)
        .map_err(|e| OfficePreviewError::new("read_error", e))
}

/// 只读读取已授权的 Office 文件；不写源文件，不扩大访问范围。
#[tauri::command]
pub async fn read_authorized_office_file(
    app_handle: AppHandle,
    source: OfficePreviewSource,
) -> Result<tauri::ipc::Response, String> {
    let (file_path, file_name) =
        resolve_authorized_office_path(&app_handle, &source).map_err(|e| e.to_wire_string())?;
    let body = tokio::task::spawn_blocking(move || load_office_file_blocking(&file_path, &file_name))
        .await
        .map_err(|e| {
            OfficePreviewError::new("read_error", format!("读取线程异常退出：{}", e)).to_wire_string()
        })?
        .map_err(|e| e.to_wire_string())?;
    Ok(tauri::ipc::Response::new(body))
}

#[tauri::command]
pub async fn open_office_file_externally(
    app_handle: AppHandle,
    source: OfficePreviewSource,
) -> Result<(), String> {
    let (file_path, file_name) =
        resolve_authorized_office_path(&app_handle, &source).map_err(|e| e.message)?;

    if !file_path.is_file() {
        return Err(format!("找不到文件：{}", file_name));
    }

    #[cfg(target_os = "android")]
    {
        let format = classify_office_extension(&file_name).map_err(|e| e.message)?;
        let bytes = fs::read(&file_path).map_err(|e| format!("读取文件失败：{}", e))?;
        let data = STANDARD.encode(&bytes);
        crate::mobile_files::mobile_file_action(
            app_handle,
            "open".into(),
            serde_json::json!({
                "name": file_name,
                "mime": format.mime(),
                "data": data,
            }),
        )
        .await?;
        return Ok(());
    }

    #[cfg(not(target_os = "android"))]
    {
        let path_str = file_path
            .to_str()
            .ok_or_else(|| "文件路径包含无效字符".to_string())?;
        app_handle
            .opener()
            .open_path(path_str, None::<&str>)
            .map_err(|e| format!("调用外部应用打开失败：{}", e))?;
        Ok(())
    }
}

fn detect_viewer_asset_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn sanitize_relative_viewer_path(uri_path: &str) -> Option<PathBuf> {
    let trimmed = uri_path.trim_start_matches('/');
    let rel_str = if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    };
    let rel = Path::new(rel_str);
    let mut clean = PathBuf::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => clean.push(seg),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        clean.push("index.html");
    }
    Some(clean)
}

pub fn handle_office_preview_protocol_request<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let build_err = |status: StatusCode, msg: &str| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(msg.as_bytes().to_vec())
            .unwrap_or_else(|_| Response::new(msg.as_bytes().to_vec()))
    };

    if request.method() != tauri::http::Method::GET && request.method() != tauri::http::Method::HEAD
    {
        return build_err(StatusCode::METHOD_NOT_ALLOWED, "Only GET/HEAD allowed");
    }

    let Some(rel_path) = sanitize_relative_viewer_path(request.uri().path()) else {
        return build_err(StatusCode::BAD_REQUEST, "Invalid viewer asset path");
    };

    let Ok(plugin_root) = plugin_root_for(ctx.app_handle()) else {
        return build_err(StatusCode::INTERNAL_SERVER_ERROR, "Plugin root unavailable");
    };

    let viewer_root = office_viewer_root(&plugin_root);

    let target = viewer_root.join(&rel_path);
    let Ok(canon_root) = dunce::canonicalize(&viewer_root) else {
        return build_err(
            StatusCode::NOT_FOUND,
            "Office preview plugin is not installed",
        );
    };
    let Ok(canon_target) = dunce::canonicalize(&target) else {
        return build_err(StatusCode::NOT_FOUND, "Viewer asset not found");
    };
    if !canon_target.starts_with(&canon_root) || !canon_target.is_file() {
        return build_err(StatusCode::FORBIDDEN, "Access outside viewer root denied");
    }

    let Ok(bytes) = fs::read(&canon_target) else {
        return build_err(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read viewer asset");
    };

    let mime = detect_viewer_asset_mime(&canon_target);
    let cache_ctrl = if canon_target.extension().and_then(|e| e.to_str()) == Some("html") {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, cache_ctrl)
        .header(
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'; worker-src 'self' blob:; connect-src 'self'; font-src 'self' data:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:;",
        )
        .body(bytes)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
#[path = "office_api_tests.rs"]
mod tests;

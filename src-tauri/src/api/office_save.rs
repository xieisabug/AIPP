//! Office 副本另存（#331）与文件引用（#330）。
//!
//! 默认另存为新文件：先写同目录临时文件并落盘，再重命名为目标；不覆盖时先以 create_new 占住目标名，
//! 目标已存在直接拒绝（由用户二次确认后才带 overwrite 重试）。任何失败都删除临时文件，原文件与已有
//! 目标保持原样，不会把残缺文件当成功结果。
use crate::api::office_api::{
    classify_office_extension, office_preview_max_bytes, resolve_authorized_office_path, validate_office_bytes,
    OfficeFormat, OfficePreviewError, OfficePreviewSource,
};
use crate::db::conversation_db::ConversationDatabase;
use crate::db::office_refs::{self, OfficeFileRef};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

pub const COPY_SUFFIX: &str = "副本";
pub const AI_SUFFIX: &str = "AI 修改";
const OUTPUT_DIR: &str = "office_outputs";

fn err(code: &str, message: impl Into<String>) -> OfficePreviewError {
    OfficePreviewError::new(code, message)
}

/// `报告.docx` + `副本` → `报告（副本）.docx`；`n > 1` 时为 `报告（副本 2）.docx`。
pub fn copy_file_name(original_name: &str, suffix: &str, n: u32) -> String {
    let path = Path::new(original_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("document");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let label = if n > 1 { format!("{suffix} {n}") } else { suffix.to_string() };
    if ext.is_empty() {
        format!("{stem}（{label}）")
    } else {
        format!("{stem}（{label}）.{ext}")
    }
}

/// 目录里第一个不冲突的副本名。
pub fn unique_copy_path(dir: &Path, original_name: &str, suffix: &str) -> PathBuf {
    for n in 1..10_000 {
        let candidate = dir.join(copy_file_name(original_name, suffix, n));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{}-{}", uuid::Uuid::new_v4().simple(), copy_file_name(original_name, suffix, 1)))
}

/// 用户给的文件名只保留最后一段、去掉 Windows 不允许的字符，并强制使用原格式扩展名。
pub fn sanitize_output_name(requested: Option<&str>, original_name: &str, format: OfficeFormat) -> String {
    let fallback = copy_file_name(original_name, AI_SUFFIX, 1);
    let Some(raw) = requested.map(str::trim).filter(|s| !s.is_empty()) else { return fallback };
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        return fallback;
    }
    let stem = match Path::new(&cleaned).extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case(format.as_str()) => {
            Path::new(&cleaned).file_stem().and_then(|s| s.to_str()).unwrap_or(&cleaned).to_string()
        }
        _ => cleaned.clone(),
    };
    let stem: String = stem.chars().take(120).collect();
    format!("{}.{}", stem, format.as_str())
}

fn is_disk_full(e: &std::io::Error) -> bool {
    // Windows: ERROR_HANDLE_DISK_FULL(39) / ERROR_DISK_FULL(112)；Unix: ENOSPC(28)
    matches!(e.raw_os_error(), Some(39) | Some(112)) || (cfg!(unix) && e.raw_os_error() == Some(28))
}

fn io_error(stage: &str, target: &Path, e: &std::io::Error) -> OfficePreviewError {
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("目标文件");
    if is_disk_full(e) {
        return err("disk_full", format!("{stage}失败：磁盘空间不足，无法写入「{name}」（{e}）"));
    }
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => err(
            "permission_denied",
            format!("{stage}失败：没有写入「{name}」的权限，或文件正被其他程序（如 Office/WPS）占用（{e}）"),
        ),
        std::io::ErrorKind::NotFound => err("target_dir_missing", format!("{stage}失败：目标位置已不存在（{e}）")),
        _ => err("write_failed", format!("{stage}失败：{e}")),
    }
}

#[derive(Debug)]
struct ProtectedCopySource {
    path: PathBuf,
    opened_path: PathBuf,
    opened_handle: same_file::Handle,
}

impl ProtectedCopySource {
    fn from_open_file(path: PathBuf, opened_file: fs::File) -> Result<Self, OfficePreviewError> {
        let origin_err = |e| io_error("绑定已打开的原文件", &path, &e);
        let opened_path = comparable_path(&path).map_err(origin_err)?.unwrap_or_else(|| path.clone());
        let opened_handle = same_file::Handle::from_file(opened_file).map_err(origin_err)?;
        Ok(Self { path, opened_path, opened_handle })
    }
}

const EDIT_COPY_ORIGIN_LIMIT: usize = 32;

/// Save-only origin bindings for already-opened editor documents. Never used to read file bytes.
/// Bounded across windows and kept for the process lifetime, independently of read-relay expiry.
#[derive(Default)]
pub struct OfficeEditCopyOrigins(Mutex<VecDeque<EditCopyOrigin>>);

struct EditCopyOrigin {
    token: String,
    window: String,
    descriptor: OfficePreviewSource,
    source: Arc<ProtectedCopySource>,
}

impl OfficeEditCopyOrigins {
    pub(crate) fn remember(
        &self,
        window: &str,
        descriptor: &OfficePreviewSource,
        path: PathBuf,
        opened_file: fs::File,
    ) -> Result<String, OfficePreviewError> {
        let source = Arc::new(ProtectedCopySource::from_open_file(path, opened_file)?);
        let mut slots = self.0.lock().map_err(|_| err("read_error", "编辑来源记录暂不可用"))?;
        if slots.len() >= EDIT_COPY_ORIGIN_LIMIT {
            slots.pop_front();
        }
        let token = uuid::Uuid::new_v4().to_string();
        slots.push_back(EditCopyOrigin {
            token: token.clone(),
            window: window.to_string(),
            descriptor: descriptor.clone(),
            source,
        });
        Ok(token)
    }

    fn get(&self, window: &str, descriptor: &OfficePreviewSource, token: Option<&str>) -> Result<Arc<ProtectedCopySource>, OfficePreviewError> {
        let slots = self.0.lock().map_err(|_| err("read_error", "编辑来源记录暂不可用"))?;
        slots.iter().find_map(|origin| {
            (token == Some(origin.token.as_str()) && origin.window == window && &origin.descriptor == descriptor)
                .then(|| Arc::clone(&origin.source))
        }).ok_or_else(|| err("file_not_found", "未找到本窗口已打开的编辑来源，请重新打开原文档"))
    }

    fn release(&self, window: &str, token: &str) {
        if let Ok(mut slots) = self.0.lock() {
            slots.retain(|origin| origin.window != window || origin.token != token);
        }
    }

    pub(crate) fn release_window(&self, window: &str) {
        if let Ok(mut slots) = self.0.lock() {
            slots.retain(|origin| origin.window != window);
        }
    }
}

/// Resolve existing aliases, or compare the directory and leaf if the file was removed.
fn comparable_path(path: &Path) -> std::io::Result<Option<PathBuf>> {
    match dunce::canonicalize(path) {
        Ok(path) => Ok(Some(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let Some(name) = path.file_name() else { return Ok(None) };
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            match dunce::canonicalize(parent) {
                Ok(parent) => Ok(Some(parent.join(name))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

fn protect_copy_source(source: &ProtectedCopySource, target: &Path) -> Result<(), OfficePreviewError> {
    let protected = || err("protected_source", "所选位置是原文件，请重新选择位置");
    if source.path == target {
        return Err(protected());
    }
    let check_err = |e| io_error("核对保存位置", target, &e);
    let target_path = comparable_path(target).map_err(check_err)?;
    if target_path.as_ref() == Some(&source.opened_path) {
        return Err(protected());
    }
    match same_file::Handle::from_path(target) {
        Ok(handle) if handle == source.opened_handle => return Err(protected()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(check_err(e)),
    }
    let source_path = comparable_path(&source.path).map_err(check_err)?;
    if source_path.is_some() && source_path == target_path {
        return Err(protected());
    }
    // Canonicalization detects symlinks; file identity also detects hard links and
    // case/short-name aliases on filesystems supporting them.
    match same_file::is_same_file(&source.path, target) {
        Ok(true) => Err(protected()),
        Ok(false) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(check_err(e)),
    }
}

/// 安全写入 Office 文件；失败时目标与原文件保持原样、不留临时文件。
pub fn write_office_file_safely(target: &Path, bytes: &[u8], overwrite: bool) -> Result<(), OfficePreviewError> {
    write_office_file(target, bytes, overwrite, None)
}

fn write_office_file(
    target: &Path,
    bytes: &[u8],
    overwrite: bool,
    protected_source: Option<&ProtectedCopySource>,
) -> Result<(), OfficePreviewError> {
    if let Some(source) = protected_source {
        protect_copy_source(source, target)?;
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| err("invalid_target", "目标路径缺少所在目录"))?;
    if !parent.is_dir() {
        return Err(err("target_dir_missing", format!("目标目录不存在：{}", parent.display())));
    }
    if target.is_dir() {
        return Err(err("invalid_target", "目标路径是一个目录"));
    }
    if !overwrite && target.exists() {
        return Err(err(
            "target_exists",
            format!("「{}」已存在，未覆盖", target.file_name().and_then(|n| n.to_str()).unwrap_or("目标文件")),
        ));
    }

    let file_name = target.file_name().and_then(|n| n.to_str()).unwrap_or("document");
    let temp = parent.join(format!(".{}.aipp-{}.tmp", file_name, &uuid::Uuid::new_v4().simple().to_string()[..8]));
    let write_temp = || -> Result<(), OfficePreviewError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| io_error("创建临时文件", target, &e))?;
        file.write_all(bytes).map_err(|e| io_error("写入", target, &e))?;
        file.sync_all().map_err(|e| io_error("落盘", target, &e))?;
        Ok(())
    };
    if let Err(e) = write_temp() {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }

    // 不覆盖：先以 create_new 占住目标名，避免检查与重命名之间被别人写入同名文件后被替换
    let mut placeholder = false;
    if !overwrite {
        match fs::OpenOptions::new().write(true).create_new(true).open(target) {
            Ok(_) => placeholder = true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temp);
                return Err(err("target_exists", format!("「{file_name}」已存在，未覆盖")));
            }
            Err(e) => {
                let _ = fs::remove_file(&temp);
                return Err(io_error("创建目标文件", target, &e));
            }
        }
    }
    if overwrite {
        if let Some(source) = protected_source {
            if let Err(e) = protect_copy_source(source, target) {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
        }
    }
    if let Err(e) = fs::rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        if placeholder {
            let _ = fs::remove_file(target);
        }
        return Err(io_error("保存", target, &e));
    }

    match fs::metadata(target) {
        Ok(meta) if meta.len() == bytes.len() as u64 => Ok(()),
        Ok(meta) => Err(err(
            "write_failed",
            format!("保存后校验失败：文件大小 {} 字节，应为 {} 字节", meta.len(), bytes.len()),
        )),
        Err(e) => Err(io_error("保存后校验", target, &e)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeSaveResult {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub format: String,
}

fn check_target_format(target: &Path, content_format: OfficeFormat) -> Result<(), OfficePreviewError> {
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let target_format = classify_office_extension(name)
        .map_err(|_| err("invalid_target", format!("另存的文件名必须以 .{} 结尾", content_format.as_str())))?;
    if target_format != content_format {
        return Err(err(
            "invalid_target",
            format!("内容是 .{} 文档，不能另存为 .{}", content_format.as_str(), target_format.as_str()),
        ));
    }
    Ok(())
}

fn save_bytes(
    target: &Path,
    bytes: &[u8],
    overwrite: bool,
    content_format: OfficeFormat,
    protected_source: Option<&ProtectedCopySource>,
) -> Result<OfficeSaveResult, OfficePreviewError> {
    check_target_format(target, content_format)?;
    write_office_file(target, bytes, overwrite, protected_source)?;
    Ok(OfficeSaveResult {
        path: target.to_string_lossy().to_string(),
        file_name: target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string(),
        size_bytes: bytes.len() as u64,
        format: content_format.as_str().to_string(),
    })
}

/// 读取并校验已授权文件（阻塞 IO）。
pub(crate) fn read_validated(path: &Path, file_name: &str) -> Result<(Vec<u8>, OfficeFormat), OfficePreviewError> {
    let max = office_preview_max_bytes();
    let meta = fs::metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            err("file_not_found", format!("找不到文件：{file_name}"))
        } else {
            err("read_error", format!("读取文件信息失败：{e}"))
        }
    })?;
    if meta.len() > max {
        return Err(err(
            "file_too_large",
            format!("文件大小（{:.1} MiB）超过当前平台上限（{} MiB）", meta.len() as f64 / 1048576.0, max / 1048576),
        ));
    }
    let bytes = fs::read(path).map_err(|e| err("read_error", format!("读取文件失败：{e}")))?;
    let format = validate_office_bytes(file_name, &bytes, max)?;
    Ok((bytes, format))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, OfficePreviewError> {
    app.path().app_data_dir().map_err(|e| err("read_error", format!("无法定位应用数据目录：{e}")))
}

pub fn outputs_root(app: &AppHandle) -> Result<PathBuf, OfficePreviewError> {
    Ok(app_data_dir(app)?.join(OUTPUT_DIR))
}

/// 会话删除后清理它的 AI 修改副本（尽力而为）。
pub fn remove_conversation_outputs(app: &AppHandle, conversation_id: i64) {
    if let Ok(root) = outputs_root(app) {
        let dir = root.join(conversation_id.to_string());
        if dir.is_dir() {
            if let Err(e) = fs::remove_dir_all(&dir) {
                tracing::warn!(conversation_id, error = %e, "Failed to remove office outputs of deleted conversation");
            }
        }
    }
}

/// 另存对话框的默认位置：原文件旁边；原文件在应用数据目录里（附件、AI 副本）时改为下载目录。
fn default_save_dir(app: &AppHandle, candidates: &[Option<&Path>]) -> PathBuf {
    let app_dir = app_data_dir(app).ok().map(|p| dunce::canonicalize(&p).unwrap_or(p));
    for dir in candidates.iter().flatten() {
        if !dir.is_dir() {
            continue;
        }
        let canon = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        if app_dir.as_ref().is_some_and(|root| canon.starts_with(root)) {
            continue;
        }
        return canon;
    }
    app.path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn conversation_conn(app: &AppHandle) -> Result<rusqlite::Connection, OfficePreviewError> {
    ConversationDatabase::new(app)
        .and_then(|db| db.get_connection())
        .map_err(|e| err("read_error", format!("打开会话数据库失败：{e}")))
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeFileRefInfo {
    pub ref_id: String,
    pub file_name: String,
    pub format: String,
    pub sha256: String,
    pub size_bytes: i64,
}

/// 用户把预览中的选区交给助手：校验文件仍是预览时打开的那一份（指纹一致），登记引用。
#[tauri::command]
pub async fn register_office_selection_ref(
    app_handle: AppHandle,
    source: OfficePreviewSource,
    expected_sha256: String,
    conversation_id: Option<i64>,
) -> Result<OfficeFileRefInfo, String> {
    let (path, file_name) = resolve_authorized_office_path(&app_handle, &source).map_err(|e| e.to_wire_string())?;
    let read_path = path.clone();
    let read_name = file_name.clone();
    let (bytes, format) = tokio::task::spawn_blocking(move || read_validated(&read_path, &read_name))
        .await
        .map_err(|e| err("read_error", format!("读取线程异常退出：{e}")).to_wire_string())?
        .map_err(|e| e.to_wire_string())?;
    let sha256 = sha256_hex(&bytes);
    if !expected_sha256.trim().is_empty() && !sha256.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err(err(
            "stale_selection",
            format!("「{file_name}」在打开预览后已被修改，选区可能已经对不上，请重新加载后再选择"),
        )
        .to_wire_string());
    }
    let conn = conversation_conn(&app_handle).map_err(|e| e.to_wire_string())?;
    // 再次选中 AI 副本时沿用它的原始路径，另存时仍默认放在用户原文件旁边
    let origin_path = match source.ref_id.as_deref() {
        Some(id) => office_refs::get(&conn, id).ok().flatten().and_then(|r| r.origin_path),
        None => Some(path.to_string_lossy().to_string()),
    };
    let record = OfficeFileRef {
        id: office_refs::new_ref_id(),
        conversation_id,
        kind: office_refs::KIND_SELECTION.into(),
        file_path: path.to_string_lossy().to_string(),
        file_name: file_name.clone(),
        format: format.as_str().into(),
        sha256: sha256.clone(),
        size_bytes: bytes.len() as i64,
        origin_ref: source.ref_id.clone(),
        origin_path,
        created_at: now_secs(),
    };
    office_refs::insert(&conn, &record).map_err(|e| err("read_error", format!("登记文件引用失败：{e}")).to_wire_string())?;
    Ok(OfficeFileRefInfo {
        ref_id: record.id,
        file_name,
        format: record.format,
        sha256,
        size_bytes: record.size_bytes,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficeFileRefStatus {
    pub ref_id: String,
    pub kind: String,
    pub file_name: String,
    pub format: String,
    pub size_bytes: i64,
    pub sha256: String,
    /// 文件仍在且大小与登记时一致。
    pub available: bool,
    /// 不可用时的原因（文件已删除、被改动）。
    pub problem: Option<String>,
}

/// 成果卡查询文件状态：只按元数据判断，不读取内容。
#[tauri::command]
pub async fn get_office_file_ref_status(app_handle: AppHandle, ref_id: String) -> Result<OfficeFileRefStatus, String> {
    let conn = conversation_conn(&app_handle).map_err(|e| e.to_wire_string())?;
    let found = office_refs::get(&conn, ref_id.trim())
        .map_err(|e| err("read_error", e.to_string()).to_wire_string())?
        .ok_or_else(|| err("file_not_found", "找不到这个文件记录（所在会话可能已删除）").to_wire_string())?;
    let (available, problem) = match fs::metadata(&found.file_path) {
        Ok(meta) if meta.len() as i64 == found.size_bytes => (true, None),
        Ok(_) => (false, Some("文件在生成后被改动过".to_string())),
        Err(_) => (false, Some("文件已被删除或移动".to_string())),
    };
    Ok(OfficeFileRefStatus {
        ref_id: found.id,
        kind: found.kind,
        file_name: found.file_name,
        format: found.format,
        size_bytes: found.size_bytes,
        sha256: found.sha256,
        available,
        problem,
    })
}

/// 另存对话框的默认路径（不冲突的副本名）。
#[tauri::command]
pub async fn suggest_office_copy_path(
    app_handle: AppHandle,
    source: OfficePreviewSource,
    purpose: Option<String>,
) -> Result<String, String> {
    let (path, file_name) = resolve_authorized_office_path(&app_handle, &source).map_err(|e| e.to_wire_string())?;
    let origin = match source.ref_id.as_deref() {
        Some(id) => conversation_conn(&app_handle)
            .ok()
            .and_then(|conn| office_refs::get(&conn, id).ok().flatten())
            .and_then(|r| r.origin_path),
        None => None,
    };
    let origin_dir = origin.as_deref().map(Path::new).and_then(Path::parent).map(Path::to_path_buf);
    let dir = default_save_dir(&app_handle, &[origin_dir.as_deref(), path.parent()]);
    // AI 副本本身已带“（AI 修改）”，另存时沿用它的名字；手动编辑的副本加“（副本）”
    let (base, suffix) = match purpose.as_deref() {
        Some("output") => (file_name.clone(), None),
        _ => (file_name.clone(), Some(COPY_SUFFIX)),
    };
    let target = match suffix {
        Some(suffix) => unique_copy_path(&dir, &base, suffix),
        None if !dir.join(&base).exists() => dir.join(&base),
        None => unique_copy_path(&dir, &base, COPY_SUFFIX),
    };
    Ok(target.to_string_lossy().to_string())
}

/// 把已授权文件（如 AI 副本）另存到用户选择的位置。
#[tauri::command]
pub async fn office_save_copy(
    app_handle: AppHandle,
    source: OfficePreviewSource,
    target_path: String,
    overwrite: Option<bool>,
) -> Result<OfficeSaveResult, String> {
    let (path, file_name) = resolve_authorized_office_path(&app_handle, &source).map_err(|e| e.to_wire_string())?;
    let target = PathBuf::from(target_path.trim());
    let overwrite = overwrite.unwrap_or(false);
    tokio::task::spawn_blocking(move || -> Result<OfficeSaveResult, OfficePreviewError> {
        let (bytes, format) = read_validated(&path, &file_name)?;
        save_bytes(&target, &bytes, overwrite, format, None)
    })
    .await
    .map_err(|e| err("write_failed", format!("保存线程异常退出：{e}")).to_wire_string())?
    .map_err(|e| e.to_wire_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteCopyHeader {
    source: OfficePreviewSource,
    copy_origin_token: Option<String>,
    target_path: String,
    #[serde(default)]
    overwrite: bool,
}

/// 二进制请求：`[u32 LE 头长度][UTF-8 JSON 头][文件字节]`，与读取响应同格式，不经 base64。
pub fn decode_framed_request(body: &[u8]) -> Result<(serde_json::Value, &[u8]), OfficePreviewError> {
    if body.len() < 4 {
        return Err(err("invalid_request", "请求缺少头部"));
    }
    let header_len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    if 4 + header_len > body.len() {
        return Err(err("invalid_request", "请求头长度越界"));
    }
    let header = serde_json::from_slice(&body[4..4 + header_len])
        .map_err(|e| err("invalid_request", format!("请求头不是有效 JSON：{e}")))?;
    Ok((header, &body[4 + header_len..]))
}

fn write_edited_copy(
    source: &ProtectedCopySource,
    target: &Path,
    bytes: &[u8],
    overwrite: bool,
) -> Result<OfficeSaveResult, OfficePreviewError> {
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let format = validate_office_bytes(name, bytes, office_preview_max_bytes())?;
    save_bytes(target, bytes, overwrite, format, Some(source))
}

fn write_bound_edited_copy(
    origins: &OfficeEditCopyOrigins,
    window: &str,
    body: &[u8],
) -> Result<OfficeSaveResult, OfficePreviewError> {
    let (header, bytes) = decode_framed_request(body)?;
    let header: WriteCopyHeader =
        serde_json::from_value(header).map_err(|e| err("invalid_request", format!("请求头字段无效：{e}")))?;
    // A current path lookup cannot identify the opened file once it has moved.
    let source = origins.get(window, &header.source, header.copy_origin_token.as_deref())?;
    let target = Path::new(header.target_path.trim());
    write_edited_copy(&source, target, bytes, header.overwrite)
}

/// 手动编辑后的文档另存为副本（字节来自查看器）。
#[tauri::command]
pub async fn office_write_copy(
    app_handle: AppHandle,
    window: tauri::WebviewWindow,
    request: tauri::ipc::Request<'_>,
) -> Result<OfficeSaveResult, String> {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err(err("invalid_request", "需要二进制请求体").to_wire_string());
    };
    let body = body.clone();
    tokio::task::spawn_blocking(move || -> Result<OfficeSaveResult, OfficePreviewError> {
        write_bound_edited_copy(&app_handle.state::<OfficeEditCopyOrigins>(), window.label(), &body)
    })
    .await
    .map_err(|e| err("write_failed", format!("保存线程异常退出：{e}")).to_wire_string())?
    .map_err(|e| e.to_wire_string())
}

/// Invalidate the save-only handle when its editor load is discarded.
#[tauri::command]
pub fn release_office_copy_origin(app_handle: AppHandle, window: tauri::WebviewWindow, token: String) {
    app_handle.state::<OfficeEditCopyOrigins>().release(window.label(), &token);
}

#[cfg(test)]
#[path = "office_save_tests.rs"]
mod tests;

#[path = "attachment_office_text.rs"]
mod office_text;
pub(super) use office_text::extract as extract_office_text;
use crate::db::conversation_db::{AttachmentType, Repository};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use mime_guess::from_path;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::{
    db::conversation_db::{ConversationDatabase, MessageAttachment},
    errors::AppError,
};
use tracing::{debug, info};

#[derive(Serialize)]
pub struct AttachmentResult {
    attachment_id: i64,
    attachment_type: i64,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
}

#[tauri::command]
pub async fn add_attachment(
    app_handle: tauri::AppHandle,
    file_url: Option<String>,
    file_content: Option<String>,
    file_name: Option<String>,
    attachment_type: Option<i64>,
    file_data_url: Option<String>,
) -> Result<AttachmentResult, AppError> {
    info!(?file_url, ?file_name, "add_attachment called");
    // 如果有 URL，使用 add_attachment_by_url
    if let Some(url) = file_url {
        #[cfg(mobile)]
        {
            crate::mobile_files::require_attachment_path(&app_handle, Path::new(&url)).map_err(AppError::Anyhow)?;
            let result = add_attachment_by_url(app_handle, url.clone()).await;
            if result.is_err() {
                let _ = std::fs::remove_file(&url);
                let _ = std::fs::remove_file(format!("{url}.metadata.json"));
                if let Some(parent) = Path::new(&url).parent() { let _ = std::fs::remove_dir(parent); }
            }
            return result;
        }
        return add_attachment_by_url(app_handle, url).await;
    }

    if let (Some(data), Some(name)) = (file_data_url, file_name.as_deref()) {
        let name = name.to_string();
        return tauri::async_runtime::spawn_blocking(move || import_dropped_file(app_handle, &name, &data))
            .await.map_err(|e| AppError::Anyhow(format!("附件导入任务失败：{e}")))?;
    }

    // 如果有 content 和其他必要参数，使用 add_attachment_content
    if let (Some(content), Some(name), Some(att_type)) = (file_content, file_name, attachment_type)
    {
        return add_attachment_content(app_handle, content, name, att_type).await;
    }

    // 如果都没有提供有效参数，返回错误
    Err(AppError::Anyhow(
        "无效的参数：需要提供文件URL或者（文件内容、文件名、附件类型）".to_string(),
    ))
}

const INLINE_TEXT_BYTES: usize = 128 * 1024;

pub async fn add_attachment_by_url(
    app_handle: tauri::AppHandle,
    file_url: String,
) -> Result<AttachmentResult, AppError> {
    tauri::async_runtime::spawn_blocking(move || add_attachment_by_path(app_handle, file_url, None))
        .await.map_err(|e| AppError::Anyhow(format!("Attachment import worker: {e}")))?
}

fn add_attachment_by_path(
    app_handle: tauri::AppHandle,
    file_url: String,
    content_mime: Option<&str>,
) -> Result<AttachmentResult, AppError> {
    // 1. 解析文件路径
    let file_path = Path::new(&file_url).to_path_buf();
    #[cfg(mobile)]
    crate::mobile_files::require_attachment_path(&app_handle, &file_path)
        .map_err(AppError::Anyhow)?;

    let (attachment_type, content, hash) = inspect_attachment(&file_path, content_mime)?;
    let db = ConversationDatabase::new(&app_handle)?;
    register_attachment(&db, file_url, attachment_type, content, Some(hash))
}

fn inspect_attachment(file_path: &Path, content_mime: Option<&str>) -> Result<(AttachmentType, Option<String>, String), AppError> {
    if !file_path.is_absolute() || !file_path.is_file() {
        return Err(AppError::Anyhow(format!("找不到附件文件 {}", file_path.display())));
    }

    // 3. 解析文件类型
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let guessed = from_path(&file_path).first_or_octet_stream().to_string();
    let file_type = if ext.is_empty() { content_mime.unwrap_or(&guessed) } else { &guessed };
    let mut file_type_classify = String::new();

    debug!(file_type, ext, "detected file type");

    if matches!(ext.as_str(), "docx" | "xlsx" | "pptx") {
        file_type_classify = ext.clone();
    } else if file_type == "application/pdf" || ext == "pdf" {
        file_type_classify = "pdf".into();
    } else if file_type.starts_with("text/") || file_type == "application/json" || matches!(ext.as_str(),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "sql" | "toml" | "yaml" | "yml" | "log" | "md" | "jsonl" | "sh" | "ps1" | "xml") {
        file_type_classify = "text".to_string();
    } else if file_type.starts_with("image/") && image::ImageFormat::from_mime_type(file_type).or_else(|| image::ImageFormat::from_extension(&ext)).is_some_and(|f| f.reading_enabled()) {
        file_type_classify = "image".to_string();
    }
    debug!(file_type_classify, "file type classify");

    // 4. 使用不同类型的文件读取方式来进行读取（Office 文件只校验并算 SHA-256，不把二进制转为文本）
    let (reader, hash_str) = match file_type_classify.as_str() {
        "image" => {
            // 入库前规整一次（缩放/压缩/转成模型支持的格式），不保留原图
            let bytes = std::fs::read(&file_path)?;
            let prepared = prepare_image_for_prompt(&bytes, &file_path.to_string_lossy())?;
            let mut hasher = Sha256::new();
            hasher.update(prepared.as_bytes());
            (Some(prepared), hex::encode(hasher.finalize()))
        }
        "text" if file_path.metadata()?.len() <= INLINE_TEXT_BYTES as u64 => {
            // 读取文本文件
            let bytes = std::fs::read(&file_path)?;
            let hash = hex::encode(Sha256::digest(&bytes));
            let content = String::from_utf8(bytes).ok().filter(|s| !s.contains('\0'));
            if content.is_none() { file_type_classify = "file".into(); }
            (content, hash)
        }
        "docx" | "xlsx" | "pptx" => {
            let max_bytes = if cfg!(mobile) {
                crate::api::office_api::MOBILE_MAX_OFFICE_BYTES
            } else {
                crate::api::office_api::DESKTOP_MAX_OFFICE_BYTES
            };
            if file_path.metadata()?.len() > max_bytes as u64 {
                return Err(AppError::Anyhow(format!("Office 附件 {} 超过 {} MiB", file_path.display(), max_bytes / (1024 * 1024))));
            }
            let bytes = std::fs::read(&file_path)?;
            let file_name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("document.docx");
            crate::api::office_api::validate_office_bytes(file_name, &bytes, max_bytes)
                .map_err(|e| AppError::Anyhow(e.message))?;
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            (None, hex::encode(hasher.finalize()))
        }
        _ => {
            let mut hash = Sha256::new();
            std::io::copy(&mut File::open(&file_path)?, &mut hash)?;
            (None, hex::encode(hash.finalize()))
        }
    };

    debug!(hash = %hash_str, "computed file hash");

    let attachment_type = match file_type_classify.as_str() {
        "image" => AttachmentType::Image,
        "text" => AttachmentType::Text,
        "docx" => AttachmentType::Word,
        "pptx" => AttachmentType::PowerPoint,
        "xlsx" => AttachmentType::Excel,
        "pdf" => AttachmentType::PDF,
        _ => AttachmentType::File,
    };
    Ok((attachment_type, reader, hash_str))
}

// A reference belongs to one message even when two messages attach the same file.
fn register_attachment(db: &ConversationDatabase, path: String, kind: AttachmentType, content: Option<String>, hash: Option<String>) -> Result<AttachmentResult, AppError> {
    let row = db.attachment_repo()?.create(&MessageAttachment {
        id: 0, message_id: -1, attachment_type: kind, attachment_url: Some(path.clone()),
        attachment_content: content, attachment_hash: hash, use_vector: false, token_count: Some(0),
    })?;
    let thumbnail = if kind == AttachmentType::Image { row.attachment_content } else { None };
    Ok(AttachmentResult { attachment_id: row.id, attachment_type: kind as i64, path, thumbnail })
}

fn validate_attachment_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() || name.contains(['/', '\\', ':', '\0']) || Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name) {
        return Err(AppError::Anyhow("无效的附件文件名".into()));
    }
    #[cfg(windows)]
    {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        if name.ends_with([' ', '.']) || name.contains(['<','>','"','|','?','*']) || name.chars().any(char::is_control)
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
            || ["COM","LPT"].iter().any(|prefix| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "1"|"2"|"3"|"4"|"5"|"6"|"7"|"8"|"9"|"¹"|"²"|"³"))) {
            return Err(AppError::Anyhow("附件名称不能用作本机文件名".into()));
        }
    }
    Ok(())
}

fn import_dropped_file(app: tauri::AppHandle, name: &str, data_url: &str) -> Result<AttachmentResult, AppError> {
    validate_attachment_name(name)?;
    let (mime, payload) = data_url.strip_prefix("data:").and_then(|s| s.split_once(";base64,")).ok_or_else(|| AppError::Anyhow("附件缺少 base64 数据".into()))?;
    #[cfg(mobile)]
    if payload.len() > (20_usize * 1024 * 1024).div_ceil(3) * 4 {
        return Err(AppError::Anyhow("移动附件超过 20 MiB".into()));
    }
    let bytes = STANDARD.decode(payload).map_err(|e| AppError::Anyhow(format!("附件解码失败：{e}")))?;
    save_received_bytes(app, name, mime, &bytes)
}

fn save_received_bytes(app: tauri::AppHandle, name: &str, mime: &str, bytes: &[u8]) -> Result<AttachmentResult, AppError> {
    validate_attachment_name(name)?;
    #[cfg(mobile)]
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(AppError::Anyhow("移动附件超过 20 MiB".into()));
    }
    let root = app.path().app_data_dir()?.join(if cfg!(mobile) { "files/mobile-files/imports" } else { "chat_attachments" });
    store_received_bytes(&root, name, bytes, |path| add_attachment_by_path(app, path.to_string_lossy().into_owned(), Some(mime)))
}

fn store_received_bytes<T>(root: &Path, name: &str, bytes: &[u8], register: impl FnOnce(&Path) -> Result<T, AppError>) -> Result<T, AppError> {
    std::fs::create_dir_all(&root)?;
    let dir = root.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&dir)?;
    let path = dir.join(name);
    let result = std::fs::write(&path, bytes).map_err(AppError::from).and_then(|()| register(&path));
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
    result
}

pub async fn add_attachment_content(
    app_handle: tauri::AppHandle,
    file_content: String,
    file_name: String,
    attachment_type: i64,
) -> Result<AttachmentResult, AppError> {
    info!(file_name, "add_attachment_content called");
    #[cfg(mobile)]
    if file_content.len() > 28 * 1024 * 1024 {
        return Err(AppError::Anyhow("MOBILE_FILE attachment-content: exceeds 20 MiB binary/28 MiB encoded limit".into()));
    }
    let kind = AttachmentType::try_from(attachment_type).map_err(|_| AppError::Anyhow("Invalid attachment type".into()))?;
    if kind == AttachmentType::Text {
        return tokio::task::spawn_blocking(move || save_received_bytes(app_handle, &file_name, "text/plain", file_content.as_bytes()))
            .await.map_err(|e| AppError::Anyhow(format!("导入文本附件失败：{e}")))?;
    }
    if kind != AttachmentType::Skill && file_content.starts_with("data:") {
        return tokio::task::spawn_blocking(move || import_dropped_file(app_handle, &file_name, &file_content))
            .await.map_err(|e| AppError::Anyhow(format!("导入附件失败：{e}")))?;
    }
    if kind != AttachmentType::Skill { validate_attachment_name(&file_name)?; }
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;

    let mut hasher = Sha256::new();
    hasher.update(file_content.as_bytes());
    let hash_str = hex::encode(hasher.finalize());

    debug!(hash = %hash_str, "computed content hash");

    register_attachment(&db, file_name, kind, Some(file_content), Some(hash_str))
}

/// Paths from another machine are inert. Only received bytes can create a local file.
pub async fn resolve_received_files(app: &tauri::AppHandle, files: Vec<MessageAttachment>, require_path: bool) -> Result<Vec<MessageAttachment>, AppError> {
    let should_restore = move |a: &MessageAttachment| {
        a.attachment_url.as_deref().is_some_and(|p| p.starts_with("aipp-remote:/"))
            && (require_path || a.attachment_type == AttachmentType::Image || matches!(a.attachment_type, AttachmentType::PDF | AttachmentType::File
                | AttachmentType::Word | AttachmentType::Excel | AttachmentType::PowerPoint)
                && a.attachment_content.as_deref().is_some_and(|s| s.starts_with("data:")))
    };
    if !files.iter().any(should_restore) { return Ok(files); }
    let root = app.path().app_data_dir()?.join(if cfg!(mobile) { "files/mobile-files/imports" } else { "chat_attachments" });
    tokio::task::spawn_blocking(move || {
        let mut files = files;
        for file in &mut files { if should_restore(file) { resolve_received_file(&root, file)?; } }
        Ok(files)
    }).await.map_err(|e| AppError::Anyhow(format!("恢复附件任务失败：{e}")))?
}

fn resolve_received_file(root: &Path, file: &mut MessageAttachment) -> Result<(), AppError> {
    let Some(name) = file.attachment_url.as_deref().and_then(|p| p.strip_prefix("aipp-remote:/")) else { return Ok(()); };
    validate_attachment_name(name)?;
    let missing = || AppError::ProviderError(format!("附件 {name} 的原文件未传到本设备，请重新选择文件"));
    let content = file.attachment_content.as_deref().ok_or_else(missing)?;
    let bytes = match file.attachment_type {
        AttachmentType::Image | AttachmentType::PDF | AttachmentType::File
        | AttachmentType::Word | AttachmentType::Excel | AttachmentType::PowerPoint => {
            let (_, data) = content.strip_prefix("data:").and_then(|s| s.split_once(";base64,")).ok_or_else(missing)?;
            STANDARD.decode(data).map_err(|e| AppError::Anyhow(format!("恢复附件 {name} 失败：{e}")))?
        }
        AttachmentType::Text => {
            if file.attachment_hash.as_deref() != Some(&hex::encode(Sha256::digest(content.as_bytes()))) { return Err(missing()); }
            content.as_bytes().to_vec()
        }
        _ => return Err(missing()),
    };
    let dir = root.join(hex::encode(Sha256::digest(&bytes)));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
    std::io::Write::write_all(&mut tmp, &bytes)?;
    // Replace an edited cache entry with the actual received bytes. Concurrent restores publish identical bytes.
    tmp.persist(&path).map_err(|e| AppError::Anyhow(format!("保存附件 {name} 失败：{e}")))?;
    file.attachment_url = Some(path.to_string_lossy().into_owned());
    Ok(())
}

pub(super) fn needs_text_reader(file: &MessageAttachment) -> bool {
    if file.attachment_type != AttachmentType::Text { return false; }
    let Some(content) = file.attachment_content.as_deref() else { return true; };
    let path = serde_json::json!(file.attachment_url.as_deref().unwrap_or_default());
    ["read_file", "execute_bash"].iter().any(|tool| content == format!("请使用 {tool} 按需读取附件 {path}"))
}

/// Prepare only registered files, before hooks or model requests can modify attachment metadata.
pub async fn prepare_chat_files(mut files: Vec<MessageAttachment>, pdf_input: bool, read_text: bool, run_command: bool) -> Result<Vec<MessageAttachment>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        for file in &mut files {
            let kind = file.attachment_type;
            let path = Path::new(file.attachment_url.as_deref().unwrap_or_default());
            let use_tool = || -> Result<String, AppError> {
                let tool = if kind == AttachmentType::Text && read_text { "read_file" } else if run_command { "execute_bash" } else {
                    return Err(AppError::ProviderError(format!("附件 {}：当前模型无法直接读取，且此会话没有可读取该格式的工具。请选择本地 Agent 或发送所需内容。", path.display())));
                };
                if !path.is_absolute() || !path.is_file() { return Err(AppError::ProviderError(format!("附件 {} 在执行机上不可用，请重新选择文件", path.display()))); }
                Ok(format!("请使用 {tool} 按需读取附件 {}", serde_json::json!(path)))
            };
            let result: anyhow::Result<String> = match kind {
                AttachmentType::Word | AttachmentType::Excel | AttachmentType::PowerPoint => {
                    // Old content-only records may already contain extracted text.
                    if !path.is_absolute() {
                        if file.attachment_content.is_some() { continue; }
                        return Err(AppError::ProviderError(format!("附件 {} 未传到本设备，请重新选择文件", path.display())));
                    }
                    let original = File::open(path).with_context(|| format!("读取 Office 附件 {} 失败，请重新选择文件", path.display()))?;
                    match office_text::extract(original, kind) {
                        Ok(text) => Ok(text),
                        Err(_) if run_command => Ok(use_tool()?),
                        Err(e) => Err(e),
                    }
                }
                AttachmentType::PDF if pdf_input => {
                    if !path.is_absolute() {
                        if file.attachment_content.is_some() { continue; }
                        return Err(AppError::ProviderError(format!("PDF 附件 {} 缺少本机文件，请重新选择", path.display())));
                    }
                    let mut bytes = Vec::new();
                    File::open(path).with_context(|| format!("读取 PDF 附件 {} 失败，请重新选择文件", path.display()))?.take(20 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                    if bytes.len() > 20 * 1024 * 1024 { Ok(use_tool()?) } else {
                        if !bytes.starts_with(b"%PDF-") { return Err(AppError::ProviderError(format!("附件 {} 缺少 PDF 文件头", path.display()))); }
                        Ok(format!("data:application/pdf;base64,{}", STANDARD.encode(bytes)))
                    }
                }
                AttachmentType::File | AttachmentType::PDF => Ok(use_tool()?),
                AttachmentType::Text if needs_text_reader(file) => Ok(use_tool()?),
                _ => continue,
            };
            file.attachment_content = Some(result.with_context(|| format!("准备附件 {} 失败", path.display()))
                .map_err(|e| AppError::ProviderError(format!("{e:#}")))?);
        }
        Ok(files)
    }).await.map_err(|e| AppError::ProviderError(format!("准备普通模型附件失败：{e}")))?
}

#[tauri::command]
pub async fn open_attachment_with_default_app(
    id: i64,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    let attachment = db.attachment_repo().unwrap().read(id)?
        .ok_or_else(|| AppError::Anyhow(format!("Attachment {id} not found")))?;

    #[cfg(target_os = "android")]
    {
        if matches!(
            attachment.attachment_type,
            AttachmentType::Word | AttachmentType::Excel | AttachmentType::PowerPoint
        ) {
            let raw_url = attachment
                .attachment_url
                .as_deref()
                .ok_or_else(|| AppError::Anyhow(format!("Attachment {id}: local path missing")))?;
            let path = Path::new(raw_url);
            crate::mobile_files::require_attachment_path(&app_handle, path)
                .map_err(AppError::Anyhow)?;
            let bytes = std::fs::read(path)?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("document.docx")
                .to_string();
            let mime = match attachment.attachment_type {
                AttachmentType::Word => {
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                }
                AttachmentType::Excel => {
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                }
                AttachmentType::PowerPoint => {
                    "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                }
                _ => "application/octet-stream",
            };
            crate::mobile_files::mobile_file_action(
                app_handle,
                "open".into(),
                serde_json::json!({"name": name, "mime": mime, "data": STANDARD.encode(&bytes)}),
            )
            .await
            .map_err(AppError::Anyhow)?;
            return Ok(());
        }

        let content = attachment.attachment_content.as_deref()
            .ok_or_else(|| AppError::Anyhow(format!("Attachment {id}: local content missing; restore attachment before opening")))?;
        let (data, mime, name) = if content.starts_with("data:image/") {
            let (header, data) = content.split_once(',').ok_or_else(|| AppError::Anyhow("Invalid image data URI".into()))?;
            let mime = header.trim_start_matches("data:").trim_end_matches(";base64");
            let ext = match mime { "image/jpeg" => "jpg", "image/png" => "png", "image/gif" => "gif", "image/webp" => "webp", _ => return Err(AppError::Anyhow("Unsupported image MIME".into())) };
            (data.to_string(), mime.to_string(), format!("attachment-{id}.{ext}"))
        } else {
            (STANDARD.encode(content.as_bytes()), "text/plain".into(), format!("attachment-{id}.txt"))
        };
        crate::mobile_files::mobile_file_action(app_handle, "open".into(), serde_json::json!({"name": name, "mime": mime, "data": data})).await.map_err(AppError::Anyhow)?;
        return Ok(());
    }

    let raw_url = attachment
        .attachment_url
        .as_ref()
        .ok_or_else(|| AppError::Anyhow(format!("Attachment {id}: path missing")))?;
    let direct_path = Path::new(raw_url);
    let resolved_path = if direct_path.exists() {
        direct_path.to_path_buf()
    } else {
        Path::new("attachments").join(raw_url)
    };
    let file_path = resolved_path
        .to_str()
        .ok_or_else(|| AppError::Anyhow("Invalid attachment path".into()))?;
    debug!(file_path, "opening attachment with default app");

    let opener = app_handle.opener();
    opener.open_path(file_path, None::<&str>)?;
    Ok(())
}

/// 文件选择入口：规整图片并返回待入库的 data URL；无法识别或压不下来时直接报错给用户。
fn prepare_image_for_prompt(bytes: &[u8], file_name: &str) -> Result<String, AppError> {
    let image = crate::utils::prompt_image::prepare(bytes)
        .map_err(|e| AppError::Anyhow(format!("图片处理失败: {e}")))?;
    log_prepared_image(file_name, &image);
    Ok(image.data_url())
}

fn log_prepared_image(file_name: &str, image: &crate::utils::prompt_image::PromptImage) {
    if image.reencoded {
        info!(
            file_name,
            source = %format!("{}x{}", image.source_width, image.source_height),
            output = %format!("{}x{} {} {}B", image.width, image.height, image.mime, image.base64.len()),
            "image attachment normalized for model input"
        );
    }
}

#[cfg(test)]
#[path = "attachment_api_tests.rs"]
mod attachment_tests;

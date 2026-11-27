//! Read-only, window-scoped access to a file explicitly opened from chat.
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tauri::{
    http::{header, Request, Response, StatusCode},
    AppHandle, Manager, WebviewWindow,
};
use tauri_plugin_opener::OpenerExt;

use super::agent_workspace_api::resolve_chat_file_link;

struct OpenFile {
    owner: String,
    file: Mutex<File>,
    size: u64,
    modified: Option<SystemTime>,
    mime: String,
    table: bool,
}

#[derive(Default)]
pub struct LinkedFiles(Mutex<HashMap<String, Arc<OpenFile>>>);

impl LinkedFiles {
    pub fn release_window(&self, label: &str) {
        if let Ok(mut entries) = self.0.lock() {
            entries.retain(|_, file| file.owner != label);
        }
    }
}

#[derive(Serialize)]
pub struct LinkedFileInfo {
    token: String,
    size: u64,
}

#[tauri::command]
pub async fn open_chat_preview_file(
    app: AppHandle,
    window: WebviewWindow,
    path: String,
    conversation_id: Option<i64>,
) -> Result<LinkedFileInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = resolve_chat_file_link(&app, &path, conversation_id)?;
        let extension =
            target.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
        let table = matches!(extension.as_str(), "csv" | "tsv");
        let mime = mime_guess::from_path(&target).first_or_octet_stream().essence_str().to_string();
        if !(table
            || mime.starts_with("audio/")
            || mime.starts_with("video/")
            || mime == "application/pdf"
            || (mime.starts_with("image/") && mime != "image/svg+xml"))
        {
            return Err(format!("不支持直接预览此文件：{path}"));
        }
        let file = File::open(&target).map_err(|e| format!("打开文件 {path} 失败：{e}"))?;
        let meta = file.metadata().map_err(|e| format!("读取 {path} 属性失败：{e}"))?;
        if !meta.is_file() {
            return Err(format!("链接目标不是文件：{path}"));
        }
        let size = meta.len();
        if size > 64 * 1024 * 1024 && (mime.starts_with("image/") || mime == "application/pdf") {
            return Err(format!("文件 {path} 超过图片与 PDF 单文件预览上限 64 MiB"));
        }
        let entry = OpenFile {
            owner: window.label().to_owned(),
            file: Mutex::new(file),
            mime,
            size,
            modified: meta.modified().ok(),
            table,
        };
        let token = uuid::Uuid::new_v4().to_string();
        app.state::<LinkedFiles>()
            .0
            .lock()
            .map_err(|_| "无法登记预览文件")?
            .insert(token.clone(), Arc::new(entry));
        Ok(LinkedFileInfo { token, size })
    })
    .await
    .map_err(|e| format!("打开预览文件失败：{e}"))?
}

#[tauri::command]
pub fn close_chat_preview_file(
    window: WebviewWindow,
    files: tauri::State<LinkedFiles>,
    token: String,
) {
    if let Ok(mut entries) = files.0.lock() {
        if entries.get(&token).is_some_and(|f| f.owner == window.label()) {
            entries.remove(&token);
        }
    }
}

#[tauri::command]
pub async fn reveal_chat_file(
    app: AppHandle,
    path: String,
    conversation_id: Option<i64>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = resolve_chat_file_link(&app, &path, conversation_id)?;
        app.opener().reveal_item_in_dir(target).map_err(|e| format!("无法定位 {path}：{e}"))
    })
    .await
    .map_err(|e| format!("定位文件失败：{e}"))?
}

pub fn serve<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    req: Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let files = ctx.app_handle().state::<LinkedFiles>();
    let token = req.uri().path().trim_start_matches('/');
    let entry = files.0.lock().ok().and_then(|entries| entries.get(token).cloned());
    let owned = entry.filter(|f| f.owner == ctx.webview_label());
    tauri::async_runtime::spawn_blocking(move || responder.respond(serve_file(owned, req)));
}

fn serve_file(entry: Option<Arc<OpenFile>>, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, "default-src 'none'")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range");
    if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if crate::mcp::builtin_mcp::interaction::is_allowed_preview_relay_origin(origin) {
            builder = builder
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin)
                .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
                .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS");
        }
    }
    let Some(entry) = entry else {
        return builder.status(StatusCode::NOT_FOUND).body(vec![]).unwrap();
    };
    if req.method() == "OPTIONS" {
        return builder.status(204).body(vec![]).unwrap();
    }
    if req.method() != "GET" && req.method() != "HEAD" {
        return builder.status(405).body(vec![]).unwrap();
    }
    builder =
        builder.header(header::CONTENT_TYPE, &entry.mime).header(header::ACCEPT_RANGES, "bytes");
    if req.method() == "HEAD" {
        return builder.header(header::CONTENT_LENGTH, entry.size).body(vec![]).unwrap();
    }
    let range = req.headers().get(header::RANGE);
    let (start, len) = if let Some(range) = range {
        let parsed =
            range.to_str().ok().and_then(|s| http_range::HttpRange::parse(s, entry.size).ok());
        let Some(r) = parsed.as_deref().and_then(|rs| rs.first()).filter(|r| r.length > 0) else {
            return builder
                .status(416)
                .header(header::CONTENT_RANGE, format!("bytes */{}", entry.size))
                .body(vec![])
                .unwrap();
        };
        (r.start, r.length.min(1024 * 1024))
    } else if entry.table {
        return builder.status(400).body(b"Range required".to_vec()).unwrap();
    } else if entry.mime.starts_with("audio/") || entry.mime.starts_with("video/") {
        (0, entry.size.min(1024 * 1024))
    } else {
        (0, entry.size)
    };
    let read = || -> Result<Vec<u8>, String> {
        let mut file = entry.file.lock().map_err(|_| "预览读取中断")?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if meta.len() != entry.size || meta.modified().ok() != entry.modified {
            return Err("预览期间文件已变化，请重新打开".into());
        }
        file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
        let mut bytes = vec![0; len as usize];
        file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    };
    match read() {
        Ok(bytes) => {
            if len > 0 && (range.is_some() || len < entry.size) {
                builder = builder.status(206).header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{}/{}", start + len - 1, entry.size),
                );
            }
            builder.header(header::CONTENT_LENGTH, len).body(bytes).unwrap()
        }
        Err(reason) => builder.status(409).body(reason.into_bytes()).unwrap(),
    }
}

#[cfg(test)]
#[path = "chat_file_preview_tests.rs"]
mod tests;

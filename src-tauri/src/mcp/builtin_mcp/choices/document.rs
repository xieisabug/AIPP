use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tauri::http::{Request, Response};
struct Document {
    html: String,
    policy: String,
    created: Instant,
}
static DOCUMENTS: LazyLock<Mutex<HashMap<String, Document>>> = LazyLock::new(Default::default);
/// 预览框自报内容高度的脚本；构建预览服务与前端 srcdoc 预览共用同一份。
pub const FRAME_HEIGHT_SCRIPT: &str = include_str!("frame-height.js");
#[derive(Deserialize)]
pub struct Allowed {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}
#[tauri::command]
pub fn create_choice_document(
    html: String,
    allowed: Vec<Allowed>,
    scripts: bool,
) -> Result<String, String> {
    if html.len() > 32 * 1024 * 1024 {
        return Err("预览文档过大".into());
    }
    let mut types: HashMap<&str, Vec<String>> = HashMap::new();
    for item in &allowed {
        if item.id.len() != 64 || !item.id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("预览资源标识无效".into());
        }
        let sources = types.entry(&item.kind).or_default();
        #[cfg(any(target_os = "windows", target_os = "android"))]
        for scheme in ["http", "https"] {
            sources.push(format!("{scheme}://aipp-preview.localhost/{}", item.id));
        }
        #[cfg(not(any(target_os = "windows", target_os = "android")))]
        sources.push(format!("aipp-preview://localhost/{}", item.id));
    }
    let urls = |k: &str| types.get(k).map(|v| v.join(" ")).unwrap_or_default();
    // 每个文档都运行测高脚本。不允许脚本的文档只放行带本次 nonce 的这一段，
    // 内容自身的脚本、事件属性和 javascript: 链接仍被拦截；允许脚本的文档
    // 不能加 nonce，否则浏览器会忽略 'unsafe-inline'，让候选自己的内联脚本失效。
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let script = if scripts {
        format!("'unsafe-inline' {}", urls("script"))
    } else {
        format!("'nonce-{nonce}'")
    };
    let policy=format!("sandbox allow-scripts; default-src 'none'; script-src {script}; style-src 'unsafe-inline' {}; img-src data: {}; font-src data: {}; media-src data: {}; form-action 'none'; base-uri 'none'; frame-src 'none'",
        urls("css"),urls("image"),urls("font"),urls("media"));
    let html = format!("{html}<script nonce=\"{nonce}\">{FRAME_HEIGHT_SCRIPT}</script>");
    let token = uuid::Uuid::new_v4().to_string();
    let mut docs = DOCUMENTS.lock().unwrap();
    docs.retain(|_, v| v.created.elapsed() < Duration::from_secs(600));
    docs.insert(token.clone(), Document { html, policy, created: Instant::now() });
    Ok(token)
}
#[tauri::command]
pub fn close_choice_document(token: String) {
    DOCUMENTS.lock().unwrap().remove(&token);
}
pub fn serve<R: tauri::Runtime>(
    _: tauri::UriSchemeContext<'_, R>,
    req: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let docs = DOCUMENTS.lock().unwrap();
    let id = req.uri().path().trim_start_matches('/');
    let response = Response::builder()
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff");
    match docs.get(id).filter(|d| d.created.elapsed() < Duration::from_secs(600)) {
        Some(doc) => response
            .header("Content-Type", "text/html; charset=utf-8")
            .header("Content-Security-Policy", &doc.policy)
            .body(doc.html.as_bytes().to_owned()),
        None => response.status(404).body(vec![]),
    }
    .unwrap_or_else(|_| Response::new(vec![]))
}
#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

//! `preview_message` 工具与界面取数：把另一条助手回复原样展示在当前管家会话里，
//! 代替转述或大段引用。模型只拿到一张很小的回执（作者、来源、字数、开头摘录）。
use anyhow::Context;
use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::db::conversation_db::ConversationDatabase;
use crate::db::message_preview::{self, MessageRef, PreviewRow};
use crate::mcp::builtin_mcp::templates::{is_butler_conversation_kind, GROUP_MEMBER_KIND};

const EXCERPT_CHARS: usize = 80;

#[derive(Debug, Clone, Serialize)]
pub struct MessagePreview {
    pub message_id: i64,
    pub conversation_id: i64,
    pub conversation_kind: String,
    /// 来源说明，例如「与写作助手的私聊」「定时任务《日报》」。
    pub source: String,
    pub author: String,
    pub created_time: String,
    pub archived: bool,
    pub content: String,
}

/// 只有管家私聊、总管家与独立任务会话可以展示；群聊与普通聊天不提供。
pub(crate) fn viewer_allowed(kind: &str) -> Result<(), String> {
    if kind == GROUP_MEMBER_KIND {
        return Err("群聊里不能用 preview_message 展示其他消息，请直接在回复中 @ 成员".into());
    }
    if is_butler_conversation_kind(kind) {
        return Ok(());
    }
    Err(format!("preview_message 只在管家私聊、总管家和独立任务会话里可用（当前会话类型 {kind}）"))
}

/// 展示的正文只留原话：去掉开头的引用标记与工具调用标记；静默回复与没有正文的消息不能展示。
pub(crate) fn display_content(row: &PreviewRow) -> Result<String, String> {
    let text = crate::utils::reply_quote::strip(&row.content);
    let text = crate::api::conversation_api::strip_mcp_tool_call_segments(text);
    if crate::api::butler_api::is_butler_silent_reply(&text) {
        return Err(format!("消息 #{} 是静默回复，没有可展示的内容", row.message_id));
    }
    if text.trim().is_empty() {
        return Err(format!("消息 #{} 没有可展示的正文（只有工具调用）", row.message_id));
    }
    Ok(text)
}

pub(crate) fn source_label(row: &PreviewRow, author: &str) -> String {
    if let Some(task) = &row.scheduled_task_name {
        return format!("定时任务《{task}》");
    }
    match row.conversation_kind.as_str() {
        "butler_main" if row.archived => "总管家旧主会话".into(),
        "butler_main" => "总管家主会话".into(),
        "butler_contact" => format!("与{author}的私聊"),
        "butler_contact_archive" => format!("与{author}的私聊（已归档）"),
        "butler_task" => format!("独立任务《{}》", row.conversation_name),
        _ => row.conversation_name.clone(),
    }
}

pub(crate) fn excerpt(content: &str) -> String {
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = flat.chars();
    let head: String = chars.by_ref().take(EXCERPT_CHARS).collect();
    if chars.next().is_some() { format!("{head}…") } else { head }
}

/// 解析引用、检查双方范围并读取展示内容。工具执行与界面渲染共用同一套规则。
pub async fn build(app: &AppHandle, viewer: i64, reference: MessageRef) -> Result<MessagePreview, String> {
    let names = super::butler_peer::bot_names(app)
        .await
        .map_err(|e| format!("读取助手名称失败: {e:#}"))?;
    let db = ConversationDatabase::new(app).map_err(|e| format!("打开会话数据库失败: {e}"))?;
    tokio::task::spawn_blocking(move || {
        let conn = db.get_connection().map_err(|e| format!("打开会话数据库失败: {e}"))?;
        let kind: Option<String> = conn
            .query_row("SELECT conversation_kind FROM conversation WHERE id=?1", [viewer], |r| r.get(0))
            .optional()
            .map_err(|e| format!("读取当前会话失败: {e}"))?;
        viewer_allowed(&kind.ok_or_else(|| format!("当前会话 #{viewer} 不存在"))?)?;
        let id = message_preview::resolve(&conn, &reference)?;
        let row = message_preview::load(&conn, id)
            .map_err(|e| format!("读取消息 #{id} 失败: {e}"))?
            .ok_or_else(|| format!("消息 #{id} 已删除或不存在"))?;
        message_preview::ensure_previewable(&row)?;
        let content = display_content(&row)?;
        let author = row
            .assistant_id
            .and_then(|assistant| names.get(&assistant).cloned())
            .unwrap_or_else(|| "助手".to_string());
        Ok(MessagePreview {
            message_id: row.message_id,
            conversation_id: row.conversation_id,
            conversation_kind: row.conversation_kind.clone(),
            source: source_label(&row, &author),
            author,
            created_time: row.created_time.clone(),
            archived: row.archived,
            content,
        })
    })
    .await
    .map_err(|e| format!("读取消息失败: {e}"))?
}

fn message_id_arg(args: &Value) -> anyhow::Result<Option<i64>> {
    match args.get("message_id") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .or_else(|| value.as_str().and_then(|s| s.trim().trim_start_matches('#').parse().ok()))
            .map(Some)
            .context("message_id 应为整数"),
    }
}

/// 工具参数里的引用（工具执行与外部渠道投递共用，接受 "#12" 这类字符串 ID）。
pub(crate) fn reference_from_args(args: &Value) -> Result<MessageRef, String> {
    let message_id = message_id_arg(args).map_err(|e| e.to_string())?;
    MessageRef::parse(message_id, args.get("work_ref").and_then(Value::as_str))
}

/// 工具回执：用户看到的是完整原文，模型只需要确认展示的是哪一条。
pub async fn tool(app: &AppHandle, current: i64, args: &Value) -> anyhow::Result<Value> {
    let reference = reference_from_args(args).map_err(anyhow::Error::msg)?;
    let preview = build(app, current, reference).await.map_err(anyhow::Error::msg)?;
    Ok(json!({
        "status": "preview_shown",
        "message_id": preview.message_id,
        "author": preview.author,
        "source": preview.source,
        "created_time": preview.created_time,
        "chars": preview.content.chars().count(),
        "excerpt": excerpt(&preview.content),
        "note": "用户已看到完整原文，不要再复述；只补充你的判断或下一步。"
    }))
}

#[tauri::command]
pub async fn get_message_preview(
    app_handle: AppHandle,
    viewer_conversation_id: i64,
    message_id: Option<i64>,
    work_ref: Option<String>,
) -> Result<MessagePreview, String> {
    let reference = MessageRef::parse(message_id, work_ref.as_deref())?;
    build(&app_handle, viewer_conversation_id, reference).await
}

#[cfg(test)]
#[path = "message_preview_tests.rs"]
mod tests;

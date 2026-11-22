//! `aipp:office` 内置工具集：`office_edit` 修改用户交给助手的 Word/Excel/PowerPoint，结果另存为新副本。
//!
//! 编辑引擎（BetterOffice）只能在 WebView 里运行，所以执行是一次中继：工具登记任务并广播
//! `office-edit-request` → 任一应用窗口认领（互斥）→ 读取任务源字节 → 在隐藏的查看器 iframe 里按步骤修改并保存
//! → 回传字节 → 这里校验 OOXML、原子写入应用数据目录下的副本并登记产物引用。原文件从不改动。
use crate::api::assistant_api::{MCPServerWithTools, MCPToolInfo};
use crate::api::office_api::{
    encode_office_file_response, office_preview_max_bytes, validate_office_bytes, OfficeFileHeader, OfficeFormat,
    OfficePreviewError,
};
use crate::api::office_save::{outputs_root, read_validated, sanitize_output_name, sha256_hex, write_office_file_safely};
use crate::db::conversation_db::ConversationDatabase;
use crate::db::office_refs::{self, OfficeFileRef};
use crate::mcp::builtin_mcp::templates::BuiltinToolInfo;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

pub const COMMAND: &str = "aipp:office";
pub const EDIT_TOOL: &str = "office_edit";
pub const EDIT_EVENT: &str = "office-edit-request";
pub const MAX_STEPS: usize = 50;
/// 没有窗口认领任务时尽快报错，而不是让模型一直等。
const CLAIM_TIMEOUT: Duration = Duration::from_secs(15);
/// 认领后的执行上限（含首次使用时下载 Office 组件）。
const RUN_TIMEOUT: Duration = Duration::from_secs(300);

pub fn is_tool(name: &str) -> bool {
    name == EDIT_TOOL
}

pub fn definitions() -> Vec<BuiltinToolInfo> {
    vec![BuiltinToolInfo {
        name: EDIT_TOOL.into(),
        description: "修改用户交给你的 Word/Excel/PowerPoint 文件，结果另存为新文件，原文件不会被改动。\
ref 来自用户消息里 <officeselection ref=\"…\"> 块，或上一次 office_edit 结果中的 output_ref（在副本上继续修改）。\
按格式可用的步骤：\
docx：replace_text{find, replace, paragraph?}（find 必须在文档或指定段落中只出现一次；paragraph 写所在段落全文用于消歧）、\
replace_paragraph{paragraph（当前段落完整原文）, text}、insert_paragraphs_after{paragraph, paragraphs[]}；\
xlsx：set_cells{sheet（工作表名）, range（A1，如 B2:C3）, values（与区域同形的二维数组，按用户输入的方式写，公式以 = 开头）}；\
pptx：replace_text{slide?（从 1 开始）, find, replace}。\
所有文字都不能包含换行；替换后的文字沿用原位置的格式。定位不唯一、原文对不上或文件在用户选择后被改动时会拒绝并说明原因，不会猜位置。\
成功后用户会在对话里看到可预览、打开、另存的文件卡片，你不需要再用 preview_file 展示。"
            .into(),
        input_schema: json!({
            "type": "object",
            "required": ["ref", "edits"],
            "properties": {
                "ref": {"type": "string", "description": "文件引用号，形如 ofr_xxxxxxxxxxxxxxxx"},
                "edits": {
                    "type": "array",
                    "description": format!("按顺序执行的修改步骤（1–{MAX_STEPS} 个），全部成功才保存"),
                    "items": {
                        "type": "object",
                        "required": ["type"],
                        "properties": {
                            "type": {"type": "string", "enum": ["replace_text", "replace_paragraph", "insert_paragraphs_after", "set_cells"]},
                            "find": {"type": "string"},
                            "replace": {"type": "string"},
                            "paragraph": {"type": "string"},
                            "text": {"type": "string"},
                            "paragraphs": {"type": "array", "items": {"type": "string"}},
                            "slide": {"type": "integer", "description": "pptx：幻灯片序号，从 1 开始"},
                            "sheet": {"type": "string"},
                            "range": {"type": "string"},
                            "values": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}}
                        }
                    }
                },
                "output_name": {"type": "string", "description": "可选：副本文件名，默认“原名（AI 修改）”"}
            }
        }),
    }]
}

/// 对所有会话类型注入（与保险箱一致）；关闭此工具集即停用。
pub fn inject(app: &AppHandle, servers: &mut Vec<MCPServerWithTools>) {
    servers.retain(|s| s.command.as_deref() != Some(COMMAND));
    let Ok(db) = crate::db::mcp_db::MCPDatabase::new(app) else { return };
    let Ok(stored) = db.get_mcp_servers() else { return };
    let Some(server) = stored.iter().find(|s| s.is_builtin && s.command.as_deref() == Some(COMMAND)) else { return };
    if !server.is_enabled {
        return;
    }
    let Ok(tools) = db.get_mcp_server_tools(server.id) else { return };
    let tools: Vec<MCPToolInfo> = definitions()
        .iter()
        .filter_map(|definition| {
            let stored = tools.iter().find(|t| t.tool_name == definition.name)?;
            Some(MCPToolInfo {
                id: stored.id,
                name: definition.name.clone(),
                description: definition.description.clone(),
                parameters: definition.input_schema.to_string(),
                is_enabled: true,
                is_auto_run: stored.is_auto_run,
            })
        })
        .collect();
    if tools.is_empty() {
        return;
    }
    servers.push(MCPServerWithTools {
        id: server.id,
        name: server.name.clone(),
        summary: String::new(),
        command: server.command.clone(),
        is_enabled: true,
        tools,
    });
}

// ------------------------------------------------------------------ step validation

fn allowed_steps(format: OfficeFormat) -> &'static [&'static str] {
    match format {
        OfficeFormat::Docx => &["replace_text", "replace_paragraph", "insert_paragraphs_after"],
        OfficeFormat::Xlsx => &["set_cells"],
        OfficeFormat::Pptx => &["replace_text"],
    }
}

fn format_label(format: OfficeFormat) -> &'static str {
    match format {
        OfficeFormat::Docx => "Word 文档",
        OfficeFormat::Xlsx => "Excel 表格",
        OfficeFormat::Pptx => "PowerPoint 演示文稿",
    }
}

fn required_str(step: &Value, key: &str, index: usize, allow_empty: bool) -> Result<String, String> {
    let value = step
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("第 {} 步缺少字符串字段 {}", index + 1, key))?;
    if !allow_empty && value.is_empty() {
        return Err(format!("第 {} 步的 {} 不能为空", index + 1, key));
    }
    Ok(value.to_string())
}

fn optional_str(step: &Value, key: &str, index: usize) -> Result<Option<String>, String> {
    match step.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("第 {} 步的 {} 必须是字符串", index + 1, key)),
    }
}

fn cell_input(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some(String::new()),
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "TRUE".into() } else { "FALSE".into() }),
        _ => None,
    }
}

/// 校验并规范化模型给的步骤：只保留已知字段，类型不对或格式不支持时给出具体原因。
pub fn normalize_steps(format: OfficeFormat, edits: &[Value]) -> Result<Vec<Value>, (String, String)> {
    if edits.is_empty() || edits.len() > MAX_STEPS {
        return Err(("invalid_edit".into(), format!("edits 需要 1–{MAX_STEPS} 个步骤")));
    }
    let allowed = allowed_steps(format);
    let mut out = Vec::with_capacity(edits.len());
    for (index, step) in edits.iter().enumerate() {
        let kind = step.get("type").and_then(Value::as_str).unwrap_or_default();
        if !allowed.contains(&kind) {
            return Err((
                "unsupported_edit".into(),
                format!(
                    "第 {} 步：{}不支持 {} 类型的修改，可用：{}",
                    index + 1,
                    format_label(format),
                    if kind.is_empty() { "（缺少 type）" } else { kind },
                    allowed.join(" / ")
                ),
            ));
        }
        let invalid = |message: String| ("invalid_edit".to_string(), message);
        let normalized = match kind {
            "replace_text" => {
                let mut v = json!({
                    "type": kind,
                    "find": required_str(step, "find", index, false).map_err(invalid)?,
                    "replace": required_str(step, "replace", index, true).map_err(invalid)?,
                });
                if let Some(p) = optional_str(step, "paragraph", index).map_err(invalid)? {
                    v["paragraph"] = json!(p);
                }
                if format == OfficeFormat::Pptx {
                    match step.get("slide") {
                        None | Some(Value::Null) => {}
                        Some(slide) => {
                            let n = slide
                                .as_u64()
                                .filter(|n| *n >= 1)
                                .ok_or_else(|| invalid(format!("第 {} 步的 slide 必须是从 1 开始的整数", index + 1)))?;
                            v["slide"] = json!(n);
                        }
                    }
                }
                v
            }
            "replace_paragraph" => json!({
                "type": kind,
                "paragraph": required_str(step, "paragraph", index, false).map_err(invalid)?,
                "text": required_str(step, "text", index, true).map_err(invalid)?,
            }),
            "insert_paragraphs_after" => {
                let paragraphs = step
                    .get("paragraphs")
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty() && a.len() <= MAX_STEPS)
                    .ok_or_else(|| invalid(format!("第 {} 步的 paragraphs 需要 1–{MAX_STEPS} 个字符串", index + 1)))?;
                let texts = paragraphs
                    .iter()
                    .map(|p| p.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| invalid(format!("第 {} 步的 paragraphs 只能包含字符串", index + 1)))?;
                json!({
                    "type": kind,
                    "paragraph": required_str(step, "paragraph", index, false).map_err(invalid)?,
                    "paragraphs": texts,
                })
            }
            "set_cells" => {
                let rows = step
                    .get("values")
                    .and_then(Value::as_array)
                    .filter(|rows| !rows.is_empty())
                    .ok_or_else(|| invalid(format!("第 {} 步的 values 必须是非空二维数组", index + 1)))?;
                let mut matrix = Vec::with_capacity(rows.len());
                for row in rows {
                    let cells = row
                        .as_array()
                        .ok_or_else(|| invalid(format!("第 {} 步的 values 每一行都必须是数组", index + 1)))?;
                    let row = cells
                        .iter()
                        .map(cell_input)
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(|| invalid(format!("第 {} 步的单元格值只能是文字、数字、布尔或 null", index + 1)))?;
                    matrix.push(row);
                }
                json!({
                    "type": kind,
                    "sheet": required_str(step, "sheet", index, false).map_err(invalid)?,
                    "range": required_str(step, "range", index, false).map_err(invalid)?,
                    "values": matrix,
                })
            }
            _ => unreachable!(),
        };
        out.push(normalized);
    }
    Ok(out)
}

// ------------------------------------------------------------------ job relay state

struct PendingJob {
    format: OfficeFormat,
    file_name: String,
    steps: Vec<Value>,
    source: Arc<Vec<u8>>,
    sha256: String,
    claimed: bool,
    claim_tx: Option<oneshot::Sender<()>>,
    result_tx: Option<oneshot::Sender<Result<Vec<u8>, (String, String)>>>,
}

#[derive(Clone, Default)]
pub struct OfficeEditState {
    jobs: Arc<StdMutex<HashMap<String, PendingJob>>>,
}

impl OfficeEditState {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingJob>> {
        self.jobs.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 第一个认领的窗口执行任务，其余返回 None。
    pub fn claim(&self, job_id: &str) -> Option<ClaimedJob> {
        let mut jobs = self.lock();
        let job = jobs.get_mut(job_id)?;
        if job.claimed {
            return None;
        }
        job.claimed = true;
        if let Some(tx) = job.claim_tx.take() {
            let _ = tx.send(());
        }
        Some(ClaimedJob {
            job_id: job_id.to_string(),
            format: job.format.as_str().to_string(),
            file_name: job.file_name.clone(),
            steps: job.steps.clone(),
        })
    }

    fn source(&self, job_id: &str) -> Option<(Arc<Vec<u8>>, String, OfficeFormat, String)> {
        let jobs = self.lock();
        let job = jobs.get(job_id).filter(|j| j.claimed)?;
        Some((job.source.clone(), job.file_name.clone(), job.format, job.sha256.clone()))
    }

    /// 只有已认领的任务能交结果；每个任务只交一次。
    pub fn finish(&self, job_id: &str, result: Result<Vec<u8>, (String, String)>) -> bool {
        let mut jobs = self.lock();
        let Some(job) = jobs.get_mut(job_id).filter(|j| j.claimed) else { return false };
        match job.result_tx.take() {
            Some(tx) => tx.send(result).is_ok(),
            None => false,
        }
    }

    fn remove(&self, job_id: &str) {
        self.lock().remove(job_id);
    }
}

struct JobGuard {
    state: OfficeEditState,
    job_id: String,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.state.remove(&self.job_id);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimedJob {
    pub job_id: String,
    pub format: String,
    pub file_name: String,
    pub steps: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditRequestEvent {
    job_id: String,
    conversation_id: Option<i64>,
    file_name: String,
    format: String,
}

// ------------------------------------------------------------------ tool execution

fn tool_error(code: &str, message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [{"type": "text", "text": format!("office_edit 失败（{code}）：{message}")}],
        "isError": true
    })
}

fn conversation_conn(app: &AppHandle) -> Result<rusqlite::Connection, String> {
    ConversationDatabase::new(app).and_then(|db| db.get_connection()).map_err(|e| format!("打开会话数据库失败：{e}"))
}

pub async fn execute(app: &AppHandle, tool_name: &str, args: &Value, conversation_id: Option<i64>) -> Value {
    if tool_name != EDIT_TOOL {
        return tool_error("unknown_tool", format!("未知的 Office 工具：{tool_name}"));
    }
    match run_edit(app, args, conversation_id).await {
        Ok(result) => json!({"content": [{"type": "json", "json": result}], "isError": false}),
        Err((code, message)) => tool_error(&code, message),
    }
}

async fn run_edit(app: &AppHandle, args: &Value, conversation_id: Option<i64>) -> Result<Value, (String, String)> {
    let e = |code: &str, message: String| (code.to_string(), message);
    let ref_id = args
        .get("ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| e("invalid_edit", "缺少 ref（文件引用号）".into()))?
        .to_string();
    let edits = args
        .get("edits")
        .and_then(Value::as_array)
        .ok_or_else(|| e("invalid_edit", "edits 必须是数组".into()))?;
    let conversation_id =
        conversation_id.ok_or_else(|| e("ref_not_found", "当前没有会话，无法使用文件引用".into()))?;

    let conn = conversation_conn(app).map_err(|m| e("read_error", m))?;
    let source_ref = office_refs::resolve_for_conversation(&conn, &ref_id, Some(conversation_id))
        .map_err(|err| e("read_error", format!("读取文件引用失败：{err}")))?
        .ok_or_else(|| e("ref_not_found", format!("本会话里没有引用 {ref_id}；只能修改用户在本会话交给你的文件或 office_edit 生成的副本")))?;
    drop(conn);

    let source_path = PathBuf::from(&source_ref.file_path);
    let source_name = source_ref.file_name.clone();
    let (bytes, format) = {
        let path = source_path.clone();
        let name = source_name.clone();
        tokio::task::spawn_blocking(move || read_validated(&path, &name))
            .await
            .map_err(|err| e("read_error", format!("读取线程异常退出：{err}")))?
            .map_err(|err: OfficePreviewError| e(&err.code, err.message))?
    };
    let sha256 = sha256_hex(&bytes);
    if !sha256.eq_ignore_ascii_case(&source_ref.sha256) {
        return Err(e(
            "stale_reference",
            format!(
                "「{source_name}」在用户选择之后已被修改（文件指纹不一致）。为避免改错位置已停止，原文件未改动；请让用户重新打开文件并重新选择"
            ),
        ));
    }
    let steps = normalize_steps(format, edits)?;

    let state = app
        .try_state::<OfficeEditState>()
        .ok_or_else(|| e("internal", "Office 修改任务状态未初始化".into()))?
        .inner()
        .clone();
    let job_id = uuid::Uuid::new_v4().to_string();
    let (claim_tx, claim_rx) = oneshot::channel();
    let (result_tx, result_rx) = oneshot::channel();
    state.lock().insert(
        job_id.clone(),
        PendingJob {
            format,
            file_name: source_name.clone(),
            steps,
            source: Arc::new(bytes),
            sha256: sha256.clone(),
            claimed: false,
            claim_tx: Some(claim_tx),
            result_tx: Some(result_tx),
        },
    );
    let _guard = JobGuard { state: state.clone(), job_id: job_id.clone() };

    app.emit(
        EDIT_EVENT,
        EditRequestEvent {
            job_id: job_id.clone(),
            conversation_id: Some(conversation_id),
            file_name: source_name.clone(),
            format: format.as_str().into(),
        },
    )
    .map_err(|err| e("no_window", format!("通知应用窗口失败：{err}")))?;

    match tokio::time::timeout(CLAIM_TIMEOUT, claim_rx).await {
        Ok(Ok(())) => {}
        _ => {
            return Err(e(
                "no_window",
                "修改在应用窗口里的 Office 组件中完成，但当前没有可用的 AIPP 窗口接手（15 秒内无响应）。请打开 AIPP 窗口后重试".into(),
            ))
        }
    }
    let output = match tokio::time::timeout(RUN_TIMEOUT, result_rx).await {
        Ok(Ok(Ok(bytes))) => bytes,
        Ok(Ok(Err((code, message)))) => return Err((code, message)),
        Ok(Err(_)) => return Err(e("cancelled", "修改任务被中断（窗口已关闭或重新加载），未生成文件".into())),
        Err(_) => return Err(e("timeout", "修改任务超过 5 分钟仍未完成，已放弃，未生成文件".into())),
    };

    save_output(app, conversation_id, &source_ref, format, output, args.get("output_name").and_then(Value::as_str)).await
}

async fn save_output(
    app: &AppHandle,
    conversation_id: i64,
    source_ref: &OfficeFileRef,
    format: OfficeFormat,
    output: Vec<u8>,
    requested_name: Option<&str>,
) -> Result<Value, (String, String)> {
    let e = |code: &str, message: String| (code.to_string(), message);
    let file_name = sanitize_output_name(requested_name, &source_ref.file_name, format);
    let checked = validate_office_bytes(&file_name, &output, office_preview_max_bytes())
        .map_err(|err| e("invalid_output", format!("生成的文件没有通过校验，未保存：{}", err.message)))?;
    if checked != format {
        return Err(e("invalid_output", "生成的文件格式与原文件不一致，未保存".into()));
    }
    let output_ref_id = office_refs::new_ref_id();
    let dir = outputs_root(app)
        .map_err(|err| e(&err.code, err.message))?
        .join(conversation_id.to_string())
        .join(&output_ref_id);
    let target = dir.join(&file_name);
    let size = output.len();
    let output_sha = sha256_hex(&output);
    {
        let dir = dir.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || -> Result<(), OfficePreviewError> {
            std::fs::create_dir_all(&dir)
                .map_err(|err| OfficePreviewError::new("write_failed", format!("创建副本目录失败：{err}")))?;
            let result = write_office_file_safely(&target, &output, false);
            if result.is_err() {
                let _ = std::fs::remove_dir(&dir);
            }
            result
        })
        .await
        .map_err(|err| e("write_failed", format!("写入线程异常退出：{err}")))?
        .map_err(|err| e(&err.code, err.message))?;
    }

    let origin_path = source_ref.origin_path.clone().or_else(|| {
        (source_ref.kind == office_refs::KIND_SELECTION).then(|| source_ref.file_path.clone())
    });
    let record = OfficeFileRef {
        id: output_ref_id.clone(),
        conversation_id: Some(conversation_id),
        kind: office_refs::KIND_OUTPUT.into(),
        file_path: target.to_string_lossy().to_string(),
        file_name: file_name.clone(),
        format: format.as_str().into(),
        sha256: output_sha.clone(),
        size_bytes: size as i64,
        origin_ref: Some(source_ref.id.clone()),
        origin_path,
        created_at: chrono::Utc::now().timestamp(),
    };
    let conn = conversation_conn(app).map_err(|m| e("read_error", m))?;
    if let Err(err) = office_refs::insert(&conn, &record) {
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(target.parent().unwrap_or(Path::new("")));
        return Err(e("write_failed", format!("登记副本失败，已删除未登记的文件：{err}")));
    }

    Ok(json!({
        "status": "saved",
        "output_ref": output_ref_id,
        "file_name": file_name,
        "format": format.as_str(),
        "size_bytes": size,
        "sha256": output_sha,
        "source_ref": source_ref.id,
        "source_file_name": source_ref.file_name,
        "note": "已另存为新文件，原文件未改动；用户可在卡片里预览、打开或另存。要在这份副本上继续修改，用 output_ref 作为 ref。"
    }))
}

// ------------------------------------------------------------------ commands used by the frontend runner

fn state_of(app: &AppHandle) -> Result<OfficeEditState, String> {
    app.try_state::<OfficeEditState>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| "Office 修改任务状态未初始化".to_string())
}

#[tauri::command]
pub async fn claim_office_edit_job(app_handle: AppHandle, job_id: String) -> Result<Option<ClaimedJob>, String> {
    Ok(state_of(&app_handle)?.claim(job_id.trim()))
}

#[tauri::command]
pub async fn read_office_edit_job_source(app_handle: AppHandle, job_id: String) -> Result<tauri::ipc::Response, String> {
    let (bytes, file_name, format, sha256) = state_of(&app_handle)?
        .source(job_id.trim())
        .ok_or_else(|| "修改任务不存在、未认领或已结束".to_string())?;
    let header = OfficeFileHeader {
        file_name,
        format: format.as_str().to_string(),
        mime: format.mime().to_string(),
        size_bytes: bytes.len() as u64,
        sha256,
        copy_origin_token: None,
    };
    Ok(tauri::ipc::Response::new(encode_office_file_response(&header, &bytes)?))
}

/// 二进制请求：`[u32 头长度][{"jobId":…}][修改后的文件字节]`。
#[tauri::command]
pub async fn complete_office_edit_job(app_handle: AppHandle, request: tauri::ipc::Request<'_>) -> Result<bool, String> {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else {
        return Err("需要二进制请求体".into());
    };
    let (header, bytes) = crate::api::office_save::decode_framed_request(body).map_err(|e| e.message)?;
    let job_id = header.get("jobId").and_then(Value::as_str).ok_or("请求头缺少 jobId")?.to_string();
    Ok(state_of(&app_handle)?.finish(&job_id, Ok(bytes.to_vec())))
}

#[tauri::command]
pub async fn fail_office_edit_job(app_handle: AppHandle, job_id: String, code: String, message: String) -> Result<bool, String> {
    let code = if code.trim().is_empty() { "edit_failed".to_string() } else { code.trim().to_string() };
    Ok(state_of(&app_handle)?.finish(job_id.trim(), Err((code, message))))
}

#[cfg(test)]
#[path = "office_tests.rs"]
mod tests;

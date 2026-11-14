//! Structured stdio launches: never interpolate package arguments into a shell command.
use super::{format, store};
use crate::db::mcp_db::MCPServer;
use crate::mcp::http_transport::ResponseRedactor;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Arc, Mutex}};
use tauri::Manager;
use tokio::process::Command;

pub const PREFIX: &str = "agent-plugin-stdio:";
static ACTIVE: std::sync::LazyLock<Mutex<HashMap<String,usize>>> = std::sync::LazyLock::new(||Mutex::new(HashMap::new()));
pub fn ensure_idle(key: &str) -> Result<(),String> {
    if ACTIVE.lock().unwrap_or_else(|e|e.into_inner()).get(key).copied().unwrap_or(0) > 0 { return Err("插件正在运行，请停止相关工具调用后再更新、回退或卸载".into()); }
    Ok(())
}
#[derive(Serialize, Deserialize)]
pub struct Binding { pub key: String, pub name: String, pub digest: String, pub reference: String }
pub fn binding(command: Option<&str>) -> Result<Option<Binding>, String> {
    command.and_then(|c| c.strip_prefix(PREFIX)).map(|raw| serde_json::from_str(raw).map_err(|e| format!("插件 stdio 绑定损坏：{e}"))).transpose()
}
pub fn endpoint(b: &Binding) -> String { format!("{PREFIX}{}:{}:{}", b.key, b.name, b.digest) }

pub fn expand(raw: &str, root: &Path, values: &HashMap<String,String>) -> Result<String,String> {
    let mut result = String::new(); let mut rest = raw;
    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 2..]; let end = tail.find('}').ok_or("插件变量未闭合")?;
        let key = &tail[..end];
        if ["CODEX_PLUGIN_ROOT", "CLAUDE_PLUGIN_ROOT", "PLUGIN_ROOT"].contains(&key) { result.push_str(&root.to_string_lossy()); }
        else { result.push_str(values.get(key).ok_or_else(|| format!("缺少插件变量 {key}，请在连接环境变量中填写"))?); }
        rest = &tail[end + 1..];
    }
    result.push_str(rest); Ok(result)
}

pub struct Launch { pub program: String, pub args: Vec<String>, pub cwd: PathBuf, pub env: HashMap<String,String> }
pub fn launch(config: &Value, root: &Path, values: &HashMap<String,String>) -> Result<Launch,String> {
    let command = config["command"].as_str().ok_or("stdio 配置缺少 command")?;
    let mut program = expand(command, root, values)?;
    if command.starts_with("./") { program = root.join(format::safe_path(command)?).to_string_lossy().into(); }
    let mut args = Vec::new();
    if let Some(items) = config.get("args") {
        for value in items.as_array().ok_or("stdio args 必须是字符串数组")? { args.push(expand(value.as_str().ok_or("stdio args 元素必须为字符串")?, root, values)?); }
    }
    let mut env = HashMap::new();
    if let Some(items) = config.get("env") {
        for (key,value) in items.as_object().ok_or("stdio env 必须是对象")? {
            if key.is_empty() || key.contains(['=', '\0']) { return Err("stdio 环境变量名无效".into()); }
            env.insert(key.clone(), expand(value.as_str().ok_or("stdio env 值必须为字符串")?,root,values)?);
        }
    }
    env.extend(values.clone());
    for key in ["CODEX_PLUGIN_ROOT", "CLAUDE_PLUGIN_ROOT", "PLUGIN_ROOT"] { env.insert(key.into(), root.to_string_lossy().into()); }
    let cwd = if let Some(dir) = config["cwd"].as_str() {
        let dir = expand(dir,root,values)?;
        let path = if dir=="." || dir=="./" {root.into()} else if Path::new(&dir).is_absolute() { PathBuf::from(dir) } else { root.join(format::safe_path(&dir)?) };
        let path = path.canonicalize().map_err(|e| format!("插件工作目录不存在：{e}"))?;
        if !path.starts_with(root.canonicalize().map_err(|e| e.to_string())?) { return Err("插件工作目录越出安装包".into()); }
        path
    } else { root.into() };
    Ok(Launch { program, args, cwd, env })
}

pub fn command(spec: Launch) -> Result<Command,String> {
    let mut program = spec.program; let mut args = spec.args;
    // npm's Windows .cmd shim needs a shell; invoke the installed JS CLI through Node instead.
    #[cfg(windows)]
    if program == "npx" || program == "npm" {
        let node = which::which("node").map_err(|e| format!("插件需要 Node.js/npm：{e}"))?;
        let cli = node.parent().ok_or("Node 路径没有父目录")?.join(format!("node_modules/npm/bin/{program}-cli.js"));
        if !cli.is_file() { return Err(format!("找不到 npm CLI：{}；请安装完整 Node.js/npm",cli.display())); }
        args.insert(0,cli.to_string_lossy().into()); program = node.to_string_lossy().into();
    }
    #[cfg(windows)]
    if Path::new(&program).is_file() && Path::new(&program).extension().is_none() {
        let first = std::fs::read(&program).map_err(|e| e.to_string())?;
        if first.starts_with(b"#!/bin/sh") || first.starts_with(b"#!/usr/bin/env bash") {
            let shell = which::which("bash").map_err(|e| format!("插件启动脚本需要 POSIX shell（例如 Git Bash）：{e}"))?;
            args.insert(0, program); program = shell.to_string_lossy().into();
        }
    }
    let mut command = Command::new(&program);
    command.args(args).current_dir(spec.cwd).envs(spec.env).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    Ok(command)
}

async fn prepare(app: &tauri::AppHandle, server: &MCPServer) -> Result<Option<(Command,Vec<String>)>,String> {
    let Some(b) = binding(server.command.as_deref())? else { return Ok(None); };
    let app = app.clone(); let server = server.clone();
    tokio::task::spawn_blocking(move || {
        let db = super::super::db(&app)?;
        let current = db.get_mcp_server(server.id).map_err(|e| e.to_string())?;
        if !current.is_enabled || current.command != server.command { return Err(format!("插件连接 {} 已停用或重新配置",server.id)); }
        super::super::guard_enable(&app,server.id)?;
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let receipt = store::receipt(&data,&b.key)?.ok_or("插件已卸载，请重新安装")?;
        if receipt.preview.digest != b.digest { return Err("插件版本已变化，请重新连接后检测工具，旧请求不会运行新代码".into()); }
        if store::modified(&data,&receipt)? { return Err("插件文件已修改，请恢复原文件后连接".into()); }
        let config = &receipt.preview.servers.iter().find(|s| s.name == b.name).ok_or("插件服务已移除")?.config;
        let credential = super::super::credentials::load(&app,&b.reference)?;
        if credential.server_id != server.id || credential.endpoint != endpoint(&b) { return Err("插件环境变量凭据与连接不匹配".into()); }
        let secrets = credential.headers.values().filter(|v| !v.is_empty()).cloned().collect();
        let spec = launch(config,&store::active(&data,&b.key)?,&credential.headers)?;
        Ok(Some((command(spec)?, secrets)))
    }).await.map_err(|e| format!("准备 stdio 工作进程：{e}"))?
}

pub struct Diagnostics { buffer: Arc<Mutex<String>>, redactor: Arc<ResponseRedactor>, task: tokio::task::JoinHandle<()>, id: i64, pid: Option<u32>, key: Option<String> }
impl Diagnostics {
    /// The complete secret set includes plugin connection credentials resolved at spawn time.
    /// Normal tool responses must use it too, not just process/transport diagnostics.
    pub(crate) fn redactor(&self) -> &ResponseRedactor { &self.redactor }

    pub fn error(&self, stage: &str, error: impl std::fmt::Display) -> String {
        let output = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        diagnostic_error(self.id, self.pid, stage, error, &output, &self.redactor)
    }
}

pub(super) fn diagnostic_error(id: i64, pid: Option<u32>, stage: &str, error: impl std::fmt::Display, output: &str, redactor: &ResponseRedactor) -> String {
    // The retained stderr is already sanitized. Also scrub protocol/transport errors,
    // which can echo tool arguments even when the subprocess writes nothing to stderr.
    redactor.clean(&format!("stdio 连接 {} / 进程 {:?} / {stage}：{error}；stderr：{}；退出码由 MCP 传输层管理，未暴露",
        id, pid, if output.is_empty() { "未收到输出" } else { output }))
}

/// Decode exactly as lossy UTF-8 would at EOF, retaining only incomplete code points
/// between reads so a chunk boundary cannot change a known value before matching.
pub(super) fn decode_stderr(bytes: &mut Vec<u8>, eof: bool) -> String {
    let mut result = String::new();
    let mut consumed = 0;
    while consumed < bytes.len() {
        match std::str::from_utf8(&bytes[consumed..]) {
            Ok(text) => { result.push_str(text); consumed = bytes.len(); }
            Err(error) => {
                let valid_end = consumed + error.valid_up_to();
                result.push_str(std::str::from_utf8(&bytes[consumed..valid_end]).expect("valid UTF-8 prefix"));
                consumed = valid_end;
                match error.error_len() {
                    Some(length) => { result.push('\u{fffd}'); consumed += length; }
                    None if eof => { result.push('\u{fffd}'); consumed = bytes.len(); }
                    None => break,
                }
            }
        }
    }
    bytes.drain(..consumed);
    result
}

fn retain_diagnostic(buffer: &Mutex<String>, safe: &str) {
    let mut output = buffer.lock().unwrap_or_else(|e| e.into_inner());
    output.push_str(safe);
    if output.len() > 16384 {
        let mut excess = output.len() - 16384;
        while !output.is_char_boundary(excess) { excess += 1; }
        output.drain(..excess);
    }
}

pub(super) async fn capture_stderr<R: tokio::io::AsyncRead + Unpin>(mut stderr: R, buffer: Arc<Mutex<String>>, redactor: Arc<ResponseRedactor>) {
    use tokio::io::AsyncReadExt;
    let mut stream = redactor.stream();
    let mut pending = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        match stderr.read(&mut chunk).await {
            Ok(0) => {
                let decoded = decode_stderr(&mut pending, true);
                retain_diagnostic(&buffer, &stream.push(&decoded));
                retain_diagnostic(&buffer, &stream.finish());
                break;
            }
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                let decoded = decode_stderr(&mut pending, false);
                retain_diagnostic(&buffer, &stream.push(&decoded));
            }
            Err(error) => {
                // A failed stream is not EOF. Do not expose a withheld secret prefix.
                retain_diagnostic(&buffer, &stream.finish_truncated());
                retain_diagnostic(&buffer, &redactor.clean(&format!("\n[stderr read error: {error}]")));
                break;
            }
        }
    }
}
impl Drop for Diagnostics { fn drop(&mut self) { self.task.abort(); if let Some(key)=&self.key { let mut active=ACTIVE.lock().unwrap_or_else(|e|e.into_inner()); if let Some(count)=active.get_mut(key) { *count=count.saturating_sub(1); } } } }

/// `extra_secrets` are values the fallback command received from vault references; like plugin
/// credentials they are scrubbed from stderr diagnostics.
pub async fn spawn(app: &tauri::AppHandle, server: &MCPServer, fallback: Command, extra_secrets: Vec<String>) -> Result<(rmcp::transport::TokioChildProcess,Diagnostics),String> {
    let key = binding(server.command.as_deref())?.map(|b|b.key);
    let _guard = if key.is_some() { Some(super::api::INSTALL_LOCK.lock().await) } else { None };
    let (command,mut secrets) = prepare(app,server).await?.unwrap_or((fallback,vec![]));
    secrets.extend(extra_secrets);
    let context_redactor = crate::vault::placeholder::current_redactor();
    let redactor = Arc::new(crate::vault::executor::redactor_for(secrets).merged(context_redactor.as_deref()));
    let (transport,stderr) = rmcp::transport::TokioChildProcess::builder(command).stderr(std::process::Stdio::piped()).spawn()
        .map_err(|e| redactor.clean(&format!("stdio 连接 {} 启动失败：{e}",server.id)))?;
    let buffer = Arc::new(Mutex::new(String::new())); let capture = buffer.clone();
    let capture_redactor = redactor.clone();
    let task = tokio::spawn(async move {
        if let Some(stderr) = stderr { capture_stderr(stderr, capture, capture_redactor).await; }
    });
    let pid = transport.id();
    if let Some(key)=&key { *ACTIVE.lock().unwrap_or_else(|e|e.into_inner()).entry(key.clone()).or_default() += 1; }
    Ok((transport,Diagnostics { buffer,redactor,task,id:server.id,pid,key }))
}

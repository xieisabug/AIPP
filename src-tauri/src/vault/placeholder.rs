//! Vault placeholders in tool calls: `<vault:key>`.
//!
//! The single decision point for assistants using the vault. Every tool call passes through
//! `execute_tool_by_transport`, which calls [`prepare`] before dispatch. It finds placeholders in the
//! JSON arguments, works out where the call goes, applies the user's "always allow" list or asks
//! them in the chat (one approval = one use), substitutes the values into a copy of the arguments,
//! and returns a redactor for everything the tool sends back. Shell commands receive values as
//! environment variables (like `op run`), never as command text.
use super::approval::{self, Decision, EntryLabel, VaultUseRequest};
use crate::db::mcp_db::MCPServer;
use crate::mcp::http_transport::ResponseRedactor;
use crate::sync::personal_vault::{self, Resolved};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;

pub const MARKER: &str = "<vault:";
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<vault:([a-z][a-z0-9_]{1,39})>").expect("valid regex"));
const PREVIEW_CHARS: usize = 4000;
const ONCE_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UseError { pub code: &'static str, pub message: String }

impl UseError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into() } }
    fn outcome(&self) -> &'static str {
        if matches!(self.code, "user_denied" | "approval_required" | "approval_timeout") { "denied" } else { "error" }
    }
}

impl std::fmt::Display for UseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}（保险箱 {}）", self.message, self.code) }
}

// ---------- finding placeholders ----------

fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(s) => out.push(s),
        Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        Value::Object(map) => map.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// Keys named in the arguments. A `<vault:` that is not a valid placeholder is an error, so a typo is
/// never sent literally.
pub(crate) fn keys_in(args: &Value) -> Result<BTreeSet<String>, UseError> {
    let mut texts = Vec::new();
    strings(args, &mut texts);
    let mut keys = BTreeSet::new();
    for text in texts.into_iter().filter(|t| t.contains(MARKER)) {
        let found: Vec<_> = PLACEHOLDER.captures_iter(text).map(|c| c[1].to_string()).collect();
        if found.len() != text.matches(MARKER).count() {
            return Err(UseError::new("invalid_placeholder", "占位符格式应为 <vault:键>，键由小写字母、数字和下划线组成"));
        }
        keys.extend(found);
    }
    Ok(keys)
}

fn substitute_value(value: &mut Value, values: &BTreeMap<String, String>) {
    match value {
        Value::String(s) if s.contains(MARKER) => {
            *s = PLACEHOLDER.replace_all(s, |c: &regex::Captures<'_>| values.get(&c[1]).cloned().unwrap_or_default()).into_owned();
        }
        Value::Array(items) => items.iter_mut().for_each(|v| substitute_value(v, values)),
        Value::Object(map) => map.values_mut().for_each(|v| substitute_value(v, values)),
        _ => {}
    }
}

// ---------- where the call goes ----------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    /// `site` (a network origin; "always allow" possible) or `local` (this machine; ask every time).
    pub kind: &'static str,
    pub label: String,
    pub origin: Option<String>,
    pub insecure: bool,
}

impl Destination {
    fn local(label: impl Into<String>) -> Self { Self { kind: "local", label: label.into(), origin: None, insecure: false } }
    fn site(label: String, url: &reqwest::Url) -> Self {
        Self { kind: "site", label, origin: Some(url.origin().ascii_serialization()), insecure: url.scheme() == "http" }
    }
    pub(crate) fn is_site(&self) -> bool { self.kind == "site" }
}

fn authority(raw: &str) -> &str {
    let after_scheme = raw.find("://").map(|i| &raw[i + 3..]).unwrap_or(raw);
    let end = after_scheme.find(['/', '?', '#']).unwrap_or(after_scheme.len());
    &raw[..raw.len() - after_scheme.len() + end]
}

fn parse_site(raw: &str) -> Result<reqwest::Url, UseError> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| UseError::new("invalid_request", format!("URL 无效: {raw}")))?;
    if !matches!(url.scheme(), "https" | "http") || url.host_str().is_none_or(str::is_empty) {
        return Err(UseError::new("invalid_request", format!("只支持 http/https 地址: {raw}")));
    }
    Ok(url)
}

const OPERATION: &str = "aipp:operation";
const BASH_TOOL: &str = "execute_bash";

fn is_shell(server: &MCPServer, tool_name: &str) -> bool {
    server.command.as_deref() == Some(OPERATION) && tool_name == BASH_TOOL
}

pub(crate) fn destination(server: &MCPServer, tool_name: &str, args: &Value) -> Result<Destination, UseError> {
    let command = server.command.as_deref().unwrap_or_default();
    if command == super::tool::COMMAND {
        if tool_name != super::tool::REQUEST_TOOL {
            return Err(UseError::new("invalid_placeholder", "保险箱自己的列表和填写工具里不需要占位符"));
        }
        let raw = args.get("url").and_then(Value::as_str).unwrap_or_default();
        if authority(raw).contains(MARKER) {
            return Err(UseError::new("invalid_placeholder", "占位符不能出现在 URL 的协议、主机或端口里，审批时需要知道请求发往哪里"));
        }
        let url = parse_site(raw)?;
        let method = args.get("method").and_then(Value::as_str).unwrap_or("GET").to_ascii_uppercase();
        let shown = raw.split(['?', '#']).next().unwrap_or(raw);
        return Ok(Destination::site(format!("网页请求 {method} {shown}"), &url));
    }
    match server.transport_type.as_str() {
        "http" | "sse" => {
            let url = parse_site(server.url.as_deref().unwrap_or_default())?;
            Ok(Destination::site(format!("MCP「{}」· {}", server.name, url.origin().ascii_serialization()), &url))
        }
        _ if command == OPERATION => Ok(Destination::local(if tool_name.contains("bash") { "本机命令" } else { "本机文件" })),
        _ if command.starts_with("aipp:") => Ok(Destination::local(format!("AIPP 工具「{} · {tool_name}」", server.name))),
        _ => Ok(Destination::local(format!("本机程序「{}」", server.name))),
    }
}

fn tool_label(server: &MCPServer, tool_name: &str) -> String { format!("{} · {tool_name}", server.name) }

fn preview(server: &MCPServer, tool_name: &str, args: &Value) -> (String, &'static str) {
    let (text, kind) = if is_shell(server, tool_name) {
        (args.get("command").and_then(Value::as_str).unwrap_or_default().to_string(), "command")
    } else {
        (serde_json::to_string_pretty(args).unwrap_or_default(), "json")
    };
    let text = if text.chars().count() > PREVIEW_CHARS { format!("{}…", text.chars().take(PREVIEW_CHARS).collect::<String>()) } else { text };
    (text, kind)
}

// ---------- who is asking ----------

pub(crate) struct Caller { pub assistant_id: Option<i64>, pub assistant_name: Option<String>, pub attended: bool }

pub(crate) fn caller(app: &AppHandle, conversation_id: Option<i64>) -> Caller {
    use crate::db::conversation_db::Repository;
    let conversation = conversation_id.and_then(|id| crate::db::conversation_db::ConversationDatabase::new(app).ok()
        .and_then(|db| db.conversation_repo().ok()).and_then(|repo| repo.read(id).ok().flatten()));
    let assistant_id = conversation.as_ref().and_then(|c| c.assistant_id);
    let assistant_name = assistant_id.and_then(|id| crate::db::assistant_db::AssistantDatabase::new(app).ok()
        .and_then(|db| db.get_assistant(id).ok()).map(|a| a.name));
    // Scheduled runs (and calls without a conversation) have nobody to ask.
    let attended = conversation.as_ref().is_some_and(|c| c.conversation_kind != crate::db::message_preview::SCHEDULED_RUN_KIND);
    Caller { assistant_id, assistant_name, attended }
}

// ---------- one free use after the user typed the value ----------

static ONCE: LazyLock<Mutex<HashMap<(i64, String), Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The user just entered this value for the assistant in this conversation: its next use there
/// within ten minutes does not ask again.
pub(crate) fn grant_once(conversation_id: i64, item_id: &str) {
    if let Ok(mut once) = ONCE.lock() {
        once.retain(|_, granted| granted.elapsed() < ONCE_TTL);
        once.insert((conversation_id, item_id.to_owned()), Instant::now());
    }
}

/// Claim and consume under the same lock, before any approval wait or database recheck. A failed
/// or cancelled call does not restore its claim: another concurrent call must ask again.
fn take_once(conversation_id: Option<i64>, item_id: &str) -> bool {
    let Some(id) = conversation_id else { return false };
    ONCE.lock().map(|mut once| once.remove(&(id, item_id.to_owned()))
        .is_some_and(|granted| granted.elapsed() < ONCE_TTL)).unwrap_or(false)
}

// ---------- shell commands ----------

/// Values handed to a shell command as environment variables, and the variable for each key.
pub(crate) struct ShellVault {
    env: Vec<(String, String)>,
    vars: BTreeMap<String, String>,
    redactor: Arc<ResponseRedactor>,
}

impl ShellVault {
    pub(crate) fn env(&self) -> &[(String, String)] { &self.env }
    pub(crate) fn redactor(&self) -> Arc<ResponseRedactor> { self.redactor.clone() }
    /// Replace placeholders with references to the environment variables for the given shell.
    pub(crate) fn rewrite(&self, command: &str, powershell: bool) -> Result<String, String> {
        rewrite_command(command, powershell, &self.vars)
    }
}

pub(crate) fn env_var(key: &str) -> String { format!("AIPP_VAULT_{}", key.to_ascii_uppercase()) }

/// Single quotes never expand variables (bash and PowerShell), so a placeholder there is refused.
pub(crate) fn rewrite_command(command: &str, powershell: bool, vars: &BTreeMap<String, String>) -> Result<String, String> {
    let escape = if powershell { '`' } else { '\\' };
    let mut out = String::with_capacity(command.len());
    let mut quote: Option<char> = None;
    let mut rest = command;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with(MARKER) {
            if let Some(found) = PLACEHOLDER.captures(rest).filter(|m| m.get(0).is_some_and(|m| m.start() == 0)) {
                if quote == Some('\'') {
                    return Err("占位符放在单引号里不会被替换，请改用双引号，例如 \"<vault:键>\"".into());
                }
                let var = vars.get(&found[1]).ok_or("占位符对应的值没有准备好")?;
                out.push_str(&if powershell { format!("${{env:{var}}}") } else { format!("${{{var}}}") });
                rest = &rest[found[0].len()..];
                continue;
            }
        }
        match (quote, c) {
            (None, '\'') => quote = Some('\''),
            (Some('\''), '\'') => quote = None,
            (None, '"') => quote = Some('"'),
            (Some('"'), '"') => quote = None,
            (None | Some('"'), e) if e == escape => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
                if let Some(next) = rest.chars().next() {
                    out.push(next);
                    rest = &rest[next.len_utf8()..];
                }
                continue;
            }
            _ => {}
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    Ok(out)
}

tokio::task_local! {
    static SHELL: Option<Arc<ShellVault>>;
    static REDACTOR: Option<Arc<ResponseRedactor>>;
    static OPAQUE_AUDIT: bool;
}

/// The vault values for the shell command currently executing, if it used placeholders.
pub(crate) fn current_shell() -> Option<Arc<ShellVault>> {
    SHELL.try_with(|shell| shell.clone()).ok().flatten()
}

/// Only in-memory, scoped to this invocation. Output sinks must clean before logging,
/// serializing or cutting output; the final cleanup remains a second boundary.
pub(crate) fn current_redactor() -> Option<Arc<ResponseRedactor>> {
    REDACTOR.try_with(|redactor| redactor.clone()).ok().flatten()
}

// Short strings are deliberately not globally replaced in ordinary tool output. JSON credentials
// can also be echoed as booleans/numbers/objects rather than strings. Audit storage must not retain
// these ambiguous payloads at all; this policy does not change the tool's normal response.
fn needs_opaque_audit(value: &str) -> bool {
    value.len() < 4 || serde_json::from_str::<Value>(value).is_ok()
}

pub(crate) fn audit_requires_opaque_values() -> bool {
    OPAQUE_AUDIT.try_with(|opaque| *opaque).unwrap_or(false)
}

/// Persistent sinks must clean while the invocation's credentials are still in scope. A later
/// history query has no redactor and must never need to reopen the vault to be safe.
pub(crate) fn clean_persisted_text(text: &str) -> String {
    current_redactor().map_or_else(|| text.to_owned(), |redactor| redactor.clean(text))
}

pub(crate) fn clean_persisted_json(text: &str) -> String {
    let Some(redactor) = current_redactor() else { return text.to_owned() };
    let Ok(mut value) = serde_json::from_str::<Value>(text) else { return redactor.clean(text) };
    let original = value.clone();
    redactor.clean_json(&mut value);
    // Preserve untouched snapshots exactly, including formatting. Callers can then detect whether
    // redaction changed undo inputs without mistaking serialization changes for lost information.
    if value == original { text.to_owned() } else { value.to_string() }
}

#[cfg(test)]
pub(crate) fn test_prepared(values: &[&str]) -> Prepared {
    Prepared {
        redactor: Some(Arc::new(super::executor::redactor_for(values.iter().map(|value| value.to_string())))),
        opaque_audit: values.iter().any(|value| needs_opaque_audit(value)),
        ..Default::default()
    }
}

/// Test helper: shell values for `key = value` pairs, as `prepare` would build them.
#[cfg(test)]
pub(crate) fn test_shell(values: &[(&str, &str)]) -> Arc<ShellVault> {
    Arc::new(ShellVault {
        env: values.iter().map(|(k, v)| (env_var(k), v.to_string())).collect(),
        vars: values.iter().map(|(k, _)| (k.to_string(), env_var(k))).collect(),
        redactor: Arc::new(super::executor::redactor_for(values.iter().map(|(_, v)| v.to_string()))),
    })
}

/// Test helper: run `future` as if `prepare` had approved a shell command.
#[cfg(test)]
pub(crate) async fn with_shell<F: std::future::Future>(shell: Arc<ShellVault>, future: F) -> F::Output {
    SHELL.scope(Some(shell), future).await
}

// ---------- prepare / run / finish ----------

struct UseRecord { item_id: String }

/// What a tool call needs after the vault step: the arguments to send and how to clean the result.
#[derive(Default)]
pub(crate) struct Prepared {
    parameters: Option<String>,
    redactor: Option<Arc<ResponseRedactor>>,
    opaque_audit: bool,
    shell: Option<Arc<ShellVault>>,
    uses: Vec<UseRecord>,
    tool: String,
    destination: String,
    conversation_id: Option<i64>,
    assistant_id: Option<i64>,
}

impl Prepared {
    pub(crate) fn parameters<'a>(&'a self, original: &'a str) -> &'a str { self.parameters.as_deref().unwrap_or(original) }
    pub(crate) fn clean(&self, text: String) -> String {
        match &self.redactor { Some(redactor) => redactor.clean(&text), None => text }
    }
    pub(crate) fn clean_result(&self, text: String) -> String {
        match &self.redactor { Some(redactor) => redactor.clean_tool_output(&text), None => text }
    }
    /// Run the tool with the shell values in scope (only shell commands read them).
    pub(crate) async fn run<F: std::future::Future>(&self, future: F) -> F::Output {
        OPAQUE_AUDIT.scope(self.opaque_audit,
            REDACTOR.scope(self.redactor.clone(), SHELL.scope(self.shell.clone(), future))).await
    }
    pub(crate) fn finish(&self, app: &AppHandle, error: Option<&str>) {
        let reason = error.map(|e| self.clean(e.to_string()));
        for record in &self.uses {
            personal_vault::log_usage(app, &personal_vault::NewUsage {
                item_id: &record.item_id, conversation_id: self.conversation_id, assistant_id: self.assistant_id,
                tool: &self.tool, destination: &self.destination,
                outcome: if error.is_some() { "error" } else { "ok" }, reason: reason.as_deref(),
            });
        }
    }
}

fn log_refusal(app: &AppHandle, entries: &BTreeMap<String, Resolved>, conversation_id: Option<i64>, caller: &Caller, tool: &str, destination: &str, error: &UseError) {
    let reason = error.to_string();
    for entry in entries.values() {
        personal_vault::log_usage(app, &personal_vault::NewUsage {
            item_id: &entry.meta.id, conversation_id, assistant_id: caller.assistant_id,
            tool, destination, outcome: error.outcome(), reason: Some(&reason),
        });
    }
}

/// Decide and substitute. No placeholder: returns immediately without touching the vault.
pub(crate) async fn prepare(app: &AppHandle, conversation_id: Option<i64>, server: &MCPServer, tool_name: &str, parameters: &str) -> Result<Prepared, String> {
    if !parameters.contains(MARKER) { return Ok(Prepared::default()); }
    let args: Value = match serde_json::from_str(parameters) { Ok(args) => args, Err(_) => return Ok(Prepared::default()) };
    let keys = keys_in(&args).map_err(|e| e.to_string())?;
    if keys.is_empty() { return Ok(Prepared::default()); }
    if is_shell(server, tool_name) {
        let mut others = args.clone();
        if let Some(map) = others.as_object_mut() { map.remove("command"); }
        if others.to_string().contains(MARKER) {
            return Err(UseError::new("invalid_placeholder", "Shell 工具只在 command 里支持占位符").to_string());
        }
    }
    let destination = destination(server, tool_name, &args).map_err(|e| e.to_string())?;
    let lookup: Vec<String> = keys.iter().cloned().collect();
    let resolve_app = app.clone();
    let entries = tauri::async_runtime::spawn_blocking(move || personal_vault::resolve_keys(&resolve_app, &lookup)).await
        .map_err(|e| UseError::new("vault_error", format!("读取保险箱失败: {e}")).to_string())?
        .map_err(|e| UseError::new(e.code, e.message).to_string())?;
    let caller = caller(app, conversation_id);
    let tool = tool_label(server, tool_name);
    let entries = match authorize(app, conversation_id, &caller, server, tool_name, &args, &entries, &destination, &tool).await {
        Ok(current) => current,
        Err(error) => {
            log_refusal(app, &entries, conversation_id, &caller, &tool, &destination.label, &error);
            return Err(error.to_string());
        }
    };
    let values: BTreeMap<String, String> = entries.iter().map(|(k, e)| (k.clone(), e.value.clone())).collect();
    let mut substituted = args.clone();
    substitute_value(&mut substituted, &values);
    let mut secrets: Vec<String> = values.values().cloned().collect();
    if server.command.as_deref() == Some(super::tool::COMMAND) && tool_name == super::tool::REQUEST_TOOL {
        secrets.extend(super::executor::basic_auth_secret(&args, &substituted));
    }
    let redactor = Arc::new(super::executor::redactor_for(secrets));
    let (parameters, shell) = if is_shell(server, tool_name) {
        let vars: BTreeMap<String, String> = keys.iter().map(|k| (k.clone(), env_var(k))).collect();
        let env = values.iter().map(|(k, v)| (env_var(k), v.clone())).collect();
        (None, Some(Arc::new(ShellVault { env, vars, redactor: redactor.clone() })))
    } else {
        (Some(substituted.to_string()), None)
    };
    Ok(Prepared {
        parameters, redactor: Some(redactor), shell,
        opaque_audit: values.values().any(|value| needs_opaque_audit(value)),
        uses: entries.values().map(|e| UseRecord { item_id: e.meta.id.clone() }).collect(),
        tool, destination: destination.label, conversation_id, assistant_id: caller.assistant_id,
    })
}

#[allow(clippy::too_many_arguments)]
async fn authorize(app: &AppHandle, conversation_id: Option<i64>, caller: &Caller, server: &MCPServer, tool_name: &str, args: &Value,
    entries: &BTreeMap<String, Resolved>, destination: &Destination, tool: &str) -> Result<BTreeMap<String, Resolved>, UseError> {
    let allowed = |entry: &Resolved| destination.origin.as_ref().is_some_and(|o| entry.meta.allowed_sites.contains(o));
    let pending: Vec<&Resolved> = entries.values().filter(|e| !allowed(e) && !(caller.attended && take_once(conversation_id, &e.meta.id))).collect();
    let mut always: Option<(Vec<String>, String)> = None;
    if !pending.is_empty() {
        let names = pending.iter().map(|e| format!("「{}」", e.meta.name)).collect::<Vec<_>>().join("");
        if !caller.attended {
            let hint = if destination.is_site() { "请用户在凭据保险箱中把这个网站加入「自动允许的网站」" } else { "本机工具每次都需要用户确认" };
            return Err(UseError::new("approval_required", format!("无人值守的任务不能询问用户：{names}没有被允许用于 {}。{hint}", destination.label)));
        }
        let (preview, preview_kind) = preview(server, tool_name, args);
        let request = VaultUseRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            conversation_id,
            message_id: crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).ok().flatten(),
            assistant_name: caller.assistant_name.clone(),
            entries: pending.iter().map(|e| EntryLabel { id: e.meta.id.clone(), key: e.meta.key.clone(), name: e.meta.name.clone() }).collect(),
            tool: tool.to_owned(),
            destination: destination.clone(),
            preview,
            preview_kind,
            can_always: destination.is_site(),
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        match approval::ask(app, request).await.map_err(|(code, message)| UseError::new(code, message))? {
            Decision::Deny => return Err(UseError::new("user_denied", format!("用户拒绝了在 {} 中使用{names}", destination.label))),
            Decision::Once => {}
            Decision::Always => {
                let Some(origin) = destination.origin.clone().filter(|_| destination.is_site()) else {
                    return Err(UseError::new("invalid_request", "本机工具不能设为总是允许"));
                };
                let ids: Vec<String> = pending.iter().map(|e| e.meta.id.clone()).collect();
                always = Some((ids, origin));
            }
        }
    }
    let confirm_app = app.clone();
    let expected = entries.clone();
    let changed_policy = always.is_some();
    let current = tauri::async_runtime::spawn_blocking(move || personal_vault::confirm_use(&confirm_app, &expected,
        always.as_ref().map(|(ids, origin)| (ids.as_slice(), origin.as_str())))).await
        .map_err(|e| UseError::new("vault_error", format!("复核保险箱失败: {e}")))?
        .map_err(|e| UseError::new(e.code, e.message))?;
    if changed_policy { crate::api::vault_api::notify_changed(app); }
    Ok(current)
}

#[cfg(test)]
#[path = "placeholder_tests.rs"]
mod tests;

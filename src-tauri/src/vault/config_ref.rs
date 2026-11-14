//! References to personal-vault credentials inside user configuration: custom MCP headers and
//! environment variables, and a provider's API key.
//!
//! The configuration stores `vault://<id>` (development builds wrote `vault://<id>#<field>`; the
//! suffix is accepted and ignored). The real value is substituted only when a request
//! is sent or a process starts, and only toward a destination the user confirmed by saving that
//! configuration in Settings. SuperAdmin tools write the same tables directly and never record a
//! confirmation, so a model that changes a URL, a launch command or a provider type cannot redirect a
//! secret. This module and `placeholder.rs` are the only places that decide where a vault value may go.
use crate::db::llm_db::LLMProviderConfig;
use crate::db::mcp_db::MCPServer;
use crate::sync::personal_vault::{self, normalize_origin, ConfigBinding, Resolved};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;
use tauri::AppHandle;

pub const MARKER: &str = "vault://";
/// In-memory-only provider config row carrying the confirmed sites from `prepare_provider_configs`
/// to `create_client_with_config`, which is where the final endpoint is known. Never persisted.
pub const PROVIDER_GUARD: &str = "__aipp_vault_guard";

static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"vault://([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(?:#[a-z][a-z0-9_]{0,39})?")
        .expect("static regex")
});

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference { pub id: String }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefError { pub code: &'static str, pub message: String }

impl RefError {
    fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into() } }
}

impl std::fmt::Display for RefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.message) }
}

pub fn contains(text: &str) -> bool { text.contains(MARKER) }

/// All references in `text`. A `vault://` that is not a well-formed reference is an error rather than
/// something to send verbatim.
pub fn parse(text: &str) -> Result<Vec<Reference>, RefError> {
    let refs: Vec<Reference> = REFERENCE.captures_iter(text).map(|c| Reference { id: c[1].to_owned() }).collect();
    if text.matches(MARKER).count() != refs.len() {
        return Err(RefError::new("invalid_reference", "保险箱引用格式无效，请重新从保险箱选择"));
    }
    Ok(refs)
}

fn ids(refs: &[Reference]) -> Vec<String> {
    refs.iter().map(|r| r.id.clone()).collect::<BTreeSet<_>>().into_iter().collect()
}

/// Where a configuration sends its values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An origin such as `https://api.openai.com`.
    Site(String),
    /// A local program, identified by its full launch specification.
    Program { fingerprint: String, command: String },
}

impl Target {
    pub fn site(url: &str) -> Result<Self, RefError> {
        normalize_origin(url).map(Self::Site).map_err(|e| RefError::new("invalid_target", e))
    }
    /// The command line and the raw environment text (references, not values). Changing either, e.g.
    /// adding `NODE_OPTIONS`, makes it a different program that needs a new confirmation.
    pub fn program(command: &str, env_text: &str) -> Self {
        let digest = Sha256::digest(format!("{}\n{}", command.trim(), env_text.trim()).as_bytes());
        Self::Program { fingerprint: hex::encode(digest), command: command.trim().to_owned() }
    }
    pub fn key(&self) -> String {
        match self { Self::Site(origin) => origin.clone(), Self::Program { fingerprint, .. } => format!("program:{fingerprint}") }
    }
    pub fn detail(&self) -> String {
        match self {
            Self::Site(origin) => origin.clone(),
            Self::Program { command, .. } => {
                let short: String = command.chars().take(80).collect();
                format!("本机程序 {short}{}", if short.len() < command.len() { "…" } else { "" })
            }
        }
    }
}

fn approved(bindings: &[ConfigBinding], target: &Target) -> bool {
    let key = target.key();
    bindings.iter().any(|b| b.target == key)
}

/// Replace every reference in `text`. With `target`, each item must have been confirmed for it; the
/// provider path passes `None` and enforces the destination later with the returned guard.
fn substitute(text: &str, items: &BTreeMap<String, Resolved>, context: &str, target: Option<&Target>, secrets: &mut Vec<String>) -> Result<String, RefError> {
    let refs = parse(text)?;
    let mut values = BTreeMap::new();
    for reference in &refs {
        let item = items.get(&reference.id).ok_or_else(|| RefError::new("credential_not_found",
            format!("{context} 引用的凭据已不在保险箱中（可能已被删除），请重新选择")))?;
        if let Some(target) = target.filter(|t| !approved(&item.config_bindings, t)) {
            return Err(RefError::new("site_not_approved", format!(
                "{context} 引用的凭据「{}」没有被确认用于 {}。配置可能被修改过，请在设置中检查后重新保存，或在凭据保险箱中确认。",
                item.meta.name, target.detail())));
        }
        // Process environment values can contain newlines. HTTP headers and provider keys cannot.
        if !matches!(target, Some(Target::Program { .. })) && item.value.contains(['\n', '\r']) {
            return Err(RefError::new("invalid_reference", format!("条目「{}」的值包含换行，不能用于请求头或 API Key", item.meta.name)));
        }
        values.insert(reference.id.clone(), item.value.clone());
    }
    secrets.extend(values.values().cloned());
    Ok(REFERENCE.replace_all(text, |c: &regex::Captures<'_>| values[&c[1]].clone()).into_owned())
}

/// Resolve the references in several values for one destination. Values without references pass
/// through untouched and the vault is never opened. Returns the values plus the secrets used.
pub(crate) fn resolve_blocking(app: &AppHandle, values: Vec<String>, target: &Target, context: &str) -> Result<(Vec<String>, Vec<String>), RefError> {
    let mut refs = Vec::new();
    for value in &values { refs.extend(parse(value)?); }
    if refs.is_empty() { return Ok((values, Vec::new())); }
    let items = personal_vault::resolve_ids(app, &ids(&refs)).map_err(|e| RefError::new(e.code, format!("{context}：{}", e.message)))?;
    resolve_with_items(values, &items, target, context)
}

pub(crate) fn resolve_with_items(values: Vec<String>, items: &BTreeMap<String, Resolved>, target: &Target, context: &str) -> Result<(Vec<String>, Vec<String>), RefError> {
    let mut secrets = Vec::new();
    let values = values.iter().map(|v| substitute(v, items, context, Some(target), &mut secrets)).collect::<Result<Vec<_>, _>>()?;
    Ok((values, secrets))
}

// ---------- MCP ----------

fn mcp_context(name: &str) -> String { format!("MCP「{name}」") }

/// Commands whose values are read by AIPP itself (builtins) or a plugin runtime: no reference support.
fn reference_capable_command(command: Option<&str>) -> bool {
    command.is_none_or(|c| !c.starts_with("aipp:") && !c.starts_with("agent-plugin-stdio:"))
}

/// The destination of a user-configured MCP server.
pub fn mcp_target(transport_type: &str, command: Option<&str>, url: Option<&str>, env_text: Option<&str>) -> Result<Target, RefError> {
    match transport_type {
        "http" => Target::site(url.unwrap_or_default()),
        "stdio" if reference_capable_command(command) => {
            Ok(Target::program(command.unwrap_or_default(), env_text.unwrap_or_default()))
        }
        _ => Err(RefError::new("invalid_target", "此连接不支持保险箱引用")),
    }
}

/// HTTP MCP: a copy of the server with references in headers and env resolved for the URL's origin.
/// `None` when the server has no references (the common case; nothing is opened).
pub(crate) async fn resolve_http_server(app: &AppHandle, server: &MCPServer) -> Result<Option<(MCPServer, Vec<String>)>, String> {
    let has_refs = server.headers.as_deref().is_some_and(contains) || server.environment_variables.as_deref().is_some_and(contains);
    if !has_refs { return Ok(None); }
    let target = mcp_target("http", None, server.url.as_deref(), None).map_err(|e| e.message)?;
    let headers: BTreeMap<String, String> = match server.headers.as_deref().filter(|v| !v.trim().is_empty()) {
        Some(raw) => serde_json::from_str(raw).map_err(|_| "Invalid MCP HTTP headers: expected a JSON object of string values")?,
        None => BTreeMap::new(),
    };
    let (names, header_values): (Vec<String>, Vec<String>) = headers.into_iter().unzip();
    let mut values = header_values;
    values.push(server.environment_variables.clone().unwrap_or_default());
    let (app, context) = (app.clone(), mcp_context(&server.name));
    let (mut values, secrets) = tokio::task::spawn_blocking(move || resolve_blocking(&app, values, &target, &context)).await
        .map_err(|e| e.to_string())?.map_err(|e| e.message)?;
    let env = values.pop().unwrap_or_default();
    let headers: BTreeMap<String, String> = names.into_iter().zip(values).collect();
    let mut resolved = server.clone();
    resolved.headers = Some(serde_json::to_string(&headers).map_err(|e| e.to_string())?);
    resolved.environment_variables = server.environment_variables.as_ref().map(|_| env);
    Ok(Some((resolved, secrets)))
}

/// stdio MCP: environment for the process, references resolved for its launch fingerprint.
pub(crate) async fn stdio_env(app: &AppHandle, server: &MCPServer) -> Result<(Vec<(String, String)>, Vec<String>), String> {
    let raw = server.environment_variables.clone().unwrap_or_default();
    if !contains(&raw) { return Ok((crate::mcp::registry_api::parse_env_vars(&raw), Vec::new())); }
    let target = mcp_target("stdio", server.command.as_deref(), None, Some(&raw)).map_err(|e| e.message)?;
    let (app, context) = (app.clone(), mcp_context(&server.name));
    tokio::task::spawn_blocking(move || resolve_stdio_env_with(&raw, |values| {
        resolve_blocking(&app, values, &target, &context)
    })).await.map_err(|e| e.to_string())?.map_err(|e| e.message)
}

/// Parse configuration syntax before resolving values. A secret's whitespace, `=` or newlines are
/// opaque process data: they must never become extra environment entries or get trimmed afterward.
fn resolve_stdio_env_with(
    raw: &str,
    resolve: impl FnOnce(Vec<String>) -> Result<(Vec<String>, Vec<String>), RefError>,
) -> Result<(Vec<(String, String)>, Vec<String>), RefError> {
    let (names, values): (Vec<String>, Vec<String>) = crate::mcp::registry_api::parse_env_vars(raw).into_iter().unzip();
    if names.iter().any(|name| contains(name)) {
        return Err(RefError::new("invalid_reference", "保险箱引用只能用于环境变量的值，不能用于变量名"));
    }
    let (values, secrets) = resolve(values)?;
    Ok((names.into_iter().zip(values).collect(), secrets))
}

// ---------- confirmation (Settings save commands only) ----------

/// Check that every reference in `texts` points at an existing entry.
pub fn validate(app: &AppHandle, texts: &[&str]) -> Result<(), String> {
    let mut refs = Vec::new();
    for text in texts { refs.extend(parse(text).map_err(|e| e.message)?); }
    if refs.is_empty() { return Ok(()); }
    let metas = personal_vault::metas(app, &ids(&refs))?;
    for reference in &refs {
        if !metas.contains_key(&reference.id) {
            return Err("引用的条目已不在保险箱中（可能已被删除），请重新选择".into());
        }
    }
    Ok(())
}

/// Record that the user confirmed sending the references in `texts` to `target`.
fn confirm(app: &AppHandle, texts: &[&str], target: &Target, source: &str, label: &str) -> Result<(), String> {
    validate(app, texts)?;
    let mut refs = Vec::new();
    for text in texts { refs.extend(parse(text).map_err(|e| e.message)?); }
    if refs.is_empty() { return Ok(()); }
    if personal_vault::bind_config(app, &ids(&refs), source, &target.key(), label, &target.detail())? {
        crate::api::vault_api::notify_changed(app);
    }
    Ok(())
}

/// Settings save of an MCP server (after the row is written, so `id` is known).
pub fn confirm_mcp(app: &AppHandle, id: i64, name: &str, transport_type: &str, command: Option<&str>, url: Option<&str>, env_text: Option<&str>, headers: Option<&str>) -> Result<(), String> {
    let texts: Vec<&str> = [env_text, headers].into_iter().flatten().collect();
    if !texts.iter().any(|t| contains(t)) { return Ok(()); }
    let target = mcp_target(transport_type, command, url, env_text).map_err(|e| e.message)?;
    confirm(app, &texts, &target, &format!("mcp:{id}"), &mcp_context(name))
}

fn provider_context(name: &str) -> String { format!("Provider「{name}」") }

/// The site a provider's requests go to, before a model is chosen.
pub fn provider_target(api_type: &str, endpoint: Option<&str>) -> Result<Target, RefError> {
    let url = crate::api::genai_client::provider_destination(api_type, endpoint).map_err(|e| RefError::new("invalid_target", e))?;
    Target::site(&url)
}

#[derive(Serialize)]
struct ProviderRow { id: i64, name: String, api_type: String, api_key: String, endpoint: Option<String> }

fn provider_rows(app: &AppHandle, filter: Option<i64>, needle: &str) -> Result<Vec<ProviderRow>, String> {
    let db = crate::db::llm_db::LLMDatabase::new(app).map_err(|e| e.to_string())?;
    provider_rows_with(&db.conn, filter, needle)
}

fn provider_rows_with(conn: &rusqlite::Connection, filter: Option<i64>, needle: &str) -> Result<Vec<ProviderRow>, String> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.api_type, k.value,
                (SELECT e.value FROM llm_provider_config e WHERE e.llm_provider_id = p.id AND e.name = 'endpoint')
         FROM llm_provider p JOIN llm_provider_config k ON k.llm_provider_id = p.id AND k.name = 'api_key'
         WHERE (?1 IS NULL OR p.id = ?1) AND instr(k.value, ?2) > 0 ORDER BY p.id").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params![filter, needle], |r| Ok(ProviderRow {
        id: r.get(0)?, name: r.get(1)?, api_type: r.get(2)?, api_key: r.get(3)?, endpoint: r.get(4)?,
    })).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string());
    rows
}

/// Before saving a referenced API key: the provider type must support it and the credential must exist.
pub fn validate_provider_key(app: &AppHandle, provider_id: i64, value: &str) -> Result<(), String> {
    let db = crate::db::llm_db::LLMDatabase::new(app).map_err(|e| e.to_string())?;
    let provider = db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
    let endpoint = db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?
        .into_iter().find(|c| c.name == "endpoint").map(|c| c.value);
    provider_target(&provider.api_type, endpoint.as_deref()).map_err(|e| e.message)?;
    validate(app, &[value])
}

/// Settings save of a provider field (API key, endpoint or type): confirm its current site.
pub fn confirm_provider(app: &AppHandle, provider_id: i64) -> Result<(), String> {
    let Some(row) = provider_rows(app, Some(provider_id), MARKER)?.into_iter().next() else { return Ok(()) };
    let target = provider_target(&row.api_type, row.endpoint.as_deref()).map_err(|e| e.message)?;
    confirm(app, &[&row.api_key], &target, &format!("provider:{}", row.id), &provider_context(&row.name))
}

// ---------- provider runtime ----------

#[derive(Serialize, Deserialize)]
struct ProviderGuard { sites: Vec<String>, context: String, credentials: Vec<String>, #[serde(default)] secrets: Vec<String> }

/// Resolve a provider's API key reference before the client is built. The confirmed sites travel in an
/// in-memory guard row; `check_provider_destination` enforces them against the final endpoint.
pub(crate) async fn prepare_provider_configs(app: &AppHandle, configs: Vec<LLMProviderConfig>) -> Result<Vec<LLMProviderConfig>, String> {
    let Some(index) = configs.iter().position(|c| c.name == "api_key" && contains(&c.value)) else { return Ok(configs) };
    let provider_id = configs[index].llm_provider_id;
    let raw = configs[index].value.clone();
    let app = app.clone();
    let (value, guard) = tokio::task::spawn_blocking(move || -> Result<(String, ProviderGuard), String> {
        let name = crate::db::llm_db::LLMDatabase::new(&app).ok()
            .and_then(|db| db.get_llm_provider(provider_id).ok()).map(|p| p.name).unwrap_or_default();
        let context = provider_context(&name);
        let refs = parse(&raw).map_err(|e| e.message)?;
        let items = personal_vault::resolve_ids(&app, &ids(&refs)).map_err(|e| format!("{context}：{}", e.message))?;
        let mut secrets = Vec::new();
        let value = substitute(&raw, &items, &context, None, &mut secrets).map_err(|e| e.message)?;
        // A site is allowed only if every referenced credential was confirmed for it.
        let mut sites: Option<BTreeSet<String>> = None;
        for item in items.values() {
            let mine: BTreeSet<String> = item.config_bindings.iter().map(|b| b.target.clone()).filter(|t| !t.starts_with("program:")).collect();
            sites = Some(match sites { Some(s) => s.intersection(&mine).cloned().collect(), None => mine });
        }
        let credentials = items.values().map(|i| i.meta.name.clone()).collect();
        Ok((value, ProviderGuard { sites: sites.unwrap_or_default().into_iter().collect(), context, credentials, secrets }))
    }).await.map_err(|e| e.to_string())??;
    let mut configs = configs;
    configs[index].value = value;
    configs.push(LLMProviderConfig {
        id: 0, name: PROVIDER_GUARD.into(), llm_provider_id: provider_id,
        value: serde_json::to_string(&guard).map_err(|e| e.to_string())?, append_location: String::new(), is_addition: false,
    });
    Ok(configs)
}

/// Error-only policy: a JSON credential can be normalized or decomposed by an upstream parser.
/// Do not try to enumerate those spellings, or broaden redaction of ordinary model/tool output.
#[derive(Clone)]
pub(crate) struct ProviderErrorRedactor {
    text: crate::mcp::http_transport::ResponseRedactor,
    opaque_errors: bool,
}

impl ProviderErrorRedactor {
    pub(crate) fn requires_opaque_errors(&self) -> bool { self.opaque_errors }

    pub(crate) fn clean(&self, text: &str) -> String {
        if self.opaque_errors {
            "Provider request failed; credential-bearing error details [REDACTED]".into()
        } else { self.text.clean(text) }
    }
}

/// Build the request-local error redactor before handing resolved authentication to a transport.
/// Include individual vault values as well as the composed API key (references can be embedded).
pub(crate) fn provider_redactor(configs: &[LLMProviderConfig]) -> Result<ProviderErrorRedactor, String> {
    let mut secrets: Vec<String> = configs.iter().filter(|c| c.name == "api_key" && !contains(&c.value))
        .map(|c| c.value.clone()).collect();
    if let Some(row) = configs.iter().find(|c| c.name == PROVIDER_GUARD) {
        let guard: ProviderGuard = serde_json::from_str(&row.value).map_err(|_| "保险箱校验信息损坏，已停止发送")?;
        secrets.extend(guard.secrets);
    }
    let opaque_errors = secrets.iter().any(|secret| {
        let token = secret.strip_prefix("Bearer ").or_else(|| secret.strip_prefix("Basic ")).unwrap_or(secret);
        serde_json::from_str::<serde_json::Value>(token).is_ok()
    });
    // Keep even short raw values; the shared transport helper also adds encoded variants.
    let mut variants = secrets.clone();
    // A JSON body can itself be quoted by the upstream error's Debug implementation.
    let mut escaped = secrets.clone();
    for _ in 0..3 {
        escaped = escaped.iter().map(|value| {
            let json = serde_json::to_string(value).expect("string serialization");
            json[1..json.len() - 1].to_owned()
        }).collect();
        variants.extend(escaped.iter().cloned());
    }
    let raw = crate::mcp::http_transport::ResponseRedactor::new(variants.into_iter());
    Ok(ProviderErrorRedactor { text: super::executor::redactor_for(secrets).merged(Some(&raw)), opaque_errors })
}

/// Called by the client builder once the final endpoint is known. An unresolved reference means a
/// caller skipped `prepare_provider_configs`: fail rather than send the reference as a key.
pub fn check_provider_destination(configs: &[LLMProviderConfig], api_key: &str, endpoint_url: &str) -> Result<(), String> {
    if contains(api_key) {
        return Err("API Key 引用了凭据保险箱，但这次请求没有经过保险箱解析，已停止发送".into());
    }
    let Some(raw) = configs.iter().find(|c| c.name == PROVIDER_GUARD).map(|c| &c.value) else { return Ok(()) };
    let guard: ProviderGuard = serde_json::from_str(raw).map_err(|_| "保险箱校验信息损坏，已停止发送")?;
    let origin = normalize_origin(endpoint_url)?;
    if guard.sites.contains(&origin) { return Ok(()); }
    Err(format!(
        "{} 的 API Key 来自凭据保险箱（{}），没有被确认用于 {origin}。配置可能被修改过，请在设置中检查后重新保存，或在凭据保险箱中确认。",
        guard.context, guard.credentials.join("、")))
}

// ---------- listing for the vault page ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReference {
    /// `mcp` or `provider`.
    pub kind: &'static str,
    pub config_id: i64,
    pub name: String,
    /// Where it currently sends the value (origin or local program), when computable.
    pub target: Option<String>,
    pub approved: bool,
    pub problem: Option<String>,
    /// Digest of this entry and the exact configuration used to produce the displayed target.
    /// No configuration values or credential fragments are returned to the frontend.
    pub snapshot: Option<String>,
}

struct ReferenceConfirmation {
    item_id: String,
    source: String,
    label: String,
    target: Target,
    snapshot: String,
}

fn confirmation_snapshot(item_id: &str, source: &str, target: &Target, configuration: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(item_id, source, target.key(), configuration)).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn require_reference(item_id: &str, texts: &[&str]) -> Result<(), String> {
    let mut found = false;
    for text in texts {
        found |= parse(text).map_err(|e| e.message)?.iter().any(|reference| reference.id == item_id);
    }
    if found { Ok(()) } else { Err("此配置已不再引用当前条目，请检查后重新确认".into()) }
}

fn mcp_confirmation(item_id: &str, server: &MCPServer) -> Result<ReferenceConfirmation, String> {
    require_reference(item_id, &[server.headers.as_deref().unwrap_or_default(), server.environment_variables.as_deref().unwrap_or_default()])?;
    let target = mcp_target(&server.transport_type, server.command.as_deref(), server.url.as_deref(), server.environment_variables.as_deref()).map_err(|e| e.message)?;
    let source = format!("mcp:{}", server.id);
    let snapshot = confirmation_snapshot(item_id, &source, &target, server)?;
    Ok(ReferenceConfirmation { item_id: item_id.into(), source, label: mcp_context(&server.name), target, snapshot })
}

fn provider_confirmation(item_id: &str, row: &ProviderRow) -> Result<ReferenceConfirmation, String> {
    require_reference(item_id, &[&row.api_key])?;
    let target = provider_target(&row.api_type, row.endpoint.as_deref()).map_err(|e| e.message)?;
    let source = format!("provider:{}", row.id);
    let snapshot = confirmation_snapshot(item_id, &source, &target, row)?;
    Ok(ReferenceConfirmation { item_id: item_id.into(), source, label: provider_context(&row.name), target, snapshot })
}

/// Only this entry is granted, never all the references in the configuration. A different entry,
/// target, header, environment, command or provider configuration cannot reuse an old confirmation.
fn confirm_snapshot(
    current: ReferenceConfirmation,
    expected_snapshot: &str,
    bind: impl FnOnce(&[String], &str, &Target, &str) -> Result<bool, String>,
) -> Result<bool, String> {
    if current.snapshot != expected_snapshot {
        return Err("配置在展示后已变化，本次未授权。请检查最新配置后重新确认".into());
    }
    bind(&[current.item_id], &current.source, &current.target, &current.label)
}

/// Every configuration referencing this credential, with its current destination and whether the
/// user confirmed it. Recomputed from the configurations, so a model's edit shows up as unconfirmed.
pub fn references(app: &AppHandle, id: &str) -> Result<Vec<ConfigReference>, String> {
    let needle = format!("{MARKER}{id}");
    let bindings = personal_vault::metas(app, &[id.to_owned()])?.remove(id).map(|(_, b)| b).unwrap_or_default();
    let entry = |kind, config_id, name: String, confirmation: Result<ReferenceConfirmation, String>| match confirmation {
        Ok(c) => ConfigReference { kind, config_id, name, approved: approved(&bindings, &c.target), target: Some(c.target.detail()), problem: None, snapshot: Some(c.snapshot) },
        Err(e) => ConfigReference { kind, config_id, name, approved: false, target: None, problem: Some(e), snapshot: None },
    };
    let mut out = Vec::new();
    let mcp = crate::db::mcp_db::MCPDatabase::new(app).map_err(|e| e.to_string())?;
    for s in mcp.get_mcp_servers().map_err(|e| e.to_string())? {
        let uses = [&s.headers, &s.environment_variables].into_iter().flatten().any(|t| t.contains(&needle));
        if uses {
            out.push(entry("mcp", s.id, mcp_context(&s.name), mcp_confirmation(id, &s)));
        }
    }
    for row in provider_rows(app, None, &needle)? {
        out.push(entry("provider", row.id, provider_context(&row.name), provider_confirmation(id, &row)));
    }
    Ok(out)
}

/// The vault page's "确认" applies to the selected entry and the configuration actually displayed.
/// Keep a write-reserved transaction on the configuration database until the grant is recorded, so
/// another writer cannot change the configuration between the snapshot check and the vault write.
pub fn confirm_reference(app: &AppHandle, item_id: &str, kind: &str, config_id: i64, expected_snapshot: &str) -> Result<(), String> {
    let bind = |ids: &[String], source: &str, target: &Target, label: &str| {
        personal_vault::bind_config(app, ids, source, &target.key(), label, &target.detail())
    };
    let changed = match kind {
        "mcp" => {
            let db = crate::db::mcp_db::MCPDatabase::new(app).map_err(|e| e.to_string())?;
            let tx = rusqlite::Transaction::new_unchecked(&db.conn, rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
            let s = db.get_mcp_server(config_id).map_err(|e| e.to_string())?;
            let changed = confirm_snapshot(mcp_confirmation(item_id, &s)?, expected_snapshot, bind)?;
            tx.commit().map_err(|e| e.to_string())?;
            changed
        }
        "provider" => {
            let db = crate::db::llm_db::LLMDatabase::new(app).map_err(|e| e.to_string())?;
            let tx = rusqlite::Transaction::new_unchecked(&db.conn, rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
            let row = provider_rows_with(&db.conn, Some(config_id), MARKER)?.into_iter().next()
                .ok_or("此配置已不再引用当前条目，请检查后重新确认")?;
            let changed = confirm_snapshot(provider_confirmation(item_id, &row)?, expected_snapshot, bind)?;
            tx.commit().map_err(|e| e.to_string())?;
            changed
        }
        other => return Err(format!("未知的配置类型 {other}")),
    };
    if changed { crate::api::vault_api::notify_changed(app); }
    Ok(())
}

#[cfg(test)]
#[path = "config_ref_tests.rs"]
mod tests;

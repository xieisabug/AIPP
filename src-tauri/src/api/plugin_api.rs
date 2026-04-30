use crate::db::assistant_db::{AssistantDatabase, AssistantPrompt};
use crate::db::connection::params;
use crate::db::conversation_db::{ConversationDatabase, Message, Repository};
use crate::skills::installer::{copy_dir_recursive, extract_zip_bytes_to_dir};
use crate::NameCacheState;
use chrono::Utc;
use rusqlite::{Connection, OpenFlags, ToSql};
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tracing::warn;

use crate::api::ai::config::get_network_proxy_from_config;
use crate::api::ai::events::{ConversationEvent, MessageAddEvent};
use crate::db::plugin_db::{
    NewPluginHookRegistration, Plugin, PluginAssistantConfiguration, PluginData, PluginDatabase,
    PluginHookAuditLog, PluginHookRegistration,
};
use crate::plugin::runtime::verify_entry_checksum as verify_runtime_entry_checksum;

const PLUGIN_TYPE_CONFIG_KEY: &str = "plugin_type";
const DEFAULT_PLUGIN_QUERY_MAX_ROWS: usize = 1000;
const ABSOLUTE_PLUGIN_QUERY_MAX_ROWS: usize = 10000;
const OFFICIAL_PLUGINS_API: &str = "https://aipp-helper.xiejingyang.com/api/plugins";
const OFFICIAL_PLUGINS_TIMEOUT_SECS: u64 = 5;
const PLUGIN_ARCHIVE_DOWNLOAD_TIMEOUT_SECS: u64 = 30;
const PLUGIN_ARCHIVE_MAX_DISCOVERY_DEPTH: usize = 6;
const IGNORED_PLUGIN_DISCOVERY_SEGMENTS: &[&str] = &[
    ".git",
    ".github",
    ".vscode",
    "coverage",
    "docs",
    "node_modules",
    "scripts",
    "src-tauri",
    "target",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginListItem {
    pub plugin_id: i64,
    pub name: String,
    pub version: String,
    pub code: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub plugin_type: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub runtime: Option<PluginRuntimeManifest>,
    #[serde(default)]
    pub contributions: PluginContributions,
    pub is_active: bool,
    pub is_installed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigItem {
    pub config_id: i64,
    pub plugin_id: i64,
    pub config_key: String,
    pub config_value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDataItem {
    pub data_id: i64,
    pub plugin_id: i64,
    pub session_id: String,
    pub data_key: String,
    pub data_value: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookRegistrationItem {
    pub id: i64,
    pub plugin_id: i64,
    pub hook_name: String,
    pub hook_kind: String,
    pub priority: i64,
    pub timeout_ms: i64,
    pub failure_policy: String,
    pub is_active: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookAuditLogItem {
    pub id: i64,
    pub plugin_id: i64,
    pub hook_name: String,
    pub conversation_id: Option<i64>,
    pub message_id: Option<i64>,
    pub status: String,
    pub action: Option<String>,
    pub duration_ms: Option<i64>,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginAssistantConfigItem {
    pub config_id: i64,
    pub plugin_id: i64,
    pub assistant_id: i64,
    pub config_key: String,
    pub config_value: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateAssistantPromptRequest {
    pub assistant_id: i64,
    pub prompt: String,
    #[serde(default)]
    pub expected_prompt_id: Option<i64>,
    #[serde(default)]
    pub expected_old_prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDataQueryRequest {
    pub database: String,
    pub sql: String,
    #[serde(default)]
    pub params: Vec<JsonValue>,
    #[serde(default)]
    pub max_rows: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginStorageQueryRequest {
    pub sql: String,
    #[serde(default)]
    pub params: Vec<JsonValue>,
    #[serde(default)]
    pub max_rows: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginStorageExecuteRequest {
    pub sql: String,
    #[serde(default)]
    pub params: Vec<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginCreateConversationRequest {
    pub assistant_id: i64,
    #[serde(default)]
    pub conversation_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginAppendMessageRequest {
    pub conversation_id: i64,
    pub message_type: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateMessageMetadataRequest {
    pub message_id: i64,
    #[serde(default)]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginSqlQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<JsonValue>>,
    pub row_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginSqlExecuteResult {
    pub rows_affected: usize,
    pub last_insert_rowid: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDatabaseSchema {
    pub database: String,
    pub tables: Vec<PluginDatabaseTableSchema>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDatabaseTableSchema {
    pub name: String,
    pub object_type: String,
    pub sql: Option<String>,
    pub columns: Vec<PluginDatabaseColumnSchema>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginDatabaseColumnSchema {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub default_value: Option<String>,
    pub primary_key: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PluginRegistryChangedEvent {
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifest {
    id: Option<String>,
    code: Option<String>,
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    author: Option<String>,
    #[serde(default, alias = "pluginType", alias = "pluginTypes", alias = "type")]
    plugin_types: Vec<String>,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    runtime: Option<PluginRuntimeManifest>,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    activation_events: Vec<String>,
    #[serde(default)]
    contributions: PluginContributions,
}

#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    code: String,
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    plugin_type: Vec<String>,
    runtime: PluginRuntimeManifest,
    contributions: PluginContributions,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributions {
    #[serde(default)]
    pub bangs: Vec<PluginBangContribution>,
    #[serde(default)]
    pub hooks: Vec<PluginHookContribution>,
    #[serde(default)]
    pub views: Vec<PluginViewContribution>,
    #[serde(default)]
    pub actions: Vec<PluginActionContribution>,
    #[serde(default)]
    pub assistant_form_fields: Vec<PluginAssistantFormFieldContribution>,
    #[serde(default)]
    pub legacy_assistant_type: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginViewContribution {
    pub id: String,
    pub location: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionContribution {
    pub id: String,
    pub location: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub order: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginAssistantFormFieldContribution {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tooltip: Option<String>,
    #[serde(default)]
    pub default_value: Option<JsonValue>,
    #[serde(default)]
    pub options: Vec<PluginSelectOption>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginSelectOption {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub tooltip: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntimeManifest {
    #[serde(rename = "type")]
    pub runtime_type: String,
    pub entry: String,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub checksum: Option<String>,
}

impl Default for PluginRuntimeManifest {
    fn default() -> Self {
        Self {
            runtime_type: "js".to_string(),
            entry: "dist/main.js".to_string(),
            protocol: None,
            checksum: None,
        }
    }
}

fn default_hook_kind() -> String {
    "event".to_string()
}

fn default_hook_priority() -> i64 {
    100
}

fn default_hook_timeout_ms() -> i64 {
    3000
}

fn default_hook_failure_policy() -> String {
    "log".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginHookContribution {
    pub name: String,
    #[serde(default = "default_hook_kind")]
    pub kind: String,
    #[serde(default = "default_hook_priority")]
    pub priority: i64,
    #[serde(default = "default_hook_timeout_ms")]
    pub timeout_ms: i64,
    #[serde(default = "default_hook_failure_policy")]
    pub failure_policy: String,
    #[serde(default = "default_hook_active")]
    pub is_active: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginBangContribution {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub complete: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub bang_type: Option<String>,
    pub executor: PluginBangExecutor,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginBangExecutor {
    #[serde(rename_all = "camelCase")]
    BuiltinTool {
        #[serde(default)]
        command: Option<String>,
        tool_name: String,
        #[serde(default)]
        arguments: HashMap<String, PluginBangArgumentSpec>,
    },
    #[serde(rename_all = "camelCase")]
    McpTool {
        server: String,
        tool_name: String,
        #[serde(default)]
        arguments: HashMap<String, PluginBangArgumentSpec>,
    },
    #[serde(rename_all = "camelCase")]
    PluginMcpTool {
        server: PluginBangServerDefinition,
        tool_name: String,
        #[serde(default)]
        arguments: HashMap<String, PluginBangArgumentSpec>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginBangServerDefinition {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub transport_type: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub environment_variables: HashMap<String, String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub timeout: Option<i32>,
    #[serde(default)]
    pub is_long_running: bool,
    #[serde(default)]
    pub proxy_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PluginBangArgumentSpec {
    pub source: PluginBangArgumentSource,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub value_type: Option<PluginBangArgumentValueType>,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum PluginBangArgumentSource {
    Raw,
    Arg,
    FirstArg,
    Named,
    Context,
    Const,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum PluginBangArgumentValueType {
    String,
    Number,
    Boolean,
    Json,
}

#[derive(Debug, Clone)]
pub struct ResolvedPluginManifest {
    pub plugin_id: Option<i64>,
    pub code: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub plugin_type: Vec<String>,
    pub permissions: Vec<String>,
    pub runtime: PluginRuntimeManifest,
    pub activation_events: Vec<String>,
    pub contributions: PluginContributions,
    pub plugin_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialPlugin {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, alias = "pluginType", alias = "pluginTypes")]
    pub plugin_types: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub min_aipp_version: Option<String>,
    #[serde(default)]
    pub source: Option<PluginInstallRecipeSource>,
    #[serde(default)]
    pub dirs: Vec<PluginInstallRecipeDir>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub is_experimental: bool,
    #[serde(default)]
    pub is_installed: bool,
    #[serde(default)]
    pub installed_version: Option<String>,
    #[serde(default)]
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInstallRecipeSource {
    #[serde(rename = "type", alias = "source_type")]
    pub source_type: PluginInstallRecipeSourceType,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(rename = "ref", alias = "git_ref", default = "default_git_ref")]
    pub git_ref: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginInstallRecipeSourceType {
    #[serde(rename = "github", alias = "git_hub")]
    GitHub,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "localZip", alias = "local_zip", alias = "file")]
    LocalZip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInstallRecipeDir {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallValidation {
    pub is_installable: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallPlanPlugin {
    pub from: String,
    pub to: String,
    pub code: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    pub detected_manifest_file: String,
    pub detected_entry_file: String,
    pub normalized_entry_file: String,
    pub plugin_type: Vec<String>,
    pub permissions: Vec<String>,
    pub runtime: PluginRuntimeManifest,
    pub contributions: PluginContributions,
    pub will_replace: bool,
    pub installed_version: Option<String>,
    pub validation: PluginInstallValidation,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArchiveInspection {
    pub source: PluginInstallRecipeSource,
    pub source_label: String,
    pub download_url: String,
    pub target_directory: String,
    pub archive_sha256: String,
    pub plugins: Vec<PluginInstallPlanPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginArchiveInstallResult {
    pub source: PluginInstallRecipeSource,
    pub source_label: String,
    pub download_url: String,
    pub target_directory: String,
    pub archive_sha256: String,
    pub installed_plugins: Vec<PluginInstallPlanPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetailItem {
    pub plugin_id: Option<i64>,
    pub code: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub plugin_type: Vec<String>,
    pub permissions: Vec<String>,
    pub runtime: PluginRuntimeManifest,
    pub contributions: PluginContributions,
    pub is_installed: bool,
    pub is_active: bool,
    pub plugin_dir: String,
    pub entry_path: String,
    pub entry_sha256: Option<String>,
    pub configs: Vec<PluginConfigItem>,
    pub hook_registrations: Vec<PluginHookRegistrationItem>,
}

fn default_git_ref() -> String {
    "main".to_string()
}

fn default_hook_active() -> bool {
    true
}

fn emit_plugin_registry_changed(app_handle: &tauri::AppHandle, reason: &str) {
    let payload = PluginRegistryChangedEvent { reason: reason.to_string() };
    if let Err(e) = app_handle.emit("plugin_registry_changed", payload) {
        warn!(error = %e, "Failed to emit plugin_registry_changed");
    }
}

fn get_plugin_root_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let plugin_root = app_data_dir.join("plugin");
    fs::create_dir_all(&plugin_root).map_err(|e| e.to_string())?;
    Ok(plugin_root)
}

fn plugin_entry_exists(app_handle: &tauri::AppHandle, code: &str) -> bool {
    match get_plugin_root_path(app_handle) {
        Ok(root) => {
            let plugin_dir = root.join(code);
            let manifest = read_plugin_manifest(&plugin_dir.join("plugin.json"));
            let runtime = manifest
                .as_ref()
                .map(resolve_runtime_manifest)
                .unwrap_or_default();
            plugin_dir.join(runtime.entry).is_file()
        }
        Err(_) => false,
    }
}

fn normalize_plugin_type_name(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    let key = value.to_ascii_lowercase();
    let normalized = match key.as_str() {
        "assistant" | "assistanttype" => "assistantType".to_string(),
        "ui" | "interface" | "interfacetype" => "interfaceType".to_string(),
        "worker" | "application" | "applicationtype" => "applicationType".to_string(),
        "theme" | "themetype" => "themeType".to_string(),
        "markdown" | "markdowntype" => "markdownType".to_string(),
        "message" | "messagetype" => "messageType".to_string(),
        "tool" | "tooltype" => "toolType".to_string(),
        "export" | "exporttype" => "exportType".to_string(),
        _ => value.to_string(),
    };
    Some(normalized)
}

fn normalize_plugin_types(raw_values: &[String]) -> Vec<String> {
    let mut unique = HashSet::new();
    let mut normalized = Vec::new();
    for value in raw_values {
        if let Some(item) = normalize_plugin_type_name(value) {
            let key = item.to_ascii_lowercase();
            if unique.insert(key) {
                normalized.push(item);
            }
        }
    }
    if normalized.is_empty() {
        vec!["assistantType".to_string()]
    } else {
        normalized
    }
}

fn normalize_permissions(raw_values: &[String]) -> Vec<String> {
    let mut unique = HashSet::new();
    let mut normalized = Vec::new();
    for value in raw_values {
        let permission = value.trim().to_ascii_lowercase();
        if permission.is_empty() {
            continue;
        }
        if unique.insert(permission.clone()) {
            normalized.push(permission);
        }
    }
    normalized
}

fn permission_matches(granted: &str, required: &str) -> bool {
    if granted == "*" || granted == required {
        return true;
    }
    if let Some(prefix) = granted.strip_suffix(".*") {
        return required.starts_with(&format!("{}.", prefix));
    }
    false
}

fn assert_plugin_permission(
    app_handle: &tauri::AppHandle,
    plugin_id: i64,
    permission: &str,
) -> Result<ResolvedPluginManifest, String> {
    let db = PluginDatabase::new(app_handle).map_err(|e| e.to_string())?;
    sync_registry(&db, app_handle)?;
    let plugin = db
        .get_plugin(plugin_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Plugin not found: {}", plugin_id))?;
    let is_active = db
        .get_plugin_status(plugin_id)
        .map_err(|e| e.to_string())?
        .map(|status| status.is_active)
        .unwrap_or(false);
    if !is_active {
        return Err(format!("Plugin is disabled: {}", plugin.folder_name));
    }
    let mut manifest = resolve_plugin_manifest_for_code(app_handle, &plugin.folder_name)
        .ok_or_else(|| format!("Plugin manifest not found or invalid: {}", plugin.folder_name))?;
    let required = permission.trim().to_ascii_lowercase();
    let has_permission =
        manifest.permissions.iter().any(|granted| permission_matches(granted, &required));
    if !has_permission {
        return Err(format!(
            "Plugin '{}' lacks required permission '{}'",
            plugin.folder_name, required
        ));
    }
    manifest.plugin_id = Some(plugin_id);
    Ok(manifest)
}

fn normalize_plugin_database_name(database: &str) -> Result<(&'static str, &'static str, String), String> {
    let normalized = database.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "conversation" | "scheduled_task" | "scheduled-tasks" | "scheduledtasks" => Ok((
            "conversation",
            "conversation.db",
            "data.read.conversation".to_string(),
        )),
        "assistant" => Ok(("assistant", "assistant.db", "data.read.assistant".to_string())),
        "llm" | "model" | "models" => Ok(("llm", "llm.db", "data.read.llm".to_string())),
        "mcp" => Ok(("mcp", "mcp.db", "data.read.mcp".to_string())),
        "plugin" => Ok(("plugin", "plugin.db", "data.read.plugin".to_string())),
        "system" => Ok(("system", "system.db", "data.read.system".to_string())),
        "artifacts" | "artifact" => Ok(("artifacts", "artifacts.db", "data.read.artifacts".to_string())),
        _ => Err(format!("Unsupported plugin data database: {}", database)),
    }
}

fn validate_single_statement(sql: &str) -> Result<String, String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err("SQL cannot be empty".to_string());
    }
    if trimmed.contains(';') {
        return Err("Only a single SQL statement without semicolons is allowed".to_string());
    }
    Ok(trimmed.to_string())
}

fn validate_readonly_sql(sql: &str) -> Result<String, String> {
    let trimmed = validate_single_statement(sql)?;
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("select") || lower.starts_with("with")) {
        return Err("Only SELECT or WITH read-only queries are allowed".to_string());
    }
    Ok(trimmed)
}

fn validate_storage_execute_sql(sql: &str) -> Result<String, String> {
    let trimmed = validate_single_statement(sql)?;
    let lower = trimmed.to_ascii_lowercase();
    let forbidden = ["attach", "detach", "pragma writable_schema", "vacuum into"];
    if forbidden.iter().any(|keyword| lower.contains(keyword)) {
        return Err("Storage SQL contains a forbidden operation".to_string());
    }
    Ok(trimmed)
}

fn clamp_query_max_rows(max_rows: Option<usize>) -> usize {
    max_rows
        .unwrap_or(DEFAULT_PLUGIN_QUERY_MAX_ROWS)
        .clamp(1, ABSOLUTE_PLUGIN_QUERY_MAX_ROWS)
}

fn json_to_sql_param(value: &JsonValue) -> Box<dyn ToSql> {
    match value {
        JsonValue::Null => Box::new(rusqlite::types::Null),
        JsonValue::Bool(value) => Box::new(*value),
        JsonValue::Number(number) => {
            if let Some(value) = number.as_i64() {
                Box::new(value)
            } else if let Some(value) = number.as_f64() {
                Box::new(value)
            } else {
                Box::new(number.to_string())
            }
        }
        JsonValue::String(value) => Box::new(value.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => Box::new(value.to_string()),
    }
}

fn row_value_to_json(row: &rusqlite::Row, index: usize) -> Result<JsonValue, String> {
    use base64::Engine;
    use rusqlite::types::ValueRef;

    let value = row.get_ref(index).map_err(|e| format!("Failed to read column: {}", e))?;
    Ok(match value {
        ValueRef::Null => JsonValue::Null,
        ValueRef::Integer(value) => JsonValue::Number(value.into()),
        ValueRef::Real(value) => serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        ValueRef::Text(value) => {
            let text = std::str::from_utf8(value).map_err(|e| format!("Invalid UTF-8: {}", e))?;
            JsonValue::String(text.to_string())
        }
        ValueRef::Blob(value) => JsonValue::String(format!(
            "base64:{}",
            base64::engine::general_purpose::STANDARD.encode(value)
        )),
    })
}

fn execute_readonly_query(
    conn: &Connection,
    sql: &str,
    params: Vec<JsonValue>,
    max_rows: usize,
) -> Result<PluginSqlQueryResult, String> {
    let sql = validate_readonly_sql(sql)?;
    let mut stmt =
        conn.prepare(&sql).map_err(|e| format!("Failed to prepare plugin query: {}", e))?;
    if !stmt.readonly() {
        return Err("Only read-only queries are allowed".to_string());
    }

    let columns: Vec<String> = stmt.column_names().iter().map(|name| name.to_string()).collect();
    let params_vec: Vec<Box<dyn ToSql>> = params.iter().map(json_to_sql_param).collect();
    let param_refs: Vec<&dyn ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();
    let mut rows = stmt
        .query(param_refs.as_slice())
        .map_err(|e| format!("Failed to execute plugin query: {}", e))?;
    let mut result_rows = Vec::new();
    let mut truncated = false;

    while let Some(row) = rows.next().map_err(|e| format!("Failed to fetch query row: {}", e))? {
        if result_rows.len() >= max_rows {
            truncated = true;
            break;
        }
        let mut row_values = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            row_values.push(row_value_to_json(row, index)?);
        }
        result_rows.push(row_values);
    }

    Ok(PluginSqlQueryResult {
        row_count: result_rows.len(),
        columns,
        rows: result_rows,
        truncated,
    })
}

fn execute_storage_statement(
    conn: &Connection,
    sql: &str,
    params: Vec<JsonValue>,
) -> Result<PluginSqlExecuteResult, String> {
    let sql = validate_storage_execute_sql(sql)?;
    let params_vec: Vec<Box<dyn ToSql>> = params.iter().map(json_to_sql_param).collect();
    let param_refs: Vec<&dyn ToSql> = params_vec.iter().map(|value| value.as_ref()).collect();
    let rows_affected = conn
        .execute(&sql, param_refs.as_slice())
        .map_err(|e| format!("Failed to execute plugin storage statement: {}", e))?;
    Ok(PluginSqlExecuteResult {
        rows_affected,
        last_insert_rowid: conn.last_insert_rowid(),
    })
}

fn quote_sql_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn read_database_schema(conn: &Connection, database: &str) -> Result<PluginDatabaseSchema, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type, sql
             FROM sqlite_master
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'
             ORDER BY type ASC, name ASC",
        )
        .map_err(|e| format!("Failed to prepare schema query: {}", e))?;
    let table_rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| format!("Failed to query database schema: {}", e))?;

    let mut tables = Vec::new();
    for table_row in table_rows {
        let (name, object_type, sql) =
            table_row.map_err(|e| format!("Failed to read schema row: {}", e))?;
        let pragma_sql = format!("PRAGMA table_info({})", quote_sql_identifier(&name));
        let mut column_stmt = conn
            .prepare(&pragma_sql)
            .map_err(|e| format!("Failed to prepare column schema query: {}", e))?;
        let column_rows = column_stmt
            .query_map([], |row| {
                Ok(PluginDatabaseColumnSchema {
                    name: row.get(1)?,
                    data_type: row.get(2)?,
                    not_null: row.get::<_, i64>(3)? != 0,
                    default_value: row.get(4)?,
                    primary_key: row.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| format!("Failed to query table columns: {}", e))?;
        let columns = column_rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read table columns: {}", e))?;
        tables.push(PluginDatabaseTableSchema { name, object_type, sql, columns });
    }

    Ok(PluginDatabaseSchema { database: database.to_string(), tables })
}

fn open_readonly_app_database(
    app_handle: &tauri::AppHandle,
    db_file: &str,
) -> Result<Connection, String> {
    let db_path = crate::db::get_db_path(app_handle, db_file)?;
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open read-only plugin data database: {}", e))
}

fn validate_plugin_code_for_storage(code: &str) -> Result<(), String> {
    if code.is_empty() {
        return Err("Plugin code cannot be empty".to_string());
    }
    if !code.chars().all(|value| value.is_ascii_alphanumeric() || value == '-' || value == '_') {
        return Err(format!("Plugin code is not valid for private storage: {}", code));
    }
    Ok(())
}

fn open_plugin_storage_database(
    app_handle: &tauri::AppHandle,
    manifest: &ResolvedPluginManifest,
) -> Result<Connection, String> {
    validate_plugin_code_for_storage(&manifest.code)?;
    let app_data_dir = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    let storage_dir = app_data_dir.join("plugin_data");
    fs::create_dir_all(&storage_dir).map_err(|e| e.to_string())?;
    let db_path = storage_dir.join(format!("{}.db", manifest.code));
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open plugin private storage database: {}", e))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .map_err(|e| format!("Failed to initialize plugin private storage database: {}", e))?;
    Ok(conn)
}

fn parse_plugin_types(raw: Option<&str>) -> Vec<String> {
    let Some(raw_value) = raw else {
        return vec!["assistantType".to_string()];
    };
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return vec!["assistantType".to_string()];
    }

    if let Ok(types) = serde_json::from_str::<Vec<String>>(trimmed) {
        return normalize_plugin_types(&types);
    }

    let csv_types: Vec<String> = trimmed
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();
    normalize_plugin_types(&csv_types)
}

fn plugin_types_to_json(types: &[String]) -> Result<Option<String>, String> {
    let normalized = normalize_plugin_types(types);
    serde_json::to_string(&normalized).map(Some).map_err(|e| e.to_string())
}

fn normalize_runtime_type(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return "js".to_string();
    }
    match normalized.as_str() {
        "wasm" | "rust-wasm" | "rustwasm" => "wasm".to_string(),
        "process" | "stdio" => "process".to_string(),
        "native" => "native".to_string(),
        "mock" => "mock".to_string(),
        "js" | "javascript" => "js".to_string(),
        _ => normalized,
    }
}

fn resolve_runtime_manifest(manifest: &PluginManifest) -> PluginRuntimeManifest {
    let mut runtime = manifest.runtime.clone().unwrap_or_else(|| PluginRuntimeManifest {
        runtime_type: "js".to_string(),
        entry: manifest.entry.clone().unwrap_or_else(|| "dist/main.js".to_string()),
        protocol: None,
        checksum: None,
    });
    runtime.runtime_type = normalize_runtime_type(&runtime.runtime_type);
    if runtime.entry.trim().is_empty() {
        runtime.entry = "dist/main.js".to_string();
    }
    runtime
}

fn hook_contributions_to_registrations(
    hooks: &[PluginHookContribution],
) -> Vec<NewPluginHookRegistration> {
    hooks
        .iter()
        .filter(|hook| !hook.name.trim().is_empty())
        .map(|hook| NewPluginHookRegistration {
            hook_name: hook.name.trim().to_string(),
            hook_kind: hook.kind.trim().to_ascii_lowercase(),
            priority: hook.priority,
            timeout_ms: hook.timeout_ms.max(1),
            failure_policy: hook.failure_policy.trim().to_ascii_lowercase(),
            is_active: hook.is_active,
        })
        .collect()
}

fn read_plugin_manifest(path: &Path) -> Option<PluginManifest> {
    if !path.is_file() {
        return None;
    }

    let raw = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            warn!(error = %e, manifest = %path.display(), "Failed to read plugin manifest");
            return None;
        }
    };

    match serde_json::from_str::<PluginManifest>(&raw) {
        Ok(manifest) => Some(manifest),
        Err(e) => {
            warn!(error = %e, manifest = %path.display(), "Failed to parse plugin manifest");
            None
        }
    }
}

fn resolve_plugin_manifest_from_dir(dir_path: &Path, code: &str) -> Option<ResolvedPluginManifest> {
    let manifest = read_plugin_manifest(&dir_path.join("plugin.json"));
    let (
        name,
        version,
        description,
        author,
        plugin_type,
        permissions,
        runtime,
        activation_events,
        contributions,
    ) =
        if let Some(manifest_data) = manifest {
            let runtime = resolve_runtime_manifest(&manifest_data);
            let mut raw_types = manifest_data.plugin_types;
            raw_types.extend(manifest_data.kinds);

            let declared_code =
                manifest_data.code.or(manifest_data.id).unwrap_or_else(|| code.to_string());
            if declared_code != code {
                warn!(
                    folder = %code,
                    manifest_code = %declared_code,
                    "Plugin folder code and manifest code mismatch, using folder code"
                );
            }

            (
                manifest_data.name.unwrap_or_else(|| code.to_string()),
                manifest_data.version.unwrap_or_else(|| "0.0.0".to_string()),
                manifest_data.description,
                manifest_data.author,
                normalize_plugin_types(&raw_types),
                normalize_permissions(&manifest_data.permissions),
                runtime,
                manifest_data.activation_events,
                manifest_data.contributions,
            )
        } else {
            let runtime = PluginRuntimeManifest::default();
            (
                code.to_string(),
                "0.0.0".to_string(),
                None,
                None,
                vec!["assistantType".to_string()],
                Vec::new(),
                runtime,
                vec!["onStartup:ui".to_string()],
                PluginContributions::default(),
            )
        };

    if !dir_path.join(&runtime.entry).is_file() {
        warn!(
            plugin_code = %code,
            entry = %runtime.entry,
            "Plugin runtime entry is missing"
        );
        return None;
    }

    Some(ResolvedPluginManifest {
        plugin_id: None,
        code: code.to_string(),
        name,
        version,
        description,
        author,
        plugin_type,
        permissions,
        runtime,
        activation_events,
        contributions,
        plugin_dir: dir_path.to_path_buf(),
    })
}

fn resolve_plugin_manifest_for_code(
    app_handle: &tauri::AppHandle,
    code: &str,
) -> Option<ResolvedPluginManifest> {
    get_plugin_root_path(app_handle)
        .ok()
        .and_then(|root| resolve_plugin_manifest_from_dir(&root.join(code), code))
}

impl PluginInstallRecipeSource {
    fn validate(&self) -> Result<(), String> {
        match self.source_type {
            PluginInstallRecipeSourceType::GitHub => {
                let repo =
                    self.repo.as_deref().ok_or_else(|| "GitHub source requires repo".to_string())?;
                validate_github_repo(repo)?;
                if self.git_ref.trim().is_empty() {
                    return Err("Git ref cannot be empty".to_string());
                }
            }
            PluginInstallRecipeSourceType::Zip => {
                let url = self.url.as_deref().ok_or_else(|| "Zip source requires url".to_string())?;
                validate_zip_url(url)?;
            }
            PluginInstallRecipeSourceType::LocalZip => {
                let path = self
                    .path
                    .as_deref()
                    .ok_or_else(|| "Local zip source requires path".to_string())?;
                validate_local_zip_path(path)?;
            }
        }
        Ok(())
    }

    fn archive_url(&self) -> Result<String, String> {
        self.validate()?;
        match self.source_type {
            PluginInstallRecipeSourceType::GitHub => Ok(format!(
                "https://codeload.github.com/{}/zip/{}",
                self.repo.as_deref().unwrap_or_default(),
                self.git_ref
            )),
            PluginInstallRecipeSourceType::Zip => Ok(self.url.clone().unwrap_or_default()),
            PluginInstallRecipeSourceType::LocalZip => Ok(self.path.clone().unwrap_or_default()),
        }
    }

    fn source_label(&self) -> String {
        match self.source_type {
            PluginInstallRecipeSourceType::GitHub => {
                format!("{}#{}", self.repo.as_deref().unwrap_or_default(), self.git_ref)
            }
            PluginInstallRecipeSourceType::Zip => self.url.clone().unwrap_or_default(),
            PluginInstallRecipeSourceType::LocalZip => self.path.clone().unwrap_or_default(),
        }
    }
}

impl OfficialPlugin {
    fn resolve_source(&self) -> Result<PluginInstallRecipeSource, String> {
        if let Some(source) = &self.source {
            source.validate()?;
            return Ok(source.clone());
        }
        if let Some(download_url) = &self.download_url {
            let source = PluginInstallRecipeSource {
                source_type: PluginInstallRecipeSourceType::Zip,
                repo: None,
                git_ref: default_git_ref(),
                url: Some(download_url.clone()),
                path: None,
            };
            source.validate()?;
            return Ok(source);
        }
        Err(format!("Official plugin {} is missing source information", self.id))
    }

    fn normalize(mut self, app_handle: &tauri::AppHandle) -> Result<Self, String> {
        let source = self.resolve_source()?;
        validate_plugin_install_dirs(&self.dirs, false)?;
        if self.source_url.is_none() {
            self.source_url = Some(official_plugin_source_url(&source));
        }
        self.source = Some(source);
        self.plugin_types = normalize_plugin_types(&self.plugin_types);
        self.permissions = normalize_permissions(&self.permissions);
        enrich_official_plugin_install_state(&mut self, app_handle)?;
        Ok(self)
    }
}

struct TempPluginExtractDir {
    path: PathBuf,
}

impl TempPluginExtractDir {
    fn new(prefix: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|e| {
            format!("Failed to create temporary plugin extraction directory {}: {}", path.display(), e)
        })?;
        Ok(Self { path })
    }
}

impl Drop for TempPluginExtractDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct DownloadedPluginArchive {
    _temp_dir: TempPluginExtractDir,
    repo_root: PathBuf,
    download_url: String,
    archive_sha256: String,
}

async fn resolve_plugin_proxy_url(
    app_handle: &tauri::AppHandle,
    use_proxy: bool,
) -> Result<Option<String>, String> {
    if !use_proxy {
        return Ok(None);
    }
    let feature_config_state = app_handle.state::<crate::FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await;
    let proxy_url = get_network_proxy_from_config(&config_feature_map);
    if proxy_url.is_none() {
        return Err("当前未配置网络代理，请先在网络设置中填写代理地址".to_string());
    }
    Ok(proxy_url)
}

fn build_plugin_archive_http_client(proxy_url: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().user_agent("AIPP plugin installer");
    if let Some(proxy_url) = proxy_url {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| format!("代理配置失败: {}", e))?;
        builder = builder.proxy(proxy);
    }
    builder.build().map_err(|e| format!("Failed to build HTTP client: {}", e))
}

async fn download_and_extract_plugin_source(
    source: &PluginInstallRecipeSource,
    expected_sha256: Option<&str>,
    proxy_url: Option<&str>,
) -> Result<DownloadedPluginArchive, String> {
    source.validate()?;
    let download_url = source.archive_url()?;
    let archive_bytes = if source.source_type == PluginInstallRecipeSourceType::LocalZip {
        let bytes = fs::read(&download_url)
            .map_err(|e| format!("读取本地插件 ZIP 失败：{}（{}）", download_url, e))?;
        bytes.into()
    } else {
        let client = build_plugin_archive_http_client(proxy_url)?;
        let response = client
            .get(&download_url)
            .timeout(Duration::from_secs(PLUGIN_ARCHIVE_DOWNLOAD_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| format_plugin_archive_download_error(source, &download_url, &e))?;

        if !response.status().is_success() {
            return Err(format!(
                "下载插件压缩包失败：{} 返回 HTTP {}。可以尝试检查链接或使用代理后重试",
                download_url,
                response.status()
            ));
        }

        response
            .bytes()
            .await
            .map_err(|e| format!("读取下载的插件压缩包失败：{}（{}）", download_url, e))?
    };
    let actual_sha256 = format!("sha256:{}", hex::encode(Sha256::digest(archive_bytes.as_ref())));
    if let Some(expected_sha256) = expected_sha256.filter(|value| !value.trim().is_empty()) {
        verify_sha256_value(&actual_sha256, expected_sha256)?;
    }

    let temp_dir = TempPluginExtractDir::new("plugin_archive_extract")?;
    extract_zip_bytes_to_dir(archive_bytes.as_ref(), &temp_dir.path)?;
    let repo_root = resolve_plugin_archive_root(&temp_dir.path)?;
    Ok(DownloadedPluginArchive {
        _temp_dir: temp_dir,
        repo_root,
        download_url,
        archive_sha256: actual_sha256,
    })
}

fn format_plugin_archive_download_error(
    source: &PluginInstallRecipeSource,
    download_url: &str,
    error: &reqwest::Error,
) -> String {
    let source_label = source.source_label();
    if error.is_timeout() {
        format!(
            "下载插件压缩包超时（{} 秒）：{}（{}）。可以尝试使用代理后重试",
            PLUGIN_ARCHIVE_DOWNLOAD_TIMEOUT_SECS, source_label, download_url
        )
    } else if error.is_connect() {
        format!(
            "连接插件压缩包地址失败：{}（{}）。请检查网络或尝试使用代理后重试",
            source_label, download_url
        )
    } else {
        format!("下载插件压缩包失败：{}（{}）：{}", source_label, download_url, error)
    }
}

fn normalize_sha256_value(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or_else(|| value.trim())
        .trim()
        .to_ascii_lowercase()
}

fn verify_sha256_value(actual: &str, expected: &str) -> Result<(), String> {
    let actual = normalize_sha256_value(actual);
    let expected = normalize_sha256_value(expected);
    if actual == expected {
        Ok(())
    } else {
        Err(format!("插件包校验失败：expected {}, got {}", expected, actual))
    }
}

fn official_plugin_source_url(source: &PluginInstallRecipeSource) -> String {
    match source.source_type {
        PluginInstallRecipeSourceType::GitHub => format!(
            "https://github.com/{}/tree/{}",
            source.repo.as_deref().unwrap_or_default(),
            source.git_ref
        ),
        PluginInstallRecipeSourceType::Zip => source.url.clone().unwrap_or_default(),
        PluginInstallRecipeSourceType::LocalZip => source.path.clone().unwrap_or_default(),
    }
}

fn enrich_official_plugin_install_state(
    plugin: &mut OfficialPlugin,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let db = PluginDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let plugins = db.get_plugins().map_err(|e| e.to_string())?;
    if let Some(local) = plugins.into_iter().find(|item| item.folder_name == plugin.code) {
        plugin.is_installed = plugin_entry_exists(app_handle, &plugin.code);
        plugin.installed_version = Some(local.version);
        plugin.is_active = db
            .get_plugin_status(local.plugin_id)
            .map_err(|e| e.to_string())?
            .map(|status| status.is_active)
            .unwrap_or(true);
    }
    Ok(())
}

fn validate_plugin_install_dirs(
    dirs: &[PluginInstallRecipeDir],
    require_non_empty: bool,
) -> Result<(), String> {
    if require_non_empty && dirs.is_empty() {
        return Err("At least one plugin directory mapping is required".to_string());
    }
    let mut seen_targets = HashSet::new();
    for dir in dirs {
        validate_relative_source_path(&dir.from)?;
        validate_target_dir_name(&dir.to)?;
        if !seen_targets.insert(dir.to.clone()) {
            return Err(format!("Duplicate plugin target directory: {}", dir.to));
        }
    }
    Ok(())
}

fn validate_github_repo(repo: &str) -> Result<(), String> {
    let mut segments = repo.split('/');
    let owner = segments.next().unwrap_or_default().trim();
    let name = segments.next().unwrap_or_default().trim();
    if owner.is_empty() || name.is_empty() || segments.next().is_some() {
        return Err(format!("GitHub source repo must be owner/repo, got: {}", repo));
    }
    Ok(())
}

fn validate_zip_url(url: &str) -> Result<(), String> {
    let parsed =
        reqwest::Url::parse(url.trim()).map_err(|e| format!("Zip source url is invalid: {}", e))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(format!("Zip source url must use http or https, got scheme: {}", scheme)),
    }
}

fn validate_local_zip_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Local zip path cannot be empty".to_string());
    }
    let path = Path::new(trimmed);
    if !path.is_file() {
        return Err(format!("Local zip file does not exist: {}", trimmed));
    }
    if path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| !value.eq_ignore_ascii_case("zip"))
        .unwrap_or(true)
    {
        return Err(format!("Local plugin archive must be a .zip file: {}", trimmed));
    }
    Ok(())
}

fn validate_relative_source_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Plugin source directory cannot be empty".to_string());
    }
    if trimmed == "." {
        return Ok(());
    }
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(format!("Invalid plugin source directory path: {}", path)),
        }
    }
    Ok(())
}

fn validate_target_dir_name(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Plugin target directory cannot be empty".to_string());
    }
    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(format!("Plugin target directory must be a single directory name, got: {}", path)),
    }
}

fn resolve_plugin_archive_root(extract_dir: &Path) -> Result<PathBuf, String> {
    let entries = collect_plugin_directory_entries(extract_dir)?;
    if entries.is_empty() {
        return Err(format!("Extracted plugin archive directory {} is empty", extract_dir.display()));
    }
    let has_files = entries.iter().any(|path| path.is_file());
    let dirs: Vec<PathBuf> = entries.into_iter().filter(|path| path.is_dir()).collect();
    if dirs.len() == 1 && !has_files {
        Ok(dirs[0].clone())
    } else {
        Ok(extract_dir.to_path_buf())
    }
}

fn collect_plugin_directory_entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)
        .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        entries.push(entry.path());
    }
    entries.sort();
    Ok(entries)
}

fn resolve_plugin_recipe_dir(repo_root: &Path, from: &str) -> Result<PathBuf, String> {
    validate_relative_source_path(from)?;
    let resolved = if from.trim() == "." { repo_root.to_path_buf() } else { repo_root.join(from) };
    if !resolved.exists() {
        return Err(format!("Configured plugin directory {} does not exist in archive", from));
    }
    if !resolved.is_dir() {
        return Err(format!("Configured plugin directory {} is not a directory in archive", from));
    }
    let canonical_root = repo_root
        .canonicalize()
        .map_err(|e| format!("Failed to resolve archive root {}: {}", repo_root.display(), e))?;
    let canonical_resolved = resolved
        .canonicalize()
        .map_err(|e| format!("Failed to resolve plugin directory {}: {}", resolved.display(), e))?;
    if !canonical_resolved.starts_with(&canonical_root) {
        return Err(format!("Configured plugin directory {} resolves outside archive root", from));
    }
    Ok(resolved)
}

fn build_archive_plan_plugins(
    configured_dirs: Option<&[PluginInstallRecipeDir]>,
    repo_root: &Path,
    plugin_root: &Path,
) -> Result<Vec<PluginInstallPlanPlugin>, String> {
    if let Some(dirs) = configured_dirs.filter(|dirs| !dirs.is_empty()) {
        validate_plugin_install_dirs(dirs, true)?;
        let mut plugins = Vec::with_capacity(dirs.len());
        for dir in dirs {
            let source_dir = resolve_plugin_recipe_dir(repo_root, &dir.from)?;
            plugins.push(build_plan_plugin(&dir.from, &dir.to, &source_dir, plugin_root)?);
        }
        return Ok(plugins);
    }

    let candidates = discover_archive_plugin_candidates(repo_root)?;
    if candidates.is_empty() {
        return Err("未在压缩包中发现可安装插件，请确认包含 plugin.json 和 dist/main.js".to_string());
    }
    let mut plugins = Vec::with_capacity(candidates.len());
    for (from, source_dir) in candidates {
        let manifest = read_plugin_manifest(&source_dir.join("plugin.json"))
            .ok_or_else(|| format!("Failed to read plugin manifest in {}", source_dir.display()))?;
        let fallback_code = source_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("plugin")
            .to_string();
        let to = manifest_declared_code(&manifest, &fallback_code);
        plugins.push(build_plan_plugin(&from, &to, &source_dir, plugin_root)?);
    }
    plugins.sort_by(|left, right| left.from.cmp(&right.from));
    Ok(plugins)
}

fn discover_archive_plugin_candidates(repo_root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut candidates = Vec::new();
    discover_archive_plugin_candidates_inner(repo_root, repo_root, 0, &mut candidates)?;
    Ok(candidates)
}

fn discover_archive_plugin_candidates_inner(
    repo_root: &Path,
    current_dir: &Path,
    depth: usize,
    candidates: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    if current_dir.join("plugin.json").is_file() {
        candidates.push((archive_relative_path(repo_root, current_dir)?, current_dir.to_path_buf()));
        return Ok(());
    }
    if depth >= PLUGIN_ARCHIVE_MAX_DISCOVERY_DEPTH {
        return Ok(());
    }
    for entry in collect_plugin_directory_entries(current_dir)? {
        if !entry.is_dir() {
            continue;
        }
        let name = entry.file_name().and_then(|value| value.to_str()).unwrap_or_default();
        if IGNORED_PLUGIN_DISCOVERY_SEGMENTS.contains(&name) {
            continue;
        }
        discover_archive_plugin_candidates_inner(repo_root, &entry, depth + 1, candidates)?;
    }
    Ok(())
}

fn archive_relative_path(repo_root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(repo_root)
        .map_err(|e| format!("Failed to build archive relative path: {}", e))?;
    if relative.as_os_str().is_empty() {
        return Ok(".".to_string());
    }
    let parts: Vec<String> = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    Ok(parts.join("/"))
}

fn manifest_declared_code(manifest: &PluginManifest, fallback: &str) -> String {
    manifest
        .code
        .clone()
        .or_else(|| manifest.id.clone())
        .unwrap_or_else(|| fallback.to_string())
}

fn build_plan_plugin(
    from: &str,
    to: &str,
    source_dir: &Path,
    plugin_root: &Path,
) -> Result<PluginInstallPlanPlugin, String> {
    let manifest_path = source_dir.join("plugin.json");
    let manifest = read_plugin_manifest(&manifest_path)
        .ok_or_else(|| format!("插件目录 {} 缺少有效 plugin.json", from))?;
    let runtime = resolve_runtime_manifest(&manifest);
    let fallback_code = source_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(to)
        .to_string();
    let declared_code = manifest_declared_code(&manifest, &fallback_code);
    let mut raw_types = manifest.plugin_types.clone();
    raw_types.extend(manifest.kinds.clone());
    let plugin_type = normalize_plugin_types(&raw_types);
    let permissions = normalize_permissions(&manifest.permissions);
    let entry_path = source_dir.join(&runtime.entry);
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    if declared_code != to {
        errors.push(format!("插件目录名与 plugin.json code 不一致：目录 {}，manifest {}", to, declared_code));
    }
    if !entry_path.is_file() {
        errors.push(format!("插件入口文件不存在：{}/{}", to, runtime.entry));
    }
    if runtime.runtime_type != "js" {
        warnings.push(format!("当前插件商店第一版主要面向 JS runtime，检测到 {}", runtime.runtime_type));
    }

    let installed_version =
        resolve_plugin_manifest_from_dir(&plugin_root.join(to), to).map(|manifest| manifest.version);
    let preview = build_plugin_preview(source_dir, manifest.description.clone())?;
    let detected_entry_file = Path::new(&runtime.entry)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("main.js")
        .to_string();

    Ok(PluginInstallPlanPlugin {
        from: from.to_string(),
        to: to.to_string(),
        code: declared_code,
        name: manifest.name.unwrap_or_else(|| to.to_string()),
        version: manifest.version.unwrap_or_else(|| "0.0.0".to_string()),
        description: manifest.description,
        author: manifest.author,
        detected_manifest_file: "plugin.json".to_string(),
        detected_entry_file,
        normalized_entry_file: runtime.entry.clone(),
        plugin_type,
        permissions,
        runtime,
        contributions: manifest.contributions,
        will_replace: plugin_root.join(to).exists(),
        installed_version,
        validation: PluginInstallValidation {
            is_installable: errors.is_empty(),
            warnings,
            errors,
        },
        preview,
    })
}

fn build_plugin_preview(source_dir: &Path, fallback: Option<String>) -> Result<Option<String>, String> {
    for name in ["README.md", "readme.md", "README.MD"] {
        let path = source_dir.join(name);
        if path.is_file() {
            let raw = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read plugin preview {}: {}", path.display(), e))?;
            return Ok(Some(raw.chars().take(1200).collect()));
        }
    }
    Ok(fallback)
}

fn install_plugin_dir_atomic(source_dir: &Path, plugin_root: &Path, code: &str) -> Result<(), String> {
    fs::create_dir_all(plugin_root)
        .map_err(|e| format!("Failed to create plugin root {}: {}", plugin_root.display(), e))?;
    let staging_dir = plugin_root.join(format!("{}.installing-{}", code, uuid::Uuid::new_v4()));
    let backup_dir = plugin_root.join(format!("{}.backup-{}", code, uuid::Uuid::new_v4()));
    let target_dir = plugin_root.join(code);
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir).map_err(|e| {
            format!("Failed to clean stale staging plugin directory {}: {}", staging_dir.display(), e)
        })?;
    }
    copy_dir_recursive(source_dir, &staging_dir)?;

    let had_existing = target_dir.exists();
    if had_existing {
        fs::rename(&target_dir, &backup_dir).map_err(|e| {
            format!("Failed to backup existing plugin directory {}: {}", target_dir.display(), e)
        })?;
    }

    match fs::rename(&staging_dir, &target_dir) {
        Ok(()) => {
            if had_existing {
                let _ = fs::remove_dir_all(&backup_dir);
            }
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging_dir);
            if had_existing && backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &target_dir);
            }
            Err(format!("Failed to install plugin directory {}: {}", code, error))
        }
    }
}

fn calculate_file_sha256(path: &Path) -> Result<String, String> {
    let bytes =
        fs::read(path).map_err(|e| format!("Failed to read file {}: {}", path.display(), e))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(&bytes))))
}

fn discover_plugins(app_handle: &tauri::AppHandle) -> Result<Vec<DiscoveredPlugin>, String> {
    let plugin_root = get_plugin_root_path(app_handle)?;
    let mut discovered = Vec::new();
    let entries = fs::read_dir(&plugin_root).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if !file_type.is_dir() {
            continue;
        }

        let dir_path = entry.path();
        let code = entry.file_name().to_string_lossy().to_string();
        if code.is_empty() {
            continue;
        }

        let Some(manifest) = resolve_plugin_manifest_from_dir(&dir_path, &code) else {
            continue;
        };

        discovered.push(DiscoveredPlugin {
            code,
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            author: manifest.author,
            plugin_type: manifest.plugin_type,
            runtime: manifest.runtime,
            contributions: manifest.contributions,
        });
    }

    discovered.sort_by(|a, b| a.code.cmp(&b.code));
    Ok(discovered)
}

fn get_plugin_type_value(db: &PluginDatabase, plugin_id: i64) -> Result<Option<String>, String> {
    let configs = db.get_plugin_configurations(plugin_id).map_err(|e| e.to_string())?;
    Ok(configs
        .into_iter()
        .find(|config| config.config_key == PLUGIN_TYPE_CONFIG_KEY)
        .and_then(|config| config.config_value))
}

fn get_plugin_types(db: &PluginDatabase, plugin_id: i64) -> Result<Vec<String>, String> {
    let raw = get_plugin_type_value(db, plugin_id)?;
    Ok(parse_plugin_types(raw.as_deref()))
}

fn plugin_hook_registration_to_item(
    registration: PluginHookRegistration,
) -> PluginHookRegistrationItem {
    PluginHookRegistrationItem {
        id: registration.id,
        plugin_id: registration.plugin_id,
        hook_name: registration.hook_name,
        hook_kind: registration.hook_kind,
        priority: registration.priority,
        timeout_ms: registration.timeout_ms,
        failure_policy: registration.failure_policy,
        is_active: registration.is_active,
    }
}

fn plugin_hook_audit_log_to_item(log: PluginHookAuditLog) -> PluginHookAuditLogItem {
    PluginHookAuditLogItem {
        id: log.id,
        plugin_id: log.plugin_id,
        hook_name: log.hook_name,
        conversation_id: log.conversation_id,
        message_id: log.message_id,
        status: log.status,
        action: log.action,
        duration_ms: log.duration_ms,
        error: log.error,
        created_at: log.created_at.to_rfc3339(),
    }
}

fn plugin_assistant_config_to_item(
    config: PluginAssistantConfiguration,
) -> PluginAssistantConfigItem {
    PluginAssistantConfigItem {
        config_id: config.config_id,
        plugin_id: config.plugin_id,
        assistant_id: config.assistant_id,
        config_key: config.config_key,
        config_value: config.config_value,
        updated_at: config.updated_at.to_rfc3339(),
    }
}

fn normalize_message_type(message_type: &str) -> Result<String, String> {
    let normalized = message_type.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "user" | "assistant" | "response" | "system" | "tool_result" | "error" => {
            Ok(normalized)
        }
        _ => Err(format!("Unsupported plugin message type: {}", message_type)),
    }
}

fn serialize_metadata(metadata: Option<JsonValue>) -> Result<Option<String>, String> {
    match metadata {
        Some(value) => serde_json::to_string(&value)
            .map(Some)
            .map_err(|e| format!("Failed to serialize metadata JSON: {}", e)),
        None => Ok(None),
    }
}

fn emit_conversation_event(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    event_type: &str,
    data: serde_json::Value,
) {
    let event = ConversationEvent {
        r#type: event_type.to_string(),
        data,
    };
    let channel = format!("conversation_event_{}", conversation_id);
    if let Err(error) = app_handle.emit(channel.as_str(), event) {
        warn!(conversation_id, event_type, error = %error, "Failed to emit plugin conversation event");
    }
}

fn ensure_default_plugins(
    db: &PluginDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let defaults: [(&str, &str, &str); 2] =
        [("代码生成", "0.0.0", "code-generate"), ("DeepResearch", "0.0.0", "deepresearch")];
    let existing = db.get_plugins().map_err(|e| e.to_string())?;
    let by_code: HashMap<String, Plugin> =
        existing.into_iter().map(|plugin| (plugin.folder_name.clone(), plugin)).collect();

    for (name, version, code) in defaults {
        if !plugin_entry_exists(app_handle, code) {
            continue;
        }
        if let Some(plugin) = by_code.get(code) {
            if db.get_plugin_status(plugin.plugin_id).map_err(|e| e.to_string())?.is_none() {
                db.upsert_plugin_status(plugin.plugin_id, true, None).map_err(|e| e.to_string())?;
            }
        } else {
            let plugin_id = db
                .add_plugin(name, version, code, Some("AIPP builtin plugin"), Some("AIPP"))
                .map_err(|e| e.to_string())?;
            db.upsert_plugin_status(plugin_id, true, None).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn sync_discovered_plugins(
    db: &PluginDatabase,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let discovered = discover_plugins(app_handle)?;
    let plugins = db.get_plugins().map_err(|e| e.to_string())?;
    let by_code: HashMap<String, Plugin> =
        plugins.into_iter().map(|plugin| (plugin.folder_name.clone(), plugin)).collect();

    for discovered_plugin in discovered {
        let plugin_id = if let Some(mut existing) = by_code.get(&discovered_plugin.code).cloned() {
            existing.name = discovered_plugin.name.clone();
            existing.version = discovered_plugin.version.clone();
            existing.description = discovered_plugin.description.clone();
            existing.author = discovered_plugin.author.clone();
            existing.updated_at = Utc::now();
            db.update_plugin(&existing).map_err(|e| e.to_string())?;
            existing.plugin_id
        } else {
            db.add_plugin(
                &discovered_plugin.name,
                &discovered_plugin.version,
                &discovered_plugin.code,
                discovered_plugin.description.as_deref(),
                discovered_plugin.author.as_deref(),
            )
            .map_err(|e| e.to_string())?
        };

        if db.get_plugin_status(plugin_id).map_err(|e| e.to_string())?.is_none() {
            db.upsert_plugin_status(plugin_id, true, None).map_err(|e| e.to_string())?;
        }

        let plugin_type_value = plugin_types_to_json(&discovered_plugin.plugin_type)?;
        db.set_plugin_configuration(
            plugin_id,
            PLUGIN_TYPE_CONFIG_KEY,
            plugin_type_value.as_deref(),
        )
        .map_err(|e| e.to_string())?;

        db.upsert_plugin_runtime(
            plugin_id,
            &discovered_plugin.runtime.runtime_type,
            &discovered_plugin.runtime.entry,
            discovered_plugin.runtime.protocol.as_deref(),
            discovered_plugin.runtime.checksum.as_deref(),
        )
        .map_err(|e| e.to_string())?;

        let hook_registrations =
            hook_contributions_to_registrations(&discovered_plugin.contributions.hooks);
        db.replace_plugin_hook_registrations(plugin_id, &hook_registrations)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn sync_registry(db: &PluginDatabase, app_handle: &tauri::AppHandle) -> Result<(), String> {
    ensure_default_plugins(db, app_handle)?;
    sync_discovered_plugins(db, app_handle)
}

fn dedupe_plugins_by_code(plugins: Vec<Plugin>) -> Vec<Plugin> {
    let mut seen_codes = HashSet::new();
    plugins.into_iter().filter(|plugin| seen_codes.insert(plugin.folder_name.clone())).collect()
}

fn plugin_to_item(
    db: &PluginDatabase,
    app_handle: &tauri::AppHandle,
    plugin: Plugin,
) -> Result<PluginListItem, String> {
    let status = db.get_plugin_status(plugin.plugin_id).map_err(|e| e.to_string())?;
    let plugin_type = get_plugin_types(db, plugin.plugin_id)?;
    let code = plugin.folder_name.clone();
    let manifest = resolve_plugin_manifest_for_code(app_handle, &code);
    Ok(PluginListItem {
        plugin_id: plugin.plugin_id,
        name: plugin.name,
        version: plugin.version,
        code: code.clone(),
        description: plugin.description,
        author: plugin.author,
        plugin_type,
        permissions: manifest.as_ref().map(|item| item.permissions.clone()).unwrap_or_default(),
        runtime: manifest.as_ref().map(|item| item.runtime.clone()),
        contributions: manifest.map(|item| item.contributions).unwrap_or_default(),
        is_active: status.map(|value| value.is_active).unwrap_or(true),
        is_installed: plugin_entry_exists(app_handle, &code),
    })
}

pub fn get_enabled_plugin_manifests(
    app_handle: &tauri::AppHandle,
) -> Result<Vec<ResolvedPluginManifest>, String> {
    let db = PluginDatabase::new(app_handle).map_err(|e| e.to_string())?;
    sync_registry(&db, app_handle)?;
    let plugins = dedupe_plugins_by_code(db.get_plugins().map_err(|e| e.to_string())?);
    let mut manifests = Vec::new();

    for plugin in plugins {
        let is_active = db
            .get_plugin_status(plugin.plugin_id)
            .map_err(|e| e.to_string())?
            .map(|status| status.is_active)
            .unwrap_or(true);
        if !is_active || !plugin_entry_exists(app_handle, &plugin.folder_name) {
            continue;
        }
        if let Some(mut manifest) = resolve_plugin_manifest_for_code(app_handle, &plugin.folder_name) {
            manifest.plugin_id = Some(plugin.plugin_id);
            manifests.push(manifest);
        }
    }

    Ok(manifests)
}

#[tauri::command]
pub async fn fetch_official_plugins(
    app_handle: tauri::AppHandle,
    use_proxy: bool,
) -> Result<Vec<OfficialPlugin>, String> {
    let proxy_url = resolve_plugin_proxy_url(&app_handle, use_proxy).await?;
    let client = build_plugin_archive_http_client(proxy_url.as_deref())?;
    let response = client
        .get(OFFICIAL_PLUGINS_API)
        .timeout(Duration::from_secs(OFFICIAL_PLUGINS_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("请求超时（超过{}秒），请尝试使用代理访问", OFFICIAL_PLUGINS_TIMEOUT_SECS)
            } else if e.is_connect() {
                "网络连接失败，请检查网络或尝试使用代理".to_string()
            } else {
                format!("获取官方插件列表失败: {}", e)
            }
        })?;

    if !response.status().is_success() {
        return Err(format!("Official plugins API returned error: {}", response.status()));
    }

    let plugins = response
        .json::<Vec<OfficialPlugin>>()
        .await
        .map_err(|e| format!("Failed to parse official plugins response: {}", e))?;
    plugins
        .into_iter()
        .map(|plugin| plugin.normalize(&app_handle))
        .collect::<Result<Vec<_>, _>>()
}

#[tauri::command]
pub async fn inspect_plugin_archive_source(
    app_handle: tauri::AppHandle,
    source: PluginInstallRecipeSource,
    dirs: Option<Vec<PluginInstallRecipeDir>>,
    expected_sha256: Option<String>,
    use_proxy: bool,
) -> Result<PluginArchiveInspection, String> {
    let plugin_root = get_plugin_root_path(&app_handle)?;
    let proxy_url = resolve_plugin_proxy_url(&app_handle, use_proxy).await?;
    let archive = download_and_extract_plugin_source(
        &source,
        expected_sha256.as_deref(),
        proxy_url.as_deref(),
    )
    .await?;
    let plugins = build_archive_plan_plugins(dirs.as_deref(), &archive.repo_root, &plugin_root)?;
    Ok(PluginArchiveInspection {
        source: source.clone(),
        source_label: source.source_label(),
        download_url: archive.download_url,
        target_directory: plugin_root.to_string_lossy().to_string(),
        archive_sha256: archive.archive_sha256,
        plugins,
    })
}

#[tauri::command]
pub async fn install_plugin_archive_source(
    app_handle: tauri::AppHandle,
    source: PluginInstallRecipeSource,
    selections: Vec<PluginInstallRecipeDir>,
    expected_sha256: Option<String>,
    use_proxy: bool,
    enable_after_install: bool,
) -> Result<PluginArchiveInstallResult, String> {
    validate_plugin_install_dirs(&selections, true)?;
    let plugin_root = get_plugin_root_path(&app_handle)?;
    let proxy_url = resolve_plugin_proxy_url(&app_handle, use_proxy).await?;
    let archive = download_and_extract_plugin_source(
        &source,
        expected_sha256.as_deref(),
        proxy_url.as_deref(),
    )
    .await?;
    let plugins = build_archive_plan_plugins(Some(&selections), &archive.repo_root, &plugin_root)?;
    let invalid = plugins.iter().find(|plugin| !plugin.validation.is_installable);
    if let Some(plugin) = invalid {
        return Err(format!(
            "插件 {} 不可安装：{}",
            plugin.to,
            plugin.validation.errors.join("；")
        ));
    }

    for plugin in &plugins {
        let source_dir = resolve_plugin_recipe_dir(&archive.repo_root, &plugin.from)?;
        install_plugin_dir_atomic(&source_dir, &plugin_root, &plugin.to)?;
    }

    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    sync_registry(&db, &app_handle)?;
    if enable_after_install {
        let all_plugins = db.get_plugins().map_err(|e| e.to_string())?;
        for installed in &plugins {
            if let Some(plugin) = all_plugins.iter().find(|item| item.folder_name == installed.to) {
                db.upsert_plugin_status(plugin.plugin_id, true, None).map_err(|e| e.to_string())?;
            }
        }
    }
    emit_plugin_registry_changed(&app_handle, "archive-installed");

    Ok(PluginArchiveInstallResult {
        source: source.clone(),
        source_label: source.source_label(),
        download_url: archive.download_url,
        target_directory: plugin_root.to_string_lossy().to_string(),
        archive_sha256: archive.archive_sha256,
        installed_plugins: plugins,
    })
}

#[tauri::command]
pub async fn get_plugin_detail(
    app_handle: tauri::AppHandle,
    code: String,
) -> Result<PluginDetailItem, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    sync_registry(&db, &app_handle)?;
    let plugin_root = get_plugin_root_path(&app_handle)?;
    let plugin_dir = plugin_root.join(&code);
    let manifest = resolve_plugin_manifest_from_dir(&plugin_dir, &code)
        .ok_or_else(|| format!("Plugin manifest not found or invalid: {}", code))?;
    let plugin = db
        .get_plugins()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|item| item.folder_name == code);
    let plugin_id = plugin.as_ref().map(|item| item.plugin_id);
    let is_active = if let Some(plugin_id) = plugin_id {
        db.get_plugin_status(plugin_id)
            .map_err(|e| e.to_string())?
            .map(|status| status.is_active)
            .unwrap_or(true)
    } else {
        false
    };
    let configs = if let Some(plugin_id) = plugin_id {
        db.get_plugin_configurations(plugin_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|config| PluginConfigItem {
                config_id: config.config_id,
                plugin_id: config.plugin_id,
                config_key: config.config_key,
                config_value: config.config_value,
            })
            .collect()
    } else {
        Vec::new()
    };
    let hook_registrations = if let Some(plugin_id) = plugin_id {
        db.get_plugin_hook_registrations(plugin_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(plugin_hook_registration_to_item)
            .collect()
    } else {
        Vec::new()
    };
    let entry_path = plugin_dir.join(&manifest.runtime.entry);
    let entry_sha256 = if entry_path.is_file() {
        Some(calculate_file_sha256(&entry_path)?)
    } else {
        None
    };

    Ok(PluginDetailItem {
        plugin_id,
        code: manifest.code,
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        author: manifest.author,
        plugin_type: manifest.plugin_type,
        permissions: manifest.permissions,
        runtime: manifest.runtime,
        contributions: manifest.contributions,
        is_installed: plugin_entry_exists(&app_handle, &code),
        is_active,
        plugin_dir: plugin_dir.to_string_lossy().to_string(),
        entry_path: entry_path.to_string_lossy().to_string(),
        entry_sha256,
        configs,
        hook_registrations,
    })
}

#[tauri::command]
pub async fn verify_plugin_entry_checksum(
    app_handle: tauri::AppHandle,
    code: String,
) -> Result<(), String> {
    let manifest = resolve_plugin_manifest_for_code(&app_handle, &code)
        .ok_or_else(|| format!("Plugin manifest not found or invalid: {}", code))?;
    let entry_path = manifest.plugin_dir.join(&manifest.runtime.entry);
    verify_runtime_entry_checksum(&entry_path, manifest.runtime.checksum.as_deref())
}

#[tauri::command]
pub async fn get_plugin_root_dir(app_handle: tauri::AppHandle) -> Result<String, String> {
    let plugin_root = get_plugin_root_path(&app_handle)?;
    Ok(plugin_root.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn list_plugins(app_handle: tauri::AppHandle) -> Result<Vec<PluginListItem>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    sync_registry(&db, &app_handle)?;
    let plugins = dedupe_plugins_by_code(db.get_plugins().map_err(|e| e.to_string())?);
    plugins.into_iter().map(|plugin| plugin_to_item(&db, &app_handle, plugin)).collect()
}

#[tauri::command]
pub async fn get_enabled_plugins(
    app_handle: tauri::AppHandle,
) -> Result<Vec<PluginListItem>, String> {
    let plugins = list_plugins(app_handle.clone()).await?;
    Ok(plugins
        .into_iter()
        .filter(|plugin| plugin.is_active && plugin_entry_exists(&app_handle, &plugin.code))
        .collect())
}

#[tauri::command]
pub async fn install_plugin(
    app_handle: tauri::AppHandle,
    name: String,
    version: String,
    code: String,
    description: Option<String>,
    author: Option<String>,
    plugin_type: Option<Vec<String>>,
) -> Result<i64, String> {
    if !plugin_entry_exists(&app_handle, &code) {
        return Err(format!("Plugin entry not found: {}/dist/main.js", code));
    }

    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let plugins = db.get_plugins().map_err(|e| e.to_string())?;
    let now = Utc::now();
    let plugin_id =
        if let Some(mut existing) = plugins.into_iter().find(|plugin| plugin.folder_name == code) {
            existing.name = name;
            existing.version = version;
            existing.description = description;
            existing.author = author;
            existing.updated_at = now;
            db.update_plugin(&existing).map_err(|e| e.to_string())?;
            existing.plugin_id
        } else {
            db.add_plugin(&name, &version, &code, description.as_deref(), author.as_deref())
                .map_err(|e| e.to_string())?
        };

    db.upsert_plugin_status(plugin_id, true, None).map_err(|e| e.to_string())?;
    let plugin_types =
        normalize_plugin_types(&plugin_type.unwrap_or_else(|| vec!["assistantType".to_string()]));
    let plugin_types_json = plugin_types_to_json(&plugin_types)?;
    db.set_plugin_configuration(plugin_id, PLUGIN_TYPE_CONFIG_KEY, plugin_types_json.as_deref())
        .map_err(|e| e.to_string())?;

    if let Some(manifest) = resolve_plugin_manifest_for_code(&app_handle, &code) {
        db.upsert_plugin_runtime(
            plugin_id,
            &manifest.runtime.runtime_type,
            &manifest.runtime.entry,
            manifest.runtime.protocol.as_deref(),
            manifest.runtime.checksum.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        let hook_registrations = hook_contributions_to_registrations(&manifest.contributions.hooks);
        db.replace_plugin_hook_registrations(plugin_id, &hook_registrations)
            .map_err(|e| e.to_string())?;
    }

    emit_plugin_registry_changed(&app_handle, "installed");
    Ok(plugin_id)
}

#[tauri::command]
pub async fn uninstall_plugin(app_handle: tauri::AppHandle, plugin_id: i64) -> Result<(), String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let plugin = db
        .get_plugin(plugin_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Plugin not found: {}", plugin_id))?;

    let plugin_dir = get_plugin_root_path(&app_handle)?.join(&plugin.folder_name);
    if plugin_dir.exists() {
        fs::remove_dir_all(&plugin_dir).map_err(|e| {
            format!("Failed to remove plugin folder '{}': {}", plugin.folder_name, e)
        })?;
    }

    db.conn
        .execute("DELETE FROM PluginData WHERE plugin_id = ?", params![plugin_id])
        .map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM PluginConfigurations WHERE plugin_id = ?", params![plugin_id])
        .map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM PluginStatus WHERE plugin_id = ?", params![plugin_id])
        .map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM PluginHookRegistration WHERE plugin_id = ?", params![plugin_id])
        .map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM PluginHookAuditLog WHERE plugin_id = ?", params![plugin_id])
        .map_err(|e| e.to_string())?;
    db.conn
        .execute(
            "DELETE FROM PluginAssistantConfigurations WHERE plugin_id = ?",
            params![plugin_id],
        )
        .map_err(|e| e.to_string())?;
    db.conn
        .execute("DELETE FROM PluginRuntime WHERE plugin_id = ?", params![plugin_id])
        .map_err(|e| e.to_string())?;
    db.delete_plugin(plugin_id).map_err(|e| e.to_string())?;

    emit_plugin_registry_changed(&app_handle, "uninstalled");
    Ok(())
}

#[tauri::command]
pub async fn enable_plugin(app_handle: tauri::AppHandle, plugin_id: i64) -> Result<(), String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.upsert_plugin_status(plugin_id, true, None).map_err(|e| e.to_string())?;
    emit_plugin_registry_changed(&app_handle, "enabled");
    Ok(())
}

#[tauri::command]
pub async fn disable_plugin(app_handle: tauri::AppHandle, plugin_id: i64) -> Result<(), String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.upsert_plugin_status(plugin_id, false, None).map_err(|e| e.to_string())?;
    emit_plugin_registry_changed(&app_handle, "disabled");
    Ok(())
}

#[tauri::command]
pub async fn get_plugin_config(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
) -> Result<Vec<PluginConfigItem>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let configs = db.get_plugin_configurations(plugin_id).map_err(|e| e.to_string())?;
    Ok(configs
        .into_iter()
        .map(|config| PluginConfigItem {
            config_id: config.config_id,
            plugin_id: config.plugin_id,
            config_key: config.config_key,
            config_value: config.config_value,
        })
        .collect())
}

#[tauri::command]
pub async fn set_plugin_config(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    key: String,
    value: Option<String>,
) -> Result<i64, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let result = db
        .set_plugin_configuration(plugin_id, &key, value.as_deref())
        .map_err(|e| e.to_string())?;
    emit_plugin_registry_changed(&app_handle, "config-updated");
    Ok(result)
}

#[tauri::command]
pub async fn get_plugin_assistant_configs(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    assistant_id: i64,
) -> Result<Vec<PluginAssistantConfigItem>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let configs = db
        .get_plugin_assistant_configurations(plugin_id, assistant_id)
        .map_err(|e| e.to_string())?;
    Ok(configs
        .into_iter()
        .map(plugin_assistant_config_to_item)
        .collect())
}

#[tauri::command]
pub async fn set_plugin_assistant_config(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    assistant_id: i64,
    key: String,
    value: Option<String>,
) -> Result<i64, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.set_plugin_assistant_configuration(plugin_id, assistant_id, &key, value.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_get_conversation_with_messages(
    app_handle: tauri::AppHandle,
    name_cache_state: tauri::State<'_, NameCacheState>,
    plugin_id: i64,
    conversation_id: i64,
) -> Result<crate::api::conversation_api::ConversationWithMessages, String> {
    assert_plugin_permission(&app_handle, plugin_id, "conversation.read")?;
    crate::api::conversation_api::get_conversation_with_messages(
        app_handle,
        name_cache_state,
        conversation_id,
    )
    .await
}

#[tauri::command]
pub async fn plugin_get_assistant_detail(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    assistant_id: i64,
) -> Result<crate::api::assistant_api::AssistantDetail, String> {
    assert_plugin_permission(&app_handle, plugin_id, "assistant.read")?;
    crate::api::assistant_api::get_assistant(app_handle, assistant_id)
}

#[tauri::command]
pub async fn plugin_update_assistant_prompt(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginUpdateAssistantPromptRequest,
) -> Result<AssistantPrompt, String> {
    assert_plugin_permission(&app_handle, plugin_id, "assistant.prompt.write")?;

    let prompt = request.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Assistant prompt cannot be empty".to_string());
    }

    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    assistant_db
        .get_assistant(request.assistant_id)
        .map_err(|e| format!("Failed to load assistant {}: {}", request.assistant_id, e))?;

    let existing_prompt = assistant_db
        .get_assistant_prompt(request.assistant_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .next();

    if let Some(expected_prompt_id) = request.expected_prompt_id {
        match existing_prompt.as_ref() {
            Some(prompt_item) if prompt_item.id == expected_prompt_id => {}
            Some(prompt_item) => {
                return Err(format!(
                    "Assistant prompt changed while editing (expected prompt id {}, found {})",
                    expected_prompt_id, prompt_item.id
                ));
            }
            None => {
                return Err(format!(
                    "Assistant prompt changed while editing (expected prompt id {}, found none)",
                    expected_prompt_id
                ));
            }
        }
    }

    if let Some(expected_old_prompt) = request.expected_old_prompt.as_ref() {
        let current_prompt = existing_prompt
            .as_ref()
            .map(|prompt_item| prompt_item.prompt.as_str())
            .unwrap_or("");
        if current_prompt != expected_old_prompt {
            return Err("Assistant prompt changed while editing; please reopen the optimizer.".to_string());
        }
    }

    let updated_prompt = match existing_prompt {
        Some(mut prompt_item) => {
            if prompt_item.prompt.as_str() != prompt.as_str() {
                assistant_db
                    .update_assistant_prompt(prompt_item.id, &prompt)
                    .map_err(|e| e.to_string())?;
                prompt_item.prompt = prompt.clone();
            }
            prompt_item
        }
        None => {
            let prompt_id = assistant_db
                .add_assistant_prompt(request.assistant_id, &prompt)
                .map_err(|e| e.to_string())?;
            AssistantPrompt {
                id: prompt_id,
                assistant_id: request.assistant_id,
                prompt,
                created_time: None,
            }
        }
    };

    Ok(updated_prompt)
}

#[tauri::command]
pub async fn get_plugin_data(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    session_id: String,
) -> Result<Vec<PluginDataItem>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let data = db.get_plugin_data_by_session(plugin_id, &session_id).map_err(|e| e.to_string())?;
    Ok(data
        .into_iter()
        .map(|item| PluginDataItem {
            data_id: item.data_id,
            plugin_id: item.plugin_id,
            session_id: item.session_id,
            data_key: item.data_key,
            data_value: item.data_value,
            created_at: item.created_at.to_rfc3339(),
            updated_at: item.updated_at.to_rfc3339(),
        })
        .collect())
}

#[tauri::command]
pub async fn set_plugin_data(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    session_id: String,
    key: String,
    value: Option<String>,
) -> Result<i64, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let existing =
        db.get_plugin_data_by_session(plugin_id, &session_id).map_err(|e| e.to_string())?;
    if let Some(item) = existing.into_iter().find(|entry| entry.data_key == key) {
        db.update_plugin_data(item.data_id, value.as_deref(), Utc::now())
            .map_err(|e| e.to_string())?;
        return Ok(item.data_id);
    }

    let now = Utc::now();
    let data = PluginData {
        data_id: 0,
        plugin_id,
        session_id,
        data_key: key,
        data_value: value,
        created_at: now,
        updated_at: now,
    };
    db.add_plugin_data(&data).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_plugin_hook_registrations(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
) -> Result<Vec<PluginHookRegistrationItem>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let registrations = db.get_plugin_hook_registrations(plugin_id).map_err(|e| e.to_string())?;
    Ok(registrations.into_iter().map(plugin_hook_registration_to_item).collect())
}

#[tauri::command]
pub async fn list_plugin_hook_audit_logs(
    app_handle: tauri::AppHandle,
    limit: Option<i64>,
) -> Result<Vec<PluginHookAuditLogItem>, String> {
    let db = PluginDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(100).clamp(1, 500);
    let logs = db.list_plugin_hook_audit_logs(limit).map_err(|e| e.to_string())?;
    Ok(logs.into_iter().map(plugin_hook_audit_log_to_item).collect())
}

#[tauri::command]
pub async fn submit_js_plugin_hook_result(
    request_id: String,
    result: Option<serde_json::Value>,
    error: Option<String>,
) -> Result<(), String> {
    crate::plugin::runtime::js_bridge::submit_js_plugin_hook_result(request_id, result, error)
}

#[tauri::command]
pub async fn plugin_data_query(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginDataQueryRequest,
) -> Result<PluginSqlQueryResult, String> {
    let (_database, db_file, permission) = normalize_plugin_database_name(&request.database)?;
    assert_plugin_permission(&app_handle, plugin_id, &permission)?;
    let conn = open_readonly_app_database(&app_handle, db_file)?;
    execute_readonly_query(&conn, &request.sql, request.params, clamp_query_max_rows(request.max_rows))
}

#[tauri::command]
pub async fn plugin_data_schema(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    database: String,
) -> Result<PluginDatabaseSchema, String> {
    let (database, db_file, permission) = normalize_plugin_database_name(&database)?;
    assert_plugin_permission(&app_handle, plugin_id, &permission)?;
    let conn = open_readonly_app_database(&app_handle, db_file)?;
    read_database_schema(&conn, database)
}

#[tauri::command]
pub async fn plugin_storage_query(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginStorageQueryRequest,
) -> Result<PluginSqlQueryResult, String> {
    let manifest = assert_plugin_permission(&app_handle, plugin_id, "plugin.storage")?;
    let conn = open_plugin_storage_database(&app_handle, &manifest)?;
    execute_readonly_query(&conn, &request.sql, request.params, clamp_query_max_rows(request.max_rows))
}

#[tauri::command]
pub async fn plugin_storage_execute(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginStorageExecuteRequest,
) -> Result<PluginSqlExecuteResult, String> {
    let manifest = assert_plugin_permission(&app_handle, plugin_id, "plugin.storage")?;
    let conn = open_plugin_storage_database(&app_handle, &manifest)?;
    execute_storage_statement(&conn, &request.sql, request.params)
}

#[tauri::command]
pub async fn plugin_storage_schema(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
) -> Result<PluginDatabaseSchema, String> {
    let manifest = assert_plugin_permission(&app_handle, plugin_id, "plugin.storage")?;
    let conn = open_plugin_storage_database(&app_handle, &manifest)?;
    read_database_schema(&conn, "plugin-storage")
}

#[tauri::command]
pub async fn plugin_create_conversation(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginCreateConversationRequest,
) -> Result<i64, String> {
    assert_plugin_permission(&app_handle, plugin_id, "conversation.write")?;
    let assistant_db = crate::db::assistant_db::AssistantDatabase::new(&app_handle)
        .map_err(|e| e.to_string())?;
    assistant_db
        .get_assistant(request.assistant_id)
        .map_err(|e| format!("Failed to load assistant {}: {}", request.assistant_id, e))?;

    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let created = repo
        .create(&crate::db::conversation_db::Conversation {
            id: 0,
            name: request
                .conversation_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("插件会话")
                .to_string(),
            assistant_id: Some(request.assistant_id),
            created_time: Utc::now(),
            updated_time: Utc::now(),
            conversation_kind: "normal".to_string(),
            parent_butler_conversation_id: None,
            source_task_title: None,
            is_hidden_from_normal_chat_list: false,
            channel_source: Some("plugin".to_string()),
            butler_task_status: None,
            butler_task_summary: None,
            butler_task_finalized_at: None,
        })
        .map_err(|e| e.to_string())?;
    Ok(created.id)
}

#[tauri::command]
pub async fn plugin_append_message(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginAppendMessageRequest,
) -> Result<Message, String> {
    assert_plugin_permission(&app_handle, plugin_id, "conversation.write")?;
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    conversation_repo
        .read(request.conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", request.conversation_id))?;

    let message_type = normalize_message_type(&request.message_type)?;
    let content = request.content.trim().to_string();
    if content.is_empty() {
        return Err("Plugin message content cannot be empty".to_string());
    }
    let metadata_json = serialize_metadata(request.metadata)?;
    let now = Utc::now();
    let message = Message {
        id: 0,
        parent_id: None,
        conversation_id: request.conversation_id,
        message_type: message_type.clone(),
        content,
        llm_model_id: None,
        llm_model_name: None,
        created_time: now,
        start_time: Some(now),
        finish_time: Some(now),
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: None,
        parent_group_id: None,
        tool_calls_json: None,
        metadata_json,
        first_token_time: None,
        ttft_ms: None,
    };
    let repo = db.message_repo().map_err(|e| e.to_string())?;
    let created = repo.create(&message).map_err(|e| e.to_string())?;
    emit_conversation_event(
        &app_handle,
        created.conversation_id,
        "message_add",
        serde_json::to_value(MessageAddEvent {
            message_id: created.id,
            message_type,
        })
        .map_err(|e| e.to_string())?,
    );
    Ok(created)
}

#[tauri::command]
pub async fn plugin_update_message_metadata(
    app_handle: tauri::AppHandle,
    plugin_id: i64,
    request: PluginUpdateMessageMetadataRequest,
) -> Result<Message, String> {
    assert_plugin_permission(&app_handle, plugin_id, "conversation.write")?;
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let repo = db.message_repo().map_err(|e| e.to_string())?;
    let existing = repo
        .read(request.message_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Message not found: {}", request.message_id))?;
    let metadata_json = serialize_metadata(request.metadata)?;
    repo.update_metadata(request.message_id, metadata_json.as_deref())
        .map_err(|e| e.to_string())?;
    let updated = repo
        .read(request.message_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Message not found after metadata update: {}", request.message_id))?;
    emit_conversation_event(
        &app_handle,
        existing.conversation_id,
        "message_metadata_update",
        serde_json::json!({
            "message_id": request.message_id,
        }),
    );
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_manifest_reads_permissions_and_bangs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let plugin_dir = temp_dir.path().join("demo-plugin");
        fs::create_dir_all(plugin_dir.join("dist")).unwrap();
        fs::write(plugin_dir.join("dist").join("main.js"), "// plugin").unwrap();
        fs::write(
            plugin_dir.join("plugin.json"),
            r#"
            {
              "id": "demo-plugin",
              "code": "demo-plugin",
              "name": "Demo Plugin",
              "version": "0.1.0",
              "pluginTypes": ["toolType"],
              "runtime": { "type": "js", "entry": "dist/main.js" },
              "permissions": ["bang.register", "markdown.register", "bang.register", "hook.chat.beforeSend"],
              "contributions": {
                "hooks": [
                  {
                    "name": "chat.beforeSend",
                    "kind": "guard",
                    "priority": 50,
                    "timeoutMs": 1500,
                    "failurePolicy": "block"
                  }
                ],
                "bangs": [
                  {
                    "name": "directory",
                    "aliases": ["dir"],
                    "executor": {
                      "type": "builtinTool",
                      "command": "aipp:operation",
                      "toolName": "list_directory",
                      "arguments": {
                        "path": {
                          "source": "firstArg",
                          "required": true
                        }
                      }
                    }
                  }
                ]
              }
            }
            "#,
        )
        .unwrap();

        let raw_manifest = fs::read_to_string(plugin_dir.join("plugin.json")).unwrap();
        let parsed_manifest: PluginManifest = serde_json::from_str(&raw_manifest).unwrap();
        assert_eq!(
            normalize_permissions(&parsed_manifest.permissions),
            vec!["bang.register", "markdown.register", "hook.chat.beforesend"]
        );

        let manifest = resolve_plugin_manifest_from_dir(&plugin_dir, "demo-plugin").unwrap();
        assert_eq!(manifest.code, "demo-plugin");
        assert_eq!(
            manifest.permissions,
            vec!["bang.register", "markdown.register", "hook.chat.beforesend"]
        );
        assert_eq!(manifest.runtime.runtime_type, "js");
        assert_eq!(manifest.runtime.entry, "dist/main.js");
        assert_eq!(manifest.contributions.hooks.len(), 1);
        assert_eq!(manifest.contributions.hooks[0].kind, "guard");
        assert_eq!(manifest.contributions.bangs.len(), 1);
        assert_eq!(manifest.contributions.bangs[0].aliases, vec!["dir"]);
    }

    #[test]
    fn normalize_permissions_dedupes_and_lowers() {
        let permissions = normalize_permissions(&[
            " Bang.Register ".to_string(),
            "markdown.register".to_string(),
            "bang.register".to_string(),
        ]);
        assert_eq!(permissions, vec!["bang.register", "markdown.register"]);
    }

    #[test]
    fn plugin_readonly_query_rejects_mutations() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT)", [])
            .unwrap();
        conn.execute("INSERT INTO demo (name) VALUES ('alpha')", []).unwrap();

        let result = execute_readonly_query(
            &conn,
            "SELECT id, name FROM demo WHERE name = ?",
            vec![JsonValue::String("alpha".to_string())],
            10,
        )
        .unwrap();
        assert_eq!(result.columns, vec!["id", "name"]);
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0][1], JsonValue::String("alpha".to_string()));

        let err = execute_readonly_query(
            &conn,
            "UPDATE demo SET name = 'beta'",
            Vec::new(),
            10,
        )
        .unwrap_err();
        assert!(err.contains("Only SELECT or WITH"));
    }

    #[test]
    fn plugin_storage_execute_and_schema_work_on_private_connection() {
        let conn = Connection::open_in_memory().unwrap();
        execute_storage_statement(
            &conn,
            "CREATE TABLE cache (key TEXT PRIMARY KEY, value TEXT)",
            Vec::new(),
        )
        .unwrap();
        execute_storage_statement(
            &conn,
            "INSERT INTO cache (key, value) VALUES (?, ?)",
            vec![
                JsonValue::String("lastSync".to_string()),
                JsonValue::String("ok".to_string()),
            ],
        )
        .unwrap();

        let result = execute_readonly_query(&conn, "SELECT value FROM cache", Vec::new(), 10)
            .unwrap();
        assert_eq!(result.rows[0][0], JsonValue::String("ok".to_string()));

        let schema = read_database_schema(&conn, "plugin-storage").unwrap();
        assert_eq!(schema.database, "plugin-storage");
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "cache");
        assert_eq!(schema.tables[0].columns[0].name, "key");
    }
}

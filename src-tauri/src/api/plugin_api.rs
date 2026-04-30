use crate::db::assistant_db::{AssistantDatabase, AssistantPrompt};
use crate::db::connection::params;
use crate::db::conversation_db::{ConversationDatabase, Message, Repository};
use crate::NameCacheState;
use chrono::Utc;
use rusqlite::{Connection, OpenFlags, ToSql};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};
use tracing::warn;

use crate::api::ai::events::{ConversationEvent, MessageAddEvent};
use crate::db::plugin_db::{
    NewPluginHookRegistration, Plugin, PluginAssistantConfiguration, PluginData, PluginDatabase,
    PluginHookAuditLog, PluginHookRegistration,
};

const PLUGIN_TYPE_CONFIG_KEY: &str = "plugin_type";
const DEFAULT_PLUGIN_QUERY_MAX_ROWS: usize = 1000;
const ABSOLUTE_PLUGIN_QUERY_MAX_ROWS: usize = 10000;

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

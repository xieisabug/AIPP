use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};
use tracing::warn;

use crate::db::plugin_db::{Plugin, PluginData, PluginDatabase};

const PLUGIN_TYPE_CONFIG_KEY: &str = "plugin_type";

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
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginContributions {
    #[serde(default)]
    pub bangs: Vec<PluginBangContribution>,
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
    pub code: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub plugin_type: Vec<String>,
    pub permissions: Vec<String>,
    pub contributions: PluginContributions,
    pub plugin_dir: PathBuf,
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
        Ok(root) => root.join(code).join("dist").join("main.js").is_file(),
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
    let main_js_path = dir_path.join("dist").join("main.js");
    if !main_js_path.is_file() {
        return None;
    }

    let manifest = read_plugin_manifest(&dir_path.join("plugin.json"));
    let (name, version, description, author, plugin_type, permissions, contributions) =
        if let Some(manifest_data) = manifest {
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
                manifest_data.contributions,
            )
        } else {
            (
                code.to_string(),
                "0.0.0".to_string(),
                None,
                None,
                vec!["assistantType".to_string()],
                Vec::new(),
                PluginContributions::default(),
            )
        };

    Some(ResolvedPluginManifest {
        code: code.to_string(),
        name,
        version,
        description,
        author,
        plugin_type,
        permissions,
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
        if let Some(manifest) = resolve_plugin_manifest_for_code(app_handle, &plugin.folder_name) {
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
              "permissions": ["bang.register", "markdown.register", "bang.register"],
              "contributions": {
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
            vec!["bang.register", "markdown.register"]
        );

        let manifest = resolve_plugin_manifest_from_dir(&plugin_dir, "demo-plugin").unwrap();
        assert_eq!(manifest.code, "demo-plugin");
        assert_eq!(manifest.permissions, vec!["bang.register", "markdown.register"]);
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
}

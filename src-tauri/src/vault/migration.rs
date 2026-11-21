//! User-confirmed migration of plaintext configuration credentials into the personal vault.
//! Candidate APIs return locations and labels only. The source value stays inside this module.
use crate::db::{llm_db::LLMDatabase, mcp_db::{MCPDatabase, MCPServer}};
use crate::sync::personal_vault::{self, CreateInput, ItemMeta, SecretValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tauri::AppHandle;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationCandidate {
    pub kind: String,
    pub config_id: i64,
    pub field: String,
    pub label: String,
    pub target: String,
    pub suggested_name: String,
    pub suggested_key: String,
}

#[derive(Clone)]
struct SourceValue {
    candidate: MigrationCandidate,
    value: String,
    prefix: String,
}

fn secret_field(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    let compact: String = normalized.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
    let separated_key = normalized.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|part| part == "key");
    ["apikey", "token", "secret", "password", "authorization", "credential"]
        .iter().any(|part| compact.contains(part)) || separated_key
}

fn plain_value(value: &str) -> Option<(String, String)> {
    if value.trim().is_empty() || crate::vault::config_ref::contains(value) {
        return None;
    }
    let (prefix, secret) = if let Some((prefix, secret)) = value.split_once(' ') {
        if prefix.eq_ignore_ascii_case("bearer") {
            (format!("{prefix} "), secret)
        } else {
            (String::new(), value)
        }
    } else {
        (String::new(), value)
    };
    (!secret.trim().is_empty()).then(|| (prefix, secret.to_owned()))
}

fn source_value_matches(current: Option<(String, String)>, source: &SourceValue) -> bool {
    current.as_ref() == Some(&(source.prefix.clone(), source.value.clone()))
}

fn key_for(name: &str) -> String {
    let mut key = String::new();
    for ch in name.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            key.push(ch);
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    let key = key.trim_matches('_');
    let key = if key.is_empty() { "credential" } else { key };
    let key = if key.as_bytes()[0].is_ascii_alphabetic() { key.to_owned() } else { format!("credential_{key}") };
    let key = if key.len() < 2 { format!("{key}_key") } else { key };
    key.chars().take(40).collect()
}

fn candidate(kind: &str, config_id: i64, field: &str, label: String, target: String) -> MigrationCandidate {
    let suggested_name = label.clone();
    MigrationCandidate {
        kind: kind.into(), config_id, field: field.into(), label, target,
        suggested_key: key_for(&suggested_name), suggested_name,
    }
}

fn target_for_server(server: &MCPServer) -> Option<String> {
    crate::vault::config_ref::mcp_target(
        &server.transport_type,
        server.command.as_deref(),
        server.url.as_deref(),
        server.environment_variables.as_deref(),
    ).ok().map(|target| match target {
        crate::vault::config_ref::Target::Site(origin) => origin,
        crate::vault::config_ref::Target::Program { fingerprint, .. } => {
            format!("本机程序 · {}", &fingerprint[..12])
        }
    })
}

fn server_candidates(server: &MCPServer) -> Vec<SourceValue> {
    let Some(target) = target_for_server(server) else { return Vec::new() };
    let mut found = Vec::new();
    if let Some(raw) = server.headers.as_deref() {
        if let Ok(headers) = serde_json::from_str::<BTreeMap<String, String>>(raw) {
            for (header, value) in headers {
                if !secret_field(&header) { continue; }
                if let Some((prefix, value)) = plain_value(&value) {
                    let label = format!("{} · 请求头 {}", server.name, header);
                    found.push(SourceValue {
                        candidate: candidate("mcp_header", server.id, &header, label, target.clone()),
                        value, prefix,
                    });
                }
            }
        }
    }
    if let Some(raw) = server.environment_variables.as_deref() {
        let variables = crate::mcp::registry_api::parse_env_vars(raw);
        for (name, value) in &variables {
            if variables.iter().filter(|(other, _)| other == name).count() != 1 { continue; }
            if !secret_field(&name) { continue; }
            if let Some((prefix, value)) = plain_value(value) {
                let label = format!("{} · 环境变量 {}", server.name, name);
                found.push(SourceValue {
                    candidate: candidate("mcp_env", server.id, name, label, target.clone()),
                    value, prefix,
                });
            }
        }
    }
    found
}

fn provider_candidates(app: &AppHandle) -> Result<Vec<SourceValue>, String> {
    let db = LLMDatabase::new(app).map_err(|e| e.to_string())?;
    let mut stmt = db.conn.prepare(
        "SELECT p.id,p.name,p.api_type,k.value,e.value
         FROM llm_provider p JOIN llm_provider_config k ON k.llm_provider_id=p.id AND k.name='api_key'
         LEFT JOIN llm_provider_config e ON e.llm_provider_id=p.id AND e.name='endpoint'
         WHERE trim(k.value) != '' ORDER BY p.id",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| Ok((
        row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
        row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?,
    ))).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    for (id, name, api_type, raw, endpoint) in rows {
        let Some((prefix, value)) = plain_value(&raw) else { continue };
        let Ok(target) = crate::vault::config_ref::provider_target(&api_type, endpoint.as_deref()) else { continue };
        found.push(SourceValue {
            candidate: candidate("provider_key", id, "api_key", format!("{} · API Key", name), target.detail()),
            value, prefix,
        });
    }
    Ok(found)
}

fn scan(app: &AppHandle) -> Result<Vec<SourceValue>, String> {
    let mcp = MCPDatabase::new(app).map_err(|e| e.to_string())?;
    let has_app_connections: bool = mcp.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_connection')",
        [], |row| row.get(0),
    ).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    for server in mcp.get_mcp_servers().map_err(|e| e.to_string())? {
        if has_app_connections {
            let is_app_connection: bool = mcp.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM app_connection WHERE server_id=?1)",
                [server.id], |row| row.get(0),
            ).map_err(|e| e.to_string())?;
            if is_app_connection { continue; }
        }
        if server.is_builtin && server.command.as_deref() != Some(crate::vault::config_ref::BUILTIN_SEARCH_COMMAND) {
            continue;
        }
        found.extend(server_candidates(&server));
    }
    found.extend(provider_candidates(app)?);
    Ok(found)
}

pub(crate) fn candidates(app: &AppHandle) -> Result<Vec<MigrationCandidate>, String> {
    if !personal_vault::capability(app).available {
        return Err("此设备的凭据保险箱不可用".into());
    }
    Ok(scan(app)?.into_iter().map(|item| item.candidate).collect())
}

fn replace_env(raw: &str, field: &str, replacement: &str) -> Result<String, String> {
    let mut found = false;
    let mut count = 0;
    let lines = raw.lines().map(|line| {
        let Some((name, value)) = line.split_once('=') else { return line.to_owned() };
        if name.trim() != field { return line.to_owned(); }
        found = true;
        count += 1;
        format!("{}={}{}", name, value.split_once(' ').filter(|(prefix, _)| prefix.eq_ignore_ascii_case("bearer"))
            .map(|(prefix, _)| format!("{prefix} ")).unwrap_or_default(), replacement)
    }).collect::<Vec<_>>();
    if !found { return Err("配置字段已变化，请重新扫描".into()); }
    if count != 1 { return Err("环境变量名称重复，无法安全迁移，请先修正配置".into()); }
    Ok(lines.join("\n"))
}

fn current_source(app: &AppHandle, requested: &MigrationCandidate) -> Result<SourceValue, String> {
    let source = scan(app)?.into_iter().find(|item| item.candidate.kind == requested.kind
        && item.candidate.config_id == requested.config_id && item.candidate.field == requested.field)
        .ok_or_else(|| "配置已变化或凭据已迁移，请重新扫描".to_string())?;
    if source.candidate.label != requested.label || source.candidate.target != requested.target {
        return Err("配置名称或目标已变化，请重新扫描并确认".into());
    }
    Ok(source)
}

fn unique_key(app: &AppHandle, requested: &str) -> Result<String, String> {
    let existing: std::collections::HashSet<String> = personal_vault::list(app)?.into_iter().map(|item| item.key).collect();
    let base = key_for(requested);
    if !existing.contains(&base) { return Ok(base); }
    for index in 2..=999 {
        let suffix = format!("_{index}");
        let candidate = format!("{}{}", base.chars().take(40 - suffix.len()).collect::<String>(), suffix);
        if !existing.contains(&candidate) { return Ok(candidate); }
    }
    Err("无法为新条目生成不重复的键".into())
}

fn replace_server_value(app: &AppHandle, source: &SourceValue, reference: &str) -> Result<(), String> {
    let mut db = MCPDatabase::new(app).map_err(|e| e.to_string())?;
    let server = db.get_mcp_server(source.candidate.config_id).map_err(|e| e.to_string())?;
    let replacement = format!("{}{}", source.prefix, reference);
    let mut updated = server.clone();
    match source.candidate.kind.as_str() {
        "mcp_header" => {
            let mut headers: BTreeMap<String, String> = serde_json::from_str(server.headers.as_deref().unwrap_or("{}"))
                .map_err(|_| "请求头配置已变化，请重新扫描".to_string())?;
            let Some(value) = headers.get_mut(&source.candidate.field) else { return Err("请求头已变化，请重新扫描".into()) };
            let Some((prefix, current_value)) = plain_value(value) else { return Err("请求头已变化，请重新扫描".into()) };
            if !source_value_matches(Some((prefix.clone(), current_value)), source) {
                return Err("凭据在迁移期间已变化，原配置保持不变，请重新扫描".into());
            }
            *value = format!("{prefix}{reference}");
            updated.headers = Some(serde_json::to_string(&headers).map_err(|_| "请求头配置更新失败")?);
        }
        "mcp_env" => {
            let raw = server.environment_variables.as_deref().unwrap_or_default();
            let current = crate::mcp::registry_api::parse_env_vars(raw).into_iter()
                .find(|(name, _)| name == &source.candidate.field)
                .and_then(|(_, value)| plain_value(&value));
            if !source_value_matches(current, source) {
                return Err("凭据在迁移期间已变化，原配置保持不变，请重新扫描".into());
            }
            updated.environment_variables = Some(replace_env(raw, &source.candidate.field, &replacement)?);
        }
        _ => return Err("不支持的迁移字段".into()),
    }
    let update_headers = match source.candidate.kind.as_str() {
        "mcp_header" => true,
        "mcp_env" => false,
        _ => return Err("不支持的迁移字段".into()),
    };
    let changed = db.replace_mcp_credential_if_unchanged(&server, &updated, update_headers)
        .map_err(|_| "保存配置失败，原配置保持不变".to_string())?;
    if !changed { return Err("配置在迁移期间已变化，原配置保持不变，请重新扫描".into()); }
    if let Err(error) = crate::vault::config_ref::confirm_mcp(app, updated.id, &updated.name, &updated.transport_type, updated.command.as_deref(),
        updated.url.as_deref(), updated.environment_variables.as_deref(), updated.headers.as_deref()) {
        let restored = db.replace_mcp_credential_if_unchanged(&updated, &server, update_headers);
        if !matches!(restored, Ok(true)) {
            return Err("目标确认失败且原配置恢复失败。凭据仍保留在保险箱中，请检查该配置后重新保存".into());
        }
        return Err(error);
    }
    crate::sync::schedule_sync_after_local_change(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{key_for, plain_value, replace_env, secret_field, server_candidates, source_value_matches, SourceValue};
    use crate::db::mcp_db::MCPServer;

    fn test_server() -> MCPServer {
        MCPServer {
            id: 7, name: "Test MCP".into(), description: String::new(), transport_type: "http".into(),
            command: None, environment_variables: Some("API_TOKEN=header-token\nPUBLIC=value".into()),
            headers: Some(r#"{"Authorization":"Bearer header-token","Content-Type":"application/json"}"#.into()),
            url: Some("https://api.example.com/mcp".into()), timeout: None, is_long_running: false,
            is_enabled: true, is_builtin: false, is_deletable: true, proxy_enabled: false, created_time: String::new(),
        }
    }

    #[test]
    fn test_secret_field_detects_common_credential_names_only() {
        assert!(secret_field("Authorization"));
        assert!(secret_field("HTTP_SEARCH_API_KEY"));
        assert!(secret_field("refresh_token"));
        assert!(!secret_field("monkey_count"));
        assert!(!secret_field("HTTP_SEARCH_ENDPOINT"));
    }

    #[test]
    fn test_plain_value_preserves_bearer_prefix_for_configuration() {
        assert_eq!(plain_value("Bearer token-value"), Some(("Bearer ".into(), "token-value".into())));
        assert_eq!(plain_value("  secret  "), Some((String::new(), "  secret  ".into())));
        assert_eq!(plain_value("vault://123"), None);
        assert_eq!(plain_value("  "), None);
    }

    #[test]
    fn test_source_value_matches_rejects_rotated_credentials() {
        let source = SourceValue {
            candidate: super::candidate("provider_key", 1, "api_key", "Provider · API Key".into(), "https://api.example.com".into()),
            value: "old-token".into(),
            prefix: "Bearer ".into(),
        };
        assert!(source_value_matches(plain_value("Bearer old-token"), &source));
        assert!(!source_value_matches(plain_value("Bearer new-token"), &source));
    }

    #[test]
    fn test_replace_env_replaces_only_the_selected_field() {
        let result = replace_env("API_KEY=old\nOTHER_TOKEN=keep", "API_KEY", "vault://id").unwrap();
        assert_eq!(result, "API_KEY=vault://id\nOTHER_TOKEN=keep");
        assert!(replace_env("API_KEY=old", "MISSING_KEY", "vault://id").is_err());
        assert!(replace_env("API_KEY=old\nAPI_KEY=new", "API_KEY", "vault://id").is_err());
    }

    #[test]
    fn test_stdio_target_is_identifiable_without_showing_command() {
        let mut first = test_server();
        first.transport_type = "stdio".into();
        first.command = Some("node private-credential.js".into());
        first.url = None;
        first.environment_variables = Some("API_TOKEN=secret-value".into());
        let first_target = super::target_for_server(&first).unwrap();
        assert!(first_target.starts_with("本机程序 · "));
        assert!(!first_target.contains("private-credential"));
        assert!(!first_target.contains("secret-value"));

        first.command = Some("node other.js".into());
        assert_ne!(first_target, super::target_for_server(&first).unwrap());
    }

    #[test]
    fn test_key_for_produces_valid_nonempty_key() {
        assert_eq!(key_for("123 API Key"), "credential_123_api_key");
        assert_eq!(key_for("密钥"), "credential");
        assert_eq!(key_for("A"), "a_key");
    }

    #[test]
    fn test_candidates_expose_locations_without_plaintext_values() {
        let candidates = server_candidates(&test_server());
        assert_eq!(candidates.len(), 2);
        let metadata = serde_json::to_string(&candidates.iter().map(|item| &item.candidate).collect::<Vec<_>>()).unwrap();
        assert!(metadata.contains("Authorization"));
        assert!(metadata.contains("API_TOKEN"));
        assert!(metadata.contains("https://api.example.com"));
        assert!(!metadata.contains("header-token"));

        let mut duplicate_env = test_server();
        duplicate_env.environment_variables = Some("API_TOKEN=first\nAPI_TOKEN=second".into());
        let candidates = server_candidates(&duplicate_env);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].candidate.kind, "mcp_header");
    }
}

fn replace_provider_value(app: &AppHandle, source: &SourceValue, reference: &str) -> Result<(), String> {
    let db = LLMDatabase::new(app).map_err(|e| e.to_string())?;
    let configs = db.get_llm_provider_config(source.candidate.config_id).map_err(|e| e.to_string())?;
    let old = configs.iter().find(|config| config.name == "api_key").ok_or("Provider API Key 已变化，请重新扫描")?;
    if !source_value_matches(plain_value(&old.value), source) {
        return Err("凭据在迁移期间已变化，原配置保持不变，请重新扫描".into());
    }
    let provider = db.get_llm_provider(source.candidate.config_id).map_err(|e| e.to_string())?;
    let endpoint = configs.iter().find(|config| config.name == "endpoint").map(|config| config.value.as_str());
    let target = crate::vault::config_ref::provider_target(&provider.api_type, endpoint).map_err(|e| e.message)?;
    if target.detail() != source.candidate.target { return Err("配置目标已变化，请重新扫描并确认".into()); }
    let replacement = format!("{}{}", source.prefix, reference);
    let changed = db.conn.execute(
        "UPDATE llm_provider_config SET value=?1 WHERE llm_provider_id=?2 AND name='api_key' AND value=?3
         AND EXISTS(SELECT 1 FROM llm_provider p WHERE p.id=?2 AND p.api_type=?4
         AND (SELECT e.value FROM llm_provider_config e WHERE e.llm_provider_id=p.id AND e.name='endpoint') IS ?5)",
        rusqlite::params![replacement, source.candidate.config_id, old.value, provider.api_type, endpoint],
    ).map_err(|_| "保存 Provider 配置失败，原配置保持不变".to_string())?;
    if changed != 1 { return Err("Provider 凭据在迁移期间已变化，原配置保持不变，请重新扫描".into()); }
    if let Err(error) = crate::vault::config_ref::confirm_provider_target(app, source.candidate.config_id, &target) {
        let restored = db.conn.execute(
            "UPDATE llm_provider_config SET value=?1 WHERE llm_provider_id=?2 AND name='api_key' AND value=?3",
            rusqlite::params![old.value, source.candidate.config_id, replacement],
        );
        if !matches!(restored, Ok(1)) {
            return Err("目标确认失败且原配置恢复失败。凭据仍保留在保险箱中，请检查该配置后重新保存".into());
        }
        return Err(error);
    }
    crate::sync::schedule_sync_after_local_change(app);
    Ok(())
}

pub(crate) fn migrate(app: &AppHandle, requested: &MigrationCandidate, name: &str, key: &str) -> Result<ItemMeta, String> {
    let source = current_source(app, requested)?;
    let name = name.trim();
    if name.is_empty() { return Err("条目名称不能为空".into()); }
    let key = unique_key(app, key)?;
    let input = CreateInput {
        name: name.into(), key, description: format!("从配置迁移：{}", source.candidate.label),
        value: SecretValue::from_migration(source.value.clone()), allowed_sites: Vec::new(), sync_enabled: true,
    };
    let meta = personal_vault::create(app, &input)?;
    let reference = format!("vault://{}", meta.id);
    let result = match source.candidate.kind.as_str() {
        "provider_key" => replace_provider_value(app, &source, &reference),
        "mcp_header" | "mcp_env" => replace_server_value(app, &source, &reference),
        _ => Err("不支持的迁移字段".into()),
    };
    if let Err(error) = result {
        if !error.contains("原配置恢复失败") {
            let _ = personal_vault::delete(app, &meta.id);
        }
        return Err(error);
    }
    Ok(meta)
}

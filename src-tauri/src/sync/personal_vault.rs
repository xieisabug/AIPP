//! Personal credential vault (凭据保险箱).
//!
//! Each entry is a key (`handle` column) and one secret value. Non-secret metadata lives in plaintext
//! columns; the value is sealed with the shared Vault key (AAD bound to the entry UUID, which is
//! stable across devices). No public API returns a value or a fragment of one: only the trusted
//! resolvers (`vault::placeholder` for tool calls, `vault::config_ref` for configurations) decrypt,
//! and their result type never prints its contents.
use super::*;
use std::collections::BTreeMap;

pub(super) const OBJECT_TYPE: &str = "vault.entry";
const SECRET_NAME: &str = "secret";
const USAGE_KEEP_PER_ITEM: i64 = 500;
const MAX_VALUE_BYTES: usize = 64 * 1024;

const USAGE_TABLE: &str = "CREATE TABLE IF NOT EXISTS vault_usage_log (
            id INTEGER PRIMARY KEY,
            item_uuid TEXT NOT NULL,
            conversation_id INTEGER,
            assistant_id INTEGER,
            tool TEXT NOT NULL,
            destination TEXT NOT NULL,
            outcome TEXT NOT NULL,
            reason TEXT,
            created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_vault_usage_item ON vault_usage_log(item_uuid, id);";

pub(crate) fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS vault_item (
            id INTEGER PRIMARY KEY,
            uuid TEXT NOT NULL UNIQUE,
            handle TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            allowed_sites TEXT NOT NULL DEFAULT '[]',
            sync_enabled INTEGER NOT NULL DEFAULT 1,
            value TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_used_at INTEGER,
            config_bindings TEXT NOT NULL DEFAULT '[]');
        CREATE INDEX IF NOT EXISTS idx_vault_item_handle ON vault_item(handle);
        CREATE TABLE IF NOT EXISTS vault_legacy_item (uuid TEXT PRIMARY KEY, auth_kind TEXT NOT NULL);",
    )
    .map_err(|e| format!("初始化凭据保险箱失败: {e}"))?;
    migrate_schema(db).map_err(|e| format!("初始化凭据保险箱失败: {e}"))?;
    db.execute_batch(USAGE_TABLE).map_err(|e| format!("初始化凭据保险箱失败: {e}"))
}

fn has_column(db: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"), [column], |r| r.get(0))
}

/// Development builds stored several auth-kind fields per item. Their values are converted once the
/// key is available (`migrate_legacy_with`); here only the schema changes.
fn migrate_schema(db: &Connection) -> rusqlite::Result<()> {
    if !has_column(db, "vault_item", "config_bindings")? {
        db.execute_batch("ALTER TABLE vault_item ADD COLUMN config_bindings TEXT NOT NULL DEFAULT '[]'")?;
    }
    if has_column(db, "vault_item", "auth_kind")? {
        db.execute_batch(
            "INSERT OR IGNORE INTO vault_legacy_item(uuid, auth_kind) SELECT uuid, auth_kind FROM vault_item;
             ALTER TABLE vault_item DROP COLUMN auth_kind;
             ALTER TABLE vault_item DROP COLUMN injection;",
        )?;
    }
    let usage_exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='vault_usage_log')", [], |r| r.get(0))?;
    if usage_exists && has_column(db, "vault_usage_log", "method")? {
        // Local-only history from development builds; the new log records tool and destination.
        db.execute_batch("DROP TABLE vault_usage_log")?;
    }
    Ok(())
}

// ---------- types ----------

/// A secret value on its way in. Never printed.
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct SecretValue(String);

impl SecretValue {
    #[cfg(test)]
    pub(crate) fn new(value: impl Into<String>) -> Self { Self(value.into()) }
    fn checked(&self) -> Result<&str, String> {
        let value = self.0.as_str();
        if value.trim().is_empty() { return Err("值不能为空".into()); }
        if value.len() > MAX_VALUE_BYTES { return Err("值过长（最多 64KB）".into()); }
        Ok(value)
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("SecretValue(..)") }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInput {
    pub name: String,
    pub key: String,
    #[serde(default)] pub description: String,
    pub value: SecretValue,
    #[serde(default)] pub allowed_sites: Vec<String>,
    #[serde(default = "default_true")] pub sync_enabled: bool,
}
fn default_true() -> bool { true }

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInput {
    pub name: Option<String>,
    pub key: Option<String>,
    pub description: Option<String>,
    pub allowed_sites: Option<Vec<String>>,
    pub sync_enabled: Option<bool>,
}

/// Metadata visible to the UI and to the model. Contains no secret value or fragment.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemMeta {
    pub id: String,
    /// What the assistant writes in a placeholder: `<vault:key>`.
    pub key: String,
    pub name: String,
    pub description: String,
    /// Network origins where the value is used without asking (the user's "always allow").
    pub allowed_sites: Vec<String>,
    pub sync_enabled: bool,
    /// `ready` or `locked` (sealed under a key this device cannot open yet).
    pub status: &'static str,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: Option<i64>,
}

/// A destination the user confirmed for configuration references to this item, by saving the
/// configuration in Settings. `target` is an origin (`https://host`) or `program:<sha256>`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBinding {
    pub target: String,
    /// The configuration on the confirming device, e.g. `provider:3`. A new confirmation from the same
    /// source replaces its previous target (an edited endpoint does not leave the old one approved).
    #[serde(default)]
    pub source: String,
    /// Which configuration asked for it, e.g. MCP「GitHub」.
    pub label: String,
    /// Human-readable target: the origin, or the local program's command line.
    pub detail: String,
    pub approved_at: i64,
}

/// Decrypted entry for the trusted resolvers only. Never serialized, never printed.
#[derive(Clone)]
pub(crate) struct Resolved {
    pub meta: ItemMeta,
    pub value: String,
    pub config_bindings: Vec<ConfigBinding>,
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved").field("key", &self.meta.key).finish_non_exhaustive()
    }
}

/// Typed failure for resolution so callers can return a stable error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolveError { pub code: &'static str, pub message: String }

impl ResolveError {
    fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into() } }
}

/// Sealed content. `fields` only exists in entries from development builds.
#[derive(Default, Serialize, Deserialize)]
struct SecretPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub id: i64,
    pub item_id: String,
    pub conversation_id: Option<i64>,
    pub assistant_id: Option<i64>,
    /// The tool that used the value, e.g. `execute_bash` or `MCP「GitHub」· create_issue`.
    pub tool: String,
    /// Where the value went, e.g. `https://api.github.com` or `本机命令`.
    pub destination: String,
    pub outcome: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct NewUsage<'a> {
    pub item_id: &'a str,
    pub conversation_id: Option<i64>,
    pub assistant_id: Option<i64>,
    pub tool: &'a str,
    pub destination: &'a str,
    pub outcome: &'a str,
    pub reason: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub available: bool,
    /// Human-readable storage backend, e.g. "Windows 凭据管理器".
    pub backend: Option<&'static str>,
    pub reason: Option<String>,
    /// Secret sync state code: unavailable | not_configured | needs_passphrase | mismatch | ready.
    pub sync_state: String,
    pub sync_message: String,
    /// Items received from other devices that cannot be decrypted until the passphrase is entered.
    pub pending_from_other_devices: i64,
}

// ---------- validation ----------

pub(crate) fn validate_key(key: &str) -> Result<(), String> {
    let ok = key.len() >= 2 && key.len() <= 40
        && key.starts_with(|c: char| c.is_ascii_lowercase())
        && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok { Ok(()) } else { Err("键需为 2–40 位小写字母、数字或下划线，并以字母开头".into()) }
}

/// Normalize a site to its origin (`scheme://host[:port]`). Only http(s), no credentials in the URL.
pub(crate) fn normalize_origin(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let url = reqwest::Url::parse(&with_scheme).map_err(|_| format!("网站地址无效: {raw}"))?;
    if !matches!(url.scheme(), "https" | "http") { return Err(format!("只支持 http/https 网站: {raw}")); }
    if !url.username().is_empty() || url.password().is_some() { return Err("网站地址不能包含用户名或密码".into()); }
    if url.host_str().is_none_or(str::is_empty) { return Err(format!("网站地址缺少主机名: {raw}")); }
    Ok(url.origin().ascii_serialization())
}

fn normalize_sites(sites: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for site in sites.iter().filter(|s| !s.trim().is_empty()) {
        let origin = normalize_origin(site)?;
        if !out.contains(&origin) { out.push(origin); }
    }
    Ok(out)
}

// ---------- crypto ----------

fn object_id(uuid: &str) -> String { format!("pvault:{uuid}") }

fn seal_payload(key: &secrets::SecretKey, uuid: &str, payload: &SecretPayload) -> Result<String, String> {
    let raw = serde_json::to_string(payload).map_err(|_| "凭据编码失败")?;
    serde_json::to_string(&secrets::seal(key, &object_id(uuid), SECRET_NAME, &raw)?).map_err(|e| e.to_string())
}

pub(super) fn seal_value(key: &secrets::SecretKey, uuid: &str, value: &str) -> Result<String, String> {
    seal_payload(key, uuid, &SecretPayload { value: Some(value.to_owned()), fields: BTreeMap::new() })
}

fn open_payload(key: &secrets::SecretKey, uuid: &str, value: &str) -> Result<SecretPayload, String> {
    let envelope: JsonValue = serde_json::from_str(value).map_err(|_| "保险箱密文格式无效")?;
    let raw = secrets::open(key, &object_id(uuid), SECRET_NAME, &envelope)
        .map_err(|_| format!("条目 {uuid} 无法用本机密钥解密（可能来自其他设备、需要同步口令）"))?;
    serde_json::from_str(&raw).map_err(|_| "保险箱内容格式无效".into())
}

pub(super) fn open_value(key: &secrets::SecretKey, uuid: &str, value: &str) -> Result<String, String> {
    let payload = open_payload(key, uuid, value)?;
    match payload.value {
        Some(value) => Ok(value),
        None => Err(format!("条目 {uuid} 还是旧格式，正在转换，请稍后重试")),
    }
}

/// Re-encrypt every item under a new key. Called inside the Vault key-switch transaction.
pub(super) fn reencrypt(db: &Connection, old: Option<&secrets::SecretKey>, new: &secrets::SecretKey) -> Result<(), String> {
    init(db)?;
    let rows = db.prepare("SELECT uuid, value FROM vault_item ORDER BY id").map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (uuid, value) in rows {
        let old = old.ok_or("保险箱已有条目但缺少原加密密钥，无法迁移")?;
        let payload = open_payload(old, &uuid, &value)?;
        db.execute("UPDATE vault_item SET value=?2 WHERE uuid=?1", params![uuid, seal_payload(new, &uuid, &payload)?])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn has_items(db: &Connection) -> Result<bool, String> {
    init(db)?;
    db.query_row("SELECT EXISTS(SELECT 1 FROM vault_item)", [], |r| r.get(0)).map_err(|e| e.to_string())
}

// ---------- row access (pure, testable) ----------

const COLUMNS: &str = "uuid, handle, name, description, allowed_sites, sync_enabled, value, created_at, updated_at, last_used_at, config_bindings";

struct Row {
    uuid: String, handle: String, name: String, description: String, allowed_sites: String,
    sync_enabled: bool, value: String, created_at: i64, updated_at: i64, last_used_at: Option<i64>,
    config_bindings: String,
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        uuid: r.get(0)?, handle: r.get(1)?, name: r.get(2)?, description: r.get(3)?, allowed_sites: r.get(4)?,
        sync_enabled: r.get(5)?, value: r.get(6)?, created_at: r.get(7)?, updated_at: r.get(8)?,
        last_used_at: r.get(9)?, config_bindings: r.get(10)?,
    })
}

impl Row {
    fn bindings(&self) -> Vec<ConfigBinding> {
        serde_json::from_str(&self.config_bindings).unwrap_or_default()
    }
    fn sites(&self) -> Vec<String> {
        serde_json::from_str(&self.allowed_sites).unwrap_or_default()
    }
    fn meta(&self, key: Option<&secrets::SecretKey>) -> ItemMeta {
        let status = match key { Some(key) if open_value(key, &self.uuid, &self.value).is_ok() => "ready", _ => "locked" };
        ItemMeta {
            id: self.uuid.clone(), key: self.handle.clone(), name: self.name.clone(),
            description: self.description.clone(), allowed_sites: self.sites(),
            sync_enabled: self.sync_enabled, status,
            created_at: self.created_at, updated_at: self.updated_at, last_used_at: self.last_used_at,
        }
    }
    fn resolve(&self, key: Option<&secrets::SecretKey>) -> Result<Resolved, ResolveError> {
        let key = key.ok_or_else(|| ResolveError::new("vault_locked", "保险箱密钥不可用，请解锁系统安全存储"))?;
        let value = open_value(key, &self.uuid, &self.value).map_err(|e| ResolveError::new("vault_locked", e))?;
        Ok(Resolved { meta: self.meta(Some(key)), value, config_bindings: self.bindings() })
    }
}

fn load(db: &Connection, uuid: &str) -> Result<Row, String> {
    db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [uuid], row)
        .optional().map_err(|e| e.to_string())?.ok_or_else(|| "条目不存在或已删除".into())
}

fn key_taken(db: &Connection, key: &str, except: Option<&str>) -> Result<bool, String> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM vault_item WHERE handle=?1 AND uuid IS NOT ?2)", params![key, except], |r| r.get(0))
        .map_err(|e| e.to_string())
}

fn now_ms() -> i64 { Utc::now().timestamp_millis() }

pub(super) fn list_with(db: &Connection, key: Option<&secrets::SecretKey>) -> Result<Vec<ItemMeta>, String> {
    init(db)?;
    let rows = db.prepare(&format!("SELECT {COLUMNS} FROM vault_item ORDER BY name COLLATE NOCASE, id")).map_err(|e| e.to_string())?
        .query_map([], row).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.meta(key)).collect())
}

pub(super) fn create_with(db: &Connection, key: &secrets::SecretKey, input: &CreateInput) -> Result<ItemMeta, String> {
    init(db)?;
    let name = input.name.trim();
    if name.is_empty() { return Err("名称不能为空".into()); }
    validate_key(&input.key)?;
    if key_taken(db, &input.key, None)? { return Err(format!("键 {} 已被使用", input.key)); }
    let value = input.value.checked()?;
    let sites = normalize_sites(&input.allowed_sites)?;
    let uuid = Uuid::new_v4().to_string();
    let now = now_ms();
    db.execute(
        "INSERT INTO vault_item(uuid, handle, name, description, allowed_sites, sync_enabled, value, created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
        params![uuid, input.key, name, input.description.trim(),
            serde_json::to_string(&sites).map_err(|e| e.to_string())?,
            input.sync_enabled, seal_value(key, &uuid, value)?, now],
    ).map_err(|e| format!("保存条目失败: {e}"))?;
    // Verify the stored ciphertext decrypts to what was entered before reporting success.
    let stored = load(db, &uuid)?;
    if open_value(key, &uuid, &stored.value)? != value { return Err("条目写入校验失败".into()); }
    Ok(stored.meta(Some(key)))
}

/// All or nothing, for a form the assistant asked the user to fill in.
pub(super) fn create_many_with(db: &Connection, key: &secrets::SecretKey, inputs: &[CreateInput]) -> Result<Vec<ItemMeta>, String> {
    init(db)?;
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    let created = inputs.iter().map(|input| create_with(&tx, key, input)).collect::<Result<Vec<_>, _>>()?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(created)
}

pub(super) fn update_with(db: &Connection, key: Option<&secrets::SecretKey>, uuid: &str, input: &UpdateInput) -> Result<ItemMeta, String> {
    init(db)?;
    let current = load(db, uuid)?;
    let name = input.name.as_deref().map(str::trim).unwrap_or(&current.name).to_owned();
    if name.is_empty() { return Err("名称不能为空".into()); }
    let entry_key = input.key.clone().unwrap_or_else(|| current.handle.clone());
    if entry_key != current.handle {
        validate_key(&entry_key)?;
        if key_taken(db, &entry_key, Some(uuid))? { return Err(format!("键 {entry_key} 已被使用")); }
    }
    let description = input.description.as_deref().map(str::trim).unwrap_or(&current.description).to_owned();
    let sites = match &input.allowed_sites {
        Some(sites) => serde_json::to_string(&normalize_sites(sites)?).map_err(|e| e.to_string())?,
        None => current.allowed_sites.clone(),
    };
    let sync_enabled = input.sync_enabled.unwrap_or(current.sync_enabled);
    db.execute(
        "UPDATE vault_item SET name=?2, handle=?3, description=?4, allowed_sites=?5, sync_enabled=?6, updated_at=?7 WHERE uuid=?1",
        params![uuid, name, entry_key, description, sites, sync_enabled, now_ms()],
    ).map_err(|e| format!("更新条目失败: {e}"))?;
    Ok(load(db, uuid)?.meta(key))
}

pub(super) fn replace_secret_with(db: &Connection, key: &secrets::SecretKey, uuid: &str, value: &SecretValue) -> Result<ItemMeta, String> {
    init(db)?;
    load(db, uuid)?;
    let value = value.checked()?;
    db.execute("UPDATE vault_item SET value=?2, updated_at=?3 WHERE uuid=?1", params![uuid, seal_value(key, uuid, value)?, now_ms()])
        .map_err(|e| format!("替换值失败: {e}"))?;
    let stored = load(db, uuid)?;
    if open_value(key, uuid, &stored.value)? != value { return Err("条目写入校验失败".into()); }
    Ok(stored.meta(Some(key)))
}

/// Add an origin to the entry's allowed sites (learned from the user's "always allow").
pub(super) fn approve_site_with(db: &Connection, uuid: &str, origin: &str) -> Result<(), String> {
    init(db)?;
    let current = load(db, uuid)?;
    let origin = normalize_origin(origin)?;
    let mut sites = current.sites();
    if sites.contains(&origin) { return Ok(()); }
    sites.push(origin);
    db.execute("UPDATE vault_item SET allowed_sites=?2, updated_at=?3 WHERE uuid=?1",
        params![uuid, serde_json::to_string(&sites).map_err(|e| e.to_string())?, now_ms()])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn delete_with(db: &Connection, uuid: &str) -> Result<(), String> {
    init(db)?;
    let deleted = db.execute("DELETE FROM vault_item WHERE uuid=?1", [uuid]).map_err(|e| e.to_string())?;
    if deleted == 0 { return Err("条目不存在或已删除".into()); }
    db.execute("DELETE FROM vault_usage_log WHERE item_uuid=?1", [uuid]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Decrypt entries by key for placeholders. Unknown keys are `credential_not_found`.
pub(super) fn resolve_keys_with(db: &Connection, key: Option<&secrets::SecretKey>, keys: &[String]) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    init(db).map_err(|e| ResolveError::new("vault_error", e))?;
    let mut out = BTreeMap::new();
    for entry_key in keys {
        let rows = db.prepare(&format!("SELECT {COLUMNS} FROM vault_item WHERE handle=?1")).and_then(|mut s| {
            s.query_map([entry_key], row)?.collect::<rusqlite::Result<Vec<_>>>()
        }).map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
        let item = match rows.as_slice() {
            [] => return Err(ResolveError::new("credential_not_found", format!(
                "保险箱里没有键为 {entry_key} 的条目。可先调用 vault_list 查看，或用 vault_request_input 请用户填写"))),
            [item] => item,
            _ => return Err(ResolveError::new("credential_ambiguous", format!("有多个条目使用键 {entry_key}（可能来自同步），请用户在保险箱中改名"))),
        };
        out.insert(entry_key.clone(), item.resolve(key)?);
    }
    Ok(out)
}

/// Re-read after the user approves. Validation and any "always allow" writes share one transaction:
/// a stale approval must neither use an old value nor grant a destination to its replacement.
pub(super) fn confirm_use_with(db: &Connection, key: &secrets::SecretKey, expected: &BTreeMap<String, Resolved>,
    always: Option<(&[String], &str)>) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    let tx = db.unchecked_transaction().map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
    let keys: Vec<String> = expected.keys().cloned().collect();
    let current = resolve_keys_with(&tx, Some(key), &keys)?;
    for (entry_key, previous) in expected {
        let entry = &current[entry_key];
        let mut approved_meta = previous.meta.clone();
        // Another completed use is not an edit to the value or the user's policy.
        approved_meta.last_used_at = entry.meta.last_used_at;
        if entry.meta != approved_meta || entry.value != previous.value || entry.config_bindings != previous.config_bindings {
            return Err(ResolveError::new("credential_changed",
                "等待确认期间保险箱条目已更改，本次调用已取消，请重新发起并确认"));
        }
    }
    if let Some((ids, origin)) = always {
        for id in ids {
            if !current.values().any(|entry| &entry.meta.id == id) {
                return Err(ResolveError::new("vault_error", "确认条目不在本次调用中"));
            }
            approve_site_with(&tx, id, origin).map_err(|e| ResolveError::new("vault_error", e))?;
        }
    }
    tx.commit().map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
    Ok(current)
}

/// Decrypt entries by id for configuration references. Missing ids are `credential_not_found`.
pub(super) fn resolve_ids_with(db: &Connection, key: Option<&secrets::SecretKey>, uuids: &[String]) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    init(db).map_err(|e| ResolveError::new("vault_error", e))?;
    let mut out = BTreeMap::new();
    for uuid in uuids {
        let item = db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [uuid], row).optional()
            .map_err(|e| ResolveError::new("vault_error", e.to_string()))?
            .ok_or_else(|| ResolveError::new("credential_not_found", format!("引用的条目 {uuid} 不在保险箱中（可能已被删除）")))?;
        out.insert(uuid.clone(), item.resolve(key)?);
    }
    Ok(out)
}

/// Metadata and confirmations for the given ids without decrypting. Unknown ids are left out.
pub(crate) fn metas_with(db: &Connection, uuids: &[String]) -> Result<BTreeMap<String, (ItemMeta, Vec<ConfigBinding>)>, String> {
    init(db)?;
    let mut out = BTreeMap::new();
    for uuid in uuids {
        if let Some(item) = db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [uuid], row).optional().map_err(|e| e.to_string())? {
            out.insert(uuid.clone(), (item.meta(None), item.bindings()));
        }
    }
    Ok(out)
}

/// Record that the user confirmed `target` for references to this item. Returns whether anything changed.
pub(crate) fn bind_config_with(db: &Connection, uuid: &str, source: &str, target: &str, label: &str, detail: &str) -> Result<bool, String> {
    init(db)?;
    let current = load(db, uuid)?;
    let before = current.bindings();
    let mut bindings: Vec<ConfigBinding> = before.iter().filter(|b| b.source != source || b.target == target).cloned().collect();
    match bindings.iter_mut().find(|b| b.source == source && b.target == target) {
        Some(existing) => { existing.label = label.to_owned(); existing.detail = detail.to_owned(); }
        None => bindings.push(ConfigBinding { target: target.into(), source: source.into(), label: label.into(), detail: detail.into(), approved_at: now_ms() }),
    }
    if bindings == before { return Ok(false); }
    db.execute("UPDATE vault_item SET config_bindings=?2, updated_at=?3 WHERE uuid=?1",
        params![uuid, serde_json::to_string(&bindings).map_err(|e| e.to_string())?, now_ms()])
        .map_err(|e| e.to_string())?;
    Ok(true)
}

fn unique_key(db: &Connection, base: &str) -> Result<String, String> {
    let base: String = base.chars().take(36).collect();
    let mut candidate = base.clone();
    let mut index = 2;
    while key_taken(db, &candidate, None)? { candidate = format!("{base}_{index}"); index += 1; }
    Ok(candidate)
}

/// Convert entries from development builds (several auth-kind fields) to one value each. A Basic
/// entry becomes `<key>_username` and `<key>_password`. Entries this device cannot decrypt yet stay
/// marked and are converted later.
pub(super) fn migrate_legacy_with(db: &Connection, key: &secrets::SecretKey) -> Result<(), String> {
    init(db)?;
    let legacy = db.prepare("SELECT uuid, auth_kind FROM vault_legacy_item").map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (uuid, kind) in legacy {
        let Some(current) = db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [&uuid], row).optional().map_err(|e| e.to_string())? else {
            db.execute("DELETE FROM vault_legacy_item WHERE uuid=?1", [&uuid]).map_err(|e| e.to_string())?;
            continue;
        };
        let Ok(payload) = open_payload(key, &uuid, &current.value) else { continue };
        if payload.value.is_none() {
            let fields = payload.fields;
            match (kind.as_str(), fields.get("username"), fields.get("password")) {
                ("basic", Some(username), Some(password)) => {
                    let username_key = unique_key(db, &format!("{}_username", current.handle))?;
                    db.execute("UPDATE vault_item SET handle=?2, name=?3, value=?4, updated_at=?5 WHERE uuid=?1",
                        params![uuid, username_key, format!("{}（用户名）", current.name), seal_value(key, &uuid, username)?, now_ms()])
                        .map_err(|e| e.to_string())?;
                    let password_uuid = Uuid::new_v4().to_string();
                    let password_key = unique_key(db, &format!("{}_password", current.handle))?;
                    db.execute(
                        "INSERT INTO vault_item(uuid, handle, name, description, allowed_sites, sync_enabled, value, created_at, updated_at)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
                        params![password_uuid, password_key, format!("{}（密码）", current.name), current.description,
                            current.allowed_sites, current.sync_enabled, seal_value(key, &password_uuid, password)?, now_ms()],
                    ).map_err(|e| e.to_string())?;
                }
                _ => {
                    let Some(value) = fields.values().next() else { continue };
                    db.execute("UPDATE vault_item SET value=?2, updated_at=?3 WHERE uuid=?1",
                        params![uuid, seal_value(key, &uuid, value)?, now_ms()]).map_err(|e| e.to_string())?;
                }
            }
            info!(uuid = %uuid, kind = %kind, "Converted a development-build vault entry to a single value");
        }
        db.execute("DELETE FROM vault_legacy_item WHERE uuid=?1", [&uuid]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn has_legacy(db: &Connection) -> bool {
    db.query_row("SELECT EXISTS(SELECT 1 FROM vault_legacy_item)", [], |r| r.get(0)).unwrap_or(false)
}

pub(super) fn log_usage_with(db: &Connection, usage: &NewUsage<'_>) -> Result<(), String> {
    init(db)?;
    let now = now_ms();
    db.execute(
        "INSERT INTO vault_usage_log(item_uuid, conversation_id, assistant_id, tool, destination, outcome, reason, created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![usage.item_id, usage.conversation_id, usage.assistant_id, usage.tool, usage.destination,
            usage.outcome, usage.reason, now],
    ).map_err(|e| e.to_string())?;
    if usage.outcome == "ok" {
        // last_used_at is local-only (not a synced column), so this does not create sync traffic.
        db.execute("UPDATE vault_item SET last_used_at=?2 WHERE uuid=?1", params![usage.item_id, now]).map_err(|e| e.to_string())?;
    }
    db.execute(
        "DELETE FROM vault_usage_log WHERE item_uuid=?1 AND id NOT IN
         (SELECT id FROM vault_usage_log WHERE item_uuid=?1 ORDER BY id DESC LIMIT ?2)",
        params![usage.item_id, USAGE_KEEP_PER_ITEM],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn usage_with(db: &Connection, uuid: &str, limit: i64) -> Result<Vec<UsageEntry>, String> {
    init(db)?;
    let rows = db.prepare(
        "SELECT id, item_uuid, conversation_id, assistant_id, tool, destination, outcome, reason, created_at
         FROM vault_usage_log WHERE item_uuid=?1 ORDER BY id DESC LIMIT ?2",
    ).map_err(|e| e.to_string())?
        .query_map(params![uuid, limit.clamp(1, USAGE_KEEP_PER_ITEM)], |r| Ok(UsageEntry {
            id: r.get(0)?, item_id: r.get(1)?, conversation_id: r.get(2)?, assistant_id: r.get(3)?,
            tool: r.get(4)?, destination: r.get(5)?, outcome: r.get(6)?, reason: r.get(7)?, created_at: r.get(8)?,
        })).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    Ok(rows)
}

// ---------- AppHandle wrappers ----------

fn backend_name() -> Option<&'static str> {
    if cfg!(target_os = "windows") { Some("Windows 凭据管理器") }
    else if cfg!(target_os = "macos") { Some("macOS 钥匙串") }
    else if cfg!(target_os = "linux") { Some("Secret Service（系统密钥环）") }
    else { None }
}

fn ensure_platform() -> Result<(), String> {
    match backend_name() {
        Some(_) => Ok(()),
        None => Err("此设备没有可用的系统安全存储，凭据保险箱已停用（不会以明文保存）".into()),
    }
}

/// Convert development-build entries as soon as the key is at hand.
fn after_unlock(db: &Connection, key: &secrets::SecretKey) {
    if has_legacy(db) {
        if let Err(error) = migrate_legacy_with(db, key) { warn!(%error, "Failed to convert development-build vault entries"); }
    }
}

/// Open system.db and the Vault key under the shared Vault lock. Provisions a local key on first use.
fn with_key<T>(app: &AppHandle, f: impl FnOnce(&Connection, &secrets::SecretKey) -> Result<T, String>) -> Result<T, String> {
    ensure_platform()?;
    let _lock = vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    init(&db.conn)?;
    let key = vault::key(app, &db.conn)?;
    after_unlock(&db.conn, &key);
    f(&db.conn, &key)
}

/// Like `with_key`, but tolerates a missing/unreadable key (metadata-only operations).
fn with_optional_key<T>(app: &AppHandle, f: impl FnOnce(&Connection, Option<&secrets::SecretKey>) -> Result<T, String>) -> Result<T, String> {
    ensure_platform()?;
    let _lock = vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    init(&db.conn)?;
    let key = if has_items(&db.conn)? { vault::key(app, &db.conn).ok() } else { None };
    if let Some(key) = &key { after_unlock(&db.conn, key); }
    f(&db.conn, key.as_ref())
}

fn with_resolver<T>(app: &AppHandle, f: impl FnOnce(&Connection, &secrets::SecretKey) -> Result<T, ResolveError>) -> Result<T, ResolveError> {
    ensure_platform().map_err(|e| ResolveError::new("vault_unavailable_on_device", e))?;
    let _lock = vault::LOCK.lock().map_err(|_| ResolveError::new("vault_error", "凭据库锁已损坏"))?;
    let db = SystemDatabase::new(app).map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
    init(&db.conn).map_err(|e| ResolveError::new("vault_error", e))?;
    let key = vault::key(app, &db.conn).map_err(|e| ResolveError::new("vault_locked", e))?;
    after_unlock(&db.conn, &key);
    f(&db.conn, &key)
}

pub(crate) fn capability(app: &AppHandle) -> Capability {
    let Some(backend) = backend_name() else {
        return Capability {
            available: false, backend: None, reason: ensure_platform().err(),
            sync_state: "unavailable".into(), sync_message: "此设备不支持凭据保险箱".into(), pending_from_other_devices: 0,
        };
    };
    let (sync_state, sync_message) = match vault::state(app) {
        Ok(state) => {
            let message = match &state {
                secrets::SecretState::Ready(_) => "端到端加密同步已开启".to_string(),
                secrets::SecretState::NotConfigured => "未设置同步口令，条目只保存在本机".to_string(),
                secrets::SecretState::NeedsPassphrase => "其他设备已开启加密同步，输入同步口令后才能同步条目".to_string(),
                secrets::SecretState::Mismatch => "同步口令已在其他设备更换，请输入新口令".to_string(),
                secrets::SecretState::Unavailable(error) => format!("凭据库不可用：{error}"),
            };
            (state.code().to_string(), message)
        }
        Err(error) => ("unavailable".to_string(), format!("读取凭据库状态失败：{error}")),
    };
    let pending = open_sync_db(app).ok().and_then(|sync| sync.query_row(
        "SELECT COUNT(*) FROM sync_secret_pending WHERE json_valid(change_json) AND json_extract(change_json,'$.object_type')=?1",
        [OBJECT_TYPE], |r| r.get(0)).ok()).unwrap_or(0);
    let available = sync_state != "unavailable";
    Capability {
        available, backend: Some(backend), reason: (!available).then(|| sync_message.clone()),
        sync_state, sync_message, pending_from_other_devices: pending,
    }
}

pub(crate) fn list(app: &AppHandle) -> Result<Vec<ItemMeta>, String> {
    with_optional_key(app, list_with)
}

pub(crate) fn create(app: &AppHandle, input: &CreateInput) -> Result<ItemMeta, String> {
    let meta = with_key(app, |db, key| create_with(db, key, input))?;
    schedule_sync_after_local_change(app);
    Ok(meta)
}

pub(crate) fn create_many(app: &AppHandle, inputs: &[CreateInput]) -> Result<Vec<ItemMeta>, String> {
    let created = with_key(app, |db, key| create_many_with(db, key, inputs))?;
    schedule_sync_after_local_change(app);
    Ok(created)
}

pub(crate) fn update(app: &AppHandle, uuid: &str, input: &UpdateInput) -> Result<ItemMeta, String> {
    let meta = with_optional_key(app, |db, key| update_with(db, key, uuid, input))?;
    schedule_sync_after_local_change(app);
    Ok(meta)
}

pub(crate) fn replace_secret(app: &AppHandle, uuid: &str, value: &SecretValue) -> Result<ItemMeta, String> {
    let meta = with_key(app, |db, key| replace_secret_with(db, key, uuid, value))?;
    schedule_sync_after_local_change(app);
    Ok(meta)
}

pub(crate) fn delete(app: &AppHandle, uuid: &str) -> Result<(), String> {
    with_optional_key(app, |db, _| delete_with(db, uuid))?;
    schedule_sync_after_local_change(app);
    Ok(())
}

/// Decrypt entries named in placeholders (trusted resolver only).
pub(crate) fn resolve_keys(app: &AppHandle, keys: &[String]) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    with_resolver(app, |db, key| resolve_keys_with(db, Some(key), keys))
}

pub(crate) fn confirm_use(app: &AppHandle, expected: &BTreeMap<String, Resolved>, always: Option<(&[String], &str)>) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    let current = with_resolver(app, |db, key| confirm_use_with(db, key, expected, always))?;
    if always.is_some() { schedule_sync_after_local_change(app); }
    Ok(current)
}

/// Decrypt the referenced entries for a configuration reference (trusted resolver only).
pub(crate) fn resolve_ids(app: &AppHandle, uuids: &[String]) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    with_resolver(app, |db, key| resolve_ids_with(db, Some(key), uuids))
}

pub(crate) fn metas(app: &AppHandle, uuids: &[String]) -> Result<BTreeMap<String, (ItemMeta, Vec<ConfigBinding>)>, String> {
    ensure_platform()?;
    let _lock = vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    metas_with(&db.conn, uuids)
}

/// Record confirmations for several items at once. Only Settings save commands call this.
pub(crate) fn bind_config(app: &AppHandle, uuids: &[String], source: &str, target: &str, label: &str, detail: &str) -> Result<bool, String> {
    ensure_platform()?;
    let changed = {
        let _lock = vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
        let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
        let mut changed = false;
        for uuid in uuids { changed |= bind_config_with(&db.conn, uuid, source, target, label, detail)?; }
        changed
    };
    if changed { schedule_sync_after_local_change(app); }
    Ok(changed)
}

pub(crate) fn log_usage(app: &AppHandle, usage: &NewUsage<'_>) {
    let result = SystemDatabase::new(app).map_err(|e| e.to_string()).and_then(|db| log_usage_with(&db.conn, usage));
    if let Err(error) = result { warn!(%error, item = usage.item_id, "Failed to record vault usage"); }
}

pub(crate) fn usage(app: &AppHandle, uuid: &str, limit: i64) -> Result<Vec<UsageEntry>, String> {
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    usage_with(&db.conn, uuid, limit)
}

// ---------- sync ----------

/// Scan side: re-seal each local value with the sync key so the wire envelope is deterministic.
/// Development-build entries that are not converted yet are held back until they are.
pub(super) fn seal_snapshots(sync: &Connection, key: &secrets::SecretKey, snapshots: Vec<LocalObjectSnapshot>) -> Result<Vec<LocalObjectSnapshot>, String> {
    let mut out = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        let mut payload: JsonValue = serde_json::from_str(&snapshot.payload_json).map_err(|e| e.to_string())?;
        let fields = payload["fields"].as_object_mut().ok_or("保险箱同步字段缺失")?;
        let uuid = field_as_string(fields, "uuid").ok_or("保险箱同步缺少 uuid")?;
        let stored = field_as_string(fields, "value").ok_or("保险箱同步缺少密文")?;
        let plain = open_payload(key, &uuid, &stored)?;
        let Some(value) = plain.value else {
            warn!(uuid = %uuid, "Holding back a vault entry that still has the development-build format");
            continue;
        };
        let sealed: JsonValue = serde_json::from_str(&seal_value(key, &uuid, &value)?).map_err(|e| e.to_string())?;
        fields.insert("value".into(), sealed);
        let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        out.push(LocalObjectSnapshot { payload_hash: sync_payload_hash(sync, Some(&payload))?, payload_json, ..snapshot });
    }
    Ok(out)
}

/// Apply side: verify the envelope opens with the current key, then store it re-sealed locally.
pub(super) fn apply(sync: &Connection, db: &Connection, spec: &SyncTableSpec, change: &PullChange, fields: &Map<String, JsonValue>) -> Result<(), String> {
    secrets::remember_server_copy(sync, change)?;
    let Some(key) = secrets::current_key() else { return secrets::park(sync, change, "需要同步口令才能解密保险箱条目"); };
    init(db)?;
    let uuid = field_as_string(fields, "uuid").ok_or("保险箱条目缺少 uuid")?;
    Uuid::parse_str(&uuid).map_err(|_| "保险箱条目 uuid 无效")?;
    let envelope = fields.get("value").ok_or("保险箱条目缺少密文")?;
    let raw = match secrets::open(&key, &object_id(&uuid), SECRET_NAME, envelope) {
        Ok(raw) => raw,
        Err(_) => return secrets::park(sync, change, "保险箱条目的密钥不匹配或密文校验失败，保留本机值"),
    };
    let payload: SecretPayload = serde_json::from_str(&raw).map_err(|_| "保险箱条目内容格式无效")?;
    let value = payload.value.ok_or("保险箱条目缺少值")?;
    let mut local = fields.clone();
    local.insert("value".into(), json!(seal_value(&key, &uuid, &value)?));
    let mapped = find_local_id_by_object_id(sync, OBJECT_TYPE, &change.object_id)?;
    let target = mapped.or(find_row_by_unique_key(db, spec, &local)?);
    let stale = match target { Some(id) => remote_row_change_is_stale(sync, change, id)?, None => false };
    if !stale {
        match target {
            Some(id) => update_row(db, spec, id, &local)?,
            None => upsert_by_unique_key(sync, db, spec, &change.object_id, &local)?,
        }
        let id: i64 = db.query_row("SELECT id FROM vault_item WHERE uuid=?1", [&uuid], |r| r.get(0)).map_err(|e| e.to_string())?;
        remap_sync_object_map(sync, OBJECT_TYPE, spec.table, id, &change.object_id)?;
    }
    upsert_shadow_from_remote_with_fields(sync, spec, change, fields)?;
    sync.execute("DELETE FROM sync_secret_pending WHERE object_id=?1 AND version<=?2", params![change.object_id, change.version])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
#[path = "personal_vault_tests.rs"]
mod tests;

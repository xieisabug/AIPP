//! Personal credential vault (凭据保险箱).
//!
//! Each item stores non-secret metadata in plaintext columns and the secret fields in `value`,
//! sealed with the shared Vault key (AAD bound to the item UUID, which is stable across devices).
//! No public API returns secret values or fragments: only `resolve` (crate-internal, used by the
//! trusted executor) decrypts, and its result type never prints its contents.
use super::*;
use std::collections::BTreeMap;

pub(super) const OBJECT_TYPE: &str = "vault.item";
const SECRET_NAME: &str = "secret";
const USAGE_KEEP_PER_ITEM: i64 = 500;

pub(crate) fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS vault_item (
            id INTEGER PRIMARY KEY,
            uuid TEXT NOT NULL UNIQUE,
            handle TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            auth_kind TEXT NOT NULL,
            injection TEXT NOT NULL,
            allowed_sites TEXT NOT NULL DEFAULT '[]',
            sync_enabled INTEGER NOT NULL DEFAULT 1,
            value TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_used_at INTEGER,
            config_bindings TEXT NOT NULL DEFAULT '[]');
        CREATE INDEX IF NOT EXISTS idx_vault_item_handle ON vault_item(handle);
        CREATE TABLE IF NOT EXISTS vault_usage_log (
            id INTEGER PRIMARY KEY,
            item_uuid TEXT NOT NULL,
            conversation_id INTEGER,
            assistant_id INTEGER,
            method TEXT NOT NULL,
            url TEXT NOT NULL,
            status INTEGER,
            outcome TEXT NOT NULL,
            reason TEXT,
            created_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_vault_usage_item ON vault_usage_log(item_uuid, id);",
    )
    .map_err(|e| format!("初始化凭据保险箱失败: {e}"))?;
    // Development databases created before configuration references existed.
    let has_bindings: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('vault_item') WHERE name='config_bindings')", [], |r| r.get(0))
        .map_err(|e| format!("初始化凭据保险箱失败: {e}"))?;
    if !has_bindings {
        db.execute_batch("ALTER TABLE vault_item ADD COLUMN config_bindings TEXT NOT NULL DEFAULT '[]'")
            .map_err(|e| format!("初始化凭据保险箱失败: {e}"))?;
    }
    Ok(())
}

// ---------- types ----------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind { Bearer, Header, Basic, Query }

impl AuthKind {
    fn as_str(self) -> &'static str {
        match self { Self::Bearer => "bearer", Self::Header => "header", Self::Basic => "basic", Self::Query => "query" }
    }
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "bearer" => Ok(Self::Bearer), "header" => Ok(Self::Header),
            "basic" => Ok(Self::Basic), "query" => Ok(Self::Query),
            other => Err(format!("未知的认证方式: {other}")),
        }
    }
}

/// Where a secret field is placed in the outgoing request. Declared by the user, never by the model.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "at")]
pub enum InjectionRule {
    Header { name: String, #[serde(default)] prefix: String, field: String },
    Query { name: String, field: String },
    /// `Authorization: Basic base64(username:password)` from the `username` / `password` fields.
    Basic,
}

impl InjectionRule {
    fn summary(&self) -> String {
        match self {
            Self::Header { name, prefix, .. } => format!("{name}: {prefix}<值>"),
            Self::Query { name, .. } => format!("?{name}=<值>"),
            Self::Basic => "Authorization: Basic <用户名:密码>".into(),
        }
    }
    pub(crate) fn header_name(&self) -> Option<&str> {
        match self { Self::Header { name, .. } => Some(name), Self::Basic => Some("authorization"), Self::Query { .. } => None }
    }
}

/// The secret fields an item holds, labelled for the reference picker. Derived from the rules, so no
/// decryption is needed (works while the vault is locked).
fn field_metas(kind: AuthKind, rules: &[InjectionRule]) -> Vec<FieldMeta> {
    let field = |key: &str, label: String| FieldMeta { key: key.into(), label };
    rules.iter().flat_map(|rule| match rule {
        InjectionRule::Basic => vec![field("username", "用户名".into()), field("password", "密码".into())],
        InjectionRule::Header { field: key, .. } if kind == AuthKind::Bearer => vec![field(key, "令牌".into())],
        InjectionRule::Header { name, field: key, .. } => vec![field(key, name.clone())],
        InjectionRule::Query { name, field: key } => vec![field(key, format!("参数 {name}"))],
    }).collect()
}

/// Input describing the secret and how it is injected. Values are consumed on save.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInput {
    pub kind: AuthKind,
    #[serde(default)] pub token: Option<String>,
    #[serde(default)] pub username: Option<String>,
    #[serde(default)] pub password: Option<String>,
    #[serde(default)] pub headers: Vec<NamedSecret>,
    #[serde(default)] pub query: Vec<NamedSecret>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedSecret {
    pub name: String,
    #[serde(default)] pub prefix: String,
    pub value: String,
}

impl std::fmt::Debug for AuthInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthInput").field("kind", &self.kind).finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInput {
    pub name: String,
    pub handle: String,
    #[serde(default)] pub description: String,
    pub auth: AuthInput,
    #[serde(default)] pub allowed_sites: Vec<String>,
    #[serde(default = "default_true")] pub sync_enabled: bool,
}
fn default_true() -> bool { true }

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInput {
    pub name: Option<String>,
    pub handle: Option<String>,
    pub description: Option<String>,
    pub allowed_sites: Option<Vec<String>>,
    pub sync_enabled: Option<bool>,
}

/// Metadata visible to the UI and to the model. Contains no secret values or fragments.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ItemMeta {
    pub id: String,
    pub handle: String,
    pub name: String,
    pub description: String,
    pub auth_kind: AuthKind,
    pub injection: Vec<String>,
    /// Secret fields a configuration reference may pick (`vault://<id>#<key>`). Labels only.
    pub fields: Vec<FieldMeta>,
    pub allowed_sites: Vec<String>,
    pub sync_enabled: bool,
    /// `ready` or `locked` (sealed under a key this device cannot open yet).
    pub status: &'static str,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FieldMeta { pub key: String, pub label: String }

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

/// Decrypted credential for the trusted executor only. Never serialized, never printed.
pub(crate) struct Resolved {
    pub meta: ItemMeta,
    pub rules: Vec<InjectionRule>,
    pub fields: BTreeMap<String, String>,
    pub config_bindings: Vec<ConfigBinding>,
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolved").field("handle", &self.meta.handle).finish_non_exhaustive()
    }
}

impl Resolved {
    /// Every secret value, for response redaction.
    pub(crate) fn secret_values(&self) -> Vec<String> {
        let mut values: Vec<String> = self.fields.values().filter(|v| !v.is_empty()).cloned().collect();
        if self.rules.iter().any(|r| matches!(r, InjectionRule::Basic)) {
            if let Some(pair) = self.basic_pair() { values.push(pair); }
        }
        values
    }
    pub(crate) fn basic_pair(&self) -> Option<String> {
        Some(format!("{}:{}", self.fields.get("username")?, self.fields.get("password")?))
    }
}

/// Typed failure for resolution so the executor can return a stable error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolveError { pub code: &'static str, pub message: String }

impl ResolveError {
    fn new(code: &'static str, message: impl Into<String>) -> Self { Self { code, message: message.into() } }
}

#[derive(Serialize, Deserialize)]
struct SecretPayload { fields: BTreeMap<String, String> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub id: i64,
    pub item_id: String,
    pub conversation_id: Option<i64>,
    pub assistant_id: Option<i64>,
    pub method: String,
    pub url: String,
    pub status: Option<i64>,
    pub outcome: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct NewUsage<'a> {
    pub item_id: &'a str,
    pub conversation_id: Option<i64>,
    pub assistant_id: Option<i64>,
    pub method: &'a str,
    pub url: &'a str,
    pub status: Option<i64>,
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

pub(crate) fn validate_handle(handle: &str) -> Result<(), String> {
    let ok = handle.len() >= 2 && handle.len() <= 40
        && handle.starts_with(|c: char| c.is_ascii_lowercase())
        && handle.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok { Ok(()) } else { Err("引用名需为 2–40 位小写字母、数字或下划线，并以字母开头".into()) }
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

fn header_name_ok(name: &str) -> bool {
    !name.is_empty() && reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_ok()
}

/// Turn user input into injection rules plus the secret field map. Rejects empty values.
fn build_auth(input: &AuthInput) -> Result<(Vec<InjectionRule>, BTreeMap<String, String>), String> {
    let mut fields = BTreeMap::new();
    let mut rules = Vec::new();
    let non_empty = |v: &Option<String>, label: &str| -> Result<String, String> {
        v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned).ok_or_else(|| format!("{label}不能为空"))
    };
    match input.kind {
        AuthKind::Bearer => {
            fields.insert("token".into(), non_empty(&input.token, "令牌")?);
            rules.push(InjectionRule::Header { name: "Authorization".into(), prefix: "Bearer ".into(), field: "token".into() });
        }
        AuthKind::Basic => {
            fields.insert("username".into(), non_empty(&input.username, "用户名")?);
            fields.insert("password".into(), non_empty(&input.password, "密码")?);
            rules.push(InjectionRule::Basic);
        }
        AuthKind::Header => {
            if input.headers.is_empty() { return Err("至少需要一个请求头".into()); }
            for (index, header) in input.headers.iter().enumerate() {
                let name = header.name.trim();
                if !header_name_ok(name) { return Err(format!("请求头名称无效: {name}")); }
                let value = header.value.trim();
                if value.is_empty() { return Err(format!("请求头 {name} 的值不能为空")); }
                let field = format!("header_{index}");
                fields.insert(field.clone(), value.to_owned());
                rules.push(InjectionRule::Header { name: name.to_owned(), prefix: header.prefix.clone(), field });
            }
        }
        AuthKind::Query => {
            if input.query.is_empty() { return Err("至少需要一个查询参数".into()); }
            for (index, param) in input.query.iter().enumerate() {
                let name = param.name.trim();
                if name.is_empty() { return Err("查询参数名不能为空".into()); }
                let value = param.value.trim();
                if value.is_empty() { return Err(format!("查询参数 {name} 的值不能为空")); }
                let field = format!("query_{index}");
                fields.insert(field.clone(), value.to_owned());
                rules.push(InjectionRule::Query { name: name.to_owned(), field });
            }
        }
    }
    let mut names: Vec<String> = rules.iter().filter_map(|r| r.header_name().map(str::to_ascii_lowercase)).collect();
    let before = names.len();
    names.sort(); names.dedup();
    if names.len() != before { return Err("同一个请求头不能注入两次".into()); }
    Ok((rules, fields))
}

// ---------- crypto ----------

fn object_id(uuid: &str) -> String { format!("pvault:{uuid}") }

pub(super) fn seal_value(key: &secrets::SecretKey, uuid: &str, fields: &BTreeMap<String, String>) -> Result<String, String> {
    let raw = serde_json::to_string(&SecretPayload { fields: fields.clone() }).map_err(|_| "凭据编码失败")?;
    serde_json::to_string(&secrets::seal(key, &object_id(uuid), SECRET_NAME, &raw)?).map_err(|e| e.to_string())
}

pub(super) fn open_value(key: &secrets::SecretKey, uuid: &str, value: &str) -> Result<BTreeMap<String, String>, String> {
    let envelope: JsonValue = serde_json::from_str(value).map_err(|_| "保险箱密文格式无效")?;
    let raw = secrets::open(key, &object_id(uuid), SECRET_NAME, &envelope)
        .map_err(|_| format!("凭据 {uuid} 无法用本机密钥解密（可能来自其他设备、需要同步口令）"))?;
    let payload: SecretPayload = serde_json::from_str(&raw).map_err(|_| "保险箱内容格式无效")?;
    Ok(payload.fields)
}

/// Re-encrypt every item under a new key. Called inside the Vault key-switch transaction.
pub(super) fn reencrypt(db: &Connection, old: Option<&secrets::SecretKey>, new: &secrets::SecretKey) -> Result<(), String> {
    init(db)?;
    let rows = db.prepare("SELECT uuid, value FROM vault_item ORDER BY id").map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    for (uuid, value) in rows {
        let old = old.ok_or("保险箱已有凭据但缺少原加密密钥，无法迁移")?;
        let fields = open_value(old, &uuid, &value)?;
        db.execute("UPDATE vault_item SET value=?2 WHERE uuid=?1", params![uuid, seal_value(new, &uuid, &fields)?])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn has_items(db: &Connection) -> Result<bool, String> {
    init(db)?;
    db.query_row("SELECT EXISTS(SELECT 1 FROM vault_item)", [], |r| r.get(0)).map_err(|e| e.to_string())
}

// ---------- row access (pure, testable) ----------

const COLUMNS: &str = "uuid, handle, name, description, auth_kind, injection, allowed_sites, sync_enabled, value, created_at, updated_at, last_used_at, config_bindings";

struct Row {
    uuid: String, handle: String, name: String, description: String, auth_kind: String,
    injection: String, allowed_sites: String, sync_enabled: bool, value: String,
    created_at: i64, updated_at: i64, last_used_at: Option<i64>, config_bindings: String,
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        uuid: r.get(0)?, handle: r.get(1)?, name: r.get(2)?, description: r.get(3)?, auth_kind: r.get(4)?,
        injection: r.get(5)?, allowed_sites: r.get(6)?, sync_enabled: r.get(7)?, value: r.get(8)?,
        created_at: r.get(9)?, updated_at: r.get(10)?, last_used_at: r.get(11)?, config_bindings: r.get(12)?,
    })
}

impl Row {
    fn rules(&self) -> Result<Vec<InjectionRule>, String> {
        serde_json::from_str(&self.injection).map_err(|_| format!("凭据 {} 的注入规则损坏", self.handle))
    }
    fn bindings(&self) -> Vec<ConfigBinding> {
        serde_json::from_str(&self.config_bindings).unwrap_or_default()
    }
    fn meta(&self, key: Option<&secrets::SecretKey>) -> Result<ItemMeta, String> {
        let status = match key { Some(key) if open_value(key, &self.uuid, &self.value).is_ok() => "ready", _ => "locked" };
        let auth_kind = AuthKind::parse(&self.auth_kind)?;
        let rules = self.rules()?;
        Ok(ItemMeta {
            id: self.uuid.clone(), handle: self.handle.clone(), name: self.name.clone(),
            description: self.description.clone(), auth_kind,
            injection: rules.iter().map(InjectionRule::summary).collect(),
            fields: field_metas(auth_kind, &rules),
            allowed_sites: serde_json::from_str(&self.allowed_sites).unwrap_or_default(),
            sync_enabled: self.sync_enabled, status,
            created_at: self.created_at, updated_at: self.updated_at, last_used_at: self.last_used_at,
        })
    }
}

fn load(db: &Connection, uuid: &str) -> Result<Row, String> {
    db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [uuid], row)
        .optional().map_err(|e| e.to_string())?.ok_or_else(|| "凭据不存在或已删除".into())
}

fn handle_taken(db: &Connection, handle: &str, except: Option<&str>) -> Result<bool, String> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM vault_item WHERE handle=?1 AND uuid IS NOT ?2)", params![handle, except], |r| r.get(0))
        .map_err(|e| e.to_string())
}

fn now_ms() -> i64 { Utc::now().timestamp_millis() }

pub(super) fn list_with(db: &Connection, key: Option<&secrets::SecretKey>) -> Result<Vec<ItemMeta>, String> {
    init(db)?;
    let rows = db.prepare(&format!("SELECT {COLUMNS} FROM vault_item ORDER BY name COLLATE NOCASE, id")).map_err(|e| e.to_string())?
        .query_map([], row).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    rows.iter().map(|r| r.meta(key)).collect()
}

pub(super) fn create_with(db: &Connection, key: &secrets::SecretKey, input: &CreateInput) -> Result<ItemMeta, String> {
    init(db)?;
    let name = input.name.trim();
    if name.is_empty() { return Err("名称不能为空".into()); }
    validate_handle(&input.handle)?;
    if handle_taken(db, &input.handle, None)? { return Err(format!("引用名 {} 已被使用", input.handle)); }
    let (rules, fields) = build_auth(&input.auth)?;
    let sites = normalize_sites(&input.allowed_sites)?;
    let uuid = Uuid::new_v4().to_string();
    let now = now_ms();
    db.execute(
        "INSERT INTO vault_item(uuid, handle, name, description, auth_kind, injection, allowed_sites, sync_enabled, value, created_at, updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
        params![uuid, input.handle, name, input.description.trim(), input.auth.kind.as_str(),
            serde_json::to_string(&rules).map_err(|e| e.to_string())?,
            serde_json::to_string(&sites).map_err(|e| e.to_string())?,
            input.sync_enabled, seal_value(key, &uuid, &fields)?, now],
    ).map_err(|e| format!("保存凭据失败: {e}"))?;
    // Verify the stored ciphertext decrypts to what was entered before reporting success.
    let stored = load(db, &uuid)?;
    if open_value(key, &uuid, &stored.value)? != fields { return Err("凭据写入校验失败".into()); }
    stored.meta(Some(key))
}

pub(super) fn update_with(db: &Connection, key: Option<&secrets::SecretKey>, uuid: &str, input: &UpdateInput) -> Result<ItemMeta, String> {
    init(db)?;
    let current = load(db, uuid)?;
    let name = input.name.as_deref().map(str::trim).unwrap_or(&current.name).to_owned();
    if name.is_empty() { return Err("名称不能为空".into()); }
    let handle = input.handle.clone().unwrap_or_else(|| current.handle.clone());
    if handle != current.handle {
        validate_handle(&handle)?;
        if handle_taken(db, &handle, Some(uuid))? { return Err(format!("引用名 {handle} 已被使用")); }
    }
    let description = input.description.as_deref().map(str::trim).unwrap_or(&current.description).to_owned();
    let sites = match &input.allowed_sites {
        Some(sites) => serde_json::to_string(&normalize_sites(sites)?).map_err(|e| e.to_string())?,
        None => current.allowed_sites.clone(),
    };
    let sync_enabled = input.sync_enabled.unwrap_or(current.sync_enabled);
    db.execute(
        "UPDATE vault_item SET name=?2, handle=?3, description=?4, allowed_sites=?5, sync_enabled=?6, updated_at=?7 WHERE uuid=?1",
        params![uuid, name, handle, description, sites, sync_enabled, now_ms()],
    ).map_err(|e| format!("更新凭据失败: {e}"))?;
    load(db, uuid)?.meta(key)
}

pub(super) fn replace_secret_with(db: &Connection, key: &secrets::SecretKey, uuid: &str, auth: &AuthInput) -> Result<ItemMeta, String> {
    init(db)?;
    load(db, uuid)?;
    let (rules, fields) = build_auth(auth)?;
    db.execute(
        "UPDATE vault_item SET auth_kind=?2, injection=?3, value=?4, updated_at=?5 WHERE uuid=?1",
        params![uuid, auth.kind.as_str(), serde_json::to_string(&rules).map_err(|e| e.to_string())?,
            seal_value(key, uuid, &fields)?, now_ms()],
    ).map_err(|e| format!("替换凭据失败: {e}"))?;
    let stored = load(db, uuid)?;
    if open_value(key, uuid, &stored.value)? != fields { return Err("凭据写入校验失败".into()); }
    stored.meta(Some(key))
}

/// Add an origin to the item's allowed sites (learned from the user's "always allow").
pub(super) fn approve_site_with(db: &Connection, uuid: &str, origin: &str) -> Result<(), String> {
    init(db)?;
    let current = load(db, uuid)?;
    let origin = normalize_origin(origin)?;
    let mut sites: Vec<String> = serde_json::from_str(&current.allowed_sites).unwrap_or_default();
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
    if deleted == 0 { return Err("凭据不存在或已删除".into()); }
    db.execute("DELETE FROM vault_usage_log WHERE item_uuid=?1", [uuid]).map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) fn resolve_with(db: &Connection, key: Option<&secrets::SecretKey>, handle: &str) -> Result<Resolved, ResolveError> {
    init(db).map_err(|e| ResolveError::new("vault_error", e))?;
    let rows = db.prepare(&format!("SELECT {COLUMNS} FROM vault_item WHERE handle=?1")).and_then(|mut s| {
        s.query_map([handle], row)?.collect::<rusqlite::Result<Vec<_>>>()
    }).map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
    let item = match rows.as_slice() {
        [] => return Err(ResolveError::new("credential_not_found", format!("保险箱里没有引用名为 {handle} 的凭据，可先调用 vault_list_credentials 查看"))),
        [item] => item,
        _ => return Err(ResolveError::new("credential_ambiguous", format!("有多条凭据使用引用名 {handle}（可能来自同步），请用户在保险箱中改名"))),
    };
    let key = key.ok_or_else(|| ResolveError::new("vault_locked", "保险箱密钥不可用，请解锁系统安全存储"))?;
    let fields = open_value(key, &item.uuid, &item.value).map_err(|e| ResolveError::new("vault_locked", e))?;
    let meta = item.meta(Some(key)).map_err(|e| ResolveError::new("vault_error", e))?;
    let rules = item.rules().map_err(|e| ResolveError::new("vault_error", e))?;
    Ok(Resolved { meta, rules, fields, config_bindings: item.bindings() })
}

/// Decrypt items by id for configuration references. Missing ids are `credential_not_found`.
pub(super) fn resolve_ids_with(db: &Connection, key: Option<&secrets::SecretKey>, uuids: &[String]) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    init(db).map_err(|e| ResolveError::new("vault_error", e))?;
    let mut out = BTreeMap::new();
    for uuid in uuids {
        let item = db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [uuid], row).optional()
            .map_err(|e| ResolveError::new("vault_error", e.to_string()))?
            .ok_or_else(|| ResolveError::new("credential_not_found", format!("引用的凭据 {uuid} 不在保险箱中（可能已被删除）")))?;
        let key = key.ok_or_else(|| ResolveError::new("vault_locked", "保险箱密钥不可用，请解锁系统安全存储"))?;
        let fields = open_value(key, &item.uuid, &item.value).map_err(|e| ResolveError::new("vault_locked", e))?;
        let meta = item.meta(Some(key)).map_err(|e| ResolveError::new("vault_error", e))?;
        let rules = item.rules().map_err(|e| ResolveError::new("vault_error", e))?;
        out.insert(uuid.clone(), Resolved { meta, rules, fields, config_bindings: item.bindings() });
    }
    Ok(out)
}

/// Metadata and confirmations for the given ids without decrypting. Unknown ids are left out.
pub(crate) fn metas_with(db: &Connection, uuids: &[String]) -> Result<BTreeMap<String, (ItemMeta, Vec<ConfigBinding>)>, String> {
    init(db)?;
    let mut out = BTreeMap::new();
    for uuid in uuids {
        if let Some(item) = db.query_row(&format!("SELECT {COLUMNS} FROM vault_item WHERE uuid=?1"), [uuid], row).optional().map_err(|e| e.to_string())? {
            out.insert(uuid.clone(), (item.meta(None)?, item.bindings()));
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

pub(super) fn log_usage_with(db: &Connection, usage: &NewUsage<'_>) -> Result<(), String> {
    init(db)?;
    let now = now_ms();
    db.execute(
        "INSERT INTO vault_usage_log(item_uuid, conversation_id, assistant_id, method, url, status, outcome, reason, created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![usage.item_id, usage.conversation_id, usage.assistant_id, usage.method, usage.url,
            usage.status, usage.outcome, usage.reason, now],
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
        "SELECT id, item_uuid, conversation_id, assistant_id, method, url, status, outcome, reason, created_at
         FROM vault_usage_log WHERE item_uuid=?1 ORDER BY id DESC LIMIT ?2",
    ).map_err(|e| e.to_string())?
        .query_map(params![uuid, limit.clamp(1, USAGE_KEEP_PER_ITEM)], |r| Ok(UsageEntry {
            id: r.get(0)?, item_id: r.get(1)?, conversation_id: r.get(2)?, assistant_id: r.get(3)?,
            method: r.get(4)?, url: r.get(5)?, status: r.get(6)?, outcome: r.get(7)?, reason: r.get(8)?, created_at: r.get(9)?,
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

/// Open system.db and the Vault key under the shared Vault lock. Provisions a local key on first use.
fn with_key<T>(app: &AppHandle, f: impl FnOnce(&Connection, &secrets::SecretKey) -> Result<T, String>) -> Result<T, String> {
    ensure_platform()?;
    let _lock = vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    init(&db.conn)?;
    let key = vault::key(app, &db.conn)?;
    f(&db.conn, &key)
}

/// Like `with_key`, but tolerates a missing/unreadable key (metadata-only operations).
fn with_optional_key<T>(app: &AppHandle, f: impl FnOnce(&Connection, Option<&secrets::SecretKey>) -> Result<T, String>) -> Result<T, String> {
    ensure_platform()?;
    let _lock = vault::LOCK.lock().map_err(|_| "凭据库锁已损坏")?;
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    init(&db.conn)?;
    let key = if has_items(&db.conn)? { vault::key(app, &db.conn).ok() } else { None };
    f(&db.conn, key.as_ref())
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
                secrets::SecretState::NotConfigured => "未设置同步口令，凭据只保存在本机".to_string(),
                secrets::SecretState::NeedsPassphrase => "其他设备已开启加密同步，输入同步口令后才能同步凭据".to_string(),
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

pub(crate) fn update(app: &AppHandle, uuid: &str, input: &UpdateInput) -> Result<ItemMeta, String> {
    let meta = with_optional_key(app, |db, key| update_with(db, key, uuid, input))?;
    schedule_sync_after_local_change(app);
    Ok(meta)
}

pub(crate) fn replace_secret(app: &AppHandle, uuid: &str, auth: &AuthInput) -> Result<ItemMeta, String> {
    let meta = with_key(app, |db, key| replace_secret_with(db, key, uuid, auth))?;
    schedule_sync_after_local_change(app);
    Ok(meta)
}

pub(crate) fn approve_site(app: &AppHandle, uuid: &str, origin: &str) -> Result<(), String> {
    with_optional_key(app, |db, _| approve_site_with(db, uuid, origin))?;
    schedule_sync_after_local_change(app);
    Ok(())
}

pub(crate) fn delete(app: &AppHandle, uuid: &str) -> Result<(), String> {
    with_optional_key(app, |db, _| delete_with(db, uuid))?;
    schedule_sync_after_local_change(app);
    Ok(())
}

pub(crate) fn resolve(app: &AppHandle, handle: &str) -> Result<Resolved, ResolveError> {
    ensure_platform().map_err(|e| ResolveError::new("vault_unavailable_on_device", e))?;
    let _lock = vault::LOCK.lock().map_err(|_| ResolveError::new("vault_error", "凭据库锁已损坏"))?;
    let db = SystemDatabase::new(app).map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
    let key = vault::key(app, &db.conn).map_err(|e| ResolveError::new("vault_locked", e))?;
    resolve_with(&db.conn, Some(&key), handle)
}

/// Decrypt the referenced items for a configuration reference (trusted resolver only).
pub(crate) fn resolve_ids(app: &AppHandle, uuids: &[String]) -> Result<BTreeMap<String, Resolved>, ResolveError> {
    ensure_platform().map_err(|e| ResolveError::new("vault_unavailable_on_device", e))?;
    let _lock = vault::LOCK.lock().map_err(|_| ResolveError::new("vault_error", "凭据库锁已损坏"))?;
    let db = SystemDatabase::new(app).map_err(|e| ResolveError::new("vault_error", e.to_string()))?;
    let key = vault::key(app, &db.conn).map_err(|e| ResolveError::new("vault_locked", e))?;
    resolve_ids_with(&db.conn, Some(&key), uuids)
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
pub(super) fn seal_snapshots(sync: &Connection, key: &secrets::SecretKey, snapshots: Vec<LocalObjectSnapshot>) -> Result<Vec<LocalObjectSnapshot>, String> {
    snapshots.into_iter().map(|snapshot| {
        let mut payload: JsonValue = serde_json::from_str(&snapshot.payload_json).map_err(|e| e.to_string())?;
        let fields = payload["fields"].as_object_mut().ok_or("保险箱同步字段缺失")?;
        let uuid = field_as_string(fields, "uuid").ok_or("保险箱同步缺少 uuid")?;
        let stored = field_as_string(fields, "value").ok_or("保险箱同步缺少密文")?;
        let plain = open_value(key, &uuid, &stored)?;
        let sealed: JsonValue = serde_json::from_str(&seal_value(key, &uuid, &plain)?).map_err(|e| e.to_string())?;
        fields.insert("value".into(), sealed);
        let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        Ok(LocalObjectSnapshot { payload_hash: sync_payload_hash(sync, Some(&payload))?, payload_json, ..snapshot })
    }).collect()
}

/// Apply side: verify the envelope opens with the current key, then store it re-sealed locally.
pub(super) fn apply(sync: &Connection, db: &Connection, spec: &SyncTableSpec, change: &PullChange, fields: &Map<String, JsonValue>) -> Result<(), String> {
    secrets::remember_server_copy(sync, change)?;
    let Some(key) = secrets::current_key() else { return secrets::park(sync, change, "需要同步口令才能解密保险箱凭据"); };
    init(db)?;
    let uuid = field_as_string(fields, "uuid").ok_or("保险箱凭据缺少 uuid")?;
    Uuid::parse_str(&uuid).map_err(|_| "保险箱凭据 uuid 无效")?;
    let envelope = fields.get("value").ok_or("保险箱凭据缺少密文")?;
    let raw = match secrets::open(&key, &object_id(&uuid), SECRET_NAME, envelope) {
        Ok(raw) => raw,
        Err(_) => return secrets::park(sync, change, "保险箱凭据的密钥不匹配或密文校验失败，保留本机值"),
    };
    let payload: SecretPayload = serde_json::from_str(&raw).map_err(|_| "保险箱凭据内容格式无效")?;
    let mut local = fields.clone();
    local.insert("value".into(), json!(seal_value(&key, &uuid, &payload.fields)?));
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

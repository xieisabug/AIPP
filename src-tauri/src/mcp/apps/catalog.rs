use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const CATALOG_URL: &str = "https://aipp-helper.xiejingyang.com/api/apps";
/// The only secret that is not a request header: a confidential OAuth client's secret, used for the
/// token exchange/refresh/revoke and stored inside the encrypted OAuth credential.
pub const OAUTH_CLIENT_SECRET_FIELD: &str = "oauth_client_secret";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppCatalog {
    pub schema_version: u32,
    pub version: String,
    pub apps: Vec<AppDefinition>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppDefinition {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub publisher: String,
    pub category: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub notice: String,
    pub platforms: Vec<String>,
    pub connection: ConnectionRecipe,
    #[serde(default)]
    pub fields: Vec<ConfigField>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub plugin_source: Option<super::bundles::PluginSource>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionRecipe {
    pub transport: String,
    pub url: String,
    pub auth: String,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub oauth_resource: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub oauth_client_auth: Option<String>,
    #[serde(default)]
    pub oauth_redirect_uri: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub options: Vec<FieldOption>,
    #[serde(default)]
    pub visible_when: Option<FieldCondition>,
    /// A literal HTTP header name. Tokens are never substituted in arbitrary URLs or commands.
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub prefix: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldOption { pub label: String, pub value: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldCondition { pub field: String, pub equals: String }

pub fn public_https(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "服务地址格式无效".to_string())?;
    let host = url.host_str().ok_or("服务地址缺少主机")?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
        || url.fragment().is_some() || url.query().is_some() || host == "localhost"
        || host.ends_with(".localhost") || host.ends_with(".local")
        || host.parse::<std::net::IpAddr>().is_ok() || !host.contains('.') {
        return Err("应用目录端点必须为不含凭据、查询参数的公共 HTTPS 域名地址".into());
    }
    Ok(url)
}

#[derive(Deserialize)]
struct RawCatalog { schema_version: u32, version: String, apps: Vec<serde_json::Value> }

impl AppCatalog {
    /// Parse a published catalog entry by entry. Identity problems (empty/duplicate ids, the reserved
    /// `builtin:` namespace) still reject the whole catalog; an entry this client cannot use (new auth
    /// kind, field type or shape) is skipped and reported so older clients keep the rest of the catalog.
    pub fn parse_lenient(value: serde_json::Value) -> Result<(AppCatalog, Vec<String>), String> {
        let raw: RawCatalog = serde_json::from_value(value).map_err(|e| format!("应用目录格式无效或接口尚未发布：{e}"))?;
        if raw.schema_version != 1 { return Err(format!("应用目录协议 {} 不受支持，请升级客户端", raw.schema_version)); }
        if raw.apps.len() > 500 { return Err("应用目录条目超过上限".into()); }
        let mut ids = HashSet::new();
        let mut apps = Vec::new();
        let mut skipped = Vec::new();
        for entry in raw.apps {
            let id = entry["id"].as_str().unwrap_or_default().to_owned();
            if id.is_empty() || id.starts_with("builtin:") || !ids.insert(id.clone()) { return Err(format!("应用目录标识「{id}」为空、重复或占用内置命名空间")); }
            let label = entry["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(&id).to_owned();
            match serde_json::from_value::<AppDefinition>(entry).map_err(|e| format!("条目格式不受支持：{e}")).and_then(|app| validate_app(&app).map(|_| app)) {
                Ok(app) => apps.push(app),
                Err(reason) => skipped.push(format!("{label}（{reason}）")),
            }
        }
        let catalog = AppCatalog { schema_version: 1, version: raw.version, apps };
        catalog.validate()?;
        Ok((catalog, skipped))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 { return Err(format!("应用目录协议 {} 不受支持，请升级客户端", self.schema_version)); }
        if self.apps.len() > 500 { return Err("应用目录条目超过上限".into()); }
        let mut ids = HashSet::new();
        for app in &self.apps {
            if app.id.is_empty() || app.id.starts_with("builtin:") || !ids.insert(&app.id) { return Err("应用目录标识为空、重复或占用内置命名空间".into()); }
            validate_app(app)?;
        }
        Ok(())
    }
}

/// Everything about one entry except its identity, which is checked across the whole catalog.
pub fn validate_app(app: &AppDefinition) -> Result<(), String> {
    if app.version.is_empty() || app.name.is_empty() { return Err("应用缺少名称或版本".into()); }
    if app.connection.transport != "http" || !["none", "fields", "oauth"].contains(&app.connection.auth.as_str()) {
        return Err(format!("{} 的连接方式不受当前客户端支持", app.name));
    }
    public_https(&app.connection.url)?;
    if let Some(method)=&app.connection.oauth_client_auth {
        if !["none","client_secret_post","client_secret_basic"].contains(&method.as_str()) {return Err("不支持的 OAuth 客户端认证方式".into());}
    }
    if let Some(uri)=&app.connection.oauth_redirect_uri { super::oauth::redirect_target(uri)?; }
    if let Some(resource) = &app.connection.oauth_resource {
        let resource = public_https(resource)?;
        if resource.origin() != public_https(&app.connection.url)?.origin() { return Err("OAuth resource 必须与 MCP 服务同源".into()); }
    }
    let confidential = app.connection.auth == "oauth" && app.connection.oauth_client_auth.as_deref().is_some_and(|m| m != "none");
    let mut keys = HashSet::new();
    for f in &app.fields {
        if !keys.insert(&f.key) || f.key.is_empty() || !["text", "password", "select", "boolean", "number"].contains(&f.kind.as_str()) {
            return Err(format!("{} 的配置字段无效", app.name));
        }
        if f.secret && !f.default.is_empty() { return Err("秘密字段不允许提供默认值".into()); }
        if f.kind == "password" && !f.secret { return Err("密码字段必须声明为秘密".into()); }
        if f.secret && f.header.is_none() && !(confidential && f.key == OAUTH_CLIENT_SECRET_FIELD) {
            return Err(format!("{} 的秘密字段 {} 必须绑定请求头", app.name, f.key));
        }
        if let Some(h) = &f.header {
            let lower = h.to_ascii_lowercase();
            if reqwest::header::HeaderName::from_bytes(h.as_bytes()).is_err()
                || ["host", "cookie", "connection", "content-length", "transfer-encoding", "proxy-authorization", "x-aipp-credential-ref"].contains(&lower.as_str()) {
                return Err("应用包含不允许覆盖的 HTTP 请求头".into());
            }
            if lower == "authorization" && !f.secret { return Err("认证字段必须声明为秘密".into()); }
        }
    }
    for f in &app.fields {
        if f.visible_when.as_ref().is_some_and(|c| !keys.contains(&c.field) || c.field == f.key) { return Err("配置字段条件引用无效".into()); }
    }
    Ok(())
}

impl AppDefinition {
    /// Header names (lower-case) whose values are credentials. Authorization is always secret.
    pub fn secret_headers(&self) -> HashSet<String> {
        self.fields.iter().filter(|f| f.secret).filter_map(|f| f.header.as_ref().map(|h| h.to_ascii_lowercase()))
            .chain(std::iter::once("authorization".to_string())).collect()
    }

    pub fn configuration(&self, values: &HashMap<String, String>) -> Result<(HashMap<String, String>, HashMap<String, String>), String> {
        let mut config = HashMap::new();
        let mut headers = HashMap::new();
        for f in &self.fields {
            if let Some(c) = &f.visible_when {
                let parent = values.get(&c.field).cloned().or_else(|| self.fields.iter().find(|v| v.key == c.field).map(|v| v.default.clone())).unwrap_or_default();
                if parent != c.equals { continue; }
            }
            let value = values.get(&f.key).unwrap_or(&f.default).trim();
            if value.len() > 8192 || (f.required && value.is_empty()) { return Err(format!("请检查 {}", f.label)); }
            if !value.is_empty() && ((f.kind == "select" && !f.options.iter().any(|o| o.value == value))
                || (f.kind == "number" && value.parse::<f64>().map_or(true, |v| !v.is_finite()))
                || (f.kind == "boolean" && !["true", "false"].contains(&value))) { return Err(format!("{} 的值无效", f.label)); }
            if !f.secret { config.insert(f.key.clone(), value.to_string()); }
            if let Some(h) = &f.header {
                if !value.is_empty() {
                    let value = format!("{}{value}", f.prefix);
                    reqwest::header::HeaderValue::from_str(&value).map_err(|_| format!("{} 包含无效字符", f.label))?;
                    headers.insert(h.to_ascii_lowercase(), value);
                }
            }
        }
        Ok((config, headers))
    }
}

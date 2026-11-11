pub mod catalog;
pub mod credentials;
pub mod oauth;
pub mod access;
pub mod bundles;

use crate::db::connection::{params, Connection, OptionalExtension};
use crate::db::mcp_db::MCPDatabase;
use catalog::{AppCatalog, AppDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;
use tauri::Emitter;
use tokio::sync::Mutex;

static CHANGE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
struct LoginJob { reference: String, url: String, abort: tokio::task::AbortHandle }
static LOGIN_JOBS: LazyLock<Mutex<HashMap<i64, LoginJob>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SESSION: LazyLock<String> = LazyLock::new(|| uuid::Uuid::new_v4().to_string());

pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS app_catalog_cache (
        id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL, fetched_at TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS app_connection (
        server_id INTEGER PRIMARY KEY REFERENCES mcp_server(id) ON DELETE CASCADE,
        request_id TEXT NOT NULL UNIQUE, app_id TEXT NOT NULL, definition TEXT NOT NULL,
        label TEXT NOT NULL, config TEXT NOT NULL, credential_ref TEXT,
        status TEXT NOT NULL, error TEXT, verified_session TEXT);
        CREATE TABLE IF NOT EXISTS app_credential_cleanup (
        reference TEXT PRIMARY KEY, server_id INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS app_tool_call_binding (
        call_id INTEGER PRIMARY KEY REFERENCES mcp_tool_call(id) ON DELETE CASCADE,
        request_id TEXT NOT NULL);")
        .map_err(|e| format!("应用连接数据库初始化失败：{e}"))?;
    let has_id = conn.prepare("PRAGMA table_info(app_connection)").map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_,String>(1)).map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?.iter().any(|name| name == "id");
    if !has_id { conn.execute_batch("ALTER TABLE app_connection ADD COLUMN id INTEGER;").map_err(|e| e.to_string())?; }
    conn.execute_batch("UPDATE app_connection SET id=server_id WHERE id IS NULL;
        CREATE TRIGGER IF NOT EXISTS app_connection_sync_id AFTER INSERT ON app_connection BEGIN
        UPDATE app_connection SET id=NEW.server_id WHERE server_id=NEW.server_id; END;").map_err(|e| e.to_string())

}

fn db(app: &tauri::AppHandle) -> Result<MCPDatabase, String> {
    let db = MCPDatabase::new(app).map_err(|e| e.to_string())?;
    init(&db.conn)?;
    Ok(db)
}

#[derive(Serialize)]
pub struct CatalogResponse { pub catalog: Option<AppCatalog>, pub fetched_at: Option<String>, pub warning: Option<String>, pub skipped: Vec<String> }

#[tauri::command]
pub async fn fetch_app_catalog(app_handle: tauri::AppHandle) -> Result<CatalogResponse, String> {
    let result = async {
        let response = oauth::http()?.get(catalog::CATALOG_URL).send().await
            .map_err(|e| format!("获取应用目录失败：{}", e.without_url()))?;
        AppCatalog::parse_lenient(oauth::json(response, "应用目录").await?)
    }.await;
    let db = db(&app_handle)?;
    match result {
        Ok((catalog, skipped)) => {
            let fetched_at = chrono::Utc::now().to_rfc3339();
            db.conn.execute("INSERT INTO app_catalog_cache VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,fetched_at=excluded.fetched_at",
                params![serde_json::to_string(&catalog).map_err(|e| e.to_string())?, fetched_at]).map_err(|e| e.to_string())?;
            Ok(CatalogResponse { catalog: Some(catalog), fetched_at: Some(fetched_at), warning: None, skipped })
        }
        Err(error) => {
            let cached: Option<(String, String)> = db.conn.query_row("SELECT payload,fetched_at FROM app_catalog_cache WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(|e| e.to_string())?;
            // A broken cache must not hide why the fetch failed: report both.
            let cached = cached.map(|(raw, time)| serde_json::from_str::<AppCatalog>(&raw).map_err(|e| e.to_string())
                .and_then(|value| value.validate().map(|_| (value, time))));
            match cached {
                Some(Ok((value, time))) => Ok(CatalogResponse { catalog: Some(value), fetched_at: Some(time), warning: Some(error), skipped: vec![] }),
                Some(Err(cache)) => Ok(CatalogResponse { catalog: None, fetched_at: None, warning: Some(format!("{error}；本地目录缓存也无法使用：{cache}")), skipped: vec![] }),
                None => Ok(CatalogResponse { catalog: None, fetched_at: None, warning: Some(error), skipped: vec![] }),
            }
        }
    }
}

#[derive(Serialize)]
pub struct AppConnection {
    pub server_id: i64,
    pub app_id: String,
    pub definition: AppDefinition,
    pub label: String,
    pub config: HashMap<String, String>,
    pub status: String,
    pub error: Option<String>,
    pub credential_status: Option<String>,
    /// Computed per device, never synced: `None` when the connection holds no credential reference.
    pub credential_on_device: Option<bool>,
    /// Stable code for the credential sync state of this device (see sync::credential_sync_state).
    pub credential_sync: String,
}

#[tauri::command]
pub async fn list_app_connections(app_handle: tauri::AppHandle) -> Result<Vec<AppConnection>, String> {
    let vault_app = app_handle.clone();
    let ((credential_status, credential_sync), stored) = tokio::task::spawn_blocking(move || {
        Ok::<_, String>((crate::sync::credential_sync_state(&vault_app), crate::sync::vault::stored_references(&vault_app)?))
    }).await.map_err(|e| format!("读取凭据状态失败: {e}"))??;
    let db = db(&app_handle)?;
    let mut statement = db.conn.prepare("SELECT c.server_id,c.app_id,c.definition,c.label,c.config,c.status,c.error,c.verified_session,c.credential_ref FROM app_connection c JOIN mcp_server s ON s.id=c.server_id").map_err(|e| e.to_string())?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, Option<String>>(7)?, r.get::<_, Option<String>>(8)?))).map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (server_id, app_id, raw, label, config, mut status, mut error, session, reference) = row.map_err(|e| e.to_string())?;
        if session.as_deref() != Some(SESSION.as_str()) {
            if status == "available" { status = "unchecked".into(); }
            else if status == "connecting" { status = "auth_required".into(); error = Some("上次连接未完成，请重新连接".into()); }
        }
        result.push(AppConnection { server_id, app_id, definition: serde_json::from_str(&raw).map_err(|e| format!("连接 {server_id} 的应用描述损坏：{e}"))?, label,
            config: serde_json::from_str(&config).map_err(|e| format!("连接 {server_id} 的配置损坏：{e}"))?, status, error, credential_status: Some(credential_status.clone()),
            credential_on_device: reference.map(|r| stored.contains(&r)), credential_sync: credential_sync.to_string() });
    }
    Ok(result)
}

pub fn set_status(app: &tauri::AppHandle, id: i64, status: &str, error: Option<&str>) -> Result<(), String> {
    db(app)?.conn.execute("UPDATE app_connection SET status=?2,error=?3,verified_session=?4 WHERE server_id=?1", params![id, status, error, SESSION.as_str()]).map_err(|e| e.to_string())?;
    let _ = app.emit("mcp_state_changed", ());
    Ok(())
}

fn set_current_status(app: &tauri::AppHandle, id: i64, reference: &str, status: &str, error: Option<&str>) -> Result<(), String> {
    let changed = db(app)?.conn.execute("UPDATE app_connection SET status=?2,error=?3,verified_session=?4 WHERE server_id=?1 AND credential_ref=?5", params![id,status,error,SESSION.as_str(),reference]).map_err(|e| e.to_string())?;
    if changed > 0 { let _ = app.emit("mcp_state_changed", ()); }
    Ok(())
}

#[tauri::command]
pub async fn get_app_authorization_url(server_id: i64) -> Result<Option<String>, String> {
    Ok(LOGIN_JOBS.lock().await.get(&server_id).map(|job| job.url.clone()))
}

/// Credential-stage errors (vault read, token refresh) are produced by AIPP itself: `oauth::json` maps the
/// provider's `error` code to fixed text and never echoes `error_description`. Only these may be matched by
/// text; tool-stage errors carry remote content and are classified through the transport's 401 signal.
pub fn is_credential_auth_failure(error: &str) -> bool {
    // A credential missing on this device is deliberately absent: see credentials::MISSING_ON_DEVICE.
    ["invalid_grant", "invalid_client", "HTTP 401", "登录已过期"].iter().any(|pattern| error.contains(pattern))
}

pub fn record_credential_error(app: &tauri::AppHandle, server: &crate::db::mcp_db::MCPServer, error: &str) {
    if is_credential_auth_failure(error) { record_auth_failure(app, server, error); }
}

/// Disable a managed connection that the provider or server no longer accepts. `server` is the snapshot
/// the failing request used; a connection reconfigured meanwhile is left alone.
pub fn record_auth_failure(app: &tauri::AppHandle, server: &crate::db::mcp_db::MCPServer, error: &str) {
    let result = (|| {
        let db = db(app)?;
        let transaction = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let current = db.get_mcp_server(server.id).map_err(|e| e.to_string())?;
        if !current.is_enabled || current.headers != server.headers || current.url != server.url || !access::is_managed(&db.conn, server.id)? { return Ok::<(),String>(()); }
        db.toggle_mcp_server(server.id, false).map_err(|e| e.to_string())?;
        db.conn.execute("UPDATE app_connection SET status='auth_required',error=?2,verified_session=?3 WHERE server_id=?1", params![server.id,error,SESSION.as_str()]).map_err(|e| e.to_string())?;
        transaction.commit().map_err(|e| e.to_string())?;
        let _ = app.emit("mcp_state_changed", ());
        Ok(())
    })();
    if let Err(error) = result { tracing::warn!(server_id=server.id, error=%error, "Failed to persist app authentication failure"); }
}

/// Lower-cased names of headers that carry secrets for this connection, from its definition snapshot.
/// `None` when unknown, in which case callers must treat every credential header as secret.
pub fn secret_header_names(app: &tauri::AppHandle, server_id: i64) -> Option<std::collections::HashSet<String>> {
    let raw: String = db(app).ok()?.conn.query_row("SELECT definition FROM app_connection WHERE server_id=?1", [server_id], |r| r.get(0)).ok()?;
    serde_json::from_str::<AppDefinition>(&raw).ok().map(|definition| definition.secret_headers())
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub app_id: String,
    pub request_id: String,
    pub label: String,
    #[serde(default)]
    pub values: HashMap<String, String>,
    pub server_id: Option<i64>,
}

#[derive(Serialize)]
pub struct ConnectResult { pub server_id: i64, pub authorization_url: Option<String> }

fn definition(conn: &Connection, request: &ConnectRequest) -> Result<AppDefinition, String> {
    if let Some(id) = request.server_id {
        let raw: String = conn.query_row("SELECT definition FROM app_connection WHERE server_id=?1 AND app_id=?2", params![id, request.app_id], |r| r.get(0)).map_err(|_| "找不到对应的应用连接")?;
        return serde_json::from_str(&raw).map_err(|_| "已保存的应用描述损坏".into());
    }
    if request.app_id.starts_with("agent-plugin:") {
        let raw: String = conn.query_row("SELECT payload FROM agent_plugin_connection_definition WHERE app_id=?1", [&request.app_id], |r| r.get(0)).map_err(|e| format!("读取插件连接定义失败：{e}"))?;
        let definition: AppDefinition = serde_json::from_str(&raw).map_err(|e| format!("插件连接定义无效：{e}"))?;
        AppCatalog { schema_version: 1, version: "1".into(), apps: vec![definition.clone()] }.validate()?;
        return Ok(definition);
    }
    let raw: String = conn.query_row("SELECT payload FROM app_catalog_cache WHERE id=1", [], |r| r.get(0)).map_err(|_| "请先加载应用目录")?;
    let catalog: AppCatalog = serde_json::from_str(&raw).map_err(|_| "应用目录缓存无效")?;
    catalog.validate()?;
    catalog.apps.into_iter().find(|a| a.id == request.app_id).ok_or("应用不在已验证的目录中".into())
}

#[tauri::command]
pub async fn connect_catalog_app(app_handle: tauri::AppHandle, mut request: ConnectRequest) -> Result<ConnectResult, String> {
    let _change = CHANGE_LOCK.lock().await;
    uuid::Uuid::parse_str(&request.request_id).map_err(|_| "连接请求标识无效")?;
    if request.label.trim().is_empty() || request.label.chars().count() > 80 { return Err("请填写不超过 80 个字符的连接名称".into()); }
    let (app, existing) = {
        let db = db(&app_handle)?;
        let app = definition(&db.conn, &request)?;
        let existing: Option<(i64, String, String)> = db.conn.query_row("SELECT server_id,status,app_id FROM app_connection WHERE request_id=?1", [&request.request_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|e| e.to_string())?;
        (app, existing)
    };
    if let Some((id, status, app_id)) = existing {
        if app_id != request.app_id { return Err("重复请求的应用身份不匹配".into()); }
        if status == "connecting" {
            if let Some(job) = LOGIN_JOBS.lock().await.get(&id) { return Ok(ConnectResult { server_id: id, authorization_url: Some(job.url.clone()) }); }
        } else if ["available", "unchecked"].contains(&status.as_str()) { return Ok(ConnectResult { server_id: id, authorization_url: None }); }
        request.server_id = Some(id);
    }
    let created_new = request.server_id.is_none();
    let old_reference: Option<String> = if let Some(id) = request.server_id {
        db(&app_handle)?.conn.query_row("SELECT credential_ref FROM app_connection WHERE server_id=?1", [id], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten()
    } else { None };
    let platform = if cfg!(target_os="windows") { "windows" } else if cfg!(target_os="macos") { "macos" } else if cfg!(target_os="android") { "android" } else { "linux" };
    if !app.platforms.iter().any(|p| p == platform) { return Err("应用尚不支持当前平台".into()); }
    let (config, headers) = app.configuration(&request.values)?;
    let reference = uuid::Uuid::new_v4().to_string();
    let needs_credentials = app.connection.auth != "none" || !headers.is_empty();
    if needs_credentials {
        let key = reference.clone();
        let initial = credentials::Credential { server_id: 0, endpoint: app.connection.url.clone(), headers: headers.clone(), oauth: None };
        let vault_app = app_handle.clone();
        tokio::task::spawn_blocking(move || credentials::save(&vault_app, &key, &initial)).await.map_err(|_| "安全存储任务失败")??;
    }
    // Secrets have been safely written before altering a previously usable connection.
    let id_result = (|| {
        let db = db(&app_handle)?;
        let conn = &db.conn;
        let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let header_json = if needs_credentials { serde_json::json!({credentials::REFERENCE_HEADER: reference}).to_string() } else { "{}".into() };
        let name = format!("{} · {}", request.label.trim(), &request.request_id[..8]);
        let id = if let Some(id) = request.server_id {
            transaction.execute("UPDATE mcp_server SET headers=?2,is_enabled=0,name=?3 WHERE id=?1", params![id, header_json, name]).map_err(|e| e.to_string())?;
            id
        } else {
            transaction.execute("INSERT INTO mcp_server(name,description,transport_type,url,headers,timeout,is_long_running,is_enabled,is_builtin,is_deletable,proxy_enabled) VALUES(?1,?2,'http',?3,?4,30000,0,0,0,1,0)",
                params![name, app.description, app.connection.url, header_json]).map_err(|e| e.to_string())?;
            transaction.last_insert_rowid()
        };
        transaction.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,credential_ref,status,verified_session) VALUES(?1,?2,?3,?4,?5,?6,?7,'connecting',?8) ON CONFLICT(server_id) DO UPDATE SET request_id=excluded.request_id,definition=excluded.definition,label=excluded.label,config=excluded.config,credential_ref=excluded.credential_ref,status='connecting',error=NULL,verified_session=excluded.verified_session",
            params![id, request.request_id, app.id, serde_json::to_string(&app).map_err(|e| e.to_string())?, request.label.trim(), serde_json::to_string(&config).map_err(|e| e.to_string())?, needs_credentials.then_some(&reference), SESSION.as_str()]).map_err(|e| e.to_string())?;
        if let Some(old) = &old_reference {
            transaction.execute("INSERT OR IGNORE INTO app_credential_cleanup(reference,server_id) VALUES(?1,?2)", params![old,id]).map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok::<i64,String>(id)
    })();
    let id = match id_result {
        Ok(id) => id,
        Err(e) => { if needs_credentials { let key = reference.clone(); let vault_app = app_handle.clone(); let _ = tokio::task::spawn_blocking(move || credentials::clear(&vault_app, &key)).await; } return Err(e); }
    };
    if let Some(job) = LOGIN_JOBS.lock().await.remove(&id) { job.abort.abort(); }
    if let Some(old) = old_reference {
        let _credential_guard = credentials::CREDENTIAL_LOCK.lock().await;
        let key = old.clone();
        let vault_app = app_handle.clone();
        let cleared = tokio::task::spawn_blocking(move || credentials::clear(&vault_app, &key)).await.map_err(|_| "清理旧凭据任务失败".to_string()).and_then(|r| r);
        if let Err(e) = cleared { set_status(&app_handle, id, "auth_required", Some(&e))?; return Err(e); }
        db(&app_handle)?.conn.execute("DELETE FROM app_credential_cleanup WHERE reference=?1", [old]).map_err(|e| e.to_string())?;
    }
    // A brand-new connection that never became usable is removed instead of lingering as a stray
    // "需要重新登录" entry; retrying with the same request id then starts clean.
    let fail_new = {
        let (app, request_id, reference) = (app_handle.clone(), request.request_id.clone(), reference.clone());
        move |error: String| {
        let (app, request_id, reference) = (app.clone(), request_id.clone(), reference.clone());
        async move {
            if !created_new { return error; }
            let discarded = db(&app).and_then(|db| discard_created(&db.conn, id, &request_id));
            let key = reference.clone(); let vault_app = app.clone();
            let _ = tokio::task::spawn_blocking(move || credentials::clear(&vault_app, &key)).await;
            let _ = app.emit("mcp_state_changed", ());
            match discarded { Ok(()) => error, Err(cleanup) => format!("{error}（清理未完成的连接 {id} 失败：{cleanup}）") }
        }
    }};
    if app.connection.auth == "oauth" {
        let http = match credentials::oauth_http_for(&app_handle, id).await {
            Ok(http) => http,
            Err(e) => return Err(if created_new { fail_new(e).await } else { e }),
        };
        let pending = match oauth::begin_options(&http,&app.connection.url, app.connection.client_id.as_deref(), &app.connection.scopes, app.connection.oauth_resource.as_deref(),app.connection.oauth_client_auth.as_deref().unwrap_or("none"),request.values.get("oauth_client_secret").map(String::as_str),app.connection.oauth_redirect_uri.as_deref()).await {
            Ok(pending) => pending,
            Err(e) => {
                if created_new { return Err(fail_new(e).await); }
                set_status(&app_handle, id, "auth_required", Some(&e))?; return Err(format!("连接 {id}：{e}"));
            }
        };
        let url = pending.url.clone();
        let handle = app_handle.clone();
        let generation = reference.clone();
        let login_reference = reference.clone();
        let job = tokio::spawn(async move {
            let result = async {
                let mut credential = oauth::finish_with(&http, pending).await?;
                credential.server_id = id;
                credential.headers = headers;
                let _change = CHANGE_LOCK.lock().await;
                let current: Option<String> = db(&handle)?.conn.query_row("SELECT credential_ref FROM app_connection WHERE server_id=?1", [id], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten();
                if current.as_deref() != Some(reference.as_str()) { return Err("连接已取消或替换，丢弃迟到授权结果".into()); }
                let key = reference.clone();
                let vault_app = handle.clone();
                tokio::task::spawn_blocking(move || credentials::save(&vault_app, &key, &credential)).await.map_err(|_| "保存 OAuth 凭据任务失败")??;
                set_current_status(&handle, id, &reference, "unchecked", None)?;
                db(&handle)?.toggle_mcp_server(id, true).map_err(|e| e.to_string())?;
                Ok::<(),String>(())
            }.await;
            if let Err(e) = result { let _ = set_current_status(&handle, id, &generation, "auth_required", Some(&e)); }
            else { let _ = verify_app_connection(handle.clone(), id).await; }
            let mut jobs = LOGIN_JOBS.lock().await;
            if jobs.get(&id).is_some_and(|job| job.reference == generation) { jobs.remove(&id); }
        });
        LOGIN_JOBS.lock().await.insert(id, LoginJob { reference: login_reference, url: url.clone(), abort: job.abort_handle() });
        let _ = app_handle.emit("mcp_state_changed", ());
        Ok(ConnectResult { server_id: id, authorization_url: Some(url) })
    } else {
        if needs_credentials {
            let credential = credentials::Credential { server_id: id, endpoint: app.connection.url, headers, oauth: None };
            let vault_app = app_handle.clone();
            let saved = tokio::task::spawn_blocking(move || credentials::save(&vault_app, &reference, &credential)).await.map_err(|_| "保存连接凭据任务失败".to_string()).and_then(|r| r);
            if let Err(e) = saved {
                if created_new { return Err(fail_new(e).await); }
                set_status(&app_handle, id, "auth_required", Some(&e))?; return Err(e);
            }
        }
        set_status(&app_handle, id, "unchecked", None)?;
        db(&app_handle)?.toggle_mcp_server(id, true).map_err(|e| e.to_string())?;
        drop(_change);
        let _ = verify_app_connection(app_handle, id).await;
        Ok(ConnectResult { server_id: id, authorization_url: None })
    }
}

#[tauri::command]
pub async fn verify_app_connection(app_handle: tauri::AppHandle, server_id: i64) -> Result<(), String> {
    let server = db(&app_handle)?.get_mcp_server(server_id).map_err(|e| e.to_string())?;
    let status_of = |app: &tauri::AppHandle| -> Result<Option<String>, String> {
        db(app)?.conn.query_row("SELECT status FROM app_connection WHERE server_id=?1", [server_id], |r| r.get(0)).optional().map_err(|e| e.to_string())
    };
    if !server.is_enabled {
        return Err(match status_of(&app_handle)?.as_deref() {
            Some("auth_required") => "连接需要重新登录，请先重新授权".into(),
            Some("disconnected") => "连接已断开，请重新连接".into(),
            Some("connecting") => "连接尚未完成授权".into(),
            _ => "连接已停用，请先启用".into(),
        });
    }
    // Writing "error" here would sync to the device that does hold the credential and mislabel it.
    let reference: Option<String> = db(&app_handle)?.conn.query_row("SELECT credential_ref FROM app_connection WHERE server_id=?1", [server_id], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten();
    if let Some(reference) = reference {
        let vault_app = app_handle.clone();
        let stored = tokio::task::spawn_blocking(move || crate::sync::vault::stored_references(&vault_app)).await.map_err(|e| format!("读取凭据状态失败: {e}"))??;
        if !stored.contains(&reference) { return Err(format!("连接 {server_id}：{}", credentials::MISSING_ON_DEVICE)); }
    }
    let result = super::registry_api::refresh_mcp_server_capabilities(app_handle.clone(), server_id).await;
    let _change = CHANGE_LOCK.lock().await;
    // Credential/401 failures were already recorded as auth_required with their cause; keep that state.
    if let Err(e) = &result {
        if status_of(&app_handle)?.as_deref() == Some("auth_required") { return Err(format!("连接 {server_id}：{e}")); }
    }
    let current = db(&app_handle)?.get_mcp_server(server_id).map_err(|e| e.to_string())?;
    if !current.is_enabled || current.headers != server.headers || current.url != server.url || current.command != server.command {
        return Err("连接在验证期间被停用、断开或修改，已丢弃过期验证结果".into());
    }
    match result {
        Ok(_) => { set_status(&app_handle, server_id, "available", None)?; Ok(()) }
        Err(e) => { set_status(&app_handle, server_id, "error", Some(&e))?; Err(format!("连接 {server_id}：{e}")) }
    }
}

#[tauri::command]
pub async fn disconnect_app_connection(app_handle: tauri::AppHandle, server_id: i64, revoke_remote: Option<bool>) -> Result<String, String> {
    let _change = CHANGE_LOCK.lock().await;
    if let Some(job) = LOGIN_JOBS.lock().await.remove(&server_id) { job.abort.abort(); }
    let reference: Option<String> = {
        let db = db(&app_handle)?;
        let reference: Option<Option<String>> = db.conn.query_row("SELECT credential_ref FROM app_connection WHERE server_id=?1", [server_id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        if reference.is_none() { return Ok("此服务器不是目录应用连接".into()); }
        db.toggle_mcp_server(server_id, false).map_err(|e| e.to_string())?;
        reference.flatten()
    };
    set_status(&app_handle, server_id, "disconnected", None)?;
    let _guard = credentials::CREDENTIAL_LOCK.lock().await;
    let mut references = {
        let db = db(&app_handle)?;
        let mut stmt = db.conn.prepare("SELECT reference FROM app_credential_cleanup WHERE server_id=?1").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([server_id], |r| r.get::<_,String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?
    };
    if let Some(key) = reference { references.push(key); }
    let mut revoked = false;
    let http = if revoke_remote.unwrap_or(true) { Some(credentials::oauth_http_for(&app_handle, server_id).await?) } else { None };
    for reference in references {
        if revoke_remote.unwrap_or(true) {
            let key = reference.clone();
            let vault_app = app_handle.clone();
            let credential = tokio::task::spawn_blocking(move || credentials::load_optional(&vault_app, &key)).await.map_err(|_| "读取待撤销凭据任务失败")??;
            if let Some(credential) = credential {
                match oauth::revoke(http.as_ref().expect("built when revoking"), &credential).await {
                    Ok(value) => revoked |= value,
                    Err(error) => {
                        let error = format!("连接已停用，但服务商撤销未完成：{error}。可重试断开，或选择删除凭据但不撤销服务商授权；启用同步时删除会传播。");
                        set_status(&app_handle, server_id, "disconnected", Some(&error))?;
                        return Err(error);
                    }
                }
            }
        }
        let key = reference.clone();
        let vault_app = app_handle.clone();
        let cleared = tokio::task::spawn_blocking(move || credentials::clear(&vault_app, &key)).await.map_err(|_| "删除凭据任务失败".to_string()).and_then(|r| r);
        if let Err(e) = cleared { set_status(&app_handle, server_id, "disconnected", Some(&e))?; return Err(e); }
        db(&app_handle)?.conn.execute("DELETE FROM app_credential_cleanup WHERE reference=?1", [reference]).map_err(|e| e.to_string())?;
    }
    db(&app_handle)?.conn.execute("UPDATE app_connection SET credential_ref=NULL WHERE server_id=?1", [server_id]).map_err(|e| e.to_string())?;
    Ok(if revoked { "已断开连接、撤销服务商令牌并删除凭据；启用同步时会同步删除" } else { "已断开并删除凭据；启用同步时会同步删除，服务商侧授权尚未撤销" }.into())
}

/// Delete the rows a failed create inserted, unless another request already took over the instance.
pub(crate) fn discard_created(conn: &Connection, id: i64, request_id: &str) -> Result<(), String> {
    let transaction = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let owned: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM app_connection WHERE server_id=?1 AND request_id=?2)", params![id, request_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !owned { return Ok(()); }
    transaction.execute("DELETE FROM app_connection WHERE server_id=?1", [id]).map_err(|e| e.to_string())?;
    transaction.execute("DELETE FROM mcp_server WHERE id=?1", [id]).map_err(|e| e.to_string())?;
    transaction.commit().map_err(|e| e.to_string())
}

pub fn guard_update(app: &tauri::AppHandle, id: i64, request: &super::registry_api::MCPServerRequest) -> Result<(), String> {
    let db = db(app)?;
    let managed: bool = db.conn.query_row("SELECT EXISTS(SELECT 1 FROM app_connection WHERE server_id=?1)", [id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if managed {
        let old = db.get_mcp_server(id).map_err(|e| e.to_string())?;
        if old.url != request.url || old.headers != request.headers || request.transport_type != old.transport_type || request.command != old.command || request.environment_variables != old.environment_variables || request.is_builtin == Some(true) {
            return Err("目录应用的认证和服务地址由连接管理维护，请从应用详情重新配置".into());
        }
        if request.is_enabled { guard_enable(app, id)?; }
    }
    Ok(())
}

pub fn guard_enable(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    let status: Option<String> = db(app)?.conn.query_row("SELECT status FROM app_connection WHERE server_id=?1", [id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    if status.as_deref().is_some_and(|s| ["disconnected", "auth_required", "connecting"].contains(&s)) {
        return Err("连接尚未完成授权或已断开，请从应用详情重新连接".into());
    }
    Ok(())
}

#[derive(Serialize)]
pub struct AssistantAccess { pub id: i64, pub name: String, pub enabled: bool }

#[tauri::command]
pub fn get_app_assistant_access(app_handle: tauri::AppHandle, server_id: i64) -> Result<Vec<AssistantAccess>, String> {
    db(&app_handle)?.get_mcp_server(server_id).map_err(|e| e.to_string())?;
    let assistants = crate::db::assistant_db::AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let mut stmt = assistants.conn.prepare("SELECT a.id,a.name,COALESCE(c.is_enabled,0) FROM assistant a LEFT JOIN assistant_mcp_config c ON c.assistant_id=a.id AND c.mcp_server_id=?1 ORDER BY a.name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([server_id], |r| Ok(AssistantAccess { id: r.get(0)?, name: r.get(1)?, enabled: r.get(2)? })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[cfg(test)]
mod apps_tests;

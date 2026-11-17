use super::*;
use crate::mcp::apps;
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

/// Only URLs already published in the helper catalog are safe to copy verbatim.
#[tauri::command]
pub fn public_connection_endpoint(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<Option<String>, String> {
    let db = open_db(&app_handle)?;
    apps::init(&db.conn)?;
    let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
    if server.transport_type != "http" {
        return Ok(None);
    }
    let Some(url) = server.url else {
        return Ok(None);
    };
    let Ok(parsed) = apps::catalog::public_https(&url) else { return Ok(None) };
    let public:bool = db.conn.query_row("SELECT EXISTS(SELECT 1 FROM app_directory_detail
        WHERE json_extract(payload,'$.connection.url')=?1 AND json_extract(payload,'$.connection.transport')='http')",
        [&url],|r|r.get(0)).map_err(|e|e.to_string())?;
    Ok(public.then(|| parsed.to_string()))
}

#[derive(Serialize)]
pub struct ConnectionCheck {
    server_id: i64,
    current: bool,
    error: Option<String>,
}

#[tauri::command]
pub fn list_connection_checks(
    app_handle: tauri::AppHandle,
) -> Result<Vec<ConnectionCheck>, String> {
    let db = open_db(&app_handle)?;
    let mut stmt = db.conn.prepare("SELECT c.server_id,c.session=?1,c.error FROM connection_check c JOIN mcp_server s ON c.server_id=s.id").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([apps::SESSION.as_str()], |r| {
            Ok(ConnectionCheck { server_id: r.get(0)?, current: r.get(1)?, error: r.get(2)? })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())
}

fn import_configs(db: &MCPDatabase, requests: &[MCPServerRequest]) -> Result<(), String> {
    if requests.is_empty() {
        return Err("配置为空".into());
    }
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    for r in requests {
        if r.name.trim().is_empty()
            || !matches!(r.transport_type.as_str(), "http" | "stdio")
            || r.is_builtin == Some(true)
            || r.command.as_deref().unwrap_or("").trim_start().starts_with("aipp:")
        {
            return Err("只允许导入具名的自定义 HTTP 或 stdio 连接".into());
        }
        if r.command
            .as_deref()
            .is_some_and(|cmd| cmd.trim_start().starts_with(apps::bundles::runtime::PREFIX))
        {
            return Err("本地插件连接请从应用目录安装".into());
        }
        if let Some(raw) = r.headers.as_deref().filter(|s| !s.trim().is_empty()) {
            let headers: HashMap<String, String> =
                serde_json::from_str(raw).map_err(|_| "请求头必须是键值均为文本的 JSON 对象")?;
            if headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case(apps::credentials::REFERENCE_HEADER))
            {
                return Err("不能导入其他连接的凭据引用".into());
            }
        }
        if cfg!(mobile) && r.transport_type == "stdio" {
            return Err(crate::api::system_api::mobile_unsupported_error("外部 stdio MCP"));
        }
        if r.transport_type == "stdio" && r.command.as_deref().unwrap_or("").trim().is_empty() {
            return Err("请填写本地命令".into());
        }
        if r.transport_type == "http"
            && !r.url.as_deref().and_then(|s| reqwest::Url::parse(s).ok()).is_some_and(|u| {
                matches!(u.scheme(), "http" | "https")
                    && u.host().is_some()
                    && u.username().is_empty()
                    && u.password().is_none()
            })
        {
            return Err("请填写 HTTP/HTTPS 地址，认证请使用请求头".into());
        }
        if [r.headers.as_deref(), r.environment_variables.as_deref()]
            .into_iter()
            .flatten()
            .any(crate::vault::config_ref::contains)
        {
            return Err("请在导入后通过设置选择本机保险箱条目".into());
        }
        tx.execute("INSERT INTO mcp_server(name,description,transport_type,command,environment_variables,headers,url,timeout,is_long_running,is_enabled,is_builtin,is_deletable,proxy_enabled)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0,1,0)", params![r.name.trim(),r.description,r.transport_type,r.command,r.environment_variables,r.headers,r.url,r.timeout,r.is_long_running])
            .map_err(|e|format!("导入「{}」失败：{e}；同名连接请先改名",r.name))?;
    }
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_disabled_connections(
    app_handle: tauri::AppHandle,
    requests: Vec<MCPServerRequest>,
) -> Result<(), String> {
    import_configs(&open_db(&app_handle)?, &requests)?;
    crate::sync::schedule_sync_after_local_change(&app_handle);
    let _ = app_handle.emit("mcp_state_changed", ());
    Ok(())
}

#[derive(Serialize, Default)]
pub struct RefreshPreview {
    token: String,
    added: Vec<String>,
    changed: Vec<String>,
    removed: Vec<String>,
    permissions: Vec<String>,
}

fn operation_scope(signature: Option<&String>) -> String {
    let value = signature
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or_default();
    if let Some(method) = value.get(0).and_then(|v| v.as_str()) {
        return format!("{} {}", method, value.get(1).and_then(|v| v.as_str()).unwrap_or(""));
    }
    if value["destructiveHint"].as_bool() == Some(true) {
        return "可能删除或覆盖数据".into();
    }
    if value["readOnlyHint"].as_bool() == Some(true) {
        return "服务声明只读".into();
    }
    "可能写入数据".into()
}

struct Pending {
    server: MCPServer,
    generation: Option<String>,
    previous: Vec<MCPServerTool>,
    signatures: HashMap<i64, String>,
    discovery: Discovery,
}
struct CheckJob {
    token: String,
    started: Instant,
    preview: Option<Pending>,
}
static PENDING: LazyLock<Mutex<HashMap<i64, CheckJob>>> = LazyLock::new(Default::default);

fn begin_check(id: i64) -> Result<String, String> {
    let mut jobs = PENDING.lock().map_err(|e| e.to_string())?;
    jobs.retain(|_, job| job.started.elapsed().as_secs() < 300);
    if jobs.len() >= 64 && !jobs.contains_key(&id) {
        return Err("待确认的检测过多，请稍后重试".into());
    }
    let token = uuid::Uuid::new_v4().to_string();
    jobs.insert(id, CheckJob { token: token.clone(), started: Instant::now(), preview: None });
    Ok(token)
}

fn generation(db: &MCPDatabase, server: &MCPServer) -> Result<Option<String>, String> {
    if !server.is_enabled {
        return Err("请先启用连接，再测试或刷新工具".into());
    }
    apps::init(&db.conn)?;
    let row = db
        .conn
        .query_row(
            "SELECT request_id,status FROM app_connection WHERE server_id=?1",
            [server.id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match row {
        Some((_, status))
            if matches!(status.as_str(), "disconnected" | "auth_required" | "connecting") =>
        {
            Err("请先完成连接授权".into())
        }
        other => Ok(other.map(|(id, _)| id)),
    }
}

pub(super) fn tool_signatures(db: &MCPDatabase, id: i64) -> Result<HashMap<i64, String>, String> {
    let mut stmt=db.conn.prepare("SELECT t.id,s.signature FROM connection_tool_signature s JOIN mcp_server_tool t ON t.id=s.tool_id WHERE t.server_id=?1").map_err(|e|e.to_string())?;
    let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())
}

fn changes(
    signatures: &HashMap<i64, String>,
    old: &[MCPServerTool],
    tools: Option<&[SimpleTool]>,
) -> RefreshPreview {
    let mut diff = RefreshPreview::default();
    if let Some(tools) = tools {
        for tool in tools {
            match old.iter().find(|t| t.tool_name == tool.name) {
                None => {
                    diff.added.push(tool.name.clone());
                    diff.permissions.push(format!(
                        "{}：{}",
                        tool.name,
                        operation_scope(Some(&tool.signature))
                    ));
                }
                Some(t)
                    if !same_tool(t, tool)
                        || signatures.get(&t.id).is_some_and(|saved| saved != &tool.signature) =>
                {
                    diff.changed.push(tool.name.clone());
                    diff.permissions.push(format!(
                        "{}：{} → {}",
                        tool.name,
                        operation_scope(signatures.get(&t.id)),
                        operation_scope(Some(&tool.signature))
                    ));
                }
                _ => {}
            }
        }
        diff.removed = old
            .iter()
            .filter(|t| !tools.iter().any(|n| n.name == t.tool_name))
            .map(|t| t.tool_name.clone())
            .collect();
    }
    diff
}

fn record(
    app: &tauri::AppHandle,
    db: &MCPDatabase,
    server: &MCPServer,
    managed: bool,
    error: Option<&str>,
) -> Result<(), String> {
    if managed {
        return apps::set_status(
            app,
            server.id,
            if error.is_some() { "error" } else { "available" },
            error,
        );
    }
    db.conn
        .execute(
            "INSERT OR REPLACE INTO connection_check(server_id,session,error) VALUES(?1,?2,?3)",
            params![server.id, apps::SESSION.as_str(), error],
        )
        .map_err(|e| e.to_string())?;
    let _ = app.emit("mcp_state_changed", ());
    Ok(())
}

/// Startup may restrict stale permissions, but never silently installs a new tool set.
pub(crate) async fn check_after_restart(
    app: &tauri::AppHandle,
    server: &MCPServer,
) -> Result<(), String> {
    let db = open_db(app)?;
    let identity = generation(&db, server)?;
    let previous = db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())?;
    let signatures = tool_signatures(&db, server.id)?;
    let (remote, _, _) = discover_capabilities(app, server).await?;
    let _change = apps::CHANGE_LOCK.lock().await;
    if let Some(reason) =
        restrict_changed_tools(&db, server, &identity, &previous, &signatures, remote.as_deref())?
    {
        db.rebuild_dynamic_mcp_catalog_force().map_err(|e| e.to_string())?;
        crate::sync::schedule_sync_after_local_change(app);
        return Err(reason);
    }
    Ok(())
}

fn restrict_changed_tools(
    db: &MCPDatabase,
    server: &MCPServer,
    identity: &Option<String>,
    previous: &[MCPServerTool],
    signatures: &HashMap<i64, String>,
    remote: Option<&[SimpleTool]>,
) -> Result<Option<String>, String> {
    let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    if db.get_mcp_server(server.id).map_err(|e| e.to_string())? != *server
        || &generation(db, server)? != identity
    {
        return Err("检测期间连接已变化".into());
    }
    let superseded: bool = tx.query_row("SELECT verified_session IS ?2 OR status!='available' FROM app_connection WHERE server_id=?1",
        params![server.id, apps::SESSION.as_str()],|r|r.get(0)).map_err(|e|e.to_string())?;
    if superseded
        || db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())? != previous
        || signatures != &tool_signatures(db, server.id)?
    {
        return Ok(None);
    }
    let diff = changes(signatures, previous, remote);
    for name in &diff.removed {
        tx.execute("UPDATE mcp_server_tool SET is_enabled=0,is_auto_run=0 WHERE server_id=?1 AND tool_name=?2",
            params![server.id,name]).map_err(|e|e.to_string())?;
    }
    if diff.added.is_empty() && diff.changed.is_empty() && diff.removed.is_empty() {
        // Older installations and synced tools may have no local comparison baseline.
        for tool in remote.unwrap_or_default() {
            if let Some(old) = previous.iter().find(|old| old.tool_name == tool.name && !signatures.contains_key(&old.id)) {
                tx.execute("INSERT INTO connection_tool_signature(tool_id,signature) VALUES(?1,?2)",
                    params![old.id,tool.signature]).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(None);
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Some(format!(
        "服务工具已变化（新增 {}、修改 {}、移除 {}），请刷新并查看变化。",
        diff.added.len(),
        diff.changed.len(),
        diff.removed.len()
    )))
}

#[tauri::command]
pub async fn preview_connection_refresh(
    app_handle: tauri::AppHandle,
    server_id: i64,
) -> Result<RefreshPreview, String> {
    let db = open_db(&app_handle)?;
    let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
    let identity = generation(&db, &server)?;
    let previous = db.get_mcp_server_tools(server_id).map_err(|e| e.to_string())?;
    let signatures = tool_signatures(&db, server_id)?;
    let token = begin_check(server_id)?;
    let result = discover_capabilities(&app_handle, &server).await;
    let _lock = apps::CHANGE_LOCK.lock().await;
    let mut jobs = PENDING.lock().map_err(|e| e.to_string())?;
    let job = jobs
        .get_mut(&server_id)
        .filter(|job| job.token == token)
        .ok_or("已有更新的检测，已忽略旧结果")?;
    let current = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
    let discovery = match result {
        Err(e) => {
            if current == server
                && generation(&db, &server).ok().as_ref() == Some(&identity)
                && !e.contains(apps::credentials::MISSING_ON_DEVICE)
            {
                record(&app_handle, &db, &server, identity.is_some(), Some(&e))?;
            }
            return Err(e);
        }
        Ok(value) => value,
    };
    if current != server || generation(&db, &server)? != identity {
        return Err("连接已重新授权，请重新检测".into());
    }
    let mut diff = changes(&signatures, &previous, discovery.0.as_deref());
    diff.token = token;
    job.preview = Some(Pending { server, generation: identity, previous, signatures, discovery });
    Ok(diff)
}

#[tauri::command]
pub async fn apply_connection_refresh(
    app_handle: tauri::AppHandle,
    server_id: i64,
    token: String,
) -> Result<(), String> {
    let _lock = apps::CHANGE_LOCK.lock().await;
    let p = {
        let mut pending = PENDING.lock().map_err(|e| e.to_string())?;
        if !pending
            .get(&server_id)
            .is_some_and(|p| p.token == token && p.started.elapsed().as_secs() < 300)
        {
            return Err("预览已失效，请重新检测".into());
        }
        pending.remove(&server_id).and_then(|job| job.preview).ok_or("检测尚未完成，请重新检测")?
    };
    let db = open_db(&app_handle)?;
    if generation(&db, &p.server)? != p.generation {
        return Err("连接已变更，请重新检测".into());
    }
    let (tools, resources, prompts) = p.discovery;
    persist_reviewed(
        &db,
        &p.server,
        &p.server.transport_type,
        tools,
        resources,
        prompts,
        Some((&p.previous, &p.signatures)),
    )?;
    db.rebuild_dynamic_mcp_catalog_force().map_err(|e| e.to_string())?;
    crate::mcp::summarizer::trigger_mcp_catalog_summary_generation(app_handle.clone(), server_id);
    crate::sync::schedule_sync_after_local_change(&app_handle);
    record(&app_handle, &db, &p.server, p.generation.is_some(), None)
}

#[cfg(test)]
#[path = "connection_management_tests.rs"]
mod tests;

use super::{format::{self, Preview, Source}, store::{self, Receipt}};
use super::super::catalog::{AppDefinition, ConnectionRecipe};
use serde::Serialize;
use serde_json::Value;
use std::{path::PathBuf, sync::LazyLock, time::Duration};
use tauri::Manager;
use tokio::sync::Mutex;

pub(super) static INSTALL_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
fn data(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    #[cfg(mobile)]
    return Err("MOBILE_UNSUPPORTED: Agent 插件包暂仅支持桌面端".into());
    #[cfg(desktop)]
    app.path().app_data_dir().map_err(|e| e.to_string())
}
async fn client(app: &tauri::AppHandle) -> Result<reqwest::Client, String> {
    let config = app.state::<crate::FeatureConfigState>();
    let map = config.config_feature_map.lock().await;
    let mut builder = reqwest::Client::builder().user_agent("AIPP-Agent-Plugins").timeout(Duration::from_secs(120));
    if let Some(proxy) = crate::api::ai::config::get_network_proxy_from_config(&map) { builder = builder.proxy(reqwest::Proxy::all(&proxy).map_err(|e| format!("插件网络代理无效：{e}"))?); }
    builder.build().map_err(|e| e.to_string())
}
async fn installed(app: &tauri::AppHandle, key: &str) -> Result<Receipt, String> {
    let directory = data(app)?; let key = key.to_string();
    tokio::task::spawn_blocking(move || store::receipt(&directory, &key)?.ok_or("插件未安装".to_string()))
        .await.map_err(|e| format!("读取插件安装记录：{e}"))?
}
async fn bytes(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = client.get(url).send().await.map_err(|e| format!("获取插件来源 {url}：{e}"))?;
    if !response.status().is_success() { return Err(format!("获取插件来源 {url}：HTTP {}", response.status())); }
    let mut result = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("下载插件：{e}"))? {
        if result.len() + chunk.len() > limit { return Err(format!("插件来源响应超过 {} MiB", limit / 1024 / 1024)); }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}
async fn revision(client: &reqwest::Client, source: &Source) -> Result<String, String> {
    source.validate()?;
    let mut url = reqwest::Url::parse(&format!("https://api.github.com/repos/{}/commits/", source.repo)).map_err(|e| e.to_string())?;
    url.path_segments_mut().map_err(|_| "GitHub 地址无效")?.pop_if_empty().push(&source.git_ref);
    let response: Value = serde_json::from_slice(&bytes(client, url.as_str(), 4 * 1024 * 1024).await?).map_err(|e| e.to_string())?;
    let sha = response["sha"].as_str().ok_or("GitHub 没有返回提交标识")?;
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("GitHub 提交标识无效".into()); }
    Ok(sha.into())
}
async fn package(app: &tauri::AppHandle, source: &Source, sha: &str) -> Result<Vec<u8>, String> {
    source.validate()?;
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("插件提交标识无效".into()); }
    let root = data(app)?.join("agent-plugin-cache");
    tokio::fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
    let path = root.join(format!("{}-{sha}.zip", source.repo.replace('/', "--")));
    if path.exists() { return tokio::fs::read(path).await.map_err(|e| e.to_string()); }
    let archive = bytes(&client(app).await?, &format!("https://codeload.github.com/{}/zip/{sha}", source.repo), 128 * 1024 * 1024).await?;
    let staging = root.join(format!("{}.download", uuid::Uuid::new_v4()));
    tokio::fs::write(&staging, &archive).await.map_err(|e| e.to_string())?;
    if let Err(e) = tokio::fs::rename(&staging, &path).await {
        let _ = tokio::fs::remove_file(&staging).await;
        if !path.exists() { return Err(format!("保存插件下载缓存：{e}")); }
    }
    Ok(archive)
}
#[derive(Serialize)]
pub struct CatalogEntry { pub name: String, pub description: String, pub category: String, pub source: Source }
#[derive(Serialize)]
pub struct PluginCatalog { pub revision: String, pub entries: Vec<CatalogEntry> }

#[tauri::command]
pub async fn fetch_agent_plugin_catalog(app_handle: tauri::AppHandle, repo: String, git_ref: String) -> Result<PluginCatalog, String> {
    let _ = data(&app_handle)?;
    let source = Source { repo, git_ref, path: "plugins".into() };
    let client = client(&app_handle).await?; let sha = revision(&client, &source).await?;
    let url = format!("https://raw.githubusercontent.com/{}/{sha}/.agents/plugins/marketplace.json", source.repo);
    let marketplace: Value = serde_json::from_slice(&bytes(&client, &url, 4 * 1024 * 1024).await?).map_err(|e| format!("Marketplace JSON 无效：{e}"))?;
    let mut entries = Vec::new();
    for plugin in marketplace["plugins"].as_array().ok_or("Marketplace 缺少 plugins 列表")? {
        let path = plugin["source"]["path"].as_str().or(plugin["source"].as_str()).ok_or("Marketplace 条目缺少 source.path")?;
        let path = format::safe_path(path)?;
        entries.push(CatalogEntry { name: plugin["interface"]["displayName"].as_str().or(plugin["name"].as_str()).unwrap_or(&path).into(),
            description: plugin["description"].as_str().unwrap_or("").into(), category: plugin["category"].as_str().unwrap_or("其他").into(),
            source: Source { repo: source.repo.clone(), git_ref: source.git_ref.clone(), path } });
    }
    Ok(PluginCatalog { revision: sha, entries })
}

#[tauri::command]
pub async fn inspect_agent_plugin(app_handle: tauri::AppHandle, source: Source) -> Result<Preview, String> {
    let sha = revision(&client(&app_handle).await?, &source).await?;
    let archive = package(&app_handle, &source, &sha).await?;
    let data = data(&app_handle)?;
    tokio::task::spawn_blocking(move || {
        let files = format::unpack(&archive, &source)?;
        let permissions = format::permissions(&archive, &source)?;
        let mut preview = format::inspect(source, sha, &files)?;
        preview.set_permissions(permissions);
        super::dependencies::resolve(&mut preview,&archive)?;
        if let Some(r) = store::receipt(&data, &preview.key)? { preview.installed_revision = Some(r.preview.revision.clone()); preview.locally_modified = store::modified(&data, &r)?; }
        Ok(preview)
    }).await.map_err(|e| format!("解析插件工作进程：{e}"))?
}

#[tauri::command]
pub async fn list_agent_plugins(app_handle: tauri::AppHandle) -> Result<Vec<Receipt>, String> {
    let data = data(&app_handle)?;
    tokio::task::spawn_blocking(move || store::list(&data)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_agent_plugin(app_handle: tauri::AppHandle, source: Source, revision: String, digest: String) -> Result<Receipt, String> {
    let _guard = INSTALL_LOCK.lock().await;
    super::runtime::ensure_idle(&source.key())?;
    let archive = package(&app_handle, &source, &revision).await?;
    let data = data(&app_handle)?;
    let installed = tokio::task::spawn_blocking(move || {
        let files = format::unpack(&archive, &source)?;
        let permissions = format::permissions(&archive, &source)?;
        let mut preview = format::inspect(source, revision, &files)?;
        preview.set_permissions(permissions);
        super::dependencies::resolve(&mut preview,&archive)?;
        if preview.digest != digest { return Err("插件内容已改变，请重新预览后安装".into()); }
        store::install(&data, &preview, &files)
    }).await.map_err(|e| format!("安装插件工作进程：{e}"))??;
    crate::api::skill_api::scan_skills(app_handle).await.map_err(|e| format!("插件已保存，但刷新技能索引失败：{e}"))?;
    Ok(installed)
}

#[tauri::command]
pub async fn rollback_agent_plugin(app_handle: tauri::AppHandle, key: String) -> Result<(), String> {
    let _guard = INSTALL_LOCK.lock().await; let data = data(&app_handle)?;
    super::runtime::ensure_idle(&key)?;
    tokio::task::spawn_blocking(move || store::rollback(&data, &key)).await.map_err(|e| e.to_string())??;
    crate::api::skill_api::scan_skills(app_handle).await?; Ok(())
}

#[tauri::command]
pub async fn uninstall_agent_plugin(app_handle: tauri::AppHandle, key: String, revoke_remote: Option<bool>) -> Result<(), String> {
    let _guard = INSTALL_LOCK.lock().await;
    super::runtime::ensure_idle(&key)?;
    let directory = data(&app_handle)?;
    let check_data = directory.clone(); let check_key = key.clone();
    tokio::task::spawn_blocking(move || {
        let r = store::receipt(&check_data, &check_key)?.ok_or("插件未安装")?;
        if store::modified(&check_data, &r)? { return Err("插件有本地修改，请先备份并恢复原文件".to_string()); }
        Ok(())
    }).await.map_err(|e| e.to_string())??;
    let connections = super::super::list_app_connections(app_handle.clone()).await?;
    for connection in connections.into_iter().filter(|c| c.app_id.starts_with(&format!("agent-plugin:{key}:"))) {
        super::super::disconnect_app_connection(app_handle.clone(), connection.server_id, revoke_remote).await
            .map_err(|e| format!("卸载停止，插件仍保留；断开连接 {} 失败：{e}", connection.label))?;
        crate::mcp::registry_api::delete_mcp_server(app_handle.clone(), connection.server_id).await
            .map_err(|e| format!("卸载停止，插件仍保留；删除连接 {} 失败：{e}", connection.label))?;
    }
    tokio::task::spawn_blocking(move || store::uninstall(&directory, &key)).await.map_err(|e| e.to_string())??;
    crate::api::skill_api::scan_skills(app_handle).await?; Ok(())
}

#[tauri::command]
pub async fn get_agent_plugin_connection(app_handle: tauri::AppHandle, key: String, server_name: String, auth: String, client_id: Option<String>, oauth_client_auth: Option<String>, oauth_redirect_uri: Option<String>) -> Result<AppDefinition, String> {
    if !["none", "oauth", "bearer", "headers"].contains(&auth.as_str()) { return Err("不支持的认证方式".into()); }
    let installed = installed(&app_handle, &key).await?;
    let server = installed.preview.servers.iter().find(|s| s.name == server_name).ok_or("插件没有此连接")?;
    if let Some(reason) = &server.reason {
        if !(reason.contains("OAuth") && (auth == "bearer" || auth == "headers" || client_id.as_ref().is_some_and(|s| !s.trim().is_empty()))) { return Err(reason.clone()); }
    }
    let mut fields = Vec::new();
    let mut push_header = |header: &str, prefix: &str| {
        fields.push(super::super::catalog::ConfigField { key:format!("header_{}",fields.len()),label:header.into(),kind:"password".into(),required:true,secret:true,help:"仅保存在加密凭据库，不写入插件文件".into(),default:String::new(),options:vec![],visible_when:None,header:Some(header.into()),prefix:prefix.into() });
    };
    if auth == "bearer" { push_header("Authorization","Bearer "); }
    if auth == "headers" {
        for header in server.config["headers"].as_object().ok_or("此插件没有声明请求头")?.keys() { push_header(header,""); }
    }
    if auth == "oauth" && oauth_client_auth.as_deref().is_some_and(|m|m!="none") {
        fields.push(super::super::catalog::ConfigField {key:"oauth_client_secret".into(),label:"自己的 OAuth Client Secret".into(),kind:"password".into(),required:true,secret:true,help:"仅用于授权服务器的令牌交换与刷新，保存在加密 Vault".into(),default:String::new(),options:vec![],visible_when:None,header:None,prefix:String::new()});
    }
    let auth = if auth == "bearer" || auth == "headers" { "fields".to_string() } else { auth };
    let definition = AppDefinition { id: format!("agent-plugin:{key}:{server_name}"), version: installed.preview.version,
        name: format!("{} · {}", installed.preview.name, server_name), description: installed.preview.description,
        publisher: installed.preview.publisher, category: "Agent 插件".into(), icon: None, homepage: None,
        notice: "配套技能已安装；连接完成后选择可使用此插件的助手。".into(), platforms: vec!["windows".into(), "macos".into(), "linux".into()],
        connection: ConnectionRecipe { transport: "http".into(), oauth_client_auth, oauth_redirect_uri, url: server.config["url"].as_str().ok_or("MCP 地址缺失")?.into(), auth,
            client_id: client_id.filter(|s| !s.trim().is_empty()).or_else(|| server.config["oauth"]["client_id"].as_str().filter(|s| !s.starts_with('<') && server.config["oauth"].get("client_secret").is_none()).map(str::to_string)),
            oauth_resource: server.config["oauth_resource"].as_str().map(str::to_string),
            scopes: server.config["scopes"].as_array().or_else(|| server.config["oauth"]["scopes"].as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default() }, fields, skill_ids: vec![] };
    let db = super::super::db(&app_handle)?;
    db.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_plugin_connection_definition (app_id TEXT PRIMARY KEY, payload TEXT NOT NULL)").map_err(|e| e.to_string())?;
    db.conn.execute("INSERT INTO agent_plugin_connection_definition VALUES(?1,?2) ON CONFLICT(app_id) DO UPDATE SET payload=excluded.payload", rusqlite::params![definition.id, serde_json::to_string(&definition).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    Ok(definition)
}

#[tauri::command]
pub async fn set_agent_plugin_assistant(app_handle: tauri::AppHandle, key: String, server_id: Option<i64>, assistant_id: i64, enabled: bool) -> Result<(), String> {
    let r = installed(&app_handle, &key).await?;
    crate::db::assistant_db::AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?
        .get_assistant(assistant_id).map_err(|e| format!("助手不存在：{e}"))?;
    let db = super::super::db(&app_handle)?;
    if let Some(server_id) = server_id {
        let server = db.get_mcp_server(server_id).map_err(|e| e.to_string())?;
        let owned_stdio = super::runtime::binding(server.command.as_deref())?.is_some_and(|b| b.key == key);
        if !owned_stdio && !r.preview.servers.iter().any(|s| s.config["url"].as_str().is_some_and(|url| Some(url) == server.url.as_deref())) { return Err("所选连接不属于此插件的 MCP 服务".into()); }
    } else if !r.preview.servers.is_empty() { return Err("请先选择插件使用的连接".into()); }
    if enabled && !r.preview.skills.is_empty() {
        crate::mcp::registry_api::ensure_agent_load_skill_for_assistant(&app_handle, assistant_id)?;
    }
    // Both associations live in assistant.db and commit together.
    let skills = crate::db::skill_db::SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let transaction = skills.conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let table_exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='assistant_mcp_config')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !table_exists { return Err("助手 MCP 授权表不存在".into()); }
    if let Some(server_id) = server_id {
        transaction.execute("INSERT INTO assistant_mcp_config(assistant_id,mcp_server_id,is_enabled) VALUES(?1,?2,?3) ON CONFLICT(assistant_id,mcp_server_id) DO UPDATE SET is_enabled=excluded.is_enabled", rusqlite::params![assistant_id,server_id,enabled]).map_err(|e| e.to_string())?;
    }
    for skill in &r.preview.skills {
        transaction.execute("INSERT INTO assistant_skill_config(assistant_id,skill_identifier,is_enabled) VALUES(?1,?2,?3) ON CONFLICT(assistant_id,skill_identifier) DO UPDATE SET is_enabled=excluded.is_enabled", rusqlite::params![assistant_id,skill.id,enabled]).map_err(|e| e.to_string())?;
    }
    transaction.commit().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_agent_plugin_assistants(app_handle: tauri::AppHandle, key: String, server_id: Option<i64>) -> Result<Vec<super::super::AssistantAccess>, String> {
    let r = installed(&app_handle, &key).await?;
    let assistants = crate::db::assistant_db::AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let mut stmt = assistants.conn.prepare("SELECT id,name FROM assistant ORDER BY name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_,i64>(0)?, row.get::<_,String>(1)?))).map_err(|e| e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())?;
    let skills = crate::db::skill_db::SkillDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    rows.into_iter().map(|(id,name)| {
        let configured = skills.get_assistant_skill_configs(id).map_err(|e| e.to_string())?;
        let mut enabled = r.preview.skills.iter().all(|s| configured.iter().any(|c| c.skill_identifier == s.id && c.is_enabled));
        if let Some(server_id) = server_id { enabled &= assistants.get_assistant_mcp_configs(id).map_err(|e| e.to_string())?.iter().any(|c| c.mcp_server_id == server_id && c.is_enabled); }
        Ok(super::super::AssistantAccess { id, name, enabled })
    }).collect()
}

#[tauri::command]
pub async fn connect_agent_plugin_stdio(app_handle: tauri::AppHandle, key: String, server_name: String, values: std::collections::HashMap<String,String>, server_id: Option<i64>) -> Result<i64,String> {
    use super::runtime;
    use tauri::Emitter;
    let _guard = INSTALL_LOCK.lock().await;
    let r = installed(&app_handle,&key).await?;
    let server = r.preview.servers.iter().find(|s| s.name == server_name).ok_or("插件服务不存在")?;
    if server.config.get("command").is_none() { return Err("所选服务不是 stdio".into()); }
    if values.iter().any(|(k,v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0')) { return Err("环境变量名称或值无效".into()); }
    let directory = data(&app_handle)?;
    let check = r.clone(); let config = server.config.clone(); let inputs = values.clone();
    tokio::task::spawn_blocking(move || {
        if store::modified(&directory,&check)? { return Err("插件有本地修改，请恢复原文件后连接".into()); }
        runtime::command(runtime::launch(&config,&store::active(&directory,&check.preview.key)?,&inputs)?)?;
        Ok::<(),String>(())
    }).await.map_err(|e| e.to_string())??;
    let _change = super::super::CHANGE_LOCK.lock().await;
    let db = super::super::db(&app_handle)?;
    let app_id = format!("agent-plugin:{key}:{server_name}");
    if let Some(id) = server_id {
        let owned: bool = db.conn.query_row("SELECT EXISTS(SELECT 1 FROM app_connection WHERE server_id=?1 AND app_id=?2)",rusqlite::params![id,app_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !owned { return Err("不能覆盖其他插件的连接".into()); }
    }
    let reference = uuid::Uuid::new_v4().to_string();
    let binding = runtime::Binding { key, name: server_name.clone(), digest: r.preview.digest.clone(), reference: reference.clone() };
    let endpoint = runtime::endpoint(&binding);
    let command = format!("{}{}",runtime::PREFIX,serde_json::to_string(&binding).map_err(|e|e.to_string())?);
    let label = format!("{} · {} · {}",r.preview.name,server_name,&reference[..8]);
    let definition = AppDefinition { id: app_id.clone(), version:r.preview.version, name:r.preview.name, description:r.preview.description,
        publisher:r.preview.publisher, category:"Agent 插件".into(), icon:None, homepage:None, notice:"本地 stdio 服务；环境变量保存在加密凭据库。".into(),
        platforms:vec!["windows".into(),"macos".into(),"linux".into()], fields:vec![],skill_ids:vec![],
        connection:ConnectionRecipe { transport:"stdio".into(),oauth_client_auth:None,oauth_redirect_uri:None,url:endpoint.clone(),auth:"none".into(),client_id:None,oauth_resource:None,scopes:vec![] } };
    let transaction = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let id = if let Some(id) = server_id {
        db.conn.execute("INSERT OR IGNORE INTO app_credential_cleanup(reference,server_id) SELECT credential_ref,server_id FROM app_connection WHERE server_id=?1 AND credential_ref IS NOT NULL",[id]).map_err(|e|e.to_string())?;
        db.update_mcp_server_with_builtin(id,&label,Some(&definition.description),"stdio",Some(&command),None,None,None,Some(60000),false,false,false,false).map_err(|e|e.to_string())?; id
    } else {
        db.upsert_mcp_server_with_builtin(&label,Some(&definition.description),"stdio",Some(&command),None,None,None,Some(60000),false,false,false,true,false).map_err(|e|e.to_string())?
    };
    db.conn.execute("INSERT INTO app_connection(server_id,request_id,app_id,definition,label,config,credential_ref,status) VALUES(?1,?2,?3,?4,?5,'{}',?2,'unchecked') ON CONFLICT(server_id) DO UPDATE SET request_id=excluded.request_id,definition=excluded.definition,label=excluded.label,credential_ref=excluded.credential_ref,status='unchecked',error=NULL",
        rusqlite::params![id,reference,app_id,serde_json::to_string(&definition).map_err(|e|e.to_string())?,label]).map_err(|e|e.to_string())?;
    transaction.commit().map_err(|e|e.to_string())?;
    drop(db);
    let vault_app = app_handle.clone(); let vault_reference = reference.clone();
    let saved = tokio::task::spawn_blocking(move || super::super::credentials::save(&vault_app,&vault_reference,&super::super::credentials::Credential { server_id:id, endpoint,headers:values,oauth:None })).await.map_err(|e|e.to_string())?;
    if let Err(error) = saved { super::super::set_status(&app_handle,id,"error",Some(&error))?; return Err(format!("连接 {id} 保存环境变量失败，未启用：{error}")); }
    super::super::db(&app_handle)?.toggle_mcp_server(id,true).map_err(|e|e.to_string())?;
    crate::sync::schedule_sync_after_local_change(&app_handle);
    let _ = app_handle.emit("mcp_state_changed",());
    Ok(id)
}

#[tauri::command]
pub async fn create_agent_plugin_assistant(app_handle: tauri::AppHandle, key: String, agent_path: String, template_id: i64) -> Result<i64,String> {
    use tauri::Emitter;
    let r = installed(&app_handle,&key).await?;
    let agent = r.preview.agents.iter().find(|a|a.path==agent_path).ok_or("插件没有此子助手定义")?;
    let db = crate::db::assistant_db::AssistantDatabase::new(&app_handle).map_err(|e|e.to_string())?;
    let template = db.get_assistant(template_id).map_err(|e|e.to_string())?;
    if !matches!(template.assistant_type.unwrap_or(0), 0 | 4) {
        return Err("插件子助手模板目前支持普通助手和原生 Agent；请选择其中一种，工作流等特殊助手配置不能直接继承".into());
    }
    let models = db.get_assistant_model(template_id).map_err(|e|e.to_string())?;
    if models.is_empty() { return Err("模板助手没有配置模型，请先配置模型".into()); }
    let providers = crate::db::llm_db::LLMDatabase::new(&app_handle).map_err(|e|e.to_string())?;
    for model in &models {
        providers.get_llm_provider(model.provider_id).map_err(|e|format!("模板助手的提供商不存在：{e}"))?;
        if template.assistant_type != Some(4) { providers.get_llm_model_detail(&model.provider_id,&model.model_code).map_err(|e|format!("模板助手的模型不存在：{e}"))?; }
    }
    let transaction = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let id = db.add_assistant(&format!("{} · {}",r.preview.name,agent.name),&format!("来自 {}/{}；{}",r.preview.source.repo,agent.path,r.preview.revision),template.assistant_type,false).map_err(|e|e.to_string())?;
    let folder = store::active(&data(&app_handle)?,&key)?;
    db.add_assistant_prompt(id,&format!("{}\n\n[AIPP 插件环境]\n原始助手定义：{}\n插件目录：{}\n模型、执行通道和工具权限由用户选定的模板助手继承；不得自行替换模型或执行通道。工具名以实际工具目录为准。",agent.content,folder.join(&agent.path).display(),folder.display())).map_err(|e|e.to_string())?;
    for model in models {
        let new_id = db.add_assistant_model(id,model.provider_id,&model.model_code,&model.alias).map_err(|e|e.to_string())?;
        for config in db.get_assistant_model_configs_with_model_id(template_id,model.id).map_err(|e|e.to_string())? {
            db.add_assistant_model_config(id,new_id,&config.name,config.value.as_deref().unwrap_or(""),&config.value_type).map_err(|e|e.to_string())?;
        }
    }
    for sql in [
        "INSERT INTO assistant_mcp_config(assistant_id,mcp_server_id,is_enabled) SELECT ?1,mcp_server_id,is_enabled FROM assistant_mcp_config WHERE assistant_id=?2",
        "INSERT INTO assistant_mcp_tool_config(assistant_id,mcp_tool_id,is_enabled,is_auto_run) SELECT ?1,mcp_tool_id,is_enabled,is_auto_run FROM assistant_mcp_tool_config WHERE assistant_id=?2",
        "INSERT INTO assistant_skill_config(assistant_id,skill_identifier,is_enabled) SELECT ?1,skill_identifier,is_enabled FROM assistant_skill_config WHERE assistant_id=?2",
        "INSERT INTO assistant_workspace(assistant_id,path) SELECT ?1,path FROM assistant_workspace WHERE assistant_id=?2",
    ] { db.conn.execute(sql,rusqlite::params![id,template_id]).map_err(|e|e.to_string())?; }
    transaction.commit().map_err(|e|e.to_string())?;
    let _ = app_handle.emit("assistant_list_changed",());
    crate::sync::schedule_sync_after_local_change(&app_handle);
    Ok(id)
}

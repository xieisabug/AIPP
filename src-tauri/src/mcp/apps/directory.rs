use super::{catalog, db, oauth, AppCatalog, AppDefinition};
use crate::db::connection::{params, OptionalExtension};
use serde_json::Value;
use std::sync::LazyLock;
use std::time::Duration;

static CLIENT: LazyLock<Result<reqwest::Client, String>> = LazyLock::new(oauth::http);

fn client() -> Result<&'static reqwest::Client, String> { CLIENT.as_ref().map_err(Clone::clone) }

#[tauri::command]
pub async fn fetch_app_directory(category: Option<String>, query: Option<String>, cursor: Option<u32>,
    version: Option<String>, native_only: bool, paged: bool, etag: Option<String>) -> Result<Value,String> {
    let mut url = reqwest::Url::parse(catalog::CATALOG_URL).map_err(|e| e.to_string())?;
    if paged { url.path_segments_mut().map_err(|_| "目录地址无效")?.push("page"); }
    {
        let mut args = url.query_pairs_mut();
        args.append_pair("lang","zh-CN");
        if native_only { args.append_pair("scope","native"); }
        if let Some(category) = category { args.append_pair("category",&category); }
        if let Some(query) = query { args.append_pair("q",&query); }
        if let Some(cursor) = cursor { args.append_pair("cursor",&cursor.to_string()); }
        if let Some(version) = version { args.append_pair("version",&version); }
    }
    let mut request = client()?.get(url);
    if let Some(tag) = etag { request = request.header(reqwest::header::IF_NONE_MATCH,tag); }
    let response = request.send().await.map_err(|e| format!("获取应用目录失败：{}",e.without_url()))?;
    if response.status()==reqwest::StatusCode::NOT_MODIFIED { return Ok(serde_json::json!({"unchanged":true})); }
    if response.status()==reqwest::StatusCode::CONFLICT { return Ok(serde_json::json!({"changed":true})); }
    let tag=response.headers().get(reqwest::header::ETAG).and_then(|s|s.to_str().ok()).map(str::to_owned);
    let payload=oauth::json(response,"应用目录").await?;
    Ok(serde_json::json!({"data":payload,"etag":tag}))
}

pub async fn detail(app: &tauri::AppHandle, id: &str, version: Option<&str>) -> Result<AppDefinition,String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'-'|b'_')) { return Err("应用标识无效".into()); }
    let database=db(app)?;
    let cached: Option<(String,String,String,i64)> = database.conn.query_row(
        "SELECT payload,version,etag,checked FROM app_directory_detail WHERE id=?1",[id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e|e.to_string())?;
    let now=chrono::Utc::now().timestamp();
    if let Some((raw,saved,_,checked))=&cached {
        if version.map_or(true,|v|v==saved) && (0..300).contains(&(now-checked)) {
            return serde_json::from_str(raw).map_err(|e|format!("应用详情缓存无效：{e}"));
        }
    }
    let mut request=client()?.get(format!("{}/{id}?lang=zh-CN",catalog::CATALOG_URL)).timeout(Duration::from_secs(10));
    if let Some((_,_,tag,_))=&cached { if !tag.is_empty() { request=request.header(reqwest::header::IF_NONE_MATCH,tag); } }
    let response=match request.send().await {
        Ok(r)=>r,
        Err(e)=> return match cached {
            Some((raw,_,_,_))=>serde_json::from_str(&raw).map_err(|e|e.to_string()),
            None=>Err(format!("获取应用 {id} 详情失败：{}",e.without_url()))
        },
    };
    if response.status()==reqwest::StatusCode::NOT_MODIFIED {
        let (raw,old,tag,_)=cached.ok_or("应用详情缓存缺失")?;
        database.conn.execute("UPDATE app_directory_detail SET version=?2,checked=?3 WHERE id=?1 AND etag=?4",
            params![id,version.unwrap_or(&old),now,tag]).map_err(|e|e.to_string())?;
        return serde_json::from_str(&raw).map_err(|e|e.to_string());
    }
    let tag=response.headers().get(reqwest::header::ETAG).and_then(|s|s.to_str().ok()).unwrap_or("").to_owned();
    let value=oauth::json(response,"应用详情").await?;
    let entry:AppDefinition=serde_json::from_value(value["app"].clone()).map_err(|e|format!("应用详情无效：{e}"))?;
    if entry.id!=id { return Err("应用详情身份不匹配".into()); }
    AppCatalog{schema_version:1,version:entry.version.clone(),apps:vec![entry.clone()]}.validate()?;
    database.conn.execute("INSERT OR REPLACE INTO app_directory_detail(id,payload,version,etag,checked) VALUES(?1,?2,?3,?4,?5)",
        params![id,serde_json::to_string(&entry).map_err(|e|e.to_string())?,version.unwrap_or(""),tag,now]).map_err(|e|e.to_string())?;
    let connected:bool=database.conn.query_row("SELECT EXISTS(SELECT 1 FROM app_connection WHERE app_id=?1)",
        [id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if connected {
        database.rebuild_dynamic_mcp_catalog_force().map_err(|e|e.to_string())?;
        super::super::summarizer::trigger_pending_app_summaries(app);
    }
    Ok(entry)
}

#[tauri::command]
pub async fn get_app_directory_detail(app_handle: tauri::AppHandle, id: String, version: Option<String>) -> Result<AppDefinition,String> {
    let mut entry=detail(&app_handle,&id,version.as_deref()).await?;
    entry.tool_summaries.clear();
    Ok(entry)
}

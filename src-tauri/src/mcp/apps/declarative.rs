use super::catalog::{public_https, AppDefinition};
use crate::db::mcp_db::MCPServer;
use crate::mcp::http_transport;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpApi {
    pub version: u32,
    pub key_url: Option<String>,
    pub probe: String,
    pub tools: Vec<Operation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: Vec<String>,
    /// Custom metadata headers accept percent-encoded text only.
    #[serde(default)]
    pub headers: Vec<String>,
    #[serde(default)]
    pub body: Option<Body>,
    #[serde(default)]
    pub result_pointer: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Body { Json, Base64 }

fn identifier(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl HttpApi {
    pub fn validate(&self, app: &AppDefinition) -> Result<(), String> {
        if self.version != 1 || self.tools.len() > 256 { return Err("不支持的 HTTP 接口版本或工具数量".into()); }
        if let Some(url) = &self.key_url { public_https(url)?; }
        let mut names = HashSet::new();
        for op in &self.tools {
            if !identifier(&op.name) || !names.insert(&op.name) || op.description.trim().is_empty()
                || !matches!(op.method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE")
                || op.method == "GET" && op.body.is_some() {
                return Err(format!("HTTP 工具声明无效：{}", op.name));
            }
            if op.result_pointer.as_ref().is_some_and(|p| !p.is_empty() && !p.starts_with('/')) {
                return Err("结果字段需要 JSON Pointer".into());
            }
            let mut parameters = HashSet::new();
            for segment in op.path.strip_prefix('/').ok_or("需要绝对路径")?.split('/') {
                if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    if !identifier(key) { return Err("路径参数名称无效".into()); }
                    parameters.insert(key);
                } else if segment.is_empty() || segment == "." || segment == ".." ||
                    !segment.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b)) {
                    return Err("HTTP 工具路径包含无效片段".into());
                }
            }
            for header in &op.headers {
                if !header.starts_with("x-") || !identifier(header) || header.starts_with("x-forwarded-")
                    || app.fields.iter().filter_map(|f| f.header.as_ref()).any(|h| h.eq_ignore_ascii_case(header))
                    || header == super::credentials::REFERENCE_HEADER { return Err("工具请求头与连接配置冲突或无效".into()); }
            }
            for key in op.query.iter().chain(&op.headers) {
                if !identifier(key) || !parameters.insert(key) { return Err("HTTP 参数名无效或重复".into()); }
            }
            if op.body.is_some() && !parameters.insert("body") { return Err("body 参数重复".into()); }
            let props = op.input_schema["properties"].as_object().ok_or("工具输入缺少 properties")?;
            if op.input_schema["type"] != "object" || props.len() != parameters.len() ||
                !props.keys().all(|key| parameters.contains(key.as_str())) { return Err(format!("工具参数与绑定不匹配：{}", op.name)); }
        }
        if !self.tools.iter().any(|op| op.name == self.probe && op.method == "GET" &&
            op.input_schema["properties"].as_object().is_some_and(|p| p.is_empty())) {
            return Err("连接检测需要无参数 GET 工具".into());
        }
        Ok(())
    }
}

impl Operation {
    fn request(&self, client: &reqwest::Client, mut url: reqwest::Url, args: &Value) -> Result<reqwest::RequestBuilder, String> {
        let args = args.as_object().ok_or("HTTP 工具参数必须是对象")?;
        let props = self.input_schema["properties"].as_object().ok_or("工具参数定义无效")?;
        if let Some(key) = args.keys().find(|k| !props.contains_key(*k)) { return Err(format!("未知参数：{key}")); }
        if let Some(required) = self.input_schema["required"].as_array() {
            for key in required.iter().filter_map(Value::as_str) {
                if !args.contains_key(key) { return Err(format!("缺少参数：{key}")); }
            }
        }
        {
            let mut path = url.path_segments_mut().map_err(|_| "HTTP 服务地址无效")?;
            path.pop_if_empty();
            for segment in self.path.trim_start_matches('/').split('/') {
                if let Some(key) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    let value = args.get(key).and_then(Value::as_str).ok_or_else(|| format!("缺少路径参数 {key}"))?;
                    if value.is_empty() || matches!(value, "." | "..") || value.contains(['/', '\\', '%']) { return Err(format!("路径参数 {key} 无效")); }
                    path.push(value);
                } else { path.push(segment); }
            }
        }
        for key in &self.query {
            if let Some(value) = args.get(key) {
                let text = match value { Value::String(s) => s.clone(), Value::Number(_) | Value::Bool(_) => value.to_string(), _ => return Err(format!("查询参数 {key} 需要标量")) };
                url.query_pairs_mut().append_pair(key, &text);
            }
        }
        let method = reqwest::Method::from_bytes(self.method.as_bytes()).map_err(|_| "HTTP 方法无效")?;
        let mut request = client.request(method, url);
        for key in &self.headers {
            if let Some(value) = args.get(key) {
                let value = value.as_str().ok_or("请求头参数需要文本")?;
                let encoded = urlencoding::encode(value);
                request = request.header(key, encoded.as_ref());
            }
        }
        if let Some(value) = args.get("body") {
            request = match self.body {
                Some(Body::Json) => request.json(value),
                Some(Body::Base64) => request.header("content-type", "application/octet-stream").body(
                    base64::engine::general_purpose::STANDARD.decode(value.as_str().ok_or("文件内容需要 Base64 文本")?).map_err(|_| "无效的 Base64 文件内容")?),
                None => return Err("该工具不接受请求体".into()),
            };
        }
        Ok(request)
    }
}

fn installed(app: &tauri::AppHandle, server: &MCPServer) -> Result<HttpApi, String> {
    let raw: String = super::db(app)?.conn.query_row("SELECT definition FROM app_connection WHERE server_id=?1", [server.id], |r| r.get(0)).map_err(|e| format!("读取 HTTP 应用定义失败：{e}"))?;
    let def: AppDefinition = serde_json::from_str(&raw).map_err(|e| format!("HTTP 应用定义无效：{e}"))?;
    super::catalog::validate_app(&def)?;
    if def.connection.transport != server.transport_type || Some(&def.connection.url) != server.url.as_ref() { return Err("HTTP 连接与安装定义不符".into()); }
    def.connection.http_api.ok_or_else(|| "缺少 HTTP 工具声明".into())
}

async fn call(app: &tauri::AppHandle, server: &MCPServer, api: &HttpApi, tool: &str, args: &Value,
    conversation: Option<i64>, probe: bool) -> Result<String, String> {
    let op = api.tools.iter().find(|op| op.name == tool).ok_or("安装定义中不存在该工具")?;
    let builder = http_transport::configured_http_builder(app, server).await?
        .timeout(Duration::from_millis(server.timeout.unwrap_or(30000).max(1) as u64))
        .retry(reqwest::retry::never());
    let (client, url, redactor) = http_transport::authenticated_client(app, server, builder).await?;
    let request = op.request(&client, public_https(&url)?, args)?;
    // Credential resolution can await a refresh. Recheck the installed connection before sending,
    // including probes, which run without a conversation or assistant permissions.
    let db = super::db(app)?;
    if !super::access::is_managed(&db.conn, server.id)? ||
        !server.is_enabled || db.get_mcp_server(server.id).map_err(|e| e.to_string())? != *server {
        return Err("HTTP 应用连接已停用、移除或变更，请重新发起请求".into());
    }
    drop(db);
    if !probe { super::access::validate_execution(app, server, tool, conversation)?; }
    let response = request.send().await.map_err(|e| format!("{} {} 请求失败，{}；提交结果可能未知", op.method, op.path, e.without_url()))?;
    let status = response.status();
    if status.as_u16() == 401 { super::record_auth_failure(app, server, http_transport::AUTH_REJECTED_MESSAGE); }
    let redactor = redactor.merged(crate::vault::placeholder::current_redactor().as_deref());
    read_result(op, response, &redactor).await
}

async fn read_result(op: &Operation, mut response: reqwest::Response, redactor: &http_transport::ResponseRedactor) -> Result<String, String> {
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("读取工具响应失败：{}", e.without_url()))? {
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 { return Err("工具响应超过 8 MiB".into()); }
        bytes.extend_from_slice(&chunk);
    }
    let text = match serde_json::from_slice::<Value>(&bytes) {
        Ok(mut value) => {
            if let Some(pointer) = op.result_pointer.as_deref().filter(|_| status.is_success()) {
                value = value.pointer(pointer).ok_or("响应缺少声明的结果字段")?.clone();
            }
            redactor.clean_json(&mut value);
            redactor.clean(&value.to_string())
        }
        Err(_) if status.is_success() && op.result_pointer.is_some() => return Err("结果字段提取需要 JSON 响应".into()),
        Err(_) => redactor.clean(&String::from_utf8_lossy(&bytes)),
    };
    if !status.is_success() { return Err(format!("{} {} HTTP {status}：{text}", op.method, op.path)); }
    Ok(json!([{"type":"text","text":text}]).to_string())
}

pub async fn probe(app: &tauri::AppHandle, server: &MCPServer) -> Result<HttpApi, String> {
    let api = installed(app, server)?;
    call(app, server, &api, &api.probe, &json!({}), None, true).await?;
    Ok(api)
}

pub(crate) fn readonly_operations(app:&tauri::AppHandle,server:&MCPServer)->Result<Vec<Operation>,String>{
    Ok(installed(app,server)?.tools.into_iter().filter(|op|op.method=="GET" && op.body.is_none()).collect())
}

pub(crate) async fn proactive_read(app:&tauri::AppHandle,server:&MCPServer,name:&str,args:&Value,session:i64)->Result<String,String>{
    let api=installed(app,server)?;
    let op=api.tools.iter().find(|op|op.name==name && op.method=="GET" && op.body.is_none()).ok_or("该操作没有已安装的只读 GET 定义")?;
    super::access::validate_execution(app,server,name,Some(session))?;
    let review=crate::mcp::tool_review::review_proactive_read(app,session,&server.name,op,args).await;
    if review.verdict!="safe" {return Ok(format!("未执行应用查询，审核结果为 {}：{}。如需用户操作，请提出建议。",review.verdict,review.reason))}
    // The request uses this exact reviewed snapshot. The ordinary transport rechecks the connection before sending.
    call(app,server,&api,name,args,Some(session),false).await
}

pub async fn execute(app: &tauri::AppHandle, server: &MCPServer, name: &str, parameters: &str, conversation: Option<i64>) -> Result<String, String> {
    let api = installed(app, server)?;
    let args = Value::Object(crate::mcp::execution_api::parse_tool_arguments(parameters).map_err(|e| format!("工具参数无效：{e:#}"))?);
    call(app, server, &api, name, &args, conversation, false).await
}

#[cfg(test)]
#[path = "declarative_tests.rs"]
mod tests;

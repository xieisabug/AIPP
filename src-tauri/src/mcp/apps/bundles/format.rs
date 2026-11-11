use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashSet}, io::{Cursor, Read}};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source { pub repo: String, #[serde(default = "default_ref")] pub git_ref: String, pub path: String }
fn default_ref() -> String { "main".into() }
impl Source {
    pub fn validate(&self) -> Result<(), String> {
        let parts: Vec<_> = self.repo.split('/').collect();
        if parts.len() != 2 || parts.iter().any(|p| p.is_empty() || p.starts_with('.') || !p.chars().all(|c| c.is_ascii_alphanumeric() || "-_ .".contains(c) && c != ' ')) {
            return Err("GitHub 仓库必须为 owner/repo".into());
        }
        safe_path(&self.path)?;
        if self.git_ref.is_empty() || self.git_ref.len() > 200 || self.git_ref.chars().any(char::is_control) { return Err("Git revision 无效".into()); }
        Ok(())
    }
    pub fn key(&self) -> String { hex::encode(Sha256::digest(format!("{}/{}", self.repo.to_lowercase(), self.path).as_bytes()))[..24].to_string() }
}

pub fn safe_path(path: &str) -> Result<String, String> {
    let path = path.strip_prefix("./").unwrap_or(path);
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':', '\0']) || path.split('/').any(|s| s.is_empty() || s == "." || s == ".." || s.ends_with(['.', ' ']) || matches!(s.split('.').next().unwrap_or("").to_ascii_uppercase().as_str(), "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5" | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9")) {
        return Err(format!("插件路径无效或越界：{path}"));
    }
    Ok(path.to_string())
}

pub type Files = BTreeMap<String, Vec<u8>>;
pub fn permissions(bytes: &[u8], source: &Source) -> Result<BTreeMap<String,u32>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut result = BTreeMap::new();
    let prefix = format!("{}/", safe_path(&source.path)?);
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() { continue; }
        if let Some(path) = entry.name().split_once('/').and_then(|(_,p)| p.strip_prefix(&prefix)) {
            result.insert(safe_path(path)?, entry.unix_mode().unwrap_or(0o644) & 0o777);
        }
    }
    Ok(result)
}
pub fn unpack(bytes: &[u8], source: &Source) -> Result<Files, String> {
    source.validate()?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("插件 ZIP 无效：{e}"))?;
    if archive.len() > 30000 { return Err("插件仓库条目超过 30000".into()); }
    let mut files = Files::new(); let mut names = HashSet::new(); let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some((_, relative)) = entry.name().split_once('/') else { continue; };
        let prefix = format!("{}/", safe_path(&source.path)?);
        let Some(path) = relative.strip_prefix(&prefix) else { continue; };
        if path.is_empty() || entry.is_dir() { continue; }
        let path = safe_path(path)?;
        if path.split('/').next().is_some_and(|s| s.eq_ignore_ascii_case(".aipp-install.json")) || !names.insert(path.to_lowercase()) { return Err(format!("插件含保留或重复路径：{path}")); }
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) { return Err(format!("插件含符号链接：{path}")); }
        total = total.checked_add(entry.size()).ok_or("插件大小溢出")?;
        if total > 64 * 1024 * 1024 || files.len() >= 8192 { return Err("单个插件超过 64 MiB 或 8192 个文件".into()); }
        let expected = entry.size(); let mut data = Vec::new();
        entry.by_ref().take(expected + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() as u64 != expected { return Err(format!("插件文件大小异常：{path}")); }
        files.insert(path, data);
    }
    if files.is_empty() { return Err(format!("仓库中没有插件目录 {}", source.path)); }
    Ok(files)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skill { pub id: String, pub path: String, pub name: String, pub description: String, pub content: String, pub files: Vec<String>, #[serde(default)] pub kind: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Agent { pub path: String, pub name: String, pub content: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Server { pub name: String, pub config: Value, pub reason: Option<String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub key: String, pub source: Source, pub revision: String, pub digest: String,
    pub name: String, pub description: String, pub version: String, pub publisher: String,
    pub manifest: Value, pub skills: Vec<Skill>, pub servers: Vec<Server>, pub notices: Vec<String>,
    pub files: Vec<String>, pub installed_revision: Option<String>, pub locally_modified: bool,
    #[serde(default)] pub file_hashes: BTreeMap<String, String>,
    #[serde(default)] pub icon: Option<String>,
    #[serde(default)] pub permissions: BTreeMap<String, u32>,
    #[serde(default)] pub content_digest: Option<String>,
    #[serde(default)] pub agents: Vec<Agent>,
    #[serde(default)] pub dependencies: Vec<super::dependencies::Dependency>,
}

impl Preview {
    pub fn set_permissions(&mut self, permissions: BTreeMap<String, u32>) {
        let content = self.content_digest.clone().unwrap_or_else(|| self.digest.clone());
        let mut hash = Sha256::new();
        hash.update(content.as_bytes());
        for (path, mode) in &permissions { hash.update((path.len() as u64).to_le_bytes()); hash.update(path.as_bytes()); hash.update(mode.to_le_bytes()); }
        self.digest = hex::encode(hash.finalize());
        self.content_digest = Some(content);
        self.permissions = permissions;
    }
}

fn json(files: &Files, path: &str) -> Result<Value, String> {
    serde_json::from_slice(files.get(path).ok_or_else(|| format!("插件缺少 {path}"))?).map_err(|e| format!("{path} JSON 无效：{e}"))
}
fn component(files: &Files, manifest: &Value, field: &str, default: &str) -> Result<Option<Value>, String> {
    match manifest.get(field) {
        Some(Value::String(path)) => Ok(Some(json(files, &safe_path(path)?)?)),
        Some(value) if value.is_object() => Ok(Some(value.clone())),
        Some(_) => Err(format!("插件 {field} 必须是路径或对象")),
        None if files.contains_key(default) => Ok(Some(json(files, default)?)),
        None => Ok(None),
    }
}
pub fn digest(files: &Files) -> String {
    let mut hash = Sha256::new();
    for (name, bytes) in files { hash.update((name.len() as u64).to_le_bytes()); hash.update(name.as_bytes()); hash.update((bytes.len() as u64).to_le_bytes()); hash.update(bytes); }
    hex::encode(hash.finalize())
}
pub fn inspect(mut source: Source, revision: String, files: &Files) -> Result<Preview, String> {
    source.validate()?;
    source.path = safe_path(&source.path)?;
    let portable = files.contains_key("plugin.json");
    let manifest = json(files, if portable { "plugin.json" } else { ".codex-plugin/plugin.json" })?;
    let name = manifest["name"].as_str().filter(|s| !s.is_empty()).ok_or("插件清单缺少 name")?.to_string();
    if portable && manifest["$schema"].as_str() != Some("https://agent-plugins.org/schemas/1.0.0/plugin.schema.json") { return Err("不支持的 plugin.json 协议；AIPP JS 插件请从原插件入口安装".into()); }
    let mut roots = vec!["skills".to_string()];
    if !portable { if let Some(value) = manifest.get("skills") {
        roots = match value {
            Value::String(s) => vec![safe_path(s.trim_end_matches('/'))?],
            Value::Array(values) => values.iter().map(|v| safe_path(v.as_str().ok_or("skills 数组必须包含路径")?.trim_end_matches('/'))).collect::<Result<_,_>>()?,
            _ => return Err("skills 必须为目录路径或路径数组".into()),
        };
    }}
    let mut skills = Vec::new();
    for (path, bytes) in files {
        if !path.ends_with("/SKILL.md") || !roots.iter().any(|r| path.strip_prefix(&format!("{r}/")).is_some_and(|p| p.matches('/').count() <= 1)) { continue; }
        let content = String::from_utf8(bytes.clone()).map_err(|e| format!("{path} 不是 UTF-8：{e}"))?;
        let metadata = crate::skills::parser::SkillParser::metadata_from_text(&content)?;
        let folder = path.trim_end_matches("/SKILL.md");
        skills.push(Skill { id: format!("agent_plugin:{}/{}", source.key(), folder), path: path.clone(), kind: "skill".into(),
            name: metadata.name.unwrap_or_else(|| folder.rsplit('/').next().unwrap_or(folder).into()), description: metadata.description.unwrap_or_default(), content,
            files: files.keys().filter(|p| p.starts_with(&format!("{folder}/")) && *p != path).cloned().collect() });
    }
    let mut notices = Vec::new(); let mut servers = Vec::new();
    let mcp = component(files, &manifest, "mcpServers", if portable { "mcp.json" } else { ".mcp.json" })?;
    if let Some(config) = mcp {
        let entries = config.get("mcpServers").unwrap_or(&config).as_object().ok_or("mcpServers 必须是对象")?;
        for (name, config) in entries {
            let kind = config["type"].as_str().unwrap_or(if config.get("command").is_some() { "stdio" } else { "http" });
            let reason = if kind == "stdio" { if config["command"].as_str().is_none() { Some("stdio 缺少 command".into()) } else { None } }
                else if !["http", "streamable_http", "streamable-http"].contains(&kind) { Some(format!("不支持的传输类型 {kind}")) }
                else if config["oauth"].get("client_secret").is_some() || config["oauth"]["client_id"].as_str().is_some_and(|s| s.starts_with('<')) || config["oauth"].get("callback_port").is_some() { Some("需要为 AIPP 注册 OAuth 客户端并适配回调；不能复用其他宿主的私密客户端配置".into()) }
                else { super::super::catalog::public_https(config["url"].as_str().unwrap_or("")).err() };
            servers.push(Server { name: name.clone(), config: config.clone(), reason });
        }
    }
    for (path,bytes) in files.iter().filter(|(p,_)|p.ends_with("agents/openai.yaml")) {
        let yaml=std::str::from_utf8(bytes).map_err(|e|format!("{path}: {e}"))?;
        let documents=yaml_rust2::YamlLoader::load_from_str(yaml).map_err(|e|format!("{path}: {e}"))?;
        if let Some(deps)=documents.first().and_then(|d|d["dependencies"]["tools"].as_vec()) {
            for dep in deps {
                let name=dep["value"].as_str().unwrap_or("未命名依赖");
                if dep["type"].as_str()==Some("mcp") {
                    if let Some(url)=dep["url"].as_str() { if !servers.iter().any(|s|s.name==name) {
                        servers.push(Server {name:name.into(),config:serde_json::json!({"type":"http","url":url}),reason:super::super::catalog::public_https(url).err()});
                    }}
                } else { notices.push(format!("技能运行依赖：{name}（{}）",dep["type"].as_str().unwrap_or("未声明类型"))); }
            }
        }
    }
    if let Some(apps) = component(files, &manifest, "apps", ".app.json")? {
        for (name, _) in apps.get("apps").unwrap_or(&apps).as_object().ok_or("apps 必须是对象")? {
            if !servers.iter().any(|s| &s.name == name) { notices.push(format!("{name} 只有 ChatGPT 注册连接器 ID，没有独立 MCP 地址；在 AIPP 中尚不可连接")); }
        }
    }
    let mut agents = Vec::new();
    for (path, bytes) in files {
        if !path.ends_with(".md") { continue; }
        if path.starts_with("commands/") {
            let content = String::from_utf8(bytes.clone()).map_err(|e|e.to_string())?;
            let metadata = crate::skills::parser::SkillParser::extract_metadata(&content,std::path::Path::new(path))?;
            let command = path.trim_end_matches(".md");
            skills.push(Skill { id:format!("agent_plugin:{}/{}",source.key(),command), path:path.clone(),name:metadata.name.unwrap_or_else(||command.into()),
                description:metadata.description.unwrap_or_else(||"插件命令模板，可通过斜杠命令调用并附加参数。".into()), content,files:files.keys().filter(|p|*p != path).cloned().collect(),kind:"command".into() });
        } else if path.starts_with("agents/") {
            agents.push(Agent { path:path.clone(),name:path.trim_start_matches("agents/").trim_end_matches(".md").into(),content:String::from_utf8(bytes.clone()).map_err(|e|e.to_string())? });
        }
    }
    for field in ["hooks"] {
        if manifest.get(field).is_some() || files.keys().any(|p| p.starts_with(&format!("{field}/"))) { notices.push(format!("包含 {field} 扩展：完整保留文件，尚未在 AIPP 注册该宿主能力")); }
    }
    if skills.iter().any(|s| s.files.iter().any(|p| p.contains("/scripts/"))) { notices.push("技能包含脚本；执行需要助手获准使用相应本地工具与依赖。安装不执行脚本。".into()); }
    let icon = manifest["interface"]["logo"].as_str().and_then(|p| safe_path(p).ok()).and_then(|p| {
        use base64::Engine;
        let mime = if p.ends_with(".png") { "image/png" } else if p.ends_with(".jpg") || p.ends_with(".jpeg") { "image/jpeg" } else if p.ends_with(".webp") { "image/webp" } else { return None; };
        files.get(&p).filter(|b| b.len() <= 1024 * 1024).map(|b| format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b)))
    });
    Ok(Preview { key: source.key(), source, revision, digest: digest(files), name: manifest["interface"]["displayName"].as_str().unwrap_or(&name).into(),
        description: manifest["interface"]["longDescription"].as_str().or(manifest["description"].as_str()).unwrap_or("").into(),
        version: manifest["version"].as_str().unwrap_or("未声明").into(), publisher: manifest["author"]["name"].as_str().unwrap_or("").into(),
        manifest, skills, servers, notices, files: files.keys().cloned().collect(), installed_revision: None, locally_modified: false,
        file_hashes: files.iter().map(|(p,b)| (p.clone(),hex::encode(Sha256::digest(b)))).collect(), icon, permissions: BTreeMap::new(), content_digest: None, agents, dependencies:vec![] })
}

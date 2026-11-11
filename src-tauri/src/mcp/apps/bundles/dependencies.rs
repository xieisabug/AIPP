use super::format::{Preview, Source};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::{collections::HashMap,io::{Cursor,Read}};

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Dependency { pub name:String, pub connector_id:String, pub source:Option<Source> }

/// Match opaque platform IDs only against another public package's explicit HTTP declaration.
pub fn resolve(preview: &mut Preview, archive: &[u8]) -> Result<(),String> {
    let mut zip=zip::ZipArchive::new(Cursor::new(archive)).map_err(|e|e.to_string())?;
    let mut apps=HashMap::<String,Value>::new(); let mut mcps=HashMap::<String,Value>::new();
    for i in 0..zip.len() {
        let mut entry=zip.by_index(i).map_err(|e|e.to_string())?;
        let parts:Vec<_>=entry.name().split('/').map(str::to_string).collect();
        if parts.len()!=4 || parts[1]!="plugins" || ![".app.json",".mcp.json"].contains(&parts[3].as_str()) {continue;}
        if entry.size()>1024*1024 {return Err("插件依赖清单超过 1 MiB".into());}
        let mut bytes=Vec::new(); entry.by_ref().take(1024*1024+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        let value:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
        if parts[3]==".app.json" {apps.insert(parts[2].clone(),value);} else {mcps.insert(parts[2].clone(),value);}
    }
    let mut providers=HashMap::<String,String>::new();
    for (package,apps) in &apps {
        let Some(entries)=apps["apps"].as_object() else {continue};
        let Some(servers)=mcps.get(package).and_then(|m|m["mcpServers"].as_object()) else {continue};
        for (name,app) in entries {
            if servers.iter().any(|(n,s)|s["url"].as_str().is_some() && (n==name || entries.len()==1)) {
                if let Some(id)=app["id"].as_str() {providers.insert(id.into(),package.clone());}
            }
        }
    }
    preview.notices.retain(|n|!n.contains("只有 ChatGPT 注册连接器 ID"));
    if let Some(entries)=apps.get(preview.source.path.trim_start_matches("plugins/")).and_then(|a|a["apps"].as_object()) {
        for (name,app) in entries {
            let Some(id)=app["id"].as_str() else {continue};
            let source=providers.get(id).map(|package|Source {repo:preview.source.repo.clone(),git_ref:preview.source.git_ref.clone(),path:format!("plugins/{package}")});
            if source.as_ref().is_some_and(|s|s.path==preview.source.path) {continue;}
            preview.dependencies.push(Dependency {name:name.clone(),connector_id:id.into(),source});
        }
    }
    Ok(())
}

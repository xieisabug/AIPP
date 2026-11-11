use super::format::{self, Files, Preview};
use crate::skills::{parser::SkillParser, types::{ScannedSkill, SkillSourceType}};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt { pub preview: Preview, pub installed_at: String, #[serde(default)] pub format_version: u32 }

pub fn root(data: &Path) -> PathBuf { data.join("agent-plugins") }
fn valid_key(key: &str) -> Result<(), String> {
    if key.len() != 24 || !key.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("插件安装标识无效".into()); }
    Ok(())
}
pub fn active(data: &Path, key: &str) -> Result<PathBuf, String> {
    valid_key(key)?;
    let folder = root(data).join(key);
    // A crash between the two renames leaves previous as the committed revision.
    Ok(if folder.join("current").exists() { folder.join("current") } else { folder.join("previous") })
}
pub fn receipt(data: &Path, key: &str) -> Result<Option<Receipt>, String> {
    let path = active(data, key)?.join(".aipp-install.json");
    if !path.exists() { return Ok(None); }
    let mut r: Receipt = serde_json::from_slice(&fs::read(&path).map_err(|e| format!("读取安装记录：{e}"))?).map_err(|e| format!("安装记录损坏：{e}"))?;
    if r.preview.key != key || r.preview.source.key() != key { return Err("插件安装记录身份不匹配".into()); }
    if r.format_version < 2 {
        let folder=active(data,key)?;let mut files=Files::new();read_tree(&folder,&folder,&mut files)?;
        if format::digest(&files)==r.preview.content_digest.as_deref().unwrap_or(&r.preview.digest) {
            let fresh=format::inspect(r.preview.source.clone(),r.preview.revision.clone(),&files)?;
            r.preview.skills=fresh.skills;r.preview.agents=fresh.agents;r.preview.servers=fresh.servers;r.preview.notices=fresh.notices;
        }
    }
    Ok(Some(r))
}
pub fn list(data: &Path) -> Result<Vec<Receipt>, String> {
    if !root(data).exists() { return Ok(vec![]); }
    let mut results = Vec::new();
    for entry in fs::read_dir(root(data)).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let key = entry.file_name().to_string_lossy().to_string();
        if valid_key(&key).is_err() { continue; }
        if let Some(r) = receipt(data, &key)? { results.push(r); }
    }
    Ok(results)
}
fn read_tree(folder: &Path, base: &Path, files: &mut Files) -> Result<(), String> {
    for entry in fs::read_dir(folder).map_err(|e| format!("读取插件目录：{e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path(); let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        { use std::os::windows::fs::MetadataExt; if metadata.file_attributes() & 0x400 != 0 { return Err("插件目录包含重解析点，不能更新".into()); } }
        if metadata.file_type().is_symlink() || !path.canonicalize().map_err(|e| e.to_string())?.starts_with(base.canonicalize().map_err(|e| e.to_string())?) { return Err("插件目录包含越界链接".into()); }
        if metadata.is_dir() { read_tree(&path, base, files)?; }
        else {
            let name = path.strip_prefix(base).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
            if name == ".aipp-install.json" { continue; }
            if metadata.len() > 64 * 1024 * 1024 || files.len() > 8192 { return Err("本地插件文件超过读取上限".into()); }
            files.insert(name, fs::read(path).map_err(|e| e.to_string())?);
        }
    }
    Ok(())
}
pub fn modified(data: &Path, r: &Receipt) -> Result<bool, String> {
    let path = active(data, &r.preview.key)?; let mut files = Files::new(); read_tree(&path, &path, &mut files)?;
    #[cfg(unix)]
    for (name, expected) in &r.preview.permissions {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path.join(name)).map_err(|e| e.to_string())?.permissions().mode() & 0o777 != *expected { return Ok(true); }
    }
    Ok(format::digest(&files) != r.preview.content_digest.as_deref().unwrap_or(&r.preview.digest))
}
pub fn install(data: &Path, preview: &Preview, files: &Files) -> Result<Receipt, String> {
    valid_key(&preview.key)?;
    if format::digest(files) != preview.content_digest.as_deref().unwrap_or(&preview.digest) { return Err("插件内容与预览摘要不一致，请重新检查".into()); }
    if let Some(old) = receipt(data, &preview.key)? {
        if modified(data, &old)? { return Err("插件有本地修改，更新已停止。请先备份并恢复原文件，避免覆盖你的修改。".into()); }
        if old.preview.digest == preview.digest { return Ok(old); }
    }
    let folder = root(data).join(&preview.key);
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let current = folder.join("current"); let previous = folder.join("previous");
    if !current.exists() && previous.exists() { fs::rename(&previous, &current).map_err(|e| format!("恢复上次安装：{e}"))?; }
    let staging = folder.join(format!("staging-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::create_dir(&staging).map_err(|e| e.to_string())?;
        for (name, bytes) in files {
            let path = staging.join(format::safe_path(name)?);
            fs::create_dir_all(path.parent().ok_or("插件文件没有父目录")?).map_err(|e| e.to_string())?;
            fs::write(&path, bytes).map_err(|e| format!("准备插件文件 {name}：{e}"))?;
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; let mode = preview.permissions.get(name).copied().unwrap_or(0o644); fs::set_permissions(&path, fs::Permissions::from_mode(mode)).map_err(|e| format!("保存文件权限 {name}：{e}"))?; }
        }
        let receipt = Receipt { preview: preview.clone(), installed_at: chrono::Utc::now().to_rfc3339(), format_version:2 };
        fs::write(staging.join(".aipp-install.json"), serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if previous.exists() { fs::remove_dir_all(&previous).map_err(|e| format!("清理旧备份：{e}"))?; }
        if current.exists() { fs::rename(&current, &previous).map_err(|e| format!("备份当前插件（旧版未修改）：{e}"))?; }
        if let Err(e) = fs::rename(&staging, &current) {
            let restore = if previous.exists() { fs::rename(&previous, &current).err().map(|e| e.to_string()) } else { None };
            return Err(format!("提交插件失败：{e}；恢复错误：{}", restore.unwrap_or_else(|| "无".into())));
        }
        Ok(receipt)
    })();
    if staging.exists() { let _ = fs::remove_dir_all(&staging); }
    result
}
pub fn rollback(data: &Path, key: &str) -> Result<(), String> {
    valid_key(key)?;
    let folder = root(data).join(key); let current = folder.join("current"); let previous = folder.join("previous");
    let old = receipt(data, key)?.ok_or("插件未安装")?;
    if modified(data, &old)? { return Err("插件有本地修改，不能回退覆盖".into()); }
    if !previous.join(".aipp-install.json").exists() { return Err("没有可回退的上一版本".into()); }
    let swap = folder.join(format!("rollback-{}", uuid::Uuid::new_v4()));
    fs::rename(&current, &swap).map_err(|e| e.to_string())?;
    if let Err(e) = fs::rename(&previous, &current) { let restore = fs::rename(&swap, &current); return Err(format!("回退失败：{e}；恢复：{restore:?}")); }
    fs::rename(&swap, &previous).map_err(|e| format!("已回退，但保存新版本备份失败：{e}"))?;
    Ok(())
}
pub fn uninstall(data: &Path, key: &str) -> Result<(), String> {
    valid_key(key)?;
    let r = receipt(data, key)?.ok_or("插件未安装")?;
    if modified(data, &r)? { return Err("插件有本地修改，请先备份并恢复原文件后卸载".into()); }
    let folder = root(data).join(key);
    let removed = root(data).join(format!("removed-{}", uuid::Uuid::new_v4()));
    fs::rename(&folder, &removed).map_err(|e| format!("卸载插件：{e}"))?;
    fs::remove_dir_all(&removed).map_err(|e| format!("插件已卸载，但清理文件失败（{}）：{e}", removed.display()))
}

pub(crate) fn scan_installed_skills(data: &Path) -> Vec<ScannedSkill> {
    let receipts = match list(data) { Ok(r) => r, Err(e) => { tracing::warn!(error=%e, "Agent plugin scan failed"); return vec![]; } };
    let mut result = Vec::new();
    for r in receipts {
        let Ok(folder) = active(data, &r.preview.key) else { continue; };
        for skill in r.preview.skills {
            let path = folder.join(&skill.path);
            if let Ok(metadata) = SkillParser::parse_metadata(&path) {
                result.push(ScannedSkill { identifier: skill.id.clone(), source_type: SkillSourceType::Custom("agent_plugin".into()),
                    source_display_name: format!("插件 · {}", r.preview.name), file_path: path.to_string_lossy().into(),
                    relative_path: skill.id.trim_start_matches("agent_plugin:").into(), metadata, display_name: skill.name, exists: true });
            }
        }
    }
    result
}
pub(crate) fn skill_context(data: &Path, id: &str) -> Result<String, String> {
    let relative = id.strip_prefix("agent_plugin:").ok_or("不是插件技能")?;
    let (key, _) = relative.split_once('/').ok_or("插件技能身份无效")?;
    let r = receipt(data, key)?.ok_or("插件已卸载")?;
    let skill = r.preview.skills.iter().find(|s| s.id == id).ok_or("技能已从插件中移除")?;
    let path = active(data, key)?;
    Ok(format!("\n\n[AIPP 插件运行环境]\n插件：{}，版本：{}，提交：{}\n技能文件：{}\n相对参考资料与脚本路径从技能文件所在目录解析。通过 load_skill 的 file 参数按需读取配套文本文件，source_type=agent_plugin，command 使用当前技能标识，不要凭空补写。安装不执行脚本，执行脚本仍需相应授权工具。工具名称以本次实际工具目录为准；技能中的宿主前缀不是新的工具。连接或必需工具不可用时明确报告，不改用其他账号或执行通道。\n配套服务：{}\n附属文件：\n{}\n", r.preview.name, r.preview.version, r.preview.revision, path.join(&skill.path).display(),
        r.preview.servers.iter().map(|s| format!("{}: {}", s.name, s.config["url"].as_str().unwrap_or("需配置"))).collect::<Vec<_>>().join("；"), skill.files.iter().map(|p| {
            let base: Vec<_> = skill.path.split('/').collect();
            let base = &base[..base.len().saturating_sub(1)];
            let target: Vec<_> = p.split('/').collect();
            let common = base.iter().zip(&target).take_while(|(a,b)| a == b).count();
            std::iter::repeat("..").take(base.len()-common).chain(target[common..].iter().copied()).collect::<Vec<_>>().join("/")
        }).collect::<Vec<_>>().join("\n")))
}

pub(crate) fn read_skill_content(data: &Path, id: &str) -> Result<crate::skills::types::SkillContent, String> {
    let (key, _) = id.strip_prefix("agent_plugin:").ok_or("不是插件技能")?.split_once('/').ok_or("插件技能标识无效")?;
    let receipt = receipt(data,key)?.ok_or("插件未安装")?;
    let skill = receipt.preview.skills.iter().find(|s|s.id == id).ok_or("插件技能已移除")?;
    let file = skill.path.rsplit('/').next().ok_or("插件技能路径无效")?;
    let text = read_skill_file(data, id, file)?;
    let metadata = SkillParser::extract_metadata(&text,Path::new(&skill.path))?;
    let mut additional_files = Vec::new();
    for relative in metadata.requires_files {
        additional_files.push(crate::skills::types::SkillFile { content: read_skill_file(data, id, &relative)?, path: relative });
    }
    let command_hint = if skill.kind == "command" { "\n[AIPP 命令模板] 用户附加的文本是此命令的参数。按原模板解释参数与 $ARGUMENTS 占位符；没有提供的必需参数应向用户询问。命令按普通对话执行，保留当前助手的模型、工具与审批设置。\n" } else { "" };
    Ok(crate::skills::types::SkillContent { identifier: id.into(), content: format!("{}{}{}", SkillParser::extract_body(&text), command_hint, skill_context(data, id)?), additional_files })
}

pub(crate) fn read_skill_file(data: &Path, id: &str, relative: &str) -> Result<String, String> {
    let (key, _) = id.strip_prefix("agent_plugin:").ok_or("只有插件技能支持配套文件读取")?.split_once('/').ok_or("插件技能标识无效")?;
    let r = receipt(data, key)?.ok_or("插件未安装")?;
    let skill = r.preview.skills.iter().find(|s| s.id == id).ok_or("技能不存在")?;
    let folder = active(data, key)?;
    if relative.starts_with('/') || relative.contains(['\\', ':', '\0']) { return Err("配套文件路径必须相对于技能目录".into()); }
    let mut parts: Vec<_> = skill.path.split('/').collect(); parts.pop();
    for part in relative.split('/') {
        if part == ".." { if parts.pop().is_none() { return Err("配套文件越出插件目录".into()); } }
        else if part != "." && !part.is_empty() { parts.push(part); }
    }
    let normalized = format::safe_path(&parts.join("/"))?;
    let path = folder.join(normalized);
    let resolved = path.canonicalize().map_err(|e| format!("读取配套文件 {relative}：{e}"))?;
    if !resolved.starts_with(folder.canonicalize().map_err(|e| e.to_string())?) || !resolved.is_file() { return Err("配套文件越出插件目录".into()); }
    let size = resolved.metadata().map_err(|e| e.to_string())?.len();
    if size > 1024 * 1024 { return Err("配套文件超过 1 MiB，不能直接放入模型上下文".into()); }
    fs::read_to_string(&resolved).map_err(|e| format!("配套文件必须为 UTF-8 文本：{e}"))
}

//! Shared application service for assistant configuration. Never copies runtime state.
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
use crate::db::{assistant_db::AssistantDatabase, llm_db::LLMDatabase};

#[cfg(test)]
#[path = "assistant_management_tests.rs"]
mod tests;

#[path = "assistant_channel.rs"]
pub(crate) mod channel;

#[path = "assistant_permissions.rs"]
pub(crate) mod permissions;

pub(crate) fn editable(db: &AssistantDatabase, id: i64) -> Result<crate::db::assistant_db::Assistant, String> {
    let assistant = db.get_assistant(id).map_err(|e| format!("读取助手 #{id} 失败：{e}"))?;
    if crate::api::butler_api::is_butler_system_assistant(&assistant) {
        return Err("系统保留的总管家助手不能修改或复制".into());
    }
    Ok(assistant)
}

pub(crate) fn agents_skills(app: &tauri::AppHandle) -> Vec<String> {
    crate::api::skill_api::create_scanner(app).scan_all().into_iter()
        .filter(|skill| skill.source_type == crate::skills::types::SkillSourceType::Agents)
        .map(|skill| skill.identifier).collect()
}

pub(crate) async fn changed(app: &tauri::AppHandle, id: i64) {
    if let Ok(db) = AssistantDatabase::new(app) {
        if let Ok(assistant) = db.get_assistant(id) {
            if let Some(cache) = app.try_state::<crate::NameCacheState>() {
                cache.assistant_names.lock().await.insert(id, assistant.name);
            }
        }
    }
    notify(app);
}

pub(crate) fn notify(app: &tauri::AppHandle) {
    let _ = app.emit("assistant_list_changed", ());
    let _ = app.emit("skills-registry-changed", ());
    crate::sync::schedule_sync_after_local_change(app);
}

pub(crate) fn kind(value: &Value) -> Result<i64, String> {
    value.as_i64().filter(|kind| (0..=4).contains(kind))
        .ok_or_else(|| "assistant_type 须为 0–4（普通、多模型、工作流、展示、Agent）".into())
}

fn text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    args.get(key).map(|v| v.as_str().ok_or_else(|| format!("{key} 必须是字符串，清空请传空字符串"))).transpose()
}

pub(crate) fn validate_binding(llm: &LLMDatabase, assistant_type: i64, provider: i64, code: &str) -> Result<(), String> {
    let p = llm.get_llm_provider(provider).map_err(|e| format!("模型提供商 #{provider} 不存在：{e}"))?;
    if !p.is_enabled { return Err(format!("模型提供商 #{provider} 已禁用")); }
    let configs = llm.get_llm_provider_config(provider).map_err(|e| e.to_string())?;
    let agent = crate::api::ai::agent_driver::registry::registry().drivers().map_err(|e| e.to_string())?
        .iter().any(|driver| driver.matches_provider(&p.api_type, &configs));
    if (assistant_type == 4) != agent {
        return Err(format!("助手类型 {assistant_type} 与提供商通道 {} 不兼容", p.api_type));
    }
    if !agent && !llm.get_llm_models(provider.to_string()).map_err(|e| e.to_string())?
        .iter().any(|model| model.3 == code) {
        return Err(format!("提供商 #{provider} 中不存在模型 {code}，未修改助手"));
    }
    // Native model codes are channel-owned, including explicit default-model sentinels.
    if !agent && code.trim().is_empty() { return Err("普通助手模型代码不能为空".into()); }
    Ok(())
}

/// Only call on a newly created assistant, never on copies or existing rows.
pub(crate) fn defaults(db: &AssistantDatabase, id: i64, model: i64, skills: &[String]) -> Result<(), String> {
    for (name, value, ty) in [("max_tokens", "2000", "number"), ("temperature", "0.7", "float"),
        ("top_p", "1.0", "float"), ("stream", "true", "boolean"),
        ("dynamic_mcp_loading_enabled", "true", "boolean"), ("mcp_loading_mode_explicit", "true", "boolean")] {
        db.add_assistant_model_config(id, model, name, value, ty).map_err(|e| e.to_string())?;
    }
    for skill in skills {
        db.conn.execute("INSERT INTO assistant_skill_config(assistant_id,skill_identifier,is_enabled,priority) VALUES(?1,?2,1,0)", params![id,skill]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Atomic profile creation/update/copy, including all configuration relations.
pub(crate) fn profile(db: &AssistantDatabase, llm: &LLMDatabase, op: &str, args: &Value, skills: &[String], dry_run: bool) -> Result<Value, String> {
    let tx = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let source = if op == "create" { None } else {
        Some(editable(db, args["assistant_id"].as_i64().ok_or("需要 assistant_id")?)?)
    };
    let old_kind = source.as_ref().and_then(|s| s.assistant_type).unwrap_or(0);
    let new_kind = args.get("assistant_type").map(kind).transpose()?.unwrap_or(old_kind);
    let name = text(args, "name")?.map(str::to_owned).unwrap_or_else(|| match &source {
        Some(source) if op == "copy" => format!("副本 {}", source.name),
        Some(source) => source.name.clone(), None => String::new(),
    });
    if name.trim().is_empty() || crate::api::butler_api::is_butler_system_assistant_name(&name) {
        return Err("助手名称不能为空或使用系统保留名称".into());
    }
    let description = text(args,"description")?.map(str::to_owned)
        .unwrap_or_else(|| source.as_ref().and_then(|s| s.description.clone()).unwrap_or_default());
    let prompt = text(args,"prompt")?;
    if args.get("alias").is_some() && args.get("provider_id").is_none() && args.get("model_code").is_none() {
        return Err("alias 须与 provider_id、model_code 一起提交".into());
    }
    let binding = if args.get("provider_id").is_some() || args.get("model_code").is_some() {
        let provider = args["provider_id"].as_i64().ok_or("修改模型须同时提供 provider_id 与 model_code")?;
        let code = text(args,"model_code")?.ok_or("修改模型须同时提供 provider_id 与 model_code")?;
        validate_binding(llm,new_kind,provider,code)?;
        Some((provider,code,text(args,"alias")?.unwrap_or(code)))
    } else { None };
    if source.is_some() && old_kind != new_kind {
        if source.as_ref().is_some_and(|s| s.id == 1) { return Err("快速使用助手不能变更类型".into()); }
        // Workflow/display payloads have no generic type migration contract.
        if matches!(old_kind,2|3) || matches!(new_kind,2|3) {
            return Err("工作流/展示助手不支持直接变更类型，请新建对应类型助手".into());
        }
        if let Some(source) = &source {
            let models = db.get_assistant_model(source.id).map_err(|e| e.to_string())?;
            if binding.is_some() && models.len() > 1 { return Err("多模型助手变更类型前须先明确处理全部模型绑定".into()); }
            if binding.is_none() { for m in models { validate_binding(llm,new_kind,m.provider_id,&m.model_code)?; } }
            if old_kind == 4 && new_kind != 4 && db.get_assistant_model_configs(source.id).map_err(|e| e.to_string())?
                .iter().any(|c| c.name.starts_with("acp_") || c.name.starts_with("codex_") || c.name.starts_with("claude_")) {
                return Err("助手仍有 Agent 专属配置，请先清除不兼容覆盖值再变更类型".into());
            }
        }
    }
    let id = match (op, &source) {
        ("update",Some(source)) => source.id,
        ("copy",Some(source)) => copy_config(db,source.id,&name,&description,new_kind)?,
        ("create",_) => {
            let id = db.add_assistant(&name,&description,Some(new_kind),false).map_err(|e| e.to_string())?;
            let model = db.add_assistant_model(id,binding.map(|b|b.0).unwrap_or(0),binding.map(|b|b.1).unwrap_or(""),binding.map(|b|b.2).unwrap_or("")).map_err(|e|e.to_string())?;
            db.add_assistant_prompt(id,prompt.unwrap_or("You are a helpful assistant.")).map_err(|e|e.to_string())?;
            defaults(db,id,model,skills)?;
            id
        },
        _ => return Err(format!("不支持的助手操作：{op}")),
    };
    if op != "create" {
        db.conn.execute("UPDATE assistant SET name=?1,description=?2,assistant_type=?3 WHERE id=?4",params![name,description,new_kind,id]).map_err(|e|e.to_string())?;
        if let Some(prompt) = prompt {
            if let Some(old) = db.get_assistant_prompt(id).map_err(|e|e.to_string())?.first() {
                db.update_assistant_prompt(old.id,prompt).map_err(|e|e.to_string())?;
            } else { db.add_assistant_prompt(id,prompt).map_err(|e|e.to_string())?; }
        }
        if let Some((provider,code,alias)) = binding {
            if let Some(model) = db.get_assistant_model(id).map_err(|e|e.to_string())?.first() {
                db.update_assistant_model(model.id,provider,code,alias).map_err(|e|e.to_string())?;
            } else { db.add_assistant_model(id,provider,code,alias).map_err(|e|e.to_string())?; }
            db.conn.execute("UPDATE assistant_model_config SET value=?1 WHERE assistant_id=?2 AND name='acp_provider'",params![provider.to_string(),id]).map_err(|e|e.to_string())?;
        }
    }
    let result = json!({"assistant_id":id,"name":name,"assistant_type":new_kind,"saved":!dry_run,"dry_run":dry_run,
        "effective_when":"后续请求使用新配置；运行中的工作不变，未执行真实试运行"});
    if !dry_run { tx.commit().map_err(|e|e.to_string())?; }
    Ok(result)
}

fn copy_config(db: &AssistantDatabase, source: i64, name: &str, description: &str, kind: i64) -> Result<i64,String> {
    let original = db.get_assistant(source).map_err(|e|e.to_string())?;
    let id = db.add_assistant(name,description,Some(kind),original.is_addition).map_err(|e|e.to_string())?;
    for prompt in db.get_assistant_prompt(source).map_err(|e|e.to_string())? {
        let new_prompt = db.add_assistant_prompt(id,&prompt.prompt).map_err(|e|e.to_string())?;
        db.conn.execute("INSERT INTO assistant_prompt_param(assistant_id,assistant_prompt_id,param_name,param_type,param_value) SELECT ?1,?2,param_name,param_type,param_value FROM assistant_prompt_param WHERE assistant_id=?3 AND assistant_prompt_id=?4",params![id,new_prompt,source,prompt.id]).map_err(|e|e.to_string())?;
    }
    let mut model_ids = std::collections::HashMap::new();
    for model in db.get_assistant_model(source).map_err(|e|e.to_string())? {
        let new_model = db.add_assistant_model(id,model.provider_id,&model.model_code,&model.alias).map_err(|e|e.to_string())?;
        model_ids.insert(model.id,new_model);
    }
    for config in db.get_assistant_model_configs(source).map_err(|e|e.to_string())? {
        let new_model = model_ids.get(&config.assistant_model_id).copied().unwrap_or(0);
        db.add_assistant_model_config(id,new_model,&config.name,config.value.as_deref().unwrap_or(""),&config.value_type).map_err(|e|e.to_string())?;
    }
    for (table,columns) in [("assistant_mcp_config","mcp_server_id,is_enabled"),
        ("assistant_mcp_tool_config","mcp_tool_id,is_enabled,is_auto_run"),
        ("assistant_skill_config","skill_identifier,is_enabled,priority"), ("assistant_workspace","path")] {
        db.conn.execute(&format!("INSERT INTO {table}(assistant_id,{columns}) SELECT ?1,{columns} FROM {table} WHERE assistant_id=?2"),params![id,source]).map_err(|e|e.to_string())?;
    }
    Ok(id)
}

pub(crate) fn directory(raw: &str) -> Result<String,String> {
    let path = std::path::Path::new(raw);
    if !path.is_absolute() { return Err("工作区/工作目录必须为绝对路径".into()); }
    let path = path.canonicalize().map_err(|e|format!("无法访问目录 {raw}：{e}"))?;
    if !path.is_dir() { return Err(format!("路径不是目录：{raw}")); }
    std::fs::read_dir(&path).map_err(|e|format!("目录不可读 {raw}：{e}"))?;
    let value = path.to_string_lossy().to_string();
    #[cfg(windows)]
    let value = value.strip_prefix(r"\\?\UNC\").map(|p| format!(r"\\{p}"))
        .unwrap_or_else(|| value.strip_prefix(r"\\?\").unwrap_or(&value).to_string());
    Ok(value)
}

pub(crate) fn config_value(db: &AssistantDatabase, id: i64, name: &str) -> Result<Option<String>,String> {
    db.conn.query_row("SELECT value FROM assistant_model_config WHERE assistant_id=?1 AND name=?2",params![id,name],|r|r.get(0)).optional().map(|v|v.flatten()).map_err(|e|e.to_string())
}

pub(crate) fn config_model_id(db: &AssistantDatabase, id: i64) -> Result<i64,String> {
    let models = db.get_assistant_model(id).map_err(|e|e.to_string())?;
    let configs = db.get_assistant_model_configs(id).map_err(|e|e.to_string())?;
    models.first().map(|m|m.id)
        .or_else(||configs.iter().find(|c|c.name=="acp_provider").map(|c|c.assistant_model_id))
        .ok_or_else(||"助手没有模型绑定行或旧版 Agent 提供商配置行".into())
}

pub(crate) fn set_mcp_mode(db: &AssistantDatabase, id: i64, mode: &str, dry_run: bool) -> Result<Value,String> {
    if !matches!(mode,"dynamic"|"static") {return Err("mode 须为 dynamic 或 static".into());}
    let tx = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    editable(db,id)?;
    let model = config_model_id(db,id)?;
    for (name,value) in [("dynamic_mcp_loading_enabled",(mode=="dynamic").to_string()),("mcp_loading_mode_explicit","true".to_string())] {
        db.conn.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type) VALUES(?1,?2,?3,?4,'boolean') ON CONFLICT(assistant_id,name) DO UPDATE SET value=excluded.value,value_type=excluded.value_type",params![id,model,name,value]).map_err(|e|e.to_string())?;
    }
    if !dry_run {tx.commit().map_err(|e|e.to_string())?;}
    Ok(json!({"assistant_id":id,"mode":mode,"saved":!dry_run,"dry_run":dry_run,"effective_when":"后续请求生效；不会更改 Skills、服务连接或工具审核"}))
}

//! Channel-aware assistant overrides; driver schemas remain authoritative.
use rusqlite::params;
use serde_json::{json, Value};
use crate::api::ai::agent_driver::registry::{registry, AgentConfigField, AgentConfigValueType};
use crate::db::{assistant_db::AssistantDatabase, llm_db::{LLMDatabase, LLMProviderConfig}};

pub(crate) const EFFECT: &str = "后续请求重新解析配置。Agent 会话按配置签名决定是否重新建立；运行中的回合不变，未做真实运行验证。";

pub(crate) fn schema(kind: i64, api: &str, providers: &[LLMProviderConfig]) -> Result<Vec<AgentConfigField>,String> {
    let mut fields = if kind == 4 {
        let drivers = registry().drivers().map_err(|e|e.to_string())?;
        let driver = drivers.iter().find(|d| d.matches_provider(api,providers)).ok_or_else(||format!("助手通道 {api} 没有可用的原生驱动"))?;
        driver.config_schema().into_iter().filter(|f| matches!(f.name.as_str(),
            "acp_working_directory"|"codex_sandbox"|"codex_approval_policy"|"codex_approvals_reviewer")).collect::<Vec<_>>()
    } else {
        vec![AgentConfigField { name:"stream".into(),required:false,value_type:AgentConfigValueType::Boolean }]
    };
    fields.push(AgentConfigField {name:"tool_review_mode".into(),required:false,
        value_type:AgentConfigValueType::Choice(vec!["off".into(),"auto_model".into()])});
    Ok(fields)
}

fn context(db: &AssistantDatabase, llm: &LLMDatabase, id: i64) -> Result<(i64,String,Vec<LLMProviderConfig>),String> {
    let assistant = db.get_assistant(id).map_err(|e|format!("读取助手 #{id} 失败：{e}"))?;
    let kind = assistant.assistant_type.unwrap_or(0);
    let models = db.get_assistant_model(id).map_err(|e|e.to_string())?;
    let configs = db.get_assistant_model_configs(id).map_err(|e|e.to_string())?;
    let provider_id = if kind==4 {
        crate::api::assistant_api::resolve_acp_provider_id(&models,&configs)
    } else {models.first().map(|m|m.provider_id).filter(|id|*id>0)};
    let Some(provider_id) = provider_id else {
        if kind == 4 { return Err("Agent 助手未绑定提供商，请先配置模型绑定".into()); }
        return Ok((kind,String::new(),Vec::new()));
    };
    let provider = llm.get_llm_provider(provider_id).map_err(|e|format!("助手提供商 #{provider_id} 不存在：{e}"))?;
    let configs = llm.get_llm_provider_config(provider_id).map_err(|e|e.to_string())?;
    Ok((kind,provider.api_type,configs))
}

pub(crate) fn read(db: &AssistantDatabase, llm: &LLMDatabase, id: i64) -> Result<Value,String> {
    let (kind,api,providers) = context(db,llm,id)?;
    let configs = db.get_assistant_model_configs(id).map_err(|e|e.to_string())?;
    let mut fields = Vec::new();
    for field in schema(kind,&api,&providers)? {
        let override_value = configs.iter().find(|c|c.name == field.name).and_then(|c|c.value.as_deref()).map(str::trim).filter(|v|!v.is_empty());
        // tool_review_mode for AIPP operations and stream are assistant-only settings.
        let provider_value = if field.name=="stream" || (field.name=="tool_review_mode" && !crate::api::ai::agent_driver::native_agents::is_native_agent(&api)) { None } else {
            providers.iter().find(|c|c.name==field.name).map(|c|c.value.trim()).filter(|v|!v.is_empty())
        };
        let default = match field.name.as_str() {
            "codex_sandbox" => Some("workspace-write".to_string()),
            "codex_approval_policy" => Some("on-request".to_string()),
            "codex_approvals_reviewer" => Some("user".to_string()),
            "tool_review_mode" => Some("off".to_string()),
            "stream" => Some(crate::api::ai::config::DEFAULT_STREAM.to_string()),
            "acp_working_directory" => Some(dirs::home_dir().ok_or("无法确定通道默认工作目录")?.to_string_lossy().to_string()),
            _ => None,
        };
        let effective = override_value.or(provider_value).map(str::to_owned).or(default);
        let effective = if field.name=="stream" {
            Some(crate::api::ai::config::stream_enabled(effective.as_deref()).to_string())
        } else {effective};
        // ACP/other native adapters expand ~; Codex uses the literal configured path.
        let effective = effective.map(|value| if field.name=="acp_working_directory" && api!="codex_app_server" && (value=="~" || value.starts_with("~/") || value.starts_with("~\\")) {
            dirs::home_dir().map(|p|p.join(value.trim_start_matches('~').trim_start_matches(['/', '\\'])).to_string_lossy().to_string()).unwrap_or(value)
        } else { value });
        fields.push(json!({"name":field.name,"schema":field.value_type,"assistant_override":override_value,
            "provider_default":provider_value,"effective_value":effective,
            "source":if override_value.is_some(){"assistant"}else if provider_value.is_some(){"provider"}else{"channel_default"}}));
    }
    Ok(json!({"assistant_id":id,"channel":api,"parameters":fields,"effective_when":EFFECT,
        "runtime_verified":false,"aipp_operation_tool_review":configs.iter().find(|c|c.name=="tool_review_mode").and_then(|c|c.value.as_deref()).unwrap_or("off"),"butler_runtime":"管家模式自动注入的权限和审核独立于这些助手显式设置"}))
}

pub(crate) fn patch(db: &AssistantDatabase, llm: &LLMDatabase, id: i64, set: &Value, remove: &Value, dry_run: bool) -> Result<Value,String> {
    super::editable(db,id)?;
    let (kind,api,providers) = context(db,llm,id)?;
    let fields = schema(kind,&api,&providers)?;
    let set = set.as_object().ok_or("set 必须为对象")?;
    let remove = remove.as_array().ok_or("remove 必须为参数名数组")?;
    if set.is_empty() && remove.is_empty() { return Err("配置补丁不能为空".into()); }
    let mut values = Vec::new();
    for (name,value) in set {
        let field = fields.iter().find(|f| &f.name==name).ok_or_else(||format!("通道 {api} 不支持助手参数 {name}"))?;
        let text = match &field.value_type {
            AgentConfigValueType::Boolean => value.as_bool().map(|v|v.to_string()),
            AgentConfigValueType::String => value.as_str().filter(|v|!v.trim().is_empty()).map(str::to_owned),
            AgentConfigValueType::Choice(choices) => value.as_str().filter(|v|choices.iter().any(|c|c==v)).map(str::to_owned),
        }.ok_or_else(||format!("{name} 值无效，合法类型/值：{}；清除覆盖请使用 remove",serde_json::to_string(&field.value_type).unwrap()))?;
        let text = if name=="acp_working_directory" { super::directory(&text)? } else {text};
        let ty = if matches!(field.value_type,AgentConfigValueType::Boolean) {"boolean"} else {"string"};
        values.push((name,text,ty));
    }
    for name in remove {
        let name = name.as_str().ok_or("remove 元素必须为参数名")?;
        if !fields.iter().any(|f|f.name==name) || set.contains_key(name) { return Err(format!("删除参数不支持或与 set 冲突：{name}")); }
    }
    let tx = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    let models = db.get_assistant_model(id).map_err(|e|e.to_string())?;
    let configs = db.get_assistant_model_configs(id).map_err(|e|e.to_string())?;
    let model = models.first().map(|m|m.id)
        .or_else(||configs.iter().find(|c|c.name=="acp_provider").map(|c|c.assistant_model_id))
        .ok_or("助手没有模型绑定行或旧版 Agent 提供商配置行")?;
    for name in remove { db.conn.execute("DELETE FROM assistant_model_config WHERE assistant_id=?1 AND name=?2",params![id,name.as_str().unwrap()]).map_err(|e|e.to_string())?; }
    for (name,value,ty) in values {
        db.conn.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(assistant_id,name) DO UPDATE SET value=excluded.value,value_type=excluded.value_type",params![id,model,name,value,ty]).map_err(|e|e.to_string())?;
    }
    let mut result = read(db,llm,id)?;
    result["saved"] = json!(!dry_run); result["dry_run"] = json!(dry_run);
    if !dry_run { tx.commit().map_err(|e|e.to_string())?; }
    Ok(result)
}

pub(crate) fn workspace(db: &AssistantDatabase, llm: &LLMDatabase, id: i64, args: &Value, dry_run: bool) -> Result<Value,String> {
    let assistant = super::editable(db,id)?;
    let action = args["action"].as_str().unwrap_or("list");
    if action=="set_directory" {
        if assistant.assistant_type != Some(4) { return Err("只有 Agent 助手支持执行工作目录".into()); }
        return patch(db,llm,id,&json!({"acp_working_directory":args.get("path").ok_or("需要 path")?}),&json!([]),dry_run);
    }
    let tx = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
    match action {
        "list" => {},
        "add" => {
            let path = super::directory(args["path"].as_str().ok_or("需要 path")?)?;
            // Canonical comparison also handles old, non-normalized UI registrations.
            if !db.get_assistant_workspaces(id).map_err(|e|e.to_string())?.iter().any(|w|same_path(&w.path,&path)) {
                db.add_assistant_workspace(id,&path).map_err(|e|e.to_string())?;
            }
        },
        "remove" => {
            let wid = args["workspace_id"].as_i64().ok_or("需要 workspace_id")?;
            let workspaces = db.get_assistant_workspaces(id).map_err(|e|e.to_string())?;
            let target = workspaces.iter().find(|w|w.id==wid).ok_or("工作区不存在或不属于指定助手")?;
            if assistant.assistant_type == Some(4) {
                let state = read(db,llm,id)?;
                let cwd = state["parameters"].as_array().and_then(|p|p.iter().find(|p|p["name"]=="acp_working_directory")).and_then(|p|p["effective_value"].as_str());
                if cwd.is_some_and(|cwd|same_path(cwd,&target.path)) {
                    return Err("该工作区仍是当前生效的 Agent 工作目录，请先显式设置其他工作目录再移除；未撤销任何权限".into());
                }
            }
            db.remove_assistant_workspace(wid).map_err(|e|e.to_string())?;
        },
        _ => return Err(format!("不支持的工作区操作：{action}")),
    }
    let channel = if assistant.assistant_type==Some(4) { read(db,llm,id)? } else { Value::Null };
    let result = json!({"assistant_id":id,"action":action,"saved":!dry_run,"dry_run":dry_run,
        "workspaces":db.get_assistant_workspaces(id).map_err(|e|e.to_string())?,"channel_config":channel,
        "permissions":"工作区仅表示 AIPP 内置操作的路径信任；目录登记不更改 Agent 沙箱或审批，移除不撤销其他独立授权。执行起点仅由 acp_working_directory 决定，与工作区列表顺序无关。"});
    if !dry_run { tx.commit().map_err(|e|e.to_string())?; }
    Ok(result)
}

pub(crate) fn same_path(a: &str, b: &str) -> bool {
    let normalize = |p: &str| crate::utils::path_utils::normalize_path_for_comparison(std::path::Path::new(p));
    match (normalize(a),normalize(b)) { (Some(a),Some(b)) => a==b, _ => a==b }
}

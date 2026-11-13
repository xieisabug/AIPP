//! Butler configuration entry points. Every action uses the existing audit pipeline.
use async_trait::async_trait;
use serde_json::{json, Value};
use tauri::AppHandle;
use crate::api::assistant_api::management::{self, channel};
use crate::db::{assistant_db::AssistantDatabase, llm_db::LLMDatabase};
use super::super::{registry::{ActionHandler, ActionRegistry}, types::*};

pub(super) async fn profile_action(app: &AppHandle, op: &str, args: Value, dry_run: bool) -> Result<Value,String> {
    let result = {
        let db = AssistantDatabase::new(app).map_err(|e|e.to_string())?;
        let llm = LLMDatabase::new(app).map_err(|e|e.to_string())?;
        let skills = if op=="create" {management::agents_skills(app)} else {Vec::new()};
        management::profile(&db,&llm,op,&args,&skills,dry_run)?
    };
    if !dry_run { management::changed(app,result["assistant_id"].as_i64().ok_or("操作未返回助手 ID")?).await; }
    Ok(result)
}

struct Handler(&'static str);
#[async_trait]
impl ActionHandler for Handler {
    async fn execute(&self, app: &AppHandle, args: Value, dry_run: bool) -> Result<Value,String> {
        if matches!(self.0,"update"|"copy") { return profile_action(app,self.0,args,dry_run).await; }
        if self.0.starts_with("group_") { return group_action(app,self.0,args,dry_run).await; }
        let id = args["assistant_id"].as_i64().ok_or("需要 assistant_id")?;
        let result = {
            let db = AssistantDatabase::new(app).map_err(|e|e.to_string())?;
            let llm = LLMDatabase::new(app).map_err(|e|e.to_string())?;
            db.get_assistant(id).map_err(|e|format!("助手 #{id} 不存在：{e}"))?;
            match self.0 {
                "channel_get" => channel::read(&db,&llm,id)?,
                "channel_patch" => channel::patch(&db,&llm,id,&args["set"],&args["remove"],dry_run)?,
                "workspace" => channel::workspace(&db,&llm,id,&args,dry_run)?,
                "mcp_get" => {
                    let mcp = crate::db::mcp_db::MCPDatabase::new(app).map_err(|e|e.to_string())?;
                    let servers = mcp.get_mcp_servers().map_err(|e|e.to_string())?;
                    let bindings = db.get_assistant_mcp_configs(id).map_err(|e|e.to_string())?;
                    let available = servers.iter().map(|s|json!({"id":s.id,"name":s.name,
                        "globally_enabled":s.is_enabled,"assistant_binding":bindings.iter().find(|c|c.mcp_server_id==s.id).map(|c|c.is_enabled)})).collect::<Vec<_>>();
                    json!({"assistant_id":id,"dynamic_override":management::config_value(&db,id,"dynamic_mcp_loading_enabled")?,
                        "available_servers":available,"bindings":bindings,
                        "tool_bindings":db.get_assistant_mcp_tool_configs(id).map_err(|e|e.to_string())?,
                        "runtime_note":"此处为助手显式配置；动态发现继续受全局启用、连接、工具开关与审核约束。管家运行时统一注入能力，不代表助手显式绑定。"})
                },
                "mcp_mode" => {
                    management::editable(&db,id)?;
                    let mode = args["mode"].as_str().ok_or("需要 mode=dynamic 或 static")?;
                    if !matches!(mode,"dynamic"|"static") { return Err("mode 须为 dynamic 或 static".into()); }
                    if !dry_run {
                        let model = db.get_assistant_model(id).map_err(|e|e.to_string())?.first().map(|m|m.id).ok_or("助手缺少模型绑定行")?;
                        let tx = db.conn.unchecked_transaction().map_err(|e|e.to_string())?;
                        for (name,value) in [("dynamic_mcp_loading_enabled",(mode=="dynamic").to_string()),("mcp_loading_mode_explicit","true".to_string())] {
                            db.conn.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type) VALUES(?1,?2,?3,?4,'boolean') ON CONFLICT(assistant_id,name) DO UPDATE SET value=excluded.value,value_type=excluded.value_type",rusqlite::params![id,model,name,value]).map_err(|e|e.to_string())?;
                        }
                        tx.commit().map_err(|e|e.to_string())?;
                    }
                    json!({"assistant_id":id,"mode":mode,"saved":!dry_run,"dry_run":dry_run,"effective_when":"后续请求生效；不会更改 Skills、服务连接或工具审核"})
                },
                "skills_get" => {
                    let skill_db = crate::db::skill_db::SkillDatabase::new(app).map_err(|e|e.to_string())?;
                    let available = crate::api::skill_api::create_scanner(app).scan_all();
                    let configs = skill_db.get_assistant_skill_configs(id).map_err(|e|e.to_string())?;
                    json!({"assistant_id":id,"available_skills":available,"bindings":configs,
                        "load_skill_ready":crate::mcp::registry_api::is_agent_load_skill_ready(app,id)?,
                        "runtime_note":"此处仅为助手显式设置；管家自动注入的全局 Skills 与此区分，不会安装或下载外部技能。"})
                },
                _ => return Err(format!("未知助手配置操作：{}",self.0)),
            }
        };
        if !dry_run && matches!(self.0,"channel_patch"|"workspace"|"mcp_mode") { management::changed(app,id).await; }
        if self.0=="mcp_get" {
            let mut result = result;
            result["dynamic_effective"] = json!(crate::mcp::is_dynamic_mcp_loading_enabled_for_assistant(app,id).await);
            return Ok(result);
        }
        Ok(result)
    }
}

async fn group_action(app: &AppHandle, op: &str, args: Value, dry_run: bool) -> Result<Value,String> {
    use crate::api::butler_group as api;
    let result = match op {
        "group_list" => json!({"groups":api::list_butler_groups(app.clone()).await?.into_iter().map(|g|json!({"group_id":g.id,"name":g.name,"members":g.members})).collect::<Vec<_>>()}),
        "group_create" => {
            if dry_run { return Err("建群不支持 dry_run；未创建群聊".into()); }
            let name = args["name"].as_str().ok_or("需要群名称 name")?;
            let name = crate::db::butler_group::validate_name(name).map_err(|e|e.to_string())?;
            let ids = args["assistant_ids"].as_array().ok_or("需要 assistant_ids 数组")?.iter()
                .map(|v|v.as_i64().filter(|v|*v>0).ok_or("assistant_ids 必须为正整数"))
                .collect::<Result<Vec<_>,_>>()?;
            let chief = match args.get("include_chief") {None=>false,Some(v)=>v.as_bool().ok_or("include_chief 须为布尔值")?};
            json!(api::create_butler_group(app.clone(),Some(name),ids,chief).await?)
        },
        "group_get"|"group_rename" => {
            let id = args["group_id"].as_i64().ok_or("需要 group_id")?;
            let current = api::get_butler_group(app.clone(),id).await?;
            if op=="group_rename" {
                let name = crate::db::butler_group::validate_name(args["name"].as_str().ok_or("需要 name")?).map_err(|e|e.to_string())?;
                if dry_run { return Ok(json!({"group_id":id,"dry_run":true,"would_set_name":name})); }
                api::rename_butler_group(app.clone(),id,name).await?;
                let updated = api::get_butler_group(app.clone(),id).await?;
                json!({"group_id":updated.id,"name":updated.name,"members":updated.members,"saved":true})
            } else { json!({"group_id":current.id,"name":current.name,"members":current.members}) }
        },
        _ => return Err(format!("未知群管理操作：{op}")),
    };
    Ok(result)
}

fn register_action(registry: &mut ActionRegistry, id: &str, op: &'static str, summary: &str, description: &str, schema: Value, write: bool, scope: ActionScope) {
    let domain = id.split('.').next().unwrap();
    registry.register(ActionMeta {
        action_id:id.into(),domain:domain.into(),summary:summary.into(),description:description.into(),
        risk_level:if write {RiskLevel::MEDIUM}else{RiskLevel::SAFE},requires_approval:false,
        approval_policy:if write {ApprovalPolicy::AllowInScope}else{ApprovalPolicy::AutoAllow},
        allowed_scopes:vec![scope],tags:vec![domain.into(),if write {"write".into()}else{"read".into()},"configuration".into()],
        args_schema:schema,result_schema:json!({"type":"object"}),supports_dry_run:write && op!="group_create",rollback_hint:None,
    },Box::new(Handler(op)));
}

pub fn register(registry: &mut ActionRegistry) {
    let mut profile = json!({"type":"object","required":["assistant_id"],"properties":{
        "assistant_id":{"type":"integer"},"name":{"type":"string"},"description":{"type":"string"},
        "assistant_type":{"type":"integer","enum":[0,1,2,3,4]},"prompt":{"type":"string"},
        "provider_id":{"type":"integer"},"model_code":{"type":"string"},"alias":{"type":"string"}}});
    register_action(registry,"assistant.update","update","修改助手基础资料","仅修改显式字段，保留其他配置；模型绑定须同时提供 provider_id 和 model_code，非法/不兼容类型变更拒绝。",profile.clone(),true,ActionScope::Assistant);
    profile["properties"]["assistant_id"]["description"] = json!("来源助手 ID；复制全部配置，不复制聊天、记忆或工作状态");
    register_action(registry,"assistant.copy","copy","复制助手配置","保留来源模型参数、MCP、Skills、工作区及提示词参数，可覆盖基础资料；不应用新建默认值。",profile,true,ActionScope::Assistant);
    let assistant = json!({"type":"object","required":["assistant_id"],"properties":{"assistant_id":{"type":"integer"}}});
    register_action(registry,"assistant.get_channel_config","channel_get","查询通道配置与合法值","返回适用字段、合法值、助手覆盖、provider 默认、最终值与来源；不启动 Agent。",assistant.clone(),false,ActionScope::Assistant);
    register_action(registry,"assistant.patch_channel_config","channel_patch","修改或清除助手通道参数","先查询支持字段；set 设置严格类型，remove 清除覆盖并恢复继承。不会切换模型或通道。",json!({"type":"object","required":["assistant_id","set","remove"],"properties":{"assistant_id":{"type":"integer"},"set":{"type":"object"},"remove":{"type":"array","items":{"type":"string"}}}}),true,ActionScope::Assistant);
    register_action(registry,"assistant.get_mcp_config","mcp_get","查询助手 MCP 可用项与设置","返回服务与工具显式绑定，区分管家自动注入与助手设置。",assistant.clone(),false,ActionScope::Assistant);
    register_action(registry,"assistant.get_skill_config","skills_get","查询助手 Skills 可用项与设置","返回发现的技能及当前启停、优先级；设置使用独立的 assistant.update_skill_config。",assistant,false,ActionScope::Assistant);
    register_action(registry,"assistant.set_mcp_mode","mcp_mode","设置助手 MCP 加载模式","只修改动态/静态 MCP，不改变服务开关或 Skills；服务设置使用 assistant.update_mcp_config。",json!({"type":"object","required":["assistant_id","mode"],"properties":{"assistant_id":{"type":"integer"},"mode":{"enum":["dynamic","static"]}}}),true,ActionScope::Assistant);
    register_action(registry,"assistant.workspace","workspace","管理助手工作区与执行目录","list 查询、add 注册路径、remove 按 workspace_id 撤销路径信任、set_directory 显式设置 Agent cwd。移除当前 cwd 前必须先显式改目录。登记路径不改变原生沙箱或审批。",json!({"type":"object","required":["assistant_id","action"],"properties":{"assistant_id":{"type":"integer"},"action":{"enum":["list","add","remove","set_directory"]},"path":{"type":"string"},"workspace_id":{"type":"integer"}}}),true,ActionScope::Assistant);
    register_action(registry,"conversation.group_list","group_list","查询管家群聊","返回群 ID 与名称，以 ID 消歧同名群。",json!({"type":"object"}),false,ActionScope::App);
    register_action(registry,"conversation.group_get","group_get","查询管家群详情","按 group_id 查询当前名称及成员。",json!({"type":"object","required":["group_id"],"properties":{"group_id":{"type":"integer"}}}),false,ActionScope::App);
    register_action(registry,"conversation.group_create","group_create","创建命名管家群","指定名称和成员，使用现有群聊服务；不支持 dry_run。",json!({"type":"object","required":["name","assistant_ids"],"properties":{"name":{"type":"string"},"assistant_ids":{"type":"array","items":{"type":"integer"}},"include_chief":{"type":"boolean"}}}),true,ActionScope::App);
    register_action(registry,"conversation.group_rename","group_rename","修改管家群名","仅按 group_id 定位；保留成员、历史、执行上下文和记忆。",json!({"type":"object","required":["group_id","name"],"properties":{"group_id":{"type":"integer"},"name":{"type":"string"}}}),true,ActionScope::App);
}

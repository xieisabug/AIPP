//! Read-only discovery and bounded collaboration inspection for every Butler assistant.
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use crate::db::{assistant_db::AssistantDatabase, conversation_db::ConversationDatabase};
use crate::mcp::builtin_mcp::templates::BuiltinToolInfo;

pub fn definitions() -> Vec<BuiltinToolInfo> {
    vec![
        BuiltinToolInfo { name:"list_assistants".into(), description:"列出管家模式所有助手（含总管家），按姓名、简介或画像搜索。返回稳定 assistant_id、摘要、联系人会话和当前工作状态；不创建会话。用 ID 消歧同名助手。".into(), input_schema:json!({"type":"object","properties":{
            "query":{"type":"string","maxLength":512},"after_id":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":50}}}) },
        BuiltinToolInfo { name:"get_assistant_details".into(), description:"按需查看助手详情。默认 profile/summary/runtime；可选 tools、skills、prompt。工具状态区分直接注入和动态发现，不代表已验证外部服务可用。prompt 仅返回配置正文，非会话历史。".into(), input_schema:json!({"type":"object","required":["assistant_id"],"properties":{
            "assistant_id":{"type":"integer","minimum":1},"sections":{"type":"array","items":{"enum":["profile","summary","runtime","tools","skills","prompt"]}},
            "offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":50},
            "prompt_offset":{"type":"integer","minimum":0},"prompt_limit":{"type":"integer","minimum":1,"maximum":12000}}}) },
        BuiltinToolInfo { name:"get_assistant_exchange".into(), description:"查看自己参与的助手协作。传 exchange_id 查看该次协作的轮次、状态和可选往来正文；省略则分页列出最近协作。答复会自动回流，不要轮询。".into(), input_schema:json!({"type":"object","properties":{
            "exchange_id":{"type":"integer","minimum":1},"include_context":{"type":"boolean","default":false},
            "before_id":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1,"maximum":50}}}) },
        BuiltinToolInfo { name:"stop_assistant_exchange".into(), description:"停止自己参与的某次协作及其后续轮次，撤销尚未投递的请求/答复，记录原因。不会强制终止正在运行的模型或进程，也不会撤销已完成操作；重复调用幂等。".into(), input_schema:json!({"type":"object","required":["exchange_id","reason"],"properties":{
            "exchange_id":{"type":"integer","minimum":1},"reason":{"type":"string","minLength":1,"maxLength":1000}}}) },
    ]
}

pub fn is_tool(name: &str) -> bool {
    matches!(name,"list_assistants"|"get_assistant_details"|"get_assistant_exchange"|"stop_assistant_exchange")
}

fn number(args: &Value, key: &str, default: usize, max: usize) -> anyhow::Result<usize> {
    let Some(value) = args.get(key) else { return Ok(default) };
    let value = value.as_u64().context(format!("{key} 必须为非负整数"))?;
    ensure!(value <= max as u64, "{key} 最大为 {max}");
    Ok(value as usize)
}
fn limit(args: &Value) -> anyhow::Result<usize> {
    let value = number(args,"limit",20,50)?;
    ensure!(value>0,"limit 必须大于 0"); Ok(value)
}
fn positive(args: &Value, key: &str) -> anyhow::Result<i64> {
    args[key].as_i64().filter(|v| *v>0).context(format!("{key} 必须为正整数"))
}
fn page(items: Vec<Value>, offset: usize, limit: usize) -> Value {
    let total=items.len();
    let next=(offset.saturating_add(limit)<total).then_some(offset.saturating_add(limit));
    json!({"items":items.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":total,"next_offset":next})
}
fn clip(s: &str, limit: usize) -> String { s.chars().take(limit).collect() }

fn contact(conn: &Connection, assistant_id: i64) -> anyhow::Result<Option<i64>> {
    Ok(conn.query_row("SELECT id FROM conversation WHERE assistant_id=?1 AND conversation_kind IN ('butler_main','butler_contact') ORDER BY id DESC LIMIT 1",
        [assistant_id],|r| r.get(0)).optional()?)
}

async fn runtime(app: &AppHandle, assistant_id: i64) -> anyhow::Result<Value> {
    let (conversation_id,queued,waiting_reply)={
        let conn=ConversationDatabase::new(app)?.get_connection()?;
        let id=contact(&conn,assistant_id)?;
        let queued: i64=conn.query_row("SELECT count(*) FROM butler_peer_exchange WHERE target=?1 AND state='queued'",[id],|r|r.get(0))?;
        let waiting_reply: i64=conn.query_row("SELECT count(*) FROM butler_peer_exchange WHERE owner=?1 AND state IN ('queued','sending','waiting','returning','reply_sending')",[id],|r|r.get(0))?;
        (id,queued,waiting_reply)
    };
    let Some(id)=conversation_id else { return Ok(json!({"conversation_id":null,"state":"not_started"})) };
    let running=app.state::<crate::state::activity_state::ConversationActivityManager>().get_runtime_state(id).await;
    let waiting_permission=!app.state::<crate::mcp::builtin_mcp::OperationState>().list_permission_requests_for_conversation(id).await.is_empty()
        || !app.state::<crate::api::ai::acp::AcpPermissionState>().list_requests_for_conversation(id).await.is_empty();
    let waiting_user=!app.state::<crate::mcp::builtin_mcp::interaction::InteractionState>().list_requests_for_conversation(id).await.is_empty();

    let state=if waiting_permission {"waiting_permission"} else if waiting_user {"waiting_user"} else if running.is_running {"running"} else if queued>0 {"queued"} else if waiting_reply>0 {"waiting_assistant"} else {"idle"};
    Ok(json!({"conversation_id":id,"state":state,"runtime_phase":running.phase,"queued_requests":queued,"waiting_replies":waiting_reply,"observed_at":chrono::Utc::now()}))
}

pub async fn tool(app: &AppHandle, current: i64, name: &str, args: &Value) -> anyhow::Result<Value> {
    // Called only after the shared Butler conversation guard in butler_peer::tool.
    if matches!(name,"get_assistant_exchange"|"stop_assistant_exchange") {
        let conn=ConversationDatabase::new(app)?.get_connection()?;
        let args=args.clone(); let name=name.to_owned();
        let result=tokio::task::spawn_blocking(move || exchange_tool(&conn,current,&name,&args)).await??;
        if name_is_stop(result.get("operation")) { super::butler_peer::updated(app); }
        return Ok(result);
    }
    let db=AssistantDatabase::new(app)?;
    let names=super::butler_peer::bot_names(app).await?;
    if name=="list_assistants" {
        let query=args.get("query").map(|q|q.as_str().context("query 必须为字符串")).transpose()?.unwrap_or("").trim().to_lowercase();
        ensure!(query.chars().count()<=512,"query 最多 512 字符");
        let after=number(args,"after_id",0,i64::MAX as usize)? as i64;
        let limit=limit(args)?;
        let mut assistants=db.get_assistants()?;
        assistants.sort_by_key(|a|a.id);
        let mut matches=Vec::new();
        for assistant in assistants.into_iter().filter(|a|a.id>after) {
            let summary=db.get_assistant_summary(assistant.id)?;
            let is_chief=super::butler_api::is_butler_system_assistant(&assistant);
            let name=names.get(&assistant.id).cloned().unwrap_or(assistant.name);
            let description=assistant.description.unwrap_or_default();
            let summary_text=summary.as_ref().map(|s|s.summary.as_str()).unwrap_or(&description);
            let tags=summary.as_ref().map(|s|s.tags_json.as_str()).unwrap_or("[]");
            if !format!("{name}\n{description}\n{summary_text}\n{tags}").to_lowercase().contains(&query) {continue}
            matches.push((assistant.id,json!({"assistant_id":assistant.id,"name":name,"summary":clip(summary_text,300),
                "tags":serde_json::from_str::<Value>(tags).unwrap_or(json!([])),"assistant_type":assistant.assistant_type,"is_chief":is_chief})));
            if matches.len()>limit {break}
        }
        let more=matches.len()>limit; matches.truncate(limit);
        let next=if more {matches.last().map(|(id,_)|*id)} else {None};
        let mut items=Vec::new();
        for (id,mut item) in matches { item["runtime"]=runtime(app,id).await?; items.push(item); }
        return Ok(json!({"assistants":items,"next_after_id":next}));
    }
    ensure!(name=="get_assistant_details","未知助手工具 {name}");
    let id=positive(args,"assistant_id")?;
    let assistant=db.get_assistant(id).with_context(||format!("找不到助手 #{id}"))?;
    let sections=sections(args)?;
    let offset=number(args,"offset",0,1_000_000)?; let limit=limit(args)?;
    let mut result=json!({"assistant_id":id,"name":names.get(&id).unwrap_or(&assistant.name)});
    for section in sections {
        match section.as_str() {
            "profile"=> {
                let models=db.get_assistant_model(id)?;
                result["profile"]=json!({"description":assistant.description,"assistant_type":assistant.assistant_type,
                    "is_chief":super::butler_api::is_butler_system_assistant(&assistant),
                    "models":models.iter().map(|m|json!({"provider_id":m.provider_id,"model_code":m.model_code,"alias":m.alias})).collect::<Vec<_>>()});
            },
            "summary"=> {result["summary"]=db.get_assistant_summary(id)?.map(|s|json!({"text":s.summary,"tags":serde_json::from_str::<Value>(&s.tags_json).unwrap_or(json!([])),"updated_time":s.updated_time,"kind":"assistant_profile","source_hash":s.source_hash})).unwrap_or(Value::Null);},
            "runtime"=> {result["runtime"]=runtime(app,id).await?;},
            "prompt"=> {
                let text=db.get_assistant_prompt(id)?.into_iter().map(|p|p.prompt).collect::<Vec<_>>().join("\n\n");
                let start=number(args,"prompt_offset",0,10_000_000)?;
                let count=number(args,"prompt_limit",6000,12000)?; ensure!(count>0,"prompt_limit 必须大于 0");
                let total=text.chars().count();
                result["prompt"]=json!({"text":text.chars().skip(start).take(count).collect::<String>(),"total_chars":total,"next_offset":(start+count<total).then_some(start+count),"source":"configured_assistant_prompt"});
            },
            "skills"=> {
                let mut skills=super::skill_api::get_enabled_assistant_skills_internal(app,id).await?;
                skills.sort_by(|a,b|a.identifier.cmp(&b.identifier));
                result["skills"]=page(skills.into_iter().map(|s|json!({"identifier":s.identifier,"name":s.display_name,"description":s.metadata.description,"source":s.source_type,"available":s.exists})).collect(),offset,limit);
            },
            "tools"=> {
                let mut info=crate::mcp::collect_mcp_info_for_assistant(app,id,None,None).await?;
                // Native-agent bridges expose explicitly selected MCP tools, not the dynamic catalog.
                if assistant.assistant_type == Some(4) {
                    info.enabled_servers=super::assistant_api::get_assistant_mcp_servers_with_tools(app.clone(),id).await
                        .map_err(anyhow::Error::msg)?.into_iter().filter(|s|s.is_enabled).collect();
                    info.dynamic_loading_enabled=false;
                }
                crate::mcp::butler_tools::inject(app,&mut info.enabled_servers,"butler_contact")?;
                let contact_id=contact(&ConversationDatabase::new(app)?.get_connection()?,id)?;
                let mcp_db=crate::db::mcp_db::MCPDatabase::new(app)?;
                let mut tools=Vec::new();
                for server in info.enabled_servers {
                    for tool in server.tools.into_iter().filter(|t|t.is_enabled) {
                        let direct=crate::mcp::builtin_mcp::templates::is_dynamic_loading_fixed_tool(server.command.as_deref(),&tool.name,true);
                        let loaded=if info.dynamic_loading_enabled&&!direct {
                            contact_id.map(|cid|mcp_db.is_tool_loaded_for_conversation(cid,server.id,&tool.name)).transpose()?.unwrap_or(false)
                        } else {false};
                        tools.push(json!({"server_id":server.id,"server":server.name,"name":tool.name,"description":clip(&tool.description,800),
                            "availability":if !info.dynamic_loading_enabled||direct {"direct"} else if loaded {"loaded_via_call_mcp_tool"} else {"requires_loading"},
                            "injected_by_butler":crate::mcp::butler_tools::is_direct_tool("butler_contact",server.command.as_deref(),&tool.name)}));
                    }
                }
                tools.sort_by_key(|t|(t["server_id"].as_i64().unwrap_or(0),t["name"].as_str().unwrap_or("").to_owned()));
                result["tools"]=page(tools,offset,limit);
                result["tools"]["scope"]=json!("Butler MCP configuration; external service connectivity and native-agent tools are not probed");
            },
            _=> unreachable!(),
        }
    }
    Ok(result)
}

fn name_is_stop(operation: Option<&Value>) -> bool { operation.and_then(Value::as_str)==Some("stop") }
fn sections(args: &Value) -> anyhow::Result<Vec<String>> {
    let values=match args.get("sections") {
        Some(v)=>v.as_array().context("sections 必须为数组")?.clone(),
        None=>vec![json!("profile"),json!("summary"),json!("runtime")],
    };
    let mut result=Vec::new();
    for value in values {
        let name=value.as_str().context("section 必须为字符串")?;
        ensure!(matches!(name,"profile"|"summary"|"runtime"|"tools"|"skills"|"prompt"),"未知详情分区 {name}");
        if !result.iter().any(|v|v==name) {result.push(name.to_owned())}
    }
    Ok(result)
}

pub(super) fn exchange_tool(conn: &Connection, current: i64, name: &str, args: &Value) -> anyhow::Result<Value> {
    let id=args.get("exchange_id").map(|_|positive(args,"exchange_id")).transpose()?;
    let root=if let Some(id)=id {
        Some(conn.query_row("SELECT root FROM butler_peer_exchange WHERE id=?1 AND (owner=?2 OR target=?2)",params![id,current],|r|r.get::<_,i64>(0))
            .optional()?.context(format!("协作 #{id} 不存在或当前会话 #{current} 未参与"))?)
    } else {None};
    if name=="stop_assistant_exchange" {
        let root=root.context("停止协作需要 exchange_id")?;
        let reason=args["reason"].as_str().map(str::trim).filter(|s|!s.is_empty()&&s.chars().count()<=1000).context("reason 必须为 1 至 1000 字符")?;
        let tx=conn.unchecked_transaction()?;
        crate::db::butler_peer::stop(&tx,root,current)?;
        tx.execute("INSERT OR IGNORE INTO butler_peer_stop(root,conversation_id,reason) VALUES (?1,?2,?3)",params![root,current,reason])?;
        tx.commit()?;
        return Ok(json!({"operation":"stop","root":root,"stopped":true,"execution_cancelled":false,"instruction":"后续协作轮次已停止；正在运行的模型或进程未被强制终止。"}));
    }
    let limit=limit(args)?;
    let before=args.get("before_id").map(|_|positive(args,"before_id")).transpose()?.unwrap_or(i64::MAX);
    let context=args.get("include_context").map(|v|v.as_bool().context("include_context 必须为布尔值")).transpose()?.unwrap_or(false);
    let body=if context {"body,reply"} else {"NULL,NULL"};
    let mut stmt=conn.prepare(&format!("SELECT id,root,owner,target,sender,recipient,state,round_no,max_rounds,stop_requested,error,{body} FROM butler_peer_exchange WHERE (?1 IS NOT NULL AND root=?1 OR ?1 IS NULL AND (owner=?2 OR target=?2)) AND id<?3 ORDER BY id DESC LIMIT ?4"))?;
    let mut items=stmt.query_map(params![root,current,before,limit as i64+1],|r| Ok(json!({
        "exchange_id":r.get::<_,i64>(0)?,"root":r.get::<_,i64>(1)?,"sender_conversation_id":r.get::<_,i64>(2)?,"recipient_conversation_id":r.get::<_,i64>(3)?,
        "sender":r.get::<_,String>(4)?,"recipient":r.get::<_,String>(5)?,"state":r.get::<_,String>(6)?,"round":r.get::<_,i64>(7)?,"max_rounds":r.get::<_,i64>(8)?,"stop_requested":r.get::<_,bool>(9)?,"error":r.get::<_,Option<String>>(10)?,
        "request":r.get::<_,Option<String>>(11)?,"reply":r.get::<_,Option<String>>(12)?,
    })))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let more=items.len()>limit; items.truncate(limit);
    let next=if more {items.last().and_then(|v|v["exchange_id"].as_i64())} else {None};
    for item in &mut items {
        for key in ["request","reply"] {
            if let Some(text)=item[key].as_str() {
                let total=text.chars().count();
                if total>6000 {
                    item[key]=json!(clip(text,6000));
                    item[format!("{key}_truncated")]=json!(true);
                }
            }
        }
    }
    let stop_reason=root.map(|root|conn.query_row("SELECT reason FROM butler_peer_stop WHERE root=?1",[root],|r|r.get::<_,String>(0)).optional()).transpose()?.flatten();
    let user_message=if context {
        root.map(|root|conn.query_row("SELECT content FROM message WHERE id=?1 AND message_type='user'",[root],|r|r.get::<_,String>(0)).optional()).transpose()?.flatten().map(|s|clip(&s,6000))
    } else {None};
    Ok(json!({"exchanges":items,"next_before_id":next,"stop_reason":stop_reason,"user_message":user_message}))
}

#[cfg(test)]
#[path="butler_assistant_tools_tests.rs"]
mod tests;

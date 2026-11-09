//! Group tools derive the room and speaker from the executing conversation.
//! No tool accepts a caller-selected group ID or another member's private session.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use tauri::AppHandle;
use crate::db::{butler_group as group, butler_group_delivery as delivery};
use crate::mcp::builtin_mcp::templates::BuiltinToolInfo;

pub fn is_tool(name:&str) -> bool {
    matches!(name,"group_members"|"group_read"|"group_post"|"group_discuss"|"group_task_start"|"group_tasks"|"group_task_stop")
}

pub fn definitions() -> Vec<BuiltinToolInfo> {
    let mut definitions = vec![
        BuiltinToolInfo { name:"group_members".into(), description:"查看本群有效成员及稳定 member_id。指定 member_id 时返回该成员在本群的实际工具定义；不读取私聊或其他群。工具配置不代表外部服务连接已经验证。".into(), input_schema:json!({"type":"object","properties":{"member_id":{"type":"integer"}}}) },
        BuiltinToolInfo { name:"group_read".into(), description:"补读自己获准的本群公共消息。seq 精确读取；否则 after/limit 分页，可按 query 字面搜索。只返回加入后和用户明确分享的历史，绝不返回其他成员工具过程。消息序号仅在本群有效。".into(), input_schema:json!({"type":"object","properties":{"seq":{"type":"integer","minimum":1},"after":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100},"query":{"type":"string","maxLength":512}}}) },
        BuiltinToolInfo { name:"group_post".into(), description:"以自己身份发布本群消息。默认仅发布进度，不唤醒成员。需要协作时明确指定 recipients 中的 member_id；reply_to 指定原群消息序号，不能绕过读取权限。最终答复由执行器自动公开，不必重复调用。相同请求重试必须沿用 request_key。".into(), input_schema:json!({"type":"object","required":["request_key","body"],"properties":{"request_key":{"type":"string","maxLength":128},"body":{"type":"string"},"recipients":{"type":"array","items":{"type":"integer"}},"reply_to":{"type":"integer"}}}) },
        BuiltinToolInfo { name:"group_discuss".into(), description:"为一条获准的群消息安排一轮顺序意见。members 按发言顺序填写 member_id；后者在轮到时才接收前者发言。每人一个席位，不自动重开或汇总。不能重复安排已有轮次；重试沿用 request_key。".into(), input_schema:json!({"type":"object","required":["request_key","message_seq","members"],"properties":{"request_key":{"type":"string","maxLength":128},"message_seq":{"type":"integer"},"members":{"type":"array","minItems":1,"maxItems":31,"items":{"type":"integer"}}}}) },
    ];
    definitions.extend([
        BuiltinToolInfo {name:"group_task_start".into(),description:"为需要独立上下文或并行执行的工作创建本群任务。member_id 可指定自己；只带任务要求与 source_seqs 明确选择的公开资料，不继承当前会话/私聊的工具历史。结果公开并回送当前执行；创建后可结束本轮等待结果。普通群聊天不要创建任务。重试沿用 request_key。".into(),input_schema:json!({"type":"object","required":["request_key","member_id","title","prompt"],"properties":{"request_key":{"type":"string","maxLength":128},"member_id":{"type":"integer"},"title":{"type":"string","maxLength":200},"prompt":{"type":"string","maxLength":90000},"source_seqs":{"type":"array","maxItems":100,"items":{"type":"integer"}}}})},
        BuiltinToolInfo {name:"group_tasks".into(),description:"查看本群独立任务的标题、执行成员、会话标识和状态。不读取任何任务的私有工具过程。".into(),input_schema:json!({"type":"object","properties":{}})},
        BuiltinToolInfo {name:"group_task_stop".into(),description:"停止当前工作派生的指定独立任务及其子任务。不能停止无关任务；不撤销已发生的外部操作。".into(),input_schema:json!({"type":"object","required":["conversation_id"],"properties":{"conversation_id":{"type":"integer"}}})},
    ]);
    if let Some(post) = definitions.iter_mut().find(|tool|tool.name == "group_post") {
        post.input_schema["properties"]["reference_only"] = json!({"type":"boolean","description":"true 表示仅引用 reply_to 的资料，不请求原作者回应；需要回应仍须指定 recipients。"});
        post.input_schema["properties"]["wait_for_reply"] = json!({"type":"boolean","description":"需要对方结果后继续当前工作时设 true，程序登记回送。派发后结束本轮，可输出 [[NO_REPLY]]；结果回到原群会话并保留原请求。普通进度或无需继续的通知不登记等待。"});
    }
    if let Some(discuss)=definitions.iter_mut().find(|tool|tool.name=="group_discuss") {
        discuss.input_schema["properties"]["mode"]=json!({"enum":["ordered","independent"],"description":"默认 ordered 依次参考前面的意见；independent 固定相同公共快照，各自判断，不读取其他成员后续意见。"});
    }
    definitions
}

fn text<'a>(args:&'a Value,key:&str) -> Result<&'a str> {
    args[key].as_str().filter(|s|!s.trim().is_empty()).with_context(||format!("缺少 {key}"))
}

pub async fn tool(app:&AppHandle,current:i64,name:&str,args:&Value) -> Result<Value> {
    super::butler_api::ensure_butler_enabled(app).await.map_err(anyhow::Error::msg)?;
    ensure!(cfg!(desktop),"协作群工具仅限桌面管家");
    let source = super::butler_peer::TOOL_MESSAGE.try_with(|v|*v).ok().flatten();
    if matches!(name,"group_task_start"|"group_task_stop"|"group_tasks") {
        let me=super::butler_group::database(app,"task_tool_scope",move |conn| {
            delivery::ensure_execution(conn,current,source)?;
            group::scope(conn,current)
        }).await.map_err(anyhow::Error::msg)?;
        let source=source.context("群任务工具缺少原执行消息")?;
        return match name {
            "group_task_start"=> {
                let request=serde_json::from_value::<crate::db::butler_group_execution::TaskRequest>(args.clone())?;
                Ok(json!({"task":super::butler_group::start_task_internal(app,me.group_id,request,Some((current,source))).await.map_err(anyhow::Error::msg)?}))
            },
            "group_task_stop"=>Ok(json!({"stopped":super::butler_group::stop_task_internal(app,me.group_id,args["conversation_id"].as_i64().context("缺少 conversation_id")?,Some((current,source))).await.map_err(anyhow::Error::msg)?})),
            _=> {
                let group=me.group_id;
                Ok(json!({"tasks":super::butler_group::database(app,"task_list",move |conn| {
                    delivery::ensure_execution(conn,current,Some(source))?;
                    crate::db::butler_group_execution::ensure_collaboration_allowed(conn,current)?;
                    crate::db::butler_group_execution::tasks(conn,group)
                }).await.map_err(anyhow::Error::msg)?}))
            }
        };
    }
    // A stop snapshot must not miss a newly registered ordinary delegation or
    // a discussion whose later seats have not entered their inboxes yet.
    let _lifecycle=if matches!(name,"group_post"|"group_discuss") {
        let room=super::butler_group::database(app,"dispatch_tool_scope",move |conn| {
            delivery::ensure_execution(conn,current,source)?;
            Ok(group::scope(conn,current)?.group_id)
        }).await.map_err(anyhow::Error::msg)?;
        Some(super::butler_group::lifecycle(room).await)
    } else {None};
    let name = name.to_owned();
    let args = args.clone();
    let requested_member = args["member_id"].as_i64();
    let result = super::butler_group::database(app,"tool",move |conn| {
        delivery::ensure_execution(conn,current,source)?;
        let me = group::scope(conn,current)?;
        match name.as_str() {
            "group_members" => {
                let members = group::members(conn,me.group_id)?.into_iter().filter(|m|m.active).collect::<Vec<_>>();
                if let Some(id) = requested_member { ensure!(members.iter().any(|m|m.id == id),"成员不属于本群"); }
                Ok(json!({"group_id":me.group_id,"self_member_id":me.id,"members":members}))
            }
            "group_read" => {
                if let Some(seq) = args["seq"].as_i64() { return Ok(json!({"message":crate::db::butler_group_execution::read_public(conn,current,seq)?})); }
                let after = args["after"].as_i64().unwrap_or(0);
                let limit = args["limit"].as_u64().unwrap_or(50).clamp(1,100) as usize;
                let query = args["query"].as_str().unwrap_or("");
                let (messages,upper)=crate::db::butler_group_execution::search_public(conn,current,after,limit,query)?;
                let next=messages.last().map(|event|event.seq).unwrap_or(after);
                Ok(json!({"messages":messages,"next_after":next,"through_seq":upper}))
            }
            "group_post" => {
                let recipients:Vec<i64> = args.get("recipients").cloned().map(serde_json::from_value).transpose()?.unwrap_or_default();
                let post = group::Post { request_key:text(&args,"request_key")?.into(),body:text(&args,"body")?.into(),
                    kind:if recipients.is_empty() { group::MessageKind::Progress } else { group::MessageKind::Message },
                    reply_to:args["reply_to"].as_i64(),reference_only:args["reference_only"].as_bool().unwrap_or(false),recipients };
                let event=if args["wait_for_reply"].as_bool().unwrap_or(false) {
                    crate::db::butler_group_dependency::request(conn,current,source,&post)?
                } else { crate::db::butler_group_dependency::post_without_wait(conn,current,source,&post)? };
                Ok(json!({"message":event,"waiting_registered":args["wait_for_reply"].as_bool().unwrap_or(false)}))
            }
            "group_discuss" => {
                let seq = args["message_seq"].as_i64().context("缺少 message_seq")?;
                group::read_as_member(conn,me.id,seq)?;
                let members:Vec<i64> = serde_json::from_value(args["members"].clone())?;
                // A member cannot recursively queue its own currently running turn.
                ensure!(!members.contains(&me.id),"当前执行成员请直接给出本轮意见，不要把自己重新排入队列");
                let key = if me.task {
                    format!("execution:{}:{}",me.conversation_id,text(&args,"request_key")?)
                } else {
                    format!("{}:{}",me.id,text(&args,"request_key")?)
                };
                if args["mode"].as_str()==Some("independent") {
                    let id=delivery::start_agent_discussion(conn,current,source,seq,&members,&key,true)?;
                    Ok(json!({"round":{"id":id,"group_id":me.group_id,"mode":"independent"}}))
                } else {
                    ensure!(args["mode"].is_null() || args["mode"]=="ordered","未知讨论模式");
                    let id=delivery::start_agent_discussion(conn,current,source,seq,&members,&key,false)?;
                    Ok(json!({"round":{"id":id,"group_id":me.group_id,"mode":"ordered"}}))
                }
            }
            _ => anyhow::bail!("未知群工具：{name}"),
        }
    }).await.map_err(anyhow::Error::msg)?;
    if let Some(group_id) = result["message"]["group_id"].as_i64().or_else(||result["round"]["group_id"].as_i64()) {
        super::butler_group::changed(app,group_id);
    }
    if let Some(id) = requested_member.filter(|_|name_is_members(&result)) {
        let member = result["members"].as_array().and_then(|members|members.iter().find(|m|m["id"].as_i64()==Some(id)))
            .context("群成员已变化")?;
        let assistant = member["assistant_id"].as_i64().context("成员缺少助手身份")?;
        let conversation = member["conversation_id"].as_i64().context("成员缺少独立群会话")?;
        let info = crate::mcp::butler_tools::collect_mcp_info_for_conversation(app,assistant,None,None,Some(conversation)).await?;
        return Ok(json!({"member":member,"tools":info.enabled_servers,"connectivity_verified":false}));
    }
    Ok(result)
}

fn name_is_members(result:&Value) -> bool { result.get("members").is_some() }

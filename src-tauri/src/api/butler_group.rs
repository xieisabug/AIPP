//! Local-user commands for group management. Agent tools must resolve scope from
//! their execution session and must not call these unrestricted UI read methods.
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use crate::db::{assistant_db::AssistantDatabase, conversation_db::ConversationDatabase,
    butler_group as group, butler_group_delivery as delivery,butler_group_execution as execution};

// Serialize creation with lifecycle snapshots. A stop can then acquire all
// affected execution admissions without missing a concurrently spawned child.
pub(super) async fn lifecycle(group:i64)->tokio::sync::OwnedMutexGuard<()> {
    static GATES:std::sync::OnceLock<Vec<std::sync::Arc<tokio::sync::Mutex<()>>>>=std::sync::OnceLock::new();
    let gates=GATES.get_or_init(||(0..64).map(|_|std::sync::Arc::new(tokio::sync::Mutex::new(()))).collect());
    gates[group.unsigned_abs() as usize % gates.len()].clone().lock_owned().await
}

fn desktop() -> Result<(),String> {
    if !cfg!(desktop) { return Err("协作群仅在桌面管家中提供".into()); }
    Ok(())
}

async fn enabled(app:&AppHandle) -> Result<(),String> {
    desktop()?;
    super::butler_api::ensure_butler_enabled(app).await
}

pub(super) async fn database<T,F>(app:&AppHandle,operation:&'static str,work:F) -> Result<T,String>
where T:Send+'static, F:FnOnce(&mut rusqlite::Connection)->anyhow::Result<T>+Send+'static {
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let db = ConversationDatabase::new(&app)?;
        let mut conn = db.get_connection()?;
        work(&mut conn)
    }).await.map_err(|e|format!("协作群 {operation} 调度失败：{e}"))?
        .map_err(|e|format!("协作群 {operation} 失败：{e:#}"))
}

pub(super) fn changed(app:&AppHandle,group_id:i64) {
    let _ = app.emit("butler_group_changed",serde_json::json!({"group_id":group_id}));
    super::butler_group_runtime::wake();
}

pub(super) async fn refresh_capabilities(app:&AppHandle,group_id:i64)->Result<Vec<(i64,String)>,String> {
    use crate::db::butler_group_capability::{self,Profile};
    let members=database(app,"capability_roster",move |conn|group::members(conn,group_id)).await?;
    let mut errors=Vec::new();
    for member in members.into_iter().filter(|m|m.active) {
        let profile=match capability_profile(app,&member).await {
            Ok(profile)=>profile,
            Err(error)=> {
                errors.push((member.id,error.clone()));
                Profile {dynamic_loading:false,native_toolcall:false,servers:vec![],error:Some(error)}
            }
        };
        let id=member.id;
        match database(app,"capability_record",move |conn|butler_group_capability::record(conn,id,profile)).await {
            Ok(true)=>changed(app,group_id),
            Ok(false)=>{},
            // Membership may change between observation and persistence. Keep
            // that error local instead of aborting the rest of the roster.
            Err(error)=>errors.push((id,error)),
        }
    }
    Ok(errors)
}

async fn capability_profile(app:&AppHandle,member:&group::Member)->Result<crate::db::butler_group_capability::Profile,String> {
        use crate::db::butler_group_capability::{Profile,Server,Tool};
        let group_id=member.group_id;
        let info=crate::mcp::butler_tools::collect_mcp_info_for_conversation(app,member.assistant_id,None,None,Some(member.conversation_id))
            .await.map_err(|error|format!("群工具能力读取失败：group={group_id}; member={}; assistant={}; {error}",member.id,member.assistant_id))?;
        let servers=info.enabled_servers.into_iter().filter(|s|s.is_enabled).map(|s| {
            let tools=s.tools.into_iter().filter(|t|t.is_enabled).map(|t| {
                let parameters=serde_json::from_str(&t.parameters).map_err(|error|format!("群工具定义无效：member={}; server={}; tool={}; {error}",member.id,s.id,t.name))?;
                Ok(Tool {name:t.name,description:t.description,auto_run:t.is_auto_run,parameters})
            }).collect::<Result<Vec<_>,String>>()?;
            Ok(Server{id:s.id,name:s.name,summary:s.summary,tools})
        }).collect::<Result<Vec<_>,String>>()?;
        Ok(Profile {dynamic_loading:info.dynamic_loading_enabled,native_toolcall:info.use_native_toolcall,servers,error:None})
}

#[tauri::command]
pub async fn create_butler_group(app_handle:AppHandle,name:String,goal:String,assistant_ids:Vec<i64>) -> Result<group::GroupView,String> {
    enabled(&app_handle).await?;
    let chief = super::butler_api::load_or_create_butler_main_internal(&app_handle).await?
        .assistant_id.ok_or("总管家主会话未绑定助手")?;
    let chief_name = super::butler_peer::bot_names(&app_handle).await.map_err(|e|e.to_string())?
        .remove(&chief).ok_or("总管家助手配置不存在")?;
    let app = app_handle.clone();
    let result = database(&app_handle,"create",move |conn| {
        let db = AssistantDatabase::new(&app)?;
        let mut invited = Vec::new();
        for id in assistant_ids {
            anyhow::ensure!(id != chief,"总管家已自动加入，无需重复选择");
            anyhow::ensure!(!invited.iter().any(|(existing,_)|*existing == id),"重复选择了助手 #{id}");
            let assistant = db.get_assistant(id)?;
            invited.push((id,assistant.name));
        }
        group::create(conn,&name,&goal,(chief,chief_name),&invited)
    }).await?;
    changed(&app_handle,result.group.id);
    Ok(result)
}

#[tauri::command]
pub async fn list_butler_groups(app_handle:AppHandle) -> Result<Vec<group::Group>,String> {
    desktop()?;
    database(&app_handle,"list",|conn|group::list(conn)).await
}

#[derive(Serialize)]
pub struct GroupDetail {
    pub group: group::Group,
    pub members: Vec<group::Member>,
    pub inbox: Vec<delivery::InboxItem>,
    pub rounds: Vec<delivery::Round>,
    pub stop_reports: Vec<StopResult>,
    pub tasks:Vec<execution::TaskView>,
}

#[tauri::command]
pub async fn get_butler_group(app_handle:AppHandle,group_id:i64) -> Result<GroupDetail,String> {
    desktop()?;
    database(&app_handle,"get",move |conn| {
        let view = group::view(conn,group_id)?;
        let mut stmt = conn.prepare("SELECT r.member_id,coalesce(r.conversation_id,m.conversation_id),r.succeeded,r.detail FROM butler_group_stop_report r
            JOIN butler_group_member m ON m.id=r.member_id WHERE r.group_id=?1 ORDER BY r.id")?;
        let reports = stmt.query_map([group_id],|r|Ok(StopResult { member_id:r.get(0)?,conversation_id:r.get(1)?,succeeded:r.get(2)?,detail:r.get(3)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(GroupDetail { group:view.group,members:view.members,inbox:delivery::inbox(conn,group_id)?,
            rounds:delivery::rounds(conn,group_id)?,stop_reports:reports,tasks:execution::tasks(conn,group_id)? })
    }).await
}

#[tauri::command]
pub async fn list_butler_group_messages(app_handle:AppHandle,group_id:i64,after:Option<i64>,limit:Option<usize>) -> Result<Vec<group::Event>,String> {
    desktop()?;
    database(&app_handle,"messages",move |conn|group::events(conn,group_id,after.unwrap_or(0),limit.unwrap_or(100))).await
}

#[derive(Deserialize)]
pub struct UserPost {
    pub request_key:String,
    pub body:String,
    pub reply_to:Option<i64>,
    #[serde(default)]
    pub reference_only:bool,
    #[serde(default)]
    pub recipients:Vec<i64>,
    #[serde(default)]
    pub ordered_members:Vec<i64>,
    #[serde(default)]
    pub independent_members:Vec<i64>,
}

#[tauri::command]
pub async fn post_butler_group_message(app_handle:AppHandle,group_id:i64,message:UserPost) -> Result<group::Event,String> {
    enabled(&app_handle).await?;
    // Record the current roster before fixing an independent discussion's
    // shared upper bound. Later capability changes stay outside that snapshot.
    refresh_capabilities(&app_handle,group_id).await?;
    let result = database(&app_handle,"post",move |conn| {
        let post = group::Post {
        request_key:message.request_key,kind:group::MessageKind::Message,body:message.body,
        reply_to:message.reply_to,reference_only:message.reference_only,recipients:message.recipients,
        };
        anyhow::ensure!(message.ordered_members.is_empty() || message.independent_members.is_empty(),"不能同时安排顺序讨论和独立判断");
        if !message.independent_members.is_empty() { delivery::post_independent(conn,group_id,&post,&message.independent_members) }
        else if !message.ordered_members.is_empty() { delivery::post_round(conn,group_id,&post,&message.ordered_members) }
        else { group::post(conn,group_id,None,&post) }
    }).await?;
    changed(&app_handle,group_id);
    Ok(result)
}

#[tauri::command]
pub async fn add_butler_group_member(app_handle:AppHandle,group_id:i64,assistant_id:i64) -> Result<group::Member,String> {
    enabled(&app_handle).await?;
    let _lifecycle = lifecycle(group_id).await;
    let app = app_handle.clone();
    let result = database(&app_handle,"add_member",move |conn| {
        let assistant = AssistantDatabase::new(&app)?.get_assistant(assistant_id)?;
        group::add_member(conn,group_id,assistant_id,&assistant.name)
    }).await?;
    changed(&app_handle,group_id);
    Ok(result)
}

#[tauri::command]
pub async fn share_butler_group_history(app_handle:AppHandle,group_id:i64,member_id:i64,message_seqs:Vec<i64>,request_key:String) -> Result<(),String> {
    enabled(&app_handle).await?;
    database(&app_handle,"share",move |conn|group::share(conn,group_id,member_id,&message_seqs,&request_key)).await?;
    changed(&app_handle,group_id);
    Ok(())
}

#[tauri::command]
pub async fn start_butler_group_round(app_handle:AppHandle,group_id:i64,message_seq:i64,member_ids:Vec<i64>,request_key:String) -> Result<delivery::Round,String> {
    enabled(&app_handle).await?;
    let result = database(&app_handle,"start_round",move |conn|delivery::start_round(conn,group_id,message_seq,&member_ids,&request_key)).await?;
    changed(&app_handle,group_id);
    Ok(result)
}

#[derive(Serialize)]
pub struct StopResult {
    pub member_id:i64,
    pub conversation_id:i64,
    pub succeeded:bool,
    pub detail:String,
}

async fn halt(app:&AppHandle,group_id:i64,m:execution::Scope) -> StopResult {
    let (succeeded,mut detail) = match super::butler_peer::halt_turn(app,m.conversation_id,m.name.clone()).await {
        Ok(report) => (true,report.describe()),
        Err(error) => (false,format!("stage=group_halt; group_id={group_id}; member_id={}; conversation_id={}; {error:#}",m.id,m.conversation_id)),
    };
    let saved_detail = detail.clone();
    let member_id = m.id;
    let conversation=m.conversation_id;
    if let Err(error) = database(app,"save_stop_report",move |conn| {
        conn.execute("INSERT INTO butler_group_stop_report(group_id,member_id,succeeded,detail,conversation_id) VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![group_id,member_id,succeeded,saved_detail,conversation])?;
        Ok(())
    }).await { detail.push_str(&format!("；停止结果持久化失败：{error}")); }
    StopResult { member_id:m.id,conversation_id:m.conversation_id,succeeded,detail }
}

pub(super) async fn start_task_internal(app:&AppHandle,group_id:i64,request:execution::TaskRequest,owner:Option<(i64,i64)>)->Result<execution::Scope,String> {
    let _lifecycle=lifecycle(group_id).await;
    let result=database(app,"start_task",move |conn|execution::start_task(conn,group_id,&request,owner)).await?;
    changed(app,group_id); Ok(result)
}

#[tauri::command]
pub async fn start_butler_group_task(app_handle:AppHandle,group_id:i64,request:execution::TaskRequest)->Result<execution::Scope,String> {
    enabled(&app_handle).await?;
    start_task_internal(&app_handle,group_id,request,None).await
}

pub(super) async fn stop_task_internal(app:&AppHandle,group_id:i64,conversation:i64,owner:Option<(i64,i64)>)->Result<Vec<StopResult>,String> {
    let _lifecycle=lifecycle(group_id).await;
    let scopes=database(app,"task_stop_scope",move |conn| {
        let target=execution::get(conn,conversation)?;
        anyhow::ensure!(target.group_id==group_id && target.task,"不是本群独立任务");
        if let Some((current,source))=owner {
            delivery::ensure_execution(conn,current,Some(source))?;
            anyhow::ensure!(crate::db::butler_group_stop::owns_task(conn,current,conversation)?,"只能停止当前工作派生的独立任务");
            anyhow::ensure!(current!=conversation,"不能通过工具停止自己的原执行，请向用户交付结果或说明阻塞");
        }
        // Lock every existing execution in this group before taking the final
        // packet-level stop snapshot. New tasks are blocked by lifecycle.
        execution::list(conn,group_id)
    }).await?;
    let ids=scopes.iter().map(|e|e.conversation_id).collect::<Vec<_>>();
    let _admission=super::butler_peer::admit_turns(&ids).await;
    let scopes=database(app,"stop_task",move |conn|execution::stop_task(conn,group_id,conversation)).await?;
    let mut results=Vec::new();
    for e in scopes {results.push(halt(app,group_id,e).await);}
    changed(app,group_id); Ok(results)
}

#[tauri::command]
pub async fn stop_butler_group_task(app_handle:AppHandle,group_id:i64,conversation_id:i64)->Result<Vec<StopResult>,String> {
    desktop()?;
    stop_task_internal(&app_handle,group_id,conversation_id,None).await
}

async fn stop_member_internal(app:&AppHandle,group_id:i64,member_id:i64,remove:bool)->Result<StopResult,String> {
    let _lifecycle=lifecycle(group_id).await;
    let scopes=database(app,"member_stop_scope",move |conn| {
        let member=group::member(conn,member_id)?;
        anyhow::ensure!(member.group_id==group_id,"成员不属于该群");
        execution::list(conn,group_id)
    }).await?;
    let ids=scopes.iter().map(|e|e.conversation_id).collect::<Vec<_>>();
    let _admission=super::butler_peer::admit_turns(&ids).await;
    let (member,scopes)=database(app,"member_stop",move |conn|if remove {group::remove_member_with_stopped(conn,group_id,member_id)} else {group::stop_member_with_stopped(conn,group_id,member_id)}).await?;
    let mut reports=Vec::new();
    for e in scopes {reports.push(halt(app,group_id,e).await);}
    let result=StopResult {member_id,conversation_id:member.conversation_id,succeeded:reports.iter().all(|r|r.succeeded),
        detail:reports.iter().map(|r|format!("会话 #{}：{}",r.conversation_id,r.detail)).collect::<Vec<_>>().join("\n")};
    changed(app,group_id); Ok(result)
}

#[tauri::command]
pub async fn remove_butler_group_member(app_handle:AppHandle,group_id:i64,member_id:i64) -> Result<StopResult,String> {
    desktop()?;
    stop_member_internal(&app_handle,group_id,member_id,true).await
}

#[tauri::command]
pub async fn stop_butler_group(app_handle:AppHandle,group_id:i64) -> Result<Vec<StopResult>,String> {
    desktop()?;
    let _lifecycle=lifecycle(group_id).await;
    let scopes=database(&app_handle,"group_stop_scope",move |conn|execution::list(conn,group_id)).await?;
    let ids=scopes.iter().map(|e|e.conversation_id).collect::<Vec<_>>();
    let _admission=super::butler_peer::admit_turns(&ids).await;
    database(&app_handle,"stop",move |conn|group::stop(conn,group_id)).await?;
    let mut results=Vec::new();
    for e in scopes {results.push(halt(&app_handle,group_id,e).await);}
    changed(&app_handle,group_id);
    Ok(results)
}

#[tauri::command]
pub async fn stop_butler_group_member(app_handle:AppHandle,group_id:i64,member_id:i64) -> Result<StopResult,String> {
    desktop()?;
    stop_member_internal(&app_handle,group_id,member_id,false).await
}

#[tauri::command]
pub async fn replace_butler_group_goal(app_handle:AppHandle,group_id:i64,goal:String,request_key:String) -> Result<Vec<StopResult>,String> {
    enabled(&app_handle).await?;
    let _lifecycle=lifecycle(group_id).await;
    let scopes=database(&app_handle,"goal_executions",move |conn|execution::list(conn,group_id)).await?;
    let members=database(&app_handle,"goal_members",move |conn|Ok(group::members(conn,group_id)?.into_iter().filter(|m|m.active).collect::<Vec<_>>())).await?;
    let ids=members.iter().map(|m|m.id).collect::<Vec<_>>();
    let conversations=scopes.iter().map(|m|m.conversation_id).collect::<Vec<_>>();
    let _admission=super::butler_peer::admit_turns(&conversations).await;
    let replaced=database(&app_handle,"replace_goal",move |conn|group::replace_goal(conn,group_id,&goal,&request_key,&ids)).await?;
    let mut results=Vec::new();
    if replaced {
        for e in scopes { results.push(halt(&app_handle,group_id,e).await); }
    }
    changed(&app_handle,group_id);
    Ok(results)
}

#[derive(Serialize)]
pub struct GroupCapabilities {
    member_id:i64,
    dynamic_loading:bool,
    connectivity_verified:bool,
    servers:Vec<crate::api::assistant_api::MCPServerWithTools>,
}

#[tauri::command]
pub async fn set_butler_group_wait_limit(app_handle:AppHandle,group_id:i64,seconds:i64,request_key:String) -> Result<(),String> {
    enabled(&app_handle).await?;
    database(&app_handle,"wait_limit",move |conn|group::set_wait_limit(conn,group_id,seconds,&request_key)).await?;
    changed(&app_handle,group_id); Ok(())
}

#[tauri::command]
pub async fn get_butler_group_member_capabilities(app_handle:AppHandle,group_id:i64,member_id:i64) -> Result<GroupCapabilities,String> {
    desktop()?;
    let member=database(&app_handle,"capability_member",move |conn| {
        let member=group::member(conn,member_id)?;
        anyhow::ensure!(member.group_id==group_id,"成员不属于该群");
        Ok(member)
    }).await?;
    let info=crate::mcp::butler_tools::collect_mcp_info_for_conversation(&app_handle,member.assistant_id,None,None,Some(member.conversation_id)).await.map_err(|e|e.to_string())?;
    Ok(GroupCapabilities { member_id,dynamic_loading:info.dynamic_loading_enabled,connectivity_verified:false,servers:info.enabled_servers })
}

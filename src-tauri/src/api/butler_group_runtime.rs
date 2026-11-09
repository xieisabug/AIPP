//! Durable group dispatch. Members run independently; each member still enters
//! ask_ai through its shared cancellation/admission gate and original channel.
use std::{collections::HashSet,sync::{Arc,Mutex},time::Duration};
use anyhow::Result;
use tauri::{AppHandle,Manager};
use crate::db::{butler_group_delivery as delivery,butler_group_execution as execution};
use super::butler_group::{database,changed};

static WAKE:tokio::sync::Notify = tokio::sync::Notify::const_new();
pub fn wake() { WAKE.notify_one(); }

// Release admission even when a spawned dispatcher is aborted or panics. A
// claimed packet remains durably fenced; removing this flag never replays it.
struct InflightGuard { members:Arc<Mutex<HashSet<i64>>>, member:i64 }
impl Drop for InflightGuard {
    fn drop(&mut self) {
        if let Ok(mut members)=self.members.lock() { members.remove(&self.member); }
        wake();
    }
}

async fn dispatch(app:&AppHandle,m:execution::Scope,p:delivery::Packet) -> Result<()> {
    let window = super::butler_api::resolve_or_create_butler_execution_window(app).map_err(anyhow::Error::msg)?;
    let result = super::ai_api::ask_ai(app.clone(),app.state(),app.state(),app.state(),app.state(),app.state(),window,
        super::ai::types::AiRequest { conversation_id:m.conversation_id.to_string(),assistant_id:m.assistant_id,prompt:p.payload,
            model:None,override_model_id:None,temperature:None,top_p:None,max_tokens:None,stream:Some(true),
            attachment_list:None,quote:None,butler_intent:None,butler_goal_revision:None },
        None,None,None,None,Some(format!("butler_group/{}",p.id))).await?;
    if let Some(error) = result.error_message { anyhow::bail!(error); }
    Ok(())
}

async fn waiting(app:&AppHandle,conversation:i64) -> bool {
    !app.state::<crate::mcp::builtin_mcp::OperationState>().list_permission_requests_for_conversation(conversation).await.is_empty()
        || !app.state::<super::ai::acp::AcpPermissionState>().list_requests_for_conversation(conversation).await.is_empty()
        || app.state::<super::ai::acp::AcpElicitationState>().has_pending_request_for_conversation(conversation).await
        || !app.state::<crate::mcp::builtin_mcp::interaction::InteractionState>().list_requests_for_conversation(conversation).await.is_empty()
}

async fn tick(app:&AppHandle,inflight:&Arc<Mutex<HashSet<i64>>>,dispatch_enabled:bool) -> Result<()> {
    let members = database(app,"dispatch_members",|conn| {
        let mut stmt = conn.prepare("SELECT e.conversation_id FROM butler_group_execution e
            JOIN butler_group_member m ON m.id=e.member_id JOIN butler_group g ON g.id=m.group_id
            WHERE g.state='active' AND m.active=1 AND e.state='active' AND EXISTS(SELECT 1 FROM butler_group_inbox i
                WHERE i.conversation_id=e.conversation_id AND i.state IN ('queued','prepared','delivered','waiting')) ORDER BY e.conversation_id")?;
        let ids=stmt.query_map([],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ids.into_iter().map(|id|execution::get(conn,id)).collect::<Result<Vec<_>>>()
    }).await.map_err(anyhow::Error::msg)?;
    let mut observed=HashSet::new();
    let mut capability_errors=std::collections::HashMap::new();
    for m in &members {
        if dispatch_enabled && observed.insert(m.group_id) {
            match super::butler_group::refresh_capabilities(app,m.group_id).await {
                Ok(errors)=>capability_errors.extend(errors),
                Err(error)=> {
                    for member in members.iter().filter(|member|member.group_id==m.group_id) {
                        capability_errors.insert(member.id,error.clone());
                    }
                }
            }
        }
    }
    for m in members {
        let member_id=m.id;
        if let Err(error)=tick_member(app,inflight,m,capability_errors.get(&member_id).cloned(),dispatch_enabled).await {
            tracing::error!(member_id,error=%format!("{error:#}"),"group member tick failed; other members continue");
        }
    }
    Ok(())
}

async fn tick_member(app:&AppHandle,inflight:&Arc<Mutex<HashSet<i64>>>,m:execution::Scope,capability_error:Option<String>,dispatch_enabled:bool) -> Result<()> {
        use rusqlite::OptionalExtension;
        let dispatch_inflight=inflight.lock().map_err(|_|anyhow::anyhow!("群调度执行集合锁损坏"))?.contains(&m.conversation_id);
        let member_id = m.id;
        let conversation=m.conversation_id;
        let active = database(app,"active_packet",move |conn| {
            let id:Option<i64> = conn.query_row("SELECT id FROM butler_group_packet WHERE conversation_id=?1 AND state IN ('prepared','delivered','running','uncertain')",
                [conversation],|r|r.get(0)).optional()?;
            id.map(|id|delivery::packet(conn,id)).transpose()
        }).await.map_err(anyhow::Error::msg)?;
        // ask_ai may remain awaiting user input for the entire native prompt.
        // Admission stays fenced, but its running packet must still be observed
        // so a question/approval can yield an ordered seat immediately.
        if dispatch_inflight && !active.as_ref().is_some_and(|packet|packet.state=="running") {return Ok(());}
        if let Some(p) = active.as_ref().filter(|p|p.state != "prepared") {
            if p.state != "running" { return Ok(()); }
            let binding=super::ai::agent_driver::lifecycle::session_binding(app,conversation)?;
            let native=if binding.is_some() {super::ai::agent_driver::lifecycle::lookup_session(app,conversation).await?} else {None};
            let native_active=native.as_ref().is_some_and(|session|session.snapshot["has_active_prompt"].as_bool()==Some(true));
            // A missing authoritative actor takes precedence over stale UI or
            // MCP activity. Do not clear those records or start another run:
            // settle from persisted evidence below, otherwise fence uncertain.
            let native_missing=binding.filter(|_|native.is_none());
            let busy = dispatch_inflight || (native_missing.is_none()
                && (native_active || super::butler_peer::busy(app,m.conversation_id).await?));
            if busy {
                let needs_yield = waiting(app,m.conversation_id).await;
                let id = p.id;
                let did_change=database(app,"yield_seat",move |conn| {
                    let overdue:bool = conn.query_row("SELECT unixepoch()-unixepoch(p.created_at)>=g.seat_wait_seconds FROM butler_group_packet p
                        JOIN butler_group_member m ON m.id=p.member_id JOIN butler_group g ON g.id=m.group_id WHERE p.id=?1",[id],|r|r.get(0))?;
                    if needs_yield || overdue {return delivery::waiting(conn,id,if needs_yield {"等待用户回答或批准"} else {"工作仍在执行，已超过本轮席位等待时间"});}
                    Ok(false)
                }).await.map_err(anyhow::Error::msg)?;
                if did_change {changed(app,m.group_id);}
                return Ok(());
            }
            let id = p.id;
            let conversation = m.conversation_id;
            let did_change=database(app,"settle",move |conn| {
                let p = delivery::packet(conn,id)?;
                if p.state != "running" { return Ok(false); }
                use crate::db::butler_group_completion::{evidence,Evidence};
                match evidence(conn,id)? {
                    Evidence::Answer(message)|Evidence::Silent(message)
                        if crate::db::butler_group_dependency::has_obligations(conn,id)? => {
                            delivery::ensure_execution(conn,conversation,Some(message))?;
                            crate::db::butler_group_dependency::suspend(conn,id,message)?;
                        }
                    Evidence::Answer(message) => {
                        if let Err(error)=delivery::publish_final(conn,id,message) {
                            delivery::fail(conn,id,&format!("stage=group_publish; conversation={conversation}; packet={id}; message={message}; {error:#}"))?;
                        }
                    }
                    Evidence::Silent(message)=>delivery::finish_silent(conn,id,message)?,
                    Evidence::Failed(reason)=>delivery::fail(conn,id,&reason)?,
                    Evidence::Pending=> {
                        if let Some(binding)=native_missing {
                            delivery::uncertain(conn,id,&format!("stage=member_completion; conversation={conversation}; packet={id}; driver={}; provider={}; run={}; 原绑定运行实例已不存在且缺少该输入的结束记录，无法确认工具结果；未自动重试",binding.driver_id,binding.provider_id,binding.run_id))?;
                        } else {
                            let overdue:bool=conn.query_row("SELECT unixepoch()-unixepoch(p.created_at)>=g.seat_wait_seconds FROM butler_group_packet p
                                JOIN butler_group_member m ON m.id=p.member_id JOIN butler_group g ON g.id=m.group_id WHERE p.id=?1",[id],|r|r.get(0))?;
                            return if overdue {delivery::waiting(conn,id,"等待原执行的持久化结束记录，尚不能确认结果；未自动重试")} else {Ok(false)};
                        }
                    },
                }
                Ok(true)
            }).await.map_err(anyhow::Error::msg)?;
            if did_change {changed(app,m.group_id);}
            return Ok(());
        }
        // Disabling Butler closes new admission, not observation of an already
        // running packet. Preserve queued/prepared work until explicitly enabled.
        if !dispatch_enabled {return Ok(());}
        if super::butler_peer::busy(app,m.conversation_id).await? { return Ok(()); }
        let group_id=m.group_id;
        let conversation=m.conversation_id;
        let (prepared,summary,failed) = database(app,"prepare",move |conn| {
            if let (Some(error),Some(packet))=(&capability_error,&active) {
                delivery::fail(conn,packet.id,&format!("stage=group_capability; conversation={conversation}; packet={}; {error}",packet.id))?;
                return Ok((None,None,true));
            }
            let queued=delivery::inbox(conn,group_id)?.into_iter()
                .filter(|i|i.conversation_id==conversation && i.state=="queued").collect::<Vec<_>>();
            let selected=queued.first().map(|first|queued.iter()
                .take_while(|i|i.round_id==first.round_id && i.snapshot_seq==first.snapshot_seq)
                .cloned().collect::<Vec<_>>()).unwrap_or_default();
            let preparation=match capability_error {
                Some(error)=>Err(anyhow::anyhow!("stage=group_capability; {error}")),
                None=>delivery::prepare_execution(conn,conversation),
            };
            match preparation {
                Ok(packet)=>Ok((packet,None,false)),
                Err(error)=> {
                    if let Some(required)=error.downcast_ref::<delivery::SummaryRequired>() {
                        return Ok((None,Some(required.0),false));
                    }
                    let diagnostic=format!("stage=group_prepare; group={group_id}; member={member_id}; conversation={conversation}; {error:#}");
                    delivery::fail_preparation(conn,conversation,&selected,&diagnostic)?;
                    tracing::error!(%diagnostic,"group preparation failed; no automatic retry");
                    Ok((None,None,true))
                }
            }
        }).await.map_err(anyhow::Error::msg)?;
        if failed { changed(app,group_id); }
        if prepared.is_none() && summary.is_none() { return Ok(()); }
        inflight.lock().map_err(|_|anyhow::anyhow!("群调度执行集合锁损坏"))?.insert(m.conversation_id);
        let app = app.clone();
        let inflight = inflight.clone();
        tauri::async_runtime::spawn(async move {
            let _guard=InflightGuard { members:inflight,member:conversation };
            let group_id = m.group_id;
            let conversation = m.conversation_id;
            let p=match (prepared,summary) {
                (Some(p),_)=>p,
                (_,Some(id))=>match super::butler_group_summary::prepare(&app,id).await {
                    Ok(p)=>p,
                    Err(error)=> {
                        let diagnostic=format!("stage=group_background_summary; group={group_id}; member={member_id}; conversation={conversation}; preparation={id}; {error:#}");
                        if let Err(save_error)=database(&app,"summary_failure",move |conn|delivery::fail_summary(conn,id,&diagnostic)).await {
                            tracing::error!(group_id,member_id,preparation=id,error=%save_error,"failed to persist group summary failure");
                        }
                        changed(&app,group_id);
                        return;
                    }
                },
                _=>return,
            };
            let packet = p.id;
            // A summary may finish after the user disables Butler. Keep its
            // frozen prepared packet, but do not open a new model turn.
            if super::butler_api::ensure_butler_enabled(&app).await.is_err() {
                changed(&app,group_id);
                return;
            }
            if let Err(error) = dispatch(&app,m,p).await {
                let diagnostic = format!("stage=group_dispatch; group={group_id}; member={member_id}; conversation={conversation}; packet={packet}; {error:#}");
                if let Err(save_error) = database(&app,"dispatch_failure",move |conn|delivery::fail(conn,packet,&diagnostic)).await {
                    tracing::error!(group_id,member_id,packet,error=%save_error,"failed to persist group dispatch failure");
                }
            }
            changed(&app,group_id);
        });
    Ok(())
}

pub fn start(app:AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = database(&app,"recover",delivery::recover).await {
            tracing::error!(%error,"group recovery failed; dispatcher stopped");
            return;
        }
        let inflight = Arc::new(Mutex::new(HashSet::new()));
        loop {
            let dispatch_enabled=super::butler_api::ensure_butler_enabled(&app).await.is_ok();
            if let Err(error) = tick(&app,&inflight,dispatch_enabled).await { tracing::error!(error=%format!("{error:#}"),"group dispatch tick failed"); }
            tokio::select! { _=WAKE.notified()=>{}, _=tokio::time::sleep(Duration::from_secs(2))=>{} }
        }
    });
}

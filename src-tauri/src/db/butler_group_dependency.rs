//! Explicit response obligations. Public results do not wake observers; only
//! a registered waiter receives a continuation, with its original request.
use anyhow::{ensure,Result};
use rusqlite::{params,Connection,OptionalExtension,TransactionBehavior};
use super::{butler_group as group,butler_group_delivery as delivery,butler_group_execution as execution};

pub fn post_without_wait(conn:&mut Connection,current:i64,source:Option<i64>,post:&group::Post) -> Result<group::Event> {
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    delivery::ensure_execution(&tx,current,source)?;
    let me=group::scope(&tx,current)?;
    let scoped_post=execution::namespace_post(&me,post)?; let post=&scoped_post;
    let registered:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_event e
        JOIN butler_group_dependency d ON d.request_seq=e.seq
        JOIN butler_group_packet p ON p.id=d.owner_packet
        WHERE e.group_id=?1 AND e.request_key=?2 AND p.member_id=?3)",
        params![me.group_id,format!("{}:{}",me.id,post.request_key),me.id],|r|r.get(0))?;
    ensure!(!registered,"请求标识已登记等待协作，重试不能改为普通消息");
    let event=execution::post_in(&tx,&me,post)?;
    tx.commit()?;
    Ok(event)
}

pub fn request(conn:&mut Connection,current:i64,source:Option<i64>,post:&group::Post) -> Result<group::Event> {
    ensure!(!post.recipients.is_empty(),"等待协作结果必须指定接收成员");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    delivery::ensure_execution(&tx,current,source)?;
    let me=group::scope(&tx,current)?;
    let scoped_post=execution::namespace_post(&me,post)?; let post=&scoped_post;
    let packet:i64=tx.query_row("SELECT id FROM butler_group_packet WHERE conversation_id=?1 AND state='running'",[current],|r|r.get(0))?;
    let previous:Option<i64>=tx.query_row("SELECT seq FROM butler_group_event WHERE group_id=?1 AND request_key=?2",
        params![me.group_id,format!("{}:{}",me.id,post.request_key)],|r|r.get(0)).optional()?;
    if let Some(seq)=previous {
        // A plain post cannot become a blocking delegation on retry, nor can a
        // later turn adopt another turn's response obligations.
        let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_dependency WHERE owner_packet=?1 AND request_seq=?2)",
            params![packet,seq],|r|r.get(0))?;
        ensure!(owned,"请求标识已用于普通消息或其他执行，不能改为本轮等待协作");
        let event=execution::post_in(&tx,&me,post)?;
        tx.commit()?;
        return Ok(event);
    }
    super::butler_group_chain::admit(&tx,packet,post.recipients.len())?;
    // Assistant identity survives leave/rejoin membership epochs. Resumptions
    // preserve ancestry but do not consume an additional delegation depth.
    super::butler_group_chain::ensure_recipients(&tx,packet,&post.recipients)?;
    let event=execution::post_in(&tx,&me,post)?;
    let existing:i64=tx.query_row("SELECT count(*) FROM butler_group_dependency WHERE owner_packet=?1 AND request_seq!=?2",params![packet,event.seq],|r|r.get(0))?;
    ensure!(existing + post.recipients.len() as i64 <= 32,"本轮协作接收者累计不能超过 32 个");
    for target in &post.recipients {
        let target_conversation:i64=tx.query_row("SELECT conversation_id FROM butler_group_inbox WHERE group_id=?1 AND member_id=?2 AND event_seq=?3",
            params![me.group_id,target,event.seq],|r|r.get(0))?;
        tx.execute("INSERT INTO butler_group_dependency(owner_packet,target_member,request_seq,target_conversation_id) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![packet,target,event.seq,target_conversation])?;
    }
    tx.commit()?;
    Ok(event)
}

pub fn has_obligations(conn:&Connection,packet:i64) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_dependency WHERE owner_packet=?1 AND state IN ('pending','ready'))",[packet],|r|r.get(0))?)
}

/// Ends this model turn while leaving the original response obligation waiting.
/// The next model turn is appended to the same member conversation, never replayed.
pub fn suspend(conn:&mut Connection,packet:i64,message:i64) -> Result<()> {
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let p=delivery::packet(&tx,packet)?;
    ensure!(p.state=="running" && has_obligations(&tx,packet)?,"本轮没有可等待的协作结果");
    let m=execution::get(&tx,p.conversation_id)?;
    group::scope(&tx,m.conversation_id)?;
    delivery::ensure_execution(&tx,m.conversation_id,Some(message))?;
    let valid:bool=tx.query_row("SELECT message_type IN ('response','assistant') AND finish_time IS NOT NULL
        AND coalesce(tool_calls_json,'') IN ('','[]','null')
        AND coalesce(CASE WHEN json_valid(metadata_json) THEN json_extract(metadata_json,'$.end_reason') END,'') NOT IN ('interrupted','cancelled','failed')
        FROM message WHERE id=?1 AND conversation_id=?2",params![message,m.conversation_id],|r|r.get(0))?;
    ensure!(valid,"群协作等待必须由原执行正常结束产生，不能覆盖失败或取消");
    tx.execute("UPDATE butler_group_packet SET state='completed' WHERE id=?1",[packet])?;
    tx.execute("UPDATE butler_group_inbox SET state='waiting',error='等待已派出的群协作结果' WHERE packet_id=?1 AND state IN ('delivered','waiting')",[packet])?;
    execution::post_in(&tx,&m,&group::Post { request_key:format!("dependency-wait:{packet}"),kind:group::MessageKind::Progress,
        body:"已请相关成员协助，收到结果后继续这项工作。".into(),reply_to:None,reference_only:false,recipients:vec![] })?;
    delivery::release_packet_rounds_in(&tx,&p)?;
    tx.commit()?;
    Ok(())
}

/// Called inside the result/failure transaction, so the receipt cannot outrun
/// its public result. No caller can choose the destination of a completion.
pub(super) fn resolve_in(conn:&Connection,packet:i64,event:&group::Event) -> Result<()> {
    let p=delivery::packet(conn,packet)?;
    let mut stmt=conn.prepare("SELECT DISTINCT d.id,d.owner_packet FROM butler_group_dependency d
        JOIN butler_group_inbox i ON i.conversation_id=d.target_conversation_id AND i.event_seq=d.request_seq
        WHERE i.packet_id=?1 AND d.state='pending'")?;
    let deps=stmt.query_map([packet],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    resolve_rows(conn,deps,event,Some(p.conversation_id))
}

fn resolve_rows(conn:&Connection,deps:Vec<(i64,i64)>,event:&group::Event,sender:Option<i64>) -> Result<()> {
    for (id,owner) in deps {
        let owner=delivery::packet(conn,owner)?;
        let m=execution::get(conn,owner.conversation_id)?;
        if !m.active || m.state!="active" || matches!(owner.state.as_str(),"cancelled"|"failed"|"uncertain") {
            conn.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE id=?1",[id])?;
            continue;
        }
        ensure!(m.group_id==event.group_id && sender!=Some(m.conversation_id),"协作回送归属不匹配");
        conn.execute("UPDATE butler_group_dependency SET state='ready',result_seq=?1 WHERE id=?2",params![event.seq,id])?;
        execution::enqueue_in(conn,m.conversation_id,event.seq,&format!("dependency:{id}"),None)?;
    }
    Ok(())
}

pub(super) fn cancel_target_in(conn:&Connection,member:i64,reason:&str) -> Result<()> {
    let mut stmt=conn.prepare("SELECT id,owner_packet FROM butler_group_dependency WHERE target_member=?1 AND state='pending'")?;
    let deps=stmt.query_map([member],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if deps.is_empty() { return Ok(()); }
    let m=group::member(conn,member)?;
    let room=group::get(conn,m.group_id)?;
    if room.state!="active" {
        conn.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE target_member=?1 AND state='pending'",[member])?;
        return Ok(());
    }
    let event=group::post_in(conn,m.group_id,None,&group::Post { request_key:format!("dependency-cancel:{member}:{}",room.seq),
        kind:group::MessageKind::Notice,body:format!("{} 的协作请求未完成：{reason}。等待者可据此处理原工作，不会自动重派。",m.name),
        reply_to:None,reference_only:false,recipients:vec![] })?;
    resolve_rows(conn,deps,&event,None)
}

pub(super) fn cancel_execution_in(conn:&Connection,conversation:i64,reason:&str)->Result<()> {
    let mut stmt=conn.prepare("SELECT id,owner_packet FROM butler_group_dependency WHERE target_conversation_id=?1 AND state='pending'")?;
    let deps=stmt.query_map([conversation],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if deps.is_empty() {return Ok(())}
    let m=execution::get(conn,conversation)?;
    let room=group::get(conn,m.group_id)?;
    if room.state!="active" {
        conn.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE target_conversation_id=?1 AND state='pending'",[conversation])?;
        return Ok(());
    }
    let event=group::post_in(conn,m.group_id,None,&group::Post {request_key:format!("execution-cancel:{conversation}:{}",room.seq),kind:group::MessageKind::Notice,
        body:format!("{} 的群执行 #{conversation} 未完成：{reason}。不会自动重派。",m.name),reply_to:None,reference_only:false,recipients:vec![]})?;
    resolve_rows(conn,deps,&event,None)
}

/// Exact source work is included even when its public event predates the cursor.
pub(super) fn resumed_work(conn:&Connection,conversation:i64,items:&[delivery::InboxItem]) -> Result<Vec<group::Event>> {
    let m=execution::get(conn,conversation)?;
    let mut events=Vec::new();
    for item in items {
        let Some(id)=item.reason.strip_prefix("dependency:") else { continue };
        let id=id.parse::<i64>()?;
        let mut stmt=conn.prepare("SELECT i.event_seq FROM butler_group_dependency d JOIN butler_group_packet p ON p.id=d.owner_packet
            JOIN butler_group_inbox i ON i.packet_id=p.id WHERE d.id=?1 AND p.conversation_id=?2 AND d.state='ready' ORDER BY i.id")?;
        let seqs=stmt.query_map(params![id,conversation],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(!seqs.is_empty(),"协作回送缺少原始工作：dependency={id}");
        for seq in seqs {
            if !events.iter().any(|e:&group::Event|e.seq==seq) { events.push(group::read_as_member(conn,m.id,seq)?); }
        }
    }
    Ok(events)
}

pub(super) fn bind_in(conn:&Connection,packet:i64,items:&[delivery::InboxItem]) -> Result<()> {
    for item in items {
        if let Some(id)=item.reason.strip_prefix("dependency:") {
            conn.execute("INSERT INTO butler_group_resume(packet_id,dependency_id) VALUES(?1,?2)",params![packet,id.parse::<i64>()?])?;
        }
    }
    Ok(())
}

pub(super) fn finish_parents_in(conn:&Connection,packet:i64,success:bool,event:&group::Event) -> Result<()> {
    finish_parents_with_state(conn,packet,if success { "answered" } else { "failed" },event)
}

pub(super) fn uncertain_parents_in(conn:&Connection,packet:i64,event:&group::Event) -> Result<()> {
    finish_parents_with_state(conn,packet,"uncertain",event)
}

pub(super) fn cancel_parents_in(conn:&Connection,packet:i64,event:&group::Event) -> Result<()> {
    finish_parents_with_state(conn,packet,"cancelled",event)
}

fn finish_parents_with_state(conn:&Connection,packet:i64,state:&str,event:&group::Event) -> Result<()> {
    let mut stmt=conn.prepare("SELECT DISTINCT d.owner_packet FROM butler_group_resume r JOIN butler_group_dependency d ON d.id=r.dependency_id WHERE r.packet_id=?1")?;
    let parents=stmt.query_map([packet],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for parent in parents {
        let pending:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_dependency WHERE owner_packet=?1 AND state IN ('pending','ready'))",[parent],|r|r.get(0))?;
        if !pending || state!="answered" {
            if state!="answered" {
                // A late sibling receipt must not resurrect an obligation that
                // has already ended in failure or an unknown execution result.
                conn.execute("UPDATE butler_group_inbox SET state='cancelled',error='原协作续接已终止，未重新唤醒' WHERE state='queued'
                    AND conversation_id=(SELECT conversation_id FROM butler_group_packet WHERE id=?1)
                    AND reason IN (SELECT 'dependency:' || id FROM butler_group_dependency WHERE owner_packet=?1 AND state='ready')",[parent])?;
                conn.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE owner_packet=?1 AND state IN ('pending','ready')",[parent])?;
            }
            conn.execute("UPDATE butler_group_inbox SET state=?1,error=?2 WHERE packet_id=?3 AND state='waiting'",
                params![state,match state { "answered"=>None,"uncertain"=>Some("群协作续接结果不明，未自动重试"),"cancelled"=>Some("群协作续接已停止，原工作不再等待；未自动重放"),_=>Some("群协作续接失败，详情见后续执行") },parent])?;
            resolve_in(conn,parent,event)?;
            finish_parents_with_state(conn,parent,state,event)?;
        }
    }
    Ok(())
}

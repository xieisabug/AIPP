//! Membership controls identity/visibility. Execution controls private context,
//! delivery cursor and cancellation. Several task executions may share a member.
use anyhow::{ensure,Result};
use rusqlite::{params,Connection,OptionalExtension,TransactionBehavior};
use serde::{Deserialize,Serialize};
use super::butler_group as group;

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Scope {
    pub member:group::Member,
    pub conversation_id:i64,
    pub delivered_through:i64,
    pub task:bool,
    pub title:String,
    pub parent_packet:Option<i64>,
    pub source_seqs:Vec<i64>,
    pub state:String,
}
impl std::ops::Deref for Scope {
    type Target=group::Member;
    fn deref(&self)->&Self::Target {&self.member}
}

pub(super) fn deserialize_scope<'de,D:serde::Deserializer<'de>>(deserializer:D)->std::result::Result<Scope,D::Error> {
    let value=serde_json::Value::deserialize(deserializer)?;
    if value.get("member").is_some() {return serde_json::from_value(value).map_err(serde::de::Error::custom)}
    let member:group::Member=serde_json::from_value(value).map_err(serde::de::Error::custom)?;
    Ok(Scope {conversation_id:member.conversation_id,delivered_through:member.delivered_through,member,
        task:false,title:String::new(),parent_packet:None,source_seqs:vec![],state:"active".into()})
}

pub fn get(conn:&Connection,conversation:i64)->Result<Scope> {
    let (member,cursor,kind,title,parent,sources,state):(i64,i64,String,String,Option<i64>,String,String)=conn.query_row(
        "SELECT member_id,delivered_through,kind,title,parent_packet,source_seqs_json,state FROM butler_group_execution WHERE conversation_id=?1",[conversation],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?;
    Ok(Scope {member:group::member(conn,member)?,conversation_id:conversation,delivered_through:cursor,task:kind=="task",title,parent_packet:parent,source_seqs:serde_json::from_str(&sources)?,state})
}

pub fn scope(conn:&Connection,conversation:i64)->Result<Scope> {
    let e=get(conn,conversation)?;
    ensure!(e.active,"群成员资格已失效");
    ensure!(e.state=="active","群内执行已停止：conversation={conversation}");
    ensure!(group::get(conn,e.group_id)?.state=="active","协作群已停止");
    Ok(e)
}

pub(super) fn advance_in(conn:&Connection,e:&Scope,through:i64)->Result<()> {
    ensure!(conn.execute("UPDATE butler_group_execution SET delivered_through=?1 WHERE conversation_id=?2 AND delivered_through=?3 AND state='active'",
        params![through,e.conversation_id,e.delivered_through])?==1,"群执行投递位置冲突");
    if !e.task {conn.execute("UPDATE butler_group_member SET delivered_through=?1 WHERE id=?2",params![through,e.id])?;}
    Ok(())
}

/// An independent opinion must not bypass its frozen input by reading later
/// public messages through tools. Pending snapshots also constrain older work
/// in the same context, which cannot subsequently forget what it has read.
pub fn snapshot_limit(conn:&Connection,conversation:i64)->Result<Option<i64>> {
    scope(conn,conversation)?;
    Ok(conn.query_row("SELECT MIN(snapshot_seq) FROM butler_group_inbox
        WHERE conversation_id=?1 AND snapshot_seq IS NOT NULL
        AND state IN ('queued','prepared','delivered','waiting')",[conversation],|r|r.get(0))?)
}

pub fn read_public(conn:&Connection,conversation:i64,seq:i64)->Result<group::Event> {
    let me=scope(conn,conversation)?;
    if let Some(upper)=snapshot_limit(conn,conversation)? {
        ensure!(seq<=upper,"本次独立判断只允许读取公共快照 #{upper} 及以前的获准消息");
    }
    group::read_as_member(conn,me.id,seq)
}

pub fn search_public(conn:&Connection,conversation:i64,after:i64,limit:usize,query:&str)->Result<(Vec<group::Event>,i64)> {
    ensure!(after>=0 && (1..=100).contains(&limit),"群消息分页参数无效");
    ensure!(query.chars().count()<=512,"搜索词最多 512 字符");
    let me=scope(conn,conversation)?;
    let upper=snapshot_limit(conn,conversation)?.unwrap_or(group::get(conn,me.group_id)?.seq);
    // Scope and visibility precede pagination; neither a search hit nor a count
    // may reveal another group or unshared pre-join history.
    let mut stmt=conn.prepare("SELECT e.seq FROM butler_group_event e WHERE e.group_id=?1 AND e.seq>?2
        AND (e.seq>?3 OR EXISTS(SELECT 1 FROM butler_group_share s WHERE s.member_id=?4 AND s.event_seq=e.seq))
        AND instr(e.body,?5)>0 AND e.seq<=?7 ORDER BY e.seq LIMIT ?6")?;
    let ids=stmt.query_map(params![me.group_id,after,me.joined_after,me.id,query,limit as i64,upper],|row|row.get::<_,i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let events=ids.into_iter().map(|seq|read_public(conn,conversation,seq)).collect::<Result<Vec<_>>>()?;
    Ok((events,upper))
}

pub fn ensure_collaboration_allowed(conn:&Connection,conversation:i64)->Result<()> {
    ensure!(snapshot_limit(conn,conversation)?.is_none(),
        "当前上下文有未完成的独立判断，不能发起协作、创建子任务或查看任务动态；请先完成独立意见，避免通过回送结果读到快照之后的内容");
    Ok(())
}

pub(super) fn enqueue_in(conn:&Connection,conversation:i64,seq:i64,reason:&str,round:Option<i64>)->Result<()> {
    let e=scope(conn,conversation)?;
    group::read_as_member(conn,e.id,seq)?;
    conn.execute("INSERT INTO butler_group_inbox(group_id,member_id,conversation_id,event_seq,reason,round_id) VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(conversation_id,event_seq,reason) DO NOTHING",params![e.group_id,e.id,conversation,seq,reason,round])?;
    Ok(())
}

pub fn list(conn:&Connection,group_id:i64)->Result<Vec<Scope>> {
    let mut stmt=conn.prepare("SELECT e.conversation_id FROM butler_group_execution e JOIN butler_group_member m ON m.id=e.member_id WHERE m.group_id=?1 ORDER BY e.conversation_id")?;
    let ids=stmt.query_map([group_id],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter().map(|id|get(conn,id)).collect()
}

pub(super) fn post_in(conn:&Connection,e:&Scope,post:&group::Post)->Result<group::Event> {
    if let Some(seq)=post.reply_to {read_public(conn,e.conversation_id,seq)?;}
    if !post.recipients.is_empty() || (post.reply_to.is_some() && !post.reference_only) {
        ensure_collaboration_allowed(conn,e.conversation_id)?;
    }
    let event=group::post_in(conn,e.group_id,Some(e.id),post)?;
    conn.execute("UPDATE butler_group_event SET sender_conversation=?1 WHERE group_id=?2 AND seq=?3 AND sender_conversation IS NULL",params![e.conversation_id,e.group_id,event.seq])?;
    let event=group::event(conn,e.group_id,event.seq)?;
    ensure!(event.sender_conversation==Some(e.conversation_id),"请求键属于另一个群执行，禁止混用");
    conn.execute("UPDATE butler_group_event SET sender_packet=(SELECT id FROM butler_group_packet WHERE conversation_id=?1 AND state='running')
        WHERE group_id=?2 AND seq=?3 AND sender_packet IS NULL",params![e.conversation_id,e.group_id,event.seq])?;
    if !post.recipients.is_empty() {
        let owner:i64=conn.query_row("SELECT id FROM butler_group_packet WHERE conversation_id=?1 AND state='running'",[e.conversation_id],|r|r.get(0))?;
        // Use the actual routed inbox, including replies to another task of the
        // same member, rather than assuming every recipient uses its main chat.
        let mut stmt=conn.prepare("SELECT conversation_id,reason FROM butler_group_inbox WHERE group_id=?1 AND event_seq=?2")?;
        let targets=stmt.query_map(params![e.group_id,event.seq],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for (target,reason) in targets {
            let return_to=if !post.reference_only {
                post.reply_to.map(|seq|super::butler_group_chain::reply_origin(conn,owner,e.group_id,seq,target)).transpose()?.flatten()
            } else {None};
            super::butler_group_chain::register_delivery(conn,owner,event.seq,&[target],&reason,return_to)?;
        }
    }
    Ok(event)
}

pub(super) fn namespace_post(e:&Scope,post:&group::Post)->Result<group::Post> {
    use sha2::{Digest,Sha256};
    ensure!(!post.request_key.trim().is_empty() && post.request_key.len()<=128,"需要稳定的消息请求标识（1–128 字节）");
    let mut scoped=post.clone();
    if e.task {scoped.request_key=format!("execution:{}:{:x}",e.conversation_id,Sha256::digest(post.request_key.as_bytes()));}
    Ok(scoped)
}

#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct TaskRequest {
    pub request_key:String,
    pub member_id:i64,
    pub title:String,
    pub prompt:String,
    #[serde(default)]
    pub source_seqs:Vec<i64>,
}

pub fn start_task(conn:&mut Connection,group_id:i64,request:&TaskRequest,owner:Option<(i64,i64)>)->Result<Scope> {
    use super::butler_group_delivery as delivery;
    ensure!(!request.request_key.trim().is_empty() && request.request_key.len()<=128,"独立任务需要稳定请求标识");
    ensure!(!request.title.trim().is_empty() && request.title.chars().count()<=200,"任务标题应为 1–200 字符");
    ensure!(!request.prompt.trim().is_empty() && request.prompt.chars().count()<=90000,"任务要求应为 1–90000 字符");
    ensure!(request.source_seqs.len()<=100,"独立任务最多显式引用 100 条公开消息");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let target=group::member(&tx,request.member_id)?;
    ensure!(target.active && target.group_id==group_id,"独立任务执行者不是本群有效成员");
    let room=group::get(&tx,group_id)?;
    ensure!(room.state=="active","协作群已停止");
    let owner_packet=if let Some((conversation,source))=owner {
        delivery::ensure_execution(&tx,conversation,Some(source))?;
        ensure_collaboration_allowed(&tx,conversation)?;
        ensure!(scope(&tx,conversation)?.group_id==group_id,"不能派发其他群的任务");
        Some(tx.query_row("SELECT id FROM butler_group_packet WHERE conversation_id=?1 AND state='running'",[conversation],|r|r.get::<_,i64>(0))?)
    } else {None};
    let encoded=serde_json::to_string(&(request,owner_packet))?;
    let key=if let Some(packet)=owner_packet {format!("packet:{packet}:{}",request.request_key)} else {format!("user:{}",request.request_key)};
    let previous:Option<(i64,String)>=tx.query_row("SELECT conversation_id,request_json FROM butler_group_execution WHERE member_id=?1 AND request_key=?2",
        params![target.id,key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((conversation,original))=previous {
        ensure!(original==encoded,"任务请求标识已用于不同内容，不能重建任务");
        return get(&tx,conversation);
    }
    for seq in &request.source_seqs {
        group::read_as_member(&tx,target.id,*seq)?;
        if let Some((conversation,_))=owner {read_public(&tx,conversation,*seq)?;}
    }
    if let Some(packet)=owner_packet {
        super::butler_group_chain::admit(&tx,packet,1)?;
        let depth:i64=tx.query_row("WITH RECURSIVE parents(packet,depth) AS (
            SELECT ?1,1 UNION ALL SELECT e.parent_packet,p.depth+1 FROM parents p
            JOIN butler_group_packet original ON original.id=p.packet JOIN butler_group_execution e ON e.conversation_id=original.conversation_id
            WHERE e.parent_packet IS NOT NULL AND p.depth<9) SELECT MAX(depth) FROM parents",[packet],|r|r.get(0))?;
        ensure!(depth<=8,"群独立任务最多 8 层，不能继续派生");
        let children:i64=tx.query_row("SELECT count(*) FROM butler_group_execution WHERE parent_packet=?1",[packet],|r|r.get(0))?;
        ensure!(children<32,"本轮独立任务不能超过 32 项");
    }
    tx.execute("INSERT INTO conversation(name,assistant_id,conversation_kind,parent_butler_conversation_id,is_hidden_from_normal_chat_list)
        VALUES(?1,?2,'butler_group_task',?3,1)",params![request.title,target.assistant_id,group_id])?;
    let conversation=tx.last_insert_rowid();
    tx.execute("INSERT INTO butler_group_execution(conversation_id,member_id,kind,title,parent_packet,request_key,request_json,source_seqs_json,delivered_through)
        VALUES(?1,?2,'task',?3,?4,?5,?6,?7,?8)",params![conversation,target.id,request.title,owner_packet,key,encoded,serde_json::to_string(&request.source_seqs)?,room.seq])?;
    let post=group::Post {request_key:format!("isolated-task:{conversation}"),kind:group::MessageKind::Notice,
        body:format!("独立任务：{}\n执行成员：{}\n要求：{}\n显式资料：{}",request.title,target.name,request.prompt,serde_json::to_string(&request.source_seqs)?),
        reply_to:None,reference_only:false,recipients:vec![]};
    let event=if let Some((origin,_))=owner {post_in(&tx,&scope(&tx,origin)?,&post)?} else {group::post_in(&tx,group_id,None,&post)?};
    enqueue_in(&tx,conversation,event.seq,&format!("task:{conversation}"),None)?;
    if let Some(packet)=owner_packet {
        tx.execute("INSERT INTO butler_group_dependency(owner_packet,target_member,target_conversation_id,request_seq) VALUES(?1,?2,?3,?4)",
            params![packet,target.id,conversation,event.seq])?;
    }
    let result=get(&tx,conversation)?;
    tx.commit()?; Ok(result)
}

pub fn descendants(conn:&Connection,seeds:&[i64])->Result<Vec<Scope>> {
    let mut ids=std::collections::BTreeSet::new();
    for seed in seeds {
        let mut stmt=conn.prepare("WITH RECURSIVE tree(conversation) AS (SELECT ?1 UNION
            SELECT e.conversation_id FROM tree t JOIN butler_group_packet p ON p.conversation_id=t.conversation
            JOIN butler_group_execution e ON e.parent_packet=p.id) SELECT conversation FROM tree")?;
        for row in stmt.query_map([seed],|r|r.get::<_,i64>(0))? {ids.insert(row?);}
    }
    ids.into_iter().map(|id|get(conn,id)).collect()
}

pub(super) fn fence_in(conn:&Connection,conversation:i64,reason:&str)->Result<()> {
    let e=get(conn,conversation)?;
    conn.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE owner_packet IN (SELECT id FROM butler_group_packet WHERE conversation_id=?1) AND state IN ('pending','ready')",[conversation])?;
    super::butler_group_dependency::cancel_execution_in(conn,conversation,reason)?;
    conn.execute("UPDATE butler_group_native_input SET state='cancelled',error=?1 WHERE packet_id IN (SELECT id FROM butler_group_packet WHERE conversation_id=?2) AND state IN ('sending','uncertain')",params![reason,conversation])?;
    conn.execute("UPDATE butler_group_preparation SET state='cancelled',error=?1 WHERE conversation_id=?2 AND state='pending'",params![reason,conversation])?;
    conn.execute("UPDATE butler_group_packet SET state='cancelled',error=?1 WHERE conversation_id=?2 AND state IN ('prepared','delivered','running','uncertain')",params![reason,conversation])?;
    conn.execute("UPDATE butler_group_inbox SET state='cancelled',error=?1 WHERE conversation_id=?2 AND state IN ('queued','prepared','delivered','waiting','uncertain')",params![reason,conversation])?;
    if e.task {conn.execute("UPDATE butler_group_execution SET state='cancelled' WHERE conversation_id=?1",[conversation])?;}
    Ok(())
}

pub fn stop_task(conn:&mut Connection,group_id:i64,conversation:i64)->Result<Vec<Scope>> {
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let task=get(&tx,conversation)?;
    ensure!(task.group_id==group_id && task.task,"只能停止本群的独立任务");
    let executions=super::butler_group_stop::fence(&tx,&[conversation],"用户停止该群独立任务及其派生工作")?;
    tx.commit()?; Ok(executions)
}

#[derive(Serialize)]
pub struct TaskView {
    pub conversation_id:i64,pub member_id:i64,pub title:String,pub parent_packet:Option<i64>,pub state:String,
}
pub fn tasks(conn:&Connection,group_id:i64)->Result<Vec<TaskView>> {
    let mut tasks=Vec::new();
    for e in list(conn,group_id)?.into_iter().filter(|e|e.task) {
        let pending:Option<String>=conn.query_row("SELECT state FROM butler_group_inbox WHERE conversation_id=?1 AND state IN ('queued','prepared','delivered','waiting','uncertain')
            ORDER BY CASE state WHEN 'uncertain' THEN 0 WHEN 'waiting' THEN 1 WHEN 'delivered' THEN 2 ELSE 3 END,id LIMIT 1",[e.conversation_id],|r|r.get(0)).optional()?;
        let latest:Option<String>=conn.query_row("SELECT state FROM butler_group_packet WHERE conversation_id=?1 ORDER BY id DESC LIMIT 1",[e.conversation_id],|r|r.get(0)).optional()?;
        tasks.push(TaskView {conversation_id:e.conversation_id,member_id:e.id,title:e.title,parent_packet:e.parent_packet,
            state:if e.state=="cancelled" {"cancelled".into()} else {pending.or(latest).unwrap_or_else(||"queued".into())}});
    }
    Ok(tasks)
}

#[cfg(test)]
#[path="butler_group_execution_tests.rs"]
mod tests;

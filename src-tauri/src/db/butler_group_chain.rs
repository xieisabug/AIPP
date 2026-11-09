//! Limits are derived from durable work ancestry, not the current model turn.
//! A resumed packet and a task packet inherit the original delegation budget.
use anyhow::{ensure, Result};
use rusqlite::{params, Connection};

pub const MAX_DELEGATIONS: i64 = 32;
pub const MAX_DEPTH: i64 = 8;
pub const TTL_SECONDS: i64 = 120 * 60;

// Resuming an owner does not add a delegation level. The UNION removes duplicate
// edges when a task's first request is also registered as a response obligation.
const GRAPH: &str = "WITH RECURSIVE edges(child,parent,cost) AS (
    SELECT i.packet_id,d.owner_packet,1 FROM butler_group_dependency d
    JOIN butler_group_inbox i ON i.conversation_id=d.target_conversation_id AND i.event_seq=d.request_seq
    WHERE i.packet_id IS NOT NULL
    UNION SELECT i.packet_id,coalesce(d.return_to_packet,d.owner_packet),CASE WHEN d.return_to_packet IS NULL THEN 1 ELSE 0 END FROM butler_group_dispatch d
    JOIN butler_group_inbox i ON i.conversation_id=d.target_conversation_id AND i.event_seq=d.event_seq AND i.reason=d.reason
    WHERE i.packet_id IS NOT NULL AND i.packet_id!=coalesce(d.return_to_packet,d.owner_packet)
    UNION SELECT r.packet_id,d.owner_packet,0 FROM butler_group_resume r
    JOIN butler_group_dependency d ON d.id=r.dependency_id
    UNION SELECT p.id,e.parent_packet,1 FROM butler_group_packet p
    JOIN butler_group_execution e ON e.conversation_id=p.conversation_id WHERE e.parent_packet IS NOT NULL
), ancestors(packet) AS (
    SELECT ?1 UNION SELECT e.parent FROM ancestors a JOIN edges e ON e.child=a.packet
), roots(packet) AS (
    SELECT a.packet FROM ancestors a WHERE NOT EXISTS(SELECT 1 FROM edges e WHERE e.child=a.packet)
), work(packet) AS (
    SELECT packet FROM roots UNION SELECT e.child FROM work w JOIN edges e ON e.parent=w.packet
)";

/// Call inside the same write transaction that registers the new dependencies.
/// Retry paths must return their persisted result before calling this function.
pub(super) fn admit(conn:&Connection,packet:i64,count:usize)->Result<()> {
    let now:i64=conn.query_row("SELECT unixepoch('now')",[],|r|r.get(0))?;
    admit_at(conn,packet,count,now)
}

pub(super) fn ensure_recipients(conn:&Connection,packet:i64,recipients:&[i64])->Result<()> {
    let mut stmt=conn.prepare(&format!("{GRAPH} SELECT DISTINCT m.assistant_id FROM ancestors a
        JOIN butler_group_packet p ON p.id=a.packet JOIN butler_group_member m ON m.id=p.member_id"))?;
    let assistants=stmt.query_map([packet],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for recipient in recipients {
        let target=super::butler_group::member(conn,*recipient)?;
        ensure!(!assistants.contains(&target.assistant_id),"群协作不能重新委托给本工作链的上游助手：packet={packet}; member={recipient}; assistant={}",target.assistant_id);
    }
    Ok(())
}

fn admit_at(conn:&Connection,packet:i64,count:usize,now:i64)->Result<()> {
    ensure!(count>0 && count<=MAX_DELEGATIONS as usize,"群工作链派发数量无效：packet={packet}; count={count}");
    let (roots,started,used):(i64,Option<i64>,i64)=conn.query_row(&format!("{GRAPH}
        SELECT (SELECT count(*) FROM roots),
            (SELECT min(unixepoch(p.created_at)) FROM roots r JOIN butler_group_packet p ON p.id=r.packet),
            ((SELECT count(*) FROM butler_group_dispatch d JOIN work w ON w.packet=d.owner_packet)
             + (SELECT count(*) FROM butler_group_dependency d JOIN work w ON w.packet=d.owner_packet
                WHERE NOT EXISTS(SELECT 1 FROM butler_group_dispatch s WHERE s.owner_packet=d.owner_packet
                    AND s.event_seq=d.request_seq AND s.target_conversation_id=d.target_conversation_id)))"),
        [packet],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    ensure!(roots>0,"群工作链缺少源请求或存在循环：packet={packet}");
    let started=started.ok_or_else(||anyhow::anyhow!("群工作链缺少有效创建时间：packet={packet}"))?;
    ensure!(now<started+TTL_SECONDS,"群工作链派发有效期已到：packet={packet}; started_at={started}; expires_at={}; 不再派生新工作",started+TTL_SECONDS);
    ensure!(used+count as i64<=MAX_DELEGATIONS,"群工作链派发预算已用尽：packet={packet}; used={used}; requested={count}; limit={MAX_DELEGATIONS}");
    // UNION (rather than ALL) bounds duplicate paths after merged resumptions.
    // Cap depth in SQL so corrupt cyclic records cannot recurse indefinitely.
    let depth:i64=conn.query_row(&format!("{GRAPH}, levels(packet,depth) AS (
        SELECT ?1,0 UNION SELECT e.parent,l.depth+e.cost FROM levels l
        JOIN edges e ON e.child=l.packet WHERE l.depth<?2
    ) SELECT max(depth) FROM levels"),params![packet,MAX_DEPTH],|r|r.get(0))?;
    ensure!(depth<MAX_DEPTH,"群工作链最多 {MAX_DEPTH} 层：packet={packet}; depth={depth}");
    Ok(())
}

/// Persist delivery ownership atomically with the public event/seat creation.
/// The actual inbox may be created later when an ordered seat becomes active.
pub(super) fn register(conn:&Connection,owner:i64,event:i64,targets:&[i64],reason:&str)->Result<()> {
    register_delivery(conn,owner,event,targets,reason,None)
}

/// A reply to an ancestor's exact public message returns to that original work;
/// it is not a new delegation from the child back to its parent.
pub(super) fn reply_origin(conn:&Connection,owner:i64,group:i64,seq:i64,target:i64)->Result<Option<i64>> {
    let origin:Option<i64>=conn.query_row("SELECT sender_packet FROM butler_group_event WHERE group_id=?1 AND seq=?2",params![group,seq],|r|r.get(0))?;
    let Some(origin)=origin else {return Ok(None)};
    let valid:bool=conn.query_row(&format!("{GRAPH} SELECT EXISTS(SELECT 1 FROM ancestors a
        JOIN butler_group_packet p ON p.id=a.packet WHERE a.packet=?2 AND p.conversation_id=?3
        AND p.state NOT IN ('cancelled','failed','uncertain'))"),params![owner,origin,target],|r|r.get(0))?;
    Ok(valid.then_some(origin))
}

pub(super) fn register_delivery(conn:&Connection,owner:i64,event:i64,targets:&[i64],reason:&str,return_to:Option<i64>)->Result<()> {
    let mut fresh=Vec::new();
    for target in targets {
        let other_owner:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_dispatch
            WHERE owner_packet!=?1 AND event_seq=?2 AND target_conversation_id=?3 AND reason=?4)",
            params![owner,event,target,reason],|r|r.get(0))?;
        ensure!(!other_owner,"派发请求已属于其他执行，不能重新归属：packet={owner}; event={event}; conversation={target}; reason={reason}");
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_dispatch
            WHERE owner_packet=?1 AND event_seq=?2 AND target_conversation_id=?3 AND reason=?4)",
            params![owner,event,target,reason],|r|r.get(0))?;
        if !exists && !fresh.contains(target) {fresh.push(*target);}
    }
    if fresh.is_empty() {return Ok(())}
    admit(conn,owner,fresh.len())?;
    let members=fresh.iter().map(|id|super::butler_group_execution::get(conn,*id).map(|e|e.id)).collect::<Result<Vec<_>>>()?;
    if return_to.is_none() {ensure_recipients(conn,owner,&members)?;}
    for target in fresh {
        conn.execute("INSERT INTO butler_group_dispatch(owner_packet,event_seq,target_conversation_id,reason,return_to_packet) VALUES(?1,?2,?3,?4,?5)",
            params![owner,event,target,reason,return_to])?;
    }
    Ok(())
}

//! Stop follows delivery ownership, not an entire recipient's private session.
use anyhow::Result;
use rusqlite::{params,Connection};
use std::collections::BTreeSet;
use super::{butler_group as group,butler_group_delivery as delivery,butler_group_execution as execution};

#[derive(Default)]
struct Plan {
    whole:BTreeSet<i64>, packets:BTreeSet<i64>, inbox:BTreeSet<i64>, rounds:BTreeSet<i64>,
}

fn plan(conn:&Connection,seeds:&[i64])->Result<Plan> {
    let mut plan=Plan::default();
    plan.whole.extend(seeds);
    loop {
        let before=(plan.whole.len(),plan.packets.len(),plan.inbox.len(),plan.rounds.len());
        for conversation in plan.whole.clone() {
            let mut stmt=conn.prepare("SELECT id FROM butler_group_packet WHERE conversation_id=?1")?;
            for id in stmt.query_map([conversation],|r|r.get::<_,i64>(0))? {plan.packets.insert(id?);}
            let mut stmt=conn.prepare("SELECT id FROM butler_group_inbox WHERE conversation_id=?1")?;
            for id in stmt.query_map([conversation],|r|r.get::<_,i64>(0))? {plan.inbox.insert(id?);}
        }
        for packet in plan.packets.clone() {
            // Returned replies go back to an ancestor; they are not child work
            // and must not propagate a child's cancellation into its parent.
            let mut stmt=conn.prepare("SELECT i.id FROM butler_group_dispatch d JOIN butler_group_inbox i
                ON i.conversation_id=d.target_conversation_id AND i.event_seq=d.event_seq AND i.reason=d.reason
                WHERE d.owner_packet=?1 AND d.return_to_packet IS NULL
                UNION SELECT i.id FROM butler_group_dependency d JOIN butler_group_inbox i
                ON i.conversation_id=d.target_conversation_id AND i.event_seq=d.request_seq WHERE d.owner_packet=?1")?;
            for id in stmt.query_map([packet],|r|r.get::<_,i64>(0))? {plan.inbox.insert(id?);}
            let mut stmt=conn.prepare("SELECT conversation_id FROM butler_group_execution WHERE parent_packet=?1")?;
            for id in stmt.query_map([packet],|r|r.get::<_,i64>(0))? {plan.whole.insert(id?);}
            let mut stmt=conn.prepare("SELECT r.packet_id FROM butler_group_resume r JOIN butler_group_dependency d ON d.id=r.dependency_id WHERE d.owner_packet=?1")?;
            for id in stmt.query_map([packet],|r|r.get::<_,i64>(0))? {plan.packets.insert(id?);}
            let mut stmt=conn.prepare("SELECT DISTINCT CAST(substr(reason,7) AS INTEGER) FROM butler_group_dispatch
                WHERE owner_packet=?1 AND reason LIKE 'round:%'")?;
            for id in stmt.query_map([packet],|r|r.get::<_,i64>(0))? {plan.rounds.insert(id?);}
        }
        for inbox in plan.inbox.clone() {
            let packet:Option<i64>=conn.query_row("SELECT packet_id FROM butler_group_inbox WHERE id=?1",[inbox],|r|r.get(0))?;
            if let Some(packet)=packet {plan.packets.insert(packet);}
        }
        if before==(plan.whole.len(),plan.packets.len(),plan.inbox.len(),plan.rounds.len()) {return Ok(plan)}
    }
}

pub fn owns_task(conn:&Connection,conversation:i64,target:i64)->Result<bool> {
    Ok(plan(conn,&[conversation])?.whole.contains(&target))
}

/// Caller owns an IMMEDIATE transaction and prevents new model admissions until
/// every returned execution has been halted. Unrelated queued work is retained.
pub(super) fn fence(conn:&Connection,seeds:&[i64],reason:&str)->Result<Vec<execution::Scope>> {
    let plan=plan(conn,seeds)?;
    let mut halt=plan.whole.clone();
    let mut affected_conversations=BTreeSet::new();
    for packet in &plan.packets {
        let p=delivery::packet(conn,*packet)?;
        if matches!(p.state.as_str(),"running"|"delivered"|"uncertain") {halt.insert(p.conversation_id);}
        affected_conversations.insert(p.conversation_id);
        conn.execute("UPDATE butler_group_dependency SET state='cancelled' WHERE owner_packet=?1 AND state IN ('pending','ready')",[packet])?;
    }
    // Fence every affected packet before emitting any receipt. Otherwise an
    // intermediate parent could be awakened while its cancellation is in flight.
    for packet in &plan.packets {
        let p=delivery::packet(conn,*packet)?;
        if p.state=="prepared" && !plan.whole.contains(&p.conversation_id) {
            // This packet has never been submitted. Unrelated obligations can
            // safely be re-prepared, retaining their original inbox identity.
            let mut stmt=conn.prepare("SELECT id FROM butler_group_inbox WHERE packet_id=?1 AND state='prepared'")?;
            let ids=stmt.query_map([packet],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            drop(stmt);
            for id in ids.into_iter().filter(|id|!plan.inbox.contains(id)) {
                conn.execute("DELETE FROM butler_group_resume WHERE packet_id=?1 AND dependency_id=(
                    SELECT CAST(substr(reason,12) AS INTEGER) FROM butler_group_inbox WHERE id=?2 AND reason LIKE 'dependency:%')",params![packet,id])?;
                conn.execute("UPDATE butler_group_inbox SET state='queued',packet_id=NULL,error=NULL WHERE id=?1",[id])?;
            }
        }
        conn.execute("UPDATE butler_group_native_input SET state='cancelled',error=?1 WHERE packet_id=?2 AND state IN ('sending','uncertain')",params![reason,packet])?;
        conn.execute("UPDATE butler_group_packet SET state='cancelled',error=?1 WHERE id=?2 AND state IN ('prepared','delivered','running','uncertain')",params![reason,packet])?;
        // Several obligations may already share one immutable model input. They
        // cannot be surgically removed or replayed after tools may have executed.
        let detail=format!("{reason}；同一输入包 #{packet} 的未完成事项随原执行中断，未自动重放；其他排队消息保留");
        conn.execute("UPDATE butler_group_inbox SET state='cancelled',error=?1 WHERE packet_id=?2 AND state IN ('prepared','delivered','waiting','uncertain')",params![detail,packet])?;
    }
    for inbox in &plan.inbox {
        let conversation:i64=conn.query_row("SELECT conversation_id FROM butler_group_inbox WHERE id=?1",[inbox],|r|r.get(0))?;
        affected_conversations.insert(conversation);
        conn.execute("UPDATE butler_group_inbox SET state='cancelled',error=?1 WHERE id=?2 AND state IN ('queued','prepared','delivered','waiting','uncertain')",params![reason,inbox])?;
    }
    for conversation in affected_conversations {
        // A frozen preparation has no model-side effects. Invalidating it leaves
        // other queued obligations intact for a fresh, correctly scoped packet.
        conn.execute("UPDATE butler_group_preparation SET state='cancelled',error=?1 WHERE conversation_id=?2 AND state='pending'",params![reason,conversation])?;
    }
    for round in &plan.rounds {
        conn.execute("UPDATE butler_group_round SET state='cancelled' WHERE id=?1 AND state='active'",[round])?;
    }
    for conversation in &plan.whole {execution::fence_in(conn,*conversation,reason)?;}
    if let Some(seed)=seeds.first() {
        let scope=execution::get(conn,*seed)?;
        let room=group::get(conn,scope.group_id)?;
        if room.state=="active" && !plan.packets.is_empty() {
            let event=group::post_in(conn,room.id,None,&group::Post {request_key:format!("stop-propagation:{seed}:{}",room.seq),
                kind:group::MessageKind::Notice,body:format!("{reason}。派生执行已封锁；已投递的同一输入包一并中断，不自动重放工具，其他排队事项保留。"),
                reply_to:None,reference_only:false,recipients:vec![]})?;
            for packet in &plan.packets {
                super::butler_group_dependency::resolve_in(conn,*packet,&event)?;
                super::butler_group_dependency::cancel_parents_in(conn,*packet,&event)?;
            }
        }
    }
    for packet in &plan.packets {delivery::release_packet_rounds_in(conn,&delivery::packet(conn,*packet)?)?;}
    halt.into_iter().map(|id|execution::get(conn,id)).collect()
}

//! Native steering has a two-phase delivery boundary. Persist the exact input
//! before writing to the process; advance the cursor only after its ACK. A lost
//! ACK is uncertain, never an invitation to resend into a new turn/session.
use anyhow::{ensure,Context,Result};
use rusqlite::{params,Connection,TransactionBehavior};
use super::{butler_group as group,butler_group_delivery as delivery};

#[derive(Debug,Clone)]
pub struct NativeInput { pub id:i64,pub payload:String }

pub fn reserve(conn:&mut Connection,conversation:i64,source:i64,run:&str,session:&str,turn:&str)->Result<Option<NativeInput>> {
    if !delivery::is_member_conversation(conn,conversation)? { return Ok(None); }
    ensure!(!run.is_empty() && !session.is_empty() && !turn.is_empty(),"原生群投递缺少 run/session/turn 身份");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let Some(plan)=delivery::build_supplement_in(&tx,conversation,source)? else {return Ok(None)};
    let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM butler_group_native_input WHERE packet_id=?1 AND state IN ('sending','uncertain'))",[plan.packet_id],|r|r.get(0))?;
    ensure!(!pending,"原生群补充已发送但尚未确认，禁止重发：packet={}",plan.packet_id);
    tx.execute("INSERT INTO butler_group_native_input(packet_id,plan_json,run_id,session_id,turn_id) VALUES(?1,?2,?3,?4,?5)",
        params![plan.packet_id,serde_json::to_string(&plan)?,run,session,turn])?;
    let input=NativeInput {id:tx.last_insert_rowid(),payload:plan.payload};
    tx.commit()?; Ok(Some(input))
}

pub fn acknowledge(conn:&mut Connection,id:i64,run:&str,session:&str,turn:&str)->Result<i64> {
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (encoded,saved_run,saved_session,saved_turn,state,message):(String,String,String,String,String,Option<i64>)=tx.query_row(
        "SELECT plan_json,run_id,session_id,turn_id,state,input_message_id FROM butler_group_native_input WHERE id=?1",[id],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
    ensure!(saved_run==run && saved_session==session && saved_turn==turn,"原生群投递确认身份不匹配：input={id}");
    if state=="delivered" {return message.context("已确认的原生群输入缺少本地消息")}
    ensure!(state=="sending","原生群投递已终止，拒绝迟到确认：input={id}, state={state}");
    let plan:delivery::SupplementPlan=serde_json::from_str(&encoded)?;
    let message=delivery::commit_supplement_in(&tx,&plan)?;
    tx.execute("UPDATE butler_group_native_input SET state='delivered',input_message_id=?1 WHERE id=?2",params![message,id])?;
    tx.commit()?; Ok(message)
}

/// A matching RPC error terminates this supplement without advancing its
/// cursor. Cancel the reservation and fail its obligations atomically; never
/// enqueue it again or claim prior tool side effects have been rolled back.
pub fn rejected(conn:&mut Connection,id:i64,reason:&str)->Result<()> {
    ensure!(!reason.trim().is_empty(),"原生群投递拒绝必须保留原始诊断");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (encoded,state):(String,String)=tx.query_row("SELECT plan_json,state FROM butler_group_native_input WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if state!="sending" {return Ok(())}
    let plan:delivery::SupplementPlan=serde_json::from_str(&encoded)?;
    tx.execute("UPDATE butler_group_native_input SET state='cancelled',error=?1 WHERE id=?2",params![reason,id])?;
    let p=delivery::packet(&tx,plan.packet_id)?;
    if matches!(p.state.as_str(),"running"|"delivered") {
        for item in &plan.selected {
            tx.execute("UPDATE butler_group_inbox SET state='delivered',packet_id=?1 WHERE id=?2 AND state='queued'",params![p.id,item.id])?;
        }
        delivery::fail_in(&tx,p.id,reason)?;
    }
    tx.commit()?; Ok(())
}

pub fn uncertain(conn:&mut Connection,id:i64,reason:&str)->Result<()> {
    ensure!(!reason.trim().is_empty(),"原生群投递结果不明必须保留原始诊断");
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (encoded,state):(String,String)=tx.query_row("SELECT plan_json,state FROM butler_group_native_input WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if state!="sending" { return Ok(()); }
    let plan:delivery::SupplementPlan=serde_json::from_str(&encoded)?;
    tx.execute("UPDATE butler_group_native_input SET state='uncertain',error=?1 WHERE id=?2",params![reason,id])?;
    let p=delivery::packet(&tx,plan.packet_id)?;
    if !matches!(p.state.as_str(),"running"|"delivered") {tx.commit()?; return Ok(())}
    tx.execute("UPDATE butler_group_packet SET state='uncertain',error=?1 WHERE id=?2",params![reason,p.id])?;
    tx.execute("UPDATE butler_group_inbox SET state='uncertain',error=?1 WHERE packet_id=?2 AND state IN ('delivered','waiting')",params![reason,p.id])?;
    for item in &plan.selected {
        tx.execute("UPDATE butler_group_inbox SET state='uncertain',packet_id=?1,error=?2 WHERE id=?3 AND state='queued'",params![p.id,reason,item.id])?;
    }
    let m=super::butler_group_execution::get(&tx,p.conversation_id)?;
    if m.active && group::get(&tx,m.group_id)?.state=="active" {
        let receipt=super::butler_group_execution::post_in(&tx,&m,&group::Post { request_key:format!("native-uncertain:{id}"),
            kind:group::MessageKind::Progress,body:format!("原生群补充投递结果不明，未自动重发：{reason}"),reply_to:None,reference_only:false,recipients:vec![] })?;
        super::butler_group_dependency::resolve_in(&tx,p.id,&receipt)?;
        super::butler_group_dependency::uncertain_parents_in(&tx,p.id,&receipt)?;
        delivery::release_packet_rounds_in(&tx,&p)?;
    }
    tx.commit()?; Ok(())
}

pub(super) fn recover_in(conn:&Connection)->Result<()> {
    let mut stmt=conn.prepare("SELECT id,plan_json,run_id,session_id,turn_id FROM butler_group_native_input WHERE state='sending'")?;
    let rows=stmt.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (id,encoded,run,session,turn) in rows {
        let plan:delivery::SupplementPlan=serde_json::from_str(&encoded)?;
        let diagnostic=format!("stage=native_group_recover; input={id}; run={run}; session={session}; turn={turn}; 应用在保存原生确认前退出，缺少原轮次投递确认；未自动重发");
        conn.execute("UPDATE butler_group_native_input SET state='uncertain',error=?1 WHERE id=?2",params![diagnostic,id])?;
        for item in plan.selected {
            conn.execute("UPDATE butler_group_inbox SET state='uncertain',packet_id=?1,error=?2 WHERE id=?3 AND state='queued'",params![plan.packet_id,diagnostic,item.id])?;
        }
    }
    Ok(())
}

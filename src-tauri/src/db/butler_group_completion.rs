//! Completion evidence belongs to the exact immutable input's message tree.
use anyhow::{Context,Result};
use rusqlite::{params,Connection,OptionalExtension};
use super::butler_group_delivery as delivery;

#[derive(Debug,PartialEq,Eq)]
pub enum Evidence {
    Pending,
    Answer(i64),
    Silent(i64),
    Failed(String),
}

pub fn evidence(conn:&Connection,packet:i64)->Result<Evidence> {
    let p=delivery::packet(conn,packet)?;
    let input=p.input_message_id.context("运行中的群输入缺少消息 ID")?;
    let row:Option<(i64,String,String,bool,Option<String>,Option<String>)>=conn.query_row(
        "WITH RECURSIVE descendants(id) AS (
            SELECT id FROM message WHERE id=?1 AND conversation_id=?2
            UNION SELECT m.id FROM message m JOIN descendants d ON m.parent_id=d.id WHERE m.conversation_id=?2
        ) SELECT m.id,m.message_type,m.content,m.finish_time IS NOT NULL,m.tool_calls_json,m.metadata_json
        FROM message m JOIN descendants d ON d.id=m.id WHERE m.id!=?1
            AND m.message_type IN ('response','assistant','error','tool_result') ORDER BY m.id DESC LIMIT 1",
        params![input,p.conversation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((id,role,body,finished,calls,metadata))=row else {return Ok(Evidence::Pending)};
    if role=="error" {
        return Ok(Evidence::Failed(format!("stage=member_execution; conversation={}; packet={packet}; message={id}; {}",
            p.conversation_id,if body.trim().is_empty() {"执行通道记录了 error，但未保存底层错误正文"} else {&body})));
    }
    if !finished || !matches!(role.as_str(),"response"|"assistant") {return Ok(Evidence::Pending)}
    let metadata=metadata.map(|s|serde_json::from_str::<serde_json::Value>(&s)).transpose()?;
    if let Some(reason)=metadata.as_ref().and_then(|v|v.get("end_reason")).and_then(|v|v.as_str()) {
        if matches!(reason,"interrupted"|"cancelled"|"failed"|"goal_superseded") {
            return Ok(Evidence::Failed(format!("stage=member_terminal; conversation={}; packet={packet}; message={id}; end_reason={reason}; {body}",p.conversation_id)));
        }
    }
    if calls.as_deref().is_some_and(|s|!matches!(s.trim(),""|"[]"|"null")) {return Ok(Evidence::Pending)}
    if body.trim()=="[[NO_REPLY]]" {return Ok(Evidence::Silent(id))}
    if super::butler_execution::is_completed_content(&role,&body,finished,calls.as_deref()) {return Ok(Evidence::Answer(id))}
    Ok(Evidence::Pending)
}

#[cfg(test)]
#[path="butler_group_completion_tests.rs"]
mod tests;

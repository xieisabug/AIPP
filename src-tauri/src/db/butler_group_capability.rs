//! Capability observations are append-only public facts, never prompt rewrites.
use anyhow::{ensure,Result};
use rusqlite::{params,Connection,OptionalExtension,TransactionBehavior};
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
use super::butler_group as group;

#[derive(Clone,Serialize,Deserialize)]
pub struct Tool {pub name:String,pub description:String,pub auto_run:bool,pub parameters:serde_json::Value}
#[derive(Clone,Serialize,Deserialize)]
pub struct Server {pub id:i64,pub name:String,pub summary:String,pub tools:Vec<Tool>}
#[derive(Clone,Serialize,Deserialize)]
pub struct Profile {pub dynamic_loading:bool,pub native_toolcall:bool,pub servers:Vec<Server>,pub error:Option<String>}

fn shorten(text:&str,limit:usize)->String {
    let mut chars=text.chars();let mut result=chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {result.push_str("…（摘要，完整定义请查询 group_members）");} result
}

pub fn record(conn:&mut Connection,member:i64,mut profile:Profile)->Result<bool> {
    profile.servers.sort_by_key(|s|s.id);
    for server in &mut profile.servers {server.tools.sort_by(|a,b|a.name.cmp(&b.name));}
    let encoded=serde_json::to_string(&profile)?;
    let fingerprint=format!("{:x}",Sha256::digest(encoded.as_bytes()));
    let tx=conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m=group::member(&tx,member)?;
    ensure!(m.active && group::get(&tx,m.group_id)?.state=="active","群成员已退出或群已停止，不能记录能力变更");
    let previous:Option<String>=tx.query_row("SELECT fingerprint FROM butler_group_capability WHERE member_id=?1",[member],|r|r.get(0)).optional()?;
    if previous.as_deref()==Some(&fingerprint) {return Ok(false)}
    let mut lines=if let Some(error)=&profile.error {vec![format!("{} 的应用工具配置读取失败（member_id={member}），当前能力未确认，不代表没有工具；未使用旧配置作为本次能力。原因：{error}",m.name)]}
        else {vec![format!("{} 的应用工具配置{}（member_id={member}）。这是配置摘要，未验证外部连通性，不代表原生通道的全部能力。动态加载={}，原生工具调用格式={}。",
        m.name,if previous.is_some(){"已变更"}else{"已记录"},profile.dynamic_loading,profile.native_toolcall)]};
    for server in &profile.servers {
        lines.push(format!("工具集 {}（id={}）：{}",shorten(&server.name,160),server.id,shorten(&server.summary,240)));
        for tool in &server.tools {
            lines.push(format!("- {}：{}；自动执行配置={}",shorten(&tool.name,160),shorten(&tool.description,320),tool.auto_run));
        }
    }
    if profile.servers.is_empty() && profile.error.is_none() {lines.push("当前没有启用的应用工具集。".into());}
    // Bound the summary, not the saved configuration or the observed fingerprint.
    let summary=shorten(&lines.join("\n"),18000);
    let seq=group::get(&tx,m.group_id)?.seq;
    let event=group::post_in(&tx,m.group_id,None,&group::Post {request_key:format!("capability:{member}:{seq}"),kind:group::MessageKind::Notice,
        body:summary,reply_to:None,reference_only:false,recipients:vec![]})?;
    tx.execute("INSERT INTO butler_group_capability(member_id,fingerprint,profile_json,event_seq) VALUES(?1,?2,?3,?4)
        ON CONFLICT(member_id) DO UPDATE SET fingerprint=excluded.fingerprint,profile_json=excluded.profile_json,event_seq=excluded.event_seq",
        params![member,fingerprint,encoded,event.seq])?;
    tx.commit()?;Ok(true)
}

#[cfg(test)]
#[path="butler_group_capability_tests.rs"]
mod tests;

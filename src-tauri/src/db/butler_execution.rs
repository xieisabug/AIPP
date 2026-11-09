//! Common contract for the three Butler execution adapters. Native tables remain authoritative:
//! task finalization locks, peer compare-and-swap claims and scheduled atomic run claims are not replaced.
use serde::{Serialize,Deserialize};
#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum Route { Task, Peer, Scheduled }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct Identity { pub route:Route, pub work_id:String, pub execution_id:String, pub request_key:String }
impl Identity {
 pub fn task(conversation:i64)->Self {Self::new(Route::Task,format!("task:{conversation}"),format!("task:{conversation}:initial"))}
 pub fn peer(chain:i64,exchange:i64)->Self {Self::new(Route::Peer,format!("chain:{chain}"),format!("peer:{exchange}"))}
 pub fn scheduled(task:i64,run:&str)->Self {Self::new(Route::Scheduled,format!("schedule:{task}"),format!("schedule:{task}:run:{run}"))}
 fn new(route:Route,work_id:String,execution_id:String)->Self {
  let request_key=match route {Route::Task=>work_id.clone(),Route::Scheduled=>format!("run:{}",execution_id.rsplit(":run:").next().unwrap_or_default()),Route::Peer=>format!("{execution_id}:reply")};
  Self{route,work_id,request_key,execution_id}
 }
}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum End { Completed, Cancelled, Interrupted, Failed, Unknown }
#[derive(Debug,Clone,Serialize,Deserialize)]
pub struct Outcome {
 pub identity:Identity,pub end:End,pub end_reason:String,pub result_ref:Option<String>,pub report_state:String,
}
impl Outcome {
 pub fn finish(identity:Identity,status:&str,reason:&str,result_ref:Option<String>,completed:bool,report_state:&str)->Self {
  let end=match reason {"cancelled"|"stopped"=>End::Cancelled,"interrupted"=>End::Interrupted,_=>match status {
   "cancelled"|"stopped"=>End::Cancelled,"interrupted"=>End::Interrupted,
   "success"|"succeeded"|"returning"|"delivered" if completed=>End::Completed,
   "failed"=>End::Failed,_=>End::Unknown}};
  let end_reason=if matches!(end,End::Unknown) && matches!(status,"success"|"succeeded"|"returning"|"delivered") {
   "缺少可确认的完成证据，不能报告成功".to_string()
  } else {reason.to_string()};
  Self{identity,end,end_reason,result_ref,report_state:report_state.to_string()}
 }
}
/// All adapters must retain uncertain delivery for inspection rather than replay side effects.
pub fn may_retry_report(state:Option<&str>)->bool {matches!(state,Some("pending"|"retry"|"continuing"|"failed"))}
/// Shared completed-content rule: a tool scaffold, partial, or empty response is not a result.
pub fn is_completed_content(role:&str,body:&str,finished:bool,tool_calls:Option<&str>)->bool {
 finished && matches!(role,"assistant"|"response") && !body.trim().is_empty() && body.trim()!="[[NO_REPLY]]"
 && !body.contains("<!-- MCP_TOOL_CALL") && !body.contains("<mcp_tool_call>")
 && tool_calls.is_none_or(|s| matches!(s.trim(),""|"[]"|"null"))
}
#[cfg(test)] #[path="butler_execution_tests.rs"] mod tests;

/// Reserve before dispatch. An interrupted reservation is never automatically replayed.
pub fn reserve_dispatch(conn:&rusqlite::Connection, owner:i64, key:&str, args:&str)->anyhow::Result<Option<String>> {
 use rusqlite::OptionalExtension;
 conn.execute_batch("CREATE TABLE IF NOT EXISTS butler_dispatch_request(owner INTEGER NOT NULL, request_key TEXT NOT NULL, args TEXT NOT NULL, response TEXT, PRIMARY KEY(owner,request_key));")?;
 let inserted=conn.execute("INSERT OR IGNORE INTO butler_dispatch_request(owner,request_key,args) VALUES(?1,?2,?3)",rusqlite::params![owner,key,args])?;
 if inserted>0 {return Ok(None)}
 let existing:Option<(String,Option<String>)>=conn.query_row("SELECT args,response FROM butler_dispatch_request WHERE owner=?1 AND request_key=?2",rusqlite::params![owner,key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
 let (old,response)=existing.ok_or_else(||anyhow::anyhow!("派发记录不存在"))?;
 anyhow::ensure!(old==args,"同一派发请求的参数已改变，请使用新的请求编号");
 Ok(Some(response.ok_or_else(||anyhow::anyhow!("派发请求已经受理但结果尚未确认，请检查已有任务，不会重复执行"))?))
}
pub fn bind_dispatch(conn:&rusqlite::Connection,owner:i64,key:&str,response:&str)->anyhow::Result<()> {
 anyhow::ensure!(conn.execute("UPDATE butler_dispatch_request SET response=?3 WHERE owner=?1 AND request_key=?2 AND response IS NULL",rusqlite::params![owner,key,response])?==1,"派发记录已绑定");Ok(())
}

//! Pure approval identity and SQLite validation, shared by application and native tools.
#[derive(Clone)]
pub(crate) struct ApprovalRequest {
 pub action_id: String,
 pub args: serde_json::Value,
}

pub const APPROVAL_TIMEOUT_SECONDS:u64=300;
#[derive(Debug,Clone,PartialEq)]
pub struct ApprovalBinding {
 pub conversation:i64, pub action:String, pub args:serde_json::Value,
 pub goal_revision:Option<i64>, pub latest_user:Option<i64>, pub expires_at:i64,
}
impl ApprovalBinding {
 pub fn validate(&self,other:&Self,now:i64)->Result<(),String>{
  if now>=self.expires_at {return Err("批准已过期，请重新确认".into())}
  if self.conversation!=other.conversation || self.action!=other.action || self.args!=other.args || self.goal_revision!=other.goal_revision || self.latest_user!=other.latest_user {
   return Err("目标、参数、会话或用户要求已变化，原批准已失效".into())
  } Ok(())
 }
}
/// An old or unidentified run cannot borrow the latest goal's authority.
fn bound_goal_revision(current: Option<i64>, source: Option<i64>) -> Result<Option<i64>, String> {
 if current.is_some() && source != current {
  return Err("操作来源目标已失效或无法核实，请按当前要求重新发起操作".into());
 }
 Ok(source)
}
pub(crate) fn binding_from_connection(conn:&rusqlite::Connection,id:i64,request:&ApprovalRequest,expires_at:i64,source_message:Option<i64>)->Result<ApprovalBinding,String>{
 let (kind,status):(String,Option<String>)=conn.query_row("SELECT conversation_kind,butler_task_status FROM conversation WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
 if !matches!(kind.as_str(),"butler_main"|"butler_contact"|"butler_task"|"butler_group_member") {return Err("管理批准仅适用于管家工作".into())}
 if matches!(status.as_deref(),Some("cancelled"|"failed"|"succeeded"|"interrupted")) {return Err("对应工作已结束，批准失效".into())}
 crate::db::butler_control::ensure_turn(conn,id).map_err(|e|e.to_string())?;
 let current=crate::db::butler_goal::current_revision(conn,id).map_err(|e|e.to_string())?;
 let source_revision=match source_message {
  Some(message)=>crate::db::butler_goal::message_revision(conn,id,message).map_err(|e|e.to_string())?,
  None=>None,
 };
 let revision=bound_goal_revision(current,source_revision)?;
 if let Some(rev)=revision {crate::db::butler_goal::assert_current(conn,id,rev).map_err(|e|e.to_string())?;}
 let latest_user=conn.query_row("SELECT MAX(id) FROM message WHERE conversation_id=?1 AND message_type='user'",[id],|r|r.get(0)).map_err(|e|e.to_string())?;
 Ok(ApprovalBinding{conversation:id,action:request.action_id.clone(),args:request.args.clone(),goal_revision:revision,latest_user,expires_at})
}

/// Native waiters retain this exact source and immutable request, not the active run.
#[derive(Clone)]
pub(crate) struct NativeApprovalGuard {
 pub(crate) frozen: ApprovalBinding,
 pub(crate) request: ApprovalRequest,
 pub(crate) source: Option<i64>,
}
impl NativeApprovalGuard {
 pub(crate) fn validate_connection(&self,conn:&rusqlite::Connection,now:i64)->Result<(),String>{
  let current=binding_from_connection(conn,self.frozen.conversation,&self.request,self.frozen.expires_at,self.source)?;
  self.frozen.validate(&current,now)
 }
}

#[cfg(test)]
#[path="butler_approval_tests.rs"]
mod tests;

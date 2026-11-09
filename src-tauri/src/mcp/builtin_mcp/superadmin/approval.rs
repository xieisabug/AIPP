//! One execution, one immutable approval. No model-supplied token can authorize an action.
use tauri::{AppHandle, Manager};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::mcp::builtin_mcp::operation::{state::OperationState, types::{PermissionRequestEvent, PermissionDecision}};
use crate::api::operation_api::{emit_permission_request_event, OPERATION_PERMISSION_REQUEST_EVENT};
use super::types::{ActionMeta, ExecuteRequest};

pub use crate::db::butler_approval::{ApprovalBinding, APPROVAL_TIMEOUT_SECONDS};
pub(crate) use crate::db::butler_approval::NativeApprovalGuard;
use crate::db::butler_approval::{ApprovalRequest, binding_from_connection};
impl From<&ExecuteRequest> for ApprovalRequest {
 fn from(request: &ExecuteRequest) -> Self {
  Self { action_id: request.action_id.clone(), args: request.args.clone() }
 }
}
fn binding(app:&AppHandle,id:i64,request:&ExecuteRequest,expires_at:i64)->Result<ApprovalBinding,String>{
 let conn=ConversationDatabase::new(app).and_then(|db|db.get_connection()).map_err(|e|e.to_string())?;
 let source=crate::api::butler_peer::TOOL_MESSAGE.try_with(|id| *id).ok().flatten();
 binding_from_connection(&conn,id,&ApprovalRequest::from(request),expires_at,source)
}
impl NativeApprovalGuard {
 pub(crate) fn capture(app:&AppHandle,conversation:i64,source:Option<i64>,action:&str,args:serde_json::Value)->Result<Option<Self>,String>{
  let conn=ConversationDatabase::new(app).and_then(|db|db.get_connection()).map_err(|e|e.to_string())?;
  let kind:String=conn.query_row("SELECT conversation_kind FROM conversation WHERE id=?1",[conversation],|r|r.get(0)).map_err(|e|e.to_string())?;
  if !matches!(kind.as_str(),"butler_main"|"butler_contact"|"butler_task"|"butler_group_member") {return Ok(None)}
  let request=ApprovalRequest{action_id:action.into(),args};
  let frozen=binding_from_connection(&conn,conversation,&request,chrono::Utc::now().timestamp()+APPROVAL_TIMEOUT_SECONDS as i64,source)?;
  Ok(Some(Self{frozen,request,source}))
 }
 pub(crate) fn validate(&self,app:&AppHandle)->Result<(),String>{
  let conn=ConversationDatabase::new(app).and_then(|db|db.get_connection()).map_err(|e|e.to_string())?;
  self.validate_connection(&conn,chrono::Utc::now().timestamp())
 }
}
/// Ordinary operation permissions retain their existing user-facing choices,
/// but Butler approvals also expire with the work that requested them.
pub(crate) fn operation_binding(app: &AppHandle, conversation: Option<i64>, operation: &str, path: &str) -> Result<Option<(ApprovalBinding, ExecuteRequest)>, String> {
 let Some(id) = conversation else { return Ok(None); };
 let db = ConversationDatabase::new(app).map_err(|e| e.to_string())?;
 let kind = db.conversation_repo().map_err(|e| e.to_string())?.read(id)
     .map_err(|e| e.to_string())?.ok_or("操作对应的会话不存在")?.conversation_kind;
 if !matches!(kind.as_str(), "butler_main" | "butler_contact" | "butler_task" | "butler_group_member") { return Ok(None); }
 let request = ExecuteRequest { action_id: format!("operation.{operation}"),
     args: serde_json::json!({"path":path}), dry_run:false, reason:None };
 let expires = chrono::Utc::now().timestamp() + APPROVAL_TIMEOUT_SECONDS as i64;
 Ok(Some((binding(app,id,&request,expires)?,request)))
}

pub async fn approve(app:&AppHandle,meta:&ActionMeta,request:&ExecuteRequest,conversation:Option<i64>, resource:Option<&serde_json::Value>)->Result<ApprovalBinding,String>{
 let id=conversation.ok_or("管理操作缺少会话身份，无法请求批准")?;
 let expires_at=chrono::Utc::now().timestamp()+APPROVAL_TIMEOUT_SECONDS as i64;
 let frozen=binding(app,id,request,expires_at)?;
 let state=app.try_state::<OperationState>().ok_or("OperationState not found")?;
 let request_id=uuid::Uuid::new_v4().to_string();
 let (tx,rx)=tokio::sync::oneshot::channel();
 let event=PermissionRequestEvent { request_id:request_id.clone(), operation:"superadmin_execute".into(),
  path:serde_json::json!({"操作":meta.summary,"动作":request.action_id,"参数":request.args,"操作对象":resource,"工作会话":id,"目标版本":frozen.goal_revision,"有效期":"5分钟，仅本次"}).to_string(),conversation_id:Some(id)};
 state.store_permission_request(event.clone(),tx).await;
 if let Err(e)=emit_permission_request_event(app,OPERATION_PERMISSION_REQUEST_EVENT,Some(id),&event){
  state.remove_permission_request(&request_id).await;return Err(format!("无法显示批准请求：{e}"));
 }
 let received=tokio::time::timeout(std::time::Duration::from_secs(APPROVAL_TIMEOUT_SECONDS),rx).await;
 state.remove_permission_request(&request_id).await;
 let _=crate::api::operation_api::emit_permission_resolved_event(app,
  crate::api::operation_api::OPERATION_PERMISSION_RESOLVED_EVENT,
  &crate::api::operation_api::PermissionResolvedEvent {request_id,conversation_id:Some(id)});
 match received {
  Ok(Ok(PermissionDecision::Allow))=>{},
  Ok(Ok(_))=>return Err("用户没有批准本次具体操作".into()),
  Ok(Err(_))=>return Err("批准等待已取消，未执行".into()),
  Err(_)=>return Err("批准已过期，未执行".into()),
 }
 validate(app,&frozen,request)?;
 Ok(frozen)
}
pub fn validate(app:&AppHandle,frozen:&ApprovalBinding,request:&ExecuteRequest)->Result<(),String>{
 let current=binding(app,frozen.conversation,request,frozen.expires_at)?;
 frozen.validate(&current,chrono::Utc::now().timestamp())
}

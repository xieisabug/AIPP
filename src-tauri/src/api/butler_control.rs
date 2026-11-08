use tauri::{AppHandle, Manager, Emitter};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::butler_control::{self, Control};
fn validate(app:&AppHandle,id:i64)->Result<ConversationDatabase,String>{
 let db=ConversationDatabase::new(app).map_err(|e|e.to_string())?;
 let c=db.conversation_repo().map_err(|e|e.to_string())?.read(id).map_err(|e|e.to_string())?.ok_or("工作不存在")?;
 if c.conversation_kind!="butler_task" {return Err("只允许控制独立管家工作".into())} Ok(db)
}
#[tauri::command]
pub async fn control_butler_task(app_handle:AppHandle,task_conversation_id:i64,action:String)->Result<Control,String>{
 let db=validate(&app_handle,task_conversation_id)?;
 let state={
  let _gate=super::butler_peer::admit_turn(task_conversation_id).await;
  butler_control::set(&db.get_connection().map_err(|e|e.to_string())?,task_conversation_id,&action).map_err(|e|e.to_string())?
 };
 if action=="cancel" {
  let window=super::butler_api::resolve_or_create_butler_execution_window(&app_handle)?;
  super::ai_api::cancel_ai(app_handle.clone(),app_handle.state(),app_handle.state(),app_handle.state(),window,task_conversation_id).await?;
 }
 let _=app_handle.emit("butler_task_control_changed",serde_json::json!({"task_conversation_id":task_conversation_id,"control":state}));
 Ok(state)
}

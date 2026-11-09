//! Resource scope is independent of action risk/approval. A confirmation cannot
//! turn another member's private context into group-readable material.
use anyhow::{ensure, Result};
use rusqlite::Connection;
use serde_json::Value;
use crate::db::{butler_group as group,butler_group_delivery as delivery};

pub fn check(conn:&Connection,current:i64,action:&str,args:&Value,source:Option<i64>) -> Result<()> {
    if !delivery::is_member_conversation(conn,current)? { return Ok(()); }
    delivery::ensure_execution(conn,current,source)?;
    let me = group::scope(conn,current)?;
    match action {
        "conversation.get" => ensure!(args["conversation_id"].as_i64()==Some(current),
            "本群成员仅能读取自己的执行会话；公共发言请使用 group_read，其他会话需用户明确分享"),
        "conversation.list" => anyhow::bail!("群内会话目录请使用 group_members；全局会话列表包含未分享的私聊"),
        "conversation.inject_system_message" => anyhow::bail!("群上下文只追加，不能改写系统前缀；请使用 group_post 追加说明"),
        "conversation.archive"|"conversation.create" => anyhow::bail!("群成员会话由群生命周期管理，不能通过全局会话操作脱离归属"),
        action if action.starts_with("task.") => anyhow::bail!("群内独立任务请使用 group_tasks、group_task_start、group_task_stop；全局 task 管理不带本群执行归属"),
        action if action.starts_with("schedule.") =>
            anyhow::bail!("该管理动作尚未提供群归属参数，不能访问群外任务或日程：{action}；group={}",me.group_id),
        // Assistant/provider/tool configuration does not contain conversation
        // history. Existing action-specific approval policy still applies.
        _ => (),
    }
    Ok(())
}

pub fn current(app:&tauri::AppHandle,id:Option<i64>) -> Result<Option<i64>> {
    let Some(id) = id else { return Ok(None) };
    let db = crate::db::conversation_db::ConversationDatabase::new(app)?;
    let conn = db.get_connection()?;
    if !delivery::is_member_conversation(&conn,id)? { return Ok(None); }
    let source = crate::api::butler_peer::TOOL_MESSAGE.try_with(|v|*v).ok().flatten();
    delivery::ensure_execution(&conn,id,source)?;
    Ok(Some(id))
}

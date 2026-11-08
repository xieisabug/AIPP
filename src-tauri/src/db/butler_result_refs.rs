//! References retained with task results for handoff and later retrieval.
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub fn result_references(conn: &Connection, task: i64, final_message: Option<i64>) -> rusqlite::Result<(Vec<Value>, Vec<Value>)> {
    let mut evidence = Vec::new();
    let mut artifacts = Vec::new();
    if let Some(id) = final_message {
        let owned: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM message WHERE id=?1 AND conversation_id=?2)", params![id,task], |r| r.get(0))?;
        if owned { evidence.push(json!({"kind":"message","conversation_id":task,"message_id":id,"label":"最终答复"})); }
    }
    let mut stmt = conn.prepare("SELECT a.id,a.message_id,a.attachment_url FROM message_attachment a JOIN message m ON m.id=a.message_id WHERE m.conversation_id=?1 ORDER BY a.id")?;
    for row in stmt.query_map([task], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?)))? {
        let (id,message_id,url)=row?;
        artifacts.push(json!({"kind":"attachment","attachment_id":id,"message_id":message_id,"conversation_id":task,"location":url,"label":"会话附件（不代表已核验成果）"}));
    }
    Ok((evidence, artifacts))
}

#[cfg(test)]
#[path = "butler_result_refs_tests.rs"]
mod tests;

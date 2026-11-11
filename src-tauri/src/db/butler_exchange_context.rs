use super::butler_timeline::{list, task_posts};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

#[derive(Serialize)]
pub struct ContextMessage {
    sender: String,
    recipient: String,
    text: String,
}

#[derive(Serialize)]
pub struct Episode {
    prompt: Option<String>,
    messages: Vec<ContextMessage>,
    status: String,
}

// Use persisted exchange identities, never a time window or the currently selected contact.
// Technical errors remain in the execution records; this read model contains dialogue only.
pub fn context(conn: &Connection, names: &HashMap<i64, String>, conversation: i64, ids: &[i64]) -> anyhow::Result<Vec<Episode>> {
    anyhow::ensure!(!ids.is_empty() && ids.len() <= 100, "需要 1–100 个通信提示 ID");
    let mut posts = list(conn, conversation)?;
    posts.extend(task_posts(conn, conversation, names)?);
    let mut groups = BTreeSet::new();
    for id in ids {
        let post = posts.iter().find(|p| p.id == *id)
            .ok_or_else(|| anyhow::anyhow!("通信提示 #{id} 不属于会话 #{conversation}"))?;
        if let Some(root) = post.root { groups.insert((false, root)); }
        if let Some(task) = post.task_id { groups.insert((true, task)); }
    }
    let mut episodes = Vec::new();
    for (task, id) in groups {
        if task {
            let (owner, executor, goal, created): (i64, i64, String, String) = conn.query_row(
                "SELECT butler_conversation_id,executor_assistant_id,goal,created_time FROM butler_task_definition WHERE task_conversation_id=?1",
                [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            let sender_id: Option<i64> = conn.query_row("SELECT assistant_id FROM conversation WHERE id=?1", [owner], |r| r.get(0))?;
            let sender = sender_id.and_then(|id| names.get(&id).cloned()).unwrap_or_else(|| "总管家".into());
            let recipient = names.get(&executor).cloned().unwrap_or_else(|| format!("助手 #{executor}"));
            let prompt = conn.query_row(
                "SELECT content FROM message WHERE conversation_id=?1 AND message_type='user'
                 AND julianday(created_time)<=julianday(?2) AND CASE WHEN json_valid(metadata_json)
                    THEN coalesce(json_extract(metadata_json,'$.origin'),'') NOT IN ('internal','butler_peer') ELSE 1 END
                 ORDER BY id DESC LIMIT 1", params![owner,created], |r| r.get(0)).optional()?
                .map(|text: String| crate::utils::turn_context::visible(&text).to_owned());
            let task_posts: Vec<_> = posts.iter().filter(|p| p.task_id == Some(id)).collect();
            let mut messages = vec![ContextMessage {sender:sender.clone(),recipient:recipient.clone(),text:goal}];
            if let Some(reply) = task_posts.iter().find(|p| p.task_reply && !p.text.is_empty()) {
                messages.push(ContextMessage {sender:recipient,recipient:sender,text:reply.text.clone()});
            }
            let status = task_posts.last().map(|p| p.status.clone()).unwrap_or_else(|| "waiting".into());
            episodes.push(Episode {prompt,messages,status});
        } else {
            let prompt = conn.query_row("SELECT content FROM message WHERE id=?1 AND message_type='user'", [id], |r| r.get(0)).optional()?
                .map(|text: String| crate::utils::turn_context::visible(&text).to_owned());
            let mut stmt = conn.prepare("SELECT sender,recipient,body,reply,state FROM butler_peer_exchange WHERE root=?1 ORDER BY round_no,id")?;
            let rows = stmt.query_map([id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,String>(4)?)))?;
            let mut messages = Vec::new();
            let mut status = "waiting".to_string();
            for row in rows {
                let (sender,recipient,body,reply,state) = row?;
                messages.push(ContextMessage {sender:sender.clone(),recipient:recipient.clone(),text:body});
                if let Some(text) = reply { messages.push(ContextMessage {sender:recipient,recipient:sender,text}); }
                status = state;
            }
            episodes.push(Episode {prompt,messages,status});
        }
    }
    Ok(episodes)
}

#[cfg(test)]
#[path = "butler_exchange_context_tests.rs"]
mod tests;

use super::types::Access;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::AppHandle;
use crate::db::conversation_db::{ConversationDatabase, ConversationRepository};

/// Every directory and history read uses this predicate. Local numeric IDs never confer access.
const VISIBLE: &str = "(
    (c.conversation_kind='normal' AND c.is_hidden_from_normal_chat_list=0
        AND c.assistant_id IN (SELECT value FROM json_each(?1)))
    OR (?2 AND c.conversation_kind='butler_contact'
        AND c.assistant_id IN (SELECT value FROM json_each(?1)))
    OR (?2 AND c.conversation_kind='butler_main' AND EXISTS (
        SELECT 1 FROM butler_main_state s WHERE s.butler_conversation_id=c.id AND s.slot=?3))
)";

pub fn catalog(app: &AppHandle, access: &Access) -> Result<Value> {
    let available = crate::api::assistant_api::get_assistants(app.clone()).map_err(anyhow::Error::msg)?;
    let assistants: Vec<_> = available.iter().filter(|a| access.assistants.contains(&a.id))
        .map(|a| json!({"id":a.id,"name":a.name})).collect();
    let db = ConversationDatabase::new(app)?;
    let conn = db.get_connection()?;
    let ids = serde_json::to_string(&access.assistants)?;
    let mut stmt = conn.prepare(&format!("WITH visible AS (
        SELECT c.id,c.name,c.conversation_kind,c.assistant_id,c.updated_time FROM conversation c WHERE {VISIBLE}
    ), recent_chats AS (
        SELECT * FROM visible WHERE conversation_kind='normal' ORDER BY updated_time DESC,id DESC LIMIT 200
    ) SELECT * FROM recent_chats UNION ALL SELECT * FROM visible WHERE conversation_kind!='normal'
      ORDER BY updated_time DESC,id DESC"))?;
    let rows = stmt.query_map(params![ids,access.butler,crate::api::butler_api::BUTLER_MAIN_SLOT], |r|
        Ok(json!({"id":r.get::<_,i64>(0)?, "name":r.get::<_,String>(1)?, "kind":r.get::<_,String>(2)?,
            "assistantId":r.get::<_,Option<i64>>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut capabilities = vec!["conversation_history"];
    if access.chat { capabilities.push("chat"); }
    Ok(json!({"protocolVersion":super::types::VERSION,"capabilities":capabilities,"assistants":assistants,"butler":access.butler,"conversations":rows}))
}

pub fn target(conn: &Connection, access: &Access, id: i64) -> Result<(String,i64)> {
    let sql = format!("SELECT c.conversation_kind,c.assistant_id FROM conversation c WHERE {VISIBLE} AND c.id=?4");
    conn.query_row(&sql,params![serde_json::to_string(&access.assistants)?,access.butler,
        crate::api::butler_api::BUTLER_MAIN_SLOT,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.context("该会话未授权、已归档或已删除")
}

pub fn history(conn: &Connection, access: &Access, id: i64, before: i64) -> Result<Value> {
    let (kind,_) = target(conn, access, id)?;
    // Only public, finished text. Tool arguments, raw metadata, assistant reasoning and
    // per-turn model context never leave the desktop through this projection.
    let sql = format!("SELECT r.id,r.message_type,r.content,r.metadata_json FROM message r
        WHERE r.conversation_id=?1 AND r.id<?2 AND (
            (r.message_type='user' AND CASE WHEN json_valid(r.metadata_json)
                THEN coalesce(json_extract(r.metadata_json,'$.origin'),'') IN ('','user') ELSE 1 END)
            OR ({}) OR (r.message_type='error' AND r.finish_time IS NOT NULL AND
                (SELECT CASE WHEN json_valid(u.metadata_json) THEN coalesce(json_extract(u.metadata_json,'$.origin'),'') IN ('','user')
                             ELSE 1 END FROM message u WHERE u.conversation_id=r.conversation_id
                      AND u.message_type='user' AND u.id<r.id ORDER BY u.id DESC LIMIT 1)))
        ORDER BY r.id DESC LIMIT 51", if kind == "normal" {
            // Normal chat persists streaming text as it arrives. Keep the same tool,
            // reasoning and audience filters for incomplete and completed messages.
            ConversationRepository::PUBLIC_BUTLER_REPLY.replacen("r.finish_time IS NOT NULL","1",1)
        } else { ConversationRepository::PUBLIC_BUTLER_REPLY.to_owned() });
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![id,before])?;
    let mut messages = Vec::new();
    let mut bytes = 0;
    let mut has_more = false;
    while let Some(row) = rows.next()? {
        if messages.len() == 50 {
            has_more = true;
            break;
        }
        let role: String = row.get(1)?;
        let body: String = row.get(2)?;
        let content = if role == "error" {
            // Provider bodies and native stderr can contain request context.
            format!("电脑执行出错，请在电脑会话 #{id} 查看错误消息 #{}",row.get::<_,i64>(0)?)
        } else if role == "user" {
            let visible = crate::utils::turn_context::visible(&body);
            if visible.contains('<') { crate::external_channels::presentation::sanitize_plain_text(visible).unwrap_or_default() }
            else { visible.to_owned() }
        } else {
            let meta = row.get::<_,Option<String>>(3)?;
            crate::db::butler_execution::agent_final_text(&body,meta.as_deref()).unwrap_or(body)
        };
        // Keep even unusually long messages intact; report the limit explicitly.
        anyhow::ensure!(content.len() <= 192 * 1024, "单条消息过长，请在电脑上查看");
        if bytes + content.len() > 192 * 1024 {
            has_more = true;
            break;
        }
        bytes += content.len();
        messages.push(json!({"id":row.get::<_,i64>(0)?,"role":role,"content":content}));
    }
    messages.reverse();
    Ok(json!({"messages":messages,"hasMore":has_more}))
}

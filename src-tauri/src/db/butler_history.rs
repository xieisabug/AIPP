use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use std::collections::HashMap;

// Only conversation kinds owned by the Butler workspace may be searched, even by numeric ID.
// 归档的联系人私聊与不再是当前主会话的总管家主会话标为 archived，让模型与界面区分新旧会话。
const ROWS: &str = "SELECT m.id, c.id, c.conversation_kind, c.assistant_id, c.name, m.message_type,
    coalesce(CASE WHEN json_valid(m.metadata_json) THEN json_extract(m.metadata_json,'$.origin') END,
        CASE WHEN c.conversation_kind='butler_task' AND m.message_type='user' AND m.id=(
            SELECT min(t.id) FROM message t WHERE t.conversation_id=c.id AND t.message_type='user') THEN 'butler_task' END),
    CASE WHEN json_valid(m.metadata_json) THEN json_extract(m.metadata_json,'$.sender_bot') END,
    m.created_time,
    c.conversation_kind='butler_contact_archive' OR (c.conversation_kind='butler_main'
        AND NOT EXISTS(SELECT 1 FROM butler_main_state s WHERE s.butler_conversation_id=c.id)), ";
const FROM: &str = " FROM message m JOIN conversation c ON c.id=m.conversation_id
    WHERE c.conversation_kind IN ('butler_main','butler_contact','butler_contact_archive','butler_task')
      AND m.message_type IN ('user','response','assistant','error') AND trim(m.content)!=''
      AND CASE WHEN json_valid(m.metadata_json) THEN coalesce(json_extract(m.metadata_json,'$.origin'),'')!='internal' ELSE 1 END";
const HUMAN: &str = " AND m.message_type='user' AND CASE WHEN json_valid(m.metadata_json)
    THEN coalesce(json_extract(m.metadata_json,'$.origin'),'') NOT IN ('internal','butler_peer','butler_task') ELSE 1 END
    AND NOT (c.conversation_kind='butler_task' AND m.id=(SELECT min(t.id) FROM message t WHERE t.conversation_id=c.id AND t.message_type='user'))";

fn entry(row: &Row<'_>, names: &HashMap<i64, String>) -> rusqlite::Result<Value> {
    let bot_id: Option<i64> = row.get(3)?;
    let title: String = row.get(4)?;
    let kind: String = row.get(2)?;
    let bot = bot_id.and_then(|id| names.get(&id)).cloned().unwrap_or_else(|| {
        if kind == "butler_contact" {
            title.clone()
        } else {
            format!("历史助手 #{}", bot_id.unwrap_or(0))
        }
    });
    let origin: Option<String> = row.get(6)?;
    let role: String = row.get(5)?;
    let speaker = match (origin.as_deref(), role.as_str()) {
        (Some("butler_peer"), _) => {
            row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "协作助手".into())
        }
        (Some("butler_task"), _) => "管家派工".into(),
        (_, "user") => "用户".into(),
        (_, "error") => "运行错误".into(),
        _ => bot.clone(),
    };
    Ok(json!({"message_id":row.get::<_,i64>(0)?, "conversation_id":row.get::<_,i64>(1)?,
        "conversation_kind":kind,"conversation_name":title,"archived":row.get::<_,bool>(9)?,"bot_id":bot_id,"bot_name":bot,
        "role":role,"speaker":speaker,"created_time":row.get::<_,String>(8)?,
        "text":row.get::<_,String>(10)?,"truncated":row.get::<_,bool>(11)?}))
}

pub fn search(
    conn: &Connection,
    names: &HashMap<i64, String>,
    query: &str,
    bot_id: Option<i64>,
    before: Option<i64>,
) -> rusqlite::Result<Value> {
    let sql = format!(
        "{ROWS}substr(m.content,max(1,instr(lower(m.content),lower(?1))-180),900),
        length(m.content)>900 {FROM} AND instr(lower(m.content),lower(?1))>0
        AND (?2 IS NULL OR c.assistant_id=?2) AND (?3 IS NULL OR m.id<?3)
        ORDER BY m.id DESC LIMIT 13"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut hits = stmt
        .query_map(params![query, bot_id, before], |row| entry(row, names))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more = hits.len() > 12;
    hits.truncate(12);
    let next = if more { hits.last().and_then(|h| h["message_id"].as_i64()) } else { None };
    Ok(json!({"matches":hits,"next_before_message_id":next}))
}

pub fn context(
    conn: &Connection,
    names: &HashMap<i64, String>,
    id: i64,
    focus: Option<&str>,
) -> Result<Value, String> {
    let read = || -> rusqlite::Result<Option<Value>> {
        let base = format!("{ROWS}substr(m.content,1,1600),length(m.content)>1600 {FROM}");
        let Some(hit) =
            conn.query_row(&format!("{ROWS}substr(m.content,max(1,instr(lower(m.content),lower(?2))-300),1600),length(m.content)>1600 {FROM} AND m.id=?1"), params![id, focus.unwrap_or("")], |r| entry(r, names)).optional()?
        else {
            return Ok(None);
        };
        let conversation = hit["conversation_id"].as_i64().unwrap();
        let hit_time = hit["created_time"].as_str().unwrap_or_default().to_owned();
        let neighbors = |direction: &str, human: bool| -> rusqlite::Result<Vec<Value>> {
            let order = if direction == "<" { "DESC" } else { "ASC" };
            let filter = if human { HUMAN } else { "" };
            let limit = if human { 1 } else { 7 };
            let mut stmt = conn.prepare(&format!(
                "{base} AND m.conversation_id=?1
                AND (coalesce(julianday(m.created_time),0), m.id) {direction} (coalesce(julianday(?3),0), ?2)
                {filter} ORDER BY coalesce(julianday(m.created_time),0) {order}, m.id {order} LIMIT {limit}"
            ))?;
            let rows = stmt.query_map(params![conversation, id, hit_time], |r| entry(r, names))?;
            rows.collect()
        };
        let mut messages = neighbors("<", false)?;
        messages.reverse();
        messages.push(hit);
        messages.extend(neighbors(">", false)?);
        Ok(Some(
            json!({"hit_message_id":id,"messages":messages,"user_before":neighbors("<",true)?.pop(),
            "user_after":neighbors(">",true)?.pop()}),
        ))
    };
    read().map_err(|e| e.to_string())?.ok_or_else(|| format!("管家消息 {id} 不存在或不在可读范围"))
}

/// 管家窗口左上角搜索：在管家主会话、联系人私聊（均含已归档）与派工任务中检索公开消息。
/// 返回完整正文，由调用方按命中位置截取摘要；按消息 id 倒序分页。
pub fn ui_search(
    conn: &Connection,
    names: &HashMap<i64, String>,
    query: &str,
    limit: u32,
    offset: u32,
) -> rusqlite::Result<Vec<Value>> {
    let sql = format!(
        "{ROWS}m.content, 0 {FROM} AND instr(lower(m.content),lower(?1))>0
        ORDER BY m.id DESC LIMIT ?2 OFFSET ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![query, limit, offset], |row| entry(row, names))?;
    rows.collect()
}

#[cfg(test)]
#[path = "butler_history_tests.rs"]
mod tests;

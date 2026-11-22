use super::Config;
use anyhow::{ensure, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Serialize, Clone)]
pub struct Source {
    pub conversation: i64,
    pub position: i64,
    pub kind: String,
    pub assistant: Option<i64>,
    pub time: i64,
    pub text: String,
    #[serde(skip)]
    pub(super) offset: usize,
    #[serde(skip)]
    pub(super) next_offset: i64,
}

// Filter before sending excerpts to a model, and again before persisting its output.
pub fn sensitive(text: &str) -> bool {
    static CREDENTIAL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r"(?i)(<vault:|vault://|bearer\s+\S+|-----begin [^-]*private key|\b(?:sk-|gh[pousr]_|github_pat_|AKIA|AIza|xox[baprs]-)[A-Za-z0-9_-]{6,}|(?:api[ _-]?key|access[ _-]?token|password|passwd|密钥|密码|令牌)\s*[:=：是为]\s*\S+|\b(?:\d[ -]?){13,19}\b)"
    ).expect("credential patterns")
    });
    CREDENTIAL.is_match(text)
}

enum Selection {
    Increment,
    Exact,
    Search,
}

fn source_query(deep: bool, selection: Selection) -> String {
    let position = if deep { "digest_position" } else { "pulse_position" };
    let offset = if deep { "digest_offset" } else { "pulse_offset" };
    let (messages, posts) = match selection {
        Selection::Increment => (
            format!("(m.id>COALESCE(p.{position},0) OR (m.id=p.{position} AND p.{offset}>=0))"),
            format!("(g.seq>COALESCE(p.{position},0) OR (g.seq=p.{position} AND p.{offset}>=0))"),
        ),
        Selection::Exact => ("c.id=?4 AND m.id=?5".into(), "g.group_id=?4 AND g.seq=?5".into()),
        Selection::Search => (
            format!("instr({},?4)>0",crate::utils::turn_context::visible_sql("m.content")),
            format!("instr({},?4)>0",crate::utils::turn_context::visible_sql("g.body")),
        ),
    };
    format!("SELECT c.id, m.id, c.conversation_kind, c.assistant_id, unixepoch(m.created_time) AS timestamp, m.content,
         CASE WHEN m.id=p.{position} THEN p.{offset} ELSE 0 END
         FROM conversation c JOIN message m ON c.id=m.conversation_id
         LEFT JOIN butler_proactive_cursor p ON p.conversation_id=c.id
         WHERE m.message_type='user' AND {messages}
         AND ((?1 AND (c.conversation_kind='butler_contact' OR (c.conversation_kind='butler_main'
              AND c.id IN (SELECT butler_conversation_id FROM butler_main_state))))
              OR (?2 AND c.conversation_kind='normal' AND c.is_hidden_from_normal_chat_list=0))
         AND (m.metadata_json IS NULL OR (json_valid(m.metadata_json) AND json_extract(m.metadata_json,'$.origin') IS NULL
              AND json_extract(m.metadata_json,'$.proactive_acceptance') IS NULL
              AND json_extract(m.metadata_json,'$.relay_origin') IS NULL AND json_extract(m.metadata_json,'$.relayOrigin') IS NULL))
         AND NOT EXISTS(SELECT 1 FROM butler_work_followup w WHERE w.execution_input_id=m.id)
         UNION ALL SELECT g.group_id,g.seq,'butler_group',NULL,unixepoch(g.created_at),g.body,
         CASE WHEN g.seq=p.{position} THEN p.{offset} ELSE 0 END
         FROM butler_chat_group_message g LEFT JOIN butler_proactive_cursor p ON p.conversation_id=g.group_id
         WHERE ?3 AND g.kind='message' AND g.sender_member IS NULL AND {posts}
         ")
}

pub(super) fn quote(
    conn: &Connection,
    cfg: &Config,
    conversation: i64,
    position: i64,
    quote: &str,
) -> Result<Option<Source>> {
    use rusqlite::OptionalExtension;
    let row = conn
        .query_row(
            &source_query(false, Selection::Exact),
            params![cfg.private_chats, cfg.normal_chats, cfg.groups, conversation, position],
            |r| {
                Ok(Source {
                    conversation,
                    position,
                    kind: r.get(2)?,
                    assistant: r.get(3)?,
                    time: r.get::<_, Option<i64>>(4)?.unwrap_or_default(),
                    text: r.get(5)?,
                    offset: 0,
                    next_offset: -1,
                })
            },
        )
        .optional()?;
    Ok(row.and_then(|mut source| {
        let visible = crate::utils::turn_context::visible(&source.text);
        if quote.trim().is_empty() || sensitive(visible) {
            return None;
        }
        let start = visible.find(quote)?;
        source.offset = visible[..start].chars().count();
        source.text = quote.to_owned();
        Some(source)
    }))
}

// Search stays with the coordinator. Only its verified, topic-specific selections
// may be returned to a delegate.
pub(super) fn search(db: &Connection, cfg: &Config, term: &str) -> Result<Vec<Source>> {
    ensure!(
        (2..=80).contains(&term.trim().chars().count()) && !sensitive(term),
        "补找原话需提供具体关键词"
    );
    let mut stmt = db
        .prepare(&format!("{} ORDER BY 5 DESC LIMIT 32", source_query(false, Selection::Search)))?;
    let mut rows = stmt.query(params![cfg.private_chats, cfg.normal_chats, cfg.groups, term])?;
    let mut candidates = Vec::new();
    while let Some(r) = rows.next()? {
        let raw: String = r.get(5)?;
        let visible = crate::utils::turn_context::visible(&raw);
        let Some(at) = visible.find(term) else { continue };
        if sensitive(visible) {
            continue;
        }
        let offset = visible[..at].chars().count().saturating_sub(100);
        candidates.push(Source {
            conversation: r.get(0)?,
            position: r.get(1)?,
            kind: r.get(2)?,
            assistant: r.get(3)?,
            time: r.get::<_, Option<i64>>(4)?.unwrap_or_default(),
            text: visible.chars().skip(offset).take(320).collect(),
            offset,
            next_offset: -1,
        });
        if candidates.len() == 12 {
            break;
        }
    }
    Ok(candidates)
}

pub fn read(conn: &Connection, config: &Config, deep: bool) -> Result<Vec<Source>> {
    let mut stmt = conn
        .prepare(&format!("{} ORDER BY 1,2 LIMIT 256", source_query(deep, Selection::Increment)))?;
    let mut rows = stmt.query(params![config.private_chats, config.normal_chats, config.groups])?;
    let mut result = Vec::new();
    let mut remaining = 16_000;
    while let Some(r) = rows.next()? {
        let body: String = r.get(5)?;
        let visible = crate::utils::turn_context::visible(&body);
        if remaining == 0 {
            break;
        }
        let offset: usize = r.get(6)?;
        let mut excerpt = visible.chars().skip(offset);
        let text: String = if sensitive(visible) || visible.trim() == "继续你的工作" {
            String::new()
        } else {
            excerpt.by_ref().take(remaining).collect()
        };
        let size = text.chars().count();
        let next_offset =
            if size == remaining && excerpt.next().is_some() { (offset + size) as i64 } else { -1 };
        result.push(Source {
            conversation: r.get(0)?,
            position: r.get(1)?,
            kind: r.get(2)?,
            assistant: r.get(3)?,
            time: r.get::<_, Option<i64>>(4)?.unwrap_or_default(),
            text,
            offset,
            next_offset,
        });
        remaining -= size;
    }
    Ok(result)
}

pub fn pending(conn: &Connection, config: &Config, deep: bool) -> Result<(usize, i64)> {
    let sql = source_query(deep, Selection::Increment);
    Ok(conn.query_row(
        &format!("SELECT count(*),coalesce(max(timestamp),0) FROM ({sql})"),
        params![config.private_chats, config.normal_chats, config.groups],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

pub(super) fn unread_digest(db: &Connection, cfg: &Config) -> Result<bool> {
    let sql = format!("SELECT EXISTS({})", source_query(true, Selection::Increment));
    Ok(db.query_row(&sql, params![cfg.private_chats, cfg.normal_chats, cfg.groups], |r| r.get(0))?)
}

pub fn advance(conn: &Connection, rows: &[Source], deep: bool) -> Result<()> {
    for row in rows {
        for prefix in if deep { &["pulse", "digest"][..] } else { &["pulse"][..] } {
            let pos = format!("{prefix}_position");
            let offset = format!("{prefix}_offset");
            conn.execute(&format!("INSERT INTO butler_proactive_cursor(conversation_id,{pos},{offset}) VALUES(?1,?2,?3)
                ON CONFLICT(conversation_id) DO UPDATE SET {pos}=excluded.{pos},{offset}=excluded.{offset}
                WHERE excluded.{pos}>{pos} OR (excluded.{pos}={pos} AND {offset}>=0 AND (excluded.{offset}=-1 OR excluded.{offset}>{offset}))"),
                params![row.conversation,row.position,row.next_offset])?;
        }
    }
    Ok(())
}

pub fn verify(conn: &Connection, source: &Source) -> Result<()> {
    let current: String = if source.kind == "butler_group" {
        conn.query_row("SELECT body FROM butler_chat_group_message WHERE group_id=?1 AND seq=?2 AND kind='message' AND sender_member IS NULL",
            params![source.conversation,source.position], |r| r.get(0))?
    } else {
        conn.query_row("SELECT m.content FROM message m JOIN conversation c ON c.id=m.conversation_id
            WHERE m.conversation_id=?1 AND m.id=?2 AND m.message_type='user' AND c.conversation_kind=?3 AND c.assistant_id IS ?4
            AND (c.conversation_kind!='butler_main' OR c.id IN (SELECT butler_conversation_id FROM butler_main_state))
            AND (m.metadata_json IS NULL OR (json_valid(m.metadata_json) AND json_extract(m.metadata_json,'$.origin') IS NULL
            AND json_extract(m.metadata_json,'$.proactive_acceptance') IS NULL
            AND json_extract(m.metadata_json,'$.relay_origin') IS NULL AND json_extract(m.metadata_json,'$.relayOrigin') IS NULL))
            AND NOT EXISTS(SELECT 1 FROM butler_work_followup WHERE execution_input_id=m.id)",
            params![source.conversation,source.position,source.kind,source.assistant], |r| r.get(0))?
    };
    let visible = crate::utils::turn_context::visible(&current);
    ensure!(
        !source.text.is_empty()
            && !sensitive(visible)
            && visible
                .chars()
                .skip(source.offset)
                .take(source.text.chars().count())
                .eq(source.text.chars()),
        "整理来源已变更"
    );
    Ok(())
}

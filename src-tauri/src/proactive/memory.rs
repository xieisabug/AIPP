use super::delta::{self, Source};
use crate::db::butler_memory::{self as memory, Memory, Scope};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

pub fn save(
    conn: &Connection,
    src: &Source,
    chief: i64,
    content: &str,
    replace: Option<(i64, i64)>,
    global: bool,
) -> Result<()> {
    delta::verify(conn, src)?;
    ensure!(!delta::sensitive(content), "记忆含敏感信息");
    ensure!(!global || src.kind != "butler_group", "群聊发言只能整理成本群记忆");
    let (scope, source) = match src.kind.as_str() {
        _ if global => (Scope::Global, memory::Source::Message(src.position)),
        "butler_group" => (
            Scope::Group(src.conversation),
            memory::Source::GroupMessage { group: src.conversation, seq: src.position },
        ),
        "butler_contact" => (
            Scope::Assistant(src.assistant.context("联系人缺少助手")?),
            memory::Source::Message(src.position),
        ),
        _ => (Scope::Assistant(chief), memory::Source::Message(src.position)),
    };
    let (written, previous) = if let Some((id, revision)) = replace {
        let old = memory::get_any(conn, id)?.context("记忆不存在")?;
        let allowed = match scope {
            Scope::Group(g) => old.scope_type == "group" && old.scope_id == Some(g),
            Scope::Assistant(a) => old.scope_type == "assistant" && old.scope_id == Some(a),
            Scope::Global => old.scope_type == "global" && old.scope_id.is_none(),
        };
        ensure!(allowed, "记忆来源与更正范围不符");
        let updated = memory::update(conn, None, id, Some(revision), content, Some(source))?;
        (updated, Some(serde_json::to_string(&old)?))
    } else {
        let (entry, created) = memory::remember(conn, scope, content, Some(source), Some(chief))?;
        if !created {
            return Ok(());
        }
        (entry, None)
    };
    conn.execute("UPDATE butler_memory SET origin='proactive' WHERE id=?1", [written.id])?;
    conn.execute("INSERT INTO butler_proactive_memory_change(memory_id,written_revision,previous) VALUES(?1,?2,?3)
        ON CONFLICT(memory_id) DO UPDATE SET written_revision=excluded.written_revision,previous=excluded.previous",
        params![written.id,written.revision,previous])?;
    Ok(())
}

pub fn undo(conn: &Connection, id: i64, revision: i64) -> Result<()> {
    let (written, previous): (i64, Option<String>) = conn
        .query_row(
            "SELECT written_revision, previous FROM butler_proactive_memory_change
        WHERE memory_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .context("本机没有这次整理的撤销记录")?;
    let current = memory::get_any(conn, id)?.context("记忆已删除")?;
    ensure!(
        current.origin == "proactive" && current.revision == written && written == revision,
        "记忆已更改，已保留最新版本"
    );
    if let Some(previous) = previous {
        let old: Memory = serde_json::from_str(&previous)?;
        conn.execute("UPDATE butler_memory SET content=?2,source_message_id=?3,source_group_id=?4,source_group_seq=?5,
            origin=?6,revision=revision+1,updated_at=?7 WHERE id=?1",
            params![id,old.content,old.source_message_id,old.source_group_id,old.source_group_seq,old.origin,chrono::Utc::now().timestamp_millis()])?;
    } else {
        memory::forget(conn, None, id, Some(revision))?;
    }
    conn.execute("DELETE FROM butler_proactive_memory_change WHERE memory_id=?1", [id])?;
    Ok(())
}
